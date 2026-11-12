// StairBlock (and WeatheringCopperStairBlock, whose additions are random
// ticks).

use super::common::{get_facing, schedule_water_tick, same_axis};
use crate::blocks::{Behaviour, NONE};
use crate::dir::{Dir, Pos};
use crate::level::Level;
use crate::property;
use crate::registry::State;
use crate::shape_classes;

pub const BEHAVIOUR: Behaviour = Behaviour { update_shape: Some(update_shape), ..NONE };

// StairsShape's indices in its property's values.
const STRAIGHT: u16 = 0;
const INNER_LEFT: u16 = 1;
const INNER_RIGHT: u16 = 2;
const OUTER_LEFT: u16 = 3;
const OUTER_RIGHT: u16 = 4;

fn shape(l: &Level) -> u16 {
    property!(l, "shape", ["straight", "inner_left", "inner_right", "outer_left", "outer_right"])
}

fn half(l: &Level) -> u16 {
    property!(l, "half", ["top", "bottom"])
}

// StairBlock.isStairs: instanceof StairBlock.
pub fn is_stairs(l: &Level, state: State) -> bool {
    shape_classes!(l, ["StairBlock", "WeatheringCopperStairBlock"])[l.block(state) as usize]
}

// StairBlock.updateShape.
fn update_shape(l: &mut Level, state: State, pos: Pos, direction: Dir, _neighbor_pos: Pos, _neighbor: State) -> State {
    schedule_water_tick(l, state, pos);
    if direction.is_horizontal() {
        let stairs_shape = get_stairs_shape(l, state, pos);
        l.reg.with(state, shape(l), stairs_shape)
    } else {
        state
    }
}

// StairBlock.getStairsShape.
fn get_stairs_shape(l: &mut Level, state: State, pos: Pos) -> u16 {
    let direction = get_facing(l, state);
    let ahead = l.get(pos.relative(direction));
    if is_stairs(l, ahead) && l.reg.get(state, half(l)) == l.reg.get(ahead, half(l)) {
        let ahead_facing = get_facing(l, ahead);
        if !same_axis(ahead_facing, get_facing(l, state)) && can_take_shape(l, state, pos, ahead_facing.opposite()) {
            if ahead_facing == direction.counter_clockwise() {
                return OUTER_LEFT;
            }
            return OUTER_RIGHT;
        }
    }
    let behind = l.get(pos.relative(direction.opposite()));
    if is_stairs(l, behind) && l.reg.get(state, half(l)) == l.reg.get(behind, half(l)) {
        let behind_facing = get_facing(l, behind);
        if !same_axis(behind_facing, get_facing(l, state)) && can_take_shape(l, state, pos, behind_facing) {
            if behind_facing == direction.counter_clockwise() {
                return INNER_LEFT;
            }
            return INNER_RIGHT;
        }
    }
    STRAIGHT
}

// StairBlock.canTakeShape.
fn can_take_shape(l: &mut Level, state: State, pos: Pos, face: Dir) -> bool {
    let other = l.get(pos.relative(face));
    !is_stairs(l, other) || get_facing(l, other) != get_facing(l, state) || l.reg.get(other, half(l)) != l.reg.get(state, half(l))
}
