// The container block entities, the item stacks they hold and the Container
// interface hoppers, droppers and comparators use on them (Container,
// WorldlyContainer, CompoundContainer, ComposterBlock's containers).
//
// A stack is an object, as in Java: slots hold references, and code that
// gives a stack to another container or changes a count in place (hopper
// transfers do both) changes what every holder sees. A stack carries the
// patch to its item's components that saved data gave it (components.rs); one
// from a loot table has none.

use super::composter;
use super::shulker_box::ShulkerBox;
use super::entities::Entity;
use super::BlockEntity;
use crate::dir::{Dir, Pos};
use crate::level::{refuse, Level};
use crate::loot::ItemStack;
use crate::nbt::{Compound, Tag};
use crate::registry::{Registry, State};
use std::cell::RefCell;
use std::rc::Rc;

pub type Stack = Rc<RefCell<ItemStack>>;

pub fn new_stack(item: u16, count: i32) -> Stack {
    Rc::new(RefCell::new(ItemStack::new(item, count)))
}

// A stack object holding a stack's value.
pub fn stack_of(stack: ItemStack) -> Stack {
    Rc::new(RefCell::new(stack))
}

// ItemStack.EMPTY (item air, which is item 0).
pub fn empty_stack() -> Stack {
    new_stack(0, 0)
}

pub fn is_empty(stack: &Stack) -> bool {
    stack.borrow().count <= 0
}

pub fn count(stack: &Stack) -> i32 {
    stack.borrow().count
}

// ItemStack.getMaxStackSize.
pub fn max_stack(reg: &Registry, stack: &Stack) -> i32 {
    stack.borrow().max_stack_size(reg)
}

// ItemStack.copy(): an empty stack copies to ItemStack.EMPTY.
pub fn copy(stack: &Stack) -> Stack {
    let s = stack.borrow();
    if s.count <= 0 {
        empty_stack()
    } else {
        stack_of(s.clone())
    }
}

// ItemStack.copy(true): the item kept even when the stack is empty.
pub fn copy_original(stack: &Stack) -> Stack {
    stack_of(stack.borrow().clone())
}

pub fn copy_with_count(stack: &Stack, count: i32) -> Stack {
    let copy = copy(stack);
    if !is_empty(&copy) {
        copy.borrow_mut().count = count;
    }
    copy
}

pub fn set_count(stack: &Stack, count: i32) {
    stack.borrow_mut().count = count;
}

pub fn grow(stack: &Stack, n: i32) {
    stack.borrow_mut().count += n;
}

pub fn shrink(stack: &Stack, n: i32) {
    stack.borrow_mut().count -= n;
}

// ItemStack.split(amount).
pub fn split(stack: &Stack, amount: i32) -> Stack {
    let min = amount.min(count(stack));
    let split = copy_with_count(stack, min);
    shrink(stack, min);
    split
}

// ItemStack.limitSize.
pub fn limit_size(stack: &Stack, max: i32) {
    if !is_empty(stack) && count(stack) > max {
        set_count(stack, max);
    }
}

// ItemStack.isSameItemSameComponents.
pub fn same_item(a: &Stack, b: &Stack) -> bool {
    let (a, b) = (a.borrow(), b.borrow());
    same_item_same_components(&a, &b)
}

// ItemStack.isSameItemSameComponents of two stacks: the same item (an empty
// stack's is air), and for two that are not empty the same patch.
pub fn same_item_same_components(a: &ItemStack, b: &ItemStack) -> bool {
    let (a_item, b_item) = (if a.count <= 0 { 0 } else { a.item }, if b.count <= 0 { 0 } else { b.item });
    a_item == b_item && (a.count <= 0 && b.count <= 0 || a.patch == b.patch)
}

// A list of empty slots.
pub fn empty_slots_n(n: usize) -> Vec<Stack> {
    (0..n).map(|_| empty_stack()).collect()
}

