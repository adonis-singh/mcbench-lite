// AbstractFurnaceBlock and its subclasses FurnaceBlock, BlastFurnaceBlock and
// SmokerBlock, with their block entities and AbstractFurnaceBlockEntity's
// ticker: fuel (FuelValues), cooking recipes (RecipeManager with the furnace's
// own CachedCheck), the lit state, recipes used, and the slots hoppers use.
// No plugin listens to FurnaceBurnEvent, FurnaceStartSmeltEvent and
// FurnaceSmeltEvent.

use super::container::{self, is_empty, same_item_same_components, set_changed_with_state, Stack};
use super::entities::{self, Entity, Furnace, FurnaceKind};
use super::{BlockEntity, Behaviour, NONE};
use crate::dir::{Dir, Pos};
use crate::level::Level;
use crate::loot::ItemStack;
use crate::registry::{CookingKind, Registry, State};

const fn behaviour(new_block_entity: fn(&mut Level, State, Pos) -> BlockEntity) -> Behaviour {
    Behaviour {
        affect_neighbors_after_removal: Some(container::affect_neighbors_after_removal),
        get_analog_output_signal: Some(get_analog_output_signal),
        new_block_entity: Some(new_block_entity),
        ticker: Some(ticker),
        ..NONE
    }
}

pub const FURNACE: Behaviour = behaviour(|_, _, _| BlockEntity::Entity(Entity::Furnace(FurnaceKind::Furnace, Furnace::new())));
pub const BLAST_FURNACE: Behaviour = behaviour(|_, _, _| BlockEntity::Entity(Entity::Furnace(FurnaceKind::BlastFurnace, Furnace::new())));
pub const SMOKER: Behaviour = behaviour(|_, _, _| BlockEntity::Entity(Entity::Furnace(FurnaceKind::Smoker, Furnace::new())));

// AbstractFurnaceBlock.getAnalogOutputSignal.
fn get_analog_output_signal(l: &mut Level, _state: State, pos: Pos, _direction: Dir) -> i32 {
    entities::container_signal(l, pos)
}

// AbstractFurnaceBlock.createFurnaceTicker on the server:
// AbstractFurnaceBlockEntity.serverTick.
fn ticker(_l: &Level, _state: State) -> Option<fn(&mut Level, Pos, State)> {
    Some(server_tick)
}

// AbstractFurnaceBlockEntity.maxStack.
const MAX_STACK: i32 = 99;

fn furnace_mut(l: &mut Level, pos: Pos) -> (FurnaceKind, &mut Furnace) {
    match l.block_entities.get_mut(&pos) {
        Some(BlockEntity::Entity(Entity::Furnace(kind, furnace))) => (*kind, furnace),
        _ => panic!("a furnace ticks without its block entity"),
    }
}

// The recipe type the furnace cooks with.
fn cooking_kind(kind: FurnaceKind) -> CookingKind {
    match kind {
        FurnaceKind::Furnace => CookingKind::Smelting,
        FurnaceKind::BlastFurnace => CookingKind::Blasting,
        FurnaceKind::Smoker => CookingKind::Smoking,
    }
}

// Ingredient.test.
fn accepts(ingredient: &[u16], stack: &ItemStack) -> bool {
    stack.count > 0 && ingredient.contains(&stack.item)
}

// The furnace's CachedCheck.getRecipeFor: its last recipe if it still matches,
// else the last of the type's recipes that matches; what is found is
// remembered.
fn quick_check(reg: &Registry, kind: CookingKind, last: &mut Option<usize>, stack: &ItemStack) -> Option<usize> {
    let matches = |i: usize| reg.cooking[i].kind == kind && accepts(&reg.cooking[i].ingredient, stack);
    let found = match *last {
        Some(i) if matches(i) => Some(i),
        // RecipeMap.getRecipesFor: an empty input has no recipes.
        _ if stack.count <= 0 => None,
        _ => (0..reg.cooking.len()).rev().find(|&i| matches(i)),
    };
    if found.is_some() {
        *last = found;
    }
    found
}

// The result of a cooking recipe (a copy of it).
fn result_of(reg: &Registry, recipe: usize) -> ItemStack {
    match &reg.cooking[recipe].result {
        Some(result) => result.clone(),
        None => crate::level::refuse(format!("the result of the cooking recipe {} (a data component this port does not model)", reg.cooking[recipe].id)),
    }
}

