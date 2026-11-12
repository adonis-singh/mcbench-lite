// BannerBlock.

use super::common::is_solid;
use super::sign;
use crate::blocks::{Behaviour, BlockEntity, NONE};
use crate::dir::{Dir, Pos};
use crate::level::Level;
use crate::registry::State;

pub const BEHAVIOUR: Behaviour = Behaviour {
    update_shape: Some(update_shape),
    can_survive: Some(can_survive),
    trigger_event: Some(sign::trigger_event),
    new_block_entity: Some(new_block_entity),
    ..NONE
};

// AbstractBannerBlock.newBlockEntity: BannerBlockEntity.
pub fn new_block_entity(_l: &mut Level, _state: State, _pos: Pos) -> BlockEntity {
    BlockEntity::Banner { name: None, patterns: Vec::new() }
}

// BannerBlock.canSurvive.
fn can_survive(l: &mut Level, _state: State, pos: Pos) -> bool {
    let below = l.get(pos.below());
    is_solid(l, below)
}

// BannerBlock.updateShape.
fn update_shape(l: &mut Level, state: State, pos: Pos, direction: Dir, _neighbor_pos: Pos, _neighbor: State) -> State {
    if direction == Dir::Down && !can_survive(l, state, pos) {
        l.known.air
    } else {
        state
    }
}
