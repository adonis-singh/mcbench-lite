// The worlds a helper keeps and the scenes made of them (PROTOCOL.md, What
// is kept), and drawing shots of them.

use crate::assets::Assets;
use crate::blueprint;
use crate::camera::Camera;
use crate::entities::{self, BlockQuad, View};
use crate::gpu::{Batches, DeviceError, Draws, Failure, Gpu, Ordered, Renderer};
use crate::picture::{self, Font, Grid};
use crate::scene::{Draw, Instance};
use mcbench_renderer::world::{Mask, World};
use serde::Deserialize;
use serde_json::json;
use serde_json::Value;
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::{mpsc, Arc};

const EVERYWHERE: Mask = Mask {
    min_x: -30_000_000,
    max_x: 30_000_000,
    min_z: -30_000_000,
    max_z: 30_000_000,
};

// A clip box at the page's heights, inclusive (resolveShot's `clip`).
#[derive(Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(deny_unknown_fields)]
pub struct Clip {
    pub min: [i32; 3],
    pub max: [i32; 3],
}

// A shot as resolveShot returns it.
#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct Shot {
    pub width: u32,
    pub height: u32,
    pub camera: Camera,
    pub clip: Option<Clip>,
    pub grid: Option<Grid>,
    #[serde(default, rename = "described")]
    pub _described: Option<Value>,
    #[serde(default, rename = "tick")]
    pub _tick: Option<Value>,
}

pub struct Kept {
    pub blueprint: Vec<u8>,
    pub y_offset: i32,
    pub entities: Vec<Value>,
    pub used: u64,
}

impl Kept {
    // The bytes it holds (Store::counted).
    pub fn bytes(&self) -> u64 {
        self.blueprint.capacity() as u64 + values_bytes(&self.entities)
    }
}

// The bytes a list of JSON values holds in memory: each value, its text and
// its lists' and objects' entries by their sizes.
fn values_bytes(values: &[Value]) -> u64 {
    fn value(v: &Value) -> usize {
        std::mem::size_of::<Value>()
            + match v {
                Value::String(s) => s.capacity(),
                Value::Array(a) => a.iter().map(value).sum::<usize>(),
                Value::Object(o) => o
                    .iter()
                    .map(|(k, v)| std::mem::size_of::<String>() + k.capacity() + value(v))
                    .sum::<usize>(),
                _ => 0,
            }
    }
    values.iter().map(|v| value(v) as u64).sum()
}

// A tile's geometry: its opaque instances' place in the scene's arena (in
// tile order, so the opaque quads draw in few calls: software rasterisers
// pay for every draw call over the whole target), its blended ones kept to
// be ordered with every tile's, and a digest of both (what the tests
// compare against a fresh load).
pub struct TileGeom {
    pub opaque_count: usize,
    // Each opaque instance's block (scene.rs pack_block), in the arena's order.
    pub blocks: Vec<u32>,
    pub blended: Vec<(crate::scene::BlendedKey, Instance)>,
    pub missing: Vec<String>,
    pub digest: u64,
}

// At most the bytes a lit world holds (Store::counted): the mesher keeps a
// section of 4,096 cells for each 16-cube over its lit volume, each cell at
// most its palette index (2 bytes) and its sky and block light (a byte
// each; renderer/src/world.rs Section), with small tables beside them.
pub fn lit_bytes(world: &World) -> u64 {
    let (min, size) = world.volume();
    let sections: u64 = (0..3)
        .map(|k| {
            if size[k] <= 0 {
                0
            } else {
                ((min[k] + size[k] - 1).div_euclid(16) - min[k].div_euclid(16) + 1) as u64
            }
        })
        .product();
    sections * 4096 * 4
}

// What an edit of the world needs: the lit world itself, and by the kept
// blueprint's palette index, the world's palette index and the state.
pub struct Lit {
    pub world: World,
    pub states: Vec<u32>,
    pub map: Vec<u16>,
}

pub struct Scene {
    // In mesh::tiles' order.
    pub tiles: Vec<(i32, i32)>,
    pub geometry: HashMap<(i32, i32), TileGeom>,
    pub arena: Arena,
    pub translucent: Option<Translucent>,
    pub solid: Option<Batches>,
    pub sorted: entities::Sorted,
    pub entities: Vec<Value>,
    // The blocks' bounds, at the page's heights (the fog's range).
    pub bounds: ([i32; 3], [i32; 3]),
    pub entity_quads: usize,
    pub blocks_count: u64,
    pub used: u64,
    // The whole world's scene keeps its lit world for edits; a clip's not.
    pub lit: Option<Lit>,
}

impl Scene {
    pub fn bytes(&self) -> u64 {
        self.arena.bytes()
            + self.translucent.as_ref().map_or(0, Translucent::bytes)
            + self.solid.as_ref().map_or(0, |s| s.bytes)
    }

    // Its quads and their order on the GPU.
    pub fn gpu_bytes(&self) -> u64 {
        self.arena.bytes()
            + self.translucent.as_ref().map_or(0, Translucent::gpu_bytes)
            + self.solid.as_ref().map_or(0, |s| s.bytes)
    }

    // What it keeps on the CPU (Store::counted): its tiles' blocks and
    // blended quads, its translucent quads' order, its entities, its lit
    // world.
    pub fn cpu_bytes(&self) -> u64 {
        use std::mem::size_of;
        let tiles: usize = self
            .geometry
            .values()
            .map(|g| {
                g.blocks.capacity() * size_of::<u32>()
                    + g.blended.capacity() * size_of::<(crate::scene::BlendedKey, Instance)>()
                    + g.missing.capacity() * size_of::<String>()
                    + g.missing.iter().map(String::capacity).sum::<usize>()
            })
            .sum();
        let placed = |list: &Vec<entities::Placed>| {
            list.capacity() * size_of::<entities::Placed>()
                + list
                    .iter()
                    .map(|p| p.quad.texture.capacity() + p.quad.layer.capacity())
                    .sum::<usize>()
        };
        (self.tiles.capacity() * size_of::<(i32, i32)>()
            + self.geometry.capacity() * size_of::<((i32, i32), TileGeom)>()
            + tiles
            + (self.arena.ranges.capacity() + self.arena.rooms.capacity())
                * size_of::<((i32, i32), (usize, u32, u32))>()
            + placed(&self.sorted.solid)
            + placed(&self.sorted.blended)
            + placed(&self.sorted.facing)) as u64
            + self.translucent.as_ref().map_or(0, Translucent::cpu_bytes)
            + values_bytes(&self.entities)
            + self.lit.as_ref().map_or(0, |l| {
                lit_bytes(&l.world)
                    + (l.states.capacity() * size_of::<u32>() + l.map.capacity() * size_of::<u16>())
                        as u64
            })
    }

    pub fn quads(&self) -> usize {
        self.geometry
            .values()
            .map(|g| g.opaque_count + g.blended.len())
            .sum()
    }

    pub fn missing(&self) -> Vec<String> {
        let mut m: Vec<String> = self
            .geometry
            .values()
            .flat_map(|g| g.missing.iter().cloned())
            .collect();
        m.sort();
        m.dedup();
        m
    }

    // The opaque block buffers: every tile's opaque quads in tile order.
    pub fn block_batches(&self) -> Vec<&Batches> {
        vec![&self.arena.batches]
    }

    // Each tile's digest, in tile order.
    pub fn digests(&self) -> Vec<((i32, i32), u64)> {
        self.tiles
            .iter()
            .map(|t| (*t, self.geometry[t].digest))
            .collect()
    }
}

// A scene's translucent block quads on the GPU, section after section (in
// tile order, a tile's sections in their key's order), and what ordering
// them for a camera needs: each Minecraft section's place, each quad's
// centroid within its section as Minecraft takes it. A section's quads are
// in one buffer.
pub struct Translucent {
    pub batches: Batches,
    order: wgpu::Buffer,
    sections: Vec<Section>,
    // By quad, in section order: (v0 + v2) / 2 relative to its section's
    // origin, in f32 (MeshData.unpackQuadCentroids).
    centroids: Vec<[f32; 3]>,
    // The page's height offset (its heights are Minecraft's less it), and
    // the overworld's height (the octree's).
    y_offset: i32,
    level: crate::assets::Level,
    // The live view's layout (per_tile): each tile's buffer.
    buffers: HashMap<(i32, i32), usize>,
}

