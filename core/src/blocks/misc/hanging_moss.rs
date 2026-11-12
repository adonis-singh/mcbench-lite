// HangingMossBlock (pale hanging moss): hangs from a block with a full
// bottom face or from more of itself; a block that loses its hold breaks on
// a scheduled tick.

use crate::blocks::{Behaviour, NONE};
use crate::dir::{Dir, Pos};
use crate::is_block;
use crate::level::{Level, PRIORITY_NORMAL};
use crate::property;
use crate::registry::{State, Support};
use crate::shape_ops;

pub const BEHAVIOUR: Behaviour = Behaviour { update_shape: Some(update_shape), tick: Some(tick), can_survive: Some(can_survive), ..NONE };

// MultifaceBlock.canAttachTo(level, direction, pos, state): the face of the
// state toward the block is full in its support shape (the sturdy face) or in
// its collision shape. The collision shape is read only if the first fails.
pub fn can_attach_to(l: &Level, direction: Dir, state: State, pos: Pos) -> bool {
    let face = direction.opposite();
    shape_ops::is_face_sturdy(l, state, pos, face, Support::Full) || shape_ops::collision_shape_at(l, state, pos).face_shape(face).is_full_block()
}

// HangingMossBlock.canStayAtPosition.
fn can_stay_at_position(l: &mut Level, pos: Pos) -> bool {
    let above = l.get(pos.above());
    can_attach_to(l, Dir::Up, above, pos.above()) || is_block!(l, above, "minecraft:pale_hanging_moss")
}

fn can_survive(l: &mut Level, _state: State, pos: Pos) -> bool {
    can_stay_at_position(l, pos)
}

// HangingMossBlock.updateShape: the tip is the block with no moss below it.
fn update_shape(l: &mut Level, state: State, pos: Pos, _direction: Dir, _neighbor_pos: Pos, _neighbor: State) -> State {
    if !can_stay_at_position(l, pos) {
        l.schedule_tick(pos, l.block(state), 1, PRIORITY_NORMAL);
    }
    let below = l.get(pos.below());
    let tip = l.block(below) != l.block(state);
    l.reg.with_bool(state, property!(l, "tip", ["true", "false"]), tip)
}

// HangingMossBlock.tick.
fn tick(l: &mut Level, _state: State, pos: Pos) {
    if !can_stay_at_position(l, pos) {
        l.destroy_block(pos, true, 512);
    }
}
