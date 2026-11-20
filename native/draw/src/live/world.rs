// The world the live view shows: a kept world (Renderer::world) pinned in
// the store (store.rs Store::pinned), so it is never let go while shown,
// does not count against `worldsKept`, and stays a world like any other: a
// base for the next revision's upload, drawn by `shots`. The engine's tick
// patches (`Ticked.changes`, the live contract's patch) are applied with the
// mesher's World::patch on rayon's threads, in order, and the tiles the
// patch answers are meshed there too; the render thread then writes only
// those tiles' quads into the scene's buffers (`install`). The world's
// blueprint with every patch applied (`Current`) is what the store keeps
// for it once a request reads it, so an upload with it as base is diffed
// against the world as it is, not as it was placed.

use crate::assets::Assets;
use crate::gpu::{DeviceError, Gpu};
use crate::scene::{Draw, Instance, TileOut, INSTANCE_BYTES};
use crate::store::{self, Lit, Scene, Translucent};
use bytemuck::Zeroable;
use mcbench_render::live::ViewKey;
use mcbench_render::Clip;
use std::collections::{BTreeMap, BTreeSet};

// A box's lowest and highest corners.
pub type Bounds = ([i32; 3], [i32; 3]);

pub struct LiveWorld {
    // The kept world's name, the view it is, and its revision and tick.
    pub name: String,
    pub view: ViewKey,
    pub revision: u64,
    pub tick: u64,
    // The box framing fits, Minecraft's coordinates (Shown::bounds, then
    // each patch's).
    pub bounds: Clip,
    // Its blueprint with every patch applied, and whether the store's copy
    // is behind it. None while a batch of patches is on rayon's threads.
    pub current: Option<Current>,
    pub stale: bool,
}

// A blueprint (the engine's McbenchEngine.write format: palette entries,
// bounds, sections of 4096 palette indices) held open to take patches.
pub struct Current {
    // Each palette entry's bytes as the blueprint writes it.
    entries: Vec<Vec<u8>>,
    bounds: [i32; 6],
    sections: BTreeMap<[i32; 3], Vec<u16>>,
}

// A palette entry: its bytes, its id and its properties.
type Entry = (Vec<u8>, String, Vec<(String, String)>);

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
    what: &'a str,
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], String> {
        let b = self
            .bytes
            .get(self.at..self.at + n)
            .ok_or_else(|| format!("The {} ends early", self.what))?;
        self.at += n;
        Ok(b)
    }
    fn int(&mut self) -> Result<i32, String> {
        let b = self.take(4)?;
        Ok(i32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn short(&mut self) -> Result<u16, String> {
        let b = self.take(2)?;
        Ok(u16::from_be_bytes([b[0], b[1]]))
    }
    fn utf(&mut self) -> Result<String, String> {
        let n = self.short()? as usize;
        let what = self.what;
        String::from_utf8(self.take(n)?.to_vec())
            .map_err(|_| format!("The {what} has a name that is not UTF-8"))
    }
    // A palette entry: its bytes, its id and properties.
    fn entry(&mut self) -> Result<Entry, String> {
        let start = self.at;
        let id = self.utf()?;
        let mut properties = Vec::new();
        for _ in 0..self.int()?.max(0) {
            properties.push((self.utf()?, self.utf()?));
        }
        Ok((self.bytes[start..self.at].to_vec(), id, properties))
    }
}

impl Current {
    pub fn read(bytes: &[u8]) -> Result<Current, String> {
        let mut r = Reader {
            bytes,
            at: 0,
            what: "blueprint",
        };
        let mut entries = Vec::new();
        for _ in 0..r.int()?.max(0) {
            entries.push(r.entry()?.0);
        }
        let mut bounds = [0; 6];
        for v in bounds.iter_mut() {
            *v = r.int()?;
        }
        let mut sections = BTreeMap::new();
        for _ in 0..r.int()?.max(0) {
            let at = [r.int()?, r.int()?, r.int()?];
            let cells = r
                .take(8192)?
                .as_chunks::<2>()
                .0
                .iter()
                .map(|c| u16::from_be_bytes(*c))
                .collect();
            sections.insert(at, cells);
        }
        if r.at != bytes.len() {
            return Err("The blueprint has trailing bytes".into());
        }
        Ok(Current {
            entries,
            bounds,
            sections,
        })
    }

    // A patch (its indices the stream's, which are this blueprint's): the
    // entries it adds, its bounds, its blocks.
    pub fn apply(&mut self, bytes: &[u8]) -> Result<(), String> {
        let mut r = Reader {
            bytes,
            at: 0,
            what: "patch",
        };
        for _ in 0..r.int()?.max(0) {
            let entry = r.entry()?.0;
            self.entries.push(entry);
        }
        for v in self.bounds.iter_mut() {
            *v = r.int()?;
        }
        let _columns = r.int()?;
        for _ in 0..r.int()?.max(0) {
            let (x, y, z) = (r.int()?, r.int()?, r.int()?);
            let value = r.short()?;
            let cells = self
                .sections
                .entry([x >> 4, y >> 4, z >> 4])
                .or_insert_with(|| vec![0; 4096]);
            cells[((y & 15) << 8 | (z & 15) << 4 | (x & 15)) as usize] = value;
        }
        Ok(())
    }

    // The blueprint's bytes; sections left all air are not written.
    pub fn bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&(self.entries.len() as i32).to_be_bytes());
        for e in &self.entries {
            out.extend_from_slice(e);
        }
        for v in self.bounds {
            out.extend_from_slice(&v.to_be_bytes());
        }
        let held: Vec<(&[i32; 3], &Vec<u16>)> = self
            .sections
            .iter()
            .filter(|(_, c)| c.iter().any(|&v| v != 0))
            .collect();
        out.extend_from_slice(&(held.len() as i32).to_be_bytes());
        for (at, cells) in held {
            for v in at {
                out.extend_from_slice(&v.to_be_bytes());
            }
            for v in cells.iter() {
                out.extend_from_slice(&v.to_be_bytes());
            }
        }
        out
    }
}

