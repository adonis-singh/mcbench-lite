// SeaPickleBlock: stands on a block with a non-empty top collision face or a
// sturdy top; a waterlogged one schedules a water tick, which is not
// modelled.

use crate::blocks::shapes::common::schedule_water_tick;
use crate::blocks::vegetation;
use crate::blocks::Behaviour;
use crate::blocks::NONE;
use crate::dir::{Dir, Pos};
use crate::level::Level;
use crate::registry::{State, Support};
use crate::shape_ops;

pub const BEHAVIOUR: Behaviour = Behaviour { update_shape: Some(update_shape), can_survive: Some(can_survive), ..NONE };

// SeaPickleBlock.mayPlaceOn.
fn may_place_on(l: &mut Level, state: State, pos: Pos) -> bool {
    !shape_ops::collision_shape_at(l, state, pos).face_shape(Dir::Up).is_empty() || shape_ops::is_face_sturdy(l, state, pos, Dir::Up, Support::Full)
}

fn can_survive(l: &mut Level, _state: State, pos: Pos) -> bool {
    vegetation::can_survive(l, pos, may_place_on)
}

// SeaPickleBlock.updateShape; the final call is VegetationBlock.updateShape,
// which asks canSurvive again.
fn update_shape(l: &mut Level, state: State, pos: Pos, direction: Dir, neighbor_pos: Pos, neighbor: State) -> State {
    if !can_survive(l, state, pos) {
        l.known.air
    } else {
        schedule_water_tick(l, state, pos);
        vegetation::update_shape(l, state, pos, direction, neighbor_pos, neighbor)
    }
}
