// DropperBlock, on DispenserBlock's code (dispenser.rs): it dispenses every
// item with DefaultDispenseItemBehavior, and into the container it faces.

use super::container::{self, copy, copy_with_count, is_empty, Cont, Container, Kind};
use super::{dispenser, hopper, BlockEntity, Behaviour};
use crate::dir::Pos;
use crate::level::Level;
use crate::registry::State;

pub const BEHAVIOUR: Behaviour = Behaviour { tick: Some(tick), new_block_entity: Some(new_block_entity), ..dispenser::BEHAVIOUR };

// DropperBlock.newBlockEntity.
fn new_block_entity(_l: &mut Level, _state: State, _pos: Pos) -> BlockEntity {
    BlockEntity::Container(Container::new(Kind::Dropper))
}

// DispenserBlock.tick, calling DropperBlock.dispenseFrom.
fn tick(l: &mut Level, state: State, pos: Pos) {
    dispense_from(l, state, pos);
}

// DropperBlock.dispenseFrom.
fn dispense_from(l: &mut Level, state: State, pos: Pos) {
    if !dispenser::dispenser_entity(l, pos, true) {
        return;
    }
    let random_slot = dispenser::get_random_slot(l, pos);
    if random_slot < 0 {
        return;
    }
    let slot = random_slot as usize;
    let item = Cont::Be(pos).get(l, slot);
    if is_empty(&item) {
        return;
    }
    let state_now = l.get(pos);
    let direction = dispenser::facing(l, state_now);
    let item_stack = match hopper::get_container_at(l, pos.relative(direction)) {
        None => dispenser::dispense(l, state, pos, &item),
        Some(mut container_at) => {
            // InventoryMoveItemEvent has no listener: the event's item is
            // the one-item copy, moved as it is.
            let moved = hopper::add_item(l, Some(&Cont::Be(pos)), &mut container_at, copy_with_count(&item, 1), Some(direction.opposite()));
            let item_stack = copy(&item);
            if is_empty(&moved) {
                container::shrink(&item_stack, 1);
            }
            item_stack
        }
    };
    Cont::Be(pos).set_item(l, slot, item_stack);
}
