// The mesher (`current`) against a reference build of it (`reference`, see
// scripts/mesher-parity.js), on real worlds:
//   mcbench-renderer-parity <pack> <worlds folder> [random patches per world] [seed]
// Each world is a folder holding `mask` (min x, max x, min z, max z) and its
// steps, taken in name order: `*.blueprint` a world to load (as the viewer
// loads it, against the world before), `*.patch` a patch to apply. Then come
// random patches over its blocks: lights, solid blocks, glass, water, slabs
// and holes, and blocks past the bounds (above, below, beside, in chunk
// columns not built) that later patches take away again, so the world grows
// and shrinks; now and then a fresh load of the patched blocks.
//
// The reference only loads: a patch is, to it, a fresh load of the blocks
// after the patch (its world before compared with that), which is what the
// current mesher's patched world must draw. Loads must answer alike (tiles
// and changed tiles). After a patch the current world must have the fresh
// load's tiles, and the viewer's tiles (each the bytes it was last drawn
// with, drawn again only when a load or a patch reports it) must be the
// fresh load's bytes: checked for the tiles either reports changed, and for
// every tile at each load and every tenth step. The current mesher draws its
// indexed tiles from snapshots (World::snapshot, as the viewer's drawing
// workers do), the reference from its world; both must match unindexed and
// indexed.

use std::collections::{BTreeMap, HashMap};
use std::time::{Duration, Instant};

pub type Tiles = Vec<(i32, i32)>;

macro_rules! mesher {
    ($name:ident, $krate:ident) => {
        mod $name {
            use super::Tiles;
            pub use $krate::world::World;
            use $krate::{mesh, pack::Pack, prbm, tile::Tile, world::Mask};

            pub struct Side {
                pub pack: Pack,
                pub world: Option<World>,
                pub tile: Tile,
            }

            impl Side {
                pub fn new(pack: &[u8]) -> Side {
                    Side { pack: Pack::read(pack).unwrap(), world: None, tile: Tile::default() }
                }

                // As lib.rs's load with `compare`: the world's tiles and those
                // that can draw differently from the world before.
                pub fn load(&mut self, bytes: &[u8], [min_x, max_x, min_z, max_z]: [i32; 4]) -> Result<(Tiles, Tiles), String> {
                    let world = World::read(&self.pack, bytes, Mask { min_x, max_x, min_z, max_z })?;
                    let tiles = mesh::tiles(&world);
                    let changed = match &self.world {
                        Some(before) => mesh::changed(before, &world),
                        None => tiles.clone(),
                    };
                    self.world = Some(world);
                    Ok((tiles, changed))
                }

                pub fn tiles(&self) -> Tiles {
                    mesh::tiles(self.world.as_ref().unwrap())
                }

                // Tile x, z unindexed and indexed, from the world.
                pub fn draw(&mut self, x: i32, z: i32) -> (Vec<u8>, Vec<u8>) {
                    mesh::tile(&self.pack, self.world.as_ref().unwrap(), x, z, &mut self.tile);
                    let (mut plain, mut indexed) = (Vec::new(), Vec::new());
                    prbm::write(&self.tile, &mut plain);
                    prbm::write_indexed(&self.tile, &mut indexed);
                    (plain, indexed)
                }
            }
        }
    };
}

mesher!(current_side, current);
mesher!(reference_side, reference);

impl current_side::Side {
    // Applies a patch: the tiles that can draw differently, and the columns relit.
    fn patch(&mut self, bytes: &[u8]) -> Result<(Tiles, usize), String> {
        let world = self.world.as_mut().unwrap();
        let changed = world.patch(&self.pack, bytes)?;
        Ok((changed, world.relit))
    }

    // Tile x, z unindexed from the world, and indexed from its snapshot
    // drawn by a world of its own, as the viewer draws it.
    fn draw_shown(&mut self, x: i32, z: i32) -> (Vec<u8>, Vec<u8>) {
        let (plain, _) = self.draw(x, z);
        let mut bytes = Vec::new();
        self.world.as_ref().unwrap().snapshot(x, z, &mut bytes);
        let (world, tx, tz) = current_side::World::from_snapshot(&bytes).unwrap();
        assert_eq!((tx, tz), (x, z));
        current::mesh::tile(&self.pack, &world, x, z, &mut self.tile);
        let mut indexed = Vec::new();
        current::prbm::write_indexed(&self.tile, &mut indexed);
        (plain, indexed)
    }
}

