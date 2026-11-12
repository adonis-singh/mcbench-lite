// BlueprintWorld (scripts/McbenchBlueMapReference.java): a blueprint
// (McbenchEngine.write) as a world, its blocks at their Minecraft
// coordinates and the light Minecraft's rules give them in the void. (What
// BlueMap makes of it through its block cache and render mask is in mesh.rs.)
//
// The world is kept by 16x16x16 section over the lit volume, so its memory
// follows the sections a blueprint has rather than its bounding box: blocks
// only where a section holds one, and light only where it is not what empty
// space gives. The light is the one fixpoint of Minecraft's propagation over
// the whole volume (what a relight of every cell from zero computes), found
// by computing only where it can differ from these rules:
//   - A column's sky light is 15 from the top down to the first block that
//     stops it (its `open` height), and below that only where light spreads.
//   - Block light reaches 14 blocks, so it is zero beyond the sections next
//     to one holding a block (those are `near`).
//   - A section that is not near has no block within 16 of any of its
//     cells, so its sky light is 15 less the distance to the nearest column
//     open to the sky at its height, the same at every height of it.
// Near sections are relit cell by cell, taking the light of every other cell
// as these rules give it.
//
// The lit volume is the bounds and MARGIN past them (16 below), and a
// volume larger than that holds the same light in every cell they share: the
// cells past MARGIN are 15 or more from every block, so a column there is
// open to the sky and holds no block light, and below the blocks every
// height holds the same air, so a cell 16 or more below the lowest block has
// the light of the cell above it. That is how a patch grows or shrinks the
// world (`resize`) without relighting it.

use crate::pack::Pack;
use std::collections::{HashMap, HashSet, VecDeque};
use std::rc::Rc;

const MARGIN: i32 = 15;
// Light of level 15 or less reaches 14 blocks sideways (every step costs
// at least 1 and a level of 0 spreads nothing); a patch relights the
// columns within this many of a change, so the columns beside them are 17
// or more from every change and keep their light.
const REACH: i32 = 16;
// Direction order: down, up, north, south, west, east.
const STEPS: [[i32; 3]; 6] = [[0, -1, 0], [0, 1, 0], [0, 0, -1], [0, 0, 1], [-1, 0, 0], [1, 0, 0]];
const OPPOSITE: [usize; 6] = [1, 0, 3, 2, 5, 4];
const SIDES: [[i32; 2]; 4] = [[-1, 0], [1, 0], [0, -1], [0, 1]];

#[derive(Clone, Copy, PartialEq)]
pub struct Mask {
    pub min_x: i32,
    pub max_x: i32,
    pub min_z: i32,
    pub max_z: i32,
}

impl Mask {
    pub fn contains(&self, x: i32, z: i32) -> bool {
        x >= self.min_x && x <= self.max_x && z >= self.min_z && z <= self.max_z
    }
}

// One kind of light over a section, by cell index (y << 8 | z << 4 | x).
// Only the cells inside the lit volume mean anything.
#[derive(Clone)]
enum Level {
    Uniform(i8),
    // The same at every height: by column (z << 4 | x).
    Columns(Rc<[i8; 256]>),
    Cells(Box<[i8; 4096]>),
}

impl Level {
    #[inline(always)]
    fn get(&self, at: usize) -> i8 {
        match self {
            Level::Uniform(value) => *value,
            Level::Columns(map) => map[at & 255],
            Level::Cells(cells) => cells[at],
        }
    }

    fn cells(&mut self) -> &mut [i8; 4096] {
        if !matches!(self, Level::Cells(_)) {
            let mut cells = Box::new([0i8; 4096]);
            for (at, cell) in cells.iter_mut().enumerate() {
                *cell = self.get(at);
            }
            *self = Level::Cells(cells);
        }
        match self {
            Level::Cells(cells) => cells,
            _ => unreachable!(),
        }
    }

    fn is(&self, value: i8) -> bool {
        matches!(self, Level::Uniform(v) if *v == value)
    }

    fn by_column(&self) -> bool {
        !matches!(self, Level::Cells(_))
    }
}

struct Section {
    blocks: Option<Box<[u16; 4096]>>,
    sky: Level,
    glow: Level,
    // This section or one of the 26 around it holds blocks.
    near: bool,
}

impl Section {
    fn open() -> Section {
        Section { blocks: None, sky: Level::Uniform(15), glow: Level::Uniform(0), near: false }
    }

    // Air in open sky with no block light.
    fn plain(&self) -> bool {
        self.blocks.is_none() && self.sky.is(15) && self.glow.is(0)
    }
}

pub struct World {
    // The blueprint's bounds (min x, y, z, max x, y, z).
    bounds: [i32; 6],
    // The state BlueMap sees per palette entry (entry 0 is always air), and
    // the state Minecraft lights it as (entry 0 as the blueprint names it).
    palette: Vec<u32>,
    light_of: Vec<u32>,
    min_x: i32,
    min_y: i32,
    min_z: i32,
    size_x: i32,
    size_y: i32,
    size_z: i32,
    // The sections over the lit volume, by section x, then z, then y (a
    // chunk column's sections follow one another).
    section_min: [i32; 3],
    section_size: [i32; 3],
    sections: Vec<Section>,
    // Per column of the volume (z-major): the lowest height that sky light
    // reaches straight down from the top (level 15 from there up).
    open: Vec<i32>,
    // Built chunk columns, sorted, each with its 256 column spans (max, min).
    pub columns: Vec<(i32, i32)>,
    spans: Vec<[(i32, i32); 256]>,
    pub mask: Mask,
    // The columns the last patch relit.
    pub relit: usize,
}

struct Input<'a> {
    what: &'static str,
    bytes: &'a [u8],
    at: usize,
    // A read failed for want of bytes.
    early: bool,
}

impl<'a> Input<'a> {
    fn new(what: &'static str, bytes: &'a [u8]) -> Input<'a> {
        Input { what, bytes, at: 0, early: false }
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        if self.at + n > self.bytes.len() {
            self.early = true;
            return Err(format!("The {} ends early", self.what));
        }
        let slice = &self.bytes[self.at..self.at + n];
        self.at += n;
        Ok(slice)
    }
    fn int(&mut self) -> Result<i32, String> {
        let b = self.take(4)?;
        Ok(i32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }
    // DataOutput.writeUTF; Minecraft's names are ASCII, where it is UTF-8.
    fn utf(&mut self) -> Result<String, String> {
        let b = self.take(2)?;
        let n = u16::from_be_bytes([b[0], b[1]]) as usize;
        let text = self.take(n)?;
        if !text.is_ascii() {
            return Err("The blueprint has a name that is not ASCII".into());
        }
        Ok(String::from_utf8(text.to_vec()).unwrap())
    }
}

#[inline(always)]
fn cell(x: i32, y: i32, z: i32) -> usize {
    (((y & 15) << 8) | ((z & 15) << 4) | (x & 15)) as usize
}

// Inclusive ranges of a section's cells inside the volume, by axis.
struct Clip {
    x: (i32, i32),
    y: (i32, i32),
    z: (i32, i32),
}

// The lit volume of a blueprint's bounds (MARGIN past them, 16 below, within
// the world's heights) and the sections over it.
#[derive(Clone, Copy, PartialEq)]
struct Frame {
    min: [i32; 3],
    size: [i32; 3],
    section_min: [i32; 3],
    section_size: [i32; 3],
}

impl Frame {
    fn of(bounds: [i32; 6]) -> Frame {
        let min = [bounds[0] - MARGIN, (-64).max(bounds[1] - MARGIN - 1), bounds[2] - MARGIN];
        let max = [bounds[3] + MARGIN, 959.min(bounds[4] + MARGIN), bounds[5] + MARGIN];
        let section_min = min.map(|value| value >> 4);
        Frame { min, size: [0, 1, 2].map(|axis| max[axis] - min[axis] + 1), section_min, section_size: [0, 1, 2].map(|axis| (max[axis] >> 4) - section_min[axis] + 1) }
    }

    fn has_section(&self, s: [i32; 3]) -> bool {
        (0..3).all(|axis| s[axis] >= self.section_min[axis] && s[axis] < self.section_min[axis] + self.section_size[axis])
    }

    fn slot(&self, [sx, sy, sz]: [i32; 3]) -> usize {
        (((sx - self.section_min[0]) * self.section_size[2] + (sz - self.section_min[2])) * self.section_size[1] + (sy - self.section_min[1])) as usize
    }

    fn footprint(&self, x: i32, z: i32) -> bool {
        x >= self.min[0] && x < self.min[0] + self.size[0] && z >= self.min[2] && z < self.min[2] + self.size[2]
    }

    fn inside(&self, x: i32, y: i32, z: i32) -> bool {
        self.footprint(x, z) && y >= self.min[1] && y < self.min[1] + self.size[1]
    }

    // The cells of section sx, sy, sz inside the volume.
    fn clip(&self, sx: i32, sy: i32, sz: i32) -> Clip {
        let range = |s: i32, axis: usize| ((s << 4).max(self.min[axis]), ((s << 4) + 15).min(self.min[axis] + self.size[axis] - 1));
        Clip { x: range(sx, 0), y: range(sy, 1), z: range(sz, 2) }
    }
}

// A blueprint read a part at a time: `take` its bytes in parts of any
// length, then `finish` lights the world. Only the header and a part of a
// section wait for the next bytes, so a large blueprint never needs to be
// held whole.
pub struct Load {
    mask: Mask,
    pending: Vec<u8>,
    // The world being read and how many of its sections are still to come.
    world: Option<(World, usize)>,
    built: Vec<(i32, i32)>,
}

const SECTION_BYTES: usize = 12 + 8192;

impl Load {
    pub fn new(mask: Mask) -> Load {
        Load { mask, pending: Vec::new(), world: None, built: Vec::new() }
    }

    pub fn take(&mut self, pack: &Pack, bytes: &[u8]) -> Result<(), String> {
        self.pending.extend_from_slice(bytes);
        if self.world.is_none() {
            let mut input = Input::new("blueprint", &self.pending);
            match Self::header(pack, &mut input, self.mask) {
                Ok(started) => {
                    let used = input.at;
                    self.world = Some(started);
                    self.pending.drain(..used);
                }
                Err(_) if input.early => return Ok(()),
                Err(error) => return Err(error),
            }
        }
        let (world, remaining) = self.world.as_mut().unwrap();
        let whole = (self.pending.len() / SECTION_BYTES).min(*remaining);
        for section in self.pending[..whole * SECTION_BYTES].chunks_exact(SECTION_BYTES) {
            let int = |at: usize| i32::from_be_bytes([section[at], section[at + 1], section[at + 2], section[at + 3]]) << 4;
            let (ox, oy, oz) = (int(0), int(4), int(8));
            for i in 0..4096i32 {
                let value = u16::from_be_bytes([section[12 + i as usize * 2], section[13 + i as usize * 2]]);
                if value == 0 {
                    continue;
                }
                let (x, y, z) = (ox + (i & 15), oy + (i >> 8), oz + (i >> 4 & 15));
                if value as usize >= world.palette.len() || !world.inside(x, y, z) {
                    return Err("The blueprint has a block outside its palette or bounds".into());
                }
                world.set(x, y, z, value);
            }
            self.built.push((ox >> 4, oz >> 4));
        }
        *remaining -= whole;
        self.pending.drain(..whole * SECTION_BYTES);
        if *remaining == 0 && !self.pending.is_empty() {
            return Err("The blueprint has trailing bytes".into());
        }
        Ok(())
    }

