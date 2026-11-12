// BaseCoralPlantTypeBlock's code the coral classes share, and CoralBlock.

use super::common::{get_facing, schedule_water_tick, waterlogged};
use crate::blocks::{Behaviour, NONE};
use crate::dir::{Dir, Pos, ALL};
use crate::level::{Level, PRIORITY_NORMAL, UPDATE_CLIENTS};
use crate::registry::{State, Support};

// CoralBlock: the coral block (no waterlogging).
pub const BLOCK_BEHAVIOUR: Behaviour = Behaviour { update_shape: Some(block_update_shape), tick: Some(block_tick), ..NONE };

// BaseCoralPlantTypeBlock.scanForWater: whether the state is waterlogged or
// a neighbour holds water (Level.getFluidState reads each neighbour in turn).
pub fn scan_for_water(l: &mut Level, state: State, pos: Pos) -> bool {
    if l.reg.get_bool(state, waterlogged(l)) {
        return true;
    }
    scan_neighbours_for_water(l, pos)
}

// CoralBlock.scanForWater.
fn scan_neighbours_for_water(l: &mut Level, pos: Pos) -> bool {
    for dir in ALL {
        let fluid = l.fluid_state(pos.relative(dir));
        if l.fluid_type(fluid).is_water() {
            return true;
        }
    }
    false
}

// BaseCoralPlantTypeBlock.tryScheduleDieTick: a coral out of water dies in
// 60 to 99 ticks.
pub fn try_schedule_die_tick(l: &mut Level, state: State, pos: Pos) {
    if !scan_for_water(l, state, pos) {
        let delay = 60 + l.random.next_int_bound(40);
        l.schedule_tick(pos, l.block(state), delay, PRIORITY_NORMAL);
    }
}

// The block a coral turns into when it dies: the dead variant's default
// state, not waterlogged (the live and dead blocks are named alike).
fn dead_state(l: &Level, state: State) -> State {
    let key = &l.reg.blocks[l.block(state) as usize].key;
    let dead = l.reg.default_state(l.reg.block_key(&format!("minecraft:dead_{}", key.strip_prefix("minecraft:").unwrap())));
    dead
}

// CoralFanBlock.tick, CoralPlantBlock.tick: the callBlockFadeEvent has no
// listener.
pub fn tick(l: &mut Level, state: State, pos: Pos) {
    if !scan_for_water(l, state, pos) {
        let dead = l.reg.with_bool(dead_state(l, state), waterlogged(l), false);
        l.set(pos, dead, UPDATE_CLIENTS);
    }
}

// CoralWallFanBlock.tick: the dead fan keeps the facing.
pub fn wall_tick(l: &mut Level, state: State, pos: Pos) {
    if !scan_for_water(l, state, pos) {
        let dead = l.reg.with_bool(dead_state(l, state), waterlogged(l), false);
        let dead = l.reg.with_dir(dead, super::common::facing(l), get_facing(l, state));
        l.set(pos, dead, UPDATE_CLIENTS);
    }
}

// BaseCoralPlantTypeBlock.canSurvive: a sturdy top below.
pub fn can_survive(l: &mut Level, _state: State, pos: Pos) -> bool {
    let below = l.get(pos.below());
    crate::shape_ops::is_face_sturdy(l, below, pos.below(), Dir::Up, Support::Full)
}

// BaseCoralWallFanBlock.canSurvive: a sturdy face behind.
pub fn wall_can_survive(l: &mut Level, state: State, pos: Pos) -> bool {
    let facing = get_facing(l, state);
    let behind_pos = pos.relative(facing.opposite());
    let behind = l.get(behind_pos);
    crate::shape_ops::is_face_sturdy(l, behind, behind_pos, facing, Support::Full)
}

// BaseCoralPlantTypeBlock.updateShape, for the dead plants and fans.
pub fn base_update_shape(l: &mut Level, state: State, pos: Pos, direction: Dir, _neighbor_pos: Pos, _neighbor: State) -> State {
    schedule_water_tick(l, state, pos);
    if direction == Dir::Down && !can_survive(l, state, pos) {
        l.known.air
    } else {
        state
    }
}

// CoralFanBlock.updateShape and CoralPlantBlock.updateShape: the die tick,
// the water tick, and BaseCoralPlantTypeBlock's, which schedules the water
// tick again. (CoralFanBlock schedules the water tick after the die tick as
// CoralPlantBlock does.)
pub fn update_shape(l: &mut Level, state: State, pos: Pos, direction: Dir, neighbor_pos: Pos, neighbor: State) -> State {
    if direction == Dir::Down && !can_survive(l, state, pos) {
        l.known.air
    } else {
        try_schedule_die_tick(l, state, pos);
        schedule_water_tick(l, state, pos);
        base_update_shape(l, state, pos, direction, neighbor_pos, neighbor)
    }
}

// CoralBlock.updateShape: a coral block next to no water dies in 60 to 99 ticks.
fn block_update_shape(l: &mut Level, state: State, pos: Pos, _direction: Dir, _neighbor_pos: Pos, _neighbor: State) -> State {
    if !scan_neighbours_for_water(l, pos) {
        let delay = 60 + l.random.next_int_bound(40);
        l.schedule_tick(pos, l.block(state), delay, PRIORITY_NORMAL);
    }
    state
}

// CoralBlock.tick.
fn block_tick(l: &mut Level, state: State, pos: Pos) {
    if !scan_neighbours_for_water(l, pos) {
        let dead = dead_state(l, state);
        l.set(pos, dead, UPDATE_CLIENTS);
    }
}
