// RepeaterBlock, on DiodeBlock's code (diode.rs).

use super::{diode, Behaviour, InputPlayer, NONE};
use crate::dir::{Dir, Pos};
use crate::level::{Level, UPDATE_ALL};
use crate::property;
use crate::registry::State;

pub const BEHAVIOUR: Behaviour = Behaviour {
    update_shape: Some(update_shape),
    neighbor_changed: Some(diode::neighbor_changed),
    on_place: Some(diode::on_place),
    affect_neighbors_after_removal: Some(diode::affect_neighbors_after_removal),
    tick: Some(diode::tick),
    get_signal: Some(diode::get_signal),
    get_direct_signal: Some(diode::get_direct_signal),
    can_survive: Some(diode::can_survive),
    use_without_item: Some(use_without_item),
    ..NONE
};

fn locked(l: &Level) -> u16 {
    property!(l, "locked", ["true", "false"])
}

fn delay(l: &Level) -> u16 {
    property!(l, "delay", ["1", "2", "3", "4"])
}

// RepeaterBlock.getDelay.
pub fn get_delay(l: &Level, state: State) -> i32 {
    l.reg.get_int(state, delay(l)) * 2
}

// RepeaterBlock.updateShape (the level is a server's, so not isClientSide).
// DiodeBlock and the blocks above it do not override updateShape: the state
// stays.
fn update_shape(l: &mut Level, state: State, pos: Pos, direction: Dir, neighbor_pos: Pos, neighbor: State) -> State {
    if direction == Dir::Down && !diode::can_survive_on(l, neighbor, neighbor_pos) {
        l.known.air
    } else if direction.axis() != l.reg.get_dir(state, diode::facing(l)).axis() {
        let is_locked = is_locked(l, pos, state);
        l.reg.with_bool(state, locked(l), is_locked)
    } else {
        state
    }
}

// RepeaterBlock.isLocked.
pub fn is_locked(l: &mut Level, pos: Pos, state: State) -> bool {
    diode::get_alternate_signal(l, pos, state) > 0
}

// RepeaterBlock.useWithoutItem: for a player who may build (the input player
// is a survival player), the delay goes to the next (4 to 1).
fn use_without_item(l: &mut Level, state: State, pos: Pos, _player: Option<InputPlayer>) -> bool {
    let state = l.reg.cycle(state, delay(l));
    l.set(pos, state, UPDATE_ALL);
    true
}