// What a batch of patches made, on rayon's threads: the world's tiles after
// it, the tiles meshed again with their quads, the blocks' bounds
// (Minecraft's heights), the batch's last revision and tick and its last
// entities, and how long World::patch and the meshing took.
pub struct Patched {
    pub tiles: Vec<(i32, i32)>,
    pub meshed: Vec<((i32, i32), TileOut)>,
    pub bounds: Bounds,
    pub revision: u64,
    pub tick: u64,
    pub entities: Vec<u8>,
    pub apply_ms: f64,
    pub mesh_ms: f64,
}

// A patch waiting: its revision, tick, changes and entities.
pub type Waiting = (u64, u64, Vec<u8>, Vec<u8>);

// The batch's meshing written into the scene, on the render thread: a
// tile's quads in place when they fit its room (what is left of the room
// zeroed: degenerate quads draw nothing), else in a buffer of its own; a
// tile gone zeroed. A tile's translucent quads replace its own
// (Translucent::replace_tile); nothing else is uploaded.
pub fn install(
    scene: &mut Scene,
    gpu: &Gpu,
    assets: &Assets,
    y_offset: i32,
    patched: Patched,
) -> Result<(), DeviceError> {
    let translucent = scene.translucent.get_or_insert_with(|| {
        Translucent::per_tile(gpu, &[], &Default::default(), y_offset, assets.level)
    });
    let now: BTreeSet<(i32, i32)> = patched.tiles.iter().copied().collect();
    for at in scene.tiles.iter().filter(|t| !now.contains(t)) {
        if let Some((b, first, _)) = scene.arena.ranges.remove(at) {
            let room = scene.arena.rooms.remove(at).expect("every tile has a room");
            zero(gpu, &scene.arena.batches.buffers[b].0, first, room);
        }
        if scene
            .geometry
            .remove(at)
            .is_some_and(|g| !g.blended.is_empty())
        {
            translucent.replace_tile(gpu, *at, &[]);
        }
    }
    for (at, mut out) in patched.meshed {
        let had = scene
            .geometry
            .get(&at)
            .is_some_and(|g| !g.blended.is_empty());
        let quads = std::mem::take(&mut out.opaque);
        let geom = store::tile_geom(&mut out, &assets.names);
        if had || !geom.blended.is_empty() {
            translucent.replace_tile(gpu, at, &geom.blended);
        }
        scene.geometry.insert(at, geom);
        let n = quads.len() as u32;
        match (
            scene.arena.ranges.get(&at).copied(),
            scene.arena.rooms.get(&at).copied(),
        ) {
            (Some((b, first, _)), Some(room)) if n <= room => {
                let buffer = &scene.arena.batches.buffers[b].0;
                if n > 0 {
                    gpu.queue.write_buffer(
                        buffer,
                        first as u64 * INSTANCE_BYTES as u64,
                        bytemuck::cast_slice(&quads),
                    );
                }
                zero(gpu, buffer, first + n, room - n);
                scene.arena.ranges.insert(at, (b, first, n));
            }
            (old, room) => {
                if let (Some((b, first, _)), Some(room)) = (old, room) {
                    zero(gpu, &scene.arena.batches.buffers[b].0, first, room);
                }
                // Room for the tile to grow by half again.
                let room = n + n / 2 + 16;
                let size = room as u64 * INSTANCE_BYTES as u64;
                let buffer = crate::gpu::quads_buffer(gpu, size, "live tile", &mut |write| {
                    if n > 0 {
                        write(0, bytemuck::cast_slice(&quads));
                    }
                });
                scene.arena.batches.buffers.push((buffer, room));
                scene.arena.batches.bytes += size;
                scene
                    .arena
                    .ranges
                    .insert(at, (scene.arena.batches.buffers.len() - 1, 0, n));
                scene.arena.rooms.insert(at, room);
            }
        }
    }
    scene.tiles = patched.tiles;
    let (lo, hi) = patched.bounds;
    scene.bounds = (
        [lo[0], lo[1] - y_offset, lo[2]],
        [hi[0], hi[1] - y_offset, hi[2]],
    );
    Ok(())
}