struct Section {
    // Its position (Minecraft's section coordinates) and its tile.
    at: [i32; 3],
    tile: (i32, i32),
    buffer: usize,
    // Its first quad's index in its buffer, and in `centroids`.
    first: u32,
    start: usize,
    count: u32,
}

// Minecraft's default render distance in chunks (Options.renderDistance).
const RENDER_DISTANCE: i32 = 12;

impl Translucent {
    pub fn new(
        gpu: &Gpu,
        tiles: &[(i32, i32)],
        geometry: &HashMap<(i32, i32), TileGeom>,
        y_offset: i32,
        level: crate::assets::Level,
    ) -> Result<Option<Translucent>, DeviceError> {
        let per_buffer = Batches::per_buffer(gpu) as u32;
        let mut quads: Vec<&Instance> = Vec::new();
        let mut sections: Vec<Section> = Vec::new();
        let mut centroids: Vec<[f32; 3]> = Vec::new();
        // Each buffer's count of quads.
        let mut counts: Vec<u32> = vec![0];
        // Each tile's blended quads by section, and in a section in the
        // order Minecraft meshes its blocks (a block's quads in the order
        // meshed): the order of its buffer, which the stable sort keeps for
        // quads equally far; with their centroids. Tiles in parallel.
        use rayon::prelude::*;
        let per_tile: Vec<(Vec<usize>, Vec<[f32; 3]>)> = tiles
            .par_iter()
            .map(|at| {
                let blended = &geometry[at].blended;
                let mut by_key: Vec<usize> = (0..blended.len()).collect();
                by_key.sort_by_key(|&i| blended[i].0);
                let centroids = by_key
                    .iter()
                    .map(|&i| {
                        let (key, q) = &blended[i];
                        // The section's origin at the page's heights.
                        let origin = [key.0[0] * 16, key.0[1] * 16 - y_offset, key.0[2] * 16]
                            .map(|v| v as f32);
                        let local = |c: usize| [0, 1, 2].map(|k| q.corners[c][k] - origin[k]);
                        let (v0, v2) = (local(0), local(2));
                        [0, 1, 2].map(|k| (v0[k] + v2[k]) / 2.0)
                    })
                    .collect();
                (by_key, centroids)
            })
            .collect();
        for (at, (by_key, tile_centroids)) in tiles.iter().zip(per_tile) {
            let blended = &geometry[at].blended;
            let mut done = 0;
            for group in by_key.chunk_by(|&a, &b| blended[a].0 .0 == blended[b].0 .0) {
                let key = blended[group[0]].0 .0;
                let n = group.len() as u32;
                if counts.last().unwrap() + n > per_buffer && *counts.last().unwrap() > 0 {
                    counts.push(0);
                }
                let buffer = counts.len() - 1;
                sections.push(Section {
                    at: key,
                    tile: *at,
                    buffer,
                    first: counts[buffer],
                    start: quads.len(),
                    count: n,
                });
                counts[buffer] += n;
                quads.extend(group.iter().map(|&i| &blended[i].1));
                centroids.extend_from_slice(&tile_centroids[done..done + group.len()]);
                done += group.len();
            }
        }
        if quads.is_empty() {
            return Ok(None);
        }
        let mut buffers = Vec::new();
        let mut bytes = 0;
        let mut at = 0;
        for &n in &counts {
            let part = &quads[at..at + n as usize];
            let size = (part.len() * crate::scene::INSTANCE_BYTES) as u64;
            let buffer = crate::gpu::quads_buffer(gpu, size, "translucent", &mut |write| {
                for (k, q) in part.iter().enumerate() {
                    write(k * crate::scene::INSTANCE_BYTES, bytemuck::bytes_of(*q));
                }
            });
            bytes += size;
            buffers.push((buffer, n));
            at += n as usize;
            // Its staging freed before the next is written (the last waited
            // for with the scene's other uploads).
            gpu.queue.submit([]);
            if at < quads.len() {
                gpu.wait()?;
            }
        }
        let order = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("translucent order"),
            size: (quads.len() * 4) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Ok(Some(Translucent {
            batches: Batches { buffers, bytes },
            order,
            sections,
            centroids,
            y_offset,
            level,
            buffers: HashMap::new(),
        }))
    }

    // A tile's translucent quads: (buffer, first quad, count) by section.
    pub fn tile_runs(&self, at: (i32, i32)) -> Vec<(&wgpu::Buffer, u32, u32)> {
        self.sections
            .iter()
            .filter(|s| s.tile == at)
            .map(|s| (&self.batches.buffers[s.buffer].0, s.first, s.count))
            .collect()
    }

    // The live view's layout: each tile's translucent quads in a buffer of
    // its own, so a tile meshed again replaces its own quads alone
    // (replace_tile) and nothing else is uploaded again.
    pub fn per_tile(
        gpu: &Gpu,
        tiles: &[(i32, i32)],
        geometry: &HashMap<(i32, i32), TileGeom>,
        y_offset: i32,
        level: crate::assets::Level,
    ) -> Translucent {
        let order = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("translucent order"),
            size: 4,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut t = Translucent {
            batches: Batches {
                buffers: Vec::new(),
                bytes: 0,
            },
            order,
            sections: Vec::new(),
            centroids: Vec::new(),
            y_offset,
            level,
            buffers: HashMap::new(),
        };
        for at in tiles {
            t.replace_tile(gpu, *at, &geometry[at].blended);
        }
        t
    }

    // One tile's translucent quads replaced (none for a tile gone or without
    // any): its sections and centroids laid out as `new` lays out a tile's,
    // in its own buffer.
    pub fn replace_tile(
        &mut self,
        gpu: &Gpu,
        at: (i32, i32),
        blended: &[(crate::scene::BlendedKey, Instance)],
    ) {
        // The tile's sections out, the others' centroids moved up.
        let mut centroids = Vec::with_capacity(self.centroids.len());
        let mut sections = Vec::with_capacity(self.sections.len());
        for mut s in std::mem::take(&mut self.sections) {
            if s.tile == at {
                continue;
            }
            let start = centroids.len();
            centroids.extend_from_slice(&self.centroids[s.start..s.start + s.count as usize]);
            s.start = start;
            sections.push(s);
        }
        self.centroids = centroids;
        self.sections = sections;
        if let Some(&b) = self.buffers.get(&at) {
            self.batches.bytes -= self.batches.buffers[b].0.size();
            self.batches.buffers[b].1 = 0;
        }
        if blended.is_empty() {
            return;
        }
        let y_offset = self.y_offset;
        let mut by_key: Vec<usize> = (0..blended.len()).collect();
        by_key.sort_by_key(|&i| blended[i].0);
        let size = (blended.len() * crate::scene::INSTANCE_BYTES) as u64;
        let buffer = crate::gpu::quads_buffer(gpu, size, "translucent", &mut |write| {
            for (k, &i) in by_key.iter().enumerate() {
                write(
                    k * crate::scene::INSTANCE_BYTES,
                    bytemuck::bytes_of(&blended[i].1),
                );
            }
        });
        let b = match self.buffers.get(&at) {
            Some(&b) => {
                self.batches.buffers[b] = (buffer, blended.len() as u32);
                b
            }
            None => {
                self.batches.buffers.push((buffer, blended.len() as u32));
                self.buffers.insert(at, self.batches.buffers.len() - 1);
                self.batches.buffers.len() - 1
            }
        };
        self.batches.bytes += size;
        let mut first = 0u32;
        for group in by_key.chunk_by(|&a, &b| blended[a].0 .0 == blended[b].0 .0) {
            let key = blended[group[0]].0 .0;
            self.sections.push(Section {
                at: key,
                tile: at,
                buffer: b,
                first,
                start: self.centroids.len(),
                count: group.len() as u32,
            });
            first += group.len() as u32;
            let origin = [key[0] * 16, key[1] * 16 - y_offset, key[2] * 16].map(|v| v as f32);
            for &i in group {
                let q = &blended[i].1;
                let local = |c: usize| [0, 1, 2].map(|k| q.corners[c][k] - origin[k]);
                let (v0, v2) = (local(0), local(2));
                self.centroids
                    .push([0, 1, 2].map(|k| (v0[k] + v2[k]) / 2.0));
            }
        }
        let needed = (self.centroids.len() * 4) as u64;
        if self.order.size() < needed {
            self.order = gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("translucent order"),
                size: (needed + needed / 2).div_ceil(4) * 4,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
    }

    pub fn bytes(&self) -> u64 {
        self.gpu_bytes() + self.cpu_bytes()
    }

    // Its quads and their order on the GPU.
    pub fn gpu_bytes(&self) -> u64 {
        self.batches.bytes + self.order.size()
    }

    // Its sections and centroids on the CPU.
    pub fn cpu_bytes(&self) -> u64 {
        (self.centroids.capacity() * 12
            + self.sections.capacity() * std::mem::size_of::<Section>()
            + self.buffers.capacity() * std::mem::size_of::<((i32, i32), usize)>()) as u64
    }

    // The quads as Minecraft draws them for a camera at `eye` (the page's
    // coordinates): its translucent layer draws the visible sections in the
    // reverse of the order its octree visits them (ChunkSectionsToRender:
    // the translucent layer's draws reversed; LevelRenderer's visibleSections
    // from SectionOcclusionGraph.addSectionsInFrustum, Octree.visitNodes),
    // each section whole, its quads sorted by VertexSorting.byDistance from
    // the camera relative to the section's origin in f32
    // (RenderSection.createVertexSorting): farthest first by the squared
    // distance to each quad's centroid, a stable sort. Written into `order`.
    pub fn ordered(&self, gpu: &Gpu, eye: [f64; 3]) -> Ordered<'_> {
        use rayon::prelude::*;
        let camera = [eye[0], eye[1] + self.y_offset as f64, eye[2]];
        let visits = octree_visits(
            &self.sections.iter().map(|s| s.at).collect::<Vec<_>>(),
            camera,
            self.level,
        );
        let mut drawn: Vec<usize> = (0..self.sections.len()).collect();
        drawn.sort_unstable_by(|&a, &b| visits[b].cmp(&visits[a]));
        let parts: Vec<Vec<u32>> = drawn
            .par_iter()
            .map(|&i| {
                let s = &self.sections[i];
                let origin = [s.at[0] * 16, s.at[1] * 16, s.at[2] * 16];
                let local = [0, 1, 2].map(|k| (camera[k] - origin[k] as f64) as f32);
                sort_quads(local, &self.centroids[s.start..s.start + s.count as usize])
                    .into_iter()
                    .map(|k| s.first + k)
                    .collect()
            })
            .collect();
        let mut order = Vec::with_capacity(self.centroids.len());
        let mut runs: Vec<(usize, u32, u32)> = Vec::new();
        for (&i, part) in drawn.iter().zip(&parts) {
            let s = &self.sections[i];
            let first = order.len() as u32;
            order.extend_from_slice(part);
            match runs.last_mut() {
                Some((buffer, _, count)) if *buffer == s.buffer => *count += s.count,
                _ => runs.push((s.buffer, first, s.count)),
            }
        }
        gpu.queue
            .write_buffer(&self.order, 0, bytemuck::cast_slice(&order));
        Ordered {
            batches: &self.batches,
            order: &self.order,
            runs,
        }
    }
}

