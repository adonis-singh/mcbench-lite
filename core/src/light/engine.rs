// Moonrise's Starlight (StarLightEngine, BlockStarLightEngine and
// SkyStarLightEngine), ported from the pinned server's own code, line for
// line: the same queues, flags, orders and caches, so that the light it
// leaves is the Java engine's, nibble for nibble and state for state.
//
// One `Run` is one engine instance working on one chunk and its neighbours
// (Java's engines are reused thread-locals with no state between runs). The
// chunk cache holds the chunks around the centre that can be lit, here as
// their light data taken out of the lighting's map for the run.

use super::nibble::{self, Nibble};
use super::{ChunkLight, Pending};
use crate::fast::FastMap;
use crate::registry::{Registry, State};
use crate::world::{Section, World};

pub const MIN_SECTION: i32 = -4;
pub const MAX_SECTION: i32 = 59;
pub const MIN_LIGHT: i32 = MIN_SECTION - 1;
pub const MAX_LIGHT: i32 = MAX_SECTION + 1;
pub const SECTIONS: usize = (MAX_SECTION - MIN_SECTION + 1) as usize;
pub const LIGHT_SECTIONS: usize = (MAX_LIGHT - MIN_LIGHT + 1) as usize;
const CACHE_LEN: usize = 25 * (LIGHT_SECTIONS + 2);

const FLAG_WRITE_LEVEL: u64 = 1 << 61;
const FLAG_RECHECK_LEVEL: u64 = 1 << 62;
const FLAG_HAS_SIDED_TRANSPARENT_BLOCKS: u64 = 1 << 63;

// AxisDirection: +x, -x, +z, -z, +y, -y, with Direction.ordinal() of the
// Minecraft direction each stands for (east, west, south, north, up, down).
#[derive(Clone, Copy)]
struct AxisDirection {
    x: i32,
    y: i32,
    z: i32,
    nms: usize,
}

const DIRECTIONS: [AxisDirection; 6] = [
    AxisDirection { x: 1, y: 0, z: 0, nms: 5 },
    AxisDirection { x: -1, y: 0, z: 0, nms: 4 },
    AxisDirection { x: 0, y: 0, z: 1, nms: 3 },
    AxisDirection { x: 0, y: 0, z: -1, nms: 2 },
    AxisDirection { x: 0, y: 1, z: 0, nms: 1 },
    AxisDirection { x: 0, y: -1, z: 0, nms: 0 },
];
const ONLY_HORIZONTAL: [usize; 4] = [0, 1, 2, 3];
const POSITIVE_Y: usize = 4;
const NEGATIVE_Y: usize = 5;

// everythingButThisDirection and everythingButTheOppositeDirection.
const fn but_this(direction: usize) -> u64 {
    63 ^ (1 << direction)
}
const fn but_opposite(direction: usize) -> u64 {
    63 ^ (1 << (direction ^ 1))
}


// A section of the caches as the propagation loops read it (Run::fast_table):
// its nibble's bytes (`read` null where the nibble cache holds none; a nibble
// with no working array reads the zeroes of ZERO_NIBBLE; `write` null until
// its working array is its own) and its blocks (AIR where the section cache
// holds none).
#[derive(Clone, Copy)]
struct Cell {
    read: *const u8,
    write: *mut u8,
    blocks: *const State,
}

static ZERO_NIBBLE: [u8; nibble::SIZE] = [0; nibble::SIZE];
static AIR: [State; 4096] = [0; 4096];

impl Cell {
    const ABSENT: Cell = Cell { read: std::ptr::null(), write: std::ptr::null_mut(), blocks: std::ptr::null() };

    #[inline(always)]
    fn present(&self) -> bool {
        !self.read.is_null()
    }

    // getUpdating of a present nibble, `local` < 4096.
    #[inline(always)]
    fn get(&self, local: usize) -> i32 {
        debug_assert!(self.present() && local < 4096);
        // SAFETY: a present cell reads a SIZE-byte array, and local >> 1 < SIZE.
        let byte = unsafe { *self.read.add(local >> 1) };
        (byte >> ((local & 1) << 2) & 15) as i32
    }

    // The light level as getLightLevel reads it: 0 where there is no nibble.
    #[inline(always)]
    fn level(&self, local: usize) -> i32 {
        if self.present() { self.get(local) } else { 0 }
    }

    // The block of a present cell at `local` < 4096.
    #[inline(always)]
    fn block(&self, local: usize) -> State {
        debug_assert!(self.present() && local < 4096);
        // SAFETY: a present cell's blocks are a 4096-state array.
        unsafe { *self.blocks.add(local) }
    }
}

// The cache index of the section holding x, y, z, and the position's index in it.
#[inline(always)]
fn cell_index(x: i32, y: i32, z: i32, section_offset: i32) -> usize {
    ((x >> 4) + 5 * (z >> 4) + 25 * (y >> 4) + section_offset) as usize
}

#[inline(always)]
fn local_index(x: i32, y: i32, z: i32) -> usize {
    (x & 15 | (z & 15) << 4 | (y & 15) << 8) as usize
}

// What a cache entry stands for in the nibble cache: nothing, the nibble the
// chunk holds at that section, or a nibble made for the run alone (Starlight
// makes these for sections the chunk holds none of, and they are not kept).
#[derive(Clone, Copy)]
enum NibRef {
    None,
    // The nibble at this place in the layer of the chunk in this cache slot.
    Real { slot: u8, layer: u8 },
    Temp(u32),
}

pub struct Run<'a> {
    reg: &'a Registry,
    world: &'a World,
    sky: bool,
    emitted_mask: i32,
    keys: [i64; 25],
    slots: [Option<ChunkLight>; 25],
    chunk_in_cache: [bool; 25],
    section_cache: Vec<Option<&'a [State; 4096]>>,
    nibble_cache: Vec<NibRef>,
    temps: Vec<Nibble>,
    encode_offset_x: i32,
    encode_offset_y: i32,
    encode_offset_z: i32,
    coordinate_offset: i32,
    chunk_offset_x: i32,
    chunk_offset_y: i32,
    chunk_offset_z: i32,
    chunk_index_offset: i32,
    chunk_section_index_offset: i32,
    increase_queue: Vec<u64>,
    increase_len: usize,
    decrease_queue: Vec<u64>,
    decrease_len: usize,
    delayed_center: [i32; 256],
    delayed_neighbour: [i32; 256],
    null_propagation_check: Vec<bool>,
    height_map_block_change: [i32; 256],
    // The caches' bytes for the propagation loops (fast_table).
    fast: Vec<Cell>,
}

