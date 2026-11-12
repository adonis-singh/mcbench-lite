// GrindstoneBlock: FaceAttachedHorizontalDirectionalBlock.updateShape breaks
// a block that cannot survive where it is, and a grindstone always can
// (GrindstoneBlock.canSurvive).

use crate::blocks::{Behaviour, NONE};
use crate::dir::{Dir, Pos};
use crate::level::Level;
use crate::registry::State;

pub const BEHAVIOUR: Behaviour = Behaviour { update_shape: Some(update_shape), can_survive: Some(can_survive), ..NONE };

fn update_shape(_l: &mut Level, state: State, _pos: Pos, _dir: Dir, _neighbor_pos: Pos, _neighbor: State) -> State {
    state
}

fn can_survive(_l: &mut Level, _state: State, _pos: Pos) -> bool {
    true
}