// AbstractFurnaceBlockEntity.getTotalCookTime.
fn total_cook_time(reg: &Registry, kind: FurnaceKind, furnace: &mut Furnace) -> i32 {
    let input = furnace.items[0].borrow().clone();
    let cook_time = quick_check(reg, cooking_kind(kind), &mut furnace.last_recipe, &input).map_or(200, |recipe| reg.cooking[recipe].cooking_time);
    // (int)Math.ceil((double)cookTime / cookSpeedMultiplier).
    (cook_time as f64 / furnace.cook_speed_multiplier).ceil() as i32
}

// AbstractFurnaceBlockEntity.getBurnDuration: FuelValues.burnDuration, which
// the blast furnace and the smoker halve.
fn burn_duration(reg: &Registry, kind: FurnaceKind, stack: &ItemStack) -> i32 {
    let duration = if stack.count <= 0 { 0 } else { reg.fuel.get(&stack.item).copied().unwrap_or(0) };
    match kind {
        FurnaceKind::Furnace => duration,
        FurnaceKind::BlastFurnace | FurnaceKind::Smoker => duration / 2,
    }
}

// AbstractFurnaceBlockEntity.canBurn.
fn can_burn(reg: &Registry, recipe: Option<usize>, items: &[Stack], max_stack_size: i32) -> bool {
    let Some(recipe) = recipe else { return false };
    if is_empty(&items[0]) {
        return false;
    }
    let result = result_of(reg, recipe);
    if result.count <= 0 {
        return false;
    }
    let slot = items[2].borrow();
    slot.count <= 0
        || same_item_same_components(&slot, &result)
            && (slot.count < max_stack_size && slot.count < slot.max_stack_size(reg) || slot.count < result.max_stack_size(reg))
}

// AbstractFurnaceBlockEntity.burn.
fn burn(reg: &Registry, recipe: Option<usize>, items: &mut [Stack], max_stack_size: i32) -> bool {
    if recipe.is_none() || !can_burn(reg, recipe, items, max_stack_size) {
        return false;
    }
    let ingredient = items[0].clone();
    let result = result_of(reg, recipe.unwrap());
    // FurnaceSmeltEvent: its result is the recipe's, a plain stack.
    if result.count > 0 {
        if is_empty(&items[2]) {
            items[2] = container::stack_of(result);
        } else {
            container::grow(&items[2], result.count);
        }
    }
    let is_wet_sponge = {
        let stack = ingredient.borrow();
        stack.count > 0 && reg.items[stack.item as usize].key == "minecraft:wet_sponge"
    };
    if is_wet_sponge && !is_empty(&items[1]) && reg.items[items[1].borrow().item as usize].key == "minecraft:bucket" {
        items[1] = container::new_stack(reg.item_by_key["minecraft:water_bucket"], 1);
    }
    container::shrink(&ingredient, 1);
    true
}