    // The palette, the bounds and the count of sections, and the world they make.
    fn header(pack: &Pack, input: &mut Input, mask: Mask) -> Result<(World, usize), String> {
        let entries = input.int()?;
        let mut palette = Vec::new();
        let mut light_of = Vec::new();
        for index in 0..entries {
            let (shown, lit) = World::entry(pack, input, index)?;
            palette.push(shown);
            light_of.push(lit);
        }
        // Empty space is entry 0: the light rules above hold for air.
        let void = light_of.first().map(|&state| pack.states[state as usize]);
        if !void.is_some_and(|state| state.emission == 0 && state.light_flags & 3 == 1 && state.light_block <= 1) {
            return Err("The blueprint's entry 0 does not light as air".into());
        }
        let mut bounds = [0i32; 6];
        for value in bounds.iter_mut() {
            *value = input.int()?;
        }
        let sections = input.int()?.max(0) as usize;
        Ok((World::empty(palette, light_of, bounds, mask), sections))
    }

    pub fn finish(mut self, pack: &Pack) -> Result<World, String> {
        let Some((mut world, 0)) = self.world.take() else {
            return Err("The blueprint ends early".into());
        };
        let everywhere = vec![true; (world.size_x * world.size_z) as usize];
        world.relight(pack, &everywhere);
        self.built.sort();
        self.built.dedup();
        for &(cx, cz) in &self.built {
            let span = world.column_spans(cx, cz);
            world.spans.push(span);
        }
        world.columns = self.built;
        Ok(world)
    }
}

impl World {
    // One palette entry: the state BlueMap sees and the state Minecraft
    // lights it as (entry 0 is air to BlueMap, whatever the blueprint names).
    fn entry(pack: &Pack, input: &mut Input, index: i32) -> Result<(u32, u32), String> {
        let id = input.utf()?;
        let mut properties = Vec::new();
        for _ in 0..input.int()? {
            properties.push((input.utf()?, input.utf()?));
        }
        let state = pack.state(&id, &properties)?;
        Ok((if index == 0 { pack.air } else { state }, state))
    }

    // A blueprint whole (Load takes it a part at a time).
    pub fn read(pack: &Pack, bytes: &[u8], mask: Mask) -> Result<World, String> {
        let mut load = Load::new(mask);
        load.take(pack, bytes)?;
        load.finish(pack)
    }

    // A world of no blocks over the volume of `bounds`, unlit.
    fn empty(palette: Vec<u32>, light_of: Vec<u32>, bounds: [i32; 6], mask: Mask) -> World {
        let frame = Frame::of(bounds);
        let count = (frame.section_size[0] * frame.section_size[1] * frame.section_size[2]) as usize;
        World {
            bounds,
            palette,
            light_of,
            min_x: frame.min[0],
            min_y: frame.min[1],
            min_z: frame.min[2],
            size_x: frame.size[0],
            size_y: frame.size[1],
            size_z: frame.size[2],
            section_min: frame.section_min,
            section_size: frame.section_size,
            sections: (0..count).map(|_| Section::open()).collect(),
            open: vec![frame.min[1]; (frame.size[0] * frame.size[2]) as usize],
            columns: Vec::new(),
            spans: Vec::new(),
            mask,
            relit: 0,
        }
    }

    fn frame(&self) -> Frame {
        Frame { min: [self.min_x, self.min_y, self.min_z], size: [self.size_x, self.size_y, self.size_z], section_min: self.section_min, section_size: self.section_size }
    }

    // Applies a patch (see the live contract: the palette entries it adds,
    // the bounds and built column count after it, and the blocks that
    // changed) and relights only where the changes can reach. The bounds may
    // grow and shrink, and chunk columns become built or empty: the world
    // first grows to hold both bounds (`resize`), takes the blocks and their
    // light, and then shrinks to the patch's bounds. Answers the tiles that
    // can draw differently from this world's: every new tile, and every tile
    // mesh::changed finds for the blocks before and after in the volume that
    // holds both (the cells only one volume holds are 15 or more from every
    // block, and no tile reads them). A patch whose blocks are not its
    // palette's, lie outside its bounds or build another count of columns
    // fails and changes nothing.
    pub fn patch(&mut self, pack: &Pack, bytes: &[u8]) -> Result<Vec<(i32, i32)>, String> {
        let mut input = Input::new("patch", bytes);
        let mut added = Vec::new();
        for _ in 0..input.int()? {
            added.push(Self::entry(pack, &mut input, (self.palette.len() + added.len()) as i32)?);
        }
        let mut bounds = [0i32; 6];
        for value in bounds.iter_mut() {
            *value = input.int()?;
        }
        let columns = input.int()?;
        let mut edits = Vec::new();
        for _ in 0..input.int()? {
            let (x, y, z) = (input.int()?, input.int()?, input.int()?);
            let b = input.take(2)?;
            edits.push((x, y, z, u16::from_be_bytes([b[0], b[1]])));
        }
        if input.at != bytes.len() {
            return Err("The patch has trailing bytes".into());
        }
        let within = |x: i32, y: i32, z: i32| x >= bounds[0] && x <= bounds[3] && y >= bounds[1] && y <= bounds[4] && z >= bounds[2] && z <= bounds[5];
        for &(x, y, z, value) in &edits {
            if value as usize >= self.palette.len() + added.len() || (value != 0 && !within(x, y, z)) {
                return Err("The patch has a block outside its palette or bounds".into());
            }
        }
        // The built chunk columns after it: a column is built exactly when it holds a block.
        let mut last = HashMap::new();
        for &(x, y, z, value) in &edits {
            last.insert([x, y, z], value);
        }
        let mut touched: HashMap<(i32, i32), bool> = HashMap::new();
        for (&[x, _, z], &value) in &last {
            *touched.entry((x >> 4, z >> 4)).or_default() |= value != 0;
        }
        for (&(cx, cz), placed) in touched.iter_mut() {
            *placed = *placed || self.holds_block_besides(cx, cz, &last);
        }
        let mut built: Vec<(i32, i32)> = self.columns.iter().filter(|column| !touched.contains_key(column)).copied().chain(touched.iter().filter(|(_, &placed)| placed).map(|(&column, _)| column)).collect();
        built.sort();
        if built.len() != columns as usize {
            return Err(format!("The patch builds {columns} chunk columns, its blocks {}", built.len()));
        }

        let old_tiles = crate::mesh::tiles(self);
        let both = [0, 1, 2].map(|axis| self.bounds[axis].min(bounds[axis]));
        let both = [both[0], both[1], both[2], self.bounds[3].max(bounds[3]), self.bounds[4].max(bounds[4]), self.bounds[5].max(bounds[5])];
        let after = bounds;
        self.resize(both);

        // Apply the edits, keeping each position's first old value; those
        // whose block ends up as it was changed nothing. Air past the volume
        // is air already.
        let mut seen = Vec::with_capacity(edits.len());
        for &(x, y, z, value) in &edits {
            if !self.inside(x, y, z) {
                continue;
            }
            seen.push(((y, z, x), self.palette_at(x, y, z) as u16));
            self.set(x, y, z, value);
        }
        seen.sort_by_key(|&(at, _)| at);
        seen.dedup_by_key(|&mut (at, _)| at);
        let changed: Vec<([i32; 3], u16)> = seen.into_iter().map(|((y, z, x), old)| ([x, y, z], old)).filter(|&([x, y, z], old)| self.palette_at(x, y, z) as u16 != old).collect();

        for (shown, lit) in added {
            self.palette.push(shown);
            self.light_of.push(lit);
        }

        // The built columns' spans: a column built before keeps its own.
        let spans = built
            .iter()
            .map(|&(cx, cz)| match self.columns.binary_search(&(cx, cz)) {
                Ok(at) => self.spans[at],
                Err(_) => self.column_spans(cx, cz),
            })
            .collect();
        self.columns = built;
        self.spans = spans;

        // Columns whose blocks or span membership differ, and the span of each.
        let mut differing = HashSet::new();
        for &([x, y, z], old) in &changed {
            let new = self.palette_at(x, y, z) as u16;
            if self.palette[old as usize] != self.palette[new as usize] || (old != 0) != (new != 0) {
                differing.insert((x, z));
            }
            if let Ok(chunk) = self.columns.binary_search(&(x >> 4, z >> 4)) {
                self.spans[chunk][(((z & 15) << 4) | (x & 15)) as usize] = self.column_span(x, z);
            }
        }

        // Relight the columns near an edit that changes what relight reads of
        // its block, all heights. The others cannot change light.
        let lit: Vec<[i32; 3]> = changed
            .iter()
            .filter(|&&([x, y, z], old)| self.light_key(pack, old) != self.light_key(pack, self.palette_at(x, y, z) as u16))
            .map(|&(at, _)| at)
            .collect();
        let (sx, sz) = (self.size_x as usize, self.size_z as usize);
        let reach = REACH as usize;
        let mut region = vec![false; sx * sz];
        for &[x, _, z] in &lit {
            let (lx, lz) = ((x - self.min_x) as usize, (z - self.min_z) as usize);
            for z in lz.saturating_sub(reach)..=(lz + reach).min(sz - 1) {
                region[z * sx + lx.saturating_sub(reach)..=z * sx + (lx + reach).min(sx - 1)].fill(true);
            }
        }
        let marked: Vec<usize> = (0..sx * sz).filter(|&column| region[column]).collect();
        self.relit = marked.len();
        if !lit.is_empty() {
            // The light of every section the relight can change, as it was.
            let chunks = self.region_chunks(&region);
            let height = self.section_size[1] as usize;
            let before: Vec<(Level, Level)> = chunks
                .iter()
                .flat_map(|&(cx, cz)| {
                    let first = self.slot(cx, self.section_min[1], cz);
                    self.sections[first..first + height].iter().map(|section| (section.sky.clone(), section.glow.clone()))
                })
                .collect();
            self.relight(pack, &region);
            for &column in &marked {
                let (x, z) = (self.min_x + (column % sx) as i32, self.min_z + (column / sx) as i32);
                let chunk = chunks.binary_search(&(x >> 4, z >> 4)).unwrap();
                let first = self.slot(x >> 4, self.section_min[1], z >> 4);
                let differs = (0..height).any(|n| {
                    let (old_sky, old_glow) = &before[chunk * height + n];
                    let section = &self.sections[first + n];
                    let sy = self.section_min[1] + n as i32;
                    let (low, high) = ((sy << 4).max(self.min_y), ((sy << 4) + 15).min(self.min_y + self.size_y - 1));
                    let heights = if old_sky.by_column() && old_glow.by_column() && section.sky.by_column() && section.glow.by_column() { low..=low } else { low..=high };
                    heights.into_iter().any(|y| {
                        let at = cell(x, y, z);
                        old_sky.get(at) != section.sky.get(at) || old_glow.get(at) != section.glow.get(at)
                    })
                });
                if differs {
                    differing.insert((x, z));
                }
            }
        }
        self.resize(after);

        let reading = crate::mesh::reading(differing);
        Ok(crate::mesh::tiles(self).into_iter().filter(|tile| reading.contains(tile) || old_tiles.binary_search(tile).is_err()).collect())
    }