// The empty slots of a container of this kind.
pub fn empty_slots(kind: &Kind) -> Vec<Stack> {
    let size = match kind {
        Kind::Chest | Kind::TrappedChest | Kind::Barrel | Kind::ShulkerBox(_) => 27,
        Kind::Hopper { .. } => 5,
        Kind::Dispenser | Kind::Dropper | Kind::Crafter { .. } => 9,
        Kind::EnderChest => 0,
    };
    (0..size).map(|_| empty_stack()).collect()
}

// HopperBlockEntity's maxStack: every container's default (Container.MAX_STACK).
pub const MAX_STACK: i32 = 99;

#[derive(Clone, Debug)]
pub enum Kind {
    // ChestBlockEntity, 27 slots. Its ContainerOpenersCounter.openCount is
    // 0: only a player opening the chest raises it.
    Chest,
    // TrappedChestBlockEntity.
    TrappedChest,
    // EnderChestBlockEntity: a BlockEntity, not a Container.
    EnderChest,
    // BarrelBlockEntity, 27 slots.
    Barrel,
    // ShulkerBoxBlockEntity, 27 slots.
    ShulkerBox(ShulkerBox),
    // HopperBlockEntity, 5 slots: its cooldownTime and tickedGameTime. Its
    // facing is the block's.
    Hopper { cooldown_time: i32, ticked_game_time: i64 },
    // DispenserBlockEntity, 9 slots.
    Dispenser,
    // DropperBlockEntity, 9 slots.
    Dropper,
    // CrafterBlockEntity, 9 slots: its craftingTicksRemaining, the triggered
    // value of its container data (1 when triggered, the saved value
    // otherwise) and which slots are disabled.
    Crafter { crafting_ticks_remaining: i32, triggered: i32, disabled: [bool; 9] },
}

#[derive(Clone, Debug)]
pub struct Container {
    pub kind: Kind,
    // The slots; empty for the ender chest, which has none.
    pub items: Vec<Stack>,
    // BaseContainerBlockEntity.name (CustomName, as the tag its codec writes).
    pub name: Option<Tag>,
    // BaseContainerBlockEntity.lockKey (the lock field, as the tag its codec
    // writes; none is LockCode.NO_LOCK).
    pub lock: Option<Tag>,
}

impl Container {
    pub fn new(kind: Kind) -> Container {
        Container { items: empty_slots(&kind), kind, name: None, lock: None }
    }

    // The class of the one kind of block the entity's type is valid for
    // (BlockEntityType.isValid).
    fn block_classes(&self) -> &'static [&'static str] {
        match self.kind {
            // BlockEntityType.CHEST is valid for the chest and the copper chests.
            Kind::Chest => &["ChestBlock", "CopperChestBlock", "WeatheringCopperChestBlock"],
            Kind::TrappedChest => &["TrappedChestBlock"],
            Kind::EnderChest => &["EnderChestBlock"],
            Kind::Barrel => &["BarrelBlock"],
            Kind::ShulkerBox(_) => &["ShulkerBoxBlock"],
            Kind::Hopper { .. } => &["HopperBlock"],
            Kind::Dispenser => &["DispenserBlock"],
            Kind::Dropper => &["DropperBlock"],
            Kind::Crafter { .. } => &["CrafterBlock"],
        }
    }

    // Whether the entity is a Container (the ender chest is not).
    pub fn is_container(&self) -> bool {
        !matches!(self.kind, Kind::EnderChest)
    }
}

// ContainerHelper.saveAllItems: the non-empty slots as ItemStackWithSlot
// writes them (Slot as a byte, the stack's count, id and component patch).
// A list with no stack is kept unless allow_empty is false.
pub fn save_all_items(reg: &Registry, items: &[Stack], allow_empty: bool, out: &mut Compound) {
    let mut list = Vec::new();
    for (slot, stack) in items.iter().enumerate() {
        if is_empty(stack) {
            continue;
        }
        let mut entry = crate::components::stack_fields(reg, &stack.borrow()).unwrap_or_else(|fail| crate::level::refuse(format!("saving a stack: {}", fail.0)));
        entry.put("Slot", Tag::Byte(slot as u8 as i8));
        list.push(Tag::Compound(entry));
    }
    if !list.is_empty() || allow_empty {
        out.put("Items", Tag::List(list));
    }
}

