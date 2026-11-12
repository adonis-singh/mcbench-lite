// CoralWallFanBlock.

use super::common::{get_facing, schedule_water_tick};
use super::coral;
use crate::blocks::{Behaviour, NONE};
use crate::dir::{Dir, Pos};
use crate::level::Level;
use crate::registry::State;

pub const BEHAVIOUR: Behaviour = Behaviour {
    update_shape: Some(update_shape),
    on_place: Some(on_place),
    tick: Some(coral::wall_tick),
    can_survive: Some(coral::wall_can_survive),
    ..NONE
};

// CoralWallFanBlock.onPlace.
fn on_place(l: &mut Level, state: State, pos: Pos, _old: State, _moved: bool) {
    coral::try_schedule_die_tick(l, state, pos);
}

// CoralWallFanBlock.updateShape.
fn update_shape(l: &mut Level, state: State, pos: Pos, direction: Dir, _neighbor_pos: Pos, _neighbor: State) -> State {
    if direction.opposite() == get_facing(l, state) && !coral::wall_can_survive(l, state, pos) {
        l.known.air
    } else {
        schedule_water_tick(l, state, pos);
        coral::try_schedule_die_tick(l, state, pos);
        // BaseCoralWallFanBlock.updateShape (super): the water tick again.
        schedule_water_tick(l, state, pos);
        state
    }
}
