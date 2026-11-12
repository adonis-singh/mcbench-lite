// CarpetBlock (and WoolCarpetBlock): gone when nothing is below.

use crate::blocks::{Behaviour, NONE};
use crate::dir::{Dir, Pos};
use crate::level::Level;
use crate::registry::State;

pub const BEHAVIOUR: Behaviour = Behaviour { update_shape: Some(update_shape), can_survive: Some(can_survive), ..NONE };

// CarpetBlock.canSurvive: !level.isEmptyBlock(pos.below()).
fn can_survive(l: &mut Level, _state: State, pos: Pos) -> bool {
    let below = l.get(pos.below());
    !l.is_air(below)
}

// CarpetBlock.updateShape: any neighbour's change tests the carpet.
fn update_shape(l: &mut Level, state: State, pos: Pos, _direction: Dir, _neighbor_pos: Pos, _neighbor: State) -> State {
    if !can_survive(l, state, pos) {
        l.known.air
    } else {
        state
    }
}
