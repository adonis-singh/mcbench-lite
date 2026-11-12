// WallBannerBlock.

use super::common::{get_facing, is_solid};
use super::sign;
use crate::blocks::{Behaviour, NONE};
use crate::dir::{Dir, Pos};
use crate::level::Level;
use crate::registry::State;

pub const BEHAVIOUR: Behaviour = Behaviour {
    update_shape: Some(update_shape),
    can_survive: Some(can_survive),
    trigger_event: Some(sign::trigger_event),
    new_block_entity: Some(super::banner::new_block_entity),
    ..NONE
};

// WallBannerBlock.canSurvive.
fn can_survive(l: &mut Level, state: State, pos: Pos) -> bool {
    let behind = l.get(pos.relative(get_facing(l, state).opposite()));
    is_solid(l, behind)
}

// WallBannerBlock.updateShape.
fn update_shape(l: &mut Level, state: State, pos: Pos, direction: Dir, _neighbor_pos: Pos, _neighbor: State) -> State {
    if direction == get_facing(l, state).opposite() && !can_survive(l, state, pos) {
        l.known.air
    } else {
        state
    }
}