// The blocks of a world as blueprints and patches give them, to make
// patches and blueprints of.
#[derive(Clone)]
struct Model {
    palette: Vec<(String, Vec<(String, String)>)>,
    blocks: HashMap<[i32; 3], u16>,
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn int(&mut self) -> i32 {
        let value = i32::from_be_bytes(self.bytes[self.at..self.at + 4].try_into().unwrap());
        self.at += 4;
        value
    }
    fn short(&mut self) -> u16 {
        let value = u16::from_be_bytes(self.bytes[self.at..self.at + 2].try_into().unwrap());
        self.at += 2;
        value
    }
    fn utf(&mut self) -> String {
        let n = self.short() as usize;
        let text = String::from_utf8(self.bytes[self.at..self.at + n].to_vec()).unwrap();
        self.at += n;
        text
    }
    fn entry(&mut self) -> (String, Vec<(String, String)>) {
        let id = self.utf();
        let properties = (0..self.int()).map(|_| (self.utf(), self.utf())).collect();
        (id, properties)
    }
}

fn put_int(out: &mut Vec<u8>, value: i32) {
    out.extend_from_slice(&value.to_be_bytes());
}

fn put_utf(out: &mut Vec<u8>, text: &str) {
    out.extend_from_slice(&(text.len() as u16).to_be_bytes());
    out.extend_from_slice(text.as_bytes());
}

fn put_entry(out: &mut Vec<u8>, (id, properties): &(String, Vec<(String, String)>)) {
    put_utf(out, id);
    put_int(out, properties.len() as i32);
    for (key, value) in properties {
        put_utf(out, key);
        put_utf(out, value);
    }
}

impl Model {
    fn read(bytes: &[u8]) -> Model {
        let mut input = Reader { bytes, at: 0 };
        let palette = (0..input.int()).map(|_| input.entry()).collect();
        for _ in 0..6 {
            input.int();
        }
        let mut blocks = HashMap::new();
        for _ in 0..input.int() {
            let (sx, sy, sz) = (input.int(), input.int(), input.int());
            for i in 0..4096 {
                let value = input.short();
                if value != 0 {
                    blocks.insert([sx * 16 + (i & 15), sy * 16 + (i >> 8), sz * 16 + (i >> 4 & 15)], value);
                }
            }
        }
        Model { palette, blocks }
    }

    // A patch's blocks; checks its bounds and built columns are the blocks'.
    fn apply(&mut self, bytes: &[u8]) -> Result<(), String> {
        let mut input = Reader { bytes, at: 0 };
        for _ in 0..input.int() {
            let entry = input.entry();
            self.palette.push(entry);
        }
        let bounds: Vec<i32> = (0..6).map(|_| input.int()).collect();
        let columns = input.int();
        for _ in 0..input.int() {
            let at = [input.int(), input.int(), input.int()];
            let value = input.short();
            if value == 0 {
                self.blocks.remove(&at);
            } else {
                self.blocks.insert(at, value);
            }
        }
        if bounds != self.bounds() || columns as usize != self.columns() {
            return Err(format!("the patch says bounds {bounds:?} and {columns} columns, its blocks {:?} and {}", self.bounds(), self.columns()));
        }
        Ok(())
    }

    fn bounds(&self) -> [i32; 6] {
        if self.blocks.is_empty() {
            return [0; 6];
        }
        let mut bounds = [i32::MAX, i32::MAX, i32::MAX, i32::MIN, i32::MIN, i32::MIN];
        for at in self.blocks.keys() {
            for axis in 0..3 {
                bounds[axis] = bounds[axis].min(at[axis]);
                bounds[axis + 3] = bounds[axis + 3].max(at[axis]);
            }
        }
        bounds
    }

    fn columns(&self) -> usize {
        let mut columns: Vec<(i32, i32)> = self.blocks.keys().map(|at| (at[0] >> 4, at[2] >> 4)).collect();
        columns.sort();
        columns.dedup();
        columns.len()
    }

    fn blueprint(&self) -> Vec<u8> {
        let mut out = Vec::new();
        put_int(&mut out, self.palette.len() as i32);
        for entry in &self.palette {
            put_entry(&mut out, entry);
        }
        for value in self.bounds() {
            put_int(&mut out, value);
        }
        let mut sections: HashMap<[i32; 3], Vec<u16>> = HashMap::new();
        for (at, &value) in &self.blocks {
            sections.entry([at[0] >> 4, at[1] >> 4, at[2] >> 4]).or_insert_with(|| vec![0; 4096])[((at[1] & 15) << 8 | (at[2] & 15) << 4 | (at[0] & 15)) as usize] = value;
        }
        let mut sections: Vec<_> = sections.into_iter().collect();
        sections.sort();
        put_int(&mut out, sections.len() as i32);
        for (at, values) in sections {
            for value in at {
                put_int(&mut out, value);
            }
            for value in values {
                out.extend_from_slice(&value.to_be_bytes());
            }
        }
        out
    }
}