// A container's type key and saved data (see BlockEntity::saved), as
// BlockEntity.saveWithoutMetadata writes it: Items lists the non-empty
// slots (ContainerHelper.saveAllItems; a shulker box leaves an empty list
// out) and a stack saves its Slot, count, id and component patch.
pub fn saved(reg: &Registry, container: &Container) -> (&'static str, Compound) {
    let mut out = Compound::new();
    if let Some(name) = &container.name {
        out.put("CustomName", name.clone());
    }
    if let Some(lock) = &container.lock {
        out.put("lock", lock.clone());
    }
    let key = match &container.kind {
        Kind::Chest => "minecraft:chest",
        Kind::TrappedChest => "minecraft:trapped_chest",
        Kind::EnderChest => "minecraft:ender_chest",
        Kind::Barrel => "minecraft:barrel",
        Kind::ShulkerBox(_) => "minecraft:shulker_box",
        Kind::Hopper { .. } => "minecraft:hopper",
        Kind::Dispenser => "minecraft:dispenser",
        Kind::Dropper => "minecraft:dropper",
        Kind::Crafter { .. } => "minecraft:crafter",
    };
    match &container.kind {
        Kind::EnderChest => {}
        Kind::ShulkerBox(_) => save_all_items(reg, &container.items, false, &mut out),
        Kind::Hopper { cooldown_time, .. } => {
            save_all_items(reg, &container.items, true, &mut out);
            out.put_int("TransferCooldown", *cooldown_time);
        }
        Kind::Crafter { crafting_ticks_remaining, triggered, disabled } => {
            out.put_int("crafting_ticks_remaining", *crafting_ticks_remaining);
            save_all_items(reg, &container.items, true, &mut out);
            out.put("disabled_slots", Tag::IntArray((0..9).filter(|i| disabled[*i as usize]).collect()));
            out.put_int("triggered", *triggered);
        }
        _ => save_all_items(reg, &container.items, true, &mut out),
    }
    out.put("components", Tag::Compound(Compound::new()));
    (key, out)
}

// BlockEntityType.isValid for a container's type.
pub fn valid(l: &Level, container: &Container, state: State) -> bool {
    container.block_classes().contains(&l.reg.blocks[l.block(state) as usize].class.as_str())
}

// BlockEntity.preRemoveSideEffects: a Container's contents drop
// (Containers.dropContents). The shulker box overrides it to do nothing,
// and the ender chest is no Container.
pub fn pre_remove_side_effects(l: &mut Level, container: &Container, pos: Pos) {
    if matches!(container.kind, Kind::ShulkerBox(_) | Kind::EnderChest) {
        return;
    }
    for stack in &container.items {
        crate::entity::item::drop_item_stack(l, pos.x() as f64, pos.y() as f64, pos.z() as f64, stack);
    }
}

// Containers.updateNeighboursAfterDestroy, which every container block's
// affectNeighborsAfterRemoval is.
pub fn affect_neighbors_after_removal(l: &mut Level, state: State, pos: Pos, _moved: bool) {
    l.update_neighbour_for_output_signal(pos, l.block(state));
}

// BaseEntityBlock.triggerEvent: Block's returns false, and the block entity
// (Level.getBlockEntity makes it if missing) answers.
pub fn trigger_event(l: &mut Level, _state: State, pos: Pos, id: i32, param: i32) -> bool {
    match l.block_entity(pos) {
        Some(BlockEntity::Container(container)) => match &mut container.kind {
            // ChestBlockEntity.triggerEvent and EnderChestBlockEntity.triggerEvent:
            // the lid controller (client-side animation) takes id 1.
            Kind::Chest | Kind::TrappedChest | Kind::EnderChest => id == 1,
            Kind::ShulkerBox(shulker) => shulker.trigger_event(id, param),
            _ => false,
        },
        Some(_) | None => false,
    }
}

// A block entity ticker's wrapper for a class whose getTicker gives none on
// the server.
pub fn no_ticker(_l: &Level, _state: State) -> Option<fn(&mut Level, Pos, State)> {
    None
}

// --- The block entity at a position ---

