// DoublePlantBlock (and TallFlowerBlock, which adds only bonemeal): the
// two halves, each holding the other.

use crate::blocks::vegetation;
use crate::blocks::{Behaviour, NONE};
use crate::dir::{Axis, Dir, Pos};
use crate::level::Level;
use crate::property;
use crate::registry::State;

pub const BEHAVIOUR: Behaviour = Behaviour { update_shape: Some(update_shape), can_survive: Some(can_survive), ..NONE };

// DoublePlantBlock.HALF: values upper, lower.
pub fn half(l: &Level) -> u16 {
    property!(l, "half", ["upper", "lower"])
}

pub const UPPER: u16 = 0;
pub const LOWER: u16 = 1;

// DoublePlantBlock.updateShape: a half whose other half is gone is air; the
// lower half also needs to survive.
pub fn update_shape(l: &mut Level, state: State, pos: Pos, direction: Dir, neighbor_pos: Pos, neighbor: State) -> State {
    let half_property = half(l);
    let double_block_half = l.reg.get(state, half_property);
    let normal = direction.axis() != Axis::Y
        || (double_block_half == LOWER) != (direction == Dir::Up)
        || l.block(neighbor) == l.block(state) && l.reg.get(neighbor, half_property) != double_block_half;
    if normal {
        if double_block_half == LOWER && direction == Dir::Down && !crate::blocks::can_survive(l, state, pos) {
            l.known.air
        } else {
            vegetation::update_shape(l, state, pos, direction, neighbor_pos, neighbor)
        }
    } else {
        l.known.air
    }
}

// DoublePlantBlock.canSurvive: the upper half needs the lower below it; the
// lower half is VegetationBlock's.
pub fn can_survive(l: &mut Level, state: State, pos: Pos) -> bool {
    if l.reg.get(state, half(l)) != UPPER {
        vegetation::can_survive(l, pos, |l, state, _| vegetation::may_place_on(l, state))
    } else {
        let below = l.get(pos.below());
        l.block(below) == l.block(state) && l.reg.get(below, half(l)) == LOWER
    }
}
