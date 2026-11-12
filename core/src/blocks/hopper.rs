// HopperBlock and HopperBlockEntity, with Paper's hopper code and its
// world configuration at the defaults (spigot.yml's hopper-check 1,
// hopper-transfer 8, hopper-amount 1, hopper-can-load-chunks false; Paper's
// hopper.cooldown-when-full true, disable-move-event false and
// ignore-occluding-blocks false). No plugin listens to the events a transfer
// calls (InventoryMoveItemEvent, InventoryPickupItemEvent,
// HopperInventorySearchEvent, EntityInsideBlockEvent): they leave the
// transfer as it was, and the code behind a listener-less event is not
// ported.
//
// A hopper without a block container at a position searches the container
// entities there (minecarts with a chest or a hopper): one of them, picked
// with the level's random, is the container.

use super::chest;
use super::container::{self, be, be_mut, copy, copy_original, count, is_empty, same_item, Cont, Container, Kind, Stack};
use super::{BlockEntity, Behaviour, NONE};
use crate::dir::{Dir, Pos};
use crate::entity::{self, Entity};
use crate::level::{Level, UPDATE_CLIENTS};
use crate::loot::ItemStack;
use crate::phys::{self, Aabb};
use crate::property;
use crate::registry::{BlockId, State};

pub const BEHAVIOUR: Behaviour = Behaviour {
    on_place: Some(on_place),
    neighbor_changed: Some(neighbor_changed),
    affect_neighbors_after_removal: Some(container::affect_neighbors_after_removal),
    get_analog_output_signal: Some(container::analog_output_signal),
    trigger_event: Some(container::trigger_event),
    new_block_entity: Some(new_block_entity),
    ticker: Some(ticker),
    entity_inside: Some(entity_inside),
    all_entities: true,
    ..NONE
};

// Spigot's hopper-transfer, hopper-check and hopper-amount.
const HOPPER_TRANSFER: i32 = 8;
const HOPPER_AMOUNT: i32 = 1;

// HopperBlock.ENABLED and FACING.
fn enabled(l: &Level) -> u16 {
    property!(l, "enabled", ["true", "false"])
}

fn facing(l: &Level) -> u16 {
    property!(l, "facing", ["down", "north", "south", "west", "east"])
}

// HopperBlock.onPlace.
fn on_place(l: &mut Level, state: State, pos: Pos, old: State, _moved: bool) {
    if l.block(old) != l.block(state) {
        check_powered_state(l, pos, state);
    }
}

// HopperBlock.neighborChanged.
fn neighbor_changed(l: &mut Level, state: State, pos: Pos, _neighbor: BlockId, _moved: bool) {
    check_powered_state(l, pos, state);
}

// HopperBlock.checkPoweredState.
fn check_powered_state(l: &mut Level, pos: Pos, state: State) {
    let flag = !l.has_neighbor_signal(pos);
    if flag != l.reg.get_bool(state, enabled(l)) {
        l.set(pos, l.reg.with_bool(state, enabled(l), flag), UPDATE_CLIENTS);
    }
}

// HopperBlock.newBlockEntity: HopperBlockEntity, not on cooldown (-1).
fn new_block_entity(_l: &mut Level, _state: State, _pos: Pos) -> BlockEntity {
    BlockEntity::Container(Container::new(Kind::Hopper { cooldown_time: -1, ticked_game_time: 0 }))
}

// HopperBlock.getTicker: HopperBlockEntity.pushItemsTick.
fn ticker(_l: &Level, _state: State) -> Option<fn(&mut Level, Pos, State)> {
    Some(push_items_tick)
}

// HopperBlock.entityInside: EntityInsideBlockEvent (no listener, so not
// cancelled), then the block entity's.
fn entity_inside(l: &mut Level, state: State, pos: Pos, e: &mut Entity, _applier: &mut crate::entity::inside::Applier, _past_edges: bool) {
    if !matches!(l.block_entity(pos), Some(BlockEntity::Container(Container { kind: Kind::Hopper { .. }, .. }))) {
        return;
    }
    // HopperBlockEntity.entityInside.
    let entity::Kind::Item(data) = &e.kind else { return };
    if data.stack.count > 0 && e.bb.mv(-(pos.x() as f64), -(pos.y() as f64), -(pos.z() as f64)).intersects(&suck_aabb()) {
        try_move_items(l, pos, state, Validator::Entity(e));
    }
}

