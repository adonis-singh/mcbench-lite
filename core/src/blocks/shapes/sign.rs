// SignBlock, the code the sign classes share: their block entities, which
// have no ticker (SignBlock.getTicker gives none), and updateShape's tail.

use super::common::schedule_water_tick;
use crate::blocks::BlockEntity;
use crate::dir::Pos;
use crate::level::Level;
use crate::registry::State;

// SignBlock.updateShape: a waterlogged sign schedules a water tick, then
// BlockBehaviour.updateShape keeps the state.
pub fn update_shape(l: &mut Level, state: State, pos: Pos) -> State {
    schedule_water_tick(l, state, pos);
    state
}

// SignBlock.newBlockEntity: SignBlockEntity (the standing and wall signs).
pub fn new_block_entity(_l: &mut Level, _state: State, _pos: Pos) -> BlockEntity {
    BlockEntity::Sign(super::super::entities::Sign::new())
}

// WallHangingSignBlock.newBlockEntity and CeilingHangingSignBlock's:
// HangingSignBlockEntity.
pub fn new_hanging_block_entity(_l: &mut Level, _state: State, _pos: Pos) -> BlockEntity {
    BlockEntity::HangingSign(super::super::entities::Sign::new())
}

// SignBlock.getTicker: none.
pub fn ticker(_l: &Level, _state: State) -> Option<fn(&mut Level, Pos, State)> {
    None
}

// BaseEntityBlock.triggerEvent: BlockBehaviour.triggerEvent gives false,
// then the block entity's, which for signs and banners is false too.
pub fn trigger_event(l: &mut Level, _state: State, pos: Pos, _id: i32, _param: i32) -> bool {
    l.block_entity(pos);
    false
}