// A section's quads in the order Minecraft draws them for a camera at
// `camera` (relative to the section's origin, f32): VertexSorting.byDistance,
// the squared distance (Vector3f.distanceSquared) to each quad's centroid,
// farthest first (Floats.compare(d[b], d[a]) in IntArrays.mergeSort: a
// stable sort, ties in the buffer's order).
pub fn sort_quads(camera: [f32; 3], centroids: &[[f32; 3]]) -> Vec<u32> {
    let d2 = |c: &[f32; 3]| {
        let (dx, dy, dz) = (camera[0] - c[0], camera[1] - c[1], camera[2] - c[2]);
        dx * dx + dy * dy + dz * dz
    };
    let mut quads: Vec<(f32, u32)> = centroids
        .iter()
        .enumerate()
        .map(|(k, c)| (d2(c), k as u32))
        .collect();
    quads.sort_by(|a, b| b.0.total_cmp(&a.0));
    quads.into_iter().map(|(_, k)| k).collect()
}

// Where Minecraft's octree of sections (Octree, built around the camera's
// section) visits each section: a key whose order is the visit order. The
// root is a cube of the smallest power of two of sections holding the
// render distance each way (at least the default, more where the world
// reaches farther: Minecraft draws no section beyond it), from the level's
// floor upwards where it spans the level's height; each branch splits at
// its centre and visits the child on the camera's side of each axis first,
// the axis the camera is farthest along deciding first
// (Octree$AxisSorting); a branch of 32 blocks holds sections.
pub fn octree_visits(
    sections: &[[i32; 3]],
    camera: [f64; 3],
    level: crate::assets::Level,
) -> Vec<u64> {
    let cam_section = camera.map(|v| (v.floor() as i32).div_euclid(16));
    let reach = sections
        .iter()
        .map(|s| {
            (s[0] - cam_section[0])
                .abs()
                .max((s[2] - cam_section[2]).abs())
        })
        .max()
        .unwrap_or(0);
    let distance = RENDER_DISTANCE.max(reach);
    let size = ((2 * distance + 1) as u32).next_power_of_two() as i32;
    let origin = cam_section.map(|v| v * 16);
    let centre_of_camera = origin.map(|v| v + 8);
    let reach_blocks = distance * 16;
    let min = [
        origin[0] - reach_blocks,
        if size >= level.height / 16 {
            level.min_y
        } else {
            origin[1] - reach_blocks
        },
        origin[2] - reach_blocks,
    ];
    sections
        .iter()
        .map(|s| {
            let block = s.map(|v| v * 16);
            let (mut lo, mut span) = (min, size * 16);
            let mut key = 0u64;
            while span > 16 {
                let centre = lo.map(|v| v + span / 2);
                let diff = [0, 1, 2].map(|k| centre_of_camera[k] - centre[k]);
                let shifts = axis_shifts(diff[0].abs(), diff[1].abs(), diff[2].abs());
                let below = [0, 1, 2].map(|k| block[k] - centre[k] < 0);
                let index: u64 = (0..3)
                    .filter(|&k| below[k] != (diff[k] < 0))
                    .map(|k| shifts[k])
                    .sum();
                key = key * 8 + index;
                lo = [0, 1, 2].map(|k| if below[k] { lo[k] } else { centre[k] });
                span /= 2;
            }
            key
        })
        .collect()
}

// Octree$AxisSorting.getAxisSorting: each axis's weight in a child's index.
fn axis_shifts(x: i32, y: i32, z: i32) -> [u64; 3] {
    if x > y && x > z {
        if y > z {
            [4, 2, 1]
        } else {
            [4, 1, 2]
        }
    } else if y > x && y > z {
        if x > z {
            [2, 4, 1]
        } else {
            [1, 4, 2]
        }
    } else if x > y {
        [2, 1, 4]
    } else {
        [1, 2, 4]
    }
}

fn digest(out: &crate::scene::TileOut) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    bytemuck::cast_slice::<Instance, u8>(&out.opaque).hash(&mut h);
    for (key, i) in &out.blended {
        key.hash(&mut h);
        bytemuck::bytes_of(i).hash(&mut h);
    }
    out.missing.hash(&mut h);
    h.finish()
}

pub fn tile_geom(out: &mut crate::scene::TileOut, names: &[String]) -> TileGeom {
    let digest = digest(out);
    TileGeom {
        opaque_count: out.opaque.len(),
        blocks: std::mem::take(&mut out.blocks),
        missing: out
            .missing
            .iter()
            .map(|&m| {
                names
                    .get(m as usize)
                    .cloned()
                    .unwrap_or_else(|| format!("material {m}"))
            })
            .collect(),
        blended: std::mem::take(&mut out.blended),
        digest,
    }
}

// Every tile's opaque instances in buffers of at most BATCH bytes, tile
// after tile in tile order; where each tile's are.
pub struct Arena {
    pub batches: Batches,
    // By tile: buffer, first instance, count.
    pub ranges: HashMap<(i32, i32), (usize, u32, u32)>,
    // By tile: the instances its place in its buffer holds (the live view
    // writes a tile meshed again in place when it fits: live/world.rs).
    pub rooms: HashMap<(i32, i32), u32>,
}

impl Arena {
    pub fn bytes(&self) -> u64 {
        self.batches.bytes
    }
}

// A tile's opaque instances on their way into an arena: meshed (on the
// CPU), or kept from the arena an edit is made of (its buffer, first
// instance and count there).
enum Part {
    Meshed(Vec<Instance>),
    Kept(usize, u32, u32),
}

impl Part {
    fn count(&self) -> u32 {
        match self {
            Part::Meshed(v) => v.len() as u32,
            Part::Kept(_, _, n) => *n,
        }
    }
}