// A container block entity that must be there: the one a transfer holds is
// the one Java holds a reference to, so a transfer that outlives it (the
// block replaced while the transfer's updates ran) is not modelled.
pub fn be(l: &Level, pos: Pos) -> &Container {
    match l.block_entities.get(&pos) {
        Some(BlockEntity::Container(container)) => container,
        _ => refuse(format!("a container block entity at {pos} replaced during a transfer")),
    }
}

pub fn be_mut(l: &mut Level, pos: Pos) -> &mut Container {
    match l.block_entities.get_mut(&pos) {
        Some(BlockEntity::Container(container)) => container,
        _ => refuse(format!("a container block entity at {pos} replaced during a transfer")),
    }
}

// BlockEntity.setChanged() of a block entity in a level: unless updates are
// ignored (BlockEntity.ignoreBlockEntityUpdates), its position's comparators
// are told, with the block its state names.
pub fn be_changed(l: &mut Level, pos: Pos) {
    if l.ignore_block_entity_updates {
        return;
    }
    set_changed_with_state(l, pos, l.world.get(pos));
}

// BlockEntity.setChanged() of the block entity at a position, with the
// shelf's override (which tells the game event listeners too).
pub fn block_entity_changed(l: &mut Level, pos: Pos) {
    match l.block_entities.get(&pos) {
        Some(BlockEntity::Entity(Entity::Shelf(_))) => super::block_entities::shelf::set_changed(l, pos),
        _ => be_changed(l, pos),
    }
}

// BlockEntity.setChanged(level, pos, state): Level.blockEntityChanged marks
// the chunk unsaved (nothing is saved here).
pub fn set_changed_with_state(l: &mut Level, pos: Pos, state: State) {
    if !l.is_air(state) {
        l.update_neighbour_for_output_signal(pos, l.block(state));
    }
}

// --- Container ---

// The three containers ComposterBlock.getContainer makes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CompostKind {
    Empty,
    Input,
    Output,
}

// ComposterBlock's EmptyContainer, InputContainer and OutputContainer, a
// SimpleContainer made for the block state when asked for.
pub struct Composter {
    pub pos: Pos,
    pub state: State,
    pub kind: CompostKind,
    pub items: Vec<Stack>,
    pub changed: bool,
}

// A Container that block code finds: a container block entity, the two
// chests of a double chest (CompoundContainer(first, second)) or a
// composter's.
pub enum Cont {
    Be(Pos),
    Double(Pos, Pos),
    Composter(Composter),
    // A container entity (a minecart with a chest or a hopper): the stacks
    // the level holds for it.
    Entity(crate::entity::EntityId),
}

fn entity_items(l: &Level, id: crate::entity::EntityId) -> &Vec<Stack> {
    l.entities.containers.get(&id).unwrap_or_else(|| refuse(format!("the container of entity {id} left the level during a transfer")))
}

// What a Cont::Be position holds: a container block entity, or the block
// entity of a furnace, a brewing stand, a jukebox, a chiseled bookshelf, a
// shelf or a decorated pot.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Holder {
    Plain,
    Furnace,
    Brewing,
    Jukebox,
    Shelf,
    ItemShelf,
    Pot,
}

pub fn holder(l: &Level, pos: Pos) -> Holder {
    match l.block_entities.get(&pos) {
        Some(BlockEntity::Container(_)) => Holder::Plain,
        Some(BlockEntity::Entity(Entity::Furnace(..))) => Holder::Furnace,
        Some(BlockEntity::Entity(Entity::BrewingStand(_))) => Holder::Brewing,
        Some(BlockEntity::Entity(Entity::Jukebox(_))) => Holder::Jukebox,
        Some(BlockEntity::Entity(Entity::ChiseledBookShelf(_))) => Holder::Shelf,
        Some(BlockEntity::Entity(Entity::Shelf(_))) => Holder::ItemShelf,
        Some(BlockEntity::Entity(Entity::DecoratedPot(_))) => Holder::Pot,
        _ => refuse(format!("a container block entity at {pos} replaced during a transfer")),
    }
}