// Whether the container block entity at the position is a hopper.
fn is_hopper(l: &Level, pos: Pos) -> bool {
    matches!(l.block_entities.get(&pos), Some(BlockEntity::Container(Container { kind: Kind::Hopper { .. }, .. })))
}

fn cooldown_time(l: &Level, pos: Pos) -> i32 {
    match be(l, pos).kind {
        Kind::Hopper { cooldown_time, .. } => cooldown_time,
        _ => panic!("not a hopper"),
    }
}

fn set_cooldown(l: &mut Level, pos: Pos, value: i32) {
    match &mut be_mut(l, pos).kind {
        Kind::Hopper { cooldown_time, .. } => *cooldown_time = value,
        _ => panic!("not a hopper"),
    }
}

fn ticked_game_time(l: &Level, pos: Pos) -> i64 {
    match be(l, pos).kind {
        Kind::Hopper { ticked_game_time, .. } => ticked_game_time,
        _ => panic!("not a hopper"),
    }
}

// HopperBlockEntity.isOnCooldown and isOnCustomCooldown.
fn is_on_cooldown(l: &Level, pos: Pos) -> bool {
    cooldown_time(l, pos) > 0
}

fn is_on_custom_cooldown(l: &Level, pos: Pos) -> bool {
    cooldown_time(l, pos) > 8
}

// Hopper.SUCK_AABB: Block.column(16.0, 11.0, 32.0).
fn suck_aabb() -> Aabb {
    Aabb::new(0.0, 11.0 / 16.0, 0.0, 1.0, 32.0 / 16.0, 1.0)
}

// HopperBlockEntity.pushItemsTick.
fn push_items_tick(l: &mut Level, pos: Pos, state: State) {
    let time = l.game_time;
    match &mut be_mut(l, pos).kind {
        Kind::Hopper { cooldown_time, ticked_game_time } => {
            *cooldown_time = cooldown_time.wrapping_sub(1);
            *ticked_game_time = time;
        }
        _ => panic!("a hopper ticks without its block entity"),
    }
    if !is_on_cooldown(l, pos) {
        set_cooldown(l, pos, 0);
        // The hopper moved nothing, which is a cooldown of hopperCheck (1)
        // only if hopperCheck > 1.
        try_move_items(l, pos, state, Validator::SuckIn);
    }
}

// What tryMoveItems runs after ejecting: suckInItems for the ticker, adding
// an item entity for the entity that is inside.
enum Validator<'a> {
    SuckIn,
    Entity(&'a mut Entity),
}

// HopperBlockEntity.getFullState: 0 empty, 1 has items, 2 full.
fn get_full_state(l: &Level, pos: Pos) -> i32 {
    let items = &be(l, pos).items;
    let mut empty = true;
    let mut full = true;
    for stack in items {
        if is_empty(stack) {
            full = false;
        } else {
            if !full {
                return 1;
            }
            empty = false;
            if count(stack) != container::max_stack(l.reg, stack) {
                return 1;
            }
        }
    }
    if empty {
        0
    } else if full {
        2
    } else {
        1
    }
}

// HopperBlockEntity.tryMoveItems.
fn try_move_items(l: &mut Level, pos: Pos, state: State, validator: Validator) -> bool {
    if !is_on_cooldown(l, pos) && l.reg.get_bool(state, enabled(l)) {
        let mut flag = false;
        let full_state = get_full_state(l, pos);
        if full_state != 0 {
            let me = match &validator {
                Validator::Entity(e) => Some(&**e),
                Validator::SuckIn => None,
            };
            flag = eject_items(l, pos, me);
        }
        if full_state != 2 || flag {
            let result = match validator {
                Validator::SuckIn => suck_in_items(l, pos),
                Validator::Entity(e) => add_item_entity(l, &mut Cont::Be(pos), e),
            };
            flag |= result;
        }
        if flag {
            set_cooldown(l, pos, HOPPER_TRANSFER);
            container::set_changed_with_state(l, pos, state);
            return true;
        }
    }
    false
}

