// Inventory of the verifier's detached input player
// (McbenchExperiment.inputPlayer), for the book a lectern's menu hands it
// (LecternMenu's take-book button calls Inventory.add). Its 36 slots; its
// equipment (armor, offhand, body, saddle) is never filled here, and its
// selected slot stays the first. The player is in survival, so it has no
// infinite materials.

use crate::loot::ItemStack;
use crate::registry::Registry;

const SLOTS: usize = 36;
const SELECTED: usize = 0;
// Container.DEFAULT_MAX_STACK_SIZE, the inventory's own maximum.
const MAX_STACK: i32 = 99;

pub struct Inventory {
    items: Vec<ItemStack>,
}

impl Default for Inventory {
    fn default() -> Inventory {
        Inventory { items: (0..SLOTS).map(|_| ItemStack::empty()).collect() }
    }
}

// ItemStack.isEmpty.
fn is_empty(stack: &ItemStack) -> bool {
    stack.item == 0 || stack.count <= 0
}

// ItemStack.isDamaged: a damageable item (max_damage and damage, and not
// unbreakable) whose damage, clamped to its maximum, is above 0.
fn is_damaged(reg: &Registry, stack: &ItemStack) -> bool {
    use crate::nbt::Tag;
    let int = |key: &str| match stack.component(reg, key) {
        Some(Tag::Int(value)) => Some(value),
        Some(other) => panic!("{key} is no int: {other:?}"),
        None => None,
    };
    let (Some(max), Some(damage)) = (int("minecraft:max_damage"), int("minecraft:damage")) else { return false };
    if stack.component(reg, "minecraft:unbreakable").is_some() {
        return false;
    }
    damage.max(0).min(max) > 0
}

impl Inventory {
    // Inventory.isEmpty: every slot and the equipment empty.
    pub fn is_empty(&self) -> bool {
        self.items.iter().all(is_empty)
    }

    // Inventory.getItem of the 36 slots (the equipment's slots after them are
    // empty).
    pub fn items(&self) -> &[ItemStack] {
        &self.items
    }

    // Inventory.removeItemNoUpdate of one of the 36 slots.
    pub fn remove_item_no_update(&mut self, slot: usize) -> ItemStack {
        std::mem::replace(&mut self.items[slot], ItemStack::empty())
    }

    // Inventory.getFreeSlot: the first empty slot.
    fn free_slot(&self) -> Option<usize> {
        self.items.iter().position(is_empty)
    }

    // Inventory.hasRemainingSpaceForItem: a stackable stack of the same item
    // and components below its maximum (ItemStack.isStackable: a maximum
    // over 1 and no damage).
    fn has_remaining_space(reg: &Registry, destination: &ItemStack, origin: &ItemStack) -> bool {
        if is_empty(destination) {
            return false;
        }
        let max = destination.max_stack_size(reg);
        max > 1 && !is_damaged(reg, destination) && destination.count < max.min(MAX_STACK) && crate::blocks::container::same_item_same_components(destination, origin)
    }

    // Inventory.getSlotWithRemainingSpace: the selected slot, then the
    // offhand (empty here), then the slots in order.
    fn slot_with_remaining_space(&self, reg: &Registry, stack: &ItemStack) -> Option<usize> {
        if Self::has_remaining_space(reg, &self.items[SELECTED], stack) {
            return Some(SELECTED);
        }
        (0..SLOTS).find(|&slot| Self::has_remaining_space(reg, &self.items[slot], stack))
    }

    // Inventory.addResource(slot, stack): what of the stack does not fit in
    // the slot. An empty slot takes a copy of the stack with no items, whose
    // maximum is the stack's.
    fn add_resource_to(&mut self, reg: &Registry, slot: usize, stack: &ItemStack) -> i32 {
        let mut count = stack.count;
        if is_empty(&self.items[slot]) {
            let mut copy = stack.clone();
            copy.count = 0;
            self.items[slot] = copy;
        }
        let max = stack.max_stack_size(reg).min(MAX_STACK);
        let item = &mut self.items[slot];
        let min = count.min(max - item.count);
        if min == 0 {
            return count;
        }
        count -= min;
        item.count += min;
        count
    }

    // Inventory.addResource(stack).
    fn add_resource(&mut self, reg: &Registry, stack: &ItemStack) -> i32 {
        match self.slot_with_remaining_space(reg, stack).or_else(|| self.free_slot()) {
            Some(slot) => self.add_resource_to(reg, slot, stack),
            None => stack.count,
        }
    }

    // Inventory.add(stack): whether any of the stack went in; what went in
    // leaves the stack.
    pub fn add(&mut self, reg: &Registry, stack: &mut ItemStack) -> bool {
        if is_empty(stack) {
            return false;
        }
        if is_damaged(reg, stack) {
            // The whole stack goes to the first free slot (copyAndClear).
            let Some(slot) = self.free_slot() else { return false };
            self.items[slot] = stack.clone();
            stack.count = 0;
            return true;
        }
        loop {
            let count = stack.count;
            stack.count = self.add_resource(reg, stack);
            if is_empty(stack) || stack.count >= count {
                return stack.count < count;
            }
        }
    }
}
