// PitcherCropBlock: DoublePlantBlock with a light requirement on the lower
// half (CropBlock.hasSufficientLight).

use super::double_plant;
use crate::blocks::vegetation;
use crate::blocks::{Behaviour, NONE};
use crate::dir::{Dir, Pos};
use crate::is_block;
use crate::level::Level;
use crate::property;
use crate::registry::State;

pub const BEHAVIOUR: Behaviour = Behaviour { update_shape: Some(update_shape), can_survive: Some(can_survive), ..NONE };

fn age(l: &Level, state: State) -> i32 {
    l.reg.get_int(state, property!(l, "age", ["0", "1", "2", "3", "4"]))
}

// PitcherCropBlock.isLower.
fn is_lower(l: &Level, state: State) -> bool {
    is_block!(l, state, "minecraft:pitcher_crop") && l.reg.get(state, double_plant::half(l)) == double_plant::LOWER
}

// PitcherCropBlock.updateShape: a double plant (age 3 and over) is a
// DoublePlantBlock; a single one only needs to survive.
fn update_shape(l: &mut Level, state: State, pos: Pos, direction: Dir, neighbor_pos: Pos, neighbor: State) -> State {
    if age(l, state) >= 3 {
        double_plant::update_shape(l, state, pos, direction, neighbor_pos, neighbor)
    } else if crate::blocks::can_survive(l, state, pos) {
        state
    } else {
        l.known.air
    }
}

// PitcherCropBlock.canSurvive: (!isLower(state) || sufficientLight) &&
// DoublePlantBlock.canSurvive, whose lower half stands on farmland.
fn can_survive(l: &mut Level, state: State, pos: Pos) -> bool {
    if is_lower(l, state) && !super::crop::has_sufficient_light(l, pos) {
        return false;
    }
    if l.reg.get(state, double_plant::half(l)) != double_plant::UPPER {
        vegetation::can_survive(l, pos, super::stem::may_place_on)
    } else {
        double_plant::can_survive(l, state, pos)
    }
}