// An arena made buffer by buffer, so that no more than a buffer's worth of
// meshed quads waits on the CPU (BATCH bytes) and the upload's staging is
// freed buffer by buffer: a tile starts a new buffer when it would pass the
// buffer's size (the tile order and the buffers are those of the whole
// world laid out at once). Kept tiles are copied on the GPU from `old`, and
// each of old's buffers is let go once every tile it holds is copied.
// The arena an edit is made of, the edited world's tiles and the tiles
// meshed again.
type EditOf<'b> = (
    Arena,
    &'b [(i32, i32)],
    &'b HashMap<(i32, i32), crate::scene::TileOut>,
);

struct ArenaBuilder<'a> {
    gpu: &'a Gpu,
    per_buffer: u32,
    pending: Vec<((i32, i32), Part)>,
    count: u32,
    buffers: Vec<(wgpu::Buffer, u32)>,
    ranges: HashMap<(i32, i32), (usize, u32, u32)>,
    old: Vec<Option<wgpu::Buffer>>,
    // By buffer of old: the last tile (in push order) copied from it.
    old_last: Vec<usize>,
    pushed: usize,
}

impl<'a> ArenaBuilder<'a> {
    fn new(gpu: &'a Gpu, old: Option<EditOf>) -> ArenaBuilder<'a> {
        let (old, old_last) = match old {
            None => (Vec::new(), Vec::new()),
            Some((arena, tiles, changed)) => {
                let mut last = vec![0; arena.batches.buffers.len()];
                for (k, t) in tiles.iter().enumerate() {
                    if !changed.contains_key(t) {
                        last[arena.ranges[t].0] = k;
                    }
                }
                (
                    arena
                        .batches
                        .buffers
                        .into_iter()
                        .map(|(b, _)| Some(b))
                        .collect(),
                    last,
                )
            }
        };
        ArenaBuilder {
            gpu,
            per_buffer: Batches::per_buffer(gpu) as u32,
            pending: Vec::new(),
            count: 0,
            buffers: Vec::new(),
            ranges: HashMap::new(),
            old,
            old_last,
            pushed: 0,
        }
    }

    // The next tile's instances; answers whether a buffer was made (the
    // caller may then make room).
    fn push(&mut self, at: (i32, i32), part: Part) -> Result<bool, DeviceError> {
        let n = part.count();
        let flushed = self.count + n > self.per_buffer && self.count > 0;
        if flushed {
            self.flush(true)?;
        }
        self.pending.push((at, part));
        self.count += n;
        self.pushed += 1;
        Ok(flushed)
    }

    // The pending tiles into a buffer; with `wait`, the GPU waited for (the
    // upload's staging then freed before the next buffer is meshed).
    fn flush(&mut self, wait: bool) -> Result<(), DeviceError> {
        let size = crate::scene::INSTANCE_BYTES;
        let b = self.buffers.len();
        let mut first = 0u32;
        let mut copies = Vec::new();
        for (at, part) in &self.pending {
            let n = part.count();
            self.ranges.insert(*at, (b, first, n));
            if let Part::Kept(ob, of, _) = part {
                if n > 0 {
                    copies.push((*ob, *of, first, n));
                }
            }
            first += n;
        }
        if self.count > 0 {
            let pending = &self.pending;
            let bytes = self.count as u64 * size as u64;
            let buffer = if copies.is_empty() {
                // All meshed: written as the buffer is made.
                crate::gpu::quads_buffer(self.gpu, bytes, "blocks", &mut |write| {
                    let mut at = 0usize;
                    for (_, part) in pending {
                        if let Part::Meshed(v) = part {
                            if !v.is_empty() {
                                write(at * size, bytemuck::cast_slice(v));
                            }
                        }
                        at += part.count() as usize;
                    }
                })
            } else {
                // An edit: the kept tiles copied on the GPU, the few meshed
                // again written into place.
                let buffer = self.gpu.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("blocks"),
                    size: bytes,
                    usage: crate::gpu::QUADS,
                    mapped_at_creation: false,
                });
                let mut at = 0u64;
                for (_, part) in pending {
                    if let Part::Meshed(v) = part {
                        if !v.is_empty() {
                            self.gpu.queue.write_buffer(
                                &buffer,
                                at * size as u64,
                                bytemuck::cast_slice(v),
                            );
                        }
                    }
                    at += part.count() as u64;
                }
                let mut encoder = self.gpu.device.create_command_encoder(&Default::default());
                for (ob, of, to, n) in copies {
                    let from = self.old[ob]
                        .as_ref()
                        .expect("an old buffer is kept until its tiles are copied");
                    encoder.copy_buffer_to_buffer(
                        from,
                        of as u64 * size as u64,
                        &buffer,
                        to as u64 * size as u64,
                        n as u64 * size as u64,
                    );
                }
                self.gpu.queue.submit([encoder.finish()]);
                buffer
            };
            self.buffers.push((buffer, self.count));
        }
        // The old buffers no tile still to come is copied from.
        for (ob, last) in self.old_last.iter().enumerate() {
            if *last < self.pushed {
                self.old[ob] = None;
            }
        }
        // The upload's staging (and the old buffers) freed once the GPU has the buffer.
        self.gpu.queue.submit([]);
        if wait {
            self.gpu.wait()?;
        }
        self.pending.clear();
        self.count = 0;
        Ok(())
    }

    // The bytes of the buffers made so far.
    fn bytes(&self) -> u64 {
        self.buffers
            .iter()
            .map(|(_, c)| *c as u64 * crate::scene::INSTANCE_BYTES as u64)
            .sum()
    }

    // The last buffer, waited for with the scene's other uploads.
    fn finish(mut self) -> Result<Arena, DeviceError> {
        self.flush(false)?;
        let bytes = self
            .buffers
            .iter()
            .map(|(_, c)| *c as u64 * crate::scene::INSTANCE_BYTES as u64)
            .sum();
        let ranges = std::mem::take(&mut self.ranges);
        let rooms = ranges.iter().map(|(&at, &(_, _, n))| (at, n)).collect();
        Ok(Arena {
            batches: Batches {
                buffers: std::mem::take(&mut self.buffers),
                bytes,
            },
            ranges,
            rooms,
        })
    }
}

#[derive(Default, Debug, Clone, serde::Serialize)]
pub struct SceneTimes {
    pub read: f64,
    pub mesh: f64,
    pub entities: f64,
    pub upload: f64,
    pub total: f64,
}

// What entities read of a lit world: its light and the mesher's lone quads.
pub struct WorldView<'a> {
    pub draw: &'a Draw<'a>,
    pub world: &'a World,
    pub names: &'a [String],
    pub lone: RefCell<HashMap<String, Vec<BlockQuad>>>,
}

impl View for WorldView<'_> {
    fn light(&self, cell: [i32; 3]) -> [i32; 2] {
        let level = self.draw.cell_light(self.world, cell);
        [level, level]
    }

    fn levels(&self, cell: [i32; 3]) -> [i32; 2] {
        let [sky, block] = self.draw.cell_levels(self.world, cell);
        [block as i32, sky as i32]
    }

    fn block_quads(&self, state: &str) -> Result<Vec<BlockQuad>, String> {
        if let Some(q) = self.lone.borrow().get(state) {
            return Ok(q.clone());
        }
        let quads = entities::lone_quads(self.draw.pack, self.names, state)?;
        self.lone
            .borrow_mut()
            .insert(state.to_string(), quads.clone());
        Ok(quads)
    }
}

pub fn entity_instances(
    placed: &[entities::Placed],
    assets: &Assets,
) -> Result<Vec<Instance>, String> {
    if placed.is_empty() {
        return Ok(Vec::new());
    }
    let atlas = &assets.entity()?.atlas;
    placed
        .iter()
        .map(|p| entities::instance(p, atlas))
        .collect()
}

// The entities of a scene (those inside a clip), placed and lit in the
// world.
fn scene_entities(
    assets: &Assets,
    draw: &Draw,
    world: &World,
    entities: &[Value],
    clip: Option<Clip>,
) -> Result<(Vec<Value>, entities::Sorted, Vec<Instance>), String> {
    // shotEntities: those whose position is in the box.
    let shown: Vec<Value> = match clip {
        None => entities.to_vec(),
        Some(c) => entities
            .iter()
            .filter(|e| {
                (0..3).all(|i| {
                    e["pos"][i]
                        .as_f64()
                        .is_some_and(|v| v >= c.min[i] as f64 && v < c.max[i] as f64 + 1.0)
                })
            })
            .cloned()
            .collect(),
    };
    let view = WorldView {
        draw,
        world,
        names: &assets.names,
        lone: RefCell::new(HashMap::new()),
    };
    let sorted = if shown.is_empty() {
        entities::Sorted::default()
    } else {
        entities::build(&assets.entity()?.capture, &shown, &view)?
    };
    let solid = entity_instances(&sorted.solid, assets)?;
    Ok((shown, sorted, solid))
}

