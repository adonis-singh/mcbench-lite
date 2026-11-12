// PotionBrewing as the server bootstraps it (PotionBrewing.bootstrap with the
// vanilla mixes; no plugin adds custom mixes): which items are brewing
// ingredients and containers, whether an ingredient mixes with a potion, and
// what it makes.

use crate::loot::ItemStack;
use crate::nbt::Tag;
use crate::registry::Registry;

// Ingredient.test.
fn accepts(ingredient: &[u16], stack: &ItemStack) -> bool {
    stack.count > 0 && ingredient.contains(&stack.item)
}

// PotionBrewing.isContainer.
fn is_container(reg: &Registry, stack: &ItemStack) -> bool {
    reg.brew_containers.iter().any(|ingredient| accepts(ingredient, stack))
}

// PotionBrewing.isIngredient: a container mix's ingredient or a potion mix's.
pub fn is_ingredient(reg: &Registry, stack: &ItemStack) -> bool {
    reg.container_mixes.iter().any(|mix| accepts(&mix.ingredient, stack)) || reg.potion_mixes.iter().any(|mix| accepts(&mix.ingredient, stack))
}

// ItemStack.is(item), for a stack that may be empty (air).
fn is_item(stack: &ItemStack, item: u16) -> bool {
    (if stack.count <= 0 { 0 } else { stack.item }) == item
}

// stack.getOrDefault(POTION_CONTENTS, EMPTY).potion(): the potion's key.
fn potion_of(reg: &Registry, stack: &ItemStack) -> Option<String> {
    match stack.component(reg, "minecraft:potion_contents") {
        Some(Tag::Compound(c)) => match c.get("potion") {
            Some(Tag::String(key)) => Some(key.clone()),
            _ => None,
        },
        _ => None,
    }
}

// PotionBrewing.hasMix(potion, ingredient).
pub fn has_mix(reg: &Registry, potion: &ItemStack, ingredient: &ItemStack) -> bool {
    is_container(reg, potion) && (has_container_mix(reg, potion, ingredient) || has_potion_mix(reg, potion, ingredient))
}

fn has_container_mix(reg: &Registry, potion: &ItemStack, ingredient: &ItemStack) -> bool {
    reg.container_mixes.iter().any(|mix| is_item(potion, mix.from) && accepts(&mix.ingredient, ingredient))
}

fn has_potion_mix(reg: &Registry, potion: &ItemStack, ingredient: &ItemStack) -> bool {
    let Some(key) = potion_of(reg, potion) else { return false };
    reg.potion_mixes.iter().any(|mix| mix.from == key && accepts(&mix.ingredient, ingredient))
}

// PotionContents.createItemStack: a new stack of the item holding the potion.
fn create_item_stack(reg: &Registry, item: u16, potion: &str) -> ItemStack {
    let mut stack = ItemStack::new(item, 1);
    let mut contents = crate::nbt::Compound::new();
    contents.put("potion", Tag::string(potion));
    stack.set_component(reg, "minecraft:potion_contents", Tag::Compound(contents));
    stack
}

// PotionBrewing.mix(ingredient, potion): what the potion becomes (the stack
// itself where nothing mixes).
pub fn mix(reg: &Registry, ingredient: &ItemStack, potion: &ItemStack) -> ItemStack {
    if potion.count <= 0 {
        return potion.clone();
    }
    let Some(key) = potion_of(reg, potion) else { return potion.clone() };
    for mix in &reg.container_mixes {
        if is_item(potion, mix.from) && accepts(&mix.ingredient, ingredient) {
            return create_item_stack(reg, mix.to, &key);
        }
    }
    for mix in &reg.potion_mixes {
        if mix.from == key && accepts(&mix.ingredient, ingredient) {
            return create_item_stack(reg, potion.item, &mix.to);
        }
    }
    potion.clone()
}
