// LeverBlock, and FaceAttachedHorizontalDirectionalBlock's methods (shared
// with ButtonBlock, the other subclass this port has).

use super::{Behaviour, InputPlayer, NONE};
use crate::dir::{Dir, Pos};
use crate::level::{Level, UPDATE_ALL};
use crate::property;
use crate::registry::{State, Support};

pub const BEHAVIOUR: Behaviour = Behaviour {
    update_shape: Some(update_shape),
    affect_neighbors_after_removal: Some(affect_neighbors_after_removal),
    get_signal: Some(get_signal),
    get_direct_signal: Some(get_direct_signal),
    can_survive: Some(can_survive),
    use_without_item: Some(pull),
    ..NONE
};

pub fn powered(l: &Level) -> u16 {
    property!(l, "powered", ["true", "false"])
}

fn face(l: &Level) -> u16 {
    property!(l, "face", ["floor", "wall", "ceiling"])
}

fn facing(l: &Level) -> u16 {
    property!(l, "facing", ["north", "south", "west", "east"])
}

// FaceAttachedHorizontalDirectionalBlock.getConnectedDirection.
pub fn connected_direction(l: &Level, state: State) -> Dir {
    match l.reg.value_name(state, face(l)) {
        "ceiling" => Dir::Down,
        "floor" => Dir::Up,
        _ => l.reg.get_dir(state, facing(l)),
    }
}

// FaceAttachedHorizontalDirectionalBlock.canSurvive, with canAttach.
pub fn can_survive(l: &mut Level, state: State, pos: Pos) -> bool {
    let direction = connected_direction(l, state).opposite();
    let attached = pos.relative(direction);
    let neighbor = l.get(attached);
    crate::shape_ops::is_face_sturdy(l, neighbor, attached, direction.opposite(), Support::Full)
}

// FaceAttachedHorizontalDirectionalBlock.updateShape; BlockBehaviour's
// returns the state.
pub fn update_shape(l: &mut Level, state: State, pos: Pos, dir: Dir, _neighbor_pos: Pos, _neighbor: State) -> State {
    if connected_direction(l, state).opposite() == dir && !can_survive(l, state, pos) {
        l.known.air
    } else {
        state
    }
}

// LeverBlock.updateNeighbours and ButtonBlock.updateNeighbours: the same
// code. Without the redstone experiments the orientation is null and draws
// nothing from the level's random.
pub fn update_neighbours(l: &mut Level, state: State, pos: Pos) {
    let opposite = connected_direction(l, state).opposite();
    let block = l.block(state);
    l.update_neighbors_at(pos, block);
    l.update_neighbors_at(pos.relative(opposite), block);
}

// LeverBlock.pull with no player: playSound is a no-op (it draws from the
// world's thread-safe random, not the level's); the game event is heard by
// sculk sensors.
fn pull(l: &mut Level, state: State, pos: Pos, _player: Option<InputPlayer>) -> bool {
    let state = l.reg.with_bool(state, powered(l), !l.reg.get_bool(state, powered(l)));
    l.set(pos, state, UPDATE_ALL);
    update_neighbours(l, state, pos);
    let event = if l.reg.get_bool(state, powered(l)) { crate::game_event!(l, "block_activate") } else { crate::game_event!(l, "block_deactivate") };
    l.game_event_at(None, event, pos);
    true
}

// LeverBlock.affectNeighborsAfterRemoval (and ButtonBlock's).
pub fn affect_neighbors_after_removal(l: &mut Level, state: State, pos: Pos, moved: bool) {
    if !moved && l.reg.get_bool(state, powered(l)) {
        update_neighbours(l, state, pos);
    }
}

// LeverBlock.getSignal (and ButtonBlock.getSignal).
pub fn get_signal(l: &mut Level, state: State, _pos: Pos, _side: Dir) -> i32 {
    if l.reg.get_bool(state, powered(l)) {
        15
    } else {
        0
    }
}

// LeverBlock.getDirectSignal (and ButtonBlock.getDirectSignal).
pub fn get_direct_signal(l: &mut Level, state: State, _pos: Pos, side: Dir) -> i32 {
    if l.reg.get_bool(state, powered(l)) && connected_direction(l, state) == side {
        15
    } else {
        0
    }
}
