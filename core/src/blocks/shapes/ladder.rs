// LadderBlock.

use super::common::{get_facing, schedule_water_tick};
use crate::blocks::{Behaviour, NONE};
use crate::dir::{Dir, Pos};
use crate::level::Level;
use crate::registry::{State, Support};

pub const BEHAVIOUR: Behaviour = Behaviour { update_shape: Some(update_shape), can_survive: Some(can_survive), ..NONE };

// LadderBlock.canSurvive: canAttachTo the block behind.
fn can_survive(l: &mut Level, state: State, pos: Pos) -> bool {
    let facing = get_facing(l, state);
    let behind_pos = pos.relative(facing.opposite());
    let behind = l.get(behind_pos);
    crate::shape_ops::is_face_sturdy(l, behind, behind_pos, facing, Support::Full)
}

// LadderBlock.updateShape.
fn update_shape(l: &mut Level, state: State, pos: Pos, direction: Dir, _neighbor_pos: Pos, _neighbor: State) -> State {
    if direction.opposite() == get_facing(l, state) && !can_survive(l, state, pos) {
        l.known.air
    } else {
        schedule_water_tick(l, state, pos);
        state
    }
}
