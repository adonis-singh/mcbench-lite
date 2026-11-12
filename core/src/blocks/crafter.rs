// CrafterBlock.

use super::container::{self, count, is_empty, max_stack, same_item, Cont, Container, Kind};
use super::hopper;
use super::{BlockEntity, Behaviour, NONE};
use crate::dir::{Dir, Pos};
use crate::level::{Level, PRIORITY_NORMAL, UPDATE_ALL, UPDATE_CLIENTS};
use crate::property;
use super::recipes;
use crate::registry::{BlockId, State};

pub const BEHAVIOUR: Behaviour = Behaviour {
    affect_neighbors_after_removal: Some(container::affect_neighbors_after_removal),
    neighbor_changed: Some(neighbor_changed),
    tick: Some(tick),
    get_analog_output_signal: Some(get_analog_output_signal),
    trigger_event: Some(container::trigger_event),
    new_block_entity: Some(new_block_entity),
    ticker: Some(ticker),
    ..NONE
};

// CrafterBlock.TRIGGERED and CRAFTING.
fn triggered(l: &Level) -> u16 {
    property!(l, "triggered", ["true", "false"])
}

fn crafting(l: &Level) -> u16 {
    property!(l, "crafting", ["true", "false"])
}

// CrafterBlock.getAnalogOutputSignal: CrafterBlockEntity.getRedstoneSignal
// counts the slots holding a stack or disabled.
fn get_analog_output_signal(l: &mut Level, _state: State, pos: Pos, _direction: Dir) -> i32 {
    match l.block_entity(pos) {
        Some(BlockEntity::Container(Container { kind: Kind::Crafter { disabled, .. }, items, .. })) => (0..9).filter(|i| !is_empty(&items[*i]) || disabled[*i]).count() as i32,
        Some(_) | None => 0,
    }
}

// CrafterBlock.neighborChanged, with setBlockEntityTriggered on the block
// entity looked up first (the same one after the state change).
fn neighbor_changed(l: &mut Level, state: State, pos: Pos, _neighbor: BlockId, _moved: bool) {
    let has_neighbor_signal = l.has_neighbor_signal(pos);
    let triggered_value = l.reg.get_bool(state, triggered(l));
    l.block_entity(pos);
    if has_neighbor_signal && !triggered_value {
        l.schedule_tick(pos, l.block(state), 4, PRIORITY_NORMAL);
        l.set(pos, l.reg.with_bool(state, triggered(l), true), UPDATE_CLIENTS);
        set_triggered(l, pos, true);
    } else if !has_neighbor_signal && triggered_value {
        l.set(pos, l.reg.with_bool(l.reg.with_bool(state, triggered(l), false), crafting(l), false), UPDATE_CLIENTS);
        set_triggered(l, pos, false);
    }
}

fn set_triggered(l: &mut Level, pos: Pos, value: bool) {
    if let Some(BlockEntity::Container(Container { kind: Kind::Crafter { triggered, .. }, .. })) = l.block_entities.get_mut(&pos) {
        *triggered = value as i32;
    }
}

// CrafterBlock.tick: dispenseFrom. The block entity is looked up
// (Level.getBlockEntity makes it if missing). With no recipe matching the
// grid the level event for it reaches nothing here. CrafterCraftEvent has no
// listener: the result stands.
fn tick(l: &mut Level, state: State, pos: Pos) {
    if !matches!(l.block_entity(pos), Some(BlockEntity::Container(Container { kind: Kind::Crafter { .. }, .. }))) {
        return;
    }
    let stacks = container::be(l, pos).items.clone();
    let input = recipes::Input::of(&stacks);
    let Some(recipe) = recipes::find(l.reg, &input) else { return };
    let result = recipes::assemble(l.reg, recipe, &input);
    if result.count <= 0 {
        return;
    }
    if let Container { kind: Kind::Crafter { crafting_ticks_remaining, .. }, .. } = container::be_mut(l, pos) {
        *crafting_ticks_remaining = 6;
    }
    l.set(pos, l.reg.with_bool(state, crafting(l), true), UPDATE_CLIENTS);
    // ItemStack.onCraftedBySystem: MapItem creates the map's saved data.
    if l.reg.items[result.item as usize].key == "minecraft:filled_map" {
        crate::level::refuse("a crafter's result that is a filled map (its saved data)");
    }
    dispense_item(l, pos, state, result);
    for remaining in recipes::remaining_items(l.reg, recipe, &input) {
        if remaining.count > 0 {
            dispense_item(l, pos, state, remaining);
        }
    }
    for stack in &stacks {
        if !is_empty(stack) {
            container::shrink(stack, 1);
        }
    }
    container::be_changed(l, pos);
}

