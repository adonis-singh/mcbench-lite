// Code the shape and attachment blocks share.

use crate::blocks::wire_power;
use crate::dir::{Dir, Pos};
use crate::level::Level;
use crate::fluid::WATER_TICK_DELAY;
use crate::property;
use crate::registry::{Fluid, State, SOLID};

// A block tag's members by block id, looked up once.
#[macro_export]
macro_rules! shape_tag {
    ($l:expr, $name:literal) => {{
        static MEMBERS: std::sync::OnceLock<&'static Vec<bool>> = std::sync::OnceLock::new();
        *MEMBERS.get_or_init(|| $l.reg.tags.get($name).unwrap_or_else(|| panic!("no tag {}", $name)))
    }};
}

// The blocks of the named Java classes (instanceof), by block id, built once.
#[macro_export]
macro_rules! shape_classes {
    ($l:expr, [$($name:literal),*]) => {{
        static MEMBERS: std::sync::OnceLock<Vec<bool>> = std::sync::OnceLock::new();
        MEMBERS.get_or_init(|| $l.reg.blocks.iter().map(|block| matches!(block.class.as_str(), $($name)|*)).collect())
    }};
}

// HorizontalDirectionalBlock.FACING.
pub fn facing(l: &Level) -> u16 {
    property!(l, "facing", ["north", "south", "west", "east"])
}

// BlockStateProperties.WATERLOGGED.
pub fn waterlogged(l: &Level) -> u16 {
    property!(l, "waterlogged", ["true", "false"])
}

// BlockSetType.canOpenByHand of a door's or trapdoor's type: false for
// iron, the only door or trapdoor type that sets it false (gold also does,
// and has none).
pub fn can_open_by_hand(l: &Level, state: State) -> bool {
    !matches!(l.reg.blocks[l.block(state) as usize].key.as_str(), "minecraft:iron_door" | "minecraft:iron_trapdoor")
}

// BlockStateProperties.POWERED, OPEN.
pub fn powered(l: &Level) -> u16 {
    property!(l, "powered", ["true", "false"])
}

pub fn open(l: &Level) -> u16 {
    property!(l, "open", ["true", "false"])
}

pub fn get_facing(l: &Level, state: State) -> Dir {
    l.reg.get_dir(state, facing(l))
}

// A waterlogged block's updateShape (and trapdoor's neighborChanged)
// schedules a water tick: scheduleTick(pos, Fluids.WATER,
// Fluids.WATER.getTickDelay(level)).
pub fn schedule_water_tick(l: &mut Level, state: State, pos: Pos) {
    if l.reg.get_bool(state, waterlogged(l)) {
        l.schedule_fluid_tick(pos, Fluid::Water, WATER_TICK_DELAY);
    }
}

// BlockStateBase.isSolid.
pub fn is_solid(l: &Level, state: State) -> bool {
    l.reg.flags(state) & SOLID != 0
}

// Whether two directions are on one axis.
pub fn same_axis(a: Dir, b: Dir) -> bool {
    a.axis() == b.axis()
}

// Quarter turns clockwise from north.
pub fn turns(dir: Dir) -> u32 {
    match dir {
        Dir::North => 0,
        Dir::East => 1,
        Dir::South => 2,
        Dir::West => 3,
        _ => panic!("{dir:?} is not horizontal"),
    }
}

// Block.isExceptionForConnection.
pub fn is_exception_for_connection(l: &Level, state: State) -> bool {
    let leaves = crate::shape_classes!(l, ["LeavesBlock", "TintedParticleLeavesBlock", "UntintedParticleLeavesBlock", "MangroveLeavesBlock"]);
    static EXCEPTIONS: std::sync::OnceLock<Vec<bool>> = std::sync::OnceLock::new();
    let exceptions = EXCEPTIONS.get_or_init(|| {
        l.reg
            .blocks
            .iter()
            .map(|block| matches!(block.key.as_str(), "minecraft:barrier" | "minecraft:carved_pumpkin" | "minecraft:jack_o_lantern" | "minecraft:melon" | "minecraft:pumpkin"))
            .collect()
    });
    let block = l.block(state) as usize;
    leaves[block] || exceptions[block] || crate::shape_tag!(l, "minecraft:shulker_boxes")[block]
}

// FenceGateBlock.connectsToDirection.
pub fn gate_connects_to_direction(l: &Level, state: State, direction: Dir) -> bool {
    get_facing(l, state).axis() == direction.clockwise().axis()
}

// CraftBlock.getBlockPower(): the strongest wire power among the neighbours
// that give a signal toward the block (in the order down, up, east, west,
// north, south), else 15 if the block has a neighbour signal.
pub fn bukkit_block_power(l: &mut Level, pos: Pos) -> i32 {
    let mut power = 0;
    for dir in [Dir::Down, Dir::Up, Dir::East, Dir::West, Dir::North, Dir::South] {
        let at = pos.relative(dir);
        if l.has_signal(at, dir) {
            let state = l.get(at);
            if l.block(state) == l.known.redstone_wire {
                power = power.max(wire_power(l, state));
            }
        }
    }
    if power > 0 {
        power
    } else if l.has_neighbor_signal(pos) {
        15
    } else {
        0
    }
}

// Level.playSound with a pitch from the level's random: the sound itself
// reaches nobody, but the pitch draws from the random.
pub fn play_sound_with_random_pitch(l: &mut Level) {
    l.random.next_float();
}

// CrossCollisionBlock.PROPERTY_BY_DIRECTION: a fence's or pane's side.
pub fn cross_property(l: &Level, dir: Dir) -> u16 {
    match dir {
        Dir::North => property!(l, "north", ["true", "false"]),
        Dir::East => property!(l, "east", ["true", "false"]),
        Dir::South => property!(l, "south", ["true", "false"]),
        Dir::West => property!(l, "west", ["true", "false"]),
        _ => panic!("{dir:?} has no cross collision side"),
    }
}
