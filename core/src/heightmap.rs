// Heightmap as LevelChunk keeps them: the four a full chunk has (the ones
// ChunkStatus.FULL keeps), primed on the chunk's creation and updated by
// every block set (Heightmap.update). A chunk made empty primes to every
// column at the world's bottom.

use crate::registry::{Registry, State, BLOCKS_MOTION, EMPTY_FLUID, LEAVES};
use crate::world::{Section, MIN_Y};

// Heightmap.Types of a full chunk, in LevelChunk.setBlockState's order.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Types {
    MotionBlocking,
    MotionBlockingNoLeaves,
    OceanFloor,
    WorldSurface,
}

pub const TYPES: [Types; 4] = [Types::MotionBlocking, Types::MotionBlockingNoLeaves, Types::OceanFloor, Types::WorldSurface];

impl Types {
    // getSerializationKey.
    pub fn key(self) -> &'static str {
        match self {
            Types::MotionBlocking => "MOTION_BLOCKING",
            Types::MotionBlockingNoLeaves => "MOTION_BLOCKING_NO_LEAVES",
            Types::OceanFloor => "OCEAN_FLOOR",
            Types::WorldSurface => "WORLD_SURFACE",
        }
    }

    // isOpaque.
    fn is_opaque(self, reg: &Registry, state: State) -> bool {
        let flags = reg.flags(state);
        let fluid = reg.fluid(state) != EMPTY_FLUID;
        match self {
            Types::WorldSurface => flags & crate::registry::AIR == 0,
            Types::OceanFloor => flags & BLOCKS_MOTION != 0,
            Types::MotionBlocking => flags & BLOCKS_MOTION != 0 || fluid,
            Types::MotionBlockingNoLeaves => (flags & BLOCKS_MOTION != 0 || fluid) && flags & LEAVES == 0,
        }
    }
}

// SimpleBitStorage(Mth.ceillog2(height + 1), 256): 11 bits for 1024 blocks.
const BITS: u32 = 11;
const VALUES_PER_LONG: usize = 64 / BITS as usize;
const LONGS: usize = (256 + VALUES_PER_LONG - 1) / VALUES_PER_LONG;
const MASK: u64 = (1 << BITS) - 1;

pub struct Heightmap {
    pub kind: Types,
    data: [u64; LONGS],
}

impl Heightmap {
    pub fn new(kind: Types) -> Heightmap {
        Heightmap { kind, data: [0; LONGS] }
    }

    // getRawData.
    pub fn raw(&self) -> &[u64] {
        &self.data
    }

    fn get(&self, index: usize) -> i32 {
        (self.data[index / VALUES_PER_LONG] >> ((index % VALUES_PER_LONG) as u32 * BITS) & MASK) as i32
    }

    fn set(&mut self, index: usize, value: i32) {
        let shift = (index % VALUES_PER_LONG) as u32 * BITS;
        let at = index / VALUES_PER_LONG;
        self.data[at] = self.data[at] & !(MASK << shift) | (value as u64 & MASK) << shift;
    }

    // getFirstAvailable.
    pub fn first_available(&self, x: i32, z: i32) -> i32 {
        self.get((x + z * 16) as usize) + MIN_Y
    }

    // Heightmap.update: `x` and `z` are within the chunk, `sections` its
    // sections (already holding the new state).
    pub fn update(&mut self, reg: &Registry, sections: &[Option<Section>], x: i32, y: i32, z: i32, state: State) -> bool {
        let first_available = self.first_available(x, z);
        if y <= first_available - 2 {
            return false;
        }
        if self.kind.is_opaque(reg, state) {
            if y >= first_available {
                self.set((x + z * 16) as usize, y + 1 - MIN_Y);
                return true;
            }
        } else if first_available - 1 == y {
            for i in (MIN_Y..y).rev() {
                let section = &sections[((i - MIN_Y) >> 4) as usize];
                let at = (((i & 15) << 8) | (z << 4) | x) as usize;
                let below = section.as_ref().map_or(0, |section| section.blocks[at]);
                if self.kind.is_opaque(reg, below) {
                    self.set((x + z * 16) as usize, i + 1 - MIN_Y);
                    return true;
                }
            }
            self.set((x + z * 16) as usize, 0);
            return true;
        }
        false
    }
}
