// SmallDripleafBlock: a DoublePlantBlock whose lower half stands on clay or
// moss (or on dirt with a water source above).

use crate::blocks::shapes::common::schedule_water_tick;
use crate::blocks::plants::double_plant;
use crate::blocks::vegetation;
use crate::blocks::{Behaviour, NONE};
use crate::dir::{Dir, Pos};
use crate::in_tag;
use crate::level::Level;
use crate::registry::State;

pub const BEHAVIOUR: Behaviour = Behaviour { update_shape: Some(update_shape), can_survive: Some(can_survive), ..NONE };

// SmallDripleafBlock.mayPlaceOn: the tag, or (the fluid at pos.above() is a
// water source && VegetationBlock.mayPlaceOn). `pos` is the block below the
// plant, so pos.above() is the plant's own position.
fn may_place_on(l: &mut Level, state: State, pos: Pos) -> bool {
    if in_tag!(l, state, "minecraft:small_dripleaf_placeable") {
        return true;
    }
    let fluid = l.fluid_state(pos.above());
    let data = &l.reg.fluids[fluid as usize];
    data.fluid == crate::registry::Fluid::Water && data.source && vegetation::may_place_on(l, state)
}

// SmallDripleafBlock.canSurvive.
fn can_survive(l: &mut Level, state: State, pos: Pos) -> bool {
    if l.reg.get(state, double_plant::half(l)) == double_plant::UPPER {
        double_plant::can_survive(l, state, pos)
    } else {
        vegetation::can_survive(l, pos, may_place_on)
    }
}

// SmallDripleafBlock.updateShape: a waterlogged state schedules a water tick;
// then DoublePlantBlock.updateShape.
fn update_shape(l: &mut Level, state: State, pos: Pos, direction: Dir, neighbor_pos: Pos, neighbor: State) -> State {
    schedule_water_tick(l, state, pos);
    double_plant::update_shape(l, state, pos, direction, neighbor_pos, neighbor)
}
