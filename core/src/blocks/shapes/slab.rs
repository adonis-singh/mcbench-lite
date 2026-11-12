// SlabBlock (and WeatheringCopperSlabBlock, whose additions are random
// ticks).

use super::common::schedule_water_tick;
use crate::blocks::{Behaviour, NONE};
use crate::dir::{Dir, Pos};
use crate::level::Level;
use crate::registry::State;

pub const BEHAVIOUR: Behaviour = Behaviour { update_shape: Some(update_shape), ..NONE };

// SlabBlock.updateShape: a waterlogged slab schedules a water tick, then
// BlockBehaviour.updateShape keeps the state.
fn update_shape(l: &mut Level, state: State, pos: Pos, _direction: Dir, _neighbor_pos: Pos, _neighbor: State) -> State {
    schedule_water_tick(l, state, pos);
    state
}
