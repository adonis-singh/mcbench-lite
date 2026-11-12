// CocoaBlock: attached to a jungle log it faces.

use crate::blocks::{Behaviour, NONE};
use crate::dir::{Dir, Pos};
use crate::in_tag;
use crate::level::Level;
use crate::property;
use crate::registry::State;

pub const BEHAVIOUR: Behaviour = Behaviour { update_shape: Some(update_shape), can_survive: Some(can_survive), ..NONE };

fn facing(l: &Level, state: State) -> Dir {
    l.reg.get_dir(state, property!(l, "facing", ["north", "south", "west", "east"]))
}

// CocoaBlock.canSurvive.
fn can_survive(l: &mut Level, state: State, pos: Pos) -> bool {
    let at = l.get(pos.relative(facing(l, state)));
    in_tag!(l, at, "minecraft:jungle_logs")
}

// CocoaBlock.updateShape (the HorizontalDirectionalBlock's is BlockBehaviour's).
fn update_shape(l: &mut Level, state: State, pos: Pos, direction: Dir, _neighbor_pos: Pos, _neighbor: State) -> State {
    if direction == facing(l, state) && !can_survive(l, state, pos) {
        l.known.air
    } else {
        state
    }
}