// HopperBlockEntity.hopperPush.
fn hopper_push(l: &mut Level, destination: &mut Cont, direction: Dir, hopper: Pos) -> bool {
    let mut found_item = false;
    let hopper_size = be(l, hopper).items.len();
    for i in 0..hopper_size {
        let item = be(l, hopper).items[i].clone();
        if !is_empty(&item) {
            found_item = true;
            let original_item_count = count(&item);
            let moved_item_count = HOPPER_AMOUNT.min(original_item_count);
            container::set_count(&item, moved_item_count);
            // callPushMoveEvent: no listener, the item moves as it is.
            let remaining_item = add_item(l, Some(&Cont::Be(hopper)), destination, item.clone(), Some(direction));
            let remaining_item_count = count(&remaining_item);
            if remaining_item_count != moved_item_count {
                let orig_item_stack = copy_original(&item);
                container::set_count(&orig_item_stack, original_item_count);
                if !is_empty(&orig_item_stack) {
                    container::set_count(&orig_item_stack, original_item_count - moved_item_count + remaining_item_count);
                }
                Cont::Be(hopper).set_item(l, i, orig_item_stack);
                destination.set_changed(l);
                return true;
            }
            container::set_count(&item, original_item_count);
        }
    }
    if found_item {
        // Paper's hopper.cooldown-when-full.
        set_cooldown(l, hopper, HOPPER_TRANSFER);
    }
    false
}

// HopperBlockEntity.hopperPull.
fn hopper_pull(l: &mut Level, hopper: Hop, container: &mut Cont, orig_item_stack: Stack, i: usize) -> bool {
    let moved_item = orig_item_stack.clone();
    let original_item_count = count(&orig_item_stack);
    let moved_item_count = HOPPER_AMOUNT.min(original_item_count);
    container.set_changed(l);
    container::set_count(&orig_item_stack, moved_item_count);
    // callPullMoveEvent: no listener, the item moves as it is.
    let remaining_item = add_item(l, Some(&*container), &mut hopper.cont(), moved_item, None);
    let remaining_item_count = count(&remaining_item);
    if remaining_item_count != moved_item_count {
        let orig_item_stack = copy_original(&orig_item_stack);
        container::set_count(&orig_item_stack, original_item_count);
        if !is_empty(&orig_item_stack) {
            container::set_count(&orig_item_stack, original_item_count - moved_item_count + remaining_item_count);
        }
        l.ignore_block_entity_updates = true;
        container.set_item(l, i, orig_item_stack);
        l.ignore_block_entity_updates = false;
        container.set_changed(l);
        true
    } else {
        container::set_count(&orig_item_stack, original_item_count);
        // Paper's hopper.cooldown-when-full: applyCooldown(hopper), which only a
        // hopper block entity has.
        if let Hop::Block(pos) = hopper {
            set_cooldown(l, pos, HOPPER_TRANSFER);
        }
        false
    }
}

// HopperBlockEntity.getSlots: the face's slots of a WorldlyContainer, else
// every slot.
fn get_slots(l: &Level, container: &Cont, direction: Dir) -> Vec<usize> {
    container.slots_for_face(l, direction).unwrap_or_else(|| (0..container.size(l)).collect())
}

// HopperBlockEntity.isFullContainer.
fn is_full_container(l: &Level, container: &Cont, direction: Dir) -> bool {
    for i in get_slots(l, container, direction) {
        let item = container.get(l, i);
        if count(&item) < container::max_stack(l.reg, &item) {
            return false;
        }
    }
    true
}

