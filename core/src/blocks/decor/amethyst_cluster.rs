// AmethystClusterBlock: attached to the sturdy face it points away from.

use crate::blocks::shapes::common::schedule_water_tick;
use crate::blocks::{Behaviour, NONE};
use crate::dir::{Dir, Pos};
use crate::level::Level;
use crate::registry::{State, Support};

pub const BEHAVIOUR: Behaviour = Behaviour { update_shape: Some(update_shape), can_survive: Some(can_survive), ..NONE };

fn facing(l: &Level, state: State) -> Dir {
    l.reg.get_dir(state, l.reg.property_of(state, "facing"))
}

// AmethystClusterBlock.canSurvive: the block behind is sturdy toward the cluster.
fn can_survive(l: &mut Level, state: State, pos: Pos) -> bool {
    let direction = facing(l, state);
    let behind_pos = pos.relative(direction.opposite());
    let behind = l.get(behind_pos);
    crate::shape_ops::is_face_sturdy(l, behind, behind_pos, direction, Support::Full)
}

// AmethystClusterBlock.updateShape.
fn update_shape(l: &mut Level, state: State, pos: Pos, direction: Dir, _neighbor_pos: Pos, _neighbor: State) -> State {
    schedule_water_tick(l, state, pos);
    if direction == facing(l, state).opposite() && !can_survive(l, state, pos) {
        l.known.air
    } else {
        state
    }
}