impl<'a> Run<'a> {
    pub fn new(reg: &'a Registry, world: &'a World, sky: bool) -> Run<'a> {
        Run {
            reg,
            world,
            sky,
            emitted_mask: if sky { 0 } else { 15 },
            keys: [0; 25],
            slots: std::array::from_fn(|_| None),
            chunk_in_cache: [false; 25],
            section_cache: vec![None; CACHE_LEN],
            nibble_cache: vec![NibRef::None; CACHE_LEN],
            temps: Vec::new(),
            encode_offset_x: 0,
            encode_offset_y: 0,
            encode_offset_z: 0,
            coordinate_offset: 0,
            chunk_offset_x: 0,
            chunk_offset_y: 0,
            chunk_offset_z: 0,
            chunk_index_offset: 0,
            chunk_section_index_offset: 0,
            increase_queue: vec![0; 4096],
            increase_len: 0,
            decrease_queue: vec![0; 4096],
            decrease_len: 0,
            delayed_center: [0; 256],
            delayed_neighbour: [0; 256],
            null_propagation_check: vec![false; LIGHT_SECTIONS],
            height_map_block_change: [i32::MIN; 256],
            fast: vec![Cell::ABSENT; CACHE_LEN],
        }
    }

    // Between runs an engine holds no queued light and no caches.
    pub fn assert_idle(&self) {
        assert!(self.increase_len == 0 && self.decrease_len == 0, "light queued past the end of a run");
        assert!(!self.chunk_in_cache.iter().any(|&cached| cached) && self.temps.is_empty(), "caches held past the end of a run");
    }

    // --- Caches (StarLightEngine) ---

    fn setup_encode_offset(&mut self, center_x: i32, _center_y: i32, center_z: i32) {
        self.encode_offset_x = 31 - center_x;
        self.encode_offset_y = (-(MIN_LIGHT - 1)) << 4;
        self.encode_offset_z = 31 - center_z;
        self.coordinate_offset = self.encode_offset_x.wrapping_add(self.encode_offset_z << 6).wrapping_add(self.encode_offset_y << 12);
        self.chunk_offset_x = 2 - (center_x >> 4);
        self.chunk_offset_y = -(MIN_LIGHT - 1);
        self.chunk_offset_z = 2 - (center_z >> 4);
        self.chunk_index_offset = self.chunk_offset_x + 5 * self.chunk_offset_z;
        self.chunk_section_index_offset = self.chunk_index_offset + 25 * self.chunk_offset_y;
    }

    // The chunk (x, z) at its cache slot.
    #[inline]
    fn slot_of(&self, chunk_x: i32, chunk_z: i32) -> usize {
        let at = chunk_x + 5 * chunk_z + self.chunk_index_offset;
        at as usize
    }

    pub fn setup_caches(&mut self, lights: &mut FastMap<i64, ChunkLight>, center_x: i32, center_y: i32, center_z: i32, relaxed: bool, two_radius: bool) {
        let center_chunk_x = center_x >> 4;
        let center_chunk_y = center_y >> 4;
        let center_chunk_z = center_z >> 4;
        self.setup_encode_offset(center_chunk_x * 16 + 7, center_chunk_y * 16 + 7, center_chunk_z * 16 + 7);
        let radius = if two_radius { 2 } else { 1 };
        for dz in -radius..=radius {
            for dx in -radius..=radius {
                let cx = center_chunk_x + dx;
                let cz = center_chunk_z + dz;
                let is_two_radius = dx.abs().max(dz.abs()) == 2;
                if !self.world.has_chunk(cx, cz) {
                    if !(relaxed | is_two_radius) {
                        panic!("java.lang.IllegalArgumentException: Trying to propagate light update before 1 radius neighbours ready");
                    }
                    continue;
                }
                let key = crate::dir::chunk_key(cx, cz);
                // canUseChunk: a chunk at LIGHT status whose light is correct.
                if !lights.get(&key).map_or(false, |light| light.light_correct) {
                    continue;
                }
                let slot = self.slot_of(cx, cz);
                self.keys[slot] = key;
                self.slots[slot] = lights.remove(&key);
                self.chunk_in_cache[slot] = true;
                if !is_two_radius {
                    self.set_blocks_for_chunk_in_cache(cx, cz);
                    self.set_nibbles_for_chunk_in_cache(cx, cz);
                }
            }
        }
    }

    // setBlocksForChunkInCache: sections that hold only air are as good as none.
    fn set_blocks_for_chunk_in_cache(&mut self, chunk_x: i32, chunk_z: i32) {
        let chunk = self.world.chunk(crate::dir::chunk_key(chunk_x, chunk_z));
        for cy in MIN_LIGHT..=MAX_LIGHT {
            let section = if cy >= MIN_SECTION && cy <= MAX_SECTION { chunk.sections[(cy - MIN_SECTION) as usize].as_ref() } else { None };
            let section = section.filter(|section| section.non_air != 0).map(|section| &*section.blocks);
            let at = chunk_x + 5 * chunk_z + 25 * cy + self.chunk_section_index_offset;
            self.section_cache[at as usize] = section;
        }
    }

    fn set_nibbles_for_chunk_in_cache(&mut self, chunk_x: i32, chunk_z: i32) {
        for cy in MIN_LIGHT..=MAX_LIGHT {
            let at = chunk_x + 5 * chunk_z + 25 * cy + self.chunk_section_index_offset;
            self.nibble_cache[at as usize] = NibRef::Real { slot: self.slot_of(chunk_x, chunk_z) as u8, layer: (cy - MIN_LIGHT) as u8 };
        }
    }

    // destroyCaches: the chunks' light goes back, and the caches are emptied.
    pub fn destroy_caches(&mut self, lights: &mut FastMap<i64, ChunkLight>) {
        for slot in 0..25 {
            if let Some(light) = self.slots[slot].take() {
                lights.insert(self.keys[slot], light);
            }
        }
        self.chunk_in_cache = [false; 25];
        self.section_cache.fill(None);
        self.nibble_cache.fill(NibRef::None);
        self.temps.clear();
    }

    #[inline]
    fn nib(&self, index: usize) -> Option<&Nibble> {
        match self.nibble_cache[index] {
            NibRef::None => None,
            NibRef::Real { slot, layer } => Some(&self.slots[slot as usize].as_ref().expect("the chunk of a cached nibble").layer(self.sky)[layer as usize]),
            NibRef::Temp(at) => Some(&self.temps[at as usize]),
        }
    }

    #[inline]
    fn nib_mut(&mut self, index: usize) -> Option<&mut Nibble> {
        match self.nibble_cache[index] {
            NibRef::None => None,
            NibRef::Real { slot, layer } => Some(&mut self.slots[slot as usize].as_mut().expect("the chunk of a cached nibble").layer_mut(self.sky)[layer as usize]),
            NibRef::Temp(at) => Some(&mut self.temps[at as usize]),
        }
    }

    #[inline]
    fn nibble_index(&self, chunk_x: i32, chunk_y: i32, chunk_z: i32) -> usize {
        (chunk_x + 5 * chunk_z + 25 * chunk_y + self.chunk_section_index_offset) as usize
    }

    // The caches' bytes, for a propagation loop: the loops change nothing of
    // a nibble but its values (fast_set) and nothing of the caches, so the
    // pointers stay valid until the loop ends. The table goes back into the
    // run after.
    fn fast_table(&mut self) -> Vec<Cell> {
        let mut cells = std::mem::take(&mut self.fast);
        for index in 0..CACHE_LEN {
            cells[index] = match self.nibble_cache[index] {
                NibRef::None => Cell::ABSENT,
                _ => {
                    let (read, write) = self.nib_mut(index).expect("a cached nibble").bytes();
                    let blocks = self.section_cache[index].map_or(AIR.as_ptr(), |blocks| blocks.as_ptr());
                    Cell { read: if read.is_null() { ZERO_NIBBLE.as_ptr() } else { read }, write, blocks }
                }
            };
        }
        cells
    }

    // SWMRNibbleArray.set through the table: in place once the nibble's
    // working array is its own, else by set, which makes it so.
    #[inline(always)]
    fn fast_set(&mut self, cells: &mut [Cell], index: usize, local: usize, value: i32) {
        let cell = cells[index];
        if !cell.write.is_null() {
            let shift = (local & 1) << 2;
            // SAFETY: `write` points at the nibble's own SIZE-byte working array (Nibble::bytes), local < 4096.
            unsafe {
                let at = cell.write.add(local >> 1);
                *at = (*at as u32 & (240 >> shift) | (value as u32) << shift) as u8;
            }
            return;
        }
        self.first_set(cells, index, local, value);
    }

    // A nibble's first write in a loop: set makes its working array its own.
    #[cold]
    #[inline(never)]
    fn first_set(&mut self, cells: &mut [Cell], index: usize, local: usize, value: i32) {
        let nibble = self.nib_mut(index).expect("a cached nibble");
        nibble.set(local, value);
        let (read, write) = nibble.bytes();
        cells[index].read = read;
        cells[index].write = write;
    }

    // updateVisible: every nibble the run changed is made visible.
    fn update_visible(&mut self) {
        for index in 0..self.nibble_cache.len() {
            if let Some(nibble) = self.nib_mut(index) {
                if nibble.is_dirty() {
                    nibble.update_visible();
                }
            }
        }
    }

    fn chunk_in_cache(&self, chunk_x: i32, chunk_z: i32) -> bool {
        self.chunk_in_cache[self.slot_of(chunk_x, chunk_z)]
    }

    #[inline]
    fn block_state(&self, world_x: i32, world_y: i32, world_z: i32) -> State {
        let index = (world_x >> 4) + 5 * (world_z >> 4) + 25 * (world_y >> 4) + self.chunk_section_index_offset;
        match self.section_cache[index as usize] {
            Some(blocks) => blocks[((world_y & 15) << 8 | (world_z & 15) << 4 | world_x & 15) as usize],
            None => 0,
        }
    }

    #[inline]
    fn block_state_at(&self, section_index: i32, local_index: i32) -> State {
        match self.section_cache[section_index as usize] {
            Some(blocks) => blocks[local_index as usize],
            None => 0,
        }
    }

    fn chunk_section(&self, chunk_x: i32, chunk_y: i32, chunk_z: i32) -> Option<&'a [State; 4096]> {
        self.section_cache[(chunk_x + 5 * chunk_z + 25 * chunk_y + self.chunk_section_index_offset) as usize]
    }

    #[inline]
    fn light_level(&self, world_x: i32, world_y: i32, world_z: i32) -> i32 {
        let index = self.nibble_index(world_x >> 4, world_y >> 4, world_z >> 4);
        match self.nib(index) {
            None => 0,
            Some(nibble) => nibble.get_updating((world_x & 15 | (world_z & 15) << 4 | (world_y & 15) << 8) as usize),
        }
    }

    #[inline]
    fn light_level_at(&self, section_index: i32, local_index: i32) -> i32 {
        match self.nib(section_index as usize) {
            None => 0,
            Some(nibble) => nibble.get_updating(local_index as usize),
        }
    }

    #[inline]
    fn set_light_level(&mut self, world_x: i32, world_y: i32, world_z: i32, level: i32) {
        let index = self.nibble_index(world_x >> 4, world_y >> 4, world_z >> 4);
        if let Some(nibble) = self.nib_mut(index) {
            nibble.set((world_x & 15 | (world_z & 15) << 4 | (world_y & 15) << 8) as usize, level);
        }
    }

    // The emptiness map of a chunk in the cache.
    fn emptiness(&self, chunk_x: i32, chunk_z: i32) -> Option<&Vec<bool>> {
        let slot = self.slot_of(chunk_x, chunk_z);
        if !self.chunk_in_cache[slot] {
            return None;
        }
        self.slots[slot].as_ref().and_then(|light| light.empty(self.sky).as_ref())
    }

    fn emptiness_mut(&mut self, chunk_x: i32, chunk_z: i32) -> &mut Vec<bool> {
        let slot = self.slot_of(chunk_x, chunk_z);
        self.slots[slot].as_mut().unwrap().empty_mut(self.sky).as_mut().unwrap()
    }

    // --- Empty sections ---

    fn init_nibble(&mut self, chunk_x: i32, chunk_y: i32, chunk_z: i32, extrude: bool, init_removed_nibbles: bool) {
        if chunk_y < MIN_LIGHT || chunk_y > MAX_LIGHT || !self.chunk_in_cache(chunk_x, chunk_z) {
            return;
        }
        let index = self.nibble_index(chunk_x, chunk_y, chunk_z);
        if self.sky {
            if matches!(self.nibble_cache[index], NibRef::None) {
                if !init_removed_nibbles {
                    panic!("java.lang.IllegalStateException");
                }
                self.temps.push(Nibble::new(true));
                self.nibble_cache[index] = NibRef::Temp(self.temps.len() as u32 - 1);
            }
            self.init_sky_nibble(index, chunk_x, chunk_y, chunk_z, extrude);
        } else {
            match self.nibble_cache[index] {
                NibRef::None => {
                    if !init_removed_nibbles {
                        panic!("java.lang.IllegalStateException");
                    }
                    self.temps.push(Nibble::new(false));
                    self.nibble_cache[index] = NibRef::Temp(self.temps.len() as u32 - 1);
                }
                _ => self.nib_mut(index).unwrap().set_non_null(),
            }
        }
    }

    // SkyStarLightEngine.initNibble(nibble, ...): `index` is the nibble's.
    fn init_sky_nibble(&mut self, index: usize, chunk_x: i32, chunk_y: i32, chunk_z: i32, extrude: bool) {
        if !self.nib(index).unwrap().is_null_updating() {
            return;
        }
        let mut lowest_y = MIN_LIGHT - 1;
        let mut curr_y = MAX_SECTION;
        while curr_y >= MIN_SECTION {
            let found = match self.emptiness(chunk_x, chunk_z) {
                None => self.chunk_section(chunk_x, curr_y, chunk_z).is_some(),
                Some(map) => !map[(curr_y - MIN_SECTION) as usize],
            };
            if found {
                lowest_y = curr_y;
                break;
            }
            curr_y -= 1;
        }
        if chunk_y > lowest_y {
            let nibble = self.nib_mut(index).unwrap();
            nibble.set_non_null();
            nibble.set_full();
        } else if extrude {
            for curr_y in chunk_y + 1..=MAX_LIGHT {
                let above = self.nibble_index(chunk_x, curr_y, chunk_z);
                if let Some(nibble) = self.nib(above) {
                    if !nibble.is_null_updating() {
                        let layer = nibble.bottom_layer();
                        let current = self.nib_mut(index).unwrap();
                        current.set_non_null();
                        current.extrude_lower(layer);
                        break;
                    }
                }
            }
        } else {
            self.nib_mut(index).unwrap().set_non_null();
        }
    }

    fn set_nibble_null(&mut self, chunk_x: i32, chunk_y: i32, chunk_z: i32) {
        let index = self.nibble_index(chunk_x, chunk_y, chunk_z);
        let sky = self.sky;
        if let Some(nibble) = self.nib_mut(index) {
            if sky {
                nibble.set_null();
            } else {
                nibble.set_hidden();
            }
        }
    }

    // StarLightEngine.handleEmptySectionChanges. `changes` is the pending
    // emptiness of the chunk's sections (filled in for a chunk with no map).
    pub fn handle_empty_section_changes(&mut self, chunk_x: i32, chunk_z: i32, changes: &mut [Option<bool>], unlit: bool) {
        let needs_init = unlit || self.emptiness(chunk_x, chunk_z).is_none();
        if needs_init {
            let slot = self.slot_of(chunk_x, chunk_z);
            *self.slots[slot].as_mut().unwrap().empty_mut(self.sky) = Some(vec![false; SECTIONS]);
        }
        for section_index in (0..changes.len()).rev() {
            let value = match changes[section_index] {
                None => {
                    if !needs_init {
                        continue;
                    }
                    let section = self.chunk_section(chunk_x, section_index as i32 + MIN_SECTION, chunk_z);
                    let value = section.is_none();
                    changes[section_index] = Some(value);
                    value
                }
                Some(value) => value,
            };
            self.emptiness_mut(chunk_x, chunk_z)[section_index] = value;
        }
        for section_index in (0..changes.len()).rev() {
            let section_y = section_index as i32 + MIN_SECTION;
            if let Some(empty) = changes[section_index] {
                if !empty {
                    for dz in -1..=1 {
                        for dx in -1..=1 {
                            let extrude = (dx | dz) != 0 || !unlit;
                            for dy in (-1..=1).rev() {
                                self.init_nibble(dx + chunk_x, dy + section_y, dz + chunk_z, extrude, false);
                            }
                        }
                    }
                }
            }
        }
        // Whether each section of the 5x5 chunks around is empty, as bits of
        // a word (section MIN_SECTION + i at bit i): by the chunk's emptiness
        // map if it has one, else by its section (no map is no emptiness).
        const _: () = assert!(SECTIONS == 64);
        let mut empty_bits = [0u64; 25];
        let mut has_map = [false; 25];
        for dz in -2..=2 {
            for dx in -2..=2 {
                let at = ((dz + 2) * 5 + dx + 2) as usize;
                match self.emptiness(dx + chunk_x, dz + chunk_z) {
                    Some(map) => {
                        has_map[at] = true;
                        empty_bits[at] = map.iter().enumerate().fold(0u64, |bits, (i, &empty)| bits | (empty as u64) << i);
                    }
                    None => {
                        empty_bits[at] = (0..SECTIONS as i32).fold(0u64, |bits, i| bits | (self.chunk_section(dx + chunk_x, i + MIN_SECTION, dz + chunk_z).is_none() as u64) << i);
                    }
                }
            }
        }
        for dz in -1..=1 {
            for dx in -1..=1 {
                let mut neighbours_loaded = true;
                let mut all = u64::MAX;
                for dz2 in -1..=1 {
                    for dx2 in -1..=1 {
                        let at = ((dz + dz2 + 2) * 5 + dx + dx2 + 2) as usize;
                        neighbours_loaded &= has_map[at];
                        all &= empty_bits[at];
                    }
                }
                for section_y in (MIN_LIGHT..=MAX_LIGHT).rev() {
                    // The sections from the one below to the one above that exist.
                    let (low, high) = ((section_y - 1).max(MIN_SECTION), (section_y + 1).min(MAX_SECTION));
                    let window = ((1u64 << (high - low + 1)) - 1) << (low - MIN_SECTION);
                    let all_empty = window & !all == 0;
                    if all_empty & neighbours_loaded {
                        self.set_nibble_null(dx + chunk_x, section_y, dz + chunk_z);
                    } else if !all_empty {
                        let extrude = (dx | dz) != 0 || !unlit;
                        self.init_nibble(dx + chunk_x, section_y, dz + chunk_z, extrude, false);
                    }
                }
            }
        }
    }

    // --- Entry points ---

    // blocksChangedInChunk.
    pub fn blocks_changed_in_chunk(&mut self, lights: &mut FastMap<i64, ChunkLight>, chunk_x: i32, chunk_z: i32, pending: &mut Pending) {
        self.setup_caches(lights, chunk_x * 16 + 7, 128, chunk_z * 16 + 7, true, true);
        if self.chunk_in_cache(chunk_x, chunk_z) {
            if pending.sections.iter().any(Option::is_some) {
                self.handle_empty_section_changes(chunk_x, chunk_z, &mut pending.sections, false);
            }
            if !pending.positions.is_empty() {
                self.propagate_block_changes(chunk_x, chunk_z, &pending.positions.to_vec());
            }
            self.update_visible();
        }
        self.destroy_caches(lights);
    }

    // light: a chunk that was just made.
    pub fn light(&mut self, lights: &mut FastMap<i64, ChunkLight>, chunk_x: i32, chunk_z: i32, empty_sections: &mut [Option<bool>]) {
        self.setup_caches(lights, chunk_x * 16 + 7, 128, chunk_z * 16 + 7, true, true);
        let slot = self.slot_of(chunk_x, chunk_z);
        // The chunk itself is not lit yet, so setupCaches did not take it.
        self.keys[slot] = crate::dir::chunk_key(chunk_x, chunk_z);
        self.slots[slot] = Some(lights.remove(&self.keys[slot]).unwrap_or_else(ChunkLight::new));
        self.slots[slot].as_mut().unwrap().reset_nibbles(self.sky);
        self.chunk_in_cache[slot] = true;
        self.set_blocks_for_chunk_in_cache(chunk_x, chunk_z);
        self.set_nibbles_for_chunk_in_cache(chunk_x, chunk_z);
        self.handle_empty_section_changes(chunk_x, chunk_z, empty_sections, true);
        self.light_chunk(chunk_x, chunk_z, true);
        self.update_visible();
        self.destroy_caches(lights);
    }

    // StarLightEngine.relightChunks: the chunks of `columns`, in order, lit
    // again from their blocks alone. Each chunk and its neighbours (in
    // Starlight's order) that can be used are lit once each, into fresh
    // nibbles, with only chunks this relight lit already in the cache; a chunk
    // of `columns` takes its emptiness map when it is lit and its fresh
    // nibbles once it and its neighbours are lit (after which nothing that is
    // lit reaches it: only the chunks around a chunk light into it). Returns
    // how many chunks were relit.
    pub fn relight_chunks(&mut self, lights: &mut FastMap<i64, ChunkLight>, columns: &[(i32, i32)]) -> usize {
        const ORDER: [(i32, i32); 9] = [(0, 0), (-1, 0), (0, -1), (1, 0), (0, 1), (-1, 1), (1, 1), (-1, -1), (1, -1)];
        // canUseChunk: a chunk at LIGHT status whose light is correct.
        let world = self.world;
        let usable = |lights: &FastMap<i64, ChunkLight>, x: i32, z: i32| {
            world.has_chunk(x, z) && lights.get(&crate::dir::chunk_key(x, z)).map_or(false, |light| light.light_correct)
        };
        let wanted: std::collections::HashSet<i64> = columns.iter().map(|&(x, z)| crate::dir::chunk_key(x, z)).collect();
        let mut relit: FastMap<i64, ChunkLight> = FastMap::default();
        let mut calls = 0;
        for &(chunk_x, chunk_z) in columns {
            if !usable(lights, chunk_x, chunk_z) {
                panic!("java.lang.IllegalStateException");
            }
            for (dx, dz) in ORDER {
                let (x, z) = (chunk_x + dx, chunk_z + dz);
                let key = crate::dir::chunk_key(x, z);
                if !usable(lights, x, z) || relit.contains_key(&key) {
                    continue;
                }
                self.setup_encode_offset(x * 16 + 7, 128, z * 16 + 7);
                // The neighbours this relight lit already, with what it gave them.
                for dz2 in -1..=1 {
                    for dx2 in -1..=1 {
                        let (x2, z2) = (x + dx2, z + dz2);
                        if !usable(lights, x2, z2) {
                            continue;
                        }
                        let key2 = crate::dir::chunk_key(x2, z2);
                        let Some(light) = relit.remove(&key2) else { continue };
                        let slot = self.slot_of(x2, z2);
                        self.keys[slot] = key2;
                        self.slots[slot] = Some(light);
                        self.chunk_in_cache[slot] = true;
                        self.set_blocks_for_chunk_in_cache(x2, z2);
                        self.set_nibbles_for_chunk_in_cache(x2, z2);
                    }
                }
                // The chunk itself, with getFilledEmptyLight's nibbles.
                let slot = self.slot_of(x, z);
                self.keys[slot] = key;
                self.slots[slot] = Some(ChunkLight::new());
                self.chunk_in_cache[slot] = true;
                self.set_blocks_for_chunk_in_cache(x, z);
                self.set_nibbles_for_chunk_in_cache(x, z);
                let chunk = self.world.chunk(key);
                let mut empty: Vec<Option<bool>> = chunk.sections.iter().map(|section| Some(section.as_ref().map_or(true, |s| s.non_air == 0))).collect();
                self.handle_empty_section_changes(x, z, &mut empty, true);
                if wanted.contains(&key) {
                    let emptiness = self.slots[slot].as_ref().unwrap().empty(self.sky).clone();
                    *lights.get_mut(&key).unwrap().empty_mut(self.sky) = emptiness;
                }
                self.light_chunk(x, z, false);
                self.destroy_caches(&mut relit);
            }
            let key = crate::dir::chunk_key(chunk_x, chunk_z);
            let fresh = relit.get_mut(&key).expect("a relit chunk's nibbles");
            for nibble in fresh.layer_mut(self.sky).iter_mut() {
                if nibble.is_dirty() {
                    nibble.update_visible();
                }
            }
            *lights.get_mut(&key).unwrap().layer_mut(self.sky) = std::mem::take(fresh.layer_mut(self.sky));
            calls += 1;
        }
        calls
    }

    fn propagate_block_changes(&mut self, chunk_x: i32, chunk_z: i32, positions: &[crate::dir::Pos]) {
        if self.sky {
            self.sky_propagate_block_changes(chunk_x, chunk_z, positions);
        } else {
            for pos in positions {
                self.check_block(pos.x(), pos.y(), pos.z());
            }
            self.perform_light_decrease();
        }
    }

    fn light_chunk(&mut self, chunk_x: i32, chunk_z: i32, needs_edge_checks: bool) {
        if self.sky {
            self.sky_light_chunk(chunk_x, chunk_z, needs_edge_checks);
        } else {
            self.block_light_chunk(chunk_x, chunk_z, needs_edge_checks);
        }
    }

    fn check_block(&mut self, world_x: i32, world_y: i32, world_z: i32) {
        if self.sky {
            self.sky_check_block(world_x, world_y, world_z);
        } else {
            self.block_check_block(world_x, world_y, world_z);
        }
    }

    fn calculate_light_value(&self, world_x: i32, world_y: i32, world_z: i32, expect: i32) -> i32 {
        if self.sky {
            self.sky_calculate_light_value(world_x, world_y, world_z, expect)
        } else {
            self.block_calculate_light_value(world_x, world_y, world_z, expect)
        }
    }

    // --- Queues ---

    fn append_to_increase_queue(&mut self, value: u64) {
        let index = self.increase_len;
        self.increase_len += 1;
        if index >= self.increase_queue.len() {
            self.resize_increase_queue();
        }
        self.increase_queue[index] = value;
    }

    fn append_to_decrease_queue(&mut self, value: u64) {
        let index = self.decrease_len;
        self.decrease_len += 1;
        if index >= self.decrease_queue.len() {
            self.resize_decrease_queue();
        }
        self.decrease_queue[index] = value;
    }

    fn resize_increase_queue(&mut self) {
        let len = self.increase_queue.len();
        self.increase_queue.resize((len + (len >> 1)).max(4), 0);
    }

    fn resize_decrease_queue(&mut self) {
        let len = self.decrease_queue.len();
        self.decrease_queue.resize((len + (len >> 1)).max(4), 0);
    }

    // (long)(x + (z << 6) + (y << 12) + encodeOffset) & 268435455L
    #[inline]
    fn encode(x: i32, y: i32, z: i32, encode_offset: i32) -> u64 {
        (x.wrapping_add(z << 6).wrapping_add(y << 12).wrapping_add(encode_offset) as i64 as u64) & 268435455
    }

    // --- Occlusion ---

    #[inline]
    fn conditional(&self, state: State) -> bool {
        self.reg.light_info(state) & 256 != 0
    }

    // getFaceOcclusionShape of a conditionally full opaque state.
    #[inline]
    fn face(&self, state: State, direction: usize) -> u8 {
        self.reg.occlusion.faces[state as usize][direction]
    }

    #[inline]
    fn emission(&self, state: State) -> i32 {
        (self.reg.light_info(state) >> 4 & 15) as i32
    }

    #[inline]
    fn opacity(&self, state: State) -> i32 {
        (self.reg.light_info(state) & 15) as i32
    }

    // --- Edges ---

    fn check_chunk_edge(&mut self, chunk_x: i32, chunk_y: i32, chunk_z: i32) {
        let center_index = self.nibble_index(chunk_x, chunk_y, chunk_z);
        if self.nib(center_index).is_none() {
            return;
        }
        for &direction in &ONLY_HORIZONTAL {
            let d = DIRECTIONS[direction];
            let (neighbour_off_x, neighbour_off_z) = (d.x, d.z);
            let neighbour_index = self.nibble_index(chunk_x + neighbour_off_x, chunk_y, chunk_z + neighbour_off_z);
            let Some(neighbour) = self.nib(neighbour_index) else { continue };
            if !(self.nib(center_index).unwrap().is_initialised_updating() || neighbour.is_initialised_updating()) {
                continue;
            }
            let (inc_x, inc_z, start_x, start_z);
            if neighbour_off_x != 0 {
                inc_x = 0;
                inc_z = 1;
                start_x = if d.x < 0 { chunk_x << 4 } else { chunk_x << 4 | 15 };
                start_z = chunk_z << 4;
            } else {
                inc_x = 1;
                inc_z = 0;
                start_z = if neighbour_off_z < 0 { chunk_z << 4 } else { chunk_z << 4 | 15 };
                start_x = chunk_x << 4;
            }
            let mut center_delayed_checks = 0usize;
            let mut neighbour_delayed_checks = 0usize;
            let mut curr_y = chunk_y << 4;
            let max_y = curr_y | 15;
            while curr_y <= max_y {
                let mut i = 0;
                let mut curr_x = start_x;
                let mut curr_z = start_z;
                while i < 16 {
                    let neighbour_x = curr_x + neighbour_off_x;
                    let neighbour_z = curr_z + neighbour_off_z;
                    let current_index = curr_x & 15 | (curr_z & 15) << 4 | (curr_y & 15) << 8;
                    let current_level = self.nib(center_index).unwrap().get_updating(current_index as usize);
                    let neighbour_local = neighbour_x & 15 | (neighbour_z & 15) << 4 | (curr_y & 15) << 8;
                    let neighbour_level = self.nib(neighbour_index).unwrap().get_updating(neighbour_local as usize);
                    if self.calculate_light_value(curr_x, curr_y, curr_z, current_level) != current_level {
                        self.delayed_center[center_delayed_checks] = current_index;
                        center_delayed_checks += 1;
                    }
                    if self.calculate_light_value(neighbour_x, curr_y, neighbour_z, neighbour_level) != neighbour_level {
                        self.delayed_neighbour[neighbour_delayed_checks] = neighbour_local;
                        neighbour_delayed_checks += 1;
                    }
                    i += 1;
                    curr_x += inc_x;
                    curr_z += inc_z;
                }
                curr_y += 1;
            }
            let curr_x0 = chunk_x << 4;
            let current_chunk_off_z = chunk_z << 4;
            let neighbour_chunk_off_x = (chunk_x + d.x) << 4;
            let neighbour_chunk_off_z = (chunk_z + d.z) << 4;
            let chunk_off_y = chunk_y << 4;
            let len = center_delayed_checks.max(neighbour_delayed_checks);
            for i in 0..len {
                if i < center_delayed_checks {
                    let value = self.delayed_center[i];
                    self.check_block(curr_x0 | value & 15, chunk_off_y | value >> 8, current_chunk_off_z | (value >> 4) & 15);
                }
                if i < neighbour_delayed_checks {
                    let value = self.delayed_neighbour[i];
                    self.check_block(neighbour_chunk_off_x | value & 15, chunk_off_y | value >> 8, neighbour_chunk_off_z | (value >> 4) & 15);
                }
            }
        }
    }

    // StarLightEngine.checkChunkEdges(lightAccess, chunk, fromSection, toSection).
    fn base_check_chunk_edges(&mut self, chunk_x: i32, chunk_z: i32, from_section: i32, to_section: i32) {
        let mut curr = to_section;
        while curr >= from_section {
            self.check_chunk_edge(chunk_x, curr, chunk_z);
            curr -= 1;
        }
        self.perform_light_decrease();
    }

    fn check_chunk_edges(&mut self, chunk_x: i32, chunk_z: i32, from_section: i32, to_section: i32) {
        if self.sky {
            self.null_propagation_check.fill(false);
            self.rewrite_nibble_cache_for_skylight();
            let mut y = to_section;
            while y >= from_section {
                self.check_null_section(chunk_x, y, chunk_z, true);
                y -= 1;
            }
        }
        self.base_check_chunk_edges(chunk_x, chunk_z, from_section, to_section);
    }

    fn propagate_neighbour_levels(&mut self, chunk_x: i32, chunk_z: i32, from_section: i32, to_section: i32) {
        let mut curr_section_y = to_section;
        while curr_section_y >= from_section {
            let center_index = self.nibble_index(chunk_x, curr_section_y, chunk_z);
            if self.nib(center_index).is_some() {
                for &direction in &ONLY_HORIZONTAL {
                    let d = DIRECTIONS[direction];
                    let neighbour_index = self.nibble_index(chunk_x + d.x, curr_section_y, chunk_z + d.z);
                    let Some(neighbour) = self.nib(neighbour_index) else { continue };
                    if !neighbour.is_initialised_updating() {
                        continue;
                    }
                    let (inc_x, inc_z, start_x, start_z);
                    if d.x != 0 {
                        inc_x = 0;
                        inc_z = 1;
                        start_x = if d.x < 0 { (chunk_x << 4) - 1 } else { (chunk_x << 4) + 16 };
                        start_z = chunk_z << 4;
                    } else {
                        inc_x = 1;
                        inc_z = 0;
                        start_z = if d.z < 0 { (chunk_z << 4) - 1 } else { (chunk_z << 4) + 16 };
                        start_x = chunk_x << 4;
                    }
                    let propagate_direction = 1u64 << (direction ^ 1);
                    let encode_offset = self.coordinate_offset;
                    let mut curr_y = curr_section_y << 4;
                    let max_y = curr_y | 15;
                    while curr_y <= max_y {
                        let mut i = 0;
                        let mut curr_x = start_x;
                        let mut curr_z = start_z;
                        while i < 16 {
                            let level = self.nib(neighbour_index).unwrap().get_updating((curr_x & 15 | (curr_z & 15) << 4 | (curr_y & 15) << 8) as usize);
                            if level > 1 {
                                let value = Self::encode(curr_x, curr_y, curr_z, encode_offset) | ((level as u64) & 15) << 28 | propagate_direction << 32 | FLAG_HAS_SIDED_TRANSPARENT_BLOCKS;
                                self.append_to_increase_queue(value);
                            }
                            i += 1;
                            curr_x += inc_x;
                            curr_z += inc_z;
                        }
                        curr_y += 1;
                    }
                }
            }
            curr_section_y -= 1;
        }
    }

    // --- Light propagation ---

    // performLightIncrease. The two kinds of entry Starlight loops over apart
    // (with and without sided transparent blocks) are one loop here: an entry
    // without them has no shape of its own, which is what the other loop reads
    // as the face shape 0. The directions are visited in the same order.
    fn perform_light_increase(&mut self) {
        let mut queue_read_index = 0usize;
        let mut queue_length = self.increase_len;
        self.increase_len = 0;
        let decode_offset_x = -self.encode_offset_x;
        let decode_offset_y = -self.encode_offset_y;
        let decode_offset_z = -self.encode_offset_z;
        let encode_offset = self.coordinate_offset;
        let section_offset = self.chunk_section_index_offset;
        let reg = self.reg;
        let occlusion = &reg.occlusion;
        let mut cells = self.fast_table();

        while queue_read_index < queue_length {
            let queue_value = self.increase_queue[queue_read_index];
            queue_read_index += 1;
            let low = queue_value as u32;
            let pos_x = (low & 63) as i32 + decode_offset_x;
            let pos_z = (low >> 6 & 63) as i32 + decode_offset_z;
            let pos_y = (low >> 12 & 65535) as i32 + decode_offset_y;
            let propagated_light_level = (queue_value >> 28 & 15) as i32;
            if queue_value & (FLAG_RECHECK_LEVEL | FLAG_WRITE_LEVEL) != 0 {
                let index = cell_index(pos_x, pos_y, pos_z, section_offset);
                let local = local_index(pos_x, pos_y, pos_z);
                if queue_value & FLAG_RECHECK_LEVEL != 0 {
                    if cells[index].level(local) != propagated_light_level {
                        continue;
                    }
                } else if cells[index].present() {
                    self.fast_set(&mut cells, index, local, propagated_light_level);
                }
            }
            let from_block = if queue_value & FLAG_HAS_SIDED_TRANSPARENT_BLOCKS != 0 { self.block_state(pos_x, pos_y, pos_z) } else { 0 };
            let from_conditional = queue_value & FLAG_HAS_SIDED_TRANSPARENT_BLOCKS != 0 && self.conditional(from_block);
            let mut directions = queue_value >> 32 & 63;
            while directions != 0 {
                let propagate = directions.trailing_zeros() as usize;
                directions &= directions - 1;
                let d = DIRECTIONS[propagate];
                let from_shape = if from_conditional { self.face(from_block, d.nms) } else { 0 };
                if from_shape != 0 && occlusion.occludes(0, from_shape) {
                    continue;
                }
                let off_x = pos_x + d.x;
                let off_y = pos_y + d.y;
                let off_z = pos_z + d.z;
                let index = cell_index(off_x, off_y, off_z, section_offset);
                let local = local_index(off_x, off_y, off_z);
                let cell = cells[index];
                if !cell.present() {
                    continue;
                }
                let current_level = cell.get(local);
                if current_level >= propagated_light_level - 1 {
                    continue;
                }
                let block_state = cell.block(local);
                let info = reg.light_info(block_state);
                let mut flags = 0u64;
                if info & 256 != 0 {
                    let culling_face = self.face(block_state, DIRECTIONS[propagate ^ 1].nms);
                    if occlusion.occludes(from_shape, culling_face) {
                        continue;
                    }
                    flags |= FLAG_HAS_SIDED_TRANSPARENT_BLOCKS;
                }
                let target_level = propagated_light_level - ((info & 15) as i32).max(1);
                if target_level > current_level {
                    self.fast_set(&mut cells, index, local, target_level);
                    if target_level > 1 {
                        if queue_length >= self.increase_queue.len() {
                            self.resize_increase_queue();
                        }
                        self.increase_queue[queue_length] = Self::encode(off_x, off_y, off_z, encode_offset) | ((target_level as u64) & 15) << 28 | but_opposite(propagate) << 32 | flags;
                        queue_length += 1;
                    }
                }
            }
        }
        self.fast = cells;
    }

    // performLightDecrease, in one loop as performLightIncrease is.
    fn perform_light_decrease(&mut self) {
        let mut queue_read_index = 0usize;
        let mut queue_length = self.decrease_len;
        self.decrease_len = 0;
        let mut increase_queue_length = self.increase_len;
        let decode_offset_x = -self.encode_offset_x;
        let decode_offset_y = -self.encode_offset_y;
        let decode_offset_z = -self.encode_offset_z;
        let encode_offset = self.coordinate_offset;
        let section_offset = self.chunk_section_index_offset;
        let emitted_mask = self.emitted_mask;
        let reg = self.reg;
        let occlusion = &reg.occlusion;
        let mut cells = self.fast_table();

        while queue_read_index < queue_length {
            let queue_value = self.decrease_queue[queue_read_index];
            queue_read_index += 1;
            let low = queue_value as u32;
            let pos_x = (low & 63) as i32 + decode_offset_x;
            let pos_z = (low >> 6 & 63) as i32 + decode_offset_z;
            let pos_y = (low >> 12 & 65535) as i32 + decode_offset_y;
            let propagated_light_level = (queue_value >> 28 & 15) as i32;
            let from_block = if queue_value & FLAG_HAS_SIDED_TRANSPARENT_BLOCKS != 0 { self.block_state(pos_x, pos_y, pos_z) } else { 0 };
            let from_conditional = queue_value & FLAG_HAS_SIDED_TRANSPARENT_BLOCKS != 0 && self.conditional(from_block);
            let mut directions = queue_value >> 32 & 63;
            while directions != 0 {
                let propagate = directions.trailing_zeros() as usize;
                directions &= directions - 1;
                let d = DIRECTIONS[propagate];
                let from_shape = if from_conditional { self.face(from_block, d.nms) } else { 0 };
                if from_shape != 0 && occlusion.occludes(0, from_shape) {
                    continue;
                }
                let off_x = pos_x + d.x;
                let off_y = pos_y + d.y;
                let off_z = pos_z + d.z;
                let index = cell_index(off_x, off_y, off_z, section_offset);
                let local = local_index(off_x, off_y, off_z);
                let cell = cells[index];
                if !cell.present() {
                    continue;
                }
                let light_level = cell.get(local);
                if light_level == 0 {
                    continue;
                }
                let block_state = cell.block(local);
                let info = reg.light_info(block_state);
                let mut flags = 0u64;
                if info & 256 != 0 {
                    let culling_face = self.face(block_state, DIRECTIONS[propagate ^ 1].nms);
                    if occlusion.occludes(from_shape, culling_face) {
                        continue;
                    }
                    flags |= FLAG_HAS_SIDED_TRANSPARENT_BLOCKS;
                }
                let target_level = (propagated_light_level - ((info & 15) as i32).max(1)).max(0);
                if light_level > target_level {
                    if increase_queue_length >= self.increase_queue.len() {
                        self.resize_increase_queue();
                    }
                    self.increase_queue[increase_queue_length] = Self::encode(off_x, off_y, off_z, encode_offset) | ((light_level as u64) & 15) << 28 | 270582939648 | FLAG_RECHECK_LEVEL | flags;
                    increase_queue_length += 1;
                } else {
                    let emitted_light = (info >> 4 & 15) as i32 & emitted_mask;
                    if emitted_light != 0 {
                        if increase_queue_length >= self.increase_queue.len() {
                            self.resize_increase_queue();
                        }
                        self.increase_queue[increase_queue_length] = Self::encode(off_x, off_y, off_z, encode_offset) | ((emitted_light as u64) & 15) << 28 | 270582939648 | flags | FLAG_WRITE_LEVEL;
                        increase_queue_length += 1;
                    }
                    self.fast_set(&mut cells, index, local, 0);
                    if target_level > 0 {
                        if queue_length >= self.decrease_queue.len() {
                            self.resize_decrease_queue();
                        }
                        self.decrease_queue[queue_length] = Self::encode(off_x, off_y, off_z, encode_offset) | ((target_level as u64) & 15) << 28 | but_opposite(propagate) << 32 | flags;
                        queue_length += 1;
                    }
                }
            }
        }
        self.fast = cells;
        self.increase_len = increase_queue_length;
        self.perform_light_increase();
    }

    // --- BlockStarLightEngine ---

    fn block_check_block(&mut self, world_x: i32, world_y: i32, world_z: i32) {
        let encode_offset = self.coordinate_offset;
        let emitted_mask = self.emitted_mask;
        let current_level = self.light_level(world_x, world_y, world_z);
        let block_state = self.block_state(world_x, world_y, world_z);
        let emitted_level = self.emission(block_state) & emitted_mask;
        self.set_light_level(world_x, world_y, world_z, emitted_level);
        if emitted_level != 0 {
            let flag = if self.conditional(block_state) { FLAG_HAS_SIDED_TRANSPARENT_BLOCKS } else { 0 };
            self.append_to_increase_queue(Self::encode(world_x, world_y, world_z, encode_offset) | ((emitted_level as u64) & 15) << 28 | 270582939648 | flag);
        }
        self.append_to_decrease_queue(Self::encode(world_x, world_y, world_z, encode_offset) | ((current_level as u64) & 15) << 28 | 270582939648);
    }

    fn block_calculate_light_value(&self, world_x: i32, world_y: i32, world_z: i32, expect: i32) -> i32 {
        let center_state = self.block_state(world_x, world_y, world_z);
        let mut level = self.emission(center_state) & self.emitted_mask;
        if level < 14 && level <= expect {
            let opacity = self.opacity(center_state).max(1);
            if opacity >= 15 {
                return level;
            }
            let conditionally_opaque = self.conditional(center_state);
            let section_offset = self.chunk_section_index_offset;
            for (direction, d) in DIRECTIONS.iter().enumerate() {
                let off_x = world_x + d.x;
                let off_y = world_y + d.y;
                let off_z = world_z + d.z;
                let section_index = (off_x >> 4) + 5 * (off_z >> 4) + 25 * (off_y >> 4) + section_offset;
                let neighbour_level = self.light_level_at(section_index, off_x & 15 | (off_z & 15) << 4 | (off_y & 15) << 8);
                if neighbour_level - 1 > level {
                    let neighbour_state = self.block_state(off_x, off_y, off_z);
                    if self.conditional(neighbour_state) {
                        let neighbour_face = self.face(neighbour_state, DIRECTIONS[direction ^ 1].nms);
                        let this_face = if conditionally_opaque { self.face(center_state, d.nms) } else { 0 };
                        if self.reg.occlusion.occludes(this_face, neighbour_face) {
                            continue;
                        }
                    }
                    let calculated = neighbour_level - opacity;
                    level = calculated.max(level);
                    if level > expect {
                        return level;
                    }
                }
            }
            level
        } else {
            level
        }
    }

    // getSources: the positions in the chunk whose block emits light, by
    // section then index.
    fn block_sources(&self, chunk_x: i32, chunk_z: i32) -> Vec<(i32, i32, i32)> {
        let mut sources = Vec::new();
        let off_x = chunk_x << 4;
        let off_z = chunk_z << 4;
        let chunk = self.world.chunk(crate::dir::chunk_key(chunk_x, chunk_z));
        for section_y in MIN_SECTION..=MAX_SECTION {
            let Some(section) = chunk.sections[(section_y - MIN_SECTION) as usize].as_ref() else { continue };
            if section.non_air == 0 {
                continue;
            }
            let off_y = section_y << 4;
            for index in 0..4096i32 {
                let state = section.blocks[index as usize];
                if self.emission(state) != 0 {
                    sources.push((off_x | index & 15, off_y | index >> 8, off_z | index >> 4 & 15));
                }
            }
        }
        sources
    }

    fn block_light_chunk(&mut self, chunk_x: i32, chunk_z: i32, needs_edge_checks: bool) {
        let emitted_mask = self.emitted_mask;
        for (x, y, z) in self.block_sources(chunk_x, chunk_z) {
            let block_state = self.block_state(x, y, z);
            let emitted_light = self.emission(block_state) & emitted_mask;
            if emitted_light > self.light_level(x, y, z) {
                let flag = if self.conditional(block_state) { FLAG_HAS_SIDED_TRANSPARENT_BLOCKS } else { 0 };
                let offset = self.coordinate_offset;
                self.append_to_increase_queue(Self::encode(x, y, z, offset) | ((emitted_light as u64) & 15) << 28 | 270582939648 | flag);
                self.set_light_level(x, y, z, emitted_light);
            }
        }
        if needs_edge_checks {
            self.perform_light_increase();
            self.check_chunk_edges(chunk_x, chunk_z, MIN_LIGHT, MAX_LIGHT);
        } else {
            self.propagate_neighbour_levels(chunk_x, chunk_z, MIN_LIGHT, MAX_LIGHT);
            self.perform_light_increase();
        }
    }

    // --- SkyStarLightEngine ---

    fn rewrite_nibble_cache_for_skylight(&mut self) {
        for index in 0..self.nibble_cache.len() {
            if let Some(nibble) = self.nib(index) {
                if nibble.is_null_updating() {
                    // Stop propagation in these areas.
                    self.nib_mut(index).unwrap().update_visible();
                    self.nibble_cache[index] = NibRef::None;
                }
            }
        }
    }

    fn check_null_section(&mut self, chunk_x: i32, chunk_y: i32, chunk_z: i32, extrude_initialised: bool) -> bool {
        if chunk_y >= MIN_LIGHT && chunk_y <= MAX_LIGHT && !self.null_propagation_check[(chunk_y - MIN_LIGHT) as usize] {
            self.null_propagation_check[(chunk_y - MIN_LIGHT) as usize] = true;
            let mut need_init_neighbours = false;
            'find: for dz in -1..=1 {
                for dx in -1..=1 {
                    let index = self.nibble_index(dx + chunk_x, chunk_y, dz + chunk_z);
                    if let Some(nibble) = self.nib(index) {
                        if !nibble.is_null_updating() {
                            need_init_neighbours = true;
                            break 'find;
                        }
                    }
                }
            }
            if need_init_neighbours {
                for dz in -1..=1 {
                    for dx in -1..=1 {
                        self.init_nibble(dx + chunk_x, chunk_y, dz + chunk_z, if (dx | dz) == 0 { extrude_initialised } else { true }, true);
                    }
                }
            }
            need_init_neighbours
        } else {
            false
        }
    }

    fn light_level_extruded(&self, world_x: i32, world_y: i32, world_z: i32) -> i32 {
        let chunk_x = world_x >> 4;
        let mut chunk_y = world_y >> 4;
        let chunk_z = world_z >> 4;
        if let Some(nibble) = self.nib(self.nibble_index(chunk_x, chunk_y, chunk_z)) {
            return nibble.get_updating((world_x & 15 | (world_z & 15) << 4 | (world_y & 15) << 8) as usize);
        }
        loop {
            chunk_y += 1;
            if chunk_y > MAX_LIGHT {
                return 15;
            }
            if let Some(nibble) = self.nib(self.nibble_index(chunk_x, chunk_y, chunk_z)) {
                return nibble.get_updating((world_x & 15 | (world_z & 15) << 4) as usize);
            }
        }
    }

    fn sky_check_block(&mut self, world_x: i32, world_y: i32, world_z: i32) {
        let encode_offset = self.coordinate_offset;
        let current_level = self.light_level(world_x, world_y, world_z);
        if current_level == 15 {
            self.append_to_increase_queue(Self::encode(world_x, world_y, world_z, encode_offset) | ((current_level as u64) & 15) << 28 | 270582939648 | FLAG_HAS_SIDED_TRANSPARENT_BLOCKS);
        } else {
            self.set_light_level(world_x, world_y, world_z, 0);
        }
        self.append_to_decrease_queue(Self::encode(world_x, world_y, world_z, encode_offset) | ((current_level as u64) & 15) << 28 | 270582939648);
    }

    fn sky_calculate_light_value(&self, world_x: i32, world_y: i32, world_z: i32, expect: i32) -> i32 {
        if expect == 15 {
            return expect;
        }
        let section_offset = self.chunk_section_index_offset;
        let center_state = self.block_state(world_x, world_y, world_z);
        let opacity = self.opacity(center_state).max(1);
        let conditionally_opaque = self.conditional(center_state);
        let mut level = 0;
        for (direction, d) in DIRECTIONS.iter().enumerate() {
            let off_x = world_x + d.x;
            let off_y = world_y + d.y;
            let off_z = world_z + d.z;
            let section_index = (off_x >> 4) + 5 * (off_z >> 4) + 25 * (off_y >> 4) + section_offset;
            let neighbour_level = self.light_level_at(section_index, off_x & 15 | (off_z & 15) << 4 | (off_y & 15) << 8);
            if neighbour_level - 1 > level {
                let neighbour_state = self.block_state(off_x, off_y, off_z);
                if self.conditional(neighbour_state) {
                    let neighbour_face = self.face(neighbour_state, DIRECTIONS[direction ^ 1].nms);
                    let this_face = if conditionally_opaque { self.face(center_state, d.nms) } else { 0 };
                    if self.reg.occlusion.occludes(this_face, neighbour_face) {
                        continue;
                    }
                }
                let calculated = neighbour_level - opacity;
                level = calculated.max(level);
                if level > expect {
                    return level;
                }
            }
        }
        level
    }

    fn sky_propagate_block_changes(&mut self, chunk_x: i32, chunk_z: i32, positions: &[crate::dir::Pos]) {
        self.rewrite_nibble_cache_for_skylight();
        self.null_propagation_check.fill(false);
        let height_map_offset = chunk_x * -16 + chunk_z * -256;
        for pos in positions {
            let index = (pos.x() + (pos.z() << 4) + height_map_offset) as usize;
            let curr = self.height_map_block_change[index];
            if pos.y() > curr {
                self.height_map_block_change[index] = pos.y();
            }
        }
        for index in 0..256usize {
            let max_y = self.height_map_block_change[index];
            if max_y != i32::MIN {
                self.height_map_block_change[index] = i32::MIN;
                let column_x = index as i32 & 15 | chunk_x << 4;
                let column_z = (index as i32) >> 4 | chunk_z << 4;
                let max_propagation_y = self.try_propagate_skylight(column_x, max_y, column_z, true, true);
                let propagate_direction = but_this(POSITIVE_Y);
                let encode_offset = self.coordinate_offset;
                if self.light_level_extruded(column_x, max_propagation_y, column_z) == 15 {
                    self.check_null_section(column_x >> 4, max_propagation_y >> 4, column_z >> 4, true);
                    let mut curr_y = max_propagation_y;
                    while curr_y >= MIN_LIGHT << 4 {
                        if curr_y & 15 == 15 {
                            self.check_null_section(column_x >> 4, curr_y >> 4, column_z >> 4, true);
                        }
                        let nibble_index = self.nibble_index(column_x >> 4, curr_y >> 4, column_z >> 4);
                        match self.nib(nibble_index) {
                            None => curr_y &= -16,
                            Some(nibble) => {
                                if nibble.get_updating((column_x & 15 | (column_z & 15) << 4 | (curr_y & 15) << 8) as usize) != 15 {
                                    break;
                                }
                                self.append_to_decrease_queue(Self::encode(column_x, curr_y, column_z, encode_offset) | 4026531840 | propagate_direction << 32);
                            }
                        }
                        curr_y -= 1;
                    }
                }
            }
        }
        self.process_delayed_increases();
        self.process_delayed_decreases();
        for pos in positions {
            self.check_block(pos.x(), pos.y(), pos.z());
        }
        self.perform_light_decrease();
    }

    fn sky_light_chunk(&mut self, chunk_x: i32, chunk_z: i32, needs_edge_checks: bool) {
        self.rewrite_nibble_cache_for_skylight();
        self.null_propagation_check.fill(false);
        let chunk = self.world.chunk(crate::dir::chunk_key(chunk_x, chunk_z));
        let mut highest_non_empty_section = MAX_SECTION;
        while highest_non_empty_section == MIN_SECTION - 1
            || chunk.sections[(highest_non_empty_section - MIN_SECTION) as usize].as_ref().map_or(true, |section| section.non_air == 0)
        {
            self.check_null_section(chunk_x, highest_non_empty_section, chunk_z, false);
            for &direction in &ONLY_HORIZONTAL {
                let d = DIRECTIONS[direction];
                let neighbour_x = chunk_x + d.x;
                let neighbour_z = chunk_z + d.z;
                let neighbour_index = self.nibble_index(neighbour_x, highest_non_empty_section, neighbour_z);
                if self.nib(neighbour_index).is_some() {
                    let (inc_x, inc_z, start_x, start_z);
                    if d.x != 0 {
                        inc_x = 0;
                        inc_z = 1;
                        start_x = if d.x < 0 { chunk_x << 4 } else { chunk_x << 4 | 15 };
                        start_z = chunk_z << 4;
                    } else {
                        inc_x = 1;
                        inc_z = 0;
                        start_z = if d.z < 0 { chunk_z << 4 } else { chunk_z << 4 | 15 };
                        start_x = chunk_x << 4;
                    }
                    let encode_offset = self.coordinate_offset;
                    let propagate_direction = 1u64 << direction;
                    let mut curr_y = highest_non_empty_section << 4;
                    let max_y = curr_y | 15;
                    while curr_y <= max_y {
                        let mut i = 0;
                        let mut curr_x = start_x;
                        let mut curr_z = start_z;
                        while i < 16 {
                            self.append_to_increase_queue(Self::encode(curr_x, curr_y, curr_z, encode_offset) | 4026531840 | propagate_direction << 32);
                            i += 1;
                            curr_x += inc_x;
                            curr_z += inc_z;
                        }
                        curr_y += 1;
                    }
                }
            }
            let before = highest_non_empty_section;
            highest_non_empty_section -= 1;
            if before == MIN_SECTION - 1 {
                break;
            }
        }
        if highest_non_empty_section >= MIN_SECTION {
            let min_x = chunk_x << 4;
            let max_x = chunk_x << 4 | 15;
            let min_z = chunk_z << 4;
            let max_z = chunk_z << 4 | 15;
            let start_y = highest_non_empty_section << 4 | 15;
            for curr_z in min_z..=max_z {
                for curr_x in min_x..=max_x {
                    self.try_propagate_skylight(curr_x, start_y + 1, curr_z, false, false);
                }
            }
        }
        if needs_edge_checks {
            self.perform_light_increase();
            let mut y = highest_non_empty_section;
            while y >= MIN_LIGHT {
                self.check_null_section(chunk_x, y, chunk_z, false);
                y -= 1;
            }
            // super.checkChunkEdges
            self.base_check_chunk_edges(chunk_x, chunk_z, MIN_LIGHT, highest_non_empty_section);
        } else {
            let mut y = highest_non_empty_section;
            while y >= MIN_LIGHT {
                self.check_null_section(chunk_x, y, chunk_z, false);
                y -= 1;
            }
            self.propagate_neighbour_levels(chunk_x, chunk_z, MIN_LIGHT, highest_non_empty_section);
            self.perform_light_increase();
        }
    }

    fn process_delayed_increases(&mut self) {
        let decode_offset_x = -self.encode_offset_x;
        let decode_offset_y = -self.encode_offset_y;
        let decode_offset_z = -self.encode_offset_z;
        for i in 0..self.increase_len {
            let value = self.increase_queue[i];
            let low = value as u32;
            let pos_x = (low & 63) as i32 + decode_offset_x;
            let pos_z = (low >> 6 & 63) as i32 + decode_offset_z;
            let pos_y = (low >> 12 & 65535) as i32 + decode_offset_y;
            let level = (value >> 28 & 15) as i32;
            self.set_light_level(pos_x, pos_y, pos_z, level);
        }
    }

    fn process_delayed_decreases(&mut self) {
        let decode_offset_x = -self.encode_offset_x;
        let decode_offset_y = -self.encode_offset_y;
        let decode_offset_z = -self.encode_offset_z;
        for i in 0..self.decrease_len {
            let value = self.decrease_queue[i];
            let low = value as u32;
            let pos_x = (low & 63) as i32 + decode_offset_x;
            let pos_z = (low >> 6 & 63) as i32 + decode_offset_z;
            let pos_y = (low >> 12 & 65535) as i32 + decode_offset_y;
            self.set_light_level(pos_x, pos_y, pos_z, 0);
        }
    }

    fn try_propagate_skylight(&mut self, world_x: i32, start_y: i32, world_z: i32, extrude_initialised: bool, delay_light_set: bool) -> i32 {
        let encode_offset = self.coordinate_offset;
        let propagate_direction = but_this(POSITIVE_Y);
        let mut start_y = start_y;
        if self.light_level_extruded(world_x, start_y + 1, world_z) != 15 {
            return start_y;
        }
        self.check_null_section(world_x >> 4, start_y >> 4, world_z >> 4, extrude_initialised);
        let mut above = self.block_state(world_x, start_y + 1, world_z);
        while start_y >= MIN_LIGHT << 4 {
            if start_y & 15 == 15 {
                self.check_null_section(world_x >> 4, start_y >> 4, world_z >> 4, extrude_initialised);
            }
            let current = self.block_state(world_x, start_y, world_z);
            let from_shape;
            if self.conditional(above) {
                from_shape = self.face(above, DIRECTIONS[NEGATIVE_Y].nms);
                if self.reg.occlusion.occludes(0, from_shape) {
                    break;
                }
            } else {
                from_shape = 0;
            }
            let mut flags = 0u64;
            if self.conditional(current) {
                let culling_face = self.face(current, DIRECTIONS[POSITIVE_Y].nms);
                if self.reg.occlusion.occludes(from_shape, culling_face) {
                    break;
                }
                flags |= FLAG_HAS_SIDED_TRANSPARENT_BLOCKS;
            }
            let opacity = self.opacity(current);
            if opacity > 0 {
                break;
            }
            self.append_to_increase_queue(Self::encode(world_x, start_y, world_z, encode_offset) | 4026531840 | propagate_direction << 32 | flags);
            above = current;
            let index = self.nibble_index(world_x >> 4, start_y >> 4, world_z >> 4);
            if self.nib(index).is_none() {
                self.increase_len -= 1;
                start_y &= -16;
                above = 0;
            } else if !delay_light_set {
                self.set_light_level(world_x, start_y, world_z, 15);
            }
            start_y -= 1;
        }
        start_y
    }
}
