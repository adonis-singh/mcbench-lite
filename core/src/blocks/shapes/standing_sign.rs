// StandingSignBlock.

use super::common::is_solid;
use super::sign;
use crate::blocks::{Behaviour, NONE};
use crate::dir::{Dir, Pos};
use crate::level::Level;
use crate::registry::State;

pub const BEHAVIOUR: Behaviour = Behaviour {
    update_shape: Some(update_shape),
    can_survive: Some(can_survive),
    trigger_event: Some(sign::trigger_event),
    new_block_entity: Some(sign::new_block_entity),
    ticker: Some(sign::ticker),
    ..NONE
};

// StandingSignBlock.canSurvive.
fn can_survive(l: &mut Level, _state: State, pos: Pos) -> bool {
    let below = l.get(pos.below());
    is_solid(l, below)
}

// StandingSignBlock.updateShape.
fn update_shape(l: &mut Level, state: State, pos: Pos, direction: Dir, _neighbor_pos: Pos, _neighbor: State) -> State {
    if direction == Dir::Down && !can_survive(l, state, pos) {
        l.known.air
    } else {
        sign::update_shape(l, state, pos)
    }
}
