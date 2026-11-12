// EnderChestBlock.

use super::container::{self, Container, Kind};
use super::{BlockEntity, Behaviour, NONE};
use crate::dir::{Dir, Pos};
use crate::level::Level;
use crate::registry::State;

pub const BEHAVIOUR: Behaviour = Behaviour {
    update_shape: Some(update_shape),
    tick: Some(tick),
    trigger_event: Some(container::trigger_event),
    new_block_entity: Some(new_block_entity),
    // getTicker gives the lid animation's ticker on the client only.
    ticker: Some(container::no_ticker),
    ..NONE
};

// EnderChestBlock.updateShape; BlockBehaviour's returns the state.
fn update_shape(l: &mut Level, state: State, pos: Pos, _direction: Dir, _neighbor_pos: Pos, _neighbor: State) -> State {
    super::shapes::common::schedule_water_tick(l, state, pos);
    state
}

// EnderChestBlock.tick: EnderChestBlockEntity.recheckOpen, which is
// ContainerOpenersCounter.recheckOpeners. No entity holds the chest open
// (none exists), so the count stays 0, and openerCountChanged posts the
// open count as a block event.
fn tick(l: &mut Level, state: State, pos: Pos) {
    if matches!(l.block_entity(pos), Some(BlockEntity::Container(Container { kind: Kind::EnderChest, .. }))) {
        l.block_event(pos, l.block(state), 1, 0);
    }
}

// EnderChestBlock.newBlockEntity.
fn new_block_entity(_l: &mut Level, _state: State, _pos: Pos) -> BlockEntity {
    BlockEntity::Container(Container::new(Kind::EnderChest))
}
