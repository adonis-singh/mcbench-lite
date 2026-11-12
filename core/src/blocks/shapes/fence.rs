// FenceBlock.

use super::common::{cross_property, gate_connects_to_direction, is_exception_for_connection, schedule_water_tick};
use crate::blocks::{Behaviour, NONE};
use crate::dir::{Dir, Pos};
use crate::level::Level;
use crate::registry::{State, Support};
use crate::{shape_classes, shape_tag};

pub const BEHAVIOUR: Behaviour = Behaviour { update_shape: Some(update_shape), ..NONE };

// FenceBlock.connectsTo, for the fence `fence` (its block is `this`).
fn connects_to(l: &Level, fence: State, state: State, is_side_solid: bool, direction: Dir) -> bool {
    let block = l.block(state) as usize;
    let is_same_fence = shape_tag!(l, "minecraft:fences")[block]
        && shape_tag!(l, "minecraft:wooden_fences")[block] == shape_tag!(l, "minecraft:wooden_fences")[l.block(fence) as usize];
    let gate = shape_classes!(l, ["FenceGateBlock"])[block] && gate_connects_to_direction(l, state, direction);
    !is_exception_for_connection(l, state) && is_side_solid || is_same_fence || gate
}

// FenceBlock.updateShape.
fn update_shape(l: &mut Level, state: State, pos: Pos, direction: Dir, neighbor_pos: Pos, neighbor: State) -> State {
    schedule_water_tick(l, state, pos);
    if direction.is_horizontal() {
        let opposite = direction.opposite();
        let connects = connects_to(l, state, neighbor, crate::shape_ops::is_face_sturdy(l, neighbor, neighbor_pos, opposite, Support::Full), opposite);
        l.reg.with_bool(state, cross_property(l, direction), connects)
    } else {
        state
    }
}
