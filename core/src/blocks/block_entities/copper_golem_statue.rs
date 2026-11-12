// CopperGolemStatueBlock and WeatheringCopperGolemStatueBlock (which adds
// random ticks and an axe that turns the statue into a golem, both
// unreachable here): the pose is the analog output, a waterlogged statue
// schedules a water tick, and the block entity keeps nothing. Using the
// statue (changing its pose, reanimating it) is a player's input.

use super::{trigger_event, Plain};
use crate::blocks::shapes::common::schedule_water_tick;
use crate::blocks::{Behaviour, BlockEntity, NONE};
use crate::dir::{Dir, Pos};
use crate::level::Level;
use crate::property;
use crate::registry::State;

pub const BEHAVIOUR: Behaviour = Behaviour {
    update_shape: Some(update_shape),
    affect_neighbors_after_removal: Some(affect_neighbors_after_removal),
    get_analog_output_signal: Some(get_analog_output_signal),
    trigger_event: Some(trigger_event),
    new_block_entity: Some(new_block_entity),
    ..NONE
};

// CopperGolemStatueBlock.POSE (standing, sitting, running, star).
fn pose(l: &Level) -> u16 {
    property!(l, "copper_golem_pose", ["standing", "sitting", "running", "star"])
}

// CopperGolemStatueBlock.updateShape: BlockBehaviour.updateShape keeps the state.
fn update_shape(l: &mut Level, state: State, pos: Pos, _direction: Dir, _neighbor_pos: Pos, _neighbor: State) -> State {
    schedule_water_tick(l, state, pos);
    state
}

// CopperGolemStatueBlock.affectNeighborsAfterRemoval.
fn affect_neighbors_after_removal(l: &mut Level, state: State, pos: Pos, _moved: bool) {
    l.update_neighbour_for_output_signal(pos, l.block(state));
}

// CopperGolemStatueBlock.getAnalogOutputSignal: the pose's ordinal plus one.
fn get_analog_output_signal(l: &mut Level, state: State, _pos: Pos, _direction: Dir) -> i32 {
    l.reg.get(state, pose(l)) as i32 + 1
}

// CopperGolemStatueBlock.newBlockEntity.
fn new_block_entity(_l: &mut Level, _state: State, _pos: Pos) -> BlockEntity {
    BlockEntity::Plain(Plain::CopperGolemStatue)
}
