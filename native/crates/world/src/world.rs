//! The world (`server/world.js` `World`): chunks by packed key in today's
//! `Map` order, a shared palette, the block count, the chunks this world
//! wrote since its last finalize (`ownedChunks`) and changed
//! (`changedChunks`, in `Set` order, which the build transfer writes).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};

use indexmap::IndexSet;
use sha2::{Digest, Sha256};

use crate::chunk::{CHUNK_SIZE, CHUNK_VOLUME, Cells, Chunk, DENSE_THRESHOLD, IntBuild};
use crate::chunk_map::ChunkMap;
use crate::js::push_json_string;
use crate::palette::Palette;
use crate::{
    AIR, BlockBox, BlockPos, BlockState, Diff, MAX_BLOCKS, MAX_COORD, ReadChunk, WorldManifest,
    WriteChunk,
};

/// Chunk coordinates are packed as 3 x 10 bits around this offset
/// (`KEY_OFFSET`).
pub const KEY_OFFSET: i32 = 512;

static WORLDS: AtomicU64 = AtomicU64::new(1);

/// The packed key of chunk `(cx, cy, cz)` (`World.keyOf`), with
/// JavaScript's 32-bit integer arithmetic.
#[inline]
pub fn key_of(cx: i32, cy: i32, cz: i32) -> u32 {
    (((cx + KEY_OFFSET) << 20) | ((cy + KEY_OFFSET) << 10) | (cz + KEY_OFFSET)) as u32
}

/// The chunk coordinates of a packed key (`World.coordsOf`).
#[inline]
pub fn coords_of(key: u32) -> [i32; 3] {
    [
        (key >> 20) as i32 - KEY_OFFSET,
        ((key >> 10) & 1023) as i32 - KEY_OFFSET,
        (key & 1023) as i32 - KEY_OFFSET,
    ]
}

#[inline]
fn cell(x: i32, y: i32, z: i32) -> usize {
    (((y & 15) << 8) | ((z & 15) << 4) | (x & 15)) as usize
}

/// The ordered set of chunk keys a world keeps (`Set` of keys).
pub type KeySet = IndexSet<u32, IntBuild>;

/// The chunks read once for every world of a chat (`chunkCache`): stored
/// chunks by hash, shared by the worlds that name them.
pub type ChunkCache = std::collections::HashMap<String, Arc<Chunk>>;

/// A world (see the module's doc).
#[derive(Debug)]
pub struct World {
    pub(crate) uid: u64,
    pub(crate) chunks: ChunkMap,
    pub(crate) palette: Arc<Palette>,
    pub(crate) block_count: usize,
    pub(crate) revision: i64,
    pub(crate) base: Option<u64>,
    /// The ticks Minecraft settled this world with for the build that made
    /// it, or `None` for the default (`settleTicks`).
    pub settle_ticks: Option<i64>,
    pub(crate) owned: KeySet,
    pub(crate) changed: KeySet,
    pub(crate) cached_bounds: OnceLock<BlockBox>,
}

/// A copy of this value: the same chunks (shared, each copied on its first
/// write by either), palette, sets, base and identity. Today's
/// `world.clone()`, a new world based on this one, is
/// [`World::clone_world`].
impl Clone for World {
    fn clone(&self) -> World {
        World {
            uid: self.uid,
            chunks: self.chunks.clone(),
            palette: self.palette.clone(),
            block_count: self.block_count,
            revision: self.revision,
            base: self.base,
            settle_ticks: self.settle_ticks,
            owned: self.owned.clone(),
            changed: self.changed.clone(),
            cached_bounds: self.cached_bounds.clone(),
        }
    }
}

impl Default for World {
    fn default() -> World {
        World::new()
    }
}

/// A chunk file's text: `JSON.stringify` of its `[index, palette index]`
/// pairs.
pub fn chunk_payload(pairs: &[(u16, u16)]) -> String {
    let mut out = String::with_capacity(2 + pairs.len() * 11);
    out.push('[');
    let mut digits = itoa_buf();
    for (at, &(index, value)) in pairs.iter().enumerate() {
        if at > 0 {
            out.push(',');
        }
        out.push('[');
        out.push_str(fmt_u16(&mut digits, index));
        out.push(',');
        out.push_str(fmt_u16(&mut digits, value));
        out.push(']');
    }
    out.push(']');
    out
}

fn itoa_buf() -> [u8; 5] {
    [0; 5]
}

fn fmt_u16(buf: &mut [u8; 5], mut value: u16) -> &str {
    let mut at = 5;
    loop {
        at -= 1;
        buf[at] = b'0' + (value % 10) as u8;
        value /= 10;
        if value == 0 {
            break;
        }
    }
    std::str::from_utf8(&buf[at..]).expect("digits")
}

