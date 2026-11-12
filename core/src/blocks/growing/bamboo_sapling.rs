// BambooSaplingBlock: stands on the bamboo_plantable_on blocks, and becomes
// bamboo when bamboo grows above it.

use crate::blocks::{Behaviour, NONE};
use crate::dir::{Dir, Pos};
use crate::in_tag;
use crate::is_block;
use crate::level::Level;
use crate::registry::State;

pub const BEHAVIOUR: Behaviour = Behaviour { update_shape: Some(update_shape), can_survive: Some(can_survive), ..NONE };

// BambooSaplingBlock.canSurvive.
fn can_survive(l: &mut Level, _state: State, pos: Pos) -> bool {
    let below = l.get(pos.below());
    in_tag!(l, below, "minecraft:bamboo_plantable_on")
}

// BambooSaplingBlock.updateShape.
fn update_shape(l: &mut Level, state: State, pos: Pos, direction: Dir, _neighbor_pos: Pos, neighbor: State) -> State {
    if !can_survive(l, state, pos) {
        l.known.air
    } else if direction == Dir::Up && is_block!(l, neighbor, "minecraft:bamboo") {
        l.reg.default_state(crate::block_id!(l, "minecraft:bamboo"))
    } else {
        state
    }
}
