// ObserverBlock.

use super::{Behaviour, NONE};
use crate::dir::{Dir, Pos};
use crate::level::{Level, PRIORITY_NORMAL, UPDATE_CLIENTS, UPDATE_KNOWN_SHAPE};
use crate::property;
use crate::registry::State;

pub const BEHAVIOUR: Behaviour = Behaviour {
    update_shape: Some(update_shape),
    on_place: Some(on_place),
    affect_neighbors_after_removal: Some(affect_neighbors_after_removal),
    tick: Some(tick),
    get_signal: Some(get_signal),
    get_direct_signal: Some(get_direct_signal),
    ..NONE
};

fn facing(l: &Level) -> u16 {
    property!(l, "facing", ["north", "east", "south", "west", "up", "down"])
}

fn powered(l: &Level) -> u16 {
    property!(l, "powered", ["true", "false"])
}

// The redstone change events (CraftEventFactory.callRedstoneChange) have no
// listener: the new current stands.
fn tick(l: &mut Level, state: State, pos: Pos) {
    if l.reg.get_bool(state, powered(l)) {
        l.set(pos, l.reg.with_bool(state, powered(l), false), UPDATE_CLIENTS);
    } else {
        l.set(pos, l.reg.with_bool(state, powered(l), true), UPDATE_CLIENTS);
        l.schedule_tick(pos, l.block(state), 2, PRIORITY_NORMAL);
    }
    update_neighbors_in_front(l, pos, state);
}

// ObserverBlock.updateShape and startSignal.
fn update_shape(l: &mut Level, state: State, pos: Pos, dir: Dir, _neighbor_pos: Pos, _neighbor: State) -> State {
    if l.reg.get_dir(state, facing(l)) == dir && !l.reg.get_bool(state, powered(l)) {
        let block = l.block(state);
        if !l.has_scheduled_tick(pos, block) {
            l.schedule_tick(pos, block, 2, PRIORITY_NORMAL);
        }
    }
    state
}

// ObserverBlock.updateNeighborsInFront. The orientation is null.
fn update_neighbors_in_front(l: &mut Level, pos: Pos, state: State) {
    let direction = l.reg.get_dir(state, facing(l));
    let block_pos = pos.relative(direction.opposite());
    let block = l.block(state);
    l.neighbor_changed(block_pos, block);
    l.update_neighbors_at_except_from_facing(block_pos, block, direction);
}

fn get_direct_signal(l: &mut Level, state: State, pos: Pos, side: Dir) -> i32 {
    super::get_signal(l, state, pos, side)
}

fn get_signal(l: &mut Level, state: State, _pos: Pos, side: Dir) -> i32 {
    if l.reg.get_bool(state, powered(l)) && l.reg.get_dir(state, facing(l)) == side {
        15
    } else {
        0
    }
}

fn on_place(l: &mut Level, state: State, pos: Pos, old: State, _moved: bool) {
    let block = l.block(state);
    if block != l.block(old) && l.reg.get_bool(state, powered(l)) && !l.has_scheduled_tick(pos, block) {
        let unpowered = l.reg.with_bool(state, powered(l), false);
        l.set(pos, unpowered, UPDATE_KNOWN_SHAPE | UPDATE_CLIENTS);
        update_neighbors_in_front(l, pos, unpowered);
    }
}

fn affect_neighbors_after_removal(l: &mut Level, state: State, pos: Pos, _moved: bool) {
    if l.reg.get_bool(state, powered(l)) && l.has_scheduled_tick(pos, l.block(state)) {
        update_neighbors_in_front(l, pos, l.reg.with_bool(state, powered(l), false));
    }
}
