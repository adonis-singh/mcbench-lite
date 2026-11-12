// The level's chunks: each a column of 16x16x16 sections of block states,
// made when first asked for (McbenchLevel.chunk), as Paper loads an empty
// void chunk on demand.

use crate::dir::{chunk_key, Pos};
use crate::fast::FastMap;
use crate::heightmap::{Heightmap, TYPES};
use crate::registry::State;
use std::cell::Cell;

pub const MIN_Y: i32 = -64;
pub const HEIGHT: i32 = 1024;
pub const SECTIONS: usize = (HEIGHT / 16) as usize;

pub struct Section {
    pub blocks: Box<[State; 4096]>,
    // Blocks that are not air (LevelChunkSection.nonEmptyBlockCount).
    pub non_air: u16,
}

pub struct Chunk {
    pub x: i32,
    pub z: i32,
    pub sections: Vec<Option<Section>>,
    // The four heightmaps a full chunk keeps (Heightmap.primeHeightmaps of
    // ChunkStatus.FULL.heightmapsAfter), in setBlockState's order.
    pub heightmaps: [Heightmap; 4],
}

impl Chunk {
    fn new(x: i32, z: i32) -> Chunk {
        Chunk { x, z, sections: (0..SECTIONS).map(|_| None).collect(), heightmaps: TYPES.map(Heightmap::new) }
    }

    pub fn is_empty(&self) -> bool {
        self.sections.iter().all(|section| section.as_ref().map_or(true, |s| s.non_air == 0))
    }
}

#[inline]
pub fn index(pos: Pos) -> usize {
    (((pos.y() & 15) << 8) | ((pos.z() & 15) << 4) | (pos.x() & 15)) as usize
}

pub struct World {
    chunks: Vec<Chunk>,
    index: FastMap<i64, usize>,
    // The air states, which a section holding only them counts as empty.
    air: Vec<bool>,
    // The chunk last asked for, which block code asks for again and again.
    last: Cell<(i64, usize)>,
    // The chunks asked for lately, by a hash of the key (chunks are never
    // removed, so an entry stays right): the next level of caching, for the
    // updates that go back and forth over a chunk's edge.
    recent: [Cell<(i64, usize)>; 8],
}

impl World {
    pub fn new(air: Vec<bool>) -> World {
        World { chunks: Vec::new(), index: FastMap::default(), air, last: Cell::new((i64::MIN, 0)), recent: std::array::from_fn(|_| Cell::new((i64::MIN, 0))) }
    }

    // Where the chunk of this key is, if there is one: the last asked for, a
    // recent one, or from the index.
    #[inline]
    fn find(&self, key: i64) -> Option<usize> {
        let (last, at) = self.last.get();
        if last == key {
            return Some(at);
        }
        let recent = &self.recent[((key ^ (key >> 32)) & 7) as usize];
        let (recent_key, at) = recent.get();
        if recent_key == key {
            self.last.set((key, at));
            return Some(at);
        }
        let at = *self.index.get(&key)?;
        recent.set((key, at));
        self.last.set((key, at));
        Some(at)
    }

    #[inline]
    fn at(&self, key: i64) -> usize {
        match self.find(key) {
            Some(at) => at,
            None => panic!("no chunk for key {key}"),
        }
    }

    pub fn chunk_keys(&self) -> impl Iterator<Item = i64> + '_ {
        self.index.keys().copied()
    }

    pub fn chunk(&self, key: i64) -> &Chunk {
        &self.chunks[self.at(key)]
    }

    pub fn outside(pos: Pos) -> bool {
        pos.y() < MIN_Y || pos.y() >= MIN_Y + HEIGHT
    }

    // The chunk at chunk x, z, made if new. Returns whether it was made.
    #[inline]
    pub fn touch(&mut self, x: i32, z: i32) -> bool {
        let key = chunk_key(x, z);
        if self.find(key).is_some() {
            return false;
        }
        self.chunks.push(Chunk::new(x, z));
        self.index.insert(key, self.chunks.len() - 1);
        self.last.set((key, self.chunks.len() - 1));
        true
    }

    pub fn has_chunk(&self, x: i32, z: i32) -> bool {
        let key = chunk_key(x, z);
        self.find(key).is_some()
    }

    // The block at `pos` in a chunk that exists (air in an empty section).
    #[inline]
    pub fn get(&self, pos: Pos) -> State {
        let (last, at) = self.last.get();
        if last == pos.chunk() {
            return self.get_in(at, pos);
        }
        self.get_other(pos)
    }

    #[inline(always)]
    fn get_in(&self, at: usize, pos: Pos) -> State {
        match &self.chunks[at].sections[((pos.y() - MIN_Y) >> 4) as usize] {
            Some(section) => section.blocks[index(pos)],
            None => 0,
        }
    }

    // The block at `pos` if it is in the chunk last asked for and inside the
    // world's height.
    #[inline(always)]
    pub fn get_last(&self, pos: Pos) -> Option<State> {
        let (last, at) = self.last.get();
        if last == pos.chunk() && !World::outside(pos) {
            Some(self.get_in(at, pos))
        } else {
            None
        }
    }

    // A block in a chunk other than the last asked for.
    #[inline(never)]
    fn get_other(&self, pos: Pos) -> State {
        self.get_in(self.at(pos.chunk()), pos)
    }

    // Whether the chunk at chunk x, z is the one last asked for (so asking for
    // it again changes nothing).
    #[inline(always)]
    pub fn is_last(&self, x: i32, z: i32) -> bool {
        self.last.get().0 == chunk_key(x, z)
    }

    pub fn section_has_only_air(&self, pos: Pos) -> bool {
        let chunk = &self.chunks[self.at(pos.chunk())];
        chunk.sections[((pos.y() - MIN_Y) >> 4) as usize].as_ref().map_or(true, |s| s.non_air == 0)
    }

    // The chunk's heightmaps after a block set (the four Heightmap.update
    // calls of LevelChunk.setBlockState).
    pub fn update_heightmaps(&mut self, reg: &crate::registry::Registry, pos: Pos, state: State) {
        let at = self.at(pos.chunk());
        let chunk = &mut self.chunks[at];
        for map in chunk.heightmaps.iter_mut() {
            map.update(reg, &chunk.sections, pos.x() & 15, pos.y(), pos.z() & 15, state);
        }
    }

    // LevelChunkSection.setBlockState: the state before.
    #[inline]
    pub fn set(&mut self, pos: Pos, state: State) -> State {
        let at = self.at(pos.chunk());
        let chunk = &mut self.chunks[at];
        let section = chunk.sections[((pos.y() - MIN_Y) >> 4) as usize].get_or_insert_with(|| Section { blocks: Box::new([0; 4096]), non_air: 0 });
        let at = index(pos);
        let old = section.blocks[at];
        section.blocks[at] = state;
        if !self.air[old as usize] {
            section.non_air -= 1;
        }
        if !self.air[state as usize] {
            section.non_air += 1;
        }
        old
    }
}
