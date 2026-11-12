// SeagrassBlock and TallSeagrassBlock (on VegetationBlock and
// DoublePlantBlock): plants that stand in water on a sturdy block.

use super::{Behaviour, NONE};
use crate::dir::{Dir, Pos};
use crate::fluid::{named, WATER_TICK_DELAY};
use crate::level::Level;
use crate::registry::{Fluid, State, Support};

pub const BEHAVIOUR: Behaviour = Behaviour { update_shape: Some(update_shape), can_survive: Some(can_survive), ..NONE };
pub const TALL_BEHAVIOUR: Behaviour = Behaviour { update_shape: Some(tall_update_shape), can_survive: Some(tall_can_survive), ..NONE };

// SeagrassBlock.mayPlaceOn and TallSeagrassBlock.mayPlaceOn: a sturdy top
// that is not a magma block.
fn may_place_on(l: &Level, state: State, pos: Pos) -> bool {
    crate::shape_ops::is_face_sturdy(l, state, pos, Dir::Up, Support::Full) && l.block(state) != named(l).magma_block
}

// VegetationBlock.canSurvive.
fn vegetation_can_survive(l: &mut Level, pos: Pos) -> bool {
    let below = l.get(pos.below());
    may_place_on(l, below, pos.below())
}

fn can_survive(l: &mut Level, _state: State, pos: Pos) -> bool {
    vegetation_can_survive(l, pos)
}

// VegetationBlock.updateShape: a plant that cannot survive becomes air
// (BlockPhysicsEvent has no listener).
fn vegetation_update_shape(l: &mut Level, state: State, pos: Pos) -> State {
    if super::can_survive(l, state, pos) {
        state
    } else {
        l.known.air
    }
}

// SeagrassBlock.updateShape.
fn update_shape(l: &mut Level, state: State, pos: Pos, _direction: Dir, _neighbor_pos: Pos, _neighbor: State) -> State {
    let shaped = vegetation_update_shape(l, state, pos);
    if !l.is_air(shaped) {
        l.schedule_fluid_tick(pos, Fluid::Water, WATER_TICK_DELAY);
    }
    shaped
}

fn is_upper(l: &Level, state: State) -> bool {
    l.reg.value_name(state, l.reg.property_of(state, "half")) == "upper"
}

// DoublePlantBlock.updateShape.
fn tall_update_shape(l: &mut Level, state: State, pos: Pos, direction: Dir, _neighbor_pos: Pos, neighbor: State) -> State {
    let upper = is_upper(l, state);
    let same_other_half = l.block(neighbor) == l.block(state) && is_upper(l, neighbor) != upper;
    if direction.axis() != crate::dir::Axis::Y || (!upper) != (direction == Dir::Up) || same_other_half {
        if !upper && direction == Dir::Down && !super::can_survive(l, state, pos) {
            l.known.air
        } else {
            vegetation_update_shape(l, state, pos)
        }
    } else {
        l.known.air
    }
}

// TallSeagrassBlock.canSurvive: the upper half needs its lower half, the
// lower stands in full water on a sturdy block.
fn tall_can_survive(l: &mut Level, state: State, pos: Pos) -> bool {
    if is_upper(l, state) {
        let below = l.get(pos.below());
        l.block(below) == l.block(state) && !is_upper(l, below)
    } else {
        let fluid = l.fluid_state(pos);
        // DoublePlantBlock.canSurvive of a lower half is VegetationBlock's.
        let survives = vegetation_can_survive(l, pos);
        survives && l.fluid_type(fluid).is_water() && l.reg.fluids[fluid as usize].amount == 8
    }
}