/// The name of a chunk file: the first 24 hex digits of its text's sha256
/// (`server/store.js` `chunkWriter`).
pub fn chunk_hash(payload: &str) -> String {
    let digest = Sha256::digest(payload.as_bytes());
    let mut out = String::with_capacity(24);
    for byte in &digest[..12] {
        out.push(char::from_digit((byte >> 4) as u32, 16).expect("hex"));
        out.push(char::from_digit((byte & 15) as u32, 16).expect("hex"));
    }
    out
}

/// Reads a chunk file's pairs, as `JSON.parse` would give them, for the
/// integers today's files hold.
pub fn parse_chunk_payload(bytes: &[u8]) -> Result<Vec<(u16, u16)>, String> {
    let mut at = 0;
    let skip = |at: &mut usize| {
        while *at < bytes.len() && matches!(bytes[*at], b' ' | b'\t' | b'\n' | b'\r') {
            *at += 1;
        }
    };
    let expect = |at: &mut usize, byte: u8| -> Result<(), String> {
        skip(at);
        if bytes.get(*at) != Some(&byte) {
            return Err(format!(
                "A chunk file is not a list of [index, palette index] pairs (at byte {})",
                *at
            ));
        }
        *at += 1;
        Ok(())
    };
    let number = |at: &mut usize| -> Result<u16, String> {
        skip(at);
        let start = *at;
        let mut value: u32 = 0;
        while *at < bytes.len() && bytes[*at].is_ascii_digit() {
            value = value * 10 + (bytes[*at] - b'0') as u32;
            if value > u16::MAX as u32 {
                return Err(format!(
                    "A chunk file holds a number past 65535 (at byte {start})"
                ));
            }
            *at += 1;
        }
        if *at == start {
            return Err(format!(
                "A chunk file is not a list of [index, palette index] pairs (at byte {start})"
            ));
        }
        Ok(value as u16)
    };
    let mut pairs = Vec::new();
    expect(&mut at, b'[')?;
    skip(&mut at);
    if bytes.get(at) == Some(&b']') {
        at += 1;
    } else {
        loop {
            expect(&mut at, b'[')?;
            let index = number(&mut at)?;
            expect(&mut at, b',')?;
            let value = number(&mut at)?;
            expect(&mut at, b']')?;
            pairs.push((index, value));
            skip(&mut at);
            match bytes.get(at) {
                Some(b',') => at += 1,
                Some(b']') => {
                    at += 1;
                    break;
                }
                _ => {
                    return Err(format!(
                        "A chunk file is not a list of [index, palette index] pairs (at byte {at})"
                    ));
                }
            }
        }
    }
    skip(&mut at);
    if at != bytes.len() {
        return Err(format!(
            "A chunk file has bytes past its pairs (at byte {at})"
        ));
    }
    Ok(pairs)
}

impl World {
    /// An empty world with a new palette.
    pub fn new() -> World {
        World::with_palette(Arc::new(Palette::new()))
    }

    /// An empty world on `palette` (`new World({ palette })`).
    pub fn with_palette(palette: Arc<Palette>) -> World {
        World::assemble(ChunkMap::default(), palette, 0, 0, None)
    }

    pub(crate) fn assemble(
        chunks: ChunkMap,
        palette: Arc<Palette>,
        block_count: usize,
        revision: i64,
        settle_ticks: Option<i64>,
    ) -> World {
        World {
            uid: WORLDS.fetch_add(1, Ordering::Relaxed),
            chunks,
            palette,
            block_count,
            revision,
            base: None,
            settle_ticks,
            owned: KeySet::default(),
            changed: KeySet::default(),
            cached_bounds: OnceLock::new(),
        }
    }

    /// Its palette, shared with the worlds cloned from it and those it was
    /// cloned from.
    pub fn palette(&self) -> &Arc<Palette> {
        &self.palette
    }

    /// The chunks in today's `Map` order.
    pub fn chunks(&self) -> impl Iterator<Item = (u32, &Arc<Chunk>)> {
        self.chunks.iter()
    }

    /// The chunk at `key`.
    pub fn chunk(&self, key: u32) -> Option<&Arc<Chunk>> {
        self.chunks.get(key)
    }

    /// How many chunks it has.
    pub fn chunk_count(&self) -> usize {
        self.chunks.len()
    }

    /// Its revision: finalizes since it was made empty.
    pub fn revision(&self) -> i64 {
        self.revision
    }

