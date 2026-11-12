// DryVegetationBlock (and ShortDryGrassBlock, TallDryGrassBlock): the tag of
// blocks dry vegetation stands on.

use crate::blocks::vegetation;
use crate::blocks::Behaviour;
use crate::dir::Pos;
use crate::in_tag;
use crate::level::Level;
use crate::registry::State;

pub const BEHAVIOUR: Behaviour = vegetation::behaviour(can_survive);

fn can_survive(l: &mut Level, _state: State, pos: Pos) -> bool {
    vegetation::can_survive(l, pos, may_place_on)
}

// DryVegetationBlock.mayPlaceOn.
fn may_place_on(l: &mut Level, state: State, _pos: Pos) -> bool {
    in_tag!(l, state, "minecraft:dry_vegetation_may_place_on")
}
