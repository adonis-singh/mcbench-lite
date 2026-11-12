// AbstractSkullBlock, which SkullBlock (skulls and heads on the floor),
// WallSkullBlock, PlayerHeadBlock, PlayerWallHeadBlock, WitherSkullBlock,
// WitherWallSkullBlock and PiglinWallSkullBlock extend: the powered state
// follows the redstone signal around the block, and the block entity
// (SkullBlockEntity) has no server ticker (AbstractSkullBlock.getTicker
// animates on the client only).
//
// WitherSkullBlock.checkSpawn runs only from setPlacedBy, which a setblock
// command never calls, so no wither is summoned.

use super::{Plain, trigger_event};
use crate::blocks::container::no_ticker;
use crate::blocks::{Behaviour, BlockEntity, NONE};
use crate::dir::Pos;
use crate::level::Level;
use crate::property;
use crate::registry::{BlockId, State};

pub const BEHAVIOUR: Behaviour = Behaviour {
    neighbor_changed: Some(neighbor_changed),
    trigger_event: Some(trigger_event),
    new_block_entity: Some(new_block_entity),
    ticker: Some(no_ticker),
    ..NONE
};

// AbstractSkullBlock.POWERED.
fn powered(l: &Level) -> u16 {
    property!(l, "powered", ["true", "false"])
}

// AbstractSkullBlock.neighborChanged.
fn neighbor_changed(l: &mut Level, state: State, pos: Pos, _neighbor_block: BlockId, _moved: bool) {
    let has_neighbor_signal = l.has_neighbor_signal(pos);
    if has_neighbor_signal != l.reg.get_bool(state, powered(l)) {
        l.set(pos, l.reg.with_bool(state, powered(l), has_neighbor_signal), 2);
    }
}

// AbstractSkullBlock.newBlockEntity.
fn new_block_entity(_l: &mut Level, _state: State, _pos: Pos) -> BlockEntity {
    BlockEntity::Plain(Plain::Skull { note_block_sound: None, custom_name: None })
}