    /// The chunks it wrote since its last finalize, in order.
    pub fn owned_chunks(&self) -> impl Iterator<Item = u32> + '_ {
        self.owned.iter().copied()
    }

    /// The chunks it changed since its last finalize, in order.
    pub fn changed_chunks(&self) -> impl Iterator<Item = u32> + '_ {
        self.changed.iter().copied()
    }

    /// A number naming this world for the life of the process (today's
    /// object identity, which `diff` checks).
    pub fn uid(&self) -> u64 {
        self.uid
    }

    /// A copy sharing this world's chunks until either writes one (`clone`).
    /// Chunks this world wrote since its last finalize it would keep writing
    /// in place, so it must be finalized first.
    pub fn clone_world(&self) -> Result<World, String> {
        if !self.owned.is_empty() {
            return Err(
                "A world with writes since its last finalize cannot be cloned: finalize it first"
                    .into(),
            );
        }
        let mut world = World::assemble(
            self.chunks.clone(),
            self.palette.clone(),
            self.block_count,
            self.revision,
            self.settle_ticks,
        );
        world.base = Some(self.uid);
        Ok(world)
    }

    /// A world sharing this one's chunks and palette, with no base
    /// (`reference`).
    pub fn reference(&self) -> World {
        World::assemble(
            self.chunks.clone(),
            self.palette.clone(),
            self.block_count,
            self.revision,
            self.settle_ticks,
        )
    }

    /// `checkCoord`.
    pub fn check_coord(pos: BlockPos) -> Result<(), String> {
        let [x, y, z] = pos;
        if [x, y, z].iter().any(|v| *v < -MAX_COORD || *v > MAX_COORD) {
            return Err(format!(
                "Position [{x}, {y}, {z}] is out of range (each axis must be an integer within +/-{MAX_COORD})"
            ));
        }
        Ok(())
    }

    /// The palette index at `pos` (`paletteIndexAt`).
    #[inline]
    pub fn palette_index_at(&self, pos: BlockPos) -> u16 {
        let [x, y, z] = pos;
        match self.chunks.get(key_of(x >> 4, y >> 4, z >> 4)) {
            Some(chunk) => chunk.get(cell(x, y, z)),
            None => 0,
        }
    }

    fn writable_chunk(&mut self, key: u32) -> &mut Chunk {
        if !self.chunks.contains(key) {
            self.chunks.insert(key, Arc::new(Chunk::new()));
            self.owned.insert(key);
        } else if !self.owned.contains(&key) {
            // A chunk this world did not create since its last finalize may
            // be shared, so it is copied first (Arc::make_mut copies it when
            // it is).
            self.owned.insert(key);
        }
        Arc::make_mut(self.chunks.get_mut(key).expect("inserted above"))
    }

    fn reset_bounds(&mut self) {
        self.cached_bounds = OnceLock::new();
    }

    /// Sets the block at `pos` (`set`); air removes it. Fails with today's
    /// message past [`MAX_COORD`] or [`MAX_BLOCKS`], before anything
    /// changes, the palette included.
    pub fn set(&mut self, pos: BlockPos, state: BlockState) -> Result<(), String> {
        self.set_block(pos, &state.id, &state.properties)
    }

    /// [`World::set`] with the id and properties borrowed.
    pub fn set_block(
        &mut self,
        pos: BlockPos,
        id: &str,
        properties: &[(String, String)],
    ) -> Result<(), String> {
        World::check_coord(pos)?;
        let [x, y, z] = pos;
        let key = key_of(x >> 4, y >> 4, z >> 4);
        let index = cell(x, y, z);
        let current = self.chunks.get(key).map_or(0, |chunk| chunk.get(index));
        if current == 0 && id != AIR && self.block_count >= MAX_BLOCKS {
            return Err(self.refusal(1));
        }
        let next = if id == AIR {
            0
        } else {
            self.palette.intern(id, properties)?
        };
        if current == next {
            return Ok(());
        }
        let chunk = self.writable_chunk(key);
        chunk.set(index, next);
        let empty = chunk.count == 0;
        if current == 0 {
            self.block_count += 1;
        } else if next == 0 {
            self.block_count -= 1;
        }
        if empty {
            self.chunks.remove(key);
        }
        self.changed.insert(key);
        self.reset_bounds();
        Ok(())
    }

    fn refusal(&self, added: i64) -> String {
        format!(
            "Build exceeds the {MAX_BLOCKS} block limit: the world has {} blocks and this call would add {added}",
            self.block_count
        )
    }

    /// How many blocks a fill of the box would add: its air cells (`airIn`).
    fn air_in(&self, min: BlockPos, max: BlockPos) -> i64 {
        let mut air: i64 = 0;
        for cx in (min[0] >> 4)..=(max[0] >> 4) {
            let from_x = (min[0] - cx * CHUNK_SIZE).max(0);
            let to_x = (max[0] - cx * CHUNK_SIZE).min(CHUNK_SIZE - 1);
            for cy in (min[1] >> 4)..=(max[1] >> 4) {
                let from_y = (min[1] - cy * CHUNK_SIZE).max(0);
                let to_y = (max[1] - cy * CHUNK_SIZE).min(CHUNK_SIZE - 1);
                for cz in (min[2] >> 4)..=(max[2] >> 4) {
                    let from_z = (min[2] - cz * CHUNK_SIZE).max(0);
                    let to_z = (max[2] - cz * CHUNK_SIZE).min(CHUNK_SIZE - 1);
                    let volume =
                        ((to_x - from_x + 1) * (to_y - from_y + 1) * (to_z - from_z + 1)) as i64;
                    match self.chunks.get(key_of(cx, cy, cz)) {
                        None => air += volume,
                        Some(chunk) if volume == CHUNK_VOLUME as i64 => {
                            air += (CHUNK_VOLUME - chunk.count) as i64
                        }
                        Some(chunk) => {
                            for ly in from_y..=to_y {
                                for lz in from_z..=to_z {
                                    let row = ((ly << 8) | (lz << 4)) as usize;
                                    for lx in from_x..=to_x {
                                        if chunk.get(row + lx as usize) == 0 {
                                            air += 1;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        air
    }

    /// Fills `area` with `state` (`fill`).
    pub fn fill(&mut self, area: BlockBox, state: BlockState) -> Result<(), String> {
        self.fill_boxes(&[area], &state.id, &state.properties, &[])
    }

    /// Sets the boxes in `clear` to air and fills the boxes in `parts` with
    /// one block; no two boxes overlap (`fillBoxes`). The block limit applies
    /// to what the call leaves, and a call it refuses writes nothing.
    pub fn fill_boxes(
        &mut self,
        parts: &[BlockBox],
        id: &str,
        properties: &[(String, String)],
        clear: &[BlockBox],
    ) -> Result<(), String> {
        for b in parts.iter().chain(clear) {
            World::check_coord(b.min)?;
            World::check_coord(b.max)?;
        }
        let volume = |b: &BlockBox| -> i64 {
            (b.max[0] - b.min[0] + 1) as i64
                * (b.max[1] - b.min[1] + 1) as i64
                * (b.max[2] - b.min[2] + 1) as i64
        };
        let room = MAX_BLOCKS as i64 - self.block_count as i64;
        if id != AIR && parts.iter().map(volume).sum::<i64>() > room {
            let mut added: i64 = 0;
            for b in parts {
                added += self.air_in(b.min, b.max);
            }
            for b in clear {
                added -= volume(b) - self.air_in(b.min, b.max);
            }
            if added > room {
                return Err(self.refusal(added));
            }
        }
        for b in clear {
            self.write_box(b.min, b.max, 0);
        }
        let next = if id == AIR {
            0
        } else {
            self.palette.intern(id, properties)?
        };
        for b in parts {
            self.write_box(b.min, b.max, next);
        }
        Ok(())
    }

    fn write_box(&mut self, min: BlockPos, max: BlockPos, next: u16) {
        for cx in (min[0] >> 4)..=(max[0] >> 4) {
            let from_x = (min[0] - cx * CHUNK_SIZE).max(0);
            let to_x = (max[0] - cx * CHUNK_SIZE).min(CHUNK_SIZE - 1);
            for cy in (min[1] >> 4)..=(max[1] >> 4) {
                let from_y = (min[1] - cy * CHUNK_SIZE).max(0);
                let to_y = (max[1] - cy * CHUNK_SIZE).min(CHUNK_SIZE - 1);
                for cz in (min[2] >> 4)..=(max[2] >> 4) {
                    let from_z = (min[2] - cz * CHUNK_SIZE).max(0);
                    let to_z = (max[2] - cz * CHUNK_SIZE).min(CHUNK_SIZE - 1);
                    let key = key_of(cx, cy, cz);
                    if next == 0 && !self.chunks.contains(key) {
                        continue;
                    }
                    let volume =
                        ((to_x - from_x + 1) * (to_y - from_y + 1) * (to_z - from_z + 1)) as usize;
                    let chunk = self.writable_chunk(key);
                    let dense = chunk.is_dense() || volume > DENSE_THRESHOLD;
                    let mut added = 0usize;
                    let mut removed = 0usize;
                    let mut mutated = false;
                    if dense {
                        let cells = chunk.ensure_dense();
                        for ly in from_y..=to_y {
                            for lz in from_z..=to_z {
                                let row = ((ly << 8) | (lz << 4)) as usize;
                                for slot in &mut cells[row + from_x as usize..=row + to_x as usize]
                                {
                                    let current = *slot;
                                    if current == next {
                                        continue;
                                    }
                                    if current == 0 {
                                        added += 1;
                                    } else if next == 0 {
                                        removed += 1;
                                    }
                                    mutated = true;
                                    *slot = next;
                                }
                            }
                        }
                    } else {
                        for ly in from_y..=to_y {
                            for lz in from_z..=to_z {
                                let row = ((ly << 8) | (lz << 4)) as usize;
                                for lx in from_x..=to_x {
                                    let index = row + lx as usize;
                                    let current = chunk.get(index);
                                    if current == next {
                                        continue;
                                    }
                                    if current == 0 {
                                        added += 1;
                                    } else if next == 0 {
                                        removed += 1;
                                    }
                                    mutated = true;
                                    chunk.set(index, next);
                                }
                            }
                        }
                    }
                    if !mutated {
                        continue;
                    }
                    if dense {
                        chunk.count = chunk.count + added - removed;
                        chunk.touch();
                    }
                    let empty = chunk.count == 0;
                    self.block_count = self.block_count + added - removed;
                    if empty {
                        self.chunks.remove(key);
                    }
                    self.changed.insert(key);
                }
            }
        }
        self.reset_bounds();
    }

    /// The block at `pos`, or `None` for air.
    pub fn get(&self, pos: BlockPos) -> Option<&BlockState> {
        self.palette.get(self.palette_index_at(pos) as usize)
    }

    /// Whether `pos` holds a block (`has`).
    pub fn has(&self, pos: BlockPos) -> bool {
        self.palette_index_at(pos) != 0
    }

    /// Rewrites every block of `id` (inside `area` when given) to what
    /// `replacement` makes of its entry, once per palette entry, and returns
    /// how many it matched (`replace`).
    pub fn replace(
        &mut self,
        id: &str,
        mut replacement: impl FnMut(&BlockState) -> Result<BlockState, String>,
        area: Option<BlockBox>,
    ) -> Result<usize, String> {
        let entries = self.palette.len();
        let matches: Vec<bool> = (0..entries)
            .map(|i| self.palette.get(i).is_some_and(|e| e.id == id))
            .collect();
        if !matches.contains(&true) {
            return Ok(0);
        }
        let mut remap: Vec<i32> = vec![-1; entries];
        let mut count = 0;
        let keys: Vec<u32> = self.chunks.keys().collect();
        for key in keys {
            let [cx, cy, cz] = coords_of(key);
            let (ox, oy, oz) = (cx * CHUNK_SIZE, cy * CHUNK_SIZE, cz * CHUNK_SIZE);
            if let Some(b) = &area
                && (ox > b.max[0]
                    || ox + 15 < b.min[0]
                    || oy > b.max[1]
                    || oy + 15 < b.min[1]
                    || oz > b.max[2]
                    || oz + 15 < b.min[2])
            {
                continue;
            }
            let original = self.chunks.get(key).expect("listed").clone();
            let mut writes: Vec<(usize, u16)> = Vec::new();
            let mut failure = None;
            original.for_each(|index, previous| {
                if failure.is_some() || !matches.get(previous as usize).copied().unwrap_or(false) {
                    return;
                }
                if let Some(b) = &area {
                    let (x, y, z) = (
                        ox + (index & 15) as i32,
                        oy + (index >> 8) as i32,
                        oz + ((index >> 4) & 15) as i32,
                    );
                    if x < b.min[0]
                        || x > b.max[0]
                        || y < b.min[1]
                        || y > b.max[1]
                        || z < b.min[2]
                        || z > b.max[2]
                    {
                        return;
                    }
                }
                count += 1;
                let mut next = remap[previous as usize];
                if next == -1 {
                    let entry = replacement(self.palette.entry(previous as usize));
                    match entry.and_then(|e| {
                        if e.id == AIR {
                            Ok(0)
                        } else {
                            self.palette.intern(&e.id, &e.properties)
                        }
                    }) {
                        Ok(index) => next = index as i32,
                        Err(error) => {
                            failure = Some(error);
                            return;
                        }
                    }
                    remap[previous as usize] = next;
                }
                if next as u16 != previous {
                    writes.push((index, next as u16));
                }
            });
            if let Some(error) = failure {
                return Err(error);
            }
            if writes.is_empty() {
                continue;
            }
            let chunk = self.writable_chunk(key);
            let mut removed = 0;
            for (index, next) in writes {
                match &mut chunk.cells {
                    Cells::Dense(dense) => dense[index] = next,
                    Cells::Sparse(sparse) if next == 0 => {
                        sparse.shift_remove(&(index as u16));
                    }
                    Cells::Sparse(sparse) => {
                        sparse.insert(index as u16, next);
                    }
                }
                if next == 0 {
                    removed += 1;
                }
            }
            chunk.count -= removed;
            chunk.touch();
            let empty = chunk.count == 0;
            self.block_count -= removed;
            if empty {
                self.chunks.remove(key);
            }
            self.changed.insert(key);
            self.reset_bounds();
        }
        Ok(count)
    }

    /// The number of blocks.
    pub fn size(&self) -> usize {
        self.block_count
    }

    /// Removes every block (`clear`).
    pub fn clear(&mut self) {
        if self.block_count == 0 {
            return;
        }
        let keys: Vec<u32> = self.chunks.keys().collect();
        for key in keys {
            self.changed.insert(key);
        }
        self.chunks = ChunkMap::default();
        self.owned = KeySet::default();
        self.block_count = 0;
        self.reset_bounds();
    }

    /// `f(pos, entry)` for every block of one chunk (`forEachInChunk`).
    pub fn for_each_in_chunk<'w>(&'w self, key: u32, mut f: impl FnMut(BlockPos, &'w BlockState)) {
        let Some(chunk) = self.chunks.get(key) else {
            return;
        };
        let [cx, cy, cz] = coords_of(key);
        let (ox, oy, oz) = (cx * CHUNK_SIZE, cy * CHUNK_SIZE, cz * CHUNK_SIZE);
        chunk.for_each(|index, value| {
            f(
                [
                    ox + (index & 15) as i32,
                    oy + (index >> 8) as i32,
                    oz + ((index >> 4) & 15) as i32,
                ],
                self.palette.entry(value as usize),
            )
        });
    }

    /// `f(pos, entry)` for every block, in today's order, only inside `area`
    /// when given (`forEach`).
    pub fn for_each<'w>(
        &'w self,
        area: Option<BlockBox>,
        mut f: impl FnMut(BlockPos, &'w BlockState),
    ) {
        let Some(b) = area else {
            for key in self.chunks.keys() {
                self.for_each_in_chunk(key, &mut f);
            }
            return;
        };
        for cx in (b.min[0] >> 4)..=(b.max[0] >> 4) {
            for cy in (b.min[1] >> 4)..=(b.max[1] >> 4) {
                for cz in (b.min[2] >> 4)..=(b.max[2] >> 4) {
                    self.for_each_in_chunk(key_of(cx, cy, cz), |p, e| {
                        if (0..3).all(|a| p[a] >= b.min[a] && p[a] <= b.max[a]) {
                            f(p, e)
                        }
                    });
                }
            }
        }
    }

    /// The box around every block, or `None` for an empty world (`bounds`),
    /// computed once per change.
    pub fn bounds(&self) -> Option<BlockBox> {
        if let Some(b) = self.cached_bounds.get() {
            return Some(*b);
        }
        if self.block_count == 0 {
            return None;
        }
        let mut min = [i32::MAX; 3];
        let mut max = [i32::MIN; 3];
        for (key, chunk) in self.chunks.iter() {
            let b = chunk.bounds().expect("a chunk in a world holds blocks");
            let [cx, cy, cz] = coords_of(key);
            let o = [cx * CHUNK_SIZE, cy * CHUNK_SIZE, cz * CHUNK_SIZE];
            for a in 0..3 {
                min[a] = min[a].min(o[a] + b[a] as i32);
                max[a] = max[a].max(o[a] + b[a + 3] as i32);
            }
        }
        Some(*self.cached_bounds.get_or_init(|| BlockBox { min, max }))
    }

    /// The render LOD of a wide flat world (`renderLod`): `(axis, scale)`.
    pub fn render_lod(&self) -> Option<(usize, i32)> {
        let b = self.bounds()?;
        if self.block_count < 500_000 {
            return None;
        }
        let dims: Vec<i32> = (0..3).map(|a| b.max[a] - b.min[a] + 1).collect();
        let least = *dims.iter().min().expect("three axes");
        let axis = dims.iter().position(|&d| d == least).expect("found");
        if dims[axis] > CHUNK_SIZE || *dims.iter().max().expect("three axes") <= 1024 {
            return None;
        }
        Some((axis, 4))
    }

    /// Ends a build: the revision goes up, the base is let go, and the
    /// chunks written become shared (`finalize`).
    pub fn finalize(&mut self) -> &mut World {
        self.revision += 1;
        self.base = None;
        self.owned.clear();
        self.changed.clear();
        self
    }

    /// The chunks changed since the last finalize and their face
    /// neighbours, in today's `Set` order (`nearChangedChunks`).
    pub fn near_changed_chunks(&self) -> KeySet {
        let mut keys = KeySet::default();
        for &key in &self.changed {
            let [cx, cy, cz] = coords_of(key);
            keys.insert(key);
            keys.insert(key_of(cx + 1, cy, cz));
            keys.insert(key_of(cx - 1, cy, cz));
            keys.insert(key_of(cx, cy + 1, cz));
            keys.insert(key_of(cx, cy - 1, cz));
            keys.insert(key_of(cx, cy, cz + 1));
            keys.insert(key_of(cx, cy, cz - 1));
        }
        keys
    }

    /// How this world differs from `before`, the world it was cloned from
    /// (`diff`).
    pub fn diff(&self, before: &World) -> Result<Diff, String> {
        if self.base != Some(before.uid) {
            return Err("World.diff requires a direct clone of the previous world".into());
        }
        let mut out = Diff::default();
        for &key in &self.changed {
            let original = before.chunks.get(key);
            let current = self.chunks.get(key);
            match (original, current) {
                (None, None) => {}
                (None, Some(c)) => out.added += c.count as u64,
                (Some(o), None) => out.removed += o.count as u64,
                (Some(o), Some(c)) => {
                    let (o, c) = (o.view(), c.view());
                    for (a, b) in o.iter().zip(c.iter()) {
                        if a == b {
                            continue;
                        }
                        if *a == 0 {
                            out.added += 1;
                        } else if *b == 0 {
                            out.removed += 1;
                        } else {
                            out.changed += 1;
                        }
                    }
                }
            }
        }
        Ok(out)
    }

    fn write_chunk_file(
        chunk: &Chunk,
        write_chunk: &mut WriteChunk<'_>,
    ) -> Result<Arc<str>, String> {
        if let Some(hash) = chunk.persisted() {
            return Ok(hash);
        }
        let payload = chunk_payload(&chunk.pairs());
        let hash: Arc<str> = chunk_hash(&payload).into();
        write_chunk(&hash, payload.as_bytes())?;
        chunk.set_persisted(Some(hash.clone()));
        Ok(hash)
    }

    /// Writes the chunks not yet stored, as [`World::persist`] does, without
    /// the manifest (`persistChunks`).
    pub fn persist_chunks(&self, write_chunk: &mut WriteChunk<'_>) -> Result<(), String> {
        for (_, chunk) in self.chunks.iter() {
            World::write_chunk_file(chunk, write_chunk)?;
        }
        Ok(())
    }

    /// Writes the chunks not yet stored and returns the manifest
    /// (`JSON.stringify(world.serializeManifest(writeChunk))`).
    pub fn persist(&self, write_chunk: &mut WriteChunk<'_>) -> Result<WorldManifest, String> {
        let mut out = String::with_capacity(128 + self.palette.len() * 48 + self.chunks.len() * 44);
        out.push_str("{\"revision\":");
        out.push_str(&self.revision.to_string());
        out.push_str(",\"blockCount\":");
        out.push_str(&self.block_count.to_string());
        out.push_str(",\"settleTicks\":");
        match self.settle_ticks {
            Some(ticks) => out.push_str(&ticks.to_string()),
            None => out.push_str("null"),
        }
        out.push_str(",\"palette\":[");
        for (at, entry) in self.palette.entries().enumerate() {
            if at > 0 {
                out.push(',');
            }
            out.push_str("{\"id\":");
            push_json_string(&mut out, &entry.id);
            out.push_str(",\"states\":{");
            for (i, (key, value)) in entry.properties.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                push_json_string(&mut out, key);
                out.push(':');
                push_json_string(&mut out, value);
            }
            out.push_str("}}");
        }
        out.push_str("],\"chunks\":[");
        for (at, (key, chunk)) in self.chunks.iter().enumerate() {
            let hash = World::write_chunk_file(chunk, write_chunk)?;
            if at > 0 {
                out.push(',');
            }
            let [cx, cy, cz] = coords_of(key);
            out.push_str(&format!("[\"{cx},{cy},{cz}\","));
            push_json_string(&mut out, &hash);
            out.push(']');
        }
        out.push_str("]}");
        Ok(WorldManifest(out))
    }

    /// Reads a stored world: the manifest and each chunk file by its hash.
    pub fn from_manifest(
        manifest: &WorldManifest,
        read_chunk: &mut ReadChunk<'_>,
    ) -> Result<World, String> {
        World::from_manifest_cached(manifest, read_chunk, &mut ChunkCache::new())
    }

    /// [`World::from_manifest`] sharing `cache`'s chunks, as one chat's
    /// worlds share them today (`fromManifest(manifest, readChunk,
    /// chunkCache)`).
    pub fn from_manifest_cached(
        manifest: &WorldManifest,
        read_chunk: &mut ReadChunk<'_>,
        cache: &mut ChunkCache,
    ) -> Result<World, String> {
        let parsed = ParsedManifest::parse(&manifest.0)?;
        let palette = Arc::new(Palette::new());
        for (id, properties) in &parsed.palette {
            palette.intern(id, properties)?;
        }
        let mut chunks = ChunkMap::with_capacity(parsed.chunks.len());
        for (key, hash) in parsed.chunks {
            let chunk = match cache.get(&hash) {
                Some(chunk) => chunk.clone(),
                None => {
                    let pairs = parse_chunk_payload(&read_chunk(&hash)?)?;
                    let mut chunk = Chunk::new();
                    if pairs.len() > DENSE_THRESHOLD {
                        chunk.ensure_dense();
                    }
                    for (index, value) in pairs {
                        if index as usize >= CHUNK_VOLUME {
                            return Err(format!(
                                "Chunk file {hash} names cell {index}, past a chunk's {CHUNK_VOLUME}"
                            ));
                        }
                        chunk.set(index as usize, value);
                    }
                    chunk.set_persisted(Some(hash.as_str().into()));
                    let chunk = Arc::new(chunk);
                    cache.insert(hash, chunk.clone());
                    chunk
                }
            };
            chunks.insert(key, chunk);
        }
        Ok(World::assemble(
            chunks,
            palette,
            parsed.block_count,
            parsed.revision,
            parsed.settle_ticks,
        ))
    }
}

/// A manifest's fields, as `World.fromManifest` reads them.
struct ParsedManifest {
    revision: i64,
    block_count: usize,
    settle_ticks: Option<i64>,
    palette: Vec<(String, Vec<(String, String)>)>,
    chunks: Vec<(u32, String)>,
}

impl ParsedManifest {
    fn parse(text: &str) -> Result<ParsedManifest, String> {
        use serde_json::Value;
        let value: Value =
            serde_json::from_str(text).map_err(|e| format!("A world manifest is not JSON: {e}"))?;
        let integer = |field: &str| -> Result<Option<i64>, String> {
            match value.get(field) {
                None | Some(Value::Null) => Ok(None),
                Some(Value::Number(n)) => n
                    .as_i64()
                    .map(Some)
                    .ok_or_else(|| format!("A world manifest's {field} is {n}, not an integer")),
                Some(other) => Err(format!(
                    "A world manifest's {field} is {other}, not a number"
                )),
            }
        };
        let revision = integer("revision")?.ok_or("A world manifest has no revision")?;
        let block_count = integer("blockCount")?.ok_or("A world manifest has no blockCount")?;
        let settle_ticks = integer("settleTicks")?;
        let mut palette = Vec::new();
        for entry in value
            .get("palette")
            .and_then(Value::as_array)
            .ok_or("A world manifest has no palette")?
        {
            let id = entry
                .get("id")
                .and_then(Value::as_str)
                .ok_or_else(|| format!("A palette entry {entry} has no id"))?;
            let mut properties = Vec::new();
            match entry.get("states") {
                None | Some(Value::Null) => {}
                Some(Value::Object(states)) => {
                    for (key, value) in states {
                        let text = value.as_str().ok_or_else(|| {
                            format!("Palette entry {id} has state {key} = {value}, not a string")
                        })?;
                        properties.push((key.clone(), text.to_string()));
                    }
                }
                Some(other) => {
                    return Err(format!(
                        "Palette entry {id} has states {other}, not an object"
                    ));
                }
            }
            palette.push((id.to_string(), properties));
        }
        let mut chunks = Vec::new();
        for pair in value
            .get("chunks")
            .and_then(Value::as_array)
            .ok_or("A world manifest has no chunks")?
        {
            let (Some(key), Some(hash)) = (
                pair.get(0).and_then(Value::as_str),
                pair.get(1).and_then(Value::as_str),
            ) else {
                return Err(format!(
                    "A world manifest's chunk {pair} is not [key, hash]"
                ));
            };
            let coords: Vec<i32> = key
                .split(',')
                .map(|n| {
                    n.parse::<i32>()
                        .map_err(|_| format!("A world manifest's chunk key {key} is not x,y,z"))
                })
                .collect::<Result<_, _>>()?;
            let [cx, cy, cz] = coords[..] else {
                return Err(format!("A world manifest's chunk key {key} is not x,y,z"));
            };
            chunks.push((key_of(cx, cy, cz), hash.to_string()));
        }
        Ok(ParsedManifest {
            revision,
            block_count: block_count as usize,
            settle_ticks,
            palette,
            chunks,
        })
    }
}
