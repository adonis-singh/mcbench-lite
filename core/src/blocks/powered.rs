// PoweredBlock: the redstone block.

use super::{Behaviour, NONE};
use crate::dir::{Dir, Pos};
use crate::level::Level;
use crate::registry::State;

pub const BEHAVIOUR: Behaviour = Behaviour { get_signal: Some(get_signal), ..NONE };

fn get_signal(_l: &mut Level, _state: State, _pos: Pos, _dir: Dir) -> i32 {
    15
}
