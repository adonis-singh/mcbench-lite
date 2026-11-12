// LanternBlock (and WeatheringLanternBlock): hangs from or stands on a block
// with a sturdy centre.

use super::can_support_center;
use crate::blocks::shapes::common::schedule_water_tick;
use crate::blocks::{Behaviour, NONE};
use crate::dir::{Dir, Pos};
use crate::level::Level;
use crate::property;
use crate::registry::State;

pub const BEHAVIOUR: Behaviour = Behaviour { update_shape: Some(update_shape), can_survive: Some(can_survive), ..NONE };

// LanternBlock.getConnectedDirection: DOWN when hanging, else UP.
fn connected_direction(l: &Level, state: State) -> Dir {
    if l.reg.get_bool(state, property!(l, "hanging", ["true", "false"])) {
        Dir::Down
    } else {
        Dir::Up
    }
}

// LanternBlock.canSurvive.
fn can_survive(l: &mut Level, state: State, pos: Pos) -> bool {
    let opposite = connected_direction(l, state).opposite();
    can_support_center(l, pos.relative(opposite), opposite.opposite())
}

// LanternBlock.updateShape.
fn update_shape(l: &mut Level, state: State, pos: Pos, direction: Dir, _neighbor_pos: Pos, _neighbor: State) -> State {
    schedule_water_tick(l, state, pos);
    if connected_direction(l, state).opposite() == direction && !can_survive(l, state, pos) {
        l.known.air
    } else {
        state
    }
}
