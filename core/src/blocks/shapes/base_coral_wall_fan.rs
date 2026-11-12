// BaseCoralWallFanBlock (the dead wall fans).

use super::common::{get_facing, schedule_water_tick};
use super::coral;
use crate::blocks::{Behaviour, NONE};
use crate::dir::{Dir, Pos};
use crate::level::Level;
use crate::registry::State;

pub const BEHAVIOUR: Behaviour = Behaviour { update_shape: Some(update_shape), can_survive: Some(coral::wall_can_survive), ..NONE };

// BaseCoralWallFanBlock.updateShape.
fn update_shape(l: &mut Level, state: State, pos: Pos, direction: Dir, _neighbor_pos: Pos, _neighbor: State) -> State {
    schedule_water_tick(l, state, pos);
    if direction.opposite() == get_facing(l, state) && !coral::wall_can_survive(l, state, pos) {
        l.known.air
    } else {
        state
    }
}
