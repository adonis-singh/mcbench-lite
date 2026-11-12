// TargetBlock. onProjectileHit needs a projectile, which this port never
// has; use and entity methods are not engine-relevant.

use super::{Behaviour, NONE};
use crate::dir::{Dir, Pos};
use crate::level::{Level, UPDATE_ALL, UPDATE_CLIENTS, UPDATE_KNOWN_SHAPE};
use crate::property;
use crate::registry::State;

pub const BEHAVIOUR: Behaviour = Behaviour { on_place: Some(on_place), tick: Some(tick), get_signal: Some(get_signal), ..NONE };

fn power(l: &Level) -> u16 {
    property!(l, "power", ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12", "13", "14", "15"])
}

fn tick(l: &mut Level, state: State, pos: Pos) {
    if l.reg.get_int(state, power(l)) != 0 {
        l.set(pos, l.reg.with_int(state, power(l), 0), UPDATE_ALL);
    }
}

fn get_signal(l: &mut Level, state: State, _pos: Pos, _side: Dir) -> i32 {
    l.reg.get_int(state, power(l))
}

fn on_place(l: &mut Level, state: State, pos: Pos, old: State, _moved: bool) {
    let block = l.block(state);
    if block != l.block(old) && l.reg.get_int(state, power(l)) > 0 && !l.has_scheduled_tick(pos, block) {
        l.set(pos, l.reg.with_int(state, power(l), 0), UPDATE_CLIENTS | UPDATE_KNOWN_SHAPE);
    }
}
