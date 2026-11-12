// ChiseledBookShelfBlock: the signal is the last interacted slot plus one.

use super::container;
use super::entities::{Entity, Shelf};
use super::{BlockEntity, Behaviour, NONE};
use crate::dir::{Dir, Pos};
use crate::level::Level;
use crate::registry::State;

pub const BEHAVIOUR: Behaviour = Behaviour {
    affect_neighbors_after_removal: Some(container::affect_neighbors_after_removal),
    get_analog_output_signal: Some(get_analog_output_signal),
    new_block_entity: Some(new_block_entity),
    ..NONE
};

// ChiseledBookShelfBlock.getAnalogOutputSignal: the block entity
// (Level.getBlockEntity makes it if missing) gives lastInteractedSlot + 1.
fn get_analog_output_signal(l: &mut Level, _state: State, pos: Pos, _direction: Dir) -> i32 {
    match l.block_entity(pos) {
        Some(BlockEntity::Entity(Entity::ChiseledBookShelf(shelf))) => shelf.last_interacted_slot + 1,
        Some(_) | None => 0,
    }
}

fn new_block_entity(_l: &mut Level, _state: State, _pos: Pos) -> BlockEntity {
    BlockEntity::Entity(Entity::ChiseledBookShelf(Shelf::new()))
}

// BlockStateProperties.SLOT_0_OCCUPIED to SLOT_5_OCCUPIED.
fn slot_occupied(l: &Level, slot: usize) -> u16 {
    match slot {
        0 => crate::property!(l, "slot_0_occupied", ["true", "false"]),
        1 => crate::property!(l, "slot_1_occupied", ["true", "false"]),
        2 => crate::property!(l, "slot_2_occupied", ["true", "false"]),
        3 => crate::property!(l, "slot_3_occupied", ["true", "false"]),
        4 => crate::property!(l, "slot_4_occupied", ["true", "false"]),
        5 => crate::property!(l, "slot_5_occupied", ["true", "false"]),
        _ => panic!("no slot {slot}"),
    }
}

fn shelf_mut(l: &mut Level, pos: Pos) -> &mut Shelf {
    match l.block_entities.get_mut(&pos) {
        Some(BlockEntity::Entity(Entity::ChiseledBookShelf(shelf))) => shelf,
        _ => panic!("a chiseled bookshelf without its block entity"),
    }
}

// ChiseledBookShelfBlockEntity.acceptsItemType: ItemStack.is(ItemTags.BOOKSHELF_BOOKS).
fn accepts_item_type(l: &Level, stack: &container::Stack) -> bool {
    let stack = stack.borrow();
    stack.count > 0 && l.reg.item_tags["minecraft:bookshelf_books"][stack.item as usize]
}

// ChiseledBookShelfBlockEntity.updateState: the slot becomes the last
// interacted one and the block shows which slots hold a book.
fn update_state(l: &mut Level, pos: Pos, slot: usize) {
    if slot >= 6 {
        // An error is logged.
        return;
    }
    shelf_mut(l, pos).last_interacted_slot = slot as i32;
    let mut state = l.get(pos);
    for i in 0..6 {
        let occupied = !container::is_empty(&shelf_mut(l, pos).items[i]);
        state = l.reg.with_bool(state, slot_occupied(l, i), occupied);
    }
    l.set(pos, state, crate::level::UPDATE_ALL);
    let event = crate::game_event!(l, "block_change");
    l.game_event_in(event, pos, crate::game_event::Context::of_state(state));
}

// ChiseledBookShelfBlockEntity.setItem.
pub fn set_item(l: &mut Level, pos: Pos, slot: usize, stack: container::Stack) {
    if accepts_item_type(l, &stack) {
        shelf_mut(l, pos).items[slot] = stack;
        update_state(l, pos, slot);
    } else if container::is_empty(&stack) {
        remove_item(l, pos, slot);
    }
}

// ChiseledBookShelfBlockEntity.removeItem.
pub fn remove_item(l: &mut Level, pos: Pos, slot: usize) -> container::Stack {
    let item = std::mem::replace(&mut shelf_mut(l, pos).items[slot], container::empty_stack());
    if !container::is_empty(&item) {
        update_state(l, pos, slot);
    }
    item
}

// ListBackedContainer.canPlaceItem (the shelf holds one of an item).
pub fn can_place_item(l: &Level, pos: Pos, slot: usize, stack: &container::Stack) -> bool {
    let current = container::slots(l, pos)[slot].clone();
    accepts_item_type(l, stack) && (container::is_empty(&current) || container::count(&current) < 1.min(container::max_stack(l.reg, stack)))
}
