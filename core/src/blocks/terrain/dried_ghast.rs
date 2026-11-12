// DriedGhastBlock: a waterlogged one gains a hydration level when its
// scheduled tick runs (a ready one spawns a ghastling: refused); a dry one
// loses one, and sculk sensors hear the change. Its tick is only scheduled by randomTick (never runs) and by
// placeLiquid.

use crate::blocks::shapes::common::{schedule_water_tick, waterlogged};
use crate::blocks::{Behaviour, NONE};
use crate::dir::{Dir, Pos};
use crate::game_event::Context;
use crate::level::{refuse, Level, UPDATE_CLIENTS};
use crate::property;
use crate::registry::State;

pub const BEHAVIOUR: Behaviour = Behaviour { update_shape: Some(update_shape), tick: Some(tick), ..NONE };

// DriedGhastBlock.DRIED_GHAST_HYDRATION_LEVELS.
fn hydration(l: &Level) -> u16 {
    property!(l, "hydration", ["0", "1", "2", "3"])
}

// DriedGhastBlock.updateShape: a waterlogged ghast schedules a water tick.
fn update_shape(l: &mut Level, state: State, pos: Pos, _direction: Dir, _neighbor_pos: Pos, _neighbor: State) -> State {
    schedule_water_tick(l, state, pos);
    state
}

// DriedGhastBlock.tick.
fn tick(l: &mut Level, state: State, pos: Pos) {
    let level = l.reg.get_int(state, hydration(l));
    if l.reg.get_bool(state, waterlogged(l)) {
        // tickWaterlogged: not ready to spawn (hydration 3) grows; else the ghastling.
        if level != 3 {
            l.set(pos, l.reg.with_int(state, hydration(l), level + 1), UPDATE_CLIENTS);
            l.game_event_in(crate::game_event!(l, "block_change"), pos, Context::of_state(state));
        } else {
            refuse(format!("a ghastling spawned at {pos}"));
        }
    } else if level > 0 {
        l.set(pos, l.reg.with_int(state, hydration(l), level - 1), UPDATE_CLIENTS);
        l.game_event_in(crate::game_event!(l, "block_change"), pos, Context::of_state(state));
    }
}
