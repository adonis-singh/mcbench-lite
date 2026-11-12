// HugeMushroomBlock: the faces touching another block of its kind are hidden
// (Paper's block-updates.disable-mushroom-block-updates is off).

use crate::blocks::{Behaviour, NONE};
use crate::dir::{Dir, Pos};
use crate::level::Level;
use crate::registry::State;

pub const BEHAVIOUR: Behaviour = Behaviour { update_shape: Some(update_shape), ..NONE };

// HugeMushroomBlock.updateShape: PipeBlock.PROPERTY_BY_DIRECTION[direction] is false.
fn update_shape(l: &mut Level, state: State, _pos: Pos, direction: Dir, _neighbor_pos: Pos, neighbor: State) -> State {
    if l.block(neighbor) == l.block(state) {
        let property = l.reg.property_of(state, direction.name());
        l.reg.with_bool(state, property, false)
    } else {
        state
    }
}
