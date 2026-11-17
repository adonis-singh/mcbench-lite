// The worlds a helper keeps and the scenes made of them (PROTOCOL.md, What
// is kept), and drawing shots of them.

use crate::assets::Assets;
use crate::blueprint;
use crate::camera::Camera;
use crate::entities::{self, BlockQuad, View};
use crate::gpu::{Batches, DeviceError, Draws, Failure, Gpu, Ordered, Renderer};
use crate::picture::Grid;
use crate::scene::{Draw, Instance};
use mcbench_renderer::world::{Mask, World};
use serde::Deserialize;
use serde_json::Value;
use std::cell::RefCell;
use std::collections::HashMap;

const EVERYWHERE: Mask = Mask { min_x: -30_000_000, max_x: 30_000_000, min_z: -30_000_000, max_z: 30_000_000 };

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

// A tile's geometry: its opaque instances' place in the scene's arena (in
// tile order, so the opaque quads draw in few calls: software rasterisers
// pay for every draw call over the whole target), its blended ones kept to
// be ordered with every tile's, and a digest of both (what the tests
// compare against a fresh load).
pub struct TileGeom {
    pub opaque_count: usize,
    pub blended: Vec<(crate::scene::BlendedKey, Instance)>,
    pub missing: Vec<String>,
    pub digest: u64,
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
        self.arena.bytes() + self.translucent.as_ref().map_or(0, Translucent::bytes) + self.solid.as_ref().map_or(0, |s| s.bytes)
    }

    pub fn quads(&self) -> usize {
        self.geometry.values().map(|g| g.opaque_count + g.blended.len()).sum()
    }

    pub fn missing(&self) -> Vec<String> {
        let mut m: Vec<String> = self.geometry.values().flat_map(|g| g.missing.iter().cloned()).collect();
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
        self.tiles.iter().map(|t| (*t, self.geometry[t].digest)).collect()
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
}

struct Section {
    // Its position (Minecraft's section coordinates).
    at: [i32; 3],
    buffer: usize,
    // Its first quad's index in its buffer, and in `centroids`.
    first: u32,
    start: usize,
    count: u32,
}

// Minecraft's default render distance in chunks (Options.renderDistance).
const RENDER_DISTANCE: i32 = 12;

