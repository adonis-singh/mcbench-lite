// MushroomBlock: stands on blocks that grow mushrooms, or on a solid-render
// block in the dark (light under 13).

use crate::blocks::vegetation;
use crate::blocks::Behaviour;
use crate::dir::Pos;
use crate::in_tag;
use crate::level::Level;
use crate::registry::{State, SOLID_RENDER};

pub const BEHAVIOUR: Behaviour = vegetation::behaviour(can_survive);

// MushroomBlock.canSurvive.
fn can_survive(l: &mut Level, _state: State, pos: Pos) -> bool {
    let below = l.get(pos.below());
    in_tag!(l, below, "minecraft:mushroom_grow_block") || l.raw_brightness(pos, 0) < 13 && l.reg.flags(below) & SOLID_RENDER != 0
}
