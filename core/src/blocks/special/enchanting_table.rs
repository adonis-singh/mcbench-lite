// EnchantingTableBlock and its block entity. The block entity ticks on the
// client only (its book animation), so nothing here changes.

use super::Special;
use crate::blocks::{Behaviour, BlockEntity, NONE};
use crate::dir::Pos;
use crate::level::Level;
use crate::registry::State;

pub const BEHAVIOUR: Behaviour = Behaviour {
    trigger_event: Some(super::trigger_event),
    new_block_entity: Some(new_block_entity),
    ticker: Some(super::no_ticker),
    ..NONE
};

fn new_block_entity(_l: &mut Level, _state: State, _pos: Pos) -> BlockEntity {
    BlockEntity::Special(Special::EnchantingTable { name: None })
}
