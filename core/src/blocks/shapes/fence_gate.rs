// FenceGateBlock. Events (BlockRedstoneEvent) have no listener: the new
// current stands.

use super::common::{get_facing, open, play_sound_with_random_pitch, powered};
use crate::blocks::{Behaviour, InputPlayer, NONE};
use crate::dir::{Dir, Pos};
use crate::level::{Level, UPDATE_CLIENTS, UPDATE_IMMEDIATE};
use crate::registry::{BlockId, State};
use crate::shape_tag;

pub const BEHAVIOUR: Behaviour = Behaviour { update_shape: Some(update_shape), neighbor_changed: Some(neighbor_changed), use_without_item: Some(use_without_item), ..NONE };

// FenceGateBlock.isWall: state.is(BlockTags.WALLS).
fn is_wall(l: &Level, state: State) -> bool {
    shape_tag!(l, "minecraft:walls")[l.block(state) as usize]
}

// FenceGateBlock.updateShape.
fn update_shape(l: &mut Level, state: State, pos: Pos, direction: Dir, _neighbor_pos: Pos, neighbor: State) -> State {
    if get_facing(l, state).clockwise().axis() != direction.axis() {
        state
    } else {
        let in_wall = is_wall(l, neighbor) || {
            let behind = l.get(pos.relative(direction.opposite()));
            is_wall(l, behind)
        };
        l.reg.with_bool(state, in_wall_property(l), in_wall)
    }
}

fn in_wall_property(l: &Level) -> u16 {
    crate::property!(l, "in_wall", ["true", "false"])
}

// FenceGateBlock.neighborChanged.
fn neighbor_changed(l: &mut Level, state: State, pos: Pos, _neighbor: BlockId, _moved: bool) {
    let has_neighbor_signal = l.has_neighbor_signal(pos);
    // The redstone event keeps the new current, which is this signal.
    if l.reg.get_bool(state, powered(l)) != has_neighbor_signal {
        let new_state = l.reg.with_bool(l.reg.with_bool(state, powered(l), has_neighbor_signal), open(l), has_neighbor_signal);
        l.set(pos, new_state, UPDATE_CLIENTS);
        if l.reg.get_bool(state, open(l)) != has_neighbor_signal {
            play_sound_with_random_pitch(l);
            let event = if has_neighbor_signal { crate::game_event!(l, "block_open") } else { crate::game_event!(l, "block_close") };
            l.game_event_at(None, event, pos);
        }
    }
}

// FenceGateBlock.useWithoutItem: an open gate closes; a closed one opens,
// facing the way the player looks (Entity.getDirection) if it faced the
// player (the way it faces is the side it opens away from).
fn use_without_item(l: &mut Level, state: State, pos: Pos, player: Option<InputPlayer>) -> bool {
    let player = player.expect("a fence gate is used by a player");
    let state = if l.reg.get_bool(state, open(l)) {
        let state = l.reg.with_bool(state, open(l), false);
        l.set(pos, state, UPDATE_CLIENTS | UPDATE_IMMEDIATE);
        state
    } else {
        let direction = player.direction();
        let mut state = state;
        if get_facing(l, state) == direction.opposite() {
            state = l.reg.with_dir(state, super::common::facing(l), direction);
        }
        let state = l.reg.with_bool(state, open(l), true);
        l.set(pos, state, UPDATE_CLIENTS | UPDATE_IMMEDIATE);
        state
    };
    play_sound_with_random_pitch(l);
    let event = if l.reg.get_bool(state, open(l)) { crate::game_event!(l, "block_open") } else { crate::game_event!(l, "block_close") };
    l.game_event_at(Some(player.source), event, pos);
    true
}
