// HangingRootsBlock: hangs from a block with a sturdy bottom face.

use crate::blocks::shapes::common::schedule_water_tick;
use crate::blocks::{Behaviour, NONE};
use crate::dir::{Dir, Pos};
use crate::level::Level;
use crate::registry::{State, Support};
use crate::shape_ops;

pub const BEHAVIOUR: Behaviour = Behaviour { update_shape: Some(update_shape), can_survive: Some(can_survive), ..NONE };

// HangingRootsBlock.canSurvive: the block above has a sturdy bottom face.
fn can_survive(l: &mut Level, _state: State, pos: Pos) -> bool {
    let above = l.get(pos.above());
    shape_ops::is_face_sturdy(l, above, pos.above(), Dir::Down, Support::Full)
}

// HangingRootsBlock.updateShape: a waterlogged state schedules a water tick.
fn update_shape(l: &mut Level, state: State, pos: Pos, direction: Dir, _neighbor_pos: Pos, _neighbor: State) -> State {
    if direction == Dir::Up && !can_survive(l, state, pos) {
        l.known.air
    } else {
        schedule_water_tick(l, state, pos);
        state
    }
}
