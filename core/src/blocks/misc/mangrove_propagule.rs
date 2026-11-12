// MangrovePropaguleBlock (a SaplingBlock): a hanging propagule needs
// mangrove leaves above it, a standing one dirt, farmland or clay below.
// randomTick (growth) never runs.

use crate::blocks::shapes::common::schedule_water_tick;
use crate::blocks::vegetation;
use crate::blocks::{Behaviour, NONE};
use crate::dir::{Dir, Pos};
use crate::is_block;
use crate::level::Level;
use crate::property;
use crate::registry::State;

pub const BEHAVIOUR: Behaviour = Behaviour { update_shape: Some(update_shape), can_survive: Some(can_survive), ..NONE };

// MangrovePropaguleBlock.HANGING.
fn is_hanging(l: &Level, state: State) -> bool {
    l.reg.get_bool(state, property!(l, "hanging", ["true", "false"]))
}

// MangrovePropaguleBlock.mayPlaceOn: VegetationBlock's or clay.
fn may_place_on(l: &mut Level, state: State, _pos: Pos) -> bool {
    vegetation::may_place_on(l, state) || is_block!(l, state, "minecraft:clay")
}

// MangrovePropaguleBlock.canSurvive.
fn can_survive(l: &mut Level, state: State, pos: Pos) -> bool {
    if is_hanging(l, state) {
        let above = l.get(pos.above());
        is_block!(l, above, "minecraft:mangrove_leaves")
    } else {
        vegetation::can_survive(l, pos, may_place_on)
    }
}

// MangrovePropaguleBlock.updateShape: a waterlogged propagule schedules a
// water tick.
fn update_shape(l: &mut Level, state: State, pos: Pos, direction: Dir, neighbor_pos: Pos, neighbor: State) -> State {
    schedule_water_tick(l, state, pos);
    if direction == Dir::Up && !can_survive(l, state, pos) {
        l.known.air
    } else {
        vegetation::update_shape(l, state, pos, direction, neighbor_pos, neighbor)
    }
}
