// RedstoneLampBlock.

use super::{Behaviour, NONE};
use crate::dir::Pos;
use crate::level::{Level, PRIORITY_NORMAL, UPDATE_CLIENTS};
use crate::property;
use crate::registry::{BlockId, State};

pub const BEHAVIOUR: Behaviour = Behaviour { neighbor_changed: Some(neighbor_changed), tick: Some(tick), ..NONE };

fn lit(l: &Level) -> u16 {
    property!(l, "lit", ["true", "false"])
}

// The redstone change event (CraftEventFactory.callRedstoneChange) has no
// listener: the new current stands.
fn neighbor_changed(l: &mut Level, state: State, pos: Pos, _neighbor: BlockId, _moved: bool) {
    let lit_value = l.reg.get_bool(state, lit(l));
    if lit_value != l.has_neighbor_signal(pos) {
        if lit_value {
            l.schedule_tick(pos, l.block(state), 4, PRIORITY_NORMAL);
        } else {
            l.set(pos, l.reg.with_bool(state, lit(l), !lit_value), UPDATE_CLIENTS);
        }
    }
}

fn tick(l: &mut Level, state: State, pos: Pos) {
    if l.reg.get_bool(state, lit(l)) && !l.has_neighbor_signal(pos) {
        l.set(pos, l.reg.with_bool(state, lit(l), false), UPDATE_CLIENTS);
    }
}
