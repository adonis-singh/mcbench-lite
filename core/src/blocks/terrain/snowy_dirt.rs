// SnowyDirtBlock (podzol) and its subclasses SpreadingSnowyDirtBlock,
// GrassBlock and MyceliumBlock, whose own code is random ticks, bonemeal and
// particles: updateShape sets snowy from the block above.

use crate::blocks::{Behaviour, NONE};
use crate::dir::{Dir, Pos};
use crate::in_tag;
use crate::level::Level;
use crate::property;
use crate::registry::State;

pub const BEHAVIOUR: Behaviour = Behaviour { update_shape: Some(update_shape), ..NONE };

// SnowyDirtBlock.updateShape: BlockState.is(BlockTags.SNOW) of the block above;
// the other sides are BlockBehaviour.updateShape, which gives the state.
fn update_shape(l: &mut Level, state: State, _pos: Pos, direction: Dir, _neighbor_pos: Pos, neighbor: State) -> State {
    if direction == Dir::Up {
        l.reg.with_bool(state, property!(l, "snowy", ["true", "false"]), in_tag!(l, neighbor, "minecraft:snow"))
    } else {
        state
    }
}
