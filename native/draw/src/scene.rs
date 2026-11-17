// The block quads a picture draws: the mesher's (renderer/, BlueMap's tile
// mesher with Minecraft's culling, models and light, used as a library and
// its tiles meshed in parallel), shaded with see_build's equations
// (each block's shade recomputed from the mesher's world: corner occlusion
// by the three blocks around a corner, smooth light, a face's light from
// the cell it faces, 1.15 for emitters, a block entity renderer's quads in
// their block's light unshaded).
//
// `Quad` is the canonical form the comparisons read; `Instance` is what the
// GPU draws, one per quad (gpu.rs draws each as two triangles, 0-1-2 and
// 0-2-3, as deepslate indexes them).

use crate::assets::{Atlas, Facts};
use mcbench_renderer::mesh;
use mcbench_renderer::pack::{self, Pack};
use mcbench_renderer::tile::{self, Tile};
use mcbench_renderer::world::World;
use rayon::prelude::*;

pub const AO_CURVE: [f32; 4] = [1.0, 0.78, 0.58, 0.42];

pub fn light_curve(level: f32) -> f32 {
    0.25 + 0.75 * (level / 15.0).powf(1.4)
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Quad {
    // Corners in see_build's world (the page's heights: Minecraft's less
    // the offset).
    pub corners: [[f32; 3]; 4],
    pub normal: [f32; 3],
    // Index into the scene's texture names.
    pub texture: u32,
    // Texture coordinates over the texture (0..1).
    pub uv: [[f32; 2]; 4],
    pub tint: [f32; 3],
    // The brightness each corner's colour is the tint times.
    pub shade: [f32; 4],
    pub transparent: bool,
    // tile::MODEL, FLUID or ENTITY.
    pub kind: u8,
    // The drawing block, at the page's heights.
    pub block: [i32; 3],
    pub emissive: bool,
}

// One quad as the GPU draws it (gpu.rs vertex layout): its corners, their
// texture coordinates over the atlas square, the brightness of each corner
// halved (so 1.15 fits a unit range), the tint, and an overlay (OverlayTexture:
// RGB and the share of the colour kept; blocks keep all of it).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Instance {
    pub corners: [[f32; 3]; 4],
    pub uv: [[f32; 2]; 4],
    pub shade: [u16; 4],
    pub tint: [u16; 4],
    pub overlay: [u8; 4],
}

pub const INSTANCE_BYTES: usize = std::mem::size_of::<Instance>();

fn unorm16(v: f32) -> u16 {
    (v.clamp(0.0, 1.0) * 65535.0).round() as u16
}

impl Instance {
    pub fn new(corners: [[f32; 3]; 4], uv: [[f32; 2]; 4], shade: [f32; 4], tint: [f32; 3], overlay: [f32; 4]) -> Instance {
        Instance {
            corners,
            uv,
            shade: shade.map(|s| unorm16(s / 2.0)),
            tint: [unorm16(tint[0]), unorm16(tint[1]), unorm16(tint[2]), 0],
            overlay: overlay.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8),
        }
    }
}

// Quad.normal of deepslate: (v1 - v0) x (v2 - v0), normalized.
pub fn normal_of(c: &[[f32; 3]; 4]) -> [f32; 3] {
    let e1 = [c[1][0] - c[0][0], c[1][1] - c[0][1], c[1][2] - c[0][2]];
    let e2 = [c[2][0] - c[0][0], c[2][1] - c[0][1], c[2][2] - c[0][2]];
    let n = [e1[1] * e2[2] - e1[2] * e2[1], e1[2] * e2[0] - e1[0] * e2[2], e1[0] * e2[1] - e1[1] * e2[0]];
    let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    if l == 0.0 {
        n
    } else {
        n.map(|v| v / l)
    }
}

// What drawing a world reads besides the world.
pub struct Draw<'a> {
    pub pack: &'a Pack,
    pub facts: &'a [Facts],
    pub y_offset: i32,
}