struct Random(u64);

impl Random {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0 >> 11
    }
    fn below(&mut self, n: u64) -> i32 {
        (self.next() % n) as i32
    }
}

// Blocks a random patch places besides the world's own: lights, blocks that
// stop light or shape it, and blocks it passes.
const NAMES: [&str; 11] = [
    "minecraft:glowstone",
    "minecraft:torch",
    "minecraft:stone",
    "minecraft:glass",
    "minecraft:water",
    "minecraft:oak_slab",
    "minecraft:oak_leaves",
    "minecraft:lantern",
    "minecraft:sea_lantern",
    "minecraft:redstone_lamp",
    "minecraft:cave_air",
];

// One of NAMES at random, as the model's palette entry.
fn name(model: &mut Model, random: &mut Random) -> u16 {
    let name = NAMES[random.below(NAMES.len() as u64) as usize];
    match model.palette.iter().position(|(id, properties)| id == name && properties.is_empty()) {
        Some(index) if index > 0 => index as u16,
        _ => {
            model.palette.push((name.to_string(), Vec::new()));
            (model.palette.len() - 1) as u16
        }
    }
}

// A random patch over `model` (which it applies to the model): its bytes.
// `grown` keeps the blocks it placed past the bounds, which a later patch
// takes away again.
fn random_patch(model: &mut Model, anchors: &[[i32; 3]], random: &mut Random, step: usize, grown: &mut Vec<[i32; 3]>) -> Vec<u8> {
    let bounds = model.bounds();
    let before = model.palette.len();
    let mut edits = Vec::new();
    let count = 1 + random.below(if step % 5 == 0 { 80 } else { 6 });
    let anchor = anchors[random.below(anchors.len() as u64) as usize];
    let scatter = random.next() % 3 == 0;
    for _ in 0..count {
        let base = if scatter { anchors[random.below(anchors.len() as u64) as usize] } else { anchor };
        let mut at = [base[0] + random.below(7) - 3, base[1] + random.below(7) - 3, base[2] + random.below(7) - 3];
        for axis in 0..3 {
            at[axis] = at[axis].clamp(bounds[axis], bounds[axis + 3]);
        }
        let value = match random.next() % 10 {
            0..=2 => 0,
            3..=6 if before > 1 => 1 + random.below(before as u64 - 1) as u16,
            _ => name(model, random),
        };
        edits.push((at, value));
    }
    // Past the bounds every third patch (above, below as water falls,
    // beside, into the next chunk columns), and away again in the next.
    if step % 3 == 1 {
        let out = match random.below(5) {
            0 => [anchor[0], (bounds[4] + 1 + random.below(30)).min(959), anchor[2]],
            1 => [anchor[0], (bounds[1] - 1 - random.below(60)).max(-64), anchor[2]],
            2 => [bounds[3] + 1 + random.below(40), anchor[1], anchor[2]],
            3 => [anchor[0], anchor[1], bounds[2] - 1 - random.below(40)],
            _ => [bounds[0] - 17 - random.below(30), anchor[1], bounds[5] + 17 + random.below(30)],
        };
        edits.push((out, name(model, random)));
        grown.push(out);
    } else if step % 3 == 2 {
        for at in grown.drain(..) {
            edits.push((at, 0));
        }
    }
    // Entries that change nothing.
    for _ in 0..random.below(3) {
        let at = anchors[random.below(anchors.len() as u64) as usize];
        edits.push((at, model.blocks.get(&at).copied().unwrap_or(0)));
    }
    for &(at, value) in &edits {
        if value == 0 {
            model.blocks.remove(&at);
        } else {
            model.blocks.insert(at, value);
        }
    }
    let mut out = Vec::new();
    put_int(&mut out, (model.palette.len() - before) as i32);
    for entry in &model.palette[before..] {
        put_entry(&mut out, entry);
    }
    for value in model.bounds() {
        put_int(&mut out, value);
    }
    put_int(&mut out, model.columns() as i32);
    put_int(&mut out, edits.len() as i32);
    for (at, value) in edits {
        for v in at {
            put_int(&mut out, v);
        }
        out.extend_from_slice(&value.to_be_bytes());
    }
    out
}

