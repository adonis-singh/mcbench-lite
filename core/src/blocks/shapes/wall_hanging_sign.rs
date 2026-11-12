// WallHangingSignBlock. It has no canSurvive of its own, so updateShape's
// check of it (BlockBehaviour.canSurvive) always passes.

use super::sign;
use crate::blocks::{Behaviour, NONE};
use crate::dir::{Dir, Pos};
use crate::level::Level;
use crate::registry::State;

pub const BEHAVIOUR: Behaviour = Behaviour {
    update_shape: Some(update_shape),
    trigger_event: Some(sign::trigger_event),
    new_block_entity: Some(sign::new_hanging_block_entity),
    ticker: Some(sign::ticker),
    ..NONE
};

// WallHangingSignBlock.updateShape.
fn update_shape(l: &mut Level, state: State, pos: Pos, _direction: Dir, _neighbor_pos: Pos, _neighbor: State) -> State {
    sign::update_shape(l, state, pos)
}
