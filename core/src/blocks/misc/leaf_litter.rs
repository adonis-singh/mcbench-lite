// LeafLitterBlock: a VegetationBlock that stands on a block with a sturdy
// top.

use crate::blocks::vegetation;
use crate::blocks::Behaviour;
use crate::dir::{Dir, Pos};
use crate::level::Level;
use crate::registry::{State, Support};
use crate::shape_ops;

pub const BEHAVIOUR: Behaviour = vegetation::behaviour(can_survive);

// LeafLitterBlock.canSurvive: the block below has a sturdy top face.
fn can_survive(l: &mut Level, _state: State, pos: Pos) -> bool {
    let below = l.get(pos.below());
    shape_ops::is_face_sturdy(l, below, pos.below(), Dir::Up, Support::Full)
}