    // Whether chunk column cx, cz is built and holds a block at a position
    // `edits` does not name.
    fn holds_block_besides(&self, cx: i32, cz: i32, edits: &HashMap<[i32; 3], u16>) -> bool {
        if self.columns.binary_search(&(cx, cz)).is_err() {
            return false;
        }
        let first = self.slot(cx, self.section_min[1], cz);
        (0..self.section_size[1] as usize).any(|n| {
            let sy = self.section_min[1] + n as i32;
            self.sections[first + n].blocks.as_ref().is_some_and(|blocks| {
                blocks.iter().enumerate().any(|(at, &block)| block != 0 && !edits.contains_key(&[(cx << 4) | (at as i32 & 15), (sy << 4) | (at as i32 >> 8), (cz << 4) | (at as i32 >> 4 & 15)]))
            })
        })
    }

    // Makes the lit volume the one `bounds` give. Every block must lie
    // within them and within the bounds before. A cell both volumes hold
    // keeps its light; a cell new to the volume takes what a fresh read
    // gives it: full sky light and no block light past the columns and
    // above the heights the volume held, and below them the light of its
    // column's lowest cell (see the top of this file).
    fn resize(&mut self, bounds: [i32; 6]) {
        let old = self.frame();
        let frame = Frame::of(bounds);
        self.bounds = bounds;
        if frame == old {
            return;
        }
        let old_max_y = old.min[1] + old.size[1] - 1;
        let floor: Vec<i8> = if frame.min[1] < old.min[1] {
            (0..old.size[0] * old.size[2])
                .map(|column| {
                    let (x, z) = (old.min[0] + column % old.size[0], old.min[2] + column / old.size[0]);
                    self.section(x, old.min[1], z).sky.get(cell(x, old.min[1], z))
                })
                .collect()
        } else {
            Vec::new()
        };
        // The sky light of a cell the old volume did not hold.
        let new_sky = |x: i32, y: i32, z: i32| -> i8 {
            if !old.footprint(x, z) || y > old_max_y {
                15
            } else {
                floor[((z - old.min[2]) * old.size[0] + (x - old.min[0])) as usize]
            }
        };

        let mut sections: Vec<Option<Section>> = std::mem::take(&mut self.sections).into_iter().map(Some).collect();
        let mut kept = Vec::new();
        let mut made = Vec::new();
        let mut grid = Vec::with_capacity((frame.section_size[0] * frame.section_size[1] * frame.section_size[2]) as usize);
        for nx in 0..frame.section_size[0] {
            for nz in 0..frame.section_size[2] {
                for ny in 0..frame.section_size[1] {
                    let s = [frame.section_min[0] + nx, frame.section_min[1] + ny, frame.section_min[2] + nz];
                    if old.has_section(s) {
                        kept.push((grid.len(), s));
                        grid.push(sections[old.slot(s)].take().unwrap());
                        continue;
                    }
                    // A section the old volume did not reach: its cells are all new, by column.
                    let mut map = [0i8; 256];
                    for (column, value) in map.iter_mut().enumerate() {
                        *value = new_sky((s[0] << 4) | (column as i32 & 15), s[1] << 4, (s[2] << 4) | (column as i32 >> 4));
                    }
                    let sky = if map.iter().all(|&value| value == map[0]) { Level::Uniform(map[0]) } else { Level::Columns(Rc::new(map)) };
                    made.push((grid.len(), s));
                    grid.push(Section { sky, ..Section::open() });
                }
            }
        }
        self.sections = grid;
        [self.min_x, self.min_y, self.min_z] = frame.min;
        [self.size_x, self.size_y, self.size_z] = frame.size;
        self.section_min = frame.section_min;
        self.section_size = frame.section_size;

        // The new cells of the sections both volumes reach.
        for &(slot, [sx, sy, sz]) in &kept {
            let (was, now) = (old.clip(sx, sy, sz), frame.clip(sx, sy, sz));
            if was.x.0 <= now.x.0 && was.x.1 >= now.x.1 && was.y.0 <= now.y.0 && was.y.1 >= now.y.1 && was.z.0 <= now.z.0 && was.z.1 >= now.z.1 {
                continue;
            }
            let section = &mut self.sections[slot];
            for y in now.y.0..=now.y.1 {
                for z in now.z.0..=now.z.1 {
                    for x in now.x.0..=now.x.1 {
                        if old.inside(x, y, z) {
                            continue;
                        }
                        let at = cell(x, y, z);
                        let value = new_sky(x, y, z);
                        if section.sky.get(at) != value {
                            section.sky.cells()[at] = value;
                        }
                        if section.glow.get(at) != 0 {
                            section.glow.cells()[at] = 0;
                        }
                    }
                }
            }
        }
        // A new section next to one holding blocks is near.
        for &(slot, [sx, sy, sz]) in &made {
            let near = (sx - 1..=sx + 1).any(|nx| (sy - 1..=sy + 1).any(|ny| (sz - 1..=sz + 1).any(|nz| frame.has_section([nx, ny, nz]) && self.sections[frame.slot([nx, ny, nz])].blocks.is_some())));
            self.sections[slot].near = near;
        }

        // A column nothing stops sky light in is open down to the volume's floor.
        let mut open = vec![frame.min[1]; (frame.size[0] * frame.size[2]) as usize];
        for z in frame.min[2]..frame.min[2] + frame.size[2] {
            for x in frame.min[0]..frame.min[0] + frame.size[0] {
                if old.footprint(x, z) {
                    let was = self.open[((z - old.min[2]) * old.size[0] + (x - old.min[0])) as usize];
                    open[((z - frame.min[2]) * frame.size[0] + (x - frame.min[0])) as usize] = if was == old.min[1] { frame.min[1] } else { was };
                }
            }
        }
        self.open = open;
    }

    // Everything `relight` and `closed` read of a palette entry's block: its
    // emission, how much light it blocks, whether sky light passes down and
    // whether it shapes light (and then which of its faces are full).
    fn light_key(&self, pack: &Pack, block: u16) -> [u8; 4] {
        let state = &pack.states[self.light_of[block as usize] as usize];
        [state.emission, state.light_block, state.light_flags & 3, if state.light_flags & 2 != 0 { state.full_faces } else { 0 }]
    }

    #[inline(always)]
    fn slot(&self, sx: i32, sy: i32, sz: i32) -> usize {
        (((sx - self.section_min[0]) * self.section_size[2] + (sz - self.section_min[2])) * self.section_size[1] + (sy - self.section_min[1])) as usize
    }

    fn has_section(&self, sx: i32, sy: i32, sz: i32) -> bool {
        let [x, y, z] = [sx - self.section_min[0], sy - self.section_min[1], sz - self.section_min[2]];
        x >= 0 && x < self.section_size[0] && y >= 0 && y < self.section_size[1] && z >= 0 && z < self.section_size[2]
    }

    #[inline(always)]
    fn section(&self, x: i32, y: i32, z: i32) -> &Section {
        &self.sections[self.slot(x >> 4, y >> 4, z >> 4)]
    }

    #[inline(always)]
    fn column(&self, x: i32, z: i32) -> usize {
        ((z - self.min_z) * self.size_x + (x - self.min_x)) as usize
    }

    #[inline(always)]
    fn inside(&self, x: i32, y: i32, z: i32) -> bool {
        x >= self.min_x
            && x < self.min_x + self.size_x
            && y >= self.min_y
            && y < self.min_y + self.size_y
            && z >= self.min_z
            && z < self.min_z + self.size_z
    }

    fn footprint(&self, x: i32, z: i32) -> bool {
        x >= self.min_x && x < self.min_x + self.size_x && z >= self.min_z && z < self.min_z + self.size_z
    }

    // The cells of section sx, sy, sz inside the volume.
    fn clip(&self, sx: i32, sy: i32, sz: i32) -> Clip {
        let range = |s: i32, min: i32, size: i32| ((s << 4).max(min), ((s << 4) + 15).min(min + size - 1));
        Clip { x: range(sx, self.min_x, self.size_x), y: range(sy, self.min_y, self.size_y), z: range(sz, self.min_z, self.size_z) }
    }

    // Sets a block inside the volume, giving its section blocks if it has none.
    fn set(&mut self, x: i32, y: i32, z: i32, value: u16) {
        let slot = self.slot(x >> 4, y >> 4, z >> 4);
        if self.sections[slot].blocks.is_none() {
            if value == 0 {
                return;
            }
            self.sections[slot].blocks = Some(Box::new([0; 4096]));
            let [sx, sy, sz] = [x >> 4, y >> 4, z >> 4];
            for nx in sx - 1..=sx + 1 {
                for ny in sy - 1..=sy + 1 {
                    for nz in sz - 1..=sz + 1 {
                        if self.has_section(nx, ny, nz) {
                            let near = self.slot(nx, ny, nz);
                            self.sections[near].near = true;
                        }
                    }
                }
            }
        }
        self.sections[slot].blocks.as_mut().unwrap()[cell(x, y, z)] = value;
    }

    #[inline(always)]
    fn palette_at(&self, x: i32, y: i32, z: i32) -> usize {
        if self.inside(x, y, z) {
            match &self.section(x, y, z).blocks {
                Some(blocks) => blocks[cell(x, y, z)] as usize,
                None => 0,
            }
        } else {
            0
        }
    }

    // Whether the blueprint names a block at a position (column spans count
    // these, whatever their state).
    pub fn occupied(&self, [x, y, z]: [i32; 3]) -> bool {
        self.palette_at(x, y, z) != 0
    }

    // The lit volume: its lowest corner and its size.
    pub fn volume(&self) -> ([i32; 3], [i32; 3]) {
        ([self.min_x, self.min_y, self.min_z], [self.size_x, self.size_y, self.size_z])
    }

    // The state the world holds at a position (BlueprintWorld's chunks).
    pub fn block(&self, [x, y, z]: [i32; 3]) -> u32 {
        self.palette[self.palette_at(x, y, z)]
    }

    // The state and the (sky, block) light at a position, as `block` and
    // `light` give them.
    #[inline(always)]
    pub fn look(&self, [x, y, z]: [i32; 3]) -> (u32, i32, i32) {
        if !self.inside(x, y, z) {
            return (self.palette[0], 15, 0);
        }
        let section = self.section(x, y, z);
        let at = cell(x, y, z);
        let block = section.blocks.as_ref().map_or(0, |blocks| blocks[at]);
        (self.palette[block as usize], section.sky.get(at) as i32, section.glow.get(at) as i32)
    }

    // The (sky, block) light the world holds at a position: open sky
    // outside the lit volume.
    #[inline(always)]
    pub fn light(&self, [x, y, z]: [i32; 3]) -> (i32, i32) {
        if self.inside(x, y, z) {
            let section = self.section(x, y, z);
            let at = cell(x, y, z);
            (section.sky.get(at) as i32, section.glow.get(at) as i32)
        } else {
            (15, 0)
        }
    }

    // The span BlueMap walks down a column: its highest to lowest block,
    // widened to steps of 8 (an empty column spans y = -64 to -57).
    pub fn span(&self, x: i32, z: i32) -> (i32, i32) {
        match self.columns.binary_search(&(x >> 4, z >> 4)) {
            Ok(at) => self.spans[at][(((z & 15) << 4) | (x & 15)) as usize],
            Err(_) => (-65 | 7, -64),
        }
    }

    fn column_spans(&self, cx: i32, cz: i32) -> [(i32, i32); 256] {
        let mut spans = [(0, 0); 256];
        for (column, span) in spans.iter_mut().enumerate() {
            let column = column as i32;
            *span = self.column_span((cx << 4) | (column & 15), (cz << 4) | (column >> 4));
        }
        spans
    }

