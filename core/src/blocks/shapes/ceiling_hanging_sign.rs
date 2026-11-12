// CeilingHangingSignBlock.

use super::sign;
use crate::blocks::{Behaviour, NONE};
use crate::dir::{Dir, Pos};
use crate::level::Level;
use crate::registry::{State, Support};

pub const BEHAVIOUR: Behaviour = Behaviour {
    update_shape: Some(update_shape),
    can_survive: Some(can_survive),
    trigger_event: Some(sign::trigger_event),
    new_block_entity: Some(sign::new_hanging_block_entity),
    ticker: Some(sign::ticker),
    ..NONE
};

// CeilingHangingSignBlock.canSurvive.
fn can_survive(l: &mut Level, _state: State, pos: Pos) -> bool {
    let above = l.get(pos.above());
    crate::shape_ops::is_face_sturdy(l, above, pos.above(), Dir::Down, Support::Center)
}

// CeilingHangingSignBlock.updateShape.
fn update_shape(l: &mut Level, state: State, pos: Pos, direction: Dir, _neighbor_pos: Pos, _neighbor: State) -> State {
    if direction == Dir::Up && !can_survive(l, state, pos) {
        l.known.air
    } else {
        sign::update_shape(l, state, pos)
    }
}
