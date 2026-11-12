// StemBlock: farmland only. Its growth is random ticks.

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

// StemBlock.mayPlaceOn, AttachedStemBlock.mayPlaceOn, CropBlock.mayPlaceOn.
pub fn may_place_on(l: &mut Level, state: State, _pos: Pos) -> bool {
    is_block!(l, state, "minecraft:farmland")
}
