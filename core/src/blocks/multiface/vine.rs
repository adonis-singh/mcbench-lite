// VineBlock: up and the four sides, a side held by the block it touches or
// by a vine above with the same side.

use super::multiface::can_attach_to_state;
use crate::blocks::{Behaviour, NONE};
use crate::dir::{Dir, Pos, HORIZONTAL};
use crate::level::Level;
use crate::property;
use crate::registry::State;

pub const BEHAVIOUR: Behaviour = Behaviour { update_shape: Some(update_shape), can_survive: Some(can_survive), ..NONE };

// VineBlock.getPropertyForFace (PROPERTY_BY_DIRECTION: no down).
fn property_for_face(l: &Level, dir: Dir) -> u16 {
    match dir {
        Dir::Up => property!(l, "up", ["true", "false"]),
        Dir::North => property!(l, "north", ["true", "false"]),
        Dir::South => property!(l, "south", ["true", "false"]),
        Dir::West => property!(l, "west", ["true", "false"]),
        Dir::East => property!(l, "east", ["true", "false"]),
        Dir::Down => panic!("a vine has no down face"),
    }
}

fn get(l: &Level, state: State, dir: Dir) -> bool {
    l.reg.get_bool(state, property_for_face(l, dir))
}

fn set(l: &Level, state: State, dir: Dir, value: bool) -> State {
    l.reg.with_bool(state, property_for_face(l, dir), value)
}

// VineBlock.hasFaces (countFaces() > 0).
fn has_faces(l: &Level, state: State) -> bool {
    [Dir::Up, Dir::North, Dir::East, Dir::South, Dir::West].into_iter().any(|dir| get(l, state, dir))
}

// VineBlock.isAcceptableNeighbour.
fn is_acceptable_neighbour(l: &mut Level, neighbor_pos: Pos, attached_face: Dir) -> bool {
    let neighbor = l.get(neighbor_pos);
    can_attach_to_state(l, attached_face, neighbor_pos, neighbor)
}

// VineBlock.canSupportAtFace.
fn can_support_at_face(l: &mut Level, state: State, pos: Pos, direction: Dir) -> bool {
    if direction == Dir::Down {
        return false;
    }
    if is_acceptable_neighbour(l, pos.relative(direction), direction) {
        true
    } else if direction == Dir::Up {
        false
    } else {
        let above = l.get(pos.above());
        l.block(above) == l.block(state) && get(l, above, direction)
    }
}

// VineBlock.getUpdatedState. The up face is asked of the block above with
// Direction.DOWN, as Minecraft writes it (so the face of that block checked
// is its up face).
fn get_updated_state(l: &mut Level, mut state: State, pos: Pos) -> State {
    let above_pos = pos.above();
    if get(l, state, Dir::Up) {
        let acceptable = is_acceptable_neighbour(l, above_pos, Dir::Down);
        state = set(l, state, Dir::Up, acceptable);
    }
    let mut above: Option<State> = None;
    for dir in HORIZONTAL {
        if get(l, state, dir) {
            let mut can_support = can_support_at_face(l, state, pos, dir);
            if !can_support {
                let above = match above {
                    Some(above) => above,
                    None => *above.insert(l.get(above_pos)),
                };
                can_support = l.block(above) == l.block(state) && get(l, above, dir);
            }
            state = set(l, state, dir, can_support);
        }
    }
    state
}

// VineBlock.canSurvive.
fn can_survive(l: &mut Level, state: State, pos: Pos) -> bool {
    let updated = get_updated_state(l, state, pos);
    has_faces(l, updated)
}

// VineBlock.updateShape (BlockBehaviour's for the neighbour below).
fn update_shape(l: &mut Level, state: State, pos: Pos, direction: Dir, _neighbor_pos: Pos, _neighbor: State) -> State {
    if direction == Dir::Down {
        state
    } else {
        let updated = get_updated_state(l, state, pos);
        if has_faces(l, updated) {
            updated
        } else {
            l.known.air
        }
    }
}
