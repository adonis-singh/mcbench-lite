// SugarCaneBlock: stands on cane, or on dirt or sand beside water or frosted
// ice. A cane that cannot survive schedules a tick (updateShape) and breaks
// in it.

use crate::blocks::{Behaviour, NONE};
use crate::dir::{Dir, Pos, HORIZONTAL};
use crate::in_tag;
use crate::is_block;
use crate::level::{Level, PRIORITY_NORMAL};
use crate::registry::State;

pub const BEHAVIOUR: Behaviour = Behaviour { update_shape: Some(update_shape), can_survive: Some(can_survive), tick: Some(tick), ..NONE };

// SugarCaneBlock.tick.
fn tick(l: &mut Level, state: State, pos: Pos) {
    if !can_survive(l, state, pos) {
        l.destroy_block(pos, true, 512);
    }
}

// SugarCaneBlock.updateShape.
fn update_shape(l: &mut Level, state: State, pos: Pos, _direction: Dir, _neighbor_pos: Pos, _neighbor: State) -> State {
    if !can_survive(l, state, pos) {
        l.schedule_tick(pos, l.block(state), 1, PRIORITY_NORMAL);
    }
    state
}

// SugarCaneBlock.canSurvive: next to water or frosted ice.
fn can_survive(l: &mut Level, state: State, pos: Pos) -> bool {
    let below = pos.below();
    let at = l.get(below);
    if l.block(at) == l.block(state) {
        return true;
    }
    if in_tag!(l, at, "minecraft:dirt") || in_tag!(l, at, "minecraft:sand") {
        for dir in HORIZONTAL {
            let side = l.get(below.relative(dir));
            if l.reg.fluids[l.fluid_of(side) as usize].fluid.is_water() || is_block!(l, side, "minecraft:frosted_ice") {
                return true;
            }
        }
    }
    false
}