impl Draw<'_> {
    // The light see_build reads at a cell of the page's heights: the
    // brighter of the mesher's sky and block light there.
    pub fn cell_light(&self, world: &World, [x, y, z]: [i32; 3]) -> i32 {
        let (sky, block) = world.light([x, y + self.y_offset, z]);
        sky.max(block)
    }

    // A tile's quads (face pairs 2k, 2k + 1 as Tile::quad wrote them).
    fn tile_quads(&self, world: &World, tile: &Tile, tx: i32, tz: i32, out: &mut Vec<Quad>) {
        let (ox, oz) = (tx * 32 + 2, tz * 32 + 2);
        let yo = self.y_offset as f32;
        for q in 0..tile.size / 2 {
            let (f0, f1) = (2 * q, 2 * q + 1);
            let p = |face: usize, c: usize| {
                let at = face * 9 + c * 3;
                [tile.position[at] + ox as f32, tile.position[at + 1] - yo, tile.position[at + 2] + oz as f32]
            };
            let corners = [p(f0, 0), p(f0, 1), p(f0, 2), p(f1, 2)];
            let uv = |face: usize, c: usize| [tile.uv[face * 6 + c * 2], tile.uv[face * 6 + c * 2 + 1]];
            let source = tile.source[f0];
            let emissive = self.pack.states[source.state as usize].emission > 0;
            let normal = normal_of(&corners);
            let entity_level = tile.sunlight[f0].max(tile.blocklight[f0]) as f32;
            let shade = self.shade(world, &corners, normal, source, emissive, entity_level);
            let f = self.facts[source.state as usize];
            let c = &tile.color[f0 * 3..f0 * 3 + 3];
            out.push(Quad {
                corners,
                normal,
                texture: tile.material[f0] as u32,
                uv: [uv(f0, 0), uv(f0, 1), uv(f0, 2), uv(f1, 2)],
                tint: [c[0], c[1], c[2]],
                shade,
                transparent: if source.kind == tile::FLUID { f.fluid_blended } else { f.model_blended },
                kind: source.kind,
                block: [source.at[0], source.at[1] - self.y_offset, source.at[2]],
                emissive,
            });
        }
    }

    // emitBlock's shading over the mesher's world: a face's light from the
    // cell it radiates into, and for an axis-aligned face on a cell
    // boundary, per corner the occlusion by the three blocks around it
    // (both sides shading: 3) and the light averaged over the face's cell
    // and those of the three that are not six full faces. What a block
    // entity renderer draws takes its block's light (the mesher's face
    // light: its own cell's, the brighter of a double block's halves)
    // unshaded.
    fn shade(&self, world: &World, c: &[[f32; 3]; 4], n: [f32; 3], source: tile::Source, emissive: bool, entity_level: f32) -> [f32; 4] {
        if source.kind == tile::ENTITY {
            return [if emissive { 1.15 } else { light_curve(entity_level) }; 4];
        }
        let pack = self.pack;
        let cell_light = |p: [i32; 3]| self.cell_light(world, p) as f32;
        let block_at = |p: [i32; 3]| {
            let state = world.block([p[0], p[1] + self.y_offset, p[2]]);
            if pack.states[state as usize].has(pack::AIR) {
                None
            } else {
                Some(state)
            }
        };
        let center = [0, 1, 2].map(|i| (c[0][i] + c[1][i] + c[2][i] + c[3][i]) / 4.0);
        let f = [0, 1, 2].map(|i| (center[i] + n[i] * 0.5).floor() as i32);
        let face_level = cell_light(f);
        let face_light = if emissive { 1.15 } else { light_curve(face_level) };
        let axis = (0..3).find(|&i| n[i].abs() > 0.99).unwrap_or(3);
        let on_boundary = axis < 3 && (center[axis] - center[axis].round()).abs() < 1e-3;
        if !on_boundary {
            return [face_light; 4];
        }
        let a1 = if axis == 0 { 1 } else { 0 };
        let a2 = if axis == 2 { 1 } else { 2 };
        let full = |b: Option<u32>| b.is_some_and(|s| self.facts[s as usize].full);
        let shades = |b: Option<u32>| b.is_some_and(|s| pack.states[s as usize].has(pack::SHADES));
        let mut out = [0.0; 4];
        for (i, v) in c.iter().enumerate() {
            let s1 = if v[a1] > f[a1] as f32 + 0.5 { 1 } else { -1 };
            let s2 = if v[a2] > f[a2] as f32 + 0.5 { 1 } else { -1 };
            let (mut c1, mut c2) = (f, f);
            c1[a1] += s1;
            c2[a2] += s2;
            let mut cc = c1;
            cc[a2] += s2;
            let (b1, b2, bc) = (block_at(c1), block_at(c2), block_at(cc));
            let occ = if shades(b1) && shades(b2) { 3 } else { shades(b1) as usize + shades(b2) as usize + shades(bc) as usize };
            out[i] = if emissive {
                1.15
            } else {
                let (mut sum, mut count) = (face_level, 1.0);
                for (b, at) in [(b1, c1), (b2, c2), (bc, cc)] {
                    if !full(b) {
                        sum += cell_light(at);
                        count += 1.0;
                    }
                }
                AO_CURVE[occ] * light_curve(sum / count)
            };
        }
        out
    }

    // Every tile of the world meshed, in parallel, into quads, in tile order.
    pub fn quads(&self, world: &World) -> Vec<Quad> {
        let tiles = mesh::tiles(world);
        let per_tile: Vec<Vec<Quad>> = tiles
            .par_iter()
            .map_init(Tile::default, |tile, &(x, z)| {
                mesh::tile(self.pack, world, x, z, tile);
                let mut out = Vec::with_capacity(tile.size / 2);
                self.tile_quads(world, tile, x, z, &mut out);
                out
            })
            .collect();
        per_tile.into_iter().flatten().collect()
    }

    // One tile's quads as instances: the opaque ones in the order meshed,
    // the blended ones with the Minecraft section they are drawn with and
    // their block's place in it (BlendedKey). A texture the
    // atlas lacks is listed and its quads left out.
    pub fn tile(&self, world: &World, boxes: &[Option<[u32; 4]>], side: u32, (x, z): (i32, i32), tile: &mut Tile) -> TileOut {
        mesh::tile(self.pack, world, x, z, tile);
        let mut quads = Vec::with_capacity(tile.size / 2);
        self.tile_quads(world, tile, x, z, &mut quads);
        let mut out = TileOut::default();
        for q in &quads {
            match boxes.get(q.texture as usize).copied().flatten() {
                Some(b) => {
                    let instance = block_instance(q, b, side);
                    if q.transparent {
                        let mc = [q.block[0], q.block[1] + self.y_offset, q.block[2]];
                        // Its section, and its block's place in the order
                        // SectionCompiler.compile meshes a section's blocks
                        // (BlockPos.betweenClosed: x fastest, then y, then z).
                        let local = mc.map(|v| v.rem_euclid(16) as u16);
                        out.blended.push(((mc.map(|v| v.div_euclid(16)), local[0] + 16 * local[1] + 256 * local[2]), instance));
                    } else {
                        out.opaque.push(instance);
                    }
                }
                None => out.missing.push(q.texture),
            }
        }
        out
    }

    // The given tiles meshed in parallel, a thread a tile, each handed to
    // `consume` in the order given, at most two tiles a thread meshed ahead
    // of the one consumed (what waits on the CPU stays bounded). Stops at
    // the first error `consume` answers.
    pub fn tiles_in_order<E: Send>(&self, world: &World, names: &[String], atlas: &Atlas, tiles: &[(i32, i32)], consume: &mut dyn FnMut((i32, i32), TileOut) -> Result<(), E>) -> Result<(), E> {
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
        use std::sync::{mpsc, Condvar, Mutex};
        let boxes: Vec<Option<[u32; 4]>> = names.iter().map(|name| atlas.boxes.get(name).copied()).collect();
        let threads = rayon::current_num_threads().min(tiles.len()).max(1);
        let ahead = 2 * threads;
        let next = AtomicUsize::new(0);
        let stop = AtomicBool::new(false);
        // How many tiles have been consumed, for the workers to wait on.
        let consumed = (Mutex::new(0usize), Condvar::new());
        let (tx, rx) = mpsc::channel::<(usize, TileOut)>();
        std::thread::scope(|scope| {
            for _ in 0..threads {
                let tx = tx.clone();
                let (next, stop, consumed, boxes) = (&next, &stop, &consumed, &boxes);
                scope.spawn(move || {
                    let mut tile = Tile::default();
                    loop {
                        let i = next.fetch_add(1, Ordering::SeqCst);
                        if i >= tiles.len() {
                            return;
                        }
                        {
                            let mut done = consumed.0.lock().unwrap();
                            while i >= *done + ahead && !stop.load(Ordering::SeqCst) {
                                done = consumed.1.wait(done).unwrap();
                            }
                        }
                        if stop.load(Ordering::SeqCst) {
                            return;
                        }
                        let out = self.tile(world, boxes, atlas.side, tiles[i], &mut tile);
                        if tx.send((i, out)).is_err() {
                            return;
                        }
                    }
                });
            }
            drop(tx);
            let mut waiting: std::collections::BTreeMap<usize, TileOut> = Default::default();
            let mut want = 0;
            let mut result = Ok(());
            for (i, out) in rx.iter() {
                waiting.insert(i, out);
                while let Some(out) = waiting.remove(&want) {
                    if result.is_ok() {
                        result = consume(tiles[want], out);
                    }
                    want += 1;
                    *consumed.0.lock().unwrap() = want;
                    consumed.1.notify_all();
                }
                if result.is_err() {
                    stop.store(true, Ordering::SeqCst);
                    consumed.1.notify_all();
                }
            }
            result
        })
    }

    // The given tiles meshed in parallel, in the order given.
    pub fn tiles(&self, world: &World, names: &[String], atlas: &Atlas, tiles: &[(i32, i32)]) -> Vec<TileOut> {
        let boxes: Vec<Option<[u32; 4]>> = names.iter().map(|name| atlas.boxes.get(name).copied()).collect();
        tiles.par_iter().map_init(Tile::default, |tile, &at| self.tile(world, &boxes, atlas.side, at, tile)).collect()
    }
}

// A blended quad's Minecraft section and its block's index in the order
// the section's blocks are meshed.
pub type BlendedKey = ([i32; 3], u16);

// A tile's instances.
#[derive(Default, Clone, PartialEq, Debug)]
pub struct TileOut {
    pub opaque: Vec<Instance>,
    pub blended: Vec<(BlendedKey, Instance)>,
    pub missing: Vec<u32>,
}

pub fn block_instance(q: &Quad, b: [u32; 4], side: u32) -> Instance {
    let size = side as f32;
    let at = |t: f32, axis: usize| (b[axis] as f32 + t * (b[axis + 2] - b[axis]) as f32) / size;
    Instance::new(q.corners, q.uv.map(|t| [at(t[0], 0), at(t[1], 1)]), q.shade, q.tint, [1.0; 4])
}