// The slots of the container block entity at a position.
pub fn slots(l: &Level, pos: Pos) -> &Vec<Stack> {
    match l.block_entities.get(&pos) {
        Some(BlockEntity::Container(container)) => &container.items,
        Some(BlockEntity::Entity(entity)) if entity.container_size().is_some() => entity.items().expect("a container"),
        _ => refuse(format!("a container block entity at {pos} replaced during a transfer")),
    }
}

pub fn slots_mut(l: &mut Level, pos: Pos) -> &mut Vec<Stack> {
    match l.block_entities.get_mut(&pos) {
        Some(BlockEntity::Container(container)) => &mut container.items,
        Some(BlockEntity::Entity(entity)) if entity.container_size().is_some() => entity.items_mut().expect("a container"),
        _ => refuse(format!("a container block entity at {pos} replaced during a transfer")),
    }
}

impl Cont {
    // Container.getContainerSize.
    pub fn size(&self, l: &Level) -> usize {
        match self {
            Cont::Be(pos) => slots(l, *pos).len(),
            Cont::Double(first, second) => slots(l, *first).len() + slots(l, *second).len(),
            Cont::Composter(c) => c.items.len(),
            Cont::Entity(id) => entity_items(l, *id).len(),
        }
    }

    // Container.getItem.
    pub fn get(&self, l: &Level, slot: usize) -> Stack {
        match self {
            Cont::Be(pos) => slots(l, *pos)[slot].clone(),
            Cont::Double(first, second) => {
                let first_size = slots(l, *first).len();
                if slot >= first_size {
                    slots(l, *second)[slot - first_size].clone()
                } else {
                    slots(l, *first)[slot].clone()
                }
            }
            // SimpleContainer.getItem: a slot out of range is ItemStack.EMPTY.
            Cont::Composter(c) => c.items.get(slot).cloned().unwrap_or_else(empty_stack),
            Cont::Entity(id) => entity_items(l, *id)[slot].clone(),
        }
    }

    // Container.getMaxStackSize.
    pub fn max_stack(&self, l: &Level) -> i32 {
        match self {
            Cont::Be(pos) => match holder(l, *pos) {
                // The chiseled bookshelf holds one of an item in a slot.
                Holder::Shelf => 1,
                _ => MAX_STACK,
            },
            Cont::Double(..) | Cont::Entity(_) => MAX_STACK,
            Cont::Composter(c) => match c.kind {
                CompostKind::Empty => MAX_STACK,
                CompostKind::Input | CompostKind::Output => 1,
            },
        }
    }

    // Container.getMaxStackSize(stack).
    pub fn max_stack_for(&self, l: &Level, stack: &Stack) -> i32 {
        self.max_stack(l).min(max_stack(l.reg, stack))
    }

    // Container.isEmpty.
    pub fn is_empty(&self, l: &Level) -> bool {
        match self {
            Cont::Be(pos) => slots(l, *pos).iter().all(is_empty),
            Cont::Double(first, second) => slots(l, *first).iter().all(is_empty) && slots(l, *second).iter().all(is_empty),
            Cont::Composter(c) => c.items.iter().all(is_empty),
            Cont::Entity(id) => entity_items(l, *id).iter().all(is_empty),
        }
    }