impl Translucent {
    fn new(gpu: &Gpu, tiles: &[(i32, i32)], geometry: &HashMap<(i32, i32), TileGeom>, y_offset: i32, level: crate::assets::Level) -> Result<Option<Translucent>, DeviceError> {
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
                        let origin = [key.0[0] * 16, key.0[1] * 16 - y_offset, key.0[2] * 16].map(|v| v as f32);
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
                sections.push(Section { at: key, buffer, first: counts[buffer], start: quads.len(), count: n });
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
        let order = gpu.device.create_buffer(&wgpu::BufferDescriptor { label: Some("translucent order"), size: (quads.len() * 4) as u64, usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        Ok(Some(Translucent { batches: Batches { buffers, bytes }, order, sections, centroids, y_offset, level }))
    }

    pub fn bytes(&self) -> u64 {
        self.batches.bytes + self.order.size() + (self.centroids.len() * 12 + self.sections.len() * std::mem::size_of::<Section>()) as u64
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
        let visits = octree_visits(&self.sections.iter().map(|s| s.at).collect::<Vec<_>>(), camera, self.level);
        let mut drawn: Vec<usize> = (0..self.sections.len()).collect();
        drawn.sort_unstable_by(|&a, &b| visits[b].cmp(&visits[a]));
        let parts: Vec<Vec<u32>> = drawn
            .par_iter()
            .map(|&i| {
                let s = &self.sections[i];
                let origin = [s.at[0] * 16, s.at[1] * 16, s.at[2] * 16];
                let local = [0, 1, 2].map(|k| (camera[k] - origin[k] as f64) as f32);
                sort_quads(local, &self.centroids[s.start..s.start + s.count as usize]).into_iter().map(|k| s.first + k).collect()
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
        gpu.queue.write_buffer(&self.order, 0, bytemuck::cast_slice(&order));
        Ordered { batches: &self.batches, order: &self.order, runs }
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
    let mut quads: Vec<(f32, u32)> = centroids.iter().enumerate().map(|(k, c)| (d2(c), k as u32)).collect();
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
pub fn octree_visits(sections: &[[i32; 3]], camera: [f64; 3], level: crate::assets::Level) -> Vec<u64> {
    let cam_section = camera.map(|v| (v.floor() as i32).div_euclid(16));
    let reach = sections.iter().map(|s| (s[0] - cam_section[0]).abs().max((s[2] - cam_section[2]).abs())).max().unwrap_or(0);
    let distance = RENDER_DISTANCE.max(reach);
    let size = ((2 * distance + 1) as u32).next_power_of_two() as i32;
    let origin = cam_section.map(|v| v * 16);
    let centre_of_camera = origin.map(|v| v + 8);
    let reach_blocks = distance * 16;
    let min = [origin[0] - reach_blocks, if size >= level.height / 16 { level.min_y } else { origin[1] - reach_blocks }, origin[2] - reach_blocks];
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
                let index: u64 = (0..3).filter(|&k| below[k] != (diff[k] < 0)).map(|k| shifts[k]).sum();
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
        if y > z { [4, 2, 1] } else { [4, 1, 2] }
    } else if y > x && y > z {
        if x > z { [2, 4, 1] } else { [1, 4, 2] }
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

fn tile_geom(out: &mut crate::scene::TileOut, names: &[String]) -> TileGeom {
    let digest = digest(out);
    TileGeom { opaque_count: out.opaque.len(), missing: out.missing.iter().map(|&m| names.get(m as usize).cloned().unwrap_or_else(|| format!("material {m}"))).collect(), blended: std::mem::take(&mut out.blended), digest }
}

// Every tile's opaque instances in buffers of at most BATCH bytes, tile
// after tile in tile order; where each tile's are.
pub struct Arena {
    pub batches: Batches,
    // By tile: buffer, first instance, count.
    pub ranges: HashMap<(i32, i32), (usize, u32, u32)>,
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
type EditOf<'b> = (Arena, &'b [(i32, i32)], &'b HashMap<(i32, i32), crate::scene::TileOut>);

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
                (arena.batches.buffers.into_iter().map(|(b, _)| Some(b)).collect(), last)
            }
        };
        ArenaBuilder { gpu, per_buffer: Batches::per_buffer(gpu) as u32, pending: Vec::new(), count: 0, buffers: Vec::new(), ranges: HashMap::new(), old, old_last, pushed: 0 }
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
                let buffer = self.gpu.device.create_buffer(&wgpu::BufferDescriptor { label: Some("blocks"), size: bytes, usage: crate::gpu::QUADS, mapped_at_creation: false });
                let mut at = 0u64;
                for (_, part) in pending {
                    if let Part::Meshed(v) = part {
                        if !v.is_empty() {
                            self.gpu.queue.write_buffer(&buffer, at * size as u64, bytemuck::cast_slice(v));
                        }
                    }
                    at += part.count() as u64;
                }
                let mut encoder = self.gpu.device.create_command_encoder(&Default::default());
                for (ob, of, to, n) in copies {
                    let from = self.old[ob].as_ref().expect("an old buffer is kept until its tiles are copied");
                    encoder.copy_buffer_to_buffer(from, of as u64 * size as u64, &buffer, to as u64 * size as u64, n as u64 * size as u64);
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

    // The last buffer, waited for with the scene's other uploads.
    fn finish(mut self) -> Result<Arena, DeviceError> {
        self.flush(false)?;
        let bytes = self.buffers.iter().map(|(_, c)| *c as u64 * crate::scene::INSTANCE_BYTES as u64).sum();
        Ok(Arena { batches: Batches { buffers: std::mem::take(&mut self.buffers), bytes }, ranges: std::mem::take(&mut self.ranges) })
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

    fn block_quads(&self, state: &str) -> Result<Vec<BlockQuad>, String> {
        if let Some(q) = self.lone.borrow().get(state) {
            return Ok(q.clone());
        }
        let quads = entities::lone_quads(self.draw.pack, self.names, state)?;
        self.lone.borrow_mut().insert(state.to_string(), quads.clone());
        Ok(quads)
    }
}

pub fn entity_instances(placed: &[entities::Placed], assets: &Assets) -> Result<Vec<Instance>, String> {
    if placed.is_empty() {
        return Ok(Vec::new());
    }
    let atlas = &assets.entity()?.atlas;
    placed.iter().map(|p| entities::instance(p, atlas)).collect()
}

// The entities of a scene (those inside a clip), placed and lit in the
// world.
fn scene_entities(assets: &Assets, draw: &Draw, world: &World, kept: &Kept, clip: Option<Clip>) -> Result<(Vec<Value>, entities::Sorted, Vec<Instance>), String> {
    // shotEntities: those whose position is in the box.
    let shown: Vec<Value> = match clip {
        None => kept.entities.clone(),
        Some(c) => kept
            .entities
            .iter()
            .filter(|e| (0..3).all(|i| e["pos"][i].as_f64().is_some_and(|v| v >= c.min[i] as f64 && v < c.max[i] as f64 + 1.0)))
            .cloned()
            .collect(),
    };
    let view = WorldView { draw, world, names: &assets.names, lone: RefCell::new(HashMap::new()) };
    let sorted = if shown.is_empty() { entities::Sorted::default() } else { entities::build(&assets.entity()?.capture, &shown, &view)? };
    let solid = entity_instances(&sorted.solid, assets)?;
    Ok((shown, sorted, solid))
}

// Meshes, lights and uploads a world, or the part of it inside `clip`: a
// group of tiles at a time, their opaque quads let go on the CPU once in a
// buffer. `room` is called after each buffer is made (the store makes room
// for the scene as it grows: PROTOCOL.md, What is kept).
pub fn make_scene(assets: &Assets, gpu: &Gpu, kept: &Kept, clip: Option<Clip>, room: &mut dyn FnMut()) -> Result<(Scene, SceneTimes), Failure> {
    let t0 = crate::now_ms();
    let mut times = SceneTimes::default();
    let y = kept.y_offset;
    let lifted = clip.map(|c| ([c.min[0], c.min[1] + y, c.min[2]], [c.max[0], c.max[1] + y, c.max[2]]));
    let clipped = blueprint::clip(&kept.blueprint, lifted)?;
    let (lo, hi) = clipped.bounds.ok_or("Nothing to draw: the world holds no blocks there")?;
    let world = World::read(&assets.pack, &clipped.bytes, EVERYWHERE)?;
    drop(clipped.bytes);
    times.read = crate::now_ms() - t0;
    let t = crate::now_ms();
    let draw = Draw { pack: &assets.pack, facts: &assets.facts, y_offset: y };
    let tiles = mcbench_renderer::mesh::tiles(&world);
    // Meshing and the upload of its opaque quads, group by group.
    let mut geometry: HashMap<(i32, i32), TileGeom> = HashMap::new();
    let mut builder = ArenaBuilder::new(gpu, None);
    let mut upload = 0.0;
    // What waits on the CPU: the tiles being meshed and a few ahead
    // (Draw::tiles_in_order), and at most a buffer of quads.
    draw.tiles_in_order(&world, &assets.names, &assets.atlas, &tiles, &mut |at, mut out| -> Result<(), Failure> {
        let u = crate::now_ms();
        geometry.insert(at, tile_geom(&mut out, &assets.names));
        if builder.push(at, Part::Meshed(out.opaque))? {
            room();
        }
        upload += crate::now_ms() - u;
        Ok(())
    })?;
    let u = crate::now_ms();
    let arena = builder.finish()?;
    upload += crate::now_ms() - u;
    times.mesh = crate::now_ms() - t - upload;
    let t = crate::now_ms();
    let (shown, sorted, solid) = scene_entities(assets, &draw, &world, kept, clip)?;
    times.entities = crate::now_ms() - t;
    let t = crate::now_ms();
    let translucent = Translucent::new(gpu, &tiles, &geometry, y, assets.level)?;
    let solid_batches = (!solid.is_empty()).then(|| Batches::upload(gpu, &solid, "entities"));
    // The buffers' contents go to the GPU with the next submission: this one.
    gpu.queue.submit([]);
    gpu.wait()?;
    times.upload = upload + crate::now_ms() - t;
    let lit = match clip {
        Some(_) => None,
        None => {
            let states = blueprint::palette_states(&assets.pack, &kept.blueprint)?;
            let map = (0..states.len() as u16).collect();
            Some(Lit { world, states, map })
        }
    };
    times.total = crate::now_ms() - t0;
    let entity_quads = sorted.solid.len() + sorted.blended.len() + sorted.facing.len();
    Ok((Scene { tiles, geometry, arena, translucent, solid: solid_batches, sorted, entities: shown, bounds: ([lo[0], lo[1] - y, lo[2]], [hi[0], hi[1] - y, hi[2]]), entity_quads, blocks_count: clipped.blocks, used: 0, lit }, times))
}

// The whole world's scene of `kept`, made from the scene of the world it
// edits (`base`, whose lit world it takes): the cells that differ applied
// through the mesher's patch, only the tiles it names meshed and uploaded
// again. Answers the scene and how many tiles were meshed.
pub fn patch_scene(assets: &Assets, gpu: &Gpu, mut base: Scene, base_blueprint: &[u8], kept: &Kept, room: &mut dyn FnMut()) -> Result<(Scene, SceneTimes, usize), Failure> {
    let t0 = crate::now_ms();
    let mut times = SceneTimes::default();
    let y = kept.y_offset;
    let mut lit = base.lit.take().ok_or("The base scene keeps no lit world")?;
    let edit = blueprint::edit(&assets.pack, &lit.states, &lit.map, &kept.blueprint, base_blueprint)?;
    let (lo, hi) = edit.bounds.ok_or("Nothing to draw: the world holds no blocks")?;
    let changed = lit.world.patch(&assets.pack, &edit.patch)?;
    // The tiles it names that the world still has (the others are gone).
    let tiles = mcbench_renderer::mesh::tiles(&lit.world);
    let changed: Vec<(i32, i32)> = changed.into_iter().filter(|at| tiles.binary_search(at).is_ok()).collect();
    lit.states.extend(edit.added_states);
    lit.map = edit.map;
    times.read = crate::now_ms() - t0;
    let t = crate::now_ms();
    let draw = Draw { pack: &assets.pack, facts: &assets.facts, y_offset: y };
    let outs = draw.tiles(&lit.world, &assets.names, &assets.atlas, &changed);
    times.mesh = crate::now_ms() - t;
    let t = crate::now_ms();
    let (shown, sorted, solid) = scene_entities(assets, &draw, &lit.world, kept, None)?;
    times.entities = crate::now_ms() - t;
    let t = crate::now_ms();
    let mut geometry = std::mem::take(&mut base.geometry);
    geometry.retain(|at, _| tiles.binary_search(at).is_ok());
    let mut outs: HashMap<(i32, i32), crate::scene::TileOut> = changed.iter().copied().zip(outs).collect();
    for (at, out) in outs.iter_mut() {
        geometry.insert(*at, tile_geom(out, &assets.names));
    }
    // The arena again, buffer by buffer: the tiles kept copied on the GPU
    // from the base's (each of its buffers let go once copied), the tiles
    // meshed again written.
    let old = std::mem::replace(&mut base.arena, Arena { batches: Batches { buffers: Vec::new(), bytes: 0 }, ranges: HashMap::new() });
    let ranges = old.ranges.clone();
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
            room();
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
    Ok((Scene { tiles, geometry, arena, translucent, solid: solid_batches, sorted, entities: shown, bounds: ([lo[0], lo[1] - y, lo[2]], [hi[0], hi[1] - y, hi[2]]), entity_quads, blocks_count: edit.blocks, used: 0, lit: Some(lit) }, times, meshed))
}


// One shot of a scene: its picture read back, before the grid and JPEG.
pub fn draw_shot(assets: &Assets, gpu: &Gpu, renderer: &Renderer, scene: &Scene, shot: &Shot, supersample: u32) -> Result<(Vec<u8>, crate::gpu::PictureTimes), Failure> {
    let draws = prepare(assets, gpu, renderer, scene, shot, supersample)?;
    renderer.picture(gpu, &shot.camera, scene.bounds, shot.width, shot.height, supersample, &draws).map_err(|DeviceError(e)| Failure::Device(e))
}

// The same shot, its parts submitted one at a time and timed (Renderer::phases).
pub fn draw_shot_phases(assets: &Assets, gpu: &Gpu, renderer: &Renderer, scene: &Scene, shot: &Shot, supersample: u32) -> Result<(Vec<u8>, crate::gpu::Phases), Failure> {
    let draws = prepare(assets, gpu, renderer, scene, shot, supersample)?;
    renderer.phases(gpu, &shot.camera, scene.bounds, shot.width, shot.height, supersample, &draws).map_err(|DeviceError(e)| Failure::Device(e))
}

fn prepare<'a>(assets: &Assets, gpu: &Gpu, renderer: &Renderer, scene: &'a Scene, shot: &Shot, supersample: u32) -> Result<Draws<'a>, Failure> {
    let side = gpu.device.limits().max_texture_dimension_2d;
    if shot.width == 0 || shot.height == 0 || shot.width.max(shot.height) * supersample > side {
        return Err(Failure::Other(format!("A shot of {}x{} at supersample {supersample} needs textures larger than this adapter's {side}", shot.width, shot.height)));
    }
    shot.camera.check()?;
    let (bw, bh) = (shot.width * supersample, shot.height * supersample);
    let frame = shot.camera.frame(bw as f64 / bh as f64);
    let behind = if scene.entities.is_empty() { Vec::new() } else { entity_instances(&entities::behind(&assets.entity()?.capture, &scene.entities, &scene.sorted, shot.camera.yaw, shot.camera.pitch, frame.eye)?, assets)? };
    if !scene.entities.is_empty() {
        renderer.entity_atlas(gpu, &assets.entity()?.atlas);
    }
    Ok(Draws { blocks: scene.block_batches(), translucent: scene.translucent.as_ref().map(|t| t.ordered(gpu, frame.eye)), solid: scene.solid.as_ref(), behind: (!behind.is_empty()).then(|| Batches::upload(gpu, &behind, "behind")) })
}

// What the helper keeps, and the bound on it (PROTOCOL.md, What is kept).
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub worlds: usize,
    pub scenes: usize,
    // The process's memory (memory::resident) the helper keeps itself
    // within between calls: the server's `memoryLimit`.
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
            Limits { worlds: 2, scenes: 3, bytes: bytes.unwrap_or(DEFAULT_SOFTWARE_BYTES) }
        } else {
            Limits { worlds: 4, scenes: 6, bytes: bytes.unwrap_or(DEFAULT_BYTES) }
        }
    }
}

// What the bound is measured with, and the one thing it lets go that the
// store does not hold.
pub trait Bound {
    // Whether the process holds more than `limit` bytes, measured after the
    // device has freed what was dropped and freed memory is given back.
    fn over(&mut self, limit: u64) -> bool;
    // Lets the picture targets of the size drawn longest ago go; false when
    // none are kept.
    fn release_targets(&mut self) -> bool;
}

pub struct Store {
    pub worlds: HashMap<String, Kept>,
    pub scenes: HashMap<(String, Option<Clip>), Scene>,
    clock: u64,
    pub limits: Limits,
}

impl Store {
    pub fn new(limits: Limits) -> Store {
        Store { worlds: HashMap::new(), scenes: HashMap::new(), clock: 0, limits }
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
            for s in self.scenes.values_mut() {
                s.lit = None;
            }
        }
        self.scenes.insert(key.clone(), scene);
        self.evict_counts(&key.0);
    }

    // Least recently used first, scenes then worlds, until the counts are
    // within the limits; the world `keep` and its scenes stay.
    fn evict_counts(&mut self, keep: &str) {
        while self.scenes.len() > self.limits.scenes {
            let Some(oldest) = self.scenes.iter().filter(|(k, _)| k.0 != keep).min_by_key(|(_, s)| s.used).map(|(k, _)| k.clone()) else { break };
            self.scenes.remove(&oldest);
        }
        while self.worlds.len() > self.limits.worlds {
            let Some(oldest) = self.worlds.iter().filter(|(n, _)| n.as_str() != keep).min_by_key(|(_, k)| k.used).map(|(n, _)| n.clone()) else { break };
            self.drop_world(&oldest);
        }
    }

    // Until the process is within the limit, during a call that began at
    // tick `since`: the scenes the call has not drawn, least recently used
    // first; then the picture targets (the size drawn longest ago first:
    // made again by the next picture of that size in tens of
    // milliseconds); after the call (`after`), then the scenes it drew (a
    // world whose scene alone passes the limit is drawn and then let go:
    // its blueprint kept, the next call meshes it again). Worlds themselves
    // (their blueprints and entities, small) are bounded by their count.
    pub fn shed(&mut self, since: u64, after: bool, bound: &mut dyn Bound) {
        let limit = self.limits.bytes;
        while bound.over(limit) {
            if let Some(oldest) = self.scenes.iter().filter(|(_, s)| s.used < since).min_by_key(|(_, s)| s.used).map(|(k, _)| k.clone()) {
                self.scenes.remove(&oldest);
            } else if bound.release_targets() {
            } else if let Some(oldest) = self.scenes.iter().filter(|_| after).min_by_key(|(_, s)| s.used).map(|(k, _)| k.clone()) {
                self.scenes.remove(&oldest);
            } else {
                return;
            }
        }
    }
}
