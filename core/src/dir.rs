// Minecraft's Direction and block positions.

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Dir {
    Down = 0,
    Up = 1,
    North = 2,
    South = 3,
    West = 4,
    East = 5,
}

use Dir::*;

// Direction.values().
pub const ALL: [Dir; 6] = [Down, Up, North, South, West, East];
// Direction.Plane.HORIZONTAL.
pub const HORIZONTAL: [Dir; 4] = [North, East, South, West];
// NeighborUpdater.UPDATE_ORDER.
pub const UPDATE_ORDER: [Dir; 6] = [West, East, Down, Up, North, South];
// BlockBehaviour.UPDATE_SHAPE_ORDER.
pub const UPDATE_SHAPE_ORDER: [Dir; 6] = [West, East, North, South, Down, Up];

impl Dir {
    #[inline]
    pub fn offset(self) -> (i32, i32, i32) {
        match self {
            Down => (0, -1, 0),
            Up => (0, 1, 0),
            North => (0, 0, -1),
            South => (0, 0, 1),
            West => (-1, 0, 0),
            East => (1, 0, 0),
        }
    }

    #[inline]
    pub fn opposite(self) -> Dir {
        match self {
            Down => Up,
            Up => Down,
            North => South,
            South => North,
            West => East,
            East => West,
        }
    }

    pub fn is_horizontal(self) -> bool {
        !matches!(self, Down | Up)
    }

    // Direction.getClockWise around the y axis.
    pub fn clockwise(self) -> Dir {
        match self {
            North => East,
            East => South,
            South => West,
            West => North,
            _ => panic!("{self:?} has no clockwise direction"),
        }
    }

    pub fn counter_clockwise(self) -> Dir {
        match self {
            North => West,
            West => South,
            South => East,
            East => North,
            _ => panic!("{self:?} has no counter-clockwise direction"),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Down => "down",
            Up => "up",
            North => "north",
            South => "south",
            West => "west",
            East => "east",
        }
    }

    pub fn by_name(name: &str) -> Dir {
        match name {
            "down" => Down,
            "up" => Up,
            "north" => North,
            "south" => South,
            "west" => West,
            "east" => East,
            _ => panic!("no direction {name}"),
        }
    }

    pub fn axis(self) -> Axis {
        match self {
            Down | Up => Axis::Y,
            North | South => Axis::Z,
            West | East => Axis::X,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Axis {
    X,
    Y,
    Z,
}

// A block position. Held as x and z in one word (x low, z high, as
// ChunkPos.asLong holds a chunk's) and y, which is the shape that goes in two
// registers: as three i32 fields it is passed in memory, through a copy the
// callee makes, which reads back what the caller has just stored in pieces.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Pos {
    xz: u64,
    y: i32,
}

impl Pos {
    #[inline(always)]
    pub const fn new(x: i32, y: i32, z: i32) -> Pos {
        Pos { xz: (x as u32 as u64) | ((z as u32 as u64) << 32), y }
    }

    #[inline(always)]
    pub const fn x(self) -> i32 {
        self.xz as i32
    }

    #[inline(always)]
    pub const fn y(self) -> i32 {
        self.y
    }

    #[inline(always)]
    pub const fn z(self) -> i32 {
        (self.xz >> 32) as i32
    }

    #[inline]
    pub fn relative(self, d: Dir) -> Pos {
        let (dx, dy, dz) = d.offset();
        Pos::new(self.x() + dx, self.y() + dy, self.z() + dz)
    }

    #[inline]
    pub fn relative_n(self, d: Dir, n: i32) -> Pos {
        let (dx, dy, dz) = d.offset();
        Pos::new(self.x() + dx * n, self.y() + dy * n, self.z() + dz * n)
    }

    pub fn below(self) -> Pos {
        self.relative(Down)
    }

    pub fn above(self) -> Pos {
        self.relative(Up)
    }

    // ChunkPos.asLong of the position's chunk.
    #[inline]
    pub fn chunk(self) -> i64 {
        chunk_key(self.x() >> 4, self.z() >> 4)
    }

    // BlockPos.asLong.
    pub fn as_long(self) -> i64 {
        (((self.x() as i64) & 0x3FF_FFFF) << 38) | (((self.z() as i64) & 0x3FF_FFFF) << 12) | ((self.y() as i64) & 0xFFF)
    }
}

// As three fields x, y and z derived it: hashed in that order, ordered by x,
// then y, then z.
impl std::hash::Hash for Pos {
    #[inline]
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write_i32(self.x());
        state.write_i32(self.y());
        state.write_i32(self.z());
    }
}

impl PartialOrd for Pos {
    fn partial_cmp(&self, other: &Pos) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Pos {
    fn cmp(&self, other: &Pos) -> std::cmp::Ordering {
        (self.x(), self.y(), self.z()).cmp(&(other.x(), other.y(), other.z()))
    }
}

impl std::fmt::Debug for Pos {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.debug_struct("Pos").field("x", &self.x()).field("y", &self.y()).field("z", &self.z()).finish()
    }
}

// ChunkPos.asLong.
#[inline]
pub fn chunk_key(x: i32, z: i32) -> i64 {
    ((x as i64) & 0xFFFF_FFFF) | (((z as i64) & 0xFFFF_FFFF) << 32)
}

impl std::fmt::Display for Pos {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}, {}, {}", self.x(), self.y(), self.z())
    }
}