// CrafterBlock.ORIENTATION's front: the part of the value before the first
// underscore (down_east, west_up).
fn front(l: &Level, state: State) -> Dir {
    let property = l.reg.property_of(state, "orientation");
    let name = l.reg.value_name(state, property);
    Dir::by_name(name.split('_').next().unwrap())
}

// CrafterBlock.dispenseItem: the stack goes into the container in front of the
// crafter (hopper-style, one at a time for a stack too big for it or into
// another crafter), and what does not goes out as an item entity. The
// InventoryMoveItemEvent has no listener.
fn dispense_item(l: &mut Level, pos: Pos, state: State, stack: crate::loot::ItemStack) {
    let direction = front(l, state);
    let container_at = hopper::get_container_at(l, pos.relative(direction));
    let mut item_stack = container::stack_of(stack.clone());
    let source = Cont::Be(pos);
    if let Some(mut container_at) = container_at {
        let into_crafter = matches!(&container_at, Cont::Be(at) if matches!(l.block_entities.get(at), Some(BlockEntity::Container(container::Container { kind: Kind::Crafter { .. }, .. }))));
        if into_crafter || container::count(&item_stack) > container_at.max_stack_for(l, &item_stack) {
            while !is_empty(&item_stack) {
                let one = container::copy_with_count(&item_stack, 1);
                let left = hopper::add_item(l, Some(&source), &mut container_at, one, Some(direction.opposite()));
                if !is_empty(&left) {
                    break;
                }
                container::shrink(&item_stack, 1);
            }
        } else {
            while !is_empty(&item_stack) {
                let count = container::count(&item_stack);
                item_stack = hopper::add_item(l, Some(&source), &mut container_at, item_stack, Some(direction.opposite()));
                if count == container::count(&item_stack) {
                    break;
                }
            }
        }
    }
    if !is_empty(&item_stack) {
        // Vec3.atCenterOf(pos).relative(direction, 0.7).
        let (step_x, step_y, step_z) = direction.offset();
        let position = crate::phys::Vec3::new(pos.x() as f64 + 0.5 + 0.7 * step_x as f64, pos.y() as f64 + 0.5 + 0.7 * step_y as f64, pos.z() as f64 + 0.5 + 0.7 * step_z as f64);
        let spawned = item_stack.borrow().clone();
        crate::entity::item::spawn_dispensed(l, spawned, 6, direction, position);
    }
}

// CrafterBlockEntity.isSlotDisabled.
pub fn is_slot_disabled(c: &Container, slot: usize) -> bool {
    matches!(&c.kind, Kind::Crafter { disabled, .. } if slot < 9 && disabled[slot])
}

// CrafterBlockEntity.canPlaceItem.
pub fn can_place_item(l: &Level, c: &Container, slot: usize) -> bool {
    if is_slot_disabled(c, slot) {
        return false;
    }
    let item = &c.items[slot];
    let n = count(item);
    n < max_stack(l.reg, item) && (is_empty(item) || !smaller_stack_exist(c, n, item, slot))
}

// CrafterBlockEntity.smallerStackExist.
fn smaller_stack_exist(c: &Container, current_size: i32, stack: &container::Stack, slot: usize) -> bool {
    for i in slot + 1..9 {
        if is_slot_disabled(c, i) {
            continue;
        }
        let item = &c.items[i];
        if is_empty(item) || count(item) < current_size && same_item(item, stack) {
            return true;
        }
    }
    false
}

// CrafterBlock.newBlockEntity: CrafterBlockEntity, not crafting, triggered
// as the state is.
fn new_block_entity(l: &mut Level, state: State, _pos: Pos) -> BlockEntity {
    BlockEntity::Container(Container::new(Kind::Crafter { crafting_ticks_remaining: 0, triggered: l.reg.get_bool(state, triggered(l)) as i32, disabled: [false; 9] }))
}

// CrafterBlock.getTicker: CrafterBlockEntity.serverTick.
fn ticker(_l: &Level, _state: State) -> Option<fn(&mut Level, Pos, State)> {
    Some(server_tick)
}

fn server_tick(l: &mut Level, pos: Pos, state: State) {
    let Some(BlockEntity::Container(Container { kind: Kind::Crafter { crafting_ticks_remaining, .. }, .. })) = l.block_entities.get_mut(&pos) else {
        panic!("a crafter ticks without its block entity")
    };
    let remaining = *crafting_ticks_remaining - 1;
    if remaining >= 0 {
        *crafting_ticks_remaining = remaining;
        if remaining == 0 {
            l.set(pos, l.reg.with_bool(state, crafting(l), false), UPDATE_ALL);
        }
    }
}
