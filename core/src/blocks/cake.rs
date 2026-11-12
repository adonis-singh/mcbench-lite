// CakeBlock and CandleCakeBlock: canSurvive, updateShape and the signal.
// Eating, candles and extinguishing need a player.

use super::{Behaviour, NONE};
use crate::dir::{Dir, Pos};
use crate::level::Level;
use crate::property;
use crate::registry::{State, SOLID};

pub const CAKE: Behaviour = Behaviour {
    update_shape: Some(update_shape),
    can_survive: Some(can_survive),
    get_analog_output_signal: Some(cake_analog_output_signal),
    ..NONE
};

pub const CANDLE_CAKE: Behaviour = Behaviour {
    update_shape: Some(update_shape),
    can_survive: Some(can_survive),
    get_analog_output_signal: Some(candle_cake_analog_output_signal),
    ..NONE
};

// CakeBlock.BITES.
fn bites(l: &Level) -> u16 {
    property!(l, "bites", ["0", "1", "2", "3", "4", "5", "6"])
}

// CakeBlock.updateShape and CandleCakeBlock.updateShape (the same code);
// BlockBehaviour's returns the state.
fn update_shape(l: &mut Level, state: State, pos: Pos, direction: Dir, _neighbor_pos: Pos, _neighbor: State) -> State {
    if direction == Dir::Down && !can_survive(l, state, pos) {
        l.known.air
    } else {
        state
    }
}

// CakeBlock.canSurvive and CandleCakeBlock.canSurvive: a solid block below.
fn can_survive(l: &mut Level, _state: State, pos: Pos) -> bool {
    let below = l.get(pos.below());
    l.reg.flags(below) & SOLID != 0
}

// CakeBlock.getOutputSignal.
fn output_signal(eaten: i32) -> i32 {
    (7 - eaten) * 2
}

// CakeBlock.getAnalogOutputSignal.
fn cake_analog_output_signal(l: &mut Level, state: State, _pos: Pos, _direction: Dir) -> i32 {
    output_signal(l.reg.get_int(state, bites(l)))
}

// CandleCakeBlock.getAnalogOutputSignal: CakeBlock.FULL_CAKE_SIGNAL.
fn candle_cake_analog_output_signal(_l: &mut Level, _state: State, _pos: Pos, _direction: Dir) -> i32 {
    output_signal(0)
}
