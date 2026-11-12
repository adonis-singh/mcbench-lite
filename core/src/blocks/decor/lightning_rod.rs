// LightningRodBlock (and WeatheringLightningRodBlock): a rod struck by
// lightning (no lightning here) powers its neighbours for 8 ticks; a powered
// rod placed schedules its tick, and one removed updates its neighbours.

use crate::blocks::shapes::common::schedule_water_tick;
use crate::blocks::{Behaviour, NONE};
use crate::dir::{Dir, Pos};
use crate::level::{Level, PRIORITY_NORMAL, UPDATE_ALL};
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

fn powered(l: &Level) -> u16 {
    property!(l, "powered", ["true", "false"])
}

fn facing(l: &Level, state: State) -> Dir {
    l.reg.get_dir(state, l.reg.property_of(state, "facing"))
}

// LightningRodBlock.updateShape.
fn update_shape(l: &mut Level, state: State, pos: Pos, _direction: Dir, _neighbor_pos: Pos, _neighbor: State) -> State {
    schedule_water_tick(l, state, pos);
    state
}

// LightningRodBlock.getSignal.
fn get_signal(l: &mut Level, state: State, _pos: Pos, _direction: Dir) -> i32 {
    if l.reg.get_bool(state, powered(l)) {
        15
    } else {
        0
    }
}

// LightningRodBlock.getDirectSignal.
fn get_direct_signal(l: &mut Level, state: State, _pos: Pos, direction: Dir) -> i32 {
    if l.reg.get_bool(state, powered(l)) && facing(l, state) == direction {
        15
    } else {
        0
    }
}

// LightningRodBlock.updateNeighbours: without the redstone experiments the
// orientation is null and draws nothing.
fn update_neighbours(l: &mut Level, state: State, pos: Pos) {
    let opposite = facing(l, state).opposite();
    l.update_neighbors_at(pos.relative(opposite), l.block(state));
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
    if l.block(state) != l.block(old) && l.reg.get_bool(state, powered(l)) && !l.has_scheduled_tick(pos, l.block(state)) {
        l.schedule_tick(pos, l.block(state), 8, PRIORITY_NORMAL);
    }
}
