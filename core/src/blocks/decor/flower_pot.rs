// FlowerPotBlock: updateShape destroys the pot when it cannot survive, and
// FlowerPotBlock does not override canSurvive (it always can). The potted
// eyeblossoms change only by random tick.

use crate::blocks::{Behaviour, NONE};
use crate::dir::{Dir, Pos};
use crate::level::Level;
use crate::registry::State;

pub const BEHAVIOUR: Behaviour = Behaviour { update_shape: Some(update_shape), ..NONE };

fn update_shape(_l: &mut Level, state: State, _pos: Pos, _direction: Dir, _neighbor_pos: Pos, _neighbor: State) -> State {
    state
}
