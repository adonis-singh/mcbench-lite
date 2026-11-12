// AmethystClusterBlock (and the buds): attached to the block behind it.

use crate::blocks::shapes::common::schedule_water_tick;
use crate::blocks::{Behaviour, NONE};
use crate::dir::{Dir, Pos};
use crate::level::Level;
use crate::property;
use crate::registry::State;

pub const BEHAVIOUR: Behaviour = Behaviour { update_shape: Some(update_shape), can_survive: Some(can_survive), ..NONE };

fn facing(l: &Level) -> u16 {
    property!(l, "facing", ["north", "east", "south", "west", "up", "down"])
}

// AmethystClusterBlock.canSurvive.
fn can_survive(l: &mut Level, state: State, pos: Pos) -> bool {
    let direction = l.reg.get_dir(state, facing(l));
    let behind_pos = pos.relative(direction.opposite());
    let behind = l.get(behind_pos);
    crate::shape_ops::is_face_sturdy(l, behind, behind_pos, direction, crate::registry::Support::Full)
}

// AmethystClusterBlock.updateShape: a waterlogged cluster schedules a water
// tick.
fn update_shape(l: &mut Level, state: State, pos: Pos, dir: Dir, _neighbor_pos: Pos, _neighbor: State) -> State {
    schedule_water_tick(l, state, pos);
    if dir == l.reg.get_dir(state, facing(l)).opposite() && !can_survive(l, state, pos) {
        l.known.air
    } else {
        state
    }
}