// `count` quads from `first` of a buffer written as zeros: four corners at
// one point, a quad that draws nothing.
fn zero(gpu: &Gpu, buffer: &wgpu::Buffer, first: u32, count: u32) {
    if count > 0 {
        let zeros = vec![Instance::zeroed(); count as usize];
        gpu.queue.write_buffer(
            buffer,
            first as u64 * INSTANCE_BYTES as u64,
            bytemuck::cast_slice(&zeros),
        );
    }
}

// One patch (the engine's bytes, read as World::patch reads them) on the
// shown world: its palette indices are the stream's, the blueprint's and
// then those the patches add; the world's palette maps the blueprint's
// through `lit.map` (an edit of a base keeps the base's indices) and takes
// each added entry at its end. Answers the patch in the world's indices
// and the bounds it carries.
pub fn translate(
    bytes: &[u8],
    lit: &mut Lit,
    pack: &mcbench_renderer::pack::Pack,
) -> Result<(Vec<u8>, Bounds), String> {
    let mut r = Reader {
        bytes,
        at: 0,
        what: "patch",
    };
    let world_len = lit.states.len();
    let stream_len = lit.map.len();
    let mut states = Vec::new();
    for _ in 0..r.int()?.max(0) {
        let (_, id, properties) = r.entry()?;
        states.push(pack.state(&id, &properties)?);
    }
    let mut bounds = [0i32; 6];
    for v in bounds.iter_mut() {
        *v = r.int()?;
    }
    let _columns = r.int()?;
    let count = r.int()?;
    let mut out = bytes.to_vec();
    for _ in 0..count.max(0) {
        r.take(12)?;
        let at = r.at;
        let value = r.short()? as usize;
        let world = if value < stream_len {
            lit.map[value] as usize
        } else if value < stream_len + states.len() {
            world_len + value - stream_len
        } else {
            return Err("The patch has a block outside its palette or bounds".into());
        };
        let world = u16::try_from(world).map_err(|_| "The world's palette is full".to_string())?;
        out[at..at + 2].copy_from_slice(&world.to_be_bytes());
    }
    if r.at != bytes.len() {
        return Err("The patch has trailing bytes".into());
    }
    for (i, state) in states.into_iter().enumerate() {
        lit.map.push((world_len + i) as u16);
        lit.states.push(state);
    }
    Ok((
        out,
        (
            [bounds[0], bounds[1], bounds[2]],
            [bounds[3], bounds[4], bounds[5]],
        ),
    ))
}

// The bounds a patch carries (its blocks' after it, Minecraft's
// coordinates), read past the palette entries it adds.
pub fn patch_bounds(bytes: &[u8]) -> Result<Bounds, String> {
    let mut r = Reader {
        bytes,
        at: 0,
        what: "patch",
    };
    for _ in 0..r.int()?.max(0) {
        r.entry()?;
    }
    let mut b = [0i32; 6];
    for v in b.iter_mut() {
        *v = r.int()?;
    }
    Ok(([b[0], b[1], b[2]], [b[3], b[4], b[5]]))
}

