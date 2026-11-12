// BambooStalkBlock: stands on the bamboo_plantable_on blocks. A stalk that
// cannot survive schedules a tick (updateShape) and breaks in it; a stalk
// under an older stalk takes its age. Its collision shape is offset by the
// position (the registry holds it at the origin): collision.rs refuses to
// read it.

use crate::blocks::{Behaviour, NONE};
use crate::dir::{Dir, Pos};
use crate::in_tag;
use crate::is_block;
use crate::level::{Level, PRIORITY_NORMAL};
use crate::property;
use crate::registry::State;

pub const BEHAVIOUR: Behaviour = Behaviour { update_shape: Some(update_shape), can_survive: Some(can_survive), tick: Some(tick), ..NONE };

// BambooStalkBlock.AGE (AGE_1).
fn age(l: &Level) -> u16 {
    property!(l, "age", ["0", "1"])
}

// BambooStalkBlock.tick.
fn tick(l: &mut Level, state: State, pos: Pos) {
    if !can_survive(l, state, pos) {
        l.destroy_block(pos, true, 512);
    }
}

// BambooStalkBlock.canSurvive.
fn can_survive(l: &mut Level, _state: State, pos: Pos) -> bool {
    let below = l.get(pos.below());
    in_tag!(l, below, "minecraft:bamboo_plantable_on")
}

// BambooStalkBlock.updateShape; state.cycle(AGE) wraps around the two ages.
fn update_shape(l: &mut Level, state: State, pos: Pos, direction: Dir, _neighbor_pos: Pos, neighbor: State) -> State {
    if !can_survive(l, state, pos) {
        l.schedule_tick(pos, l.block(state), 1, PRIORITY_NORMAL);
    }
    let age = age(l);
    if direction == Dir::Up && is_block!(l, neighbor, "minecraft:bamboo") && l.reg.get(neighbor, age) > l.reg.get(state, age) {
        l.reg.with(state, age, (l.reg.get(state, age) + 1) % 2)
    } else {
        state
    }
}
