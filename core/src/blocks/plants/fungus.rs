// FungusBlock: nylium, mycelium and soul soil as well as dirt.

use crate::blocks::vegetation;
use crate::blocks::Behaviour;
use crate::dir::Pos;
use crate::level::Level;
use crate::registry::State;
use crate::{in_tag, is_block};

pub const BEHAVIOUR: Behaviour = vegetation::behaviour(can_survive);

fn can_survive(l: &mut Level, _state: State, pos: Pos) -> bool {
    vegetation::can_survive(l, pos, may_place_on)
}

// FungusBlock.mayPlaceOn.
fn may_place_on(l: &mut Level, state: State, _pos: Pos) -> bool {
    in_tag!(l, state, "minecraft:nylium") || is_block!(l, state, "minecraft:mycelium") || is_block!(l, state, "minecraft:soul_soil") || vegetation::may_place_on(l, state)
}
