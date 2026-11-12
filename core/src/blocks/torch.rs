// RedstoneTorchBlock, with BaseTorchBlock's updateShape and canSurvive.
// wall_torch.rs reuses these with its own hasNeighborSignal.

use super::{Behaviour, NONE};
use crate::dir::{Dir, Pos, ALL};
use crate::level::{Level, PRIORITY_NORMAL, UPDATE_ALL};
use crate::property;
use crate::registry::{BlockId, State, Support};

pub const BEHAVIOUR: Behaviour = Behaviour {
    update_shape: Some(update_shape),
    neighbor_changed: Some(neighbor_changed),
    on_place: Some(on_place),
    affect_neighbors_after_removal: Some(affect_neighbors_after_removal),
    tick: Some(tick),
    get_signal: Some(get_signal),
    get_direct_signal: Some(get_direct_signal),
    can_survive: Some(can_survive),
    ..NONE
};

pub fn lit(l: &Level) -> u16 {
    property!(l, "lit", ["true", "false"])
}

// RedstoneTorchBlock.RECENT_TOGGLE_TIMER, MAX_RECENT_TOGGLES, RESTART_DELAY
// and TOGGLE_DELAY.
const RECENT_TOGGLE_TIMER: i64 = 60;
const MAX_RECENT_TOGGLES: i32 = 8;
const RESTART_DELAY: i32 = 160;
const TOGGLE_DELAY: i32 = 2;

// BaseTorchBlock.updateShape.
pub fn update_shape(l: &mut Level, state: State, pos: Pos, dir: Dir, _neighbor_pos: Pos, _neighbor: State) -> State {
    if dir == Dir::Down && !can_survive(l, state, pos) {
        l.known.air
    } else {
        state
    }
}

// BaseTorchBlock.canSurvive: Block.canSupportCenter(level, below, UP). Its
// unstable-bottom-center tag only applies to DOWN.
pub fn can_survive(l: &mut Level, _state: State, pos: Pos) -> bool {
    let below = l.get(pos.below());
    crate::shape_ops::is_face_sturdy(l, below, pos.below(), Dir::Up, Support::Center)
}

// RedstoneTorchBlock.onPlace.
fn on_place(l: &mut Level, state: State, pos: Pos, _old: State, _moved: bool) {
    notify_neighbors(l, pos, state);
}

// RedstoneTorchBlock.notifyNeighbors. randomOrientation is null: the
// redstone experiments are not enabled, so nothing is drawn from the random.
fn notify_neighbors(l: &mut Level, pos: Pos, state: State) {
    let block = l.block(state);
    for dir in ALL {
        l.update_neighbors_at(pos.relative(dir), block);
    }
}

fn affect_neighbors_after_removal(l: &mut Level, state: State, pos: Pos, moved: bool) {
    if !moved {
        notify_neighbors(l, pos, state);
    }
}

fn get_signal(l: &mut Level, state: State, _pos: Pos, side: Dir) -> i32 {
    if l.reg.get_bool(state, lit(l)) && Dir::Up != side {
        15
    } else {
        0
    }
}

// RedstoneTorchBlock.hasNeighborSignal.
fn has_neighbor_signal(l: &mut Level, pos: Pos, _state: State) -> bool {
    l.has_signal(pos.below(), Dir::Down)
}

// RedstoneTorchBlock.getDirectSignal (also the wall torch's): state.getSignal
// is the block's own getSignal.
pub fn get_direct_signal(l: &mut Level, state: State, pos: Pos, side: Dir) -> i32 {
    if side == Dir::Down {
        super::get_signal(l, state, pos, side)
    } else {
        0
    }
}

fn tick(l: &mut Level, state: State, pos: Pos) {
    tick_with(l, state, pos, has_neighbor_signal);
}

fn neighbor_changed(l: &mut Level, state: State, pos: Pos, neighbor: BlockId, moved: bool) {
    neighbor_changed_with(l, state, pos, neighbor, moved, has_neighbor_signal);
}

// RedstoneTorchBlock.tick, with the class's hasNeighborSignal.
pub fn tick_with(l: &mut Level, state: State, pos: Pos, has_neighbor_signal: fn(&mut Level, Pos, State) -> bool) {
    let has_neighbor_signal = has_neighbor_signal(l, pos, state);
    while let Some(&(_, when)) = l.redstone_toggles.front() {
        if l.game_time - when > RECENT_TOGGLE_TIMER {
            l.redstone_toggles.pop_front();
        } else {
            break;
        }
    }
    // The redstone change event (BlockRedstoneEvent) has no listener: the
    // new current stands.
    let lit_value = l.reg.get_bool(state, lit(l));
    if lit_value {
        if has_neighbor_signal {
            l.set(pos, l.reg.with_bool(state, lit(l), false), UPDATE_ALL);
            if is_toggled_too_frequently(l, pos, true) {
                // levelEvent(1502): McbenchLevel.levelEvent is a no-op.
                let current = l.get(pos);
                let block = l.block(current);
                l.schedule_tick(pos, block, RESTART_DELAY, PRIORITY_NORMAL);
            }
        }
    } else if !has_neighbor_signal && !is_toggled_too_frequently(l, pos, false) {
        l.set(pos, l.reg.with_bool(state, lit(l), true), UPDATE_ALL);
    }
}

// RedstoneTorchBlock.neighborChanged, with the class's hasNeighborSignal.
pub fn neighbor_changed_with(l: &mut Level, state: State, pos: Pos, _neighbor: BlockId, _moved: bool, has_neighbor_signal: fn(&mut Level, Pos, State) -> bool) {
    let block = l.block(state);
    if l.reg.get_bool(state, lit(l)) == has_neighbor_signal(l, pos, state) && !l.will_tick_this_tick(pos, block) {
        l.schedule_tick(pos, block, TOGGLE_DELAY, PRIORITY_NORMAL);
    }
}

// RedstoneTorchBlock.isToggledTooFrequently.
fn is_toggled_too_frequently(l: &mut Level, pos: Pos, log_toggle: bool) -> bool {
    if log_toggle {
        l.redstone_toggles.push_back((pos, l.game_time));
    }
    let mut i = 0;
    for &(at, _) in &l.redstone_toggles {
        if at == pos {
            i += 1;
            if i >= MAX_RECENT_TOGGLES {
                return true;
            }
        }
    }
    false
}