    fn column_span(&self, x: i32, z: i32) -> (i32, i32) {
        // The heights of the column's blocks, highest and lowest.
        let mut top = -65;
        let mut bottom = -64;
        if self.footprint(x, z) {
            let first = self.slot(x >> 4, self.section_min[1], z >> 4);
            let held = |n: usize| self.sections[first + n].blocks.as_ref();
            let heights = |n: usize| (0..16).map(move |y| ((self.section_min[1] + n as i32) << 4) + y);
            let height = self.section_size[1] as usize;
            let highest = (0..height).rev().find_map(|n| held(n).and_then(|blocks| heights(n).rev().find(|&y| blocks[cell(x, y, z)] != 0)));
            if let Some(highest) = highest {
                top = highest;
                bottom = (0..height).find_map(|n| held(n).and_then(|blocks| heights(n).find(|&y| blocks[cell(x, y, z)] != 0))).unwrap();
            }
        }
        (top | 7, bottom & !7)
    }

    // Whether light cannot pass from `source` into its neighbour `target` in
    // `direction`: either one's facing side is a full face it shapes light with.
    fn closed(pack: &Pack, source: u32, target: u32, direction: usize) -> bool {
        let (s, t) = (&pack.states[source as usize], &pack.states[target as usize]);
        (s.light_flags & 2 != 0 && s.full_faces >> direction & 1 == 1)
            || (t.light_flags & 2 != 0 && t.full_faces >> OPPOSITE[direction] & 1 == 1)
    }

    // The lowest height of column x, z that sky light reaches straight down
    // from the top of the volume (it stops at the first block it cannot pass
    // into, or that a full face closes from above).
    fn sky_stop(&self, pack: &Pack, x: i32, z: i32) -> i32 {
        let air = self.light_of[0];
        let mut above = air;
        let mut y = self.min_y + self.size_y - 1;
        while y >= self.min_y {
            match &self.section(x, y, z).blocks {
                None => {
                    if Self::closed(pack, above, air, 0) {
                        return y + 1;
                    }
                    above = air;
                    y = ((y >> 4) << 4) - 1;
                }
                Some(blocks) => {
                    let floor = ((y >> 4) << 4).max(self.min_y);
                    while y >= floor {
                        let block = self.light_of[blocks[cell(x, y, z)] as usize];
                        if pack.states[block as usize].light_flags & 1 == 0 || Self::closed(pack, above, block, 0) {
                            return y + 1;
                        }
                        above = block;
                        y -= 1;
                    }
                }
            }
        }
        self.min_y
    }

    // The chunk columns (section x, z) holding a column `region` marks, sorted.
    fn region_chunks(&self, region: &[bool]) -> Vec<(i32, i32)> {
        let mut chunks = HashSet::new();
        for (column, &marked) in region.iter().enumerate() {
            if marked {
                let (x, z) = (self.min_x + column as i32 % self.size_x, self.min_z + column as i32 / self.size_x);
                chunks.insert((x >> 4, z >> 4));
            }
        }
        let mut chunks: Vec<(i32, i32)> = chunks.into_iter().collect();
        chunks.sort();
        chunks
    }

    // The sky light of the sections of chunk column cx, cz that are not near
    // a block: 15 less the distance to the nearest column of the volume that
    // is open to the sky at their height. No block is within 16 of them, so
    // which columns within 15 are open is the same at every height of a run
    // of such sections, as is the light.
    fn far_light(&mut self, cx: i32, cz: i32) {
        let height = self.section_size[1];
        let mut n = height - 1;
        while n >= 0 {
            if self.sections[self.slot(cx, self.section_min[1] + n, cz)].near {
                n -= 1;
                continue;
            }
            let top = n;
            while n >= 0 && !self.sections[self.slot(cx, self.section_min[1] + n, cz)].near {
                n -= 1;
            }
            let y = ((self.section_min[1] + n + 1) << 4).max(self.min_y);
            let level = self.far_level(cx, cz, y);
            for m in n + 1..=top {
                let slot = self.slot(cx, self.section_min[1] + m, cz);
                self.sections[slot].sky = level.clone();
                self.sections[slot].glow = Level::Uniform(0);
            }
        }
    }

    fn far_level(&self, cx: i32, cz: i32, y: i32) -> Level {
        let (x0, z0) = (cx << 4, cz << 4);
        let open = |x: i32, z: i32| self.footprint(x, z) && self.open[self.column(x, z)] <= y;
        if (z0..z0 + 16).all(|z| (x0..x0 + 16).all(|x| !self.footprint(x, z) || open(x, z))) {
            return Level::Uniform(15);
        }
        // Manhattan distances to an open column, capped at 16, over the
        // chunk and the 16 columns around it (two passes of a distance transform).
        let mut d = [[16u8; 48]; 48];
        for (wz, row) in d.iter_mut().enumerate() {
            for (wx, distance) in row.iter_mut().enumerate() {
                if open(x0 - 16 + wx as i32, z0 - 16 + wz as i32) {
                    *distance = 0;
                }
            }
        }
        for wz in 0..48 {
            for wx in 0..48 {
                let mut best = d[wz][wx];
                if wx > 0 {
                    best = best.min(d[wz][wx - 1] + 1);
                }
                if wz > 0 {
                    best = best.min(d[wz - 1][wx] + 1);
                }
                d[wz][wx] = best.min(16);
            }
        }
        for wz in (0..48).rev() {
            for wx in (0..48).rev() {
                let mut best = d[wz][wx];
                if wx < 47 {
                    best = best.min(d[wz][wx + 1] + 1);
                }
                if wz < 47 {
                    best = best.min(d[wz + 1][wx] + 1);
                }
                d[wz][wx] = best.min(16);
            }
        }
        let mut map = [0i8; 256];
        let mut values = HashSet::new();
        for z in 0..16 {
            for x in 0..16 {
                let value = 15 - (d[z + 16][x + 16] as i8).min(15);
                map[z << 4 | x] = value;
                if self.footprint(x0 + x as i32, z0 + z as i32) {
                    values.insert(value);
                }
            }
        }
        if values.len() == 1 {
            Level::Uniform(*values.iter().next().unwrap())
        } else {
            Level::Columns(Rc::new(map))
        }
    }

    // Computes the light of the columns `region` marks (one flag per column,
    // z-major), all heights, from their blocks, taking the light of the
    // unmarked columns beside them as it stands. With every column marked
    // this is the whole volume's light.
    fn relight(&mut self, pack: &Pack, region: &[bool]) {
        let chunks = self.region_chunks(region);
        let max_y = self.min_y + self.size_y - 1;
        let (bottom, height) = (self.section_min[1], self.section_size[1]);
        for (column, &marked) in region.iter().enumerate() {
            if marked {
                let (x, z) = (self.min_x + column as i32 % self.size_x, self.min_z + column as i32 / self.size_x);
                self.open[column] = self.sky_stop(pack, x, z);
            }
        }
        for &(cx, cz) in &chunks {
            self.far_light(cx, cz);
        }

        // The near sections' cells in marked columns start over: sky light
        // 15 where it comes straight down, else zero; no block light.
        let near: Vec<(i32, i32, i32)> = chunks
            .iter()
            .flat_map(|&(cx, cz)| (0..height).map(move |n| (cx, bottom + n, cz)))
            .filter(|&(sx, sy, sz)| self.sections[self.slot(sx, sy, sz)].near)
            .collect();
        for &(sx, sy, sz) in &near {
            let clip = self.clip(sx, sy, sz);
            let slot = self.slot(sx, sy, sz);
            let marked: Vec<(i32, i32, i32)> = (clip.z.0..=clip.z.1)
                .flat_map(|z| (clip.x.0..=clip.x.1).map(move |x| (x, z)))
                .filter_map(|(x, z)| {
                    let column = self.column(x, z);
                    region[column].then(|| (x, z, self.open[column]))
                })
                .collect();
            let section = &mut self.sections[slot];
            if !section.sky.is(15) || marked.iter().any(|&(_, _, open)| open > clip.y.0) {
                let sky = section.sky.cells();
                for &(x, z, open) in &marked {
                    for y in clip.y.0..=clip.y.1 {
                        sky[cell(x, y, z)] = if y >= open { 15 } else { 0 };
                    }
                }
            }
            if !section.glow.is(0) {
                let glow = section.glow.cells();
                for &(x, z, _) in &marked {
                    for y in clip.y.0..=clip.y.1 {
                        glow[cell(x, y, z)] = 0;
                    }
                }
            }
        }

        // Sky light: the cells next to one that takes light (in a marked
        // column, a near section and below where sky light comes straight
        // down) and that do not take it themselves give theirs.
        let mut queue = VecDeque::new();
        for &(sx, sy, sz) in &near {
            let clip = self.clip(sx, sy, sz);
            for z in clip.z.0..=clip.z.1 {
                for x in clip.x.0..=clip.x.1 {
                    let column = self.column(x, z);
                    let open = self.open[column];
                    if !region[column] || open <= clip.y.0 {
                        continue;
                    }
                    let (low, high) = (clip.y.0, clip.y.1.min(open - 1));
                    if high == open - 1 {
                        if open <= max_y {
                            queue.push_back([x, open, z]);
                        }
                    } else if !self.section(x, high + 1, z).near {
                        queue.push_back([x, high + 1, z]);
                    }
                    if low > self.min_y && !self.section(x, low - 1, z).near {
                        queue.push_back([x, low - 1, z]);
                    }
                    for [dx, dz] in SIDES {
                        let (nx, nz) = (x + dx, z + dz);
                        if !self.footprint(nx, nz) {
                            continue;
                        }
                        let beside = self.column(nx, nz);
                        let from = if region[beside] && self.section(nx, low, nz).near { low.max(self.open[beside]) } else { low };
                        for y in from..=high {
                            queue.push_back([nx, y, nz]);
                        }
                    }
                }
            }
            // A section's givers spread before the next section's are found:
            // light settles the same in any order, and the queue stays short.
            self.spread(pack, &mut queue, true, region);
        }

        // Block light: every light source in a marked column, and the lit
        // cells of the unmarked columns beside a marked one.
        let emission: Vec<u8> = self.light_of.iter().map(|&state| pack.states[state as usize].emission).collect();
        for &(sx, sy, sz) in &near {
            let slot = self.slot(sx, sy, sz);
            let Some(blocks) = &self.sections[slot].blocks else {
                continue;
            };
            let clip = self.clip(sx, sy, sz);
            let mut sources = Vec::new();
            for z in clip.z.0..=clip.z.1 {
                for x in clip.x.0..=clip.x.1 {
                    if !region[self.column(x, z)] {
                        continue;
                    }
                    for y in clip.y.0..=clip.y.1 {
                        let level = emission[blocks[cell(x, y, z)] as usize];
                        if level > 0 {
                            sources.push(([x, y, z], level));
                        }
                    }
                }
            }
            for (at, level) in sources {
                self.sections[slot].glow.cells()[cell(at[0], at[1], at[2])] = level as i8;
                queue.push_back(at);
            }
        }
        for (column, &marked) in region.iter().enumerate() {
            if !marked {
                continue;
            }
            let (x, z) = (self.min_x + column as i32 % self.size_x, self.min_z + column as i32 / self.size_x);
            for [dx, dz] in SIDES {
                let (nx, nz) = (x + dx, z + dz);
                if !self.footprint(nx, nz) || region[self.column(nx, nz)] {
                    continue;
                }
                for n in 0..height {
                    let sy = self.section_min[1] + n;
                    let glow = &self.sections[self.slot(nx >> 4, sy, nz >> 4)].glow;
                    if glow.is(0) {
                        continue;
                    }
                    let clip = self.clip(nx >> 4, sy, nz >> 4);
                    for y in clip.y.0..=clip.y.1 {
                        if glow.get(cell(nx, y, nz)) > 0 {
                            queue.push_back([nx, y, nz]);
                        }
                    }
                }
            }
        }
        self.spread(pack, &mut queue, false, region);

        // Sections that came out the same everywhere keep one value.
        for &(sx, sy, sz) in &near {
            let clip = self.clip(sx, sy, sz);
            let slot = self.slot(sx, sy, sz);
            let section = &mut self.sections[slot];
            for level in [&mut section.sky, &mut section.glow] {
                if let Level::Cells(cells) = level {
                    let first = cells[cell(clip.x.0, clip.y.0, clip.z.0)];
                    let same = (clip.y.0..=clip.y.1).all(|y| (clip.z.0..=clip.z.1).all(|z| (clip.x.0..=clip.x.1).all(|x| cells[cell(x, y, z)] == first)));
                    if same {
                        *level = Level::Uniform(first);
                    }
                }
            }
        }
    }

