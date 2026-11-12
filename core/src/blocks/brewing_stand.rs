// BrewingStandBlock and BrewingStandBlockEntity: the fuel, the brew and the
// bottle states of serverTick, the slots hoppers use (canPlaceItem and the
// sides of getSlotsForFace). No plugin listens to BrewingStandFuelEvent,
// BrewingStartEvent and BrewEvent.

use super::container::{self, is_empty, set_changed_with_state, Stack};
use super::entities::{self, BrewingStand, Entity};
use super::{potion_brewing, BlockEntity, Behaviour, NONE};
use crate::dir::{Dir, Pos};
use crate::level::{Level, UPDATE_CLIENTS};
use crate::loot::ItemStack;
use crate::property;
use crate::registry::{Registry, State};

pub const BEHAVIOUR: Behaviour = Behaviour {
    affect_neighbors_after_removal: Some(container::affect_neighbors_after_removal),
    get_analog_output_signal: Some(get_analog_output_signal),
    new_block_entity: Some(new_block_entity),
    ticker: Some(ticker),
    ..NONE
};

// BrewingStandBlock.HAS_BOTTLE.
fn has_bottle(l: &Level, index: usize) -> u16 {
    match index {
        0 => property!(l, "has_bottle_0", ["true", "false"]),
        1 => property!(l, "has_bottle_1", ["true", "false"]),
        2 => property!(l, "has_bottle_2", ["true", "false"]),
        _ => panic!("no bottle {index}"),
    }
}

// BrewingStandBlock.getAnalogOutputSignal.
fn get_analog_output_signal(l: &mut Level, _state: State, pos: Pos, _direction: Dir) -> i32 {
    entities::container_signal(l, pos)
}

fn new_block_entity(_l: &mut Level, _state: State, _pos: Pos) -> BlockEntity {
    BlockEntity::Entity(Entity::BrewingStand(BrewingStand::new()))
}

// BrewingStandBlock.getTicker on the server.
fn ticker(_l: &Level, _state: State) -> Option<fn(&mut Level, Pos, State)> {
    Some(server_tick)
}

fn stand_mut(l: &mut Level, pos: Pos) -> &mut BrewingStand {
    match l.block_entities.get_mut(&pos) {
        Some(BlockEntity::Entity(Entity::BrewingStand(stand))) => stand,
        _ => panic!("a brewing stand ticks without its block entity"),
    }
}

fn is_item_key(reg: &Registry, stack: &ItemStack, key: &str) -> bool {
    stack.count > 0 && reg.items[stack.item as usize].key == key
}

// ItemStack.is(ItemTags.BREWING_FUEL).
fn is_fuel(reg: &Registry, stack: &ItemStack) -> bool {
    stack.count > 0 && reg.item_tags["minecraft:brewing_fuel"][stack.item as usize]
}

// BrewingStandBlockEntity.isBrewable.
fn is_brewable(reg: &Registry, items: &[Stack]) -> bool {
    let ingredient = items[3].borrow();
    if ingredient.count <= 0 || !potion_brewing::is_ingredient(reg, &ingredient) {
        return false;
    }
    items[..3].iter().any(|bottle| {
        let bottle = bottle.borrow();
        bottle.count > 0 && potion_brewing::has_mix(reg, &bottle, &ingredient)
    })
}