#[derive(Default)]
struct Tally {
    loads: usize,
    patches: usize,
    moved: usize,
    tiles: usize,
    bytes: usize,
    differences: usize,
    current_load: Duration,
    reference_load: Duration,
    current_draw: Duration,
    reference_draw: Duration,
    current_patch: Duration,
}

struct Pair {
    current: current_side::Side,
    reference: reference_side::Side,
    mask: [i32; 4],
    // The viewer's tiles: each as the current mesher last drew it.
    shown: BTreeMap<(i32, i32), (Vec<u8>, Vec<u8>)>,
    tally: Tally,
    name: String,
}

impl Pair {
    fn differ(&mut self, what: String) {
        self.tally.differences += 1;
        if self.tally.differences <= 20 {
            eprintln!("{}: {what}", self.name);
        }
    }

    // Draws `tiles` with the current mesher, as the viewer does, keeping them shown.
    fn redraw(&mut self, tiles: &[(i32, i32)]) {
        for &(x, z) in tiles {
            let started = Instant::now();
            let drawn = self.current.draw_shown(x, z);
            self.tally.current_draw += started.elapsed();
            self.shown.insert((x, z), drawn);
        }
    }

    // The shown `tiles` against the reference's, which must be the world's
    // tiles, byte for byte.
    fn check(&mut self, tiles: &[(i32, i32)], what: &str) {
        for &(x, z) in tiles {
            let started = Instant::now();
            let expected = self.reference.draw(x, z);
            self.tally.reference_draw += started.elapsed();
            self.tally.tiles += 1;
            self.tally.bytes += expected.0.len() + expected.1.len();
            if self.shown.get(&(x, z)) != Some(&expected) {
                self.differ(format!("{what}: tile {x},{z} differs ({:?} and {} bytes)", self.shown.get(&(x, z)).map(|(plain, _)| plain.len()), expected.0.len()));
            }
        }
    }

    // Loads a world in both; true when it loaded.
    fn load(&mut self, bytes: &[u8]) -> bool {
        let started = Instant::now();
        let current = self.current.load(bytes, self.mask);
        self.tally.current_load += started.elapsed();
        let started = Instant::now();
        let reference = self.reference.load(bytes, self.mask);
        self.tally.reference_load += started.elapsed();
        self.tally.loads += 1;
        if current != reference {
            self.differ(format!("load {}: {:?} against {:?}", self.tally.loads, current.as_ref().map(|(t, c)| (t.len(), c.len())), reference.as_ref().map(|(t, c)| (t.len(), c.len()))));
        }
        let Ok((tiles, changed)) = current else {
            return false;
        };
        assert!(changed.iter().all(|tile| tiles.contains(tile)));
        self.shown.retain(|tile, _| tiles.contains(tile));
        self.redraw(&changed);
        self.check(&tiles, &format!("load {}", self.tally.loads));
        true
    }

