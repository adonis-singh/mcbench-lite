// DirtPathBlock: like farmland, a path under something solid turns to dirt
// one tick after the block above changes.

use super::farmland::{survives_under, turn_to_dirt};
use crate::blocks::{Behaviour, NONE};
use crate::dir::{Dir, Pos};
use crate::level::{Level, PRIORITY_NORMAL};
use crate::registry::State;

pub const BEHAVIOUR: Behaviour = Behaviour { update_shape: Some(update_shape), can_survive: Some(can_survive), tick: Some(tick), ..NONE };

// DirtPathBlock.updateShape.
fn update_shape(l: &mut Level, state: State, pos: Pos, direction: Dir, _neighbor_pos: Pos, _neighbor: State) -> State {
    if direction == Dir::Up && !can_survive(l, state, pos) {
        l.schedule_tick(pos, l.block(state), 1, PRIORITY_NORMAL);
    }
    state
}

// DirtPathBlock.canSurvive.
fn can_survive(l: &mut Level, _state: State, pos: Pos) -> bool {
    let above = l.get(pos.above());
    survives_under(l, above)
}

// DirtPathBlock.tick: FarmBlock.turnToDirt(null, ...).
fn tick(l: &mut Level, state: State, pos: Pos) {
    if !can_survive(l, state, pos) {
        turn_to_dirt(l, state, pos);
    }
}