// AbstractFurnaceBlockEntity.serverTick. A furnace that is lit, or has fuel
// and an ingredient, looks up the recipe of its ingredient (remembering it);
// one that is not lit burns its fuel when it could cook; one that is lit cooks
// (cookingTimer up to cookingTotalTime) and then burns the ingredient into
// the result. A furnace not lit cools its cooking timer by 2 a tick. The
// block's lit state is set when the burning changed during the tick.
fn server_tick(l: &mut Level, pos: Pos, state: State) {
    let reg = l.reg;
    let (kind, furnace) = furnace_mut(l, pos);
    let was_lit = furnace.lit_time_remaining > 0;
    let mut changed = false;
    if was_lit {
        furnace.lit_time_remaining -= 1;
    }
    let fuel = furnace.items[1].clone();
    let ingredient = furnace.items[0].clone();
    let has_ingredient = !is_empty(&ingredient);
    let has_fuel = !is_empty(&fuel);
    if furnace.lit_time_remaining > 0 || has_fuel && has_ingredient {
        let recipe = if has_ingredient {
            let input = ingredient.borrow().clone();
            quick_check(reg, cooking_kind(kind), &mut furnace.last_recipe, &input)
        } else {
            None
        };
        if furnace.lit_time_remaining <= 0 && can_burn(reg, recipe, &furnace.items, MAX_STACK) {
            // FurnaceBurnEvent: the fuel's burn time, burning, consuming the fuel.
            furnace.lit_time_remaining = burn_duration(reg, kind, &fuel.borrow());
            furnace.lit_total_time = furnace.lit_time_remaining;
            if furnace.lit_time_remaining > 0 {
                changed = true;
                if has_fuel {
                    let item = fuel.borrow().item;
                    container::shrink(&fuel, 1);
                    if is_empty(&fuel) {
                        furnace.items[1] = match reg.remainders.get(&item) {
                            Some(&(remainder, count)) => container::new_stack(remainder, count),
                            None => container::empty_stack(),
                        };
                    }
                }
            }
        }
        if furnace.lit_time_remaining > 0 && can_burn(reg, recipe, &furnace.items, MAX_STACK) {
            if recipe.is_some() && furnace.cooking_timer == 0 {
                // FurnaceStartSmeltEvent.
                furnace.cooking_total_time = total_cook_time(reg, kind, furnace);
            }
            furnace.cooking_timer += 1;
            if furnace.cooking_timer >= furnace.cooking_total_time {
                furnace.cooking_timer = 0;
                furnace.cooking_total_time = total_cook_time(reg, kind, furnace);
                if burn(reg, recipe, &mut furnace.items, MAX_STACK) {
                    // setRecipeUsed.
                    let id = &reg.cooking[recipe.unwrap()].id;
                    match furnace.recipes_used.iter_mut().find(|(key, _)| key == id) {
                        Some((_, count)) => *count += 1,
                        None => furnace.recipes_used.push((id.clone(), 1)),
                    }
                }
                changed = true;
            }
        } else {
            furnace.cooking_timer = 0;
        }
    } else if furnace.lit_time_remaining <= 0 && furnace.cooking_timer > 0 {
        // Mth.clamp(cookingTimer - 2, 0, cookingTotalTime).
        let n = furnace.cooking_timer - 2;
        furnace.cooking_timer = if n < 0 { 0 } else { n.min(furnace.cooking_total_time) };
    }
    let now_lit = furnace.lit_time_remaining > 0;
    let mut state = state;
    if was_lit != now_lit {
        changed = true;
        state = l.reg.with_bool(state, l.reg.property_of(state, "lit"), now_lit);
        l.set(pos, state, crate::level::UPDATE_ALL);
    }
    if changed {
        set_changed_with_state(l, pos, state);
    }
}

// AbstractFurnaceBlockEntity.setItem.
pub fn set_item(l: &mut Level, pos: Pos, slot: usize, stack: Stack) {
    let reg = l.reg;
    let (kind, furnace) = furnace_mut(l, pos);
    let same = !is_empty(&stack) && same_item_same_components(&furnace.items[slot].borrow(), &stack.borrow());
    furnace.items[slot] = stack.clone();
    container::limit_size(&stack, MAX_STACK.min(container::max_stack(reg, &stack)));
    if slot == 0 && !same {
        furnace.cooking_total_time = total_cook_time(reg, kind, furnace);
        furnace.cooking_timer = 0;
        container::be_changed(l, pos);
    }
}

// AbstractFurnaceBlockEntity.canPlaceItem.
pub fn can_place_item(l: &Level, pos: Pos, slot: usize, stack: &Stack) -> bool {
    match slot {
        2 => false,
        1 => {
            let stack = stack.borrow();
            let current = container::slots(l, pos)[1].borrow();
            let is_bucket = |s: &ItemStack| s.count > 0 && l.reg.items[s.item as usize].key == "minecraft:bucket";
            // FuelValues.isFuel: the item is one that burns.
            (stack.count > 0 && l.reg.fuel.contains_key(&stack.item)) || is_bucket(&stack) && !is_bucket(&current)
        }
        _ => true,
    }
}

// AbstractFurnaceBlockEntity.canTakeItemThroughFace.
pub fn can_take_item_through_face(l: &Level, slot: usize, stack: &Stack, face: Dir) -> bool {
    if face != Dir::Down || slot != 1 {
        return true;
    }
    let stack = stack.borrow();
    stack.count > 0 && matches!(l.reg.items[stack.item as usize].key.as_str(), "minecraft:water_bucket" | "minecraft:bucket")
}
