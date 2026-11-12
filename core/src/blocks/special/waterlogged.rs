// The classes whose only engine-relevant override is updateShape scheduling a
// water tick when waterlogged (BarrierBlock, LightBlock, HeavyCoreBlock,
// ConduitBlock, DecoratedPotBlock, LightningRodBlock...).

use crate::blocks::shapes::common::schedule_water_tick;
use crate::blocks::{Behaviour, NONE};
use crate::dir::{Dir, Pos};
use crate::level::Level;
use crate::registry::State;

pub const BEHAVIOUR: Behaviour = Behaviour { update_shape: Some(update_shape), ..NONE };

pub fn update_shape(l: &mut Level, state: State, pos: Pos, _dir: Dir, _neighbor_pos: Pos, _neighbor: State) -> State {
    schedule_water_tick(l, state, pos);
    state
}