// BrewingStandBlockEntity.serverTick.
fn server_tick(l: &mut Level, pos: Pos, state: State) {
    let reg = l.reg;
    let items = stand_mut(l, pos).items.clone();
    if stand_mut(l, pos).fuel <= 0 && is_fuel(reg, &items[4].borrow()) {
        // BrewingStandFuelEvent: the fuel power 20, consuming the fuel.
        stand_mut(l, pos).fuel = 20;
        container::shrink(&items[4], 1);
        set_changed_with_state(l, pos, state);
    }
    let brewable = is_brewable(reg, &items);
    let brewing = stand_mut(l, pos).brew_time > 0;
    let ingredient = items[3].clone();
    if brewing {
        let stand = stand_mut(l, pos);
        stand.brew_time -= 1;
        let finished = stand.brew_time == 0;
        // ItemStack.is(item): getItem() is air for an empty stack.
        let same_ingredient = {
            let stack = ingredient.borrow();
            stand.ingredient.is_some_and(|item| (if stack.count <= 0 { 0 } else { stack.item }) == item)
        };
        if finished && brewable {
            do_brew(l, pos);
        } else if !brewable || !same_ingredient {
            stand_mut(l, pos).brew_time = 0;
        }
        set_changed_with_state(l, pos, state);
    } else if brewable && stand_mut(l, pos).fuel > 0 {
        let stand = stand_mut(l, pos);
        stand.fuel -= 1;
        // BrewingStartEvent: the brew takes 400 ticks.
        stand.brew_time = 400;
        stand.ingredient = Some(if is_empty(&ingredient) { 0 } else { ingredient.borrow().item });
        set_changed_with_state(l, pos, state);
    }
    let stand = stand_mut(l, pos);
    let potion_bits = [!is_empty(&stand.items[0]), !is_empty(&stand.items[1]), !is_empty(&stand.items[2])];
    if stand.last_potion_count != Some(potion_bits) {
        stand.last_potion_count = Some(potion_bits);
        // (The state is a brewing stand's, which this ticker only runs for.)
        let mut new = state;
        for (i, bit) in potion_bits.into_iter().enumerate() {
            new = l.reg.with_bool(new, has_bottle(l, i), bit);
        }
        l.set(pos, new, UPDATE_CLIENTS);
    }
}

// BrewingStandBlockEntity.doBrew.
fn do_brew(l: &mut Level, pos: Pos) {
    let reg = l.reg;
    let items = stand_mut(l, pos).items.clone();
    let ingredient = items[3].clone();
    let results: Vec<ItemStack> = (0..3).map(|i| potion_brewing::mix(reg, &ingredient.borrow(), &items[i].borrow())).collect();
    // The BrewEvent's results go into the slots as copies.
    for (i, result) in results.into_iter().enumerate() {
        stand_mut(l, pos).items[i] = if result.count <= 0 { container::empty_stack() } else { container::stack_of(result) };
    }
    container::shrink(&ingredient, 1);
    // itemStack.getItem() is air once the stack is empty, which leaves nothing.
    let item = if is_empty(&ingredient) { 0 } else { ingredient.borrow().item };
    let remainder = match reg.remainders.get(&item) {
        Some(&(item, count)) => ItemStack::new(item, count),
        None => ItemStack::empty(),
    };
    let mut new_ingredient = ingredient;
    if remainder.count > 0 {
        if is_empty(&new_ingredient) {
            new_ingredient = container::stack_of(remainder);
        } else {
            crate::entity::item::drop_item_stack(l, pos.x() as f64, pos.y() as f64, pos.z() as f64, &container::stack_of(remainder));
        }
    }
    stand_mut(l, pos).items[3] = new_ingredient;
}

// BrewingStandBlockEntity.canPlaceItem.
pub fn can_place_item(l: &Level, pos: Pos, slot: usize, stack: &Stack) -> bool {
    let reg = l.reg;
    let stack = stack.borrow();
    match slot {
        3 => potion_brewing::is_ingredient(reg, &stack),
        4 => is_fuel(reg, &stack),
        _ => {
            let bottle = ["minecraft:potion", "minecraft:splash_potion", "minecraft:lingering_potion", "minecraft:glass_bottle"].iter().any(|key| is_item_key(reg, &stack, key));
            bottle && is_empty(&container::slots(l, pos)[slot])
        }
    }
}

// BrewingStandBlockEntity.canTakeItemThroughFace.
pub fn can_take_item_through_face(l: &Level, slot: usize, stack: &Stack) -> bool {
    slot != 3 || is_item_key(l.reg, &stack.borrow(), "minecraft:glass_bottle")
}