// HopperBlockEntity.ejectItems.
fn eject_items(l: &mut Level, pos: Pos, me: Option<&Entity>) -> bool {
    // getAttachedContainer: the container at the position the hopper faces
    // (the block entity's facing is its block's).
    let state = l.get(pos);
    let hopper_facing = l.reg.get_dir(state, facing(l));
    let Some(mut attached) = get_container_at_for(l, pos.relative(hopper_facing), me) else { return false };
    let opposite = hopper_facing.opposite();
    if is_full_container(l, &attached, opposite) {
        false
    } else {
        hopper_push(l, &mut attached, opposite, pos)
    }
}

// The hopper a transfer moves into: a hopper block, or a hopper minecart.
#[derive(Clone, Copy)]
pub enum Hop {
    Block(Pos),
    Cart(crate::entity::EntityId),
}

impl Hop {
    fn cont(self) -> Cont {
        match self {
            Hop::Block(pos) => Cont::Be(pos),
            Hop::Cart(id) => Cont::Entity(id),
        }
    }
}

// HopperBlockEntity.suckInItems.
fn suck_in_items(l: &mut Level, pos: Pos) -> bool {
    suck_in_items_for(l, Hop::Block(pos), pos.x() as f64 + 0.5, pos.y() as f64 + 0.5, pos.z() as f64 + 0.5, true, None)
}

// HopperBlockEntity.suckInItems(level, hopper) for a hopper at a level
// position (getLevelX/Y/Z); `me` is the entity being ticked, if the hopper is
// a minecart.
fn suck_in_items_for(l: &mut Level, hopper: Hop, lx: f64, ly: f64, lz: f64, grid_aligned: bool, me: Option<&Entity>) -> bool {
    let above = Pos::new(phys::floor(lx), phys::floor(ly + 1.0), phys::floor(lz));
    let block_state = l.get(above);
    // getSourceContainer: the container at the position.
    let source = get_container_at_with(l, above, block_state, lx, ly + 1.0, lz, false, me);
    if let Some(mut source) = source {
        let direction = Dir::Down;
        for i in get_slots(l, &source, direction) {
            if try_take_in_item_from_slot(l, hopper, &mut source, i, direction) {
                return true;
            }
        }
        false
    } else {
        // isGridAligned and an isCollisionShapeFullBlock state that does not
        // let hoppers through.
        let flag = grid_aligned && crate::shape_ops::is_collision_shape_full_block(l, block_state, above) && !l.reg.in_tag(block_state, "minecraft:does_not_block_hoppers");
        if !flag {
            for id in items_at_and_above(l, lx, ly, lz, me) {
                // The entity may have been taken from the level since the
                // search (by the transfers before it).
                let Some(mut e) = l.entities.all.remove(&id) else { crate::level::refuse("an item entity left the level during a hopper's pickup") };
                let picked = add_item_entity(l, &mut hopper.cont(), &mut e);
                if !e.removed {
                    l.entities.all.insert(id, e);
                }
                if picked {
                    return true;
                }
            }
        }
        false
    }
}

// MinecartHopper's HopperBlockEntity.suckInItems(level, this).
pub fn suck_in_items_cart(l: &mut Level, cart: &mut Entity) -> bool {
    let (x, y, z) = (cart.pos.x, cart.pos.y + 0.5, cart.pos.z);
    suck_in_items_for(l, Hop::Cart(cart.id), x, y, z, false, Some(&*cart))
}

// HopperBlockEntity.addItem(hopper, itemEntity) for a hopper minecart.
pub fn add_item_entity_to_cart(l: &mut Level, cart: &Entity, e: &mut Entity) -> bool {
    add_item_entity(l, &mut Cont::Entity(cart.id), e)
}

// HopperBlockEntity.tryTakeInItemFromSlot.
fn try_take_in_item_from_slot(l: &mut Level, hopper: Hop, container: &mut Cont, slot: usize, direction: Dir) -> bool {
    let item = container.get(l, slot);
    if !is_empty(&item) && can_take_item_from_container(l, &hopper.cont(), container, &item, slot, direction) {
        hopper_pull(l, hopper, container, item, slot)
    } else {
        false
    }
}

