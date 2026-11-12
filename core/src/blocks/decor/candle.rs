// CandleBlock: updateShape schedules the water tick of a waterlogged candle
// and nothing else; canSurvive is read
// only when placing.

use super::can_support_center;
use crate::blocks::shapes::common::{schedule_water_tick, waterlogged};
use crate::blocks::{Behaviour, NONE};
use crate::dir::{Dir, Pos};
use crate::level::{Level, UPDATE_ALL};
use crate::property;
use crate::registry::{Fluid, State};

pub const BEHAVIOUR: Behaviour = Behaviour { update_shape: Some(update_shape), can_survive: Some(can_survive), ..NONE };

// CandleBlock.canSurvive.
fn can_survive(l: &mut Level, _state: State, pos: Pos) -> bool {
    can_support_center(l, pos.below(), Dir::Up)
}

// CandleBlock.updateShape.
fn update_shape(l: &mut Level, state: State, pos: Pos, _direction: Dir, _neighbor_pos: Pos, _neighbor: State) -> State {
    schedule_water_tick(l, state, pos);
    state
}

// CandleBlock.placeLiquid: water waterlogs the candle, which puts out a lit
// one (AbstractCandleBlock.extinguish: setLit(false) with flags 11; its
// particles and sound draw nothing from the level's random), then schedules
// the fluid tick. The extinguish ends with a block change event that sculk
// sensors hear.
pub fn place_liquid(l: &mut Level, pos: Pos, state: State, fluid: Fluid) -> bool {
    let waterlogged = waterlogged(l);
    if !l.reg.get_bool(state, waterlogged) && fluid == Fluid::Water {
        let wet = l.reg.with_bool(state, waterlogged, true);
        let lit = property!(l, "lit", ["true", "false"]);
        if l.reg.get_bool(state, lit) {
            l.set(pos, l.reg.with_bool(wet, lit, false), UPDATE_ALL | 8);
            l.game_event_at(None, crate::game_event!(l, "block_change"), pos);
        } else {
            l.set(pos, wet, UPDATE_ALL);
        }
        l.schedule_fluid_tick(pos, fluid, crate::fluid::tick_delay(fluid));
        true
    } else {
        false
    }
}