    // Applies a patch to the current mesher and loads the blocks after it
    // (`after`) in the reference.
    fn patch(&mut self, bytes: &[u8], after: &[u8], full: bool) -> Result<(), String> {
        let started = Instant::now();
        let current = self.current.patch(bytes);
        self.tally.current_patch += started.elapsed();
        self.tally.patches += 1;
        let (changed, _) = current?;
        let started = Instant::now();
        let (tiles, reported) = self.reference.load(after, self.mask).unwrap();
        self.tally.reference_load += started.elapsed();
        let what = format!("patch {}", self.tally.patches);
        if self.current.tiles() != tiles {
            self.differ(format!("{what}: the tiles differ from a fresh load's ({} and {})", self.current.tiles().len(), tiles.len()));
        }
        self.shown.retain(|tile, _| tiles.contains(tile));
        self.redraw(&changed);
        let mut checked = if full { tiles.clone() } else { changed.iter().chain(&reported).copied().filter(|tile| tiles.contains(tile)).collect() };
        checked.sort();
        checked.dedup();
        self.check(&checked, &what);
        Ok(())
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let pack = std::fs::read(&args[1]).unwrap();
    let randoms: usize = args.get(3).map_or(40, |n| n.parse().unwrap());
    let seed: u64 = args.get(4).map_or(1, |n| n.parse().unwrap());
    let mut worlds: Vec<_> = std::fs::read_dir(&args[2]).unwrap().map(|entry| entry.unwrap().path()).filter(|path| path.is_dir()).collect();
    worlds.sort();
    let mut total = Tally::default();
    for folder in worlds {
        let name = folder.file_name().unwrap().to_string_lossy().to_string();
        let mask: Vec<i32> = std::fs::read_to_string(folder.join("mask")).unwrap().split_whitespace().map(|n| n.parse().unwrap()).collect();
        let mut pair = Pair {
            current: current_side::Side::new(&pack),
            reference: reference_side::Side::new(&pack),
            mask: [mask[0], mask[1], mask[2], mask[3]],
            shown: BTreeMap::new(),
            tally: Tally::default(),
            name: name.clone(),
        };
        let mut steps: Vec<_> = std::fs::read_dir(&folder).unwrap().map(|entry| entry.unwrap().path()).filter(|path| path.extension().is_some_and(|e| e == "blueprint" || e == "patch")).collect();
        steps.sort();
        let mut model: Option<Model> = None;
        for (n, step) in steps.iter().enumerate() {
            let bytes = std::fs::read(step).unwrap();
            if step.extension().unwrap() == "blueprint" {
                if pair.load(&bytes) {
                    model = Some(Model::read(&bytes));
                }
                continue;
            }
            let model = model.as_mut().unwrap();
            let (bounds, columns) = (model.bounds(), model.columns());
            if let Err(error) = model.apply(&bytes) {
                pair.differ(format!("{}: {error}", step.display()));
            }
            pair.tally.moved += (model.bounds() != bounds || model.columns() != columns) as usize;
            if let Err(error) = pair.patch(&bytes, &model.blueprint(), n % 10 == 9) {
                pair.differ(format!("{}: the patch failed: {error}", step.display()));
            }
        }
        let Some(mut model) = model else {
            println!("{name}: no world loaded");
            continue;
        };
        let mut random = Random(seed.wrapping_mul(0x9e3779b97f4a7c15) ^ name.bytes().fold(7u64, |h, b| h.wrapping_mul(31).wrapping_add(b as u64)) | 1);
        let mut anchors: Vec<[i32; 3]> = model.blocks.keys().copied().collect();
        anchors.sort();
        let budget = if anchors.len() > 2_000_000 { randoms.min(8) } else { randoms };
        let mut grown = Vec::new();
        for step in 0..budget {
            if anchors.is_empty() {
                break;
            }
            let (bounds, columns) = (model.bounds(), model.columns());
            let bytes = random_patch(&mut model, &anchors, &mut random, step, &mut grown);
            pair.tally.moved += (model.bounds() != bounds || model.columns() != columns) as usize;
            if let Err(error) = pair.patch(&bytes, &model.blueprint(), step % 10 == 9) {
                panic!("{name}: a random patch failed: {error}");
            }
            if step % 10 == 9 && model.blocks.len() < 2_000_000 {
                pair.load(&model.blueprint());
            }
            anchors.extend(model.blocks.keys().take(4).copied());
        }
        let tiles = pair.reference.tiles();
        pair.check(&tiles, "the end");
        let t = &pair.tally;
        println!(
            "{name}: {} loads, {} patches ({} moved the bounds or columns), {} tiles ({} bytes), {} differences; load {:.0}/{:.0} ms, draw {:.0}/{:.0} ms, patch {:.0} ms (current/reference)",
            t.loads,
            t.patches,
            t.moved,
            t.tiles,
            t.bytes,
            t.differences,
            t.current_load.as_secs_f64() * 1e3,
            t.reference_load.as_secs_f64() * 1e3,
            t.current_draw.as_secs_f64() * 1e3,
            t.reference_draw.as_secs_f64() * 1e3,
            t.current_patch.as_secs_f64() * 1e3,
        );
        total.loads += t.loads;
        total.patches += t.patches;
        total.moved += t.moved;
        total.tiles += t.tiles;
        total.bytes += t.bytes;
        total.differences += t.differences;
        total.current_load += t.current_load;
        total.reference_load += t.reference_load;
        total.current_draw += t.current_draw;
        total.reference_draw += t.reference_draw;
        total.current_patch += t.current_patch;
    }
    let t = &total;
    println!(
        "total: {} loads, {} patches ({} moved the bounds or columns), {} tiles ({} bytes) compared, {} differences; load {:.0}/{:.0} ms, draw {:.0}/{:.0} ms, patch {:.0} ms (current/reference)",
        t.loads,
        t.patches,
        t.moved,
        t.tiles,
        t.bytes,
        t.differences,
        t.current_load.as_secs_f64() * 1e3,
        t.reference_load.as_secs_f64() * 1e3,
        t.current_draw.as_secs_f64() * 1e3,
        t.reference_draw.as_secs_f64() * 1e3,
        t.current_patch.as_secs_f64() * 1e3,
    );
    std::process::exit(if t.differences == 0 { 0 } else { 1 });
}