// HopperBlockEntity.getItemsAtAndAbove: the item entities alive in the
// box above the hopper.
fn items_at_and_above(l: &mut Level, lx: f64, ly: f64, lz: f64, me: Option<&Entity>) -> Vec<entity::EntityId> {
    let aabb = suck_aabb().mv(lx - 0.5, ly - 0.5, lz - 0.5);
    entity::item_entities_in(l, me, &aabb, |e| !e.removed)
}

// HopperBlockEntity.addItem(Container, ItemEntity): the entity's stack goes
// in as far as it fits; an entity whose stack went in whole is picked up.
fn add_item_entity(l: &mut Level, container: &mut Cont, e: &mut Entity) -> bool {
    let entity::Kind::Item(data) = &mut e.kind else { panic!("a hopper picks up an item entity") };
    // InventoryPickupItemEvent has no listener.
    let item_stack = container::stack_of(data.stack.clone());
    let item_stack = if is_empty(&item_stack) { container::empty_stack() } else { item_stack };
    let remaining = add_item(l, None, container, item_stack, None);
    if is_empty(&remaining) {
        entity::item::set_item(e, ItemStack::empty());
        entity::item::discard(l, e);
        true
    } else {
        let r = remaining.borrow();
        entity::item::set_item(e, r.clone());
        false
    }
}

// HopperBlockEntity.addItem(source, destination, stack, direction).
pub fn add_item(l: &mut Level, source: Option<&Cont>, destination: &mut Cont, mut stack: Stack, direction: Option<Dir>) -> Stack {
    if let (Some(direction), true) = (direction, destination.is_worldly(l)) {
        let slots = destination.slots_for_face(l, direction).unwrap();
        for slot in slots {
            if is_empty(&stack) {
                break;
            }
            stack = try_move_in_item(l, source, destination, stack, slot, Some(direction));
        }
        return stack;
    }
    let size = destination.size(l);
    for i in 0..size {
        if is_empty(&stack) {
            break;
        }
        stack = try_move_in_item(l, source, destination, stack, i, direction);
    }
    stack
}

// HopperBlockEntity.canPlaceItemInContainer.
fn can_place_item_in_container(l: &Level, container: &Cont, stack: &Stack, slot: usize, direction: Option<Dir>) -> bool {
    container.can_place_item(l, slot, stack) && (!container.is_worldly(l) || container.can_place_item_through_face(l, slot, stack, direction))
}

// HopperBlockEntity.canTakeItemFromContainer: the container's canTakeItem
// for the hopper, and the sided check of a WorldlyContainer.
fn can_take_item_from_container(l: &Level, hopper: &Cont, container: &Cont, stack: &Stack, slot: usize, direction: Dir) -> bool {
    container.can_take_item(l, hopper, slot, stack) && (!container.is_worldly(l) || container.can_take_item_through_face(l, slot, stack, direction))
}

// HopperBlockEntity.canMergeItems.
fn can_merge_items(l: &Level, stack1: &Stack, stack2: &Stack) -> bool {
    count(stack1) < container::max_stack(l.reg, stack1) && same_item(stack1, stack2)
}

