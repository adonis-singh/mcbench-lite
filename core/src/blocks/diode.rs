// DiodeBlock: shared by RepeaterBlock and ComparatorBlock. The functions
// are DiodeBlock's own code; where it calls a method its subclasses
// override (isLocked, getDelay, shouldTurnOn, getInputSignal,
// getOutputSignal, checkTickOnNeighbor, sideInputDiodesOnly), `variant`
// picks the subclass the state's block is.

use super::{comparator, repeater};
use crate::dir::{Dir, Pos};
use crate::level::{refuse, Level, PRIORITY_EXTREMELY_HIGH, PRIORITY_HIGH, PRIORITY_VERY_HIGH, UPDATE_CLIENTS};
use crate::property;
use crate::registry::{BlockId, State, Support};

// HorizontalDirectionalBlock.FACING.
pub fn facing(l: &Level) -> u16 {
    property!(l, "facing", ["north", "south", "west", "east"])
}

// DiodeBlock.POWERED.
pub fn powered(l: &Level) -> u16 {
    property!(l, "powered", ["true", "false"])
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Variant {
    Repeater,
    Comparator,
}

fn variant(l: &Level, state: State) -> Variant {
    let block = l.block(state);
    if block == l.known.repeater {
        Variant::Repeater
    } else if block == l.known.comparator {
        Variant::Comparator
    } else {
        panic!("{} is not a diode", l.reg.describe(state))
    }
}

// DiodeBlock.canSurviveOn.
pub fn can_survive_on(l: &Level, state: State, pos: Pos) -> bool {
    crate::shape_ops::is_face_sturdy(l, state, pos, Dir::Up, Support::Rigid)
}

// DiodeBlock.canSurvive.
pub fn can_survive(l: &mut Level, _state: State, pos: Pos) -> bool {
    let below = l.get(pos.below());
    can_survive_on(l, below, pos.below())
}

// DiodeBlock.tick.
pub fn tick(l: &mut Level, state: State, pos: Pos) {
    if !is_locked(l, pos, state) {
        let powered_value = l.reg.get_bool(state, powered(l));
        let should_turn_on = should_turn_on(l, pos, state);
        if powered_value && !should_turn_on {
            // The redstone change event has no listener: the new current stands.
            l.set(pos, l.reg.with_bool(state, powered(l), false), UPDATE_CLIENTS);
        } else if !powered_value {
            l.set(pos, l.reg.with_bool(state, powered(l), true), UPDATE_CLIENTS);
            if !should_turn_on {
                let delay = get_delay(l, state);
                l.schedule_tick(pos, l.block(state), delay, PRIORITY_VERY_HIGH);
            }
        }
    }
}

// DiodeBlock.getDirectSignal.
pub fn get_direct_signal(l: &mut Level, state: State, pos: Pos, side: Dir) -> i32 {
    super::get_signal(l, state, pos, side)
}

// DiodeBlock.getSignal.
pub fn get_signal(l: &mut Level, state: State, pos: Pos, side: Dir) -> i32 {
    if !l.reg.get_bool(state, powered(l)) {
        0
    } else if l.reg.get_dir(state, facing(l)) == side {
        get_output_signal(l, pos, state)
    } else {
        0
    }
}

// DiodeBlock.neighborChanged. A diode that cannot survive drops its item
// and is removed.
pub fn neighbor_changed(l: &mut Level, state: State, pos: Pos, _neighbor: BlockId, _moved: bool) {
    if super::can_survive(l, state, pos) {
        check_tick_on_neighbor(l, pos, state);
    } else {
        // The block entity (a comparator's) is read for the drops, which do
        // not copy it.
        if l.has_block_entity(state) {
            l.block_entity(pos);
        }
        crate::entity::item::drop_resources(l, state, pos);
        l.remove_block(pos, false);
        for dir in crate::dir::ALL {
            l.update_neighbors_at(pos.relative(dir), l.block(state));
        }
    }
}

// DiodeBlock.checkTickOnNeighbor, as the subclass has it.
pub fn check_tick_on_neighbor(l: &mut Level, pos: Pos, state: State) {
    match variant(l, state) {
        Variant::Repeater => check_tick_on_neighbor_diode(l, pos, state),
        Variant::Comparator => comparator::check_tick_on_neighbor(l, pos, state),
    }
}

// DiodeBlock's own checkTickOnNeighbor.
fn check_tick_on_neighbor_diode(l: &mut Level, pos: Pos, state: State) {
    if !is_locked(l, pos, state) {
        let powered_value = l.reg.get_bool(state, powered(l));
        let should_turn_on = should_turn_on(l, pos, state);
        if powered_value != should_turn_on && !l.will_tick_this_tick(pos, l.block(state)) {
            let mut priority = PRIORITY_HIGH;
            if should_prioritize(l, pos, state) {
                priority = PRIORITY_EXTREMELY_HIGH;
            } else if powered_value {
                priority = PRIORITY_VERY_HIGH;
            }
            let delay = get_delay(l, state);
            l.schedule_tick(pos, l.block(state), delay, priority);
        }
    }
}

// DiodeBlock.isLocked, as the subclass has it.
pub fn is_locked(l: &mut Level, pos: Pos, state: State) -> bool {
    match variant(l, state) {
        Variant::Repeater => repeater::is_locked(l, pos, state),
        Variant::Comparator => false,
    }
}

// DiodeBlock.shouldTurnOn, as the subclass has it.
pub fn should_turn_on(l: &mut Level, pos: Pos, state: State) -> bool {
    match variant(l, state) {
        Variant::Repeater => get_input_signal(l, pos, state) > 0,
        Variant::Comparator => comparator::should_turn_on(l, pos, state),
    }
}

// DiodeBlock.getInputSignal, as the subclass has it.
fn get_input_signal(l: &mut Level, pos: Pos, state: State) -> i32 {
    match variant(l, state) {
        Variant::Repeater => input_signal(l, pos, state),
        Variant::Comparator => comparator::get_input_signal(l, pos, state),
    }
}

// DiodeBlock.getInputSignal itself.
pub fn input_signal(l: &mut Level, pos: Pos, state: State) -> i32 {
    let direction = l.reg.get_dir(state, facing(l));
    let at = pos.relative(direction);
    let signal = l.signal(at, direction);
    if signal >= 15 {
        signal
    } else {
        let at_state = l.get(at);
        let wire = if l.block(at_state) == l.known.redstone_wire { super::wire_power(l, at_state) } else { 0 };
        signal.max(wire)
    }
}

// DiodeBlock.getAlternateSignal.
pub fn get_alternate_signal(l: &mut Level, pos: Pos, state: State) -> i32 {
    let direction = l.reg.get_dir(state, facing(l));
    let clockwise = direction.clockwise();
    let counter_clockwise = direction.counter_clockwise();
    let diodes_only = variant(l, state) == Variant::Repeater;
    let first = l.control_input_signal(pos.relative(clockwise), clockwise, diodes_only);
    let second = l.control_input_signal(pos.relative(counter_clockwise), counter_clockwise, diodes_only);
    first.max(second)
}

// DiodeBlock.onPlace.
pub fn on_place(l: &mut Level, state: State, pos: Pos, _old: State, _moved: bool) {
    update_neighbors_in_front(l, pos, state);
}

// DiodeBlock.affectNeighborsAfterRemoval.
pub fn affect_neighbors_after_removal(l: &mut Level, state: State, pos: Pos, moved: bool) {
    if !moved {
        update_neighbors_in_front(l, pos, state);
    }
}

// DiodeBlock.updateNeighborsInFront.
pub fn update_neighbors_in_front(l: &mut Level, pos: Pos, state: State) {
    let direction = l.reg.get_dir(state, facing(l));
    let at = pos.relative(direction.opposite());
    let block = l.block(state);
    l.neighbor_changed(at, block);
    l.update_neighbors_at_except_from_facing(at, block, direction);
}

// DiodeBlock.getOutputSignal, as the subclass has it.
pub fn get_output_signal(l: &mut Level, pos: Pos, state: State) -> i32 {
    match variant(l, state) {
        Variant::Repeater => 15,
        Variant::Comparator => comparator::get_output_signal(l, pos),
    }
}

// DiodeBlock.shouldPrioritize.
pub fn should_prioritize(l: &mut Level, pos: Pos, state: State) -> bool {
    let opposite = l.reg.get_dir(state, facing(l)).opposite();
    let behind = l.get(pos.relative(opposite));
    super::is_diode(l, behind) && l.reg.get_dir(behind, facing(l)) != opposite
}

// getDelay, as the subclass has it.
pub fn get_delay(l: &Level, state: State) -> i32 {
    match variant(l, state) {
        Variant::Repeater => repeater::get_delay(l, state),
        Variant::Comparator => 2,
    }
}
