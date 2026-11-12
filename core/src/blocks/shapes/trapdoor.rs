// TrapDoorBlock (and WeatheringCopperTrapDoorBlock, whose additions are
// random ticks). Events (BlockRedstoneEvent) have no listener: the new
// current stands.

use super::common::{bukkit_block_power, can_open_by_hand, open, play_sound_with_random_pitch, powered, schedule_water_tick};
use crate::blocks::{Behaviour, InputPlayer, NONE};
use crate::dir::{Dir, Pos};
use crate::level::{refuse, Level, UPDATE_CLIENTS};
use crate::registry::{BlockId, State, SIGNAL_SOURCE};

pub const BEHAVIOUR: Behaviour = Behaviour { update_shape: Some(update_shape), neighbor_changed: Some(neighbor_changed), use_without_item: Some(use_without_item), ..NONE };

// TrapDoorBlock.updateShape.
fn update_shape(l: &mut Level, state: State, pos: Pos, _direction: Dir, _neighbor_pos: Pos, _neighbor: State) -> State {
    schedule_water_tick(l, state, pos);
    state
}

// TrapDoorBlock.neighborChanged. A redstone wire above that the opening
// trapdoor breaks drops its item (an entity this port does not model).
fn neighbor_changed(l: &mut Level, state: State, pos: Pos, neighbor_block: BlockId, _moved: bool) {
    let mut has_neighbor_signal = l.has_neighbor_signal(pos);
    if has_neighbor_signal != l.reg.get_bool(state, powered(l)) {
        let power = bukkit_block_power(l, pos);
        let old_power = if l.reg.get_bool(state, open(l)) { 15 } else { 0 };
        if (old_power == 0) ^ (power == 0) || l.reg.flags(l.reg.default_state(neighbor_block)) & SIGNAL_SOURCE != 0 {
            has_neighbor_signal = power > 0;
        }
        let opens = l.reg.get_bool(state, open(l)) != has_neighbor_signal;
        if has_neighbor_signal && opens {
            let above = l.get(pos.above());
            if l.block(above) == l.known.redstone_wire {
                refuse(format!("drops of {} at {}", l.reg.describe(above), pos.above()));
            }
        }
        let mut state = state;
        if opens {
            state = l.reg.with_bool(state, open(l), has_neighbor_signal);
            play_sound_with_random_pitch(l);
            let event = if has_neighbor_signal { crate::game_event!(l, "block_open") } else { crate::game_event!(l, "block_close") };
            l.game_event_at(None, event, pos);
        }
        l.set(pos, l.reg.with_bool(state, powered(l), has_neighbor_signal), UPDATE_CLIENTS);
        schedule_water_tick(l, state, pos);
    }
}

// TrapDoorBlock.useWithoutItem: a type that cannot be opened by hand (iron)
// passes; else toggle: the open state flips (flags 2), a waterlogged
// trapdoor schedules its water, and playSound draws the pitch and sends the
// game event with the player as its source.
fn use_without_item(l: &mut Level, state: State, pos: Pos, player: Option<InputPlayer>) -> bool {
    if !can_open_by_hand(l, state) {
        return false;
    }
    let state = l.reg.cycle(state, open(l));
    l.set(pos, state, UPDATE_CLIENTS);
    schedule_water_tick(l, state, pos);
    play_sound_with_random_pitch(l);
    let event = if l.reg.get_bool(state, open(l)) { crate::game_event!(l, "block_open") } else { crate::game_event!(l, "block_close") };
    l.game_event_at(player.map(|p| p.source), event, pos);
    true
}
