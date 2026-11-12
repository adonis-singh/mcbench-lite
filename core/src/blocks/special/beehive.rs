// BeehiveBlock and BeehiveBlockEntity: a hive made without NBT has no bees,
// so its tick has nothing to release or hear, and fire beside it has no bee
// to send out. Its comparator signal is its honey level.

use super::Special;
use crate::blocks::{Behaviour, BlockEntity, NONE};
use crate::dir::{Dir, Pos};
use crate::level::Level;
use crate::property;
use crate::registry::State;

pub const BEHAVIOUR: Behaviour = Behaviour {
    update_shape: Some(update_shape),
    get_analog_output_signal: Some(get_analog_output_signal),
    trigger_event: Some(super::trigger_event),
    new_block_entity: Some(new_block_entity),
    ticker: Some(super::idle_ticker),
    ..NONE
};

fn honey_level(l: &Level) -> u16 {
    property!(l, "honey_level", ["0", "1", "2", "3", "4", "5"])
}

fn new_block_entity(_l: &mut Level, _state: State, _pos: Pos) -> BlockEntity {
    BlockEntity::Special(Special::Beehive)
}

// BeehiveBlock.updateShape: a fire block beside the hive empties it
// (emptyAllLivingFromHive finds no bee); the block entity is made if missing.
fn update_shape(l: &mut Level, state: State, pos: Pos, _dir: Dir, neighbor_pos: Pos, _neighbor: State) -> State {
    let beside = l.get(neighbor_pos);
    if l.reg.blocks[l.block(beside) as usize].class == "FireBlock" {
        l.block_entity(pos);
    }
    state
}

fn get_analog_output_signal(l: &mut Level, state: State, _pos: Pos, _dir: Dir) -> i32 {
    l.reg.get_int(state, honey_level(l))
}
