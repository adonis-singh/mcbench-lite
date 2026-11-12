// DecoratedPotBlock and DecoratedPotBlockEntity: a pot is placed without
// sherds, and its block entity is a single-slot Container
// (ContainerSingleItem): hoppers, droppers and crafters fill and empty it
// (setTheItem and splitTheItem tell no one; the transfer's setChanged tells
// the comparators), and its comparator signal is that of its slot. Only a
// player's click or a projectile makes it wobble or crack.

use crate::blocks::container;
use crate::blocks::entities::{Entity, Pot};
use crate::blocks::{Behaviour, BlockEntity, InputPlayer, NONE};
use crate::dir::{Dir, Pos};
use crate::level::Level;
use crate::registry::State;

pub const BEHAVIOUR: Behaviour = Behaviour {
    update_shape: Some(super::waterlogged::update_shape),
    affect_neighbors_after_removal: Some(container::affect_neighbors_after_removal),
    get_analog_output_signal: Some(get_analog_output_signal),
    trigger_event: Some(trigger_event),
    new_block_entity: Some(new_block_entity),
    use_without_item: Some(use_without_item),
    ..NONE
};

// DecoratedPotBlock.useWithoutItem (an empty hand: useItemOn tries it): the
// sound (a fixed pitch: the world's thread-safe random), the negative wobble
// (DecoratedPotBlockEntity.wobble: a block event, WobbleStyle.NEGATIVE's
// ordinal), and the player as the source of a block change.
fn use_without_item(l: &mut Level, state: State, pos: Pos, player: Option<InputPlayer>) -> bool {
    if !matches!(l.block_entity(pos), Some(BlockEntity::Entity(Entity::DecoratedPot(_)))) {
        return false;
    }
    l.block_event(pos, l.block(state), 1, 1);
    l.game_event_at(player.map(|p| p.source), crate::game_event!(l, "block_change"), pos);
    true
}

fn new_block_entity(_l: &mut Level, _state: State, _pos: Pos) -> BlockEntity {
    BlockEntity::Entity(Entity::DecoratedPot(Pot::new()))
}

// AbstractContainerMenu.getRedstoneSignalFromBlockEntity of the pot's slot
// (Level.getBlockEntity makes the block entity if missing).
fn get_analog_output_signal(l: &mut Level, _state: State, pos: Pos, _dir: Dir) -> i32 {
    crate::blocks::entities::container_signal(l, pos)
}

// BaseEntityBlock.triggerEvent: Block's returns false, then the block entity
// (Level.getBlockEntity makes it if missing) answers:
// DecoratedPotBlockEntity.triggerEvent takes a wobble (the client's
// animation) of a style that exists.
fn trigger_event(l: &mut Level, _state: State, pos: Pos, id: i32, param: i32) -> bool {
    match l.block_entity(pos) {
        Some(BlockEntity::Entity(Entity::DecoratedPot(_))) => id == 1 && (0..2).contains(&param),
        Some(_) | None => false,
    }
}
