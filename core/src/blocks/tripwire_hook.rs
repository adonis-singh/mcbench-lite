// TripWireHookBlock, with Paper's world configuration at the defaults
// (block-updates.disable-tripwire-updates and
// unsupported-settings.skip-tripwire-hook-placement-validation off). The
// redstone change events have no listener: the new current stands.

use super::{Behaviour, NONE};
use crate::dir::{Dir, Pos};
use crate::level::{Level, PRIORITY_NORMAL, UPDATE_ALL};
use crate::property;
use crate::registry::{BlockId, State, Support};
use std::sync::OnceLock;

pub const BEHAVIOUR: Behaviour = Behaviour {
    update_shape: Some(update_shape),
    affect_neighbors_after_removal: Some(affect_neighbors_after_removal),
    tick: Some(tick),
    get_signal: Some(get_signal),
    get_direct_signal: Some(get_direct_signal),
    can_survive: Some(can_survive),
    ..NONE
};

// Blocks.TRIPWIRE_HOOK and Blocks.TRIPWIRE.
pub fn hook_block(l: &Level) -> BlockId {
    static ID: OnceLock<BlockId> = OnceLock::new();
    *ID.get_or_init(|| l.reg.block_key("minecraft:tripwire_hook"))
}

pub fn tripwire_block(l: &Level) -> BlockId {
    static ID: OnceLock<BlockId> = OnceLock::new();
    *ID.get_or_init(|| l.reg.block_key("minecraft:tripwire"))
}

// TripWireHookBlock.FACING, POWERED and ATTACHED.
pub fn facing_property(l: &Level) -> u16 {
    property!(l, "facing", ["north", "south", "west", "east"])
}

fn powered_property(l: &Level) -> u16 {
    property!(l, "powered", ["true", "false"])
}

fn attached_property(l: &Level) -> u16 {
    property!(l, "attached", ["true", "false"])
}

// TripWireBlock.DISARMED.
fn disarmed_property(l: &Level) -> u16 {
    property!(l, "disarmed", ["true", "false"])
}

// TripWireHookBlock.canSurvive: the block behind the hook is sturdy on the
// face the hook faces (as the code reads: isFaceSturdy(level, pos, direction)).
fn can_survive(l: &mut Level, state: State, pos: Pos) -> bool {
    let direction = l.reg.get_dir(state, facing_property(l));
    let behind = pos.relative(direction.opposite());
    let behind_state = l.get(behind);
    direction.is_horizontal() && crate::shape_ops::is_face_sturdy(l, behind_state, behind, direction, Support::Full)
}

// TripWireHookBlock.updateShape; BlockBehaviour's returns the state.
fn update_shape(l: &mut Level, state: State, pos: Pos, direction: Dir, _neighbor_pos: Pos, _neighbor: State) -> State {
    if direction.opposite() == l.reg.get_dir(state, facing_property(l)) && !can_survive(l, state, pos) {
        l.known.air
    } else {
        state
    }
}