    // Spreads light from the queued cells into the cells that take it: in
    // a marked column and a near section (and, for sky light, below where it
    // comes straight down). Every other cell already holds its light.
    fn spread(&mut self, pack: &Pack, queue: &mut VecDeque<[i32; 3]>, sky_light: bool, region: &[bool]) {
        while let Some([x, y, z]) = queue.pop_front() {
            let from = self.section(x, y, z);
            let at = cell(x, y, z);
            let current = if sky_light { from.sky.get(at) } else { from.glow.get(at) } as i32;
            if current <= 1 {
                continue;
            }
            let source = self.light_of[from.blocks.as_ref().map_or(0, |blocks| blocks[at]) as usize];
            for (direction, step) in STEPS.iter().enumerate() {
                let (nx, ny, nz) = (x + step[0], y + step[1], z + step[2]);
                if !self.inside(nx, ny, nz) {
                    continue;
                }
                let column = self.column(nx, nz);
                if !region[column] || (sky_light && ny >= self.open[column]) {
                    continue;
                }
                let slot = self.slot(nx >> 4, ny >> 4, nz >> 4);
                if !self.sections[slot].near {
                    continue;
                }
                let to = cell(nx, ny, nz);
                let target = self.light_of[self.sections[slot].blocks.as_ref().map_or(0, |blocks| blocks[to]) as usize];
                if Self::closed(pack, source, target, direction) {
                    continue;
                }
                let t = &pack.states[target as usize];
                let next = if sky_light && direction == 0 && current == 15 && t.light_flags & 1 != 0 {
                    15
                } else {
                    current - (t.light_block as i32).max(1)
                };
                let level = if sky_light { &mut self.sections[slot].sky } else { &mut self.sections[slot].glow };
                if next > level.get(to) as i32 {
                    level.cells()[to] = next as i8;
                    queue.push_back([nx, ny, nz]);
                }
            }
        }
    }

    // The columns (x, z) at which the two worlds differ at some height: in
    // the block, whether a block is named there, or the light.
    pub fn differing_columns(before: &World, after: &World) -> HashSet<(i32, i32)> {
        let mut positions = Vec::new();
        for world in [before, after] {
            for sx in 0..world.section_size[0] {
                for sz in 0..world.section_size[2] {
                    for sy in 0..world.section_size[1] {
                        let at = [world.section_min[0] + sx, world.section_min[1] + sy, world.section_min[2] + sz];
                        if !world.sections[world.slot(at[0], at[1], at[2])].plain() {
                            positions.push(at);
                        }
                    }
                }
            }
        }
        positions.sort();
        positions.dedup();
        let mut columns = HashSet::new();
        for [sx, sy, sz] in positions {
            let views = [before.view(sx, sy, sz), after.view(sx, sy, sz)];
            // Air with light that is the same at every height in both,
            // over the same heights: one height tells.
            let by_column = views.iter().all(|view| view.section.is_none_or(|section| section.blocks.is_none() && section.sky.by_column() && section.glow.by_column())) && views[0].heights == views[1].heights;
            for z in sz << 4..(sz << 4) + 16 {
                for x in sx << 4..(sx << 4) + 16 {
                    if columns.contains(&(x, z)) {
                        continue;
                    }
                    let heights = match views[0].heights {
                        Some((low, _)) if by_column => low..=low,
                        _ => sy << 4..=(sy << 4) + 15,
                    };
                    if heights.into_iter().any(|y| views[0].get(x, y, z) != views[1].get(x, y, z)) {
                        columns.insert((x, z));
                    }
                }
            }
        }
        columns
    }

    fn view(&self, sx: i32, sy: i32, sz: i32) -> View<'_> {
        let held = self.has_section(sx, sy, sz);
        View {
            world: self,
            section: held.then(|| &self.sections[self.slot(sx, sy, sz)]),
            heights: held.then(|| self.clip(sx, sy, sz).y),
        }
    }

    // What tile tx, tz reads of the world, as bytes `from_snapshot` makes a
    // world of that draws the tile as this one does: the palette, the mask,
    // and the volume, sections and spans of the chunk columns holding the
    // columns the tile reads (mesh::reads), at every height. The world it
    // makes holds nothing else; it is for drawing that tile, by another
    // module of the same build.
    pub fn snapshot(&self, tx: i32, tz: i32, out: &mut Vec<u8>) {
        let ((x0, x1), (z0, z1)) = (crate::mesh::reads(tx), crate::mesh::reads(tz));
        let min = [(x0 & !15).max(self.min_x), self.min_y, (z0 & !15).max(self.min_z)];
        let max = [((x1 & !15) + 15).min(self.min_x + self.size_x - 1), self.min_y + self.size_y - 1, ((z1 & !15) + 15).min(self.min_z + self.size_z - 1)];
        let size = [0, 1, 2].map(|axis| (max[axis] - min[axis] + 1).max(0));
        let section_min = min.map(|value| value >> 4);
        let section_size = if size.contains(&0) { [0; 3] } else { [0, 1, 2].map(|axis| (max[axis] >> 4) - section_min[axis] + 1) };
        out.clear();
        let int = |out: &mut Vec<u8>, value: i32| out.extend_from_slice(&value.to_le_bytes());
        for value in [tx, tz, self.mask.min_x, self.mask.max_x, self.mask.min_z, self.mask.max_z, self.palette.len() as i32] {
            int(out, value);
        }
        for &state in &self.palette {
            out.extend_from_slice(&state.to_le_bytes());
        }
        for value in min.into_iter().chain(size).chain(section_min).chain(section_size) {
            int(out, value);
        }
        let level = |out: &mut Vec<u8>, level: &Level| match level {
            Level::Uniform(value) => out.push(*value as u8),
            Level::Columns(map) => out.extend(map.iter().map(|&value| value as u8)),
            Level::Cells(cells) => out.extend(cells.iter().map(|&value| value as u8)),
        };
        let kind = |level: &Level| match level {
            Level::Uniform(_) => 0u8,
            Level::Columns(_) => 1,
            Level::Cells(_) => 2,
        };
        for sx in section_min[0]..section_min[0] + section_size[0] {
            for sz in section_min[2]..section_min[2] + section_size[2] {
                for sy in section_min[1]..section_min[1] + section_size[1] {
                    let section = &self.sections[self.slot(sx, sy, sz)];
                    out.push(section.blocks.is_some() as u8 | kind(&section.sky) << 1 | kind(&section.glow) << 3);
                    if let Some(blocks) = &section.blocks {
                        for &block in blocks.iter() {
                            out.extend_from_slice(&block.to_le_bytes());
                        }
                    }
                    level(out, &section.sky);
                    level(out, &section.glow);
                }
            }
        }
        let held: Vec<usize> = (0..self.columns.len())
            .filter(|&at| {
                let (cx, cz) = self.columns[at];
                cx >= section_min[0] && cx < section_min[0] + section_size[0] && cz >= section_min[2] && cz < section_min[2] + section_size[2]
            })
            .collect();
        int(out, held.len() as i32);
        for at in held {
            int(out, self.columns[at].0);
            int(out, self.columns[at].1);
            for &(top, bottom) in self.spans[at].iter() {
                int(out, top);
                int(out, bottom);
            }
        }
    }

    // The world of a snapshot (see `snapshot`), and the tile it is for.
    pub fn from_snapshot(bytes: &[u8]) -> Result<(World, i32, i32), String> {
        let mut input = Snapshot { bytes, at: 0 };
        let (tx, tz) = (input.int()?, input.int()?);
        let mask = Mask { min_x: input.int()?, max_x: input.int()?, min_z: input.int()?, max_z: input.int()? };
        let palette = (0..input.int()?).map(|_| input.int().map(|state| state as u32)).collect::<Result<Vec<u32>, String>>()?;
        let mut four = [[0i32; 3]; 4];
        for vector in four.iter_mut() {
            for value in vector.iter_mut() {
                *value = input.int()?;
            }
        }
        let [min, size, section_min, section_size] = four;
        let count = (section_size[0] * section_size[1] * section_size[2]) as usize;
        let mut sections = Vec::with_capacity(count);
        for _ in 0..count {
            let flags = input.take(1)?[0];
            let blocks = if flags & 1 != 0 {
                let mut blocks = Box::new([0u16; 4096]);
                for (block, pair) in blocks.iter_mut().zip(input.take(8192)?.chunks_exact(2)) {
                    *block = u16::from_le_bytes([pair[0], pair[1]]);
                }
                Some(blocks)
            } else {
                None
            };
            let sky = input.level(flags >> 1 & 3)?;
            let glow = input.level(flags >> 3 & 3)?;
            sections.push(Section { blocks, sky, glow, near: false });
        }
        let mut columns = Vec::new();
        let mut spans = Vec::new();
        for _ in 0..input.int()? {
            columns.push((input.int()?, input.int()?));
            let mut span = [(0, 0); 256];
            for column in span.iter_mut() {
                *column = (input.int()?, input.int()?);
            }
            spans.push(span);
        }
        if input.at != bytes.len() {
            return Err("The snapshot has trailing bytes".into());
        }
        let world = World {
            bounds: [0; 6],
            palette,
            light_of: Vec::new(),
            min_x: min[0],
            min_y: min[1],
            min_z: min[2],
            size_x: size[0],
            size_y: size[1],
            size_z: size[2],
            section_min,
            section_size,
            sections,
            open: Vec::new(),
            columns,
            spans,
            mask,
            relit: 0,
        };
        Ok((world, tx, tz))
    }
}

// A snapshot's bytes, read in order (little endian).
struct Snapshot<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Snapshot<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let slice = self.bytes.get(self.at..self.at + n).ok_or("The snapshot ends early")?;
        self.at += n;
        Ok(slice)
    }

    fn int(&mut self) -> Result<i32, String> {
        Ok(i32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn level(&mut self, kind: u8) -> Result<Level, String> {
        let signed = |bytes: &[u8]| bytes.iter().map(|&value| value as i8).collect::<Vec<i8>>();
        Ok(match kind {
            0 => Level::Uniform(self.take(1)?[0] as i8),
            1 => Level::Columns(Rc::new(signed(self.take(256)?).try_into().unwrap())),
            2 => Level::Cells(signed(self.take(4096)?).into_boxed_slice().try_into().unwrap()),
            _ => return Err("The snapshot has an unknown kind of light".into()),
        })
    }
}

// What a world holds over one section, for comparing two worlds.
struct View<'a> {
    world: &'a World,
    section: Option<&'a Section>,
    heights: Option<(i32, i32)>,
}

