// SpawnerBlock and SpawnerBlockEntity (BaseSpawner). A spawner acts only
// for a player within its range (isNearPlayer), and none exists: its ticker
// counts a delay it never uses. Its saved data stays what a new spawner
// writes.

use super::Special;
use crate::blocks::{Behaviour, BlockEntity, NONE};
use crate::dir::Pos;
use crate::level::Level;
use crate::registry::State;

pub const BEHAVIOUR: Behaviour = Behaviour {
    trigger_event: Some(super::trigger_event),
    new_block_entity: Some(new_block_entity),
    ticker: Some(super::idle_ticker),
    ..NONE
};

fn new_block_entity(_l: &mut Level, _state: State, _pos: Pos) -> BlockEntity {
    BlockEntity::Special(Special::Spawner)
}
