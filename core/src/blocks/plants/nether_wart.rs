// NetherWartBlock: soul sand only.

use crate::blocks::vegetation;
use crate::blocks::Behaviour;
use crate::dir::Pos;
use crate::is_block;
use crate::level::Level;
use crate::registry::State;

pub const BEHAVIOUR: Behaviour = vegetation::behaviour(can_survive);

fn can_survive(l: &mut Level, _state: State, pos: Pos) -> bool {
    vegetation::can_survive(l, pos, |l, state, _| is_block!(l, state, "minecraft:soul_sand"))
}