    // Container.setItem: the slot now holds this very stack, cut to the
    // container's limit, and the container changed (a hopper's setItem does
    // not tell anyone).
    pub fn set_item(&mut self, l: &mut Level, slot: usize, stack: Stack) {
        match self {
            Cont::Be(pos) => {
                let pos = *pos;
                match holder(l, pos) {
                    Holder::Plain => {
                        let tells = !matches!(be(l, pos).kind, Kind::Hopper { .. });
                        // CrafterBlockEntity.setItem: a disabled slot is enabled first
                        // (setSlotState, which tells the comparators too).
                        if let Kind::Crafter { disabled, .. } = &mut be_mut(l, pos).kind {
                            if disabled[slot] {
                                disabled[slot] = false;
                                be_changed(l, pos);
                            }
                        }
                        be_mut(l, pos).items[slot] = stack.clone();
                        limit_size(&stack, MAX_STACK.min(max_stack(l.reg, &stack)));
                        if tells {
                            be_changed(l, pos);
                        }
                    }
                    Holder::Furnace => super::furnace::set_item(l, pos, slot, stack),
                    // BaseContainerBlockEntity.setItem.
                    Holder::Brewing => {
                        slots_mut(l, pos)[slot] = stack.clone();
                        limit_size(&stack, MAX_STACK.min(max_stack(l.reg, &stack)));
                        be_changed(l, pos);
                    }
                    // ContainerSingleItem.setItem.
                    Holder::Jukebox => {
                        if slot == 0 {
                            super::jukebox::set_the_item(l, pos, stack);
                        }
                    }
                    Holder::Shelf => super::chiseled_bookshelf::set_item(l, pos, slot, stack),
                    // ListBackedContainer.setItem: setItemNoUpdate, then the
                    // shelf's setChanged.
                    Holder::ItemShelf => {
                        slots_mut(l, pos)[slot] = stack.clone();
                        limit_size(&stack, MAX_STACK.min(max_stack(l.reg, &stack)));
                        super::block_entities::shelf::set_changed(l, pos);
                    }
                    // ContainerSingleItem.setItem: setTheItem, which holds the
                    // stack as it is and tells no one.
                    Holder::Pot => {
                        if slot == 0 {
                            slots_mut(l, pos)[0] = stack;
                        }
                    }
                }
            }
            Cont::Double(first, second) => {
                let (first, second) = (*first, *second);
                let first_size = slots(l, first).len();
                let (pos, slot) = if slot >= first_size { (second, slot - first_size) } else { (first, slot) };
                let mut half = Cont::Be(pos);
                half.set_item(l, slot, stack);
            }
            Cont::Composter(_) => {
                let limit = self.max_stack_for(l, &stack);
                let Cont::Composter(c) = self else { unreachable!() };
                c.items[slot] = stack.clone();
                limit_size(&stack, limit);
                self.set_changed(l);
            }
            Cont::Entity(id) => {
                // ContainerEntity.setChestVehicleItem.
                let limit = MAX_STACK.min(max_stack(l.reg, &stack));
                let id = *id;
                l.entities.containers.get_mut(&id).unwrap_or_else(|| refuse(format!("the container of entity {id} left the level during a transfer")))[slot] = stack.clone();
                limit_size(&stack, limit);
            }
        }
    }

    // Container.removeItem(slot, count) of a block entity (ContainerHelper.removeItem,
    // then setChanged if something came out; a hopper's does not tell anyone).
    pub fn remove_item(&mut self, l: &mut Level, slot: usize, amount: i32) -> Stack {
        let Cont::Be(pos) = *self else { refuse("removeItem of a container that is no block entity") };
        match holder(l, pos) {
            Holder::Jukebox => {
                // ContainerSingleItem.removeItem: the one slot's item, whatever the amount.
                if slot != 0 {
                    return empty_stack();
                }
                super::jukebox::split_the_item(l, pos)
            }
            Holder::Shelf => super::chiseled_bookshelf::remove_item(l, pos, slot),
            // ContainerSingleItem.removeItem: DecoratedPotBlockEntity.splitTheItem,
            // which tells no one; an item split to nothing becomes ItemStack.EMPTY.
            Holder::Pot => {
                if slot != 0 {
                    return empty_stack();
                }
                let item = slots(l, pos)[0].clone();
                let removed = split(&item, amount);
                if is_empty(&item) {
                    slots_mut(l, pos)[0] = empty_stack();
                }
                removed
            }
            // ListBackedContainer.removeItem: ContainerHelper.removeItem, then
            // the shelf's setChanged if something came out.
            Holder::ItemShelf => {
                let stack = slots(l, pos)[slot].clone();
                let removed = if !is_empty(&stack) && amount > 0 { split(&stack, amount) } else { empty_stack() };
                if !is_empty(&removed) {
                    super::block_entities::shelf::set_changed(l, pos);
                }
                removed
            }
            kind => {
                let stack = slots(l, pos)[slot].clone();
                let removed = if !is_empty(&stack) && amount > 0 { split(&stack, amount) } else { empty_stack() };
                if !is_empty(&removed) && !(kind == Holder::Plain && matches!(be(l, pos).kind, Kind::Hopper { .. })) {
                    be_changed(l, pos);
                }
                removed
            }
        }
    }

