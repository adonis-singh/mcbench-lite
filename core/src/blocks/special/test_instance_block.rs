// TestInstanceBlock and TestInstanceBlockEntity: a block entity holding no
// test (a new one is cleared); running one needs a game master.

use super::Special;
use crate::blocks::{Behaviour, BlockEntity, NONE};
use crate::dir::Pos;
use crate::level::Level;
use crate::registry::State;

pub const BEHAVIOUR: Behaviour = Behaviour { trigger_event: Some(super::trigger_event), new_block_entity: Some(new_block_entity), ..NONE };

fn new_block_entity(_l: &mut Level, _state: State, _pos: Pos) -> BlockEntity {
    BlockEntity::Special(Special::TestInstanceBlock)
}
