// ComparatorBlock, on DiodeBlock's code (diode.rs).

use super::{diode, BlockEntity, Behaviour, InputPlayer, NONE};
use crate::dir::{Dir, Pos};
use crate::level::{Level, PRIORITY_HIGH, PRIORITY_NORMAL, UPDATE_CLIENTS};
use crate::property;
use crate::registry::State;

pub const BEHAVIOUR: Behaviour = Behaviour {
    update_shape: Some(update_shape),
    neighbor_changed: Some(diode::neighbor_changed),
    on_place: Some(diode::on_place),
    affect_neighbors_after_removal: Some(diode::affect_neighbors_after_removal),
    tick: Some(tick),
    get_signal: Some(diode::get_signal),
    get_direct_signal: Some(diode::get_direct_signal),
    trigger_event: Some(trigger_event),
    can_survive: Some(diode::can_survive),
    new_block_entity: Some(new_block_entity),
    use_without_item: Some(use_without_item),
    ..NONE
};

// ComparatorBlock.MODE: compare or subtract.
fn mode(l: &Level) -> u16 {
    property!(l, "mode", ["compare", "subtract"])
}

fn is_subtract(l: &Level, state: State) -> bool {
    l.reg.value_name(state, mode(l)) == "subtract"
}

// ComparatorBlock.updateShape.
fn update_shape(l: &mut Level, state: State, _pos: Pos, direction: Dir, neighbor_pos: Pos, neighbor: State) -> State {
    if direction == Dir::Down && !diode::can_survive_on(l, neighbor, neighbor_pos) {
        l.known.air
    } else {
        state
    }
}

// ComparatorBlock.getOutputSignal: the block entity's output signal
// (Level.getBlockEntity makes it if missing).
pub fn get_output_signal(l: &mut Level, pos: Pos) -> i32 {
    match l.block_entity(pos) {
        Some(BlockEntity::Comparator { output }) => *output,
        Some(_) | None => 0,
    }
}

// ComparatorBlock.calculateOutputSignal.
fn calculate_output_signal(l: &mut Level, pos: Pos, state: State) -> i32 {
    let input_signal = get_input_signal(l, pos, state);
    if input_signal == 0 {
        0
    } else {
        let alternate_signal = diode::get_alternate_signal(l, pos, state);
        if alternate_signal > input_signal {
            0
        } else if is_subtract(l, state) {
            input_signal - alternate_signal
        } else {
            input_signal
        }
    }
}

// ComparatorBlock.shouldTurnOn.
pub fn should_turn_on(l: &mut Level, pos: Pos, state: State) -> bool {
    let input_signal = get_input_signal(l, pos, state);
    if input_signal == 0 {
        false
    } else {
        let alternate_signal = diode::get_alternate_signal(l, pos, state);
        input_signal > alternate_signal || input_signal == alternate_signal && !is_subtract(l, state)
    }
}

// ComparatorBlock.getInputSignal.
pub fn get_input_signal(l: &mut Level, pos: Pos, state: State) -> i32 {
    let mut i = diode::input_signal(l, pos, state);
    let direction = l.reg.get_dir(state, diode::facing(l));
    let mut at = pos.relative(direction);
    let mut at_state = l.get(at);
    if l.has_analog_output(at_state) {
        i = super::get_analog_output_signal(l, at_state, at, direction.opposite());
    } else if i < 15 && l.is_conductor(at_state) {
        at = at.relative(direction);
        at_state = l.get(at);
        let item_frame = get_item_frame(l, direction, at);
        let frame_output = item_frame.unwrap_or(i32::MIN);
        let block_output = if l.has_analog_output(at_state) { super::get_analog_output_signal(l, at_state, at, direction.opposite()) } else { i32::MIN };
        let max = frame_output.max(block_output);
        if max != i32::MIN {
            i = max;
        }
    }
    i
}

// ComparatorBlock.getItemFrame, as the frame's analog output if there is
// exactly one facing this way; the entity lookup loads no chunks.
fn get_item_frame(l: &mut Level, direction: Dir, pos: Pos) -> Option<i32> {
    crate::entity::frame::comparator_frame(l, direction, pos)
}

// ComparatorBlock.checkTickOnNeighbor.
pub fn check_tick_on_neighbor(l: &mut Level, pos: Pos, state: State) {
    let block = l.block(state);
    if !l.will_tick_this_tick(pos, block) {
        let i = calculate_output_signal(l, pos, state);
        let i1 = get_output_signal(l, pos);
        if i != i1 || l.reg.get_bool(state, diode::powered(l)) != should_turn_on(l, pos, state) {
            let priority = if diode::should_prioritize(l, pos, state) { PRIORITY_HIGH } else { PRIORITY_NORMAL };
            l.schedule_tick(pos, block, 2, priority);
        }
    }
}

// ComparatorBlock.refreshOutputState.
fn refresh_output_state(l: &mut Level, pos: Pos, state: State) {
    let i = calculate_output_signal(l, pos, state);
    let mut i1 = 0;
    if let Some(BlockEntity::Comparator { output }) = l.block_entity(pos) {
        i1 = *output;
        *output = i;
    }
    if i1 != i || !is_subtract(l, state) {
        let should_turn_on = should_turn_on(l, pos, state);
        let powered_value = l.reg.get_bool(state, diode::powered(l));
        // The redstone change event has no listener: the new current stands.
        if powered_value && !should_turn_on {
            l.set(pos, l.reg.with_bool(state, diode::powered(l), false), UPDATE_CLIENTS);
        } else if !powered_value && should_turn_on {
            l.set(pos, l.reg.with_bool(state, diode::powered(l), true), UPDATE_CLIENTS);
        }
        diode::update_neighbors_in_front(l, pos, state);
    }
}

// ComparatorBlock.tick.
fn tick(l: &mut Level, state: State, pos: Pos) {
    refresh_output_state(l, pos, state);
}

// ComparatorBlock.triggerEvent: the block entity's triggerEvent is
// BlockEntity's, false; getBlockEntity makes the entity if missing.
fn trigger_event(l: &mut Level, _state: State, pos: Pos, _id: i32, _param: i32) -> bool {
    l.block_entity(pos);
    false
}

// ComparatorBlock.newBlockEntity: ComparatorBlockEntity, output 0.
fn new_block_entity(_l: &mut Level, _state: State, _pos: Pos) -> BlockEntity {
    BlockEntity::Comparator { output: 0 }
}

// ComparatorBlock.useWithoutItem: for a player who may build (the input
// player is a survival player), the mode flips; the click sound draws from
// the world's thread-safe random, not the level's; the output is refreshed.
fn use_without_item(l: &mut Level, state: State, pos: Pos, _player: Option<InputPlayer>) -> bool {
    let state = l.reg.cycle(state, mode(l));
    l.set(pos, state, UPDATE_CLIENTS);
    refresh_output_state(l, pos, state);
    true
}
