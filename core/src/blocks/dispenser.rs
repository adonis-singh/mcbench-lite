// DispenserBlock and DispenserBlockEntity, which DropperBlock extends
// (dropper.rs). Dispensing an item is DefaultDispenseItemBehavior: the item
// spawns as an item entity in front of the dispenser. An item with another
// DispenseItemBehavior (a projectile, a bucket, a boat, armour...) refuses.
// No plugin listens to BlockPreDispenseEvent, BlockDispenseEvent and
// BlockFailedDispenseEvent, and a dispense's level events and game events
// reach nothing here.

use super::container::{self, be, is_empty, Cont, Container, Kind, Stack};
use super::{BlockEntity, Behaviour, NONE};
use crate::dir::{Dir, Pos};
use crate::entity;
use crate::level::{refuse, Level, PRIORITY_NORMAL, UPDATE_CLIENTS};
use crate::loot::ItemStack;
use crate::phys::Vec3;
use crate::property;
use crate::registry::{BlockId, State};

pub const BEHAVIOUR: Behaviour = Behaviour {
    affect_neighbors_after_removal: Some(container::affect_neighbors_after_removal),
    neighbor_changed: Some(neighbor_changed),
    tick: Some(tick),
    get_analog_output_signal: Some(container::analog_output_signal),
    trigger_event: Some(container::trigger_event),
    new_block_entity: Some(new_block_entity),
    ..NONE
};

// DispenserBlock.TRIGGERED.
fn triggered(l: &Level) -> u16 {
    property!(l, "triggered", ["true", "false"])
}

// DispenserBlock.neighborChanged.
pub fn neighbor_changed(l: &mut Level, state: State, pos: Pos, _neighbor: BlockId, _moved: bool) {
    let signal = l.has_neighbor_signal(pos) || l.has_neighbor_signal(pos.above());
    let triggered_value = l.reg.get_bool(state, triggered(l));
    if signal && !triggered_value {
        l.schedule_tick(pos, l.block(state), 4, PRIORITY_NORMAL);
        l.set(pos, l.reg.with_bool(state, triggered(l), true), UPDATE_CLIENTS);
    } else if !signal && triggered_value {
        l.set(pos, l.reg.with_bool(state, triggered(l), false), UPDATE_CLIENTS);
    }
}

// DispenserBlock.tick.
fn tick(l: &mut Level, state: State, pos: Pos) {
    dispense_from(l, state, pos);
}

// DispenserBlock.FACING.
pub fn facing(l: &Level, state: State) -> Dir {
    l.reg.get_dir(state, l.reg.property_of(state, "facing"))
}

// Level.getBlockEntity(pos, type): the block entity (made if missing) if it
// is of the dispenser or dropper type; without it the dispense is ignored
// with a log.
pub fn dispenser_entity(l: &mut Level, pos: Pos, dropper: bool) -> bool {
    match l.block_entity(pos) {
        Some(BlockEntity::Container(Container { kind: Kind::Dispenser, .. })) => !dropper,
        Some(BlockEntity::Container(Container { kind: Kind::Dropper, .. })) => dropper,
        Some(_) | None => false,
    }
}

// DispenserBlockEntity.getRandomSlot: a slot drawn by reservoir sampling
// from the level's random over the stacks there are.
pub fn get_random_slot(l: &mut Level, pos: Pos) -> i32 {
    let mut i = -1;
    let mut i1 = 1;
    let size = be(l, pos).items.len();
    for slot in 0..size {
        if !is_empty(&be(l, pos).items[slot].clone()) {
            let bound = i1;
            i1 += 1;
            if l.random.next_int_bound(bound) == 0 {
                i = slot as i32;
            }
        }
    }
    i
}

// DispenserBlock.dispenseFrom.
pub fn dispense_from(l: &mut Level, state: State, pos: Pos) {
    if !dispenser_entity(l, pos, false) {
        return;
    }
    let random_slot = get_random_slot(l, pos);
    if random_slot < 0 {
        // The failed dispense event is not cancelled: its level event
        // reaches nothing, and the click is a game event.
        let event = crate::game_event!(l, "block_activate");
        l.game_event_in(event, pos, crate::game_event::Context::of_state(state));
        return;
    }
    let item = Cont::Be(pos).get(l, random_slot as usize);
    let result = super::dispense::dispense_method(l, state, pos, &item);
    Cont::Be(pos).set_item(l, random_slot as usize, result);
}

// DefaultDispenseItemBehavior.dispense: execute, then a sound and an
// animation (level events). The state is the one the dispenser's tick got.
pub fn dispense(l: &mut Level, state: State, pos: Pos, item: &Stack) -> Stack {
    let direction = facing(l, state);
    // execute: DispenserBlock.getDispensePosition(blockSource), the center
    // 0.7 in front of the dispenser.
    let (step_x, step_y, step_z) = direction.offset();
    let center = Vec3::new(pos.x() as f64 + 0.5, pos.y() as f64 + 0.5, pos.z() as f64 + 0.5);
    let position = center.add(0.7 * step_x as f64 + 0.0, 0.7 * step_y as f64 + 0.0, 0.7 * step_z as f64 + 0.0);
    let item_stack = container::split(item, 1);
    // spawnItem: an empty stack counts as spawned, without an entity.
    if !is_empty(&item_stack) {
        let spawned = item_stack.borrow().clone();
        entity::item::spawn_dispensed(l, spawned, 6, direction, position);
    }
    item.clone()
}

// DispenserBlock.newBlockEntity.
fn new_block_entity(_l: &mut Level, _state: State, _pos: Pos) -> BlockEntity {
    BlockEntity::Container(Container::new(Kind::Dispenser))
}
