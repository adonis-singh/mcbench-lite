// RespawnAnchorBlock: its analog output is its charge, scaled to 15. Using
// it needs a player.

use crate::blocks::{Behaviour, NONE};
use crate::dir::{Dir, Pos};
use crate::level::Level;
use crate::property;
use crate::registry::State;

pub const BEHAVIOUR: Behaviour = Behaviour { get_analog_output_signal: Some(get_analog_output_signal), ..NONE };

fn charges(l: &Level) -> u16 {
    property!(l, "charges", ["0", "1", "2", "3", "4"])
}

// RespawnAnchorBlock.getScaledChargeLevel(state, 15).
fn get_analog_output_signal(l: &mut Level, state: State, _pos: Pos, _dir: Dir) -> i32 {
    ((l.reg.get_int(state, charges(l)) as f32 / 4.0f32 * 15.0f32).floor()) as i32
}