    // Container.setChanged (the shelf's tells the listeners too).
    pub fn set_changed(&mut self, l: &mut Level) {
        match self {
            Cont::Be(pos) => match holder(l, *pos) {
                Holder::ItemShelf => super::block_entities::shelf::set_changed(l, *pos),
                _ => be_changed(l, *pos),
            },
            Cont::Double(first, second) => {
                be_changed(l, *first);
                be_changed(l, *second);
            }
            Cont::Composter(c) => composter::container_changed(l, c),
            // AbstractMinecartContainer.setChanged does nothing.
            Cont::Entity(_) => {}
        }
    }

    // Container.canPlaceItem.
    pub fn can_place_item(&self, l: &Level, slot: usize, stack: &Stack) -> bool {
        match self {
            Cont::Be(pos) => match holder(l, *pos) {
                Holder::Plain => {
                    let c = be(l, *pos);
                    match c.kind {
                        Kind::Crafter { .. } => super::crafter::can_place_item(l, c, slot),
                        _ => true,
                    }
                }
                Holder::Furnace => super::furnace::can_place_item(l, *pos, slot, stack),
                Holder::Brewing => super::brewing_stand::can_place_item(l, *pos, slot, stack),
                Holder::Jukebox => super::jukebox::can_place_item(l, *pos, slot, stack),
                Holder::Shelf => super::chiseled_bookshelf::can_place_item(l, *pos, slot, stack),
                // ListBackedContainer.canPlaceItem: an empty slot, or one below
                // getMaxStackSize(stack).
                Holder::ItemShelf => {
                    let current = slots(l, *pos)[slot].clone();
                    is_empty(&current) || count(&current) < MAX_STACK.min(max_stack(l.reg, stack))
                }
                // Container.canPlaceItem.
                Holder::Pot => true,
            },
            Cont::Double(first, second) => {
                let first_size = slots(l, *first).len();
                if slot >= first_size {
                    Cont::Be(*second).can_place_item(l, slot - first_size, stack)
                } else {
                    Cont::Be(*first).can_place_item(l, slot, stack)
                }
            }
            Cont::Composter(_) | Cont::Entity(_) => true,
        }
    }

    // WorldlyContainer.getSlotsForFace, or none for a container that is not
    // a WorldlyContainer.
    pub fn slots_for_face(&self, l: &Level, face: Dir) -> Option<Vec<usize>> {
        match self {
            Cont::Be(pos) => match holder(l, *pos) {
                Holder::Plain => match be(l, *pos).kind {
                    Kind::ShulkerBox(_) => Some((0..27).collect()),
                    _ => None,
                },
                // AbstractFurnaceBlockEntity: the result and the fuel below, the ingredient above, the fuel at the sides.
                Holder::Furnace => Some(match face {
                    Dir::Down => vec![2, 1],
                    Dir::Up => vec![0],
                    _ => vec![1],
                }),
                // BrewingStandBlockEntity: the bottles and the ingredient below, the ingredient above, the bottles and the fuel at the sides.
                Holder::Brewing => Some(match face {
                    Dir::Up => vec![3],
                    Dir::Down => vec![0, 1, 2, 3],
                    _ => vec![0, 1, 2, 4],
                }),
                Holder::Jukebox | Holder::Shelf | Holder::ItemShelf | Holder::Pot => None,
            },
            Cont::Double(..) | Cont::Entity(_) => None,
            Cont::Composter(c) => Some(match c.kind {
                CompostKind::Empty => vec![],
                CompostKind::Input => if face == Dir::Up { vec![0] } else { vec![] },
                CompostKind::Output => if face == Dir::Down { vec![0] } else { vec![] },
            }),
        }
    }

    pub fn is_worldly(&self, l: &Level) -> bool {
        self.slots_for_face(l, Dir::Down).is_some()
    }