// Meshes, lights and uploads a world, or the part of it inside `clip`: a
// group of tiles at a time, their opaque quads let go on the CPU once in a
// buffer. `room` is called after each buffer is made with the scene's bytes
// so far (the store makes room for the scene as it grows: PROTOCOL.md, What
// is kept).
pub fn make_scene(
    assets: &Assets,
    gpu: &Gpu,
    kept: &Kept,
    clip: Option<Clip>,
    room: &mut dyn FnMut(Growing),
) -> Result<(Scene, SceneTimes), Failure> {
    let t0 = crate::now_ms();
    let mut times = SceneTimes::default();
    let y = kept.y_offset;
    let lifted = clip.map(|c| {
        (
            [c.min[0], c.min[1] + y, c.min[2]],
            [c.max[0], c.max[1] + y, c.max[2]],
        )
    });
    let clipped = blueprint::clip(&kept.blueprint, lifted)?;
    let (lo, hi) = clipped
        .bounds
        .ok_or("Nothing to draw: the world holds no blocks there")?;
    let world = World::read(&assets.pack, &clipped.bytes, EVERYWHERE)?;
    drop(clipped.bytes);
    times.read = crate::now_ms() - t0;
    let mut scene = mesh_world(
        assets,
        gpu,
        &world,
        y,
        &kept.entities,
        clip,
        (lo, hi),
        clipped.blocks,
        room,
        &mut times,
    )?;
    // The whole world's scene keeps its lit world for edits; a clip's not.
    if clip.is_none() {
        let states = blueprint::palette_states(&assets.pack, &kept.blueprint)?;
        let map = (0..states.len() as u16).collect();
        scene.lit = Some(Lit { world, states, map });
    }
    times.total = crate::now_ms() - t0;
    Ok((scene, times))
}

// The scene of a lit world: its tiles meshed and uploaded a group at a
// time, its entities placed (those inside `clip`), its translucent quads;
// it keeps no lit world. `bounds` are the blocks' at Minecraft's heights.
#[allow(clippy::too_many_arguments)]
pub fn mesh_world(
    assets: &Assets,
    gpu: &Gpu,
    world: &World,
    y: i32,
    entities: &[Value],
    clip: Option<Clip>,
    (lo, hi): ([i32; 3], [i32; 3]),
    blocks: u64,
    room: &mut dyn FnMut(Growing),
    times: &mut SceneTimes,
) -> Result<Scene, Failure> {
    let t = crate::now_ms();
    let draw = Draw {
        pack: &assets.pack,
        facts: &assets.facts,
        y_offset: y,
    };
    let tiles = mcbench_renderer::mesh::tiles(world);
    let world_bytes = lit_bytes(world);
    // Meshing and the upload of its opaque quads, group by group.
    let mut geometry: HashMap<(i32, i32), TileGeom> = HashMap::new();
    let mut builder = ArenaBuilder::new(gpu, None);
    let mut upload = 0.0;
    // What waits on the CPU: the tiles being meshed and a few ahead
    // (Draw::tiles_in_order), and at most a buffer of quads.
    draw.tiles_in_order(
        world,
        &assets.names,
        &assets.atlas,
        &tiles,
        &mut |at, mut out| -> Result<(), Failure> {
            let u = crate::now_ms();
            geometry.insert(at, tile_geom(&mut out, &assets.names));
            if builder.push(at, Part::Meshed(out.opaque))? {
                room(Growing {
                    gpu: builder.bytes(),
                    cpu: world_bytes,
                });
            }
            upload += crate::now_ms() - u;
            Ok(())
        },
    )?;
    let u = crate::now_ms();
    let arena = builder.finish()?;
    upload += crate::now_ms() - u;
    times.mesh = crate::now_ms() - t - upload;
    let t = crate::now_ms();
    let (shown, sorted, solid) = scene_entities(assets, &draw, world, entities, clip)?;
    times.entities = crate::now_ms() - t;
    let t = crate::now_ms();
    let translucent = Translucent::new(gpu, &tiles, &geometry, y, assets.level)?;
    let solid_batches = (!solid.is_empty()).then(|| Batches::upload(gpu, &solid, "entities"));
    // The buffers' contents go to the GPU with the next submission: this one.
    gpu.queue.submit([]);
    gpu.wait()?;
    times.upload = upload + crate::now_ms() - t;
    let entity_quads = sorted.solid.len() + sorted.blended.len() + sorted.facing.len();
    Ok(Scene {
        tiles,
        geometry,
        arena,
        translucent,
        solid: solid_batches,
        sorted,
        entities: shown,
        bounds: ([lo[0], lo[1] - y, lo[2]], [hi[0], hi[1] - y, hi[2]]),
        entity_quads,
        blocks_count: blocks,
        used: 0,
        lit: None,
    })
}

// The whole world's scene of `kept`, made from the scene of the world it
// edits (`base`, whose lit world it takes): the cells that differ applied
// through the mesher's patch, only the tiles it names meshed and uploaded
// again. Answers the scene and how many tiles were meshed.
pub fn patch_scene(
    assets: &Assets,
    gpu: &Gpu,
    mut base: Scene,
    base_blueprint: &[u8],
    kept: &Kept,
    room: &mut dyn FnMut(Growing),
) -> Result<(Scene, SceneTimes, usize), Failure> {
    let t0 = crate::now_ms();
    let mut times = SceneTimes::default();
    let y = kept.y_offset;
    let mut lit = base.lit.take().ok_or("The base scene keeps no lit world")?;
    let edit = blueprint::edit(
        &assets.pack,
        &lit.states,
        &lit.map,
        &kept.blueprint,
        base_blueprint,
    )?;
    let (lo, hi) = edit
        .bounds
        .ok_or("Nothing to draw: the world holds no blocks")?;
    let changed = lit.world.patch(&assets.pack, &edit.patch)?;
    // The tiles it names that the world still has (the others are gone).
    let tiles = mcbench_renderer::mesh::tiles(&lit.world);
    let changed: Vec<(i32, i32)> = changed
        .into_iter()
        .filter(|at| tiles.binary_search(at).is_ok())
        .collect();
    lit.states.extend(edit.added_states);
    lit.map = edit.map;
    times.read = crate::now_ms() - t0;
    let t = crate::now_ms();
    let draw = Draw {
        pack: &assets.pack,
        facts: &assets.facts,
        y_offset: y,
    };
    let outs = draw.tiles(&lit.world, &assets.names, &assets.atlas, &changed);
    times.mesh = crate::now_ms() - t;
    let t = crate::now_ms();
    let (shown, sorted, solid) = scene_entities(assets, &draw, &lit.world, &kept.entities, None)?;
    times.entities = crate::now_ms() - t;
    let t = crate::now_ms();
    let mut geometry = std::mem::take(&mut base.geometry);
    geometry.retain(|at, _| tiles.binary_search(at).is_ok());
    let mut outs: HashMap<(i32, i32), crate::scene::TileOut> =
        changed.iter().copied().zip(outs).collect();
    for (at, out) in outs.iter_mut() {
        geometry.insert(*at, tile_geom(out, &assets.names));
    }
    // The arena again, buffer by buffer: the tiles kept copied on the GPU
    // from the base's (each of its buffers let go once copied), the tiles
    // meshed again written.
    let old = std::mem::replace(
        &mut base.arena,
        Arena {
            batches: Batches {
                buffers: Vec::new(),
                bytes: 0,
            },
            ranges: HashMap::new(),
            rooms: HashMap::new(),
        },
    );
    let ranges = old.ranges.clone();
    // The base's buffers are counted until the edit's are made (each is let
    // go as soon as its tiles are copied).
    let (old_bytes, world_bytes) = (old.bytes(), lit_bytes(&lit.world));
    let mut builder = ArenaBuilder::new(gpu, Some((old, &tiles, &outs)));
    for at in &tiles {
        let part = match outs.remove(at) {
            Some(out) => Part::Meshed(out.opaque),
            None => {
                let (b, first, n) = ranges[at];
                Part::Kept(b, first, n)
            }
        };
        if builder.push(*at, part)? {
            room(Growing {
                gpu: old_bytes + builder.bytes(),
                cpu: world_bytes,
            });
        }
    }
    let arena = builder.finish()?;
    drop(base);
    let translucent = Translucent::new(gpu, &tiles, &geometry, y, assets.level)?;
    let solid_batches = (!solid.is_empty()).then(|| Batches::upload(gpu, &solid, "entities"));
    gpu.queue.submit([]);
    gpu.wait()?;
    times.upload = crate::now_ms() - t;
    times.total = crate::now_ms() - t0;
    let entity_quads = sorted.solid.len() + sorted.blended.len() + sorted.facing.len();
    let meshed = changed.len();
    Ok((
        Scene {
            tiles,
            geometry,
            arena,
            translucent,
            solid: solid_batches,
            sorted,
            entities: shown,
            bounds: ([lo[0], lo[1] - y, lo[2]], [hi[0], hi[1] - y, hi[2]]),
            entity_quads,
            blocks_count: edit.blocks,
            used: 0,
            lit: Some(lit),
        },
        times,
        meshed,
    ))
}