impl View<'_> {
    // The state, whether a block is named, and the light at a position of the section.
    fn get(&self, x: i32, y: i32, z: i32) -> (u32, bool, i8, i8) {
        match self.section {
            Some(section) if self.world.inside(x, y, z) => {
                let at = cell(x, y, z);
                let block = section.blocks.as_ref().map_or(0, |blocks| blocks[at]);
                (self.world.palette[block as usize], block != 0, section.sky.get(at), section.glow.get(at))
            }
            _ => (self.world.palette[0], false, 15, 0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pack::{Block, State, AIR, NONE};
    use std::collections::BTreeMap;

    // A pack of light-relevant blocks, one state each: air and cave_air,
    // stone, glass, glowstone, a torch, water, a bottom slab (full only on
    // its down face), leaves, and a wire and a repeater, which light
    // treats as air (as it does glass; the repeater's full faces do not
    // count, as it does not shape light): what a light-neutral change swaps.
    const NAMES: [&str; 11] = ["air", "cave_air", "stone", "glass", "glowstone", "torch", "water", "slab", "leaves", "wire", "repeater"];
    const NEUTRAL: [usize; 4] = [0, 3, 9, 10];

    fn pack() -> Pack {
        // (flags, emission, light_block, light_flags, full_faces)
        let table: [(u8, u8, u8, u8, u8); 11] = [
            (AIR, 0, 0, 1, 0),
            (AIR, 0, 0, 1, 0),
            (3, 0, 15, 2, 63),
            (4, 0, 0, 1, 0),
            (3, 15, 15, 2, 63),
            (0, 14, 0, 1, 0),
            (0, 0, 1, 1, 0),
            (1, 0, 15, 2, 1),
            (0, 0, 1, 0, 0),
            (0, 0, 0, 1, 0),
            (0, 0, 0, 1, 63),
        ];
        let mut ids: Vec<u32> = (0..11).collect();
        ids.sort_by_key(|&i| format!("minecraft:{}", NAMES[i as usize]));
        Pack {
            blocks: NAMES.iter().enumerate().map(|(i, name)| Block { id: format!("minecraft:{name}"), first: i as u32, properties: Vec::new() }).collect(),
            by_id: ids,
            states: table
                .iter()
                .enumerate()
                .map(|(i, &(flags, emission, light_block, light_flags, full_faces))| State {
                    flags,
                    level: 0,
                    emission,
                    light_block,
                    light_flags,
                    full_faces,
                    block: i as u16,
                    tint: 0,
                    selection: NONE,
                })
                .collect(),
            air: 0,
            water_selection: NONE,
            water_tint: 0,
            water_blends: false,
            tints: Vec::new(),
            matrices: Vec::new(),
            directions: Vec::new(),
            elements: Vec::new(),
            meshes: Vec::new(),
            variants: Vec::new(),
            sets: Vec::new(),
            selections: Vec::new(),
            liquid_up: Vec::new(),
            liquid_side: [0.0; 8],
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

    fn put_int(out: &mut Vec<u8>, value: i32) {
        out.extend_from_slice(&value.to_be_bytes());
    }

    fn put_entry(out: &mut Vec<u8>, name: usize) {
        let id = format!("minecraft:{}", NAMES[name]);
        out.extend_from_slice(&(id.len() as u16).to_be_bytes());
        out.extend_from_slice(id.as_bytes());
        put_int(out, 0);
    }

    // Blocks as position -> index into NAMES (air is not stored).
    type Model = BTreeMap<(i32, i32, i32), usize>;

    fn bounds(model: &Model) -> [i32; 6] {
        let mut bounds = [i32::MAX, i32::MAX, i32::MAX, i32::MIN, i32::MIN, i32::MIN];
        for &(x, y, z) in model.keys() {
            for (axis, value) in [x, y, z].into_iter().enumerate() {
                bounds[axis] = bounds[axis].min(value);
                bounds[axis + 3] = bounds[axis + 3].max(value);
            }
        }
        bounds
    }

    fn built(model: &Model) -> Vec<(i32, i32)> {
        let mut built: Vec<(i32, i32)> = model.keys().map(|&(x, _, z)| (x >> 4, z >> 4)).collect();
        built.sort();
        built.dedup();
        built
    }

    // A blueprint of the model; its palette as indices into NAMES (entry 0
    // is `zero`), in first-seen order.
    fn blueprint(model: &Model, zero: usize) -> (Vec<u8>, Vec<usize>) {
        blueprint_in(model, zero, bounds(model))
    }

    // The same, saying `bounds` (which hold the blocks) are its bounds.
    fn blueprint_in(model: &Model, zero: usize, bounds: [i32; 6]) -> (Vec<u8>, Vec<usize>) {
        let mut palette = vec![zero];
        let mut out = Vec::new();
        let mut sections: BTreeMap<(i32, i32, i32), Vec<u16>> = BTreeMap::new();
        for (&(x, y, z), &name) in model {
            let at = match palette.iter().position(|&n| n == name) {
                Some(at) if at > 0 => at,
                _ => {
                    palette.push(name);
                    palette.len() - 1
                }
            };
            sections.entry((x >> 4, y >> 4, z >> 4)).or_insert_with(|| vec![0; 4096])[((y & 15) << 8 | (z & 15) << 4 | (x & 15)) as usize] = at as u16;
        }
        put_int(&mut out, palette.len() as i32);
        for &name in &palette {
            put_entry(&mut out, name);
        }
        for value in bounds {
            put_int(&mut out, value);
        }
        put_int(&mut out, sections.len() as i32);
        for ((x, y, z), indices) in sections {
            for value in [x, y, z] {
                put_int(&mut out, value);
            }
            for value in indices {
                out.extend_from_slice(&value.to_be_bytes());
            }
        }
        (out, palette)
    }

    // A patch to `model` (the world after it) with the stream palette
    // `palette` (which it appends to), listing `edits` (position, name).
    fn patch_bytes(model: &Model, palette: &mut Vec<usize>, edits: &[((i32, i32, i32), usize)]) -> Vec<u8> {
        let before = palette.len();
        let mut indices = Vec::new();
        for &(_, name) in edits {
            let at = if name == 0 {
                0
            } else {
                match palette.iter().position(|&n| n == name) {
                    Some(at) if at > 0 => at,
                    _ => {
                        palette.push(name);
                        palette.len() - 1
                    }
                }
            };
            indices.push(at as u16);
        }
        let mut out = Vec::new();
        put_int(&mut out, (palette.len() - before) as i32);
        for &name in &palette[before..] {
            put_entry(&mut out, name);
        }
        for value in bounds(model) {
            put_int(&mut out, value);
        }
        put_int(&mut out, built(model).len() as i32);
        put_int(&mut out, edits.len() as i32);
        for (&((x, y, z), _), index) in edits.iter().zip(indices) {
            for value in [x, y, z] {
                put_int(&mut out, value);
            }
            out.extend_from_slice(&index.to_be_bytes());
        }
        out
    }

    const MASK: Mask = Mask { min_x: -100000, max_x: 100000, min_z: -100000, max_z: 100000 };

    // The state a position lights as.
    fn lit_as(world: &World, [x, y, z]: [i32; 3]) -> u32 {
        world.light_of[world.palette_at(x, y, z)]
    }

    // Every observable the mesher reads, cell for cell.
    fn assert_same(patched: &World, fresh: &World, what: &str) {
        assert_eq!(patched.bounds, fresh.bounds, "{what}: bounds");
        assert_eq!(patched.volume(), fresh.volume(), "{what}: volume");
        assert_eq!(patched.columns, fresh.columns, "{what}: columns");
        assert_eq!(patched.spans, fresh.spans, "{what}: spans");
        let ([min_x, min_y, min_z], [size_x, size_y, size_z]) = fresh.volume();
        for y in min_y..min_y + size_y {
            for z in min_z..min_z + size_z {
                for x in min_x..min_x + size_x {
                    let at = [x, y, z];
                    assert!(patched.block(at) == fresh.block(at) && patched.occupied(at) == fresh.occupied(at) && lit_as(patched, at) == lit_as(fresh, at), "{what}: block at {at:?}");
                    assert!(patched.light(at) == fresh.light(at), "{what}: light at {at:?}: {:?} against {:?}", patched.light(at), fresh.light(at));
                }
            }
        }
        assert!(World::differing_columns(patched, fresh).is_empty(), "{what}: differing columns");
    }

    // The light Minecraft's rules give every cell of the volume: each
    // column's sky light straight down, block light at each source, both
    // spread through the whole volume cell by cell.
    fn reference_light(pack: &Pack, world: &World) -> Vec<(i8, i8)> {
        let ([min_x, min_y, min_z], [sx, sy, sz]) = world.volume();
        let (sx, sy, sz) = (sx as usize, sy as usize, sz as usize);
        let index = |x: usize, y: usize, z: usize| (y * sz + z) * sx + x;
        let state = |x: usize, y: usize, z: usize| lit_as(world, [min_x + x as i32, min_y + y as i32, min_z + z as i32]);
        let mut light = vec![(0i8, 0i8); sx * sy * sz];
        for sky_light in [true, false] {
            let mut queue = VecDeque::new();
            for z in 0..sz {
                for x in 0..sx {
                    let mut above = world.light_of[0];
                    for y in (0..sy).rev() {
                        let block = state(x, y, z);
                        if sky_light {
                            if pack.states[block as usize].light_flags & 1 == 0 || World::closed(pack, above, block, 0) {
                                break;
                            }
                            light[index(x, y, z)].0 = 15;
                            queue.push_back((x, y, z));
                            above = block;
                        } else if pack.states[block as usize].emission > 0 {
                            light[index(x, y, z)].1 = pack.states[block as usize].emission as i8;
                            queue.push_back((x, y, z));
                        }
                    }
                }
            }
            while let Some((x, y, z)) = queue.pop_front() {
                let level = |light: &(i8, i8)| if sky_light { light.0 } else { light.1 } as i32;
                let current = level(&light[index(x, y, z)]);
                let source = state(x, y, z);
                for (direction, step) in STEPS.iter().enumerate() {
                    let (nx, ny, nz) = (x as i32 + step[0], y as i32 + step[1], z as i32 + step[2]);
                    if nx < 0 || ny < 0 || nz < 0 || nx >= sx as i32 || ny >= sy as i32 || nz >= sz as i32 {
                        continue;
                    }
                    let (nx, ny, nz) = (nx as usize, ny as usize, nz as usize);
                    let target = state(nx, ny, nz);
                    if World::closed(pack, source, target, direction) {
                        continue;
                    }
                    let t = &pack.states[target as usize];
                    let next = if sky_light && direction == 0 && current == 15 && t.light_flags & 1 != 0 { 15 } else { current - (t.light_block as i32).max(1) };
                    let cell = &mut light[index(nx, ny, nz)];
                    if next > level(cell) {
                        if sky_light {
                            cell.0 = next as i8;
                        } else {
                            cell.1 = next as i8;
                        }
                        queue.push_back((nx, ny, nz));
                    }
                }
            }
        }
        light
    }

    fn assert_reference_light(pack: &Pack, world: &World, what: &str) {
        let ([min_x, min_y, min_z], [sx, sy, sz]) = world.volume();
        let light = reference_light(pack, world);
        for y in 0..sy {
            for z in 0..sz {
                for x in 0..sx {
                    let (sky, glow) = light[((y * sz + z) * sx + x) as usize];
                    let at = [min_x + x, min_y + y, min_z + z];
                    assert_eq!(world.light(at), (sky as i32, glow as i32), "{what}: light at {at:?}");
                }
            }
        }
    }

    fn random_model(random: &mut Random, size: i32, density: u64) -> Model {
        let mut model = Model::new();
        for x in 0..size {
            for z in 0..size {
                if random.next() % 100 < density {
                    for _ in 0..1 + random.below(3) {
                        model.insert((x, 60 + random.below(14), z), 2 + random.below(7) as usize);
                    }
                }
            }
        }
        // Anchors at the corners hold the bounds and the built columns.
        for (x, z) in [(0, 0), (size - 1, 0), (0, size - 1), (size - 1, size - 1)] {
            model.insert((x, 60, z), 2);
            model.insert((x, 73, z), 2);
        }
        model
    }

    // The columns a patch relights: those within REACH of an edit that
    // changes the light class of its block (air, glass, wire and repeater
    // are one class, every other block its own), in the volume that holds
    // the world before and after it.
    fn relit_columns(volume: ([i32; 3], [i32; 3]), model: &Model, next: &Model, edits: &[((i32, i32, i32), usize)]) -> usize {
        let ([min_x, _, min_z], [size_x, _, size_z]) = volume;
        let mut columns = std::collections::HashSet::new();
        for &((x, y, z), _) in edits {
            let (old, new) = (model.get(&(x, y, z)).copied().unwrap_or(0), next.get(&(x, y, z)).copied().unwrap_or(0));
            if old == new || (NEUTRAL.contains(&old) && NEUTRAL.contains(&new)) {
                continue;
            }
            for cx in (x - REACH).max(min_x)..=(x + REACH).min(min_x + size_x - 1) {
                for cz in (z - REACH).max(min_z)..=(z + REACH).min(min_z + size_z - 1) {
                    columns.insert((cx, cz));
                }
            }
        }
        columns.len()
    }

    fn union(a: [i32; 6], b: [i32; 6]) -> [i32; 6] {
        [a[0].min(b[0]), a[1].min(b[1]), a[2].min(b[2]), a[3].max(b[3]), a[4].max(b[4]), a[5].max(b[5])]
    }

    // The world of `model` over the volume of `bounds`, which hold its blocks.
    fn read_in(pack: &Pack, model: &Model, zero: usize, bounds: [i32; 6]) -> World {
        World::read(pack, &blueprint_in(model, zero, bounds).0, MASK).unwrap()
    }

    // Patches a world through a random sequence and checks each against a
    // fresh read of the same blocks. With `neutral`, most edits swap air,
    // glass, wire and repeaters (which light cannot tell apart) and one in
    // ten places a light source or a torch; otherwise now and then an edit
    // lands past the bounds (they grow) or empties a corner that holds them
    // (they shrink), and chunk columns are built and emptied. Returns the
    // patches applied, those that moved the bounds or the built columns,
    // and those applied without relighting a column.
    fn run(seed: u64, size: i32, density: u64, steps: usize, neutral: bool) -> (usize, usize, usize) {
        let pack = pack();
        let mut random = Random(seed);
        let mut model = random_model(&mut random, size, density);
        let zero = random.below(2) as usize;
        let (base, mut palette) = blueprint(&model, zero);
        let mut world = World::read(&pack, &base, MASK).unwrap();
        let (mut applied, mut moved, mut unlit) = (0, 0, 0);
        for step in 0..steps {
            let mut edits: Vec<((i32, i32, i32), usize)> = Vec::new();
            let mut next = model.clone();
            let count = 1 + random.below(if step % 4 == 0 { 120 } else { 6 });
            // Edits cluster around one point or scatter over the build.
            let (cx, cz) = (random.below(size as u64), random.below(size as u64));
            let spread = if random.next() % 2 == 0 { 6 } else { size as u64 };
            for _ in 0..count {
                let x = (cx + random.below(spread) - spread as i32 / 2).clamp(0, size - 1);
                let z = (cz + random.below(spread) - spread as i32 / 2).clamp(0, size - 1);
                let y = 60 + random.below(14);
                // Past the bounds now and then: above, below (as water falls), beside.
                let at = match (neutral, random.next() % 40) {
                    (false, 0) => (x, y + 14 + random.below(30), z),
                    (false, 1) => (x, y - 20 - random.below(60), z),
                    (false, 2) => (x - 10 - random.below(40), y, z),
                    (false, 3) => (x, y, z + 10 + random.below(40)),
                    _ => (x, y, z),
                };
                if edits.iter().any(|&(p, _)| p == at) {
                    continue;
                }
                let name = if neutral {
                    match random.next() % 10 {
                        0 => 4 + random.below(2) as usize,
                        n => NEUTRAL[n as usize % 4],
                    }
                } else {
                    match random.next() % 10 {
                        0..=2 => 0,
                        3 => 4,
                        4 => 5,
                        5 => 2,
                        n => 1 + n as usize % 8,
                    }
                };
                let name = if name == 1 { 3 } else { name };
                // A neutral run only swaps blocks of the light class of air, but for its lights.
                if neutral && name != 4 && name != 5 && !NEUTRAL.contains(&next.get(&at).copied().unwrap_or(0)) {
                    continue;
                }
                if name == 0 { next.remove(&at) } else { next.insert(at, name) };
                edits.push((at, name));
            }
            // Now and then the blocks at the bounds' edge go: the bounds shrink.
            if !neutral && random.next() % 6 == 0 {
                let edge = bounds(&next);
                let axis = random.below(6) as usize;
                for (&at, _) in model.iter().filter(|(&(x, y, z), _)| [x, y, z, x, y, z][axis] == edge[axis]) {
                    if next.len() > 1 && !edits.iter().any(|&(p, _)| p == at) {
                        next.remove(&at);
                        edits.push((at, 0));
                    }
                }
            }
            if next.is_empty() {
                continue;
            }
            // Entries that change nothing: the block already there.
            for _ in 0..random.below(4) {
                let at = (random.below(size as u64), 60 + random.below(14), random.below(size as u64));
                if !edits.iter().any(|&(p, _)| p == at) {
                    edits.push((at, next.get(&at).copied().unwrap_or(0)));
                }
            }
            moved += (bounds(&next) != bounds(&model) || built(&next) != built(&model)) as usize;
            let mut stream = palette.clone();
            let bytes = patch_bytes(&next, &mut stream, &edits);
            let both = union(bounds(&model), bounds(&next));
            let (before, after) = (read_in(&pack, &model, zero, both), read_in(&pack, &next, zero, both));
            let tiles = world.patch(&pack, &bytes).unwrap();
            let relit = relit_columns(after.volume(), &model, &next, &edits);
            let what = format!("seed {seed} step {step}");
            applied += 1;
            assert_eq!(world.relit, relit, "{what}: relit columns");
            unlit += (relit == 0) as usize;
            model = next;
            palette = stream;
            let fresh = World::read(&pack, &blueprint(&model, zero).0, MASK).unwrap();
            assert_same(&world, &fresh, &what);
            if step % 8 == 0 {
                assert_reference_light(&pack, &world, &what);
            }
            assert_eq!(tiles, crate::mesh::changed(&before, &after), "{what}: changed tiles");
        }
        (applied, moved, unlit)
    }

    #[test]
    fn patched_worlds_equal_fresh_reads_cell_for_cell() {
        let (mut applied, mut moved) = (0, 0);
        for (seed, size, density) in [(1, 40, 30), (2, 40, 80), (3, 70, 15), (4, 24, 50), (5, 90, 5), (6, 40, 100)] {
            let (a, m, _) = run(u64::wrapping_mul(seed, 0x9e3779b97f4a7c15), size, density, 30, false);
            applied += a;
            moved += m;
        }
        assert!(applied > 100 && moved > 30, "{applied} applied, {moved} moved the bounds or the built columns");
    }

    #[test]
    fn mostly_light_neutral_patches_equal_fresh_reads_and_relight_only_where_light_changes() {
        let (mut applied, mut unlit) = (0, 0);
        for (seed, size, density) in [(11, 40, 30), (12, 40, 80), (13, 70, 15), (14, 24, 50), (15, 90, 5), (16, 40, 100)] {
            let (a, _, u) = run(u64::wrapping_mul(seed, 0x9e3779b97f4a7c15), size, density, 40, true);
            applied += a;
            unlit += u;
        }
        // Each patch is checked against the columns its light-changing edits reach (none: none relit).
        assert!(unlit > 50 && applied > unlit + 20, "{unlit} of {applied} applied patches relit nothing");
    }

    // A volume larger than the bounds lights every cell of the bounds' own
    // volume as it does (what `resize` keeps).
    #[test]
    fn a_larger_volume_holds_the_same_light() {
        let pack = pack();
        for seed in 0..36u64 {
            let mut random = Random(u64::wrapping_mul(seed + 101, 0x9e3779b97f4a7c15));
            let model = if seed % 2 == 0 { random_model(&mut random, [8, 30, 60][seed as usize % 3], 40) } else { tall_model(&mut random, [8, 30, 70][seed as usize % 3]) };
            let tight = bounds(&model);
            let mut wide = tight;
            for axis in 0..6 {
                let grow = [0, 1, 5, 17, 40][random.below(5) as usize];
                wide[axis] += if axis < 3 { -grow } else { grow };
            }
            let small = World::read(&pack, &blueprint(&model, 0).0, MASK).unwrap();
            let large = read_in(&pack, &model, 0, wide);
            let ([min_x, min_y, min_z], [sx, sy, sz]) = small.volume();
            for y in min_y..min_y + sy {
                for z in min_z..min_z + sz {
                    for x in min_x..min_x + sx {
                        assert_eq!(small.light([x, y, z]), large.light([x, y, z]), "seed {seed}: light at {x},{y},{z} in {wide:?}");
                    }
                }
            }
        }
    }

    // Water falling from a ledge to the world's floor, a block at a time,
    // then drying up from the top: each patch grows or shrinks the bounds.
    #[test]
    fn a_column_falling_to_the_floor_and_drying_up_patches_as_fresh_reads() {
        let pack = pack();
        let mut model = random_model(&mut Random(99), 30, 40);
        let (base, mut palette) = blueprint(&model, 0);
        let mut world = World::read(&pack, &base, MASK).unwrap();
        let (x, z) = (31, 12);
        let top = 70;
        let mut step = |model: &mut Model, at: (i32, i32, i32), name: usize, world: &mut World| {
            let mut next = model.clone();
            if name == 0 { next.remove(&at) } else { next.insert(at, name) };
            let both = union(bounds(model), bounds(&next));
            let (before, after) = (read_in(&pack, model, 0, both), read_in(&pack, &next, 0, both));
            let tiles = world.patch(&pack, &patch_bytes(&next, &mut palette, &[(at, name)])).unwrap();
            *model = next;
            assert_same(world, &World::read(&pack, &blueprint(model, 0).0, MASK).unwrap(), &format!("{at:?}"));
            assert_eq!(tiles, crate::mesh::changed(&before, &after), "{at:?}: changed tiles");
        };
        for y in (-64..=top).rev() {
            step(&mut model, (x, y, z), 6, &mut world);
        }
        assert_reference_light(&pack, &world, "fallen");
        for y in -64..=top {
            step(&mut model, (x, y, z), 0, &mut world);
        }
        assert_reference_light(&pack, &world, "dried");
    }

    // Tall worlds with wide gaps between their blocks: roofs and floating
    // slabs high over the ground, caves, lights, water and leaves, and
    // columns of the volume with nothing in them.
    fn tall_model(random: &mut Random, size: i32) -> Model {
        let mut model = Model::new();
        for x in 0..size {
            for z in 0..size {
                model.insert((x, -60, z), 2);
                if random.next() % 9 == 0 {
                    model.insert((x, -59, z), [4, 5, 6, 8][random.below(4) as usize]);
                }
            }
        }
        for _ in 0..1 + random.below(4) {
            let (x0, z0, w, d) = (random.below(size as u64), random.below(size as u64), 1 + random.below(40), 1 + random.below(40));
            let y = -40 + random.below(300);
            let name = [2, 7, 8, 6, 3][random.below(5) as usize];
            for x in x0..(x0 + w).min(size) {
                for z in z0..(z0 + d).min(size) {
                    if random.next() % 13 != 0 {
                        model.insert((x, y, z), name);
                    }
                }
            }
            if random.next() % 2 == 0 {
                model.insert((x0, y - 1 - random.below(30), z0), [4, 5][random.below(2) as usize]);
            }
        }
        // A cave: stone with a hollow lit by a torch.
        let (cx, cz) = (random.below(size as u64), random.below(size as u64));
        for x in cx - 6..cx + 6 {
            for y in -58..-46 {
                for z in cz - 6..cz + 6 {
                    let edge = (x - cx).abs() > 3 || (z - cz).abs() > 3 || !(-56..-49).contains(&y);
                    if edge {
                        model.insert((x, y, z), 2);
                    }
                }
            }
        }
        model.insert((cx, -55, cz), 5);
        model
    }

    #[test]
    fn tall_worlds_with_gaps_are_lit_as_minecraft_lights_every_cell() {
        let pack = pack();
        for seed in 0..24u64 {
            let mut random = Random(u64::wrapping_mul(seed + 31, 0x9e3779b97f4a7c15));
            let size = [8, 30, 70][seed as usize % 3];
            let model = tall_model(&mut random, size);
            let world = World::read(&pack, &blueprint(&model, seed as usize % 2).0, MASK).unwrap();
            assert_reference_light(&pack, &world, &format!("seed {seed}"));
            // A patch at the top and one past it relight, and still match.
            let mut world = world;
            let (_, mut palette) = blueprint(&model, seed as usize % 2);
            let mut model = model;
            let top = bounds(&model)[4];
            for at in [(size / 2, top, size / 2), (size / 2, top + 9, size / 2)] {
                let mut next = model.clone();
                let name = if next.get(&at) == Some(&2) { 0 } else { 2 };
                if name == 0 { next.remove(&at) } else { next.insert(at, name) };
                world.patch(&pack, &patch_bytes(&next, &mut palette, &[(at, name)])).unwrap();
                model = next;
                assert_reference_light(&pack, &world, &format!("seed {seed}, patched at {at:?}"));
            }
        }
    }

    #[test]
    fn patches_build_and_empty_chunk_columns() {
        let pack = pack();
        // Chunk columns 0, 1 and 3 are built; 2 is not.
        let mut model = Model::new();
        for x in [0, 1, 20, 21, 60] {
            model.insert((x, 64, 0), 2);
        }
        model.insert((60, 65, 0), 2);
        model.insert((0, 65, 0), 2);
        let (base, mut palette) = blueprint(&model, 0);
        let mut world = World::read(&pack, &base, MASK).unwrap();
        let read = |model: &Model| World::read(&pack, &blueprint(model, 0).0, MASK).unwrap();
        let mut apply = |world: &mut World, model: &mut Model, edits: &[((i32, i32, i32), usize)], what: &str| {
            let mut next = model.clone();
            for &(at, name) in edits {
                if name == 0 { next.remove(&at) } else { next.insert(at, name) };
            }
            let before = read(model);
            let tiles = world.patch(&pack, &patch_bytes(&next, &mut palette, edits)).unwrap();
            assert_same(world, &read(&next), what);
            let fresh = read(&next);
            for tile in crate::mesh::tiles(&fresh) {
                if crate::mesh::tiles(&before).binary_search(&tile).is_err() {
                    assert!(tiles.contains(&tile), "{what}: new tile {tile:?} drawn");
                }
            }
            *model = next;
        };
        apply(&mut world, &mut model, &[((60, 64, 0), 0), ((59, 64, 0), 2)], "a block moved in a column");
        apply(&mut world, &mut model, &[((20, 64, 0), 0), ((21, 64, 0), 0), ((40, 64, 0), 2)], "columns swapped");
        apply(&mut world, &mut model, &[((80, 64, 0), 2)], "a column built past the bounds");
        apply(&mut world, &mut model, &[((40, 64, 0), 0)], "a column emptied");
        apply(&mut world, &mut model, &[((80, 64, 0), 0), ((1, 70, 0), 4)], "the bounds shrink and grow");
        // A patch whose count of built columns is not its blocks' fails and changes nothing.
        let mut wrong = model.clone();
        wrong.insert((100, 64, 100), 2);
        let mut bytes = patch_bytes(&wrong, &mut palette.clone(), &[((100, 64, 100), 2)]);
        let at = bytes.len() - 22;
        bytes[at..at + 4].copy_from_slice(&7i32.to_be_bytes());
        assert!(world.patch(&pack, &bytes).unwrap_err().contains("chunk columns"));
        assert_same(&world, &read(&model), "a patch of the wrong column count");
    }

    #[test]
    fn a_failed_patch_changes_nothing() {
        let pack = pack();
        let mut model = Model::new();
        for x in [0, 20] {
            model.insert((x, 64, 0), 2);
        }
        let (base, palette) = blueprint(&model, 0);
        let mut world = World::read(&pack, &base, MASK).unwrap();
        let mut next = model.clone();
        next.insert((5, 64, 0), 4);
        let good = patch_bytes(&next, &mut palette.clone(), &[((5, 64, 0), 4)]);
        // Ends early, trailing bytes, an unknown block, an index past the palette, a block outside the bounds.
        let mut trailing = good.clone();
        trailing.push(0);
        let early = &good[..good.len() - 1];
        let mut unknown = good.clone();
        unknown[10] = b'x';
        let mut index = good.clone();
        let end = index.len();
        index[end - 2..].copy_from_slice(&9u16.to_be_bytes());
        let mut outside = good.clone();
        outside[end - 14..end - 10].copy_from_slice(&500i32.to_be_bytes());
        for bytes in [trailing, early.to_vec(), unknown, index, outside] {
            assert!(world.patch(&pack, &bytes).is_err());
            assert_same(&world, &World::read(&pack, &base, MASK).unwrap(), "after a failed patch");
        }
        world.patch(&pack, &good).unwrap();
    }

    #[test]
    fn entries_that_change_nothing_change_no_tile() {
        let pack = pack();
        let mut model = random_model(&mut Random(7), 40, 60);
        model.insert((10, 70, 10), 4);
        let (base, palette) = blueprint(&model, 0);
        let mut world = World::read(&pack, &base, MASK).unwrap();
        let mut edits: Vec<_> = model.iter().step_by(7).map(|(&at, &name)| (at, name)).collect();
        edits.push(((3, 65, 3), 0));
        let bytes = patch_bytes(&model, &mut palette.clone(), &edits);
        assert!(world.patch(&pack, &bytes).unwrap().is_empty());
        assert_same(&world, &World::read(&pack, &base, MASK).unwrap(), "no changes");
    }

    // A snapshot's world reads as the world over every position its tile reads.
    #[test]
    fn a_snapshot_reads_as_the_world_where_its_tile_reads() {
        let pack = pack();
        for seed in 0..6u64 {
            let mut random = Random(u64::wrapping_mul(seed + 7, 0x9e3779b97f4a7c15));
            let model = if seed % 2 == 0 { random_model(&mut random, 70, 30) } else { tall_model(&mut random, 40) };
            let world = World::read(&pack, &blueprint(&model, 0).0, Mask { min_x: 3, max_x: 50, min_z: -100, max_z: 40 }).unwrap();
            let ([_, min_y, _], [_, size_y, _]) = world.volume();
            let mut bytes = Vec::new();
            for (tx, tz) in crate::mesh::tiles(&world).into_iter().chain([(9, 9)]) {
                world.snapshot(tx, tz, &mut bytes);
                let (copy, x, z) = World::from_snapshot(&bytes).unwrap();
                assert_eq!((x, z, copy.mask == world.mask), (tx, tz, true));
                let ((x0, x1), (z0, z1)) = (crate::mesh::reads(tx), crate::mesh::reads(tz));
                for x in x0..=x1 {
                    for z in z0..=z1 {
                        assert_eq!(copy.span(x, z), world.span(x, z), "seed {seed} tile {tx},{tz}: span of {x},{z}");
                        for y in min_y - 2..min_y + size_y + 2 {
                            assert!(copy.look([x, y, z]) == world.look([x, y, z]) && copy.block([x, y, z]) == world.block([x, y, z]), "seed {seed} tile {tx},{tz}: {x},{y},{z}");
                        }
                    }
                }
            }
        }
    }

    // A blueprint read in parts of any length is the blueprint read whole.
    #[test]
    fn a_blueprint_read_in_parts_is_read_whole() {
        let pack = pack();
        let model = tall_model(&mut Random(5), 30);
        let (bytes, _) = blueprint(&model, 1);
        let whole = World::read(&pack, &bytes, MASK).unwrap();
        for part in [1, 7, 100, 8204, 9000, 1 << 20] {
            let mut load = Load::new(MASK);
            for piece in bytes.chunks(part) {
                load.take(&pack, piece).unwrap();
            }
            assert_same(&load.finish(&pack).unwrap(), &whole, &format!("parts of {part}"));
        }
        let mut load = Load::new(MASK);
        load.take(&pack, &bytes[..bytes.len() - 1]).unwrap();
        assert_eq!(load.finish(&pack).err().unwrap(), "The blueprint ends early");
        let mut load = Load::new(MASK);
        assert_eq!(load.take(&pack, &[bytes.clone(), vec![0]].concat()).unwrap_err(), "The blueprint has trailing bytes");
    }

    #[test]
    fn a_world_spread_over_a_wide_tall_box_keeps_memory_by_section() {
        let pack = pack();
        let mut model = Model::new();
        // Two floors far apart and a pillar 960 blocks tall between them.
        for x in 0..20 {
            for z in 0..20 {
                model.insert((x - 600, -60, z), 2);
                model.insert((x + 600, -60, z + 300), 2);
            }
        }
        for y in -60..900 {
            model.insert((0, y, 0), 2);
        }
        let world = World::read(&pack, &blueprint(&model, 0).0, MASK).unwrap();
        let held = world.sections.iter().filter(|section| section.blocks.is_some()).count();
        let lit = world.sections.iter().filter(|section| matches!(section.sky, Level::Cells(_)) || matches!(section.glow, Level::Cells(_))).count();
        assert!(held <= 70 && lit <= 150, "{held} sections hold blocks, {lit} hold light cell by cell");
        assert_eq!(world.light([1, 500, 0]), (15, 0));
        assert_eq!(world.light([0, 901, 0]), (15, 0));
        assert_eq!(world.light([0, 500, 0]), (0, 0));
    }
}
