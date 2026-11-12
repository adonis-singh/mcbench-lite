// LightningRodBlock (and WeatheringLightningRodBlock, whose additions are
// random ticks). A lightning rod is powered by a lightning strike, and none
// strikes here; a state placed powered turns itself off after 8 ticks.

use crate::blocks::{Behaviour, NONE};
use crate::dir::{Dir, Pos};
use crate::level::{Level, PRIORITY_NORMAL, UPDATE_ALL};
use crate::property;
use crate::registry::State;

pub const BEHAVIOUR: Behaviour = Behaviour {
    update_shape: Some(super::waterlogged::update_shape),
    affect_neighbors_after_removal: Some(affect_neighbors_after_removal),
    tick: Some(tick),
    get_signal: Some(get_signal),
    get_direct_signal: Some(get_direct_signal),
    on_place: Some(on_place),
    ..NONE
};

fn facing(l: &Level) -> u16 {
    property!(l, "facing", ["north", "east", "south", "west", "up", "down"])
}

fn powered(l: &Level) -> u16 {
    property!(l, "powered", ["true", "false"])
}

fn get_signal(l: &mut Level, state: State, _pos: Pos, _dir: Dir) -> i32 {
    if l.reg.get_bool(state, powered(l)) {
        15
    } else {
        0
    }
}

fn get_direct_signal(l: &mut Level, state: State, _pos: Pos, dir: Dir) -> i32 {
    if l.reg.get_bool(state, powered(l)) && l.reg.get_dir(state, facing(l)) == dir {
        15
    } else {
        0
    }
}

// LightningRodBlock.updateNeighbours (the orientation is null).
fn update_neighbours(l: &mut Level, state: State, pos: Pos) {
    let opposite = l.reg.get_dir(state, facing(l)).opposite();
    let block = l.block(state);
    l.update_neighbors_at(pos.relative(opposite), block);
}

// LightningRodBlock.tick.
fn tick(l: &mut Level, state: State, pos: Pos) {
    l.set(pos, l.reg.with_bool(state, powered(l), false), UPDATE_ALL);
    update_neighbours(l, state, pos);
}

// LightningRodBlock.affectNeighborsAfterRemoval.
fn affect_neighbors_after_removal(l: &mut Level, state: State, pos: Pos, _moved: bool) {
    if l.reg.get_bool(state, powered(l)) {
        update_neighbours(l, state, pos);
    }
}

// LightningRodBlock.onPlace.
fn on_place(l: &mut Level, state: State, pos: Pos, old: State, _moved: bool) {
    let block = l.block(state);
    if block != l.block(old) && l.reg.get_bool(state, powered(l)) && !l.has_scheduled_tick(pos, block) {
        l.schedule_tick(pos, block, 8, PRIORITY_NORMAL);
    }
}