// TripWireHookBlock.calculateState: finds the hook at the other end of the
// wire, and sets both hooks and the wire between them to the state the wire
// gives. `search_range` is the distance of the tripwire `state`, which
// replaces what the level holds there (a wire being placed or removed).
pub fn calculate_state(l: &mut Level, pos: Pos, hook_state: State, attaching: bool, should_notify_neighbours: bool, search_range: i32, state: Option<State>) {
    let direction = l.reg.get_dir(hook_state, facing_property(l));
    let was_attached = l.reg.get_bool(hook_state, attached_property(l));
    let was_powered = l.reg.get_bool(hook_state, powered_property(l));
    let block = l.block(hook_state);
    let mut attached = !attaching;
    let mut powered = false;
    let mut end = 0;
    let mut states: [Option<State>; 42] = [None; 42];
    for i1 in 1..42 {
        let block_pos = pos.relative_n(direction, i1);
        let mut block_state = l.get(block_pos);
        if l.block(block_state) == hook_block(l) {
            if l.reg.get_dir(block_state, facing_property(l)) == direction.opposite() {
                end = i1;
            }
            break;
        }
        if l.block(block_state) != tripwire_block(l) && i1 != search_range {
            states[i1 as usize] = None;
            attached = false;
        } else {
            if i1 == search_range {
                block_state = state.unwrap_or(block_state);
            }
            let armed = !l.reg.get_bool(block_state, disarmed_property(l));
            let powered_value = l.reg.get_bool(block_state, powered_property(l));
            powered |= armed && powered_value;
            states[i1 as usize] = Some(block_state);
            if i1 == search_range {
                l.schedule_tick(pos, block, 10, PRIORITY_NORMAL);
                attached &= armed;
            }
        }
    }
    attached &= end > 1;
    powered &= attached;
    let mut new_state = l.reg.default_state(block);
    new_state = l.reg.with_bool(new_state, attached_property(l), attached);
    new_state = l.reg.with_bool(new_state, powered_property(l), powered);
    if end > 0 {
        let end_pos = pos.relative_n(direction, end);
        let opposite = direction.opposite();
        l.set(end_pos, l.reg.with_dir(new_state, facing_property(l), opposite), UPDATE_ALL);
        notify_neighbors(l, block, end_pos, opposite);
        emit_state(l, end_pos, attached, powered, was_attached, was_powered);
    }
    emit_state(l, pos, attached, powered, was_attached, was_powered);
    if !attaching {
        let current = l.get(pos);
        if l.block(current) == hook_block(l) {
            l.set(pos, l.reg.with_dir(new_state, facing_property(l), direction), UPDATE_ALL);
        }
        if should_notify_neighbours {
            notify_neighbors(l, block, pos, direction);
        }
    }
    if was_attached != attached {
        for i2 in 1..end {
            let block_pos = pos.relative_n(direction, i2);
            if let Some(wire_state) = states[i2 as usize] {
                let current = l.get(block_pos);
                if l.block(current) == tripwire_block(l) || l.block(current) == hook_block(l) {
                    l.set(block_pos, l.reg.with_bool(wire_state, attached_property(l), attached), UPDATE_ALL);
                }
            }
        }
    }
}

// TripWireHookBlock.tick.
fn tick(l: &mut Level, state: State, pos: Pos) {
    calculate_state(l, pos, state, false, true, -1, None);
}

// TripWireHookBlock.emitState: the sounds reach nothing, but the detach
// sound's pitch draws from the level's random, and the game events are heard
// by sculk sensors.
fn emit_state(l: &mut Level, pos: Pos, attached: bool, powered: bool, was_attached: bool, was_powered: bool) {
    if powered && !was_powered {
        let event = crate::game_event!(l, "block_activate");
        l.game_event_at(None, event, pos);
    } else if !powered && was_powered {
        let event = crate::game_event!(l, "block_deactivate");
        l.game_event_at(None, event, pos);
    } else if attached && !was_attached {
        let event = crate::game_event!(l, "block_attach");
        l.game_event_at(None, event, pos);
    } else if !attached && was_attached {
        l.random.next_float();
        let event = crate::game_event!(l, "block_detach");
        l.game_event_at(None, event, pos);
    }
}

// TripWireHookBlock.notifyNeighbors: without the redstone experiments the
// orientation is null and draws nothing from the level's random.
fn notify_neighbors(l: &mut Level, block: BlockId, pos: Pos, direction: Dir) {
    let opposite = direction.opposite();
    l.update_neighbors_at(pos, block);
    l.update_neighbors_at(pos.relative(opposite), block);
}

// TripWireHookBlock.affectNeighborsAfterRemoval.
fn affect_neighbors_after_removal(l: &mut Level, state: State, pos: Pos, moved: bool) {
    if !moved {
        let attached = l.reg.get_bool(state, attached_property(l));
        let powered = l.reg.get_bool(state, powered_property(l));
        if attached || powered {
            calculate_state(l, pos, state, true, false, -1, None);
        }
        if powered {
            let block = l.block(state);
            let direction = l.reg.get_dir(state, facing_property(l));
            notify_neighbors(l, block, pos, direction);
        }
    }
}

// TripWireHookBlock.getSignal.
fn get_signal(l: &mut Level, state: State, _pos: Pos, _side: Dir) -> i32 {
    if l.reg.get_bool(state, powered_property(l)) {
        15
    } else {
        0
    }
}

// TripWireHookBlock.getDirectSignal.
fn get_direct_signal(l: &mut Level, state: State, _pos: Pos, side: Dir) -> i32 {
    if !l.reg.get_bool(state, powered_property(l)) {
        0
    } else if l.reg.get_dir(state, facing_property(l)) == side {
        15
    } else {
        0
    }
}
