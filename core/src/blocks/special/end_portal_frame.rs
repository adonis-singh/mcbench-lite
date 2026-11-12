// EndPortalFrameBlock: an analog output of 15 with an eye.

use crate::blocks::{Behaviour, NONE};
use crate::dir::{Dir, Pos};
use crate::level::Level;
use crate::property;
use crate::registry::State;

pub const BEHAVIOUR: Behaviour = Behaviour { get_analog_output_signal: Some(get_analog_output_signal), ..NONE };

fn eye(l: &Level) -> u16 {
    property!(l, "eye", ["true", "false"])
}

fn get_analog_output_signal(l: &mut Level, state: State, _pos: Pos, _dir: Dir) -> i32 {
    if l.reg.get_bool(state, eye(l)) {
        15
    } else {
        0
    }
}
