// ChorusPlantBlock: stands on end stone or on chorus plant, and its six
// connections follow its neighbours. A plant that cannot survive schedules a
// tick (updateShape) and breaks in it. (Paper's disableChorusPlantUpdates is
// off.)

use crate::blocks::{Behaviour, NONE};
use crate::dir::{Dir, Pos, HORIZONTAL};
use crate::is_block;
use crate::level::{Level, PRIORITY_NORMAL};
use crate::registry::State;

pub const BEHAVIOUR: Behaviour = Behaviour { update_shape: Some(update_shape), can_survive: Some(can_survive), tick: Some(tick), ..NONE };

// ChorusPlantBlock.tick.
fn tick(l: &mut Level, state: State, pos: Pos) {
    if !can_survive(l, state, pos) {
        l.destroy_block(pos, true, 512);
    }
}

// ChorusPlantBlock.updateShape (BlockBehaviour.updateShape returns the state).
fn update_shape(l: &mut Level, state: State, pos: Pos, direction: Dir, _neighbor_pos: Pos, neighbor: State) -> State {
    if !can_survive(l, state, pos) {
        l.schedule_tick(pos, l.block(state), 1, PRIORITY_NORMAL);
        state
    } else {
        let connects = l.block(neighbor) == l.block(state)
            || is_block!(l, neighbor, "minecraft:chorus_flower")
            || direction == Dir::Down && is_block!(l, neighbor, "minecraft:end_stone");
        let side = l.reg.property_of(state, direction.name());
        l.reg.with_bool(state, side, connects)
    }
}

// ChorusPlantBlock.canSurvive.
fn can_survive(l: &mut Level, state: State, pos: Pos) -> bool {
    let this = l.block(state);
    let below = l.get(pos.below());
    let above = l.get(pos.above());
    let flag = !l.is_air(above) && !l.is_air(below);
    for dir in HORIZONTAL {
        let at = pos.relative(dir);
        let side = l.get(at);
        if l.block(side) == this {
            if flag {
                return false;
            }
            let side_below = l.get(at.below());
            if l.block(side_below) == this || is_block!(l, side_below, "minecraft:end_stone") {
                return true;
            }
        }
    }
    l.block(below) == this || is_block!(l, below, "minecraft:end_stone")
}