// Patches applied in order to the lit world and to its blueprint, and the
// tiles they answer meshed (rayon's threads). A patch the world refuses
// changes nothing and fails the batch with its sentence; the patches
// before it stay applied.
pub fn apply(
    lit: &mut Lit,
    current: &mut Current,
    assets: &Assets,
    y_offset: i32,
    patches: &[Waiting],
) -> Result<Patched, String> {
    let mut changed: BTreeSet<(i32, i32)> = BTreeSet::new();
    let mut bounds = ([0; 3], [0; 3]);
    let t = crate::now_ms();
    for (_, _, bytes, _) in patches {
        // The map and states are put back if the world refuses the patch.
        let (map, states) = (lit.map.len(), lit.states.len());
        let (translated, b) = translate(bytes, lit, &assets.pack)?;
        match lit.world.patch(&assets.pack, &translated) {
            Ok(tiles) => changed.extend(tiles),
            Err(e) => {
                lit.map.truncate(map);
                lit.states.truncate(states);
                return Err(format!(
                    "The live world could not take a tick's changes: {e}"
                ));
            }
        }
        current.apply(bytes)?;
        bounds = b;
    }
    let apply_ms = crate::now_ms() - t;
    let t = crate::now_ms();
    let tiles = mcbench_renderer::mesh::tiles(&lit.world);
    let changed: Vec<(i32, i32)> = changed
        .into_iter()
        .filter(|at| tiles.binary_search(at).is_ok())
        .collect();
    let draw = Draw {
        pack: &assets.pack,
        facts: &assets.facts,
        y_offset,
    };
    let outs = draw.tiles(&lit.world, &assets.names, &assets.atlas, &changed);
    let (revision, tick, _, entities) = patches.last().expect("a batch holds a patch");
    Ok(Patched {
        tiles,
        meshed: changed.into_iter().zip(outs).collect(),
        bounds,
        revision: *revision,
        tick: *tick,
        entities: entities.clone(),
        apply_ms,
        mesh_ms: crate::now_ms() - t,
    })
}

// The shown world's scene made again from its lit world on a new device (a
// lost device's recreate): nothing is read from the core.
pub fn remesh(
    old: &Scene,
    lit: &Lit,
    entities: &[serde_json::Value],
    y_offset: i32,
    assets: &Assets,
    gpu: &Gpu,
) -> Result<Scene, crate::gpu::Failure> {
    let (lo, hi) = old.bounds;
    let bounds = (
        [lo[0], lo[1] + y_offset, lo[2]],
        [hi[0], hi[1] + y_offset, hi[2]],
    );
    let mut times = store::SceneTimes::default();
    let scene = store::mesh_world(
        assets,
        gpu,
        &lit.world,
        y_offset,
        entities,
        None,
        bounds,
        old.blocks_count,
        &mut |_| {},
        &mut times,
    )?;
    Ok(scene)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_blueprint_reads_takes_a_patch_and_writes_back() {
        // Palette air and one entry; one section with a block.
        let mut b = Vec::new();
        b.extend_from_slice(&2i32.to_be_bytes());
        for id in ["minecraft:air", "minecraft:stone"] {
            b.extend_from_slice(&(id.len() as u16).to_be_bytes());
            b.extend_from_slice(id.as_bytes());
            b.extend_from_slice(&0i32.to_be_bytes());
        }
        for v in [0i32, 0, 0, 0, 0, 0] {
            b.extend_from_slice(&v.to_be_bytes());
        }
        b.extend_from_slice(&1i32.to_be_bytes());
        for v in [0i32, 0, 0] {
            b.extend_from_slice(&v.to_be_bytes());
        }
        let mut cells = vec![0u8; 8192];
        cells[1] = 1;
        b.extend_from_slice(&cells);
        let mut c = Current::read(&b).unwrap();
        assert_eq!(c.bytes(), b);
        // A patch removing the block and placing it at x 17 (another section).
        let mut p = Vec::new();
        p.extend_from_slice(&0i32.to_be_bytes());
        for v in [17i32, 0, 0, 17, 0, 0] {
            p.extend_from_slice(&v.to_be_bytes());
        }
        p.extend_from_slice(&1i32.to_be_bytes());
        p.extend_from_slice(&2i32.to_be_bytes());
        for (x, v) in [(0i32, 0u16), (17, 1)] {
            for w in [x, 0, 0] {
                p.extend_from_slice(&w.to_be_bytes());
            }
            p.extend_from_slice(&v.to_be_bytes());
        }
        c.apply(&p).unwrap();
        let again = Current::read(&c.bytes()).unwrap();
        assert_eq!(
            again.sections.len(),
            1,
            "the emptied section is not written"
        );
        assert_eq!(again.sections[&[1, 0, 0]][1], 1);
        assert_eq!(again.bounds, [17, 0, 0, 17, 0, 0]);
    }
}