// One shot of a scene: its picture read back, before the grid and JPEG.
pub fn draw_shot(
    assets: &Assets,
    gpu: &Gpu,
    renderer: &Renderer,
    scene: &Scene,
    shot: &Shot,
    supersample: u32,
) -> Result<(Vec<u8>, crate::gpu::PictureTimes), Failure> {
    let draws = prepare(assets, gpu, renderer, scene, shot, supersample)?;
    renderer
        .picture(
            gpu,
            &shot.camera,
            scene.bounds,
            shot.width,
            shot.height,
            supersample,
            &draws,
        )
        .map_err(|DeviceError(e)| Failure::Device(e))
}

// The same shot, its parts submitted one at a time and timed (Renderer::phases).
pub fn draw_shot_phases(
    assets: &Assets,
    gpu: &Gpu,
    renderer: &Renderer,
    scene: &Scene,
    shot: &Shot,
    supersample: u32,
) -> Result<(Vec<u8>, crate::gpu::Phases), Failure> {
    let draws = prepare(assets, gpu, renderer, scene, shot, supersample)?;
    renderer
        .phases(
            gpu,
            &shot.camera,
            scene.bounds,
            shot.width,
            shot.height,
            supersample,
            &draws,
        )
        .map_err(|DeviceError(e)| Failure::Device(e))
}

fn prepare<'a>(
    assets: &Assets,
    gpu: &Gpu,
    renderer: &Renderer,
    scene: &'a Scene,
    shot: &Shot,
    supersample: u32,
) -> Result<Draws<'a>, Failure> {
    let side = gpu.device.limits().max_texture_dimension_2d;
    if shot.width == 0 || shot.height == 0 || shot.width.max(shot.height) * supersample > side {
        return Err(Failure::Other(format!("A shot of {}x{} at supersample {supersample} needs textures larger than this adapter's {side}", shot.width, shot.height)));
    }
    shot.camera.check()?;
    let (bw, bh) = (shot.width * supersample, shot.height * supersample);
    let frame = shot.camera.frame(bw as f64 / bh as f64);
    let behind = if scene.entities.is_empty() {
        Vec::new()
    } else {
        entity_instances(
            &entities::behind(
                &assets.entity()?.capture,
                &scene.entities,
                &scene.sorted,
                shot.camera.yaw,
                shot.camera.pitch,
                frame.eye,
            )?,
            assets,
        )?
    };
    if !scene.entities.is_empty() {
        renderer.entity_atlas(gpu, &assets.entity()?.atlas);
    }
    Ok(Draws {
        blocks: scene.block_batches(),
        translucent: scene
            .translucent
            .as_ref()
            .map(|t| t.ordered(gpu, frame.eye)),
        solid: scene.solid.as_ref(),
        behind: (!behind.is_empty()).then(|| Batches::upload(gpu, &behind, "behind")),
    })
}

// What the helper keeps, and the bound on it (PROTOCOL.md, What is kept).
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub worlds: usize,
    pub scenes: usize,
    // The process's memory the helper keeps itself within between calls, by
    // the bound's count (Store::counted): the server's `memoryLimit`.
    pub bytes: u64,
}

// The default bounds of the process's memory. On a GPU adapter: what keeps
// a 1 to 2 million block chat's world, its edit and a large shot's picture
// targets (on the M3, whose GPU shares the machine's memory, pagoda2m drawn
// holds about 800 MB in all). On a software adapter, whose "GPU" memory is
// the process's own: less, so a large shot's picture targets (519 MB) are
// made again by each call rather than kept beside a large world.
pub const DEFAULT_BYTES: u64 = 1 << 30;
pub const DEFAULT_SOFTWARE_BYTES: u64 = 768 << 20;

impl Limits {
    // A software adapter keeps fewer worlds and scenes too.
    pub fn new(bytes: Option<u64>, software: bool) -> Limits {
        if software {
            Limits {
                worlds: 2,
                scenes: 3,
                bytes: bytes.unwrap_or(DEFAULT_SOFTWARE_BYTES),
            }
        } else {
            Limits {
                worlds: 4,
                scenes: 6,
                bytes: bytes.unwrap_or(DEFAULT_BYTES),
            }
        }
    }
}

pub struct Store {
    pub worlds: HashMap<String, Kept>,
    pub scenes: HashMap<(String, Option<Clip>), Scene>,
    clock: u64,
    pub limits: Limits,
    // The world the live view shows: not counted in the limits of worlds and
    // scenes, and neither it nor its whole scene is let go while it is shown
    // (the renderer's answers, 2). Its clips' scenes are kept as any other.
    pub pinned: Option<String>,
    // What the bound counts besides what the store, the picture targets and
    // the device hold. In the helper, its process's memory less what the
    // count holds: measured when it is ready (`ready`), and then once, for
    // the helper's life, after the first reply that drew a picture
    // (`release`), when the driver has made what it keeps for drawing (its
    // pool of host memory on an RTX 5090, SwiftShader's own memory). In the
    // app's process (`in_process`) none: the process holds the app's own
    // memory too, which is not the renderer's to bound. The keep-or-evict
    // decisions rest on counts, so the same requests keep the same worlds
    // and scenes on any load.
    pub overhead: u64,
    // Whether a reply has drawn a picture, and whether the overhead has
    // been measured after it.
    drawn: bool,
    settled: bool,
    // Whether the process is the helper's own, whose memory is measured
    // for the overhead and to give freed memory back.
    measured: bool,
    pub counting: Counting,
}

// How the GPU's memory is counted (Store::counted). A discrete GPU's memory
// is its own: only what is read back into the process is counted (the
// picture targets' readbacks, the live view's ring). On every other adapter
// the GPU's memory is the process's or the machine's (a software adapter's
// is the process's heap, an integrated GPU's the machine's, Apple's
// footprint holds it), and all of it is counted: as the device reports it
// (Gpu::memory: the blocks Vulkan's and Direct3D 12's allocator has
// reserved, Metal's allocated size), which holds what is freed inside a
// block until the whole block is; on GL, whose driver allocates each
// resource on its own, as the resources' sizes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Counting {
    Readbacks,
    Device,
    Resources,
}

impl Counting {
    pub fn of(gpu: &Gpu) -> Counting {
        if gpu.info.device_type == wgpu::DeviceType::DiscreteGpu {
            Counting::Readbacks
        } else if gpu.memory().is_some() {
            Counting::Device
        } else {
            Counting::Resources
        }
    }

    // Whether the resources on the GPU are counted by their sizes (as
    // opposed to their readbacks alone, or the device's figure).
    pub fn sizes(self) -> bool {
        self == Counting::Resources
    }

    // Whether resources about to be made are counted whole (they are not
    // in the device's figure yet).
    pub fn new_whole(self) -> bool {
        self != Counting::Readbacks
    }
}

// The bytes of a scene being made, so far: its quads on the GPU and what it
// holds on the CPU (its lit world), passed to `room` as it grows.
#[derive(Clone, Copy, Debug, Default)]
pub struct Growing {
    pub gpu: u64,
    pub cpu: u64,
}

impl Growing {
    // Its quads already made are in the device's figure where it is read.
    pub fn counted(&self, counting: Counting) -> u64 {
        self.cpu + if counting.sizes() { self.gpu } else { 0 }
    }
}

