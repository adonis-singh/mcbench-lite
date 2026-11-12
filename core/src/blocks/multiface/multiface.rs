// MultifaceBlock (the parent of MultifaceSpreadeableBlock, GlowLichenBlock
// and SculkVeinBlock): a block with a face boolean per side, each held by the
// full face of the block it touches.

use crate::blocks::{Behaviour, NONE};
use crate::dir::{Dir, Pos, ALL};
use crate::level::Level;
use crate::property;
use crate::registry::{State, Support};
use crate::shape_ops;

pub const BEHAVIOUR: Behaviour = Behaviour { update_shape: Some(update_shape), can_survive: Some(can_survive), ..NONE };

// PipeBlock.PROPERTY_BY_DIRECTION (MultifaceBlock.getFaceProperty).
pub fn face_property(l: &Level, dir: Dir) -> u16 {
    match dir {
        Dir::Down => property!(l, "down", ["true", "false"]),
        Dir::Up => property!(l, "up", ["true", "false"]),
        Dir::North => property!(l, "north", ["true", "false"]),
        Dir::South => property!(l, "south", ["true", "false"]),
        Dir::West => property!(l, "west", ["true", "false"]),
        Dir::East => property!(l, "east", ["true", "false"]),
    }
}

// MultifaceBlock.hasFace (every face is supported by the two blocks that
// have this behaviour).
pub fn has_face(l: &Level, state: State, dir: Dir) -> bool {
    l.reg.get_bool(state, face_property(l, dir))
}

// MultifaceBlock.hasAnyFace.
pub fn has_any_face(l: &Level, state: State) -> bool {
    ALL.iter().any(|&dir| has_face(l, state, dir))
}

// MultifaceBlock.canAttachTo(level, direction, pos, state): the neighbour's
// support shape or its collision shape at its position is full on the face
// toward the block.
pub fn can_attach_to_state(l: &Level, direction: Dir, pos: Pos, state: State) -> bool {
    let face = direction.opposite();
    shape_ops::is_face_sturdy(l, state, pos, face, Support::Full) || shape_ops::collision_shape_at(l, state, pos).face_shape(face).is_full_block()
}

// MultifaceBlock.canAttachTo(level, pos, direction).
pub fn can_attach_to(l: &mut Level, pos: Pos, direction: Dir) -> bool {
    let at_pos = pos.relative(direction);
    let at = l.get(at_pos);
    can_attach_to_state(l, direction, at_pos, at)
}

// MultifaceBlock.updateShape: a waterlogged block schedules a water tick;
// no face is air; a face whose neighbour cannot
// hold it goes.
fn update_shape(l: &mut Level, state: State, pos: Pos, direction: Dir, neighbor_pos: Pos, neighbor: State) -> State {
    crate::blocks::shapes::common::schedule_water_tick(l, state, pos);
    if !has_any_face(l, state) {
        l.known.air
    } else if has_face(l, state, direction) && !can_attach_to_state(l, direction, neighbor_pos, neighbor) {
        remove_face(l, state, direction)
    } else {
        state
    }
}

// MultifaceBlock.removeFace.
fn remove_face(l: &Level, state: State, dir: Dir) -> State {
    let state = l.reg.with_bool(state, face_property(l, dir), false);
    if has_any_face(l, state) {
        state
    } else {
        l.known.air
    }
}

// MultifaceBlock.canSurvive: some face, and every face held.
fn can_survive(l: &mut Level, state: State, pos: Pos) -> bool {
    let mut any = false;
    for dir in ALL {
        if has_face(l, state, dir) {
            if !can_attach_to(l, pos, dir) {
                return false;
            }
            any = true;
        }
    }
    any
}
