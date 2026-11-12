// TrappedChestBlock, on ChestBlock's code (chest.rs).

use super::container::{Container, Kind};
use super::{chest, BlockEntity, Behaviour};
use crate::dir::{Dir, Pos};
use crate::level::Level;
use crate::registry::State;

pub const BEHAVIOUR: Behaviour = Behaviour {
    get_signal: Some(get_signal),
    get_direct_signal: Some(get_direct_signal),
    new_block_entity: Some(new_block_entity),
    ..chest::BEHAVIOUR
};

// ChestBlockEntity.getOpenCount: the block entity's openers counter, 0 (see
// Kind::Chest).
fn open_count(l: &mut Level, pos: Pos) -> i32 {
    let state = l.get(pos);
    if l.has_block_entity(state) {
        l.block_entity(pos);
    }
    0
}

// TrappedChestBlock.getSignal.
fn get_signal(l: &mut Level, _state: State, pos: Pos, _side: Dir) -> i32 {
    open_count(l, pos).clamp(0, 15)
}

// TrappedChestBlock.getDirectSignal.
fn get_direct_signal(l: &mut Level, state: State, pos: Pos, side: Dir) -> i32 {
    if side == Dir::Up {
        super::get_signal(l, state, pos, side)
    } else {
        0
    }
}

// TrappedChestBlock.newBlockEntity.
fn new_block_entity(_l: &mut Level, _state: State, _pos: Pos) -> BlockEntity {
    BlockEntity::Container(Container::new(Kind::TrappedChest))
}