// What the bound counts outside the store: the picture targets (the one
// thing it lets go that the store does not hold) and `extra` bytes, about
// to be allocated or held elsewhere (the live view's targets).
pub struct Room<'a> {
    pub gpu: &'a Gpu,
    pub renderer: &'a Renderer,
    pub extra: u64,
}

impl Store {
    pub fn new(limits: Limits) -> Store {
        Store {
            worlds: HashMap::new(),
            scenes: HashMap::new(),
            clock: 0,
            limits,
            pinned: None,
            overhead: 0,
            drawn: false,
            settled: false,
            measured: false,
            counting: Counting::Resources,
        }
    }

    // In the helper, once the device and the renderer are made: how the
    // GPU's memory is counted, and the overhead measured (freed memory
    // given back first): the process's memory less the device's where that
    // is counted.
    pub fn ready(&mut self, gpu: &Gpu) {
        self.in_process(gpu);
        self.measured = true;
        crate::memory::give_back();
        let device = match self.counting {
            Counting::Device => gpu.memory().expect("the device reports its memory"),
            _ => 0,
        };
        self.overhead = crate::memory::held(gpu.allocated()).saturating_sub(device);
    }

    // In the app's process (live::Draw): whether the GPU's memory is
    // counted; no overhead, and the process's memory is never read.
    pub fn in_process(&mut self, gpu: &Gpu) {
        self.counting = Counting::of(gpu);
    }

    // The bytes the store keeps on the CPU, and on the GPU where the
    // resources are counted by their sizes (Counting).
    pub fn kept_bytes(&self) -> u64 {
        self.worlds.values().map(Kept::bytes).sum::<u64>()
            + self
                .scenes
                .values()
                .map(|s| {
                    s.cpu_bytes()
                        + if self.counting.sizes() {
                            s.gpu_bytes()
                        } else {
                            0
                        }
                })
                .sum::<u64>()
    }

    // The bound's count of the process's memory: the overhead, what the
    // store keeps, the picture targets kept (their readbacks, or whole by
    // their sizes), the device's memory where it is counted as the device
    // reports it, and the room's extra bytes.
    pub fn counted(&self, room: &Room) -> u64 {
        let (targets, device) = match self.counting {
            Counting::Readbacks => (room.renderer.kept_targets_bytes(room.gpu, false), 0),
            Counting::Device => (0, room.gpu.memory().expect("the device reports its memory")),
            Counting::Resources => (room.renderer.kept_targets_bytes(room.gpu, true), 0),
        };
        self.overhead + self.kept_bytes() + targets + device + room.extra
    }

    pub fn tick(&mut self) -> u64 {
        self.clock += 1;
        self.clock
    }

    pub fn put_world(&mut self, name: &str, kept: Kept) {
        self.drop_world(name);
        self.worlds.insert(name.to_string(), kept);
        self.evict_counts(name);
    }

    pub fn drop_world(&mut self, name: &str) -> bool {
        self.scenes.retain(|(world, _), _| world != name);
        self.worlds.remove(name).is_some()
    }

    // A scene of a world kept. Only the newest whole-world scene keeps its
    // lit world (an edit is of the world drawn last; an older one is read
    // again).
    pub fn put_scene(&mut self, key: (String, Option<Clip>), scene: Scene) {
        if key.1.is_none() && scene.lit.is_some() {
            let pinned = self.pinned.clone();
            for (k, s) in self.scenes.iter_mut() {
                if !(k.1.is_none() && Some(&k.0) == pinned.as_ref()) {
                    s.lit = None;
                }
            }
        }
        self.scenes.insert(key.clone(), scene);
        self.evict_counts(&key.0);
    }

    // Whether a scene is the shown world's whole scene.
    pub fn shown(&self, key: &(String, Option<Clip>)) -> bool {
        key.1.is_none() && self.pinned.as_ref() == Some(&key.0)
    }

    // Least recently used first, scenes then worlds, until the counts are
    // within the limits; the world `keep` and its scenes stay, and the shown
    // world and its whole scene, which neither limit counts.
    fn evict_counts(&mut self, keep: &str) {
        let scenes = |s: &Store| s.scenes.keys().filter(|k| !s.shown(k)).count();
        let worlds = |s: &Store| {
            s.worlds
                .keys()
                .filter(|n| s.pinned.as_ref() != Some(*n))
                .count()
        };
        while scenes(self) > self.limits.scenes {
            let Some(oldest) = self
                .scenes
                .iter()
                .filter(|(k, _)| k.0 != keep && !self.shown(k))
                .min_by_key(|(_, s)| s.used)
                .map(|(k, _)| k.clone())
            else {
                break;
            };
            self.scenes.remove(&oldest);
        }
        while worlds(self) > self.limits.worlds {
            let Some(oldest) = self
                .worlds
                .iter()
                .filter(|(n, _)| n.as_str() != keep && self.pinned.as_ref() != Some(*n))
                .min_by_key(|(_, k)| k.used)
                .map(|(n, _)| n.clone())
            else {
                break;
            };
            self.drop_world(&oldest);
        }
    }

    // Until the bound's count is within the limit, during a call that began
    // at tick `since`: the scenes the call has not drawn, least recently
    // used first; then the picture targets (the size drawn longest ago
    // first: made again by the next picture of that size in tens of
    // milliseconds); after the call (`after`), then the scenes it drew (a
    // world whose scene alone passes the limit is drawn and then let go:
    // its blueprint kept, the next call meshes it again). Worlds themselves
    // (their blueprints and entities, small) are bounded by their count.
    // Counts only, so the same requests keep the same worlds and scenes on
    // any load (microseconds); answers whether anything was let go.
    pub fn shed(&mut self, since: u64, after: bool, room: &Room) -> bool {
        let limit = self.limits.bytes;
        let mut let_go = false;
        while self.counted(room) > limit {
            if let Some(oldest) = self
                .scenes
                .iter()
                .filter(|(k, s)| s.used < since && !self.shown(k))
                .min_by_key(|(_, s)| s.used)
                .map(|(k, _)| k.clone())
            {
                self.scenes.remove(&oldest);
            } else if room.renderer.release_oldest_targets() {
            } else if let Some(oldest) = self
                .scenes
                .iter()
                .filter(|(k, _)| after && !self.shown(k))
                .min_by_key(|(_, s)| s.used)
                .map(|(k, _)| k.clone())
            {
                self.scenes.remove(&oldest);
            } else {
                break;
            }
            let_go = true;
            if self.counting == Counting::Device {
                // What was let go freed by the device before its memory is
                // read again (a device error is answered by the request's
                // own next wait).
                let _ = room.gpu.device.poll(wgpu::PollType::wait_indefinitely());
            }
        }
        self.drawn |= after;
        let_go
    }

    // In the helper, what the allocators keep of freed memory given back to
    // the system when the process holds more than the limit (a malloc_trim
    // takes milliseconds, so only then). The process's memory is read here
    // to give memory back, never to choose what is kept.
    pub fn give_back_over(&self, room: &Room) {
        if self.measured && crate::memory::held(room.gpu.allocated()) > self.limits.bytes {
            crate::memory::give_back();
        }
    }

    // Once no request waits (PROTOCOL.md, What is kept): the memory the
    // device freed given back, and in the helper what the allocators keep
    // of it where the process holds more than the limit (give_back_over).
    // After the helper's first reply that drew a picture, its overhead
    // measured, once: freed memory given back, then the process's memory
    // less what the count holds.
    pub fn release(&mut self, room: &Room) -> Result<(), DeviceError> {
        room.gpu.wait()?;
        if self.measured && self.drawn && !self.settled {
            crate::memory::give_back();
            let counted = self.counted(room) - self.overhead;
            self.overhead = crate::memory::held(room.gpu.allocated()).saturating_sub(counted);
            self.settled = true;
        } else {
            self.give_back_over(room);
        }
        Ok(())
    }
}

// A world to keep (PROTOCOL.md world, Renderer::world): its name, the
// page's height offset, the blueprint, the entities, and the kept world it
// is an edit of and whether that base stays kept beside it.
pub struct Upload {
    pub name: String,
    pub y_offset: i32,
    pub blueprint: Vec<u8>,
    pub entities: Vec<Value>,
    pub base: Option<String>,
    pub keep_base: bool,
}