// HopperBlockEntity.tryMoveInItem.
fn try_move_in_item(l: &mut Level, source: Option<&Cont>, destination: &mut Cont, mut stack: Stack, slot: usize, direction: Option<Dir>) -> Stack {
    let item = destination.get(l, slot);
    if can_place_item_in_container(l, destination, &stack, slot, direction) {
        let mut flag = false;
        let is_empty_destination = destination.is_empty(l);
        if is_empty(&item) {
            let mut leftover = container::empty_stack();
            if !is_empty(&stack) && count(&stack) > destination.max_stack(l) {
                leftover = stack.clone();
                stack = container::split(&stack, destination.max_stack(l));
            }
            l.ignore_block_entity_updates = true;
            destination.set_item(l, slot, stack);
            l.ignore_block_entity_updates = false;
            stack = leftover;
            flag = true;
        } else if can_merge_items(l, &item, &stack) {
            let i = container::max_stack(l.reg, &stack).min(destination.max_stack(l)) - count(&item);
            let min = count(&stack).min(i);
            container::shrink(&stack, min);
            container::grow(&item, min);
            flag = min > 0;
        }
        if flag {
            if let Cont::Be(dest_pos) = destination {
                let dest_pos = *dest_pos;
                if is_empty_destination && is_hopper(l, dest_pos) && !is_on_custom_cooldown(l, dest_pos) {
                    let mut min = 0;
                    if let Some(Cont::Be(source_pos)) = source {
                        if is_hopper(l, *source_pos) && ticked_game_time(l, dest_pos) >= ticked_game_time(l, *source_pos) {
                            min = 1;
                        }
                    }
                    set_cooldown(l, dest_pos, HOPPER_TRANSFER - min);
                }
            }
            destination.set_changed(l);
        }
    }
    stack
}

// HopperBlockEntity.getContainerAt(level, pos): the block container at the
// position, else a container entity there.
pub fn get_container_at(l: &mut Level, pos: Pos) -> Option<Cont> {
    get_container_at_for(l, pos, None)
}

// The same, for a search made while `me` is being ticked.
fn get_container_at_for(l: &mut Level, pos: Pos, me: Option<&Entity>) -> Option<Cont> {
    let state = l.get(pos);
    get_container_at_with(l, pos, state, pos.x() as f64 + 0.5, pos.y() as f64 + 0.5, pos.z() as f64 + 0.5, true, me)
}

// HopperBlockEntity.getContainerAt(level, pos, state, x, y, z, optimizeEntities):
// the block's container, else (Paper's hopper.ignore-occluding-blocks is off)
// one of the container entities around (x, y, z).
fn get_container_at_with(l: &mut Level, pos: Pos, state: State, x: f64, y: f64, z: f64, _optimize_entities: bool, me: Option<&Entity>) -> Option<Cont> {
    match get_block_container(l, pos, state) {
        Some(container) => Some(container),
        None => get_entity_container(l, x, y, z, me),
    }
}

// HopperBlockEntity.getEntityContainer: the container entities alive in the
// box around the point, one picked by the level's random.
fn get_entity_container(l: &mut Level, x: f64, y: f64, z: f64, me: Option<&Entity>) -> Option<Cont> {
    let aabb = Aabb::new(x - 0.5, y - 0.5, z - 0.5, x + 0.5, y + 0.5, z + 0.5);
    let found = entity::entities_in(l, me, None, entity::Only::Containers, &aabb, |e| !e.removed);
    if found.is_empty() {
        None
    } else {
        let pick = l.random.next_int_bound(found.len() as i32) as usize;
        Some(Cont::Entity(found[pick]))
    }
}

// HopperBlockEntity.getBlockContainer.
fn get_block_container(l: &mut Level, pos: Pos, state: State) -> Option<Cont> {
    // hopper-can-load-chunks is false.
    if !l.has_chunk_at(pos) {
        return None;
    }
    // WorldlyContainerHolder: only the composter.
    if l.reg.blocks[l.block(state) as usize].class == "ComposterBlock" {
        return Some(Cont::Composter(super::composter::get_container(l, state, pos)));
    }
    if l.has_block_entity(state) {
        match l.block_entity(pos) {
            Some(BlockEntity::Container(c)) if c.is_container() => {
                if matches!(c.kind, Kind::Chest | Kind::TrappedChest) {
                    // ChestBlock.getContainer(chest, state, level, pos, true).
                    chest::get_container(l, state, pos, true)
                } else {
                    Some(Cont::Be(pos))
                }
            }
            // The furnaces, the brewing stand, the jukebox, the chiseled
            // bookshelf, the shelf and the decorated pot are containers too
            // (with their own slots and rules).
            Some(BlockEntity::Entity(entity)) if entity.container_size().is_some() => Some(Cont::Be(pos)),
            _ => None,
        }
    } else {
        None
    }
}
