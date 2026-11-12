// CactusFlowerBlock: on a cactus, farmland or a block with a sturdy centre
// of its top.

use crate::blocks::vegetation;
use crate::blocks::Behaviour;
use crate::dir::{Dir, Pos};
use crate::is_block;
use crate::level::Level;
use crate::registry::{State, Support};

pub const BEHAVIOUR: Behaviour = vegetation::behaviour(can_survive);

fn can_survive(l: &mut Level, _state: State, pos: Pos) -> bool {
    vegetation::can_survive(l, pos, may_place_on)
}

// CactusFlowerBlock.mayPlaceOn: the block below the flower is the state.
fn may_place_on(l: &mut Level, state: State, pos: Pos) -> bool {
    is_block!(l, state, "minecraft:cactus") || is_block!(l, state, "minecraft:farmland") || crate::shape_ops::is_face_sturdy(l, state, pos, Dir::Up, Support::Center)
}
