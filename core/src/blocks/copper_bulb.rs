// CopperBulbBlock, and WeatheringCopperBulbBlock on top of it (its random
// ticks do not run: randomTickSpeed is 0 and this level ticks no chunk
// randomly).

use super::{Behaviour, NONE};
use crate::dir::{Dir, Pos};
use crate::level::{Level, UPDATE_ALL};
use crate::property;
use crate::registry::{BlockId, State};

pub const BEHAVIOUR: Behaviour = Behaviour {
    on_place: Some(on_place),
    neighbor_changed: Some(neighbor_changed),
    get_analog_output_signal: Some(get_analog_output_signal),
    ..NONE
};

fn powered(l: &Level) -> u16 {
    property!(l, "powered", ["true", "false"])
}

fn lit(l: &Level) -> u16 {
    property!(l, "lit", ["true", "false"])
}

// CopperBulbBlock.onPlace (the level is a ServerLevel).
fn on_place(l: &mut Level, state: State, pos: Pos, old: State, _moved: bool) {
    if l.block(old) != l.block(state) {
        check_and_flip(l, state, pos);
    }
}

// CopperBulbBlock.neighborChanged.
fn neighbor_changed(l: &mut Level, state: State, pos: Pos, _neighbor: BlockId, _moved: bool) {
    check_and_flip(l, state, pos);
}

// CopperBulbBlock.checkAndFlip: a rising signal flips the light. The sound
// reaches nothing.
fn check_and_flip(l: &mut Level, state: State, pos: Pos) {
    let has_neighbor_signal = l.has_neighbor_signal(pos);
    let powered_value = l.reg.get_bool(state, powered(l));
    if has_neighbor_signal != powered_value {
        let mut new = state;
        if !powered_value {
            // BlockState.cycle(LIT).
            new = l.reg.with_bool(new, lit(l), !l.reg.get_bool(new, lit(l)));
        }
        l.set(pos, l.reg.with_bool(new, powered(l), has_neighbor_signal), UPDATE_ALL);
    }
}

// CopperBulbBlock.getAnalogOutputSignal.
fn get_analog_output_signal(l: &mut Level, _state: State, pos: Pos, _direction: Dir) -> i32 {
    let state = l.get(pos);
    if l.reg.get_bool(state, lit(l)) {
        15
    } else {
        0
    }
}
