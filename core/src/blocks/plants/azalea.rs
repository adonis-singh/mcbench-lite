// AzaleaBlock: clay as well as dirt.

use crate::blocks::vegetation;
use crate::blocks::Behaviour;
use crate::dir::Pos;
use crate::is_block;
use crate::level::Level;
use crate::registry::State;

pub const BEHAVIOUR: Behaviour = vegetation::behaviour(can_survive);

fn can_survive(l: &mut Level, _state: State, pos: Pos) -> bool {
    vegetation::can_survive(l, pos, may_place_on)
}

// AzaleaBlock.mayPlaceOn.
fn may_place_on(l: &mut Level, state: State, _pos: Pos) -> bool {
    is_block!(l, state, "minecraft:clay") || vegetation::may_place_on(l, state)
}