    // WorldlyContainer.canPlaceItemThroughFace.
    pub fn can_place_item_through_face(&self, l: &Level, slot: usize, stack: &Stack, face: Option<Dir>) -> bool {
        match self {
            Cont::Be(pos) => match holder(l, *pos) {
                // Block.byItem(stack.getItem()) instanceof ShulkerBoxBlock.
                Holder::Plain => match be(l, *pos).kind {
                    Kind::ShulkerBox(_) => {
                        let s = stack.borrow();
                        let block = if s.count <= 0 { None } else { l.reg.items[s.item as usize].block };
                        !block.is_some_and(|b| l.reg.blocks[b as usize].class == "ShulkerBoxBlock")
                    }
                    _ => true,
                },
                // The furnace and the brewing stand: canPlaceItem.
                Holder::Furnace | Holder::Brewing => self.can_place_item(l, slot, stack),
                Holder::Jukebox | Holder::Shelf | Holder::ItemShelf | Holder::Pot => true,
            },
            Cont::Double(..) | Cont::Entity(_) => true,
            Cont::Composter(c) => match c.kind {
                CompostKind::Input => !c.changed && face == Some(Dir::Up) && composter::compostable(l, stack),
                CompostKind::Output | CompostKind::Empty => false,
            },
        }
    }

    // WorldlyContainer.canTakeItemThroughFace.
    pub fn can_take_item_through_face(&self, l: &Level, slot: usize, stack: &Stack, face: Dir) -> bool {
        match self {
            Cont::Be(pos) => match holder(l, *pos) {
                Holder::Plain | Holder::Jukebox | Holder::Shelf | Holder::ItemShelf | Holder::Pot => true,
                Holder::Furnace => super::furnace::can_take_item_through_face(l, slot, stack, face),
                Holder::Brewing => super::brewing_stand::can_take_item_through_face(l, slot, stack),
            },
            Cont::Double(..) | Cont::Entity(_) => true,
            Cont::Composter(c) => match c.kind {
                CompostKind::Output => !c.changed && face == Dir::Down && composter::is_bone_meal(l, stack),
                CompostKind::Input | CompostKind::Empty => false,
            },
        }
    }

    // Container.canTakeItem(target, slot, stack): whether a hopper (the
    // target) may take the stack from this container.
    pub fn can_take_item(&self, l: &Level, target: &Cont, _slot: usize, stack: &Stack) -> bool {
        match self {
            Cont::Be(pos) => match holder(l, *pos) {
                Holder::Jukebox => (0..target.size(l)).any(|i| is_empty(&target.get(l, i))),
                Holder::Shelf => (0..target.size(l)).any(|i| {
                    let item = target.get(l, i);
                    is_empty(&item) || same_item(stack, &item) && count(&item) + count(stack) <= target.max_stack_for(l, &item)
                }),
                _ => true,
            },
            Cont::Double(..) | Cont::Composter(_) | Cont::Entity(_) => true,
        }
    }
}

// AbstractContainerMenu.getRedstoneSignalFromContainer.
pub fn redstone_signal(l: &Level, container: &Cont) -> i32 {
    let mut f = 0.0f32;
    let size = container.size(l);
    for i in 0..size {
        let item = container.get(l, i);
        if !is_empty(&item) {
            f += count(&item) as f32 / container.max_stack_for(l, &item) as f32;
        }
    }
    f /= size as f32;
    lerp_discrete(f)
}

// Mth.lerpDiscrete(f, 0, 15).
pub fn lerp_discrete(f: f32) -> i32 {
    (f * 14.0f32).floor() as i32 + if f > 0.0 { 1 } else { 0 }
}

// getAnalogOutputSignal of a block whose signal is
// AbstractContainerMenu.getRedstoneSignalFromBlockEntity(level.getBlockEntity(pos)).
pub fn analog_output_signal(l: &mut Level, _state: State, pos: Pos, _direction: Dir) -> i32 {
    match l.block_entity(pos) {
        Some(BlockEntity::Container(container)) if container.is_container() => redstone_signal(l, &Cont::Be(pos)),
        Some(_) | None => 0,
    }
}
