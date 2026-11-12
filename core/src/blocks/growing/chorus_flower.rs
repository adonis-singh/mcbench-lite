// ChorusFlowerBlock: stands on end stone or chorus plant, or hangs from one
// chorus plant beside it over air. A flower that cannot survive schedules a
// tick (updateShape, not for the block above) and breaks in it.

use crate::blocks::{Behaviour, NONE};
use crate::dir::{Dir, Pos, HORIZONTAL};
use crate::is_block;
use crate::level::{Level, PRIORITY_NORMAL};
use crate::registry::State;

pub const BEHAVIOUR: Behaviour = Behaviour { update_shape: Some(update_shape), can_survive: Some(can_survive), tick: Some(tick), ..NONE };

// ChorusFlowerBlock.tick.
fn tick(l: &mut Level, state: State, pos: Pos) {
    if !can_survive(l, state, pos) {
        l.destroy_block(pos, true, 512);
    }
}

// ChorusFlowerBlock.updateShape.
fn update_shape(l: &mut Level, state: State, pos: Pos, direction: Dir, _neighbor_pos: Pos, _neighbor: State) -> State {
    if direction != Dir::Up && !can_survive(l, state, pos) {
        l.schedule_tick(pos, l.block(state), 1, PRIORITY_NORMAL);
    }
    state
}

// ChorusFlowerBlock.canSurvive; the plant is Blocks.CHORUS_PLANT.
fn can_survive(l: &mut Level, _state: State, pos: Pos) -> bool {
    let below = l.get(pos.below());
    if is_block!(l, below, "minecraft:chorus_plant") || is_block!(l, below, "minecraft:end_stone") {
        return true;
    }
    if !l.is_air(below) {
        return false;
    }
    let mut flag = false;
    for dir in HORIZONTAL {
        let side = l.get(pos.relative(dir));
        if is_block!(l, side, "minecraft:chorus_plant") {
            if flag {
                return false;
            }
            flag = true;
        } else if !l.is_air(side) {
            return false;
        }
    }
    flag
}
