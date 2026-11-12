// MossyCarpetBlock (the pale moss carpet): a base on a block below, or no
// base on a carpet with a base, and a low or tall wall on each side held by
// the block it touches.

use super::multiface::can_attach_to;
use crate::blocks::{Behaviour, NONE};
use crate::dir::{Dir, Pos, HORIZONTAL};
use crate::is_block;
use crate::level::Level;
use crate::property;
use crate::registry::State;

pub const BEHAVIOUR: Behaviour = Behaviour { update_shape: Some(update_shape), can_survive: Some(can_survive), ..NONE };

// WallSide: values none, low, tall.
const WALL_NONE: u16 = 0;
const WALL_LOW: u16 = 1;
const WALL_TALL: u16 = 2;

fn base(l: &Level) -> u16 {
    property!(l, "bottom", ["true", "false"])
}

// MossyCarpetBlock.getPropertyForFace (horizontal directions).
fn side(l: &Level, dir: Dir) -> u16 {
    match dir {
        Dir::North => property!(l, "north", ["none", "low", "tall"]),
        Dir::East => property!(l, "east", ["none", "low", "tall"]),
        Dir::South => property!(l, "south", ["none", "low", "tall"]),
        Dir::West => property!(l, "west", ["none", "low", "tall"]),
        _ => panic!("{dir:?} has no wall side"),
    }
}

// MossyCarpetBlock.canSurvive.
fn can_survive(l: &mut Level, state: State, pos: Pos) -> bool {
    let below = l.get(pos.below());
    if l.reg.get_bool(state, base(l)) {
        !l.is_air(below)
    } else {
        l.block(below) == l.block(state) && l.reg.get_bool(below, base(l))
    }
}

// MossyCarpetBlock.hasFaces.
fn has_faces(l: &Level, state: State) -> bool {
    l.reg.get_bool(state, base(l)) || HORIZONTAL.into_iter().any(|dir| l.reg.get(state, side(l, dir)) != WALL_NONE)
}

// MossyCarpetBlock.canSupportAtFace.
fn can_support_at_face(l: &mut Level, pos: Pos, direction: Dir) -> bool {
    direction != Dir::Up && can_attach_to(l, pos, direction)
}

// MossyCarpetBlock.getUpdatedState (the blocks above and below are read the
// first time a low wall asks for them).
fn get_updated_state(l: &mut Level, mut state: State, pos: Pos, tip: bool) -> State {
    let mut above: Option<State> = None;
    let mut below: Option<State> = None;
    let tip = tip || l.reg.get_bool(state, base(l));
    for dir in HORIZONTAL {
        let property = side(l, dir);
        let mut wall = if can_support_at_face(l, pos, dir) {
            if tip {
                WALL_LOW
            } else {
                l.reg.get(state, property)
            }
        } else {
            WALL_NONE
        };
        if wall == WALL_LOW {
            let above_state = match above {
                Some(above) => above,
                None => *above.insert(l.get(pos.above())),
            };
            if is_block!(l, above_state, "minecraft:pale_moss_carpet") && l.reg.get(above_state, property) != WALL_NONE && !l.reg.get_bool(above_state, base(l)) {
                wall = WALL_TALL;
            }
            if !l.reg.get_bool(state, base(l)) {
                let below_state = match below {
                    Some(below) => below,
                    None => *below.insert(l.get(pos.below())),
                };
                if is_block!(l, below_state, "minecraft:pale_moss_carpet") && l.reg.get(below_state, property) == WALL_NONE {
                    wall = WALL_NONE;
                }
            }
        }
        state = l.reg.with(state, property, wall);
    }
    state
}

// MossyCarpetBlock.updateShape.
fn update_shape(l: &mut Level, state: State, pos: Pos, _direction: Dir, _neighbor_pos: Pos, _neighbor: State) -> State {
    if !can_survive(l, state, pos) {
        l.known.air
    } else {
        let updated = get_updated_state(l, state, pos, false);
        if has_faces(l, updated) {
            updated
        } else {
            l.known.air
        }
    }
}
