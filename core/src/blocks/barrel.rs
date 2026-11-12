// BarrelBlock.

use super::container::{self, Container, Kind};
use super::{BlockEntity, Behaviour, NONE};
use crate::dir::Pos;
use crate::level::Level;
use crate::registry::State;

pub const BEHAVIOUR: Behaviour = Behaviour {
    affect_neighbors_after_removal: Some(container::affect_neighbors_after_removal),
    tick: Some(tick),
    get_analog_output_signal: Some(container::analog_output_signal),
    trigger_event: Some(container::trigger_event),
    new_block_entity: Some(new_block_entity),
    ..NONE
};

// BarrelBlock.tick: BarrelBlockEntity.recheckOpen, which is
// ContainerOpenersCounter.recheckOpeners. No entity holds the barrel open
// (none exists), so the count stays 0 and openerCountChanged does nothing:
// all it does is look up the block entity.
fn tick(l: &mut Level, _state: State, pos: Pos) {
    l.block_entity(pos);
}

// BarrelBlock.newBlockEntity.
fn new_block_entity(_l: &mut Level, _state: State, _pos: Pos) -> BlockEntity {
    BlockEntity::Container(Container::new(Kind::Barrel))
}