// What keeping a world made: its scene's counts and times, the tiles meshed
// when it was applied to its base as an edit, and the store's tick of it.
pub struct Made {
    pub quads: usize,
    pub entity_quads: usize,
    pub tiles: usize,
    pub blocks: u64,
    pub bounds: ([i32; 3], [i32; 3]),
    pub bytes: u64,
    pub missing: Vec<String>,
    pub times: SceneTimes,
    pub meshed: Option<usize>,
    pub used: u64,
}

// Keeps a world and its whole scene (PROTOCOL.md, What is kept): an edit of
// a base whose lit scene is kept and that is not kept itself is patched,
// anything else read fresh. `between` runs after each of the scene's
// buffers is made (the live view's turn in-process).
pub fn keep_world(
    store: &mut Store,
    assets: &Assets,
    gpu: &Gpu,
    renderer: &Renderer,
    upload: Upload,
    between: &mut dyn FnMut(&Store),
) -> Result<Made, Failure> {
    let Upload {
        name,
        y_offset,
        blueprint,
        entities,
        base,
        keep_base,
    } = upload;
    if let Some(base) = &base {
        if !store.worlds.contains_key(base) {
            return Err(Failure::Other(format!("No world named {base} is kept")));
        }
        // The shown world is let go only by a new revision of itself.
        if store.pinned.as_ref() == Some(base) && *base != name && !keep_base {
            return Err(Failure::Other(format!("The world {base} is shown in the live view: an edit of it is kept under its own name, or with keepBase")));
        }
    }
    let used = store.tick();
    let kept = Kept {
        blueprint,
        y_offset,
        entities,
        used,
    };
    let edited = match &base {
        Some(base)
            if !keep_base
                && store.worlds[base].y_offset == y_offset
                && store
                    .scenes
                    .get(&(base.clone(), None))
                    .is_some_and(|s| s.lit.is_some()) =>
        {
            let base_scene = store.scenes.remove(&(base.clone(), None)).unwrap();
            let base_kept = store.worlds.remove(base).unwrap();
            store.drop_world(base);
            Some((base_scene, base_kept))
        }
        _ => None,
    };
    if let (Some(base), false) = (&base, keep_base) {
        store.drop_world(base);
    }
    store.drop_world(&name);
    // Room made for the scene, and the world, as its buffers are made.
    let kept_bytes = kept.bytes();
    let mut room = |growing: Growing| {
        let extra = kept_bytes + growing.counted(store.counting);
        let room = Room {
            gpu,
            renderer,
            extra,
        };
        if store.shed(used, false, &room) {
            // What was let go freed by the device before more is made (a
            // device error is answered by the request's own next wait).
            let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
        }
        store.give_back_over(&room);
        between(store);
    };
    let (mut scene, times, meshed) = match edited {
        Some((base_scene, base_kept)) => {
            let (scene, times, meshed) = patch_scene(
                assets,
                gpu,
                base_scene,
                &base_kept.blueprint,
                &kept,
                &mut room,
            )?;
            (scene, times, Some(meshed))
        }
        None => {
            let (scene, times) = make_scene(assets, gpu, &kept, None, &mut room)?;
            (scene, times, None)
        }
    };
    scene.used = used;
    let made = Made {
        quads: scene.quads(),
        entity_quads: scene.entity_quads,
        tiles: scene.tiles.len(),
        blocks: scene.blocks_count,
        bounds: scene.bounds,
        bytes: scene.bytes(),
        missing: scene.missing(),
        times,
        meshed,
        used,
    };
    store.put_world(&name, kept);
    store.put_scene((name, None), scene);
    Ok(made)
}

// Each shot's timings (PROTOCOL.md shots' reply), its encoded picture, and
// the store's tick of the call.
pub type Drawn = (Vec<Value>, Vec<Vec<u8>>, u64);

// Pictures of a kept world's shots and their timings (PROTOCOL.md shots):
// each shot drawn on this thread in order, its grid and encoding (JPEG at
// `quality`, or PNG for none) on rayon's threads, so the next shot draws
// while one encodes. `cancelled` is asked before each shot; `between` runs
// after each shot's draw and each buffer of a scene made for one (the live
// view's turn in-process). Answers the store's tick of the call with them.
#[allow(clippy::too_many_arguments)]
pub fn draw_shots(
    store: &mut Store,
    assets: &Assets,
    gpu: &Gpu,
    renderer: &Renderer,
    font: &Arc<Font>,
    name: &str,
    shots: &[Shot],
    supersample: u32,
    quality: Option<u8>,
    cancelled: &dyn Fn() -> bool,
    between: &mut dyn FnMut(&Store),
) -> Result<Drawn, Failure> {
    if !(1..=4).contains(&supersample) {
        return Err("supersample must be 1 to 4".into());
    }
    if !store.worlds.contains_key(name) {
        return Err(Failure::Other(format!("No world named {name} is kept")));
    }
    let used = store.tick();
    store.worlds.get_mut(name).unwrap().used = used;
    // The scenes the shots draw count as drawn by this call; then room
    // made for the picture targets the shots will make.
    for shot in shots {
        if let Some(scene) = store.scenes.get_mut(&(name.to_string(), shot.clip)) {
            scene.used = used;
        }
    }
    let sizes: Vec<(u32, u32)> = shots.iter().map(|s| (s.width, s.height)).collect();
    let extra = renderer.new_targets_bytes(gpu, &sizes, supersample, store.counting.new_whole());
    store.shed(
        used,
        false,
        &Room {
            gpu,
            renderer,
            extra,
        },
    );
    gpu.wait()?;
    let (done_tx, done_rx) = mpsc::channel::<(usize, Result<(Vec<u8>, f64, f64), String>)>();
    let mut timings: Vec<Value> = Vec::new();
    let mut started = Vec::new();
    for (i, shot) in shots.iter().enumerate() {
        if cancelled() {
            return Err(Failure::Cancelled);
        }
        let t0 = crate::now_ms();
        started.push(t0);
        let key = (name.to_string(), shot.clip);
        let mut scene_ms = 0.0;
        if !store.scenes.contains_key(&key) {
            // The world out of the store while its scene is made (room is
            // made in the store as it grows), then back.
            let kept = store.worlds.remove(name).expect("the world is kept");
            let kept_bytes = kept.bytes();
            let made = make_scene(assets, gpu, &kept, shot.clip, &mut |growing| {
                let extra = kept_bytes + growing.counted(store.counting);
                let room = Room {
                    gpu,
                    renderer,
                    extra,
                };
                if store.shed(used, false, &room) {
                    let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
                }
                store.give_back_over(&room);
                between(store);
            });
            store.worlds.insert(name.to_string(), kept);
            let (scene, times) = made?;
            scene_ms = times.total;
            store.put_scene(key.clone(), scene);
        }
        let used = store.tick();
        store.scenes.get_mut(&key).unwrap().used = used;
        let scene = &store.scenes[&key];
        let (rgba, times) = draw_shot(assets, gpu, renderer, scene, shot, supersample)?;
        timings.push(json!({ "width": shot.width, "height": shot.height, "timings": { "scene": crate::round2(scene_ms), "draw": crate::round2(times.draw_ms), "readback": crate::round2(times.readback_ms) } }));
        let grid = shot.grid.clone();
        let (w, h) = (shot.width, shot.height);
        let tx = done_tx.clone();
        let font = font.clone();
        rayon::spawn(move || {
            let t = crate::now_ms();
            let rgb = picture::finish(rgba, w, h, grid.as_ref(), &font);
            let grid_ms = crate::now_ms() - t;
            let t = crate::now_ms();
            let encoded = match quality {
                Some(q) => picture::jpeg(&rgb, w, h, q),
                None => picture::png(&rgb, w, h),
            };
            let _ = tx.send((
                i,
                encoded.map(|bytes| (bytes, grid_ms, crate::now_ms() - t)),
            ));
        });
        between(store);
    }
    drop(done_tx);
    let mut pictures: Vec<Option<Vec<u8>>> = vec![None; shots.len()];
    for (i, result) in done_rx {
        let (bytes, grid_ms, encode_ms) = result?;
        timings[i]["bytes"] = json!(bytes.len());
        timings[i]["timings"]["grid"] = json!(crate::round2(grid_ms));
        timings[i]["timings"]["encode"] = json!(crate::round2(encode_ms));
        timings[i]["timings"]["total"] = json!(crate::round2(crate::now_ms() - started[i]));
        pictures[i] = Some(bytes);
    }
    Ok((
        timings,
        pictures
            .into_iter()
            .map(|p| p.expect("every shot encoded"))
            .collect(),
        used,
    ))
}
