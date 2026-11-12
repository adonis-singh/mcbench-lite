// The crafting recipes a crafter's grid is matched against and what they
// make: CraftingInput (CraftingInput.ofPositioned), RecipeManager.getRecipeFor
// over the CRAFTING recipes (RecipeCache is only a memo of that), each
// recipe's matches(), assemble() and getRemainingItems().
//
// ShapedRecipe, ShapelessRecipe and TransmuteRecipe come from the registry's
// recipe lines; the CustomRecipe classes are ported one by one below. A
// recipe whose result needs a data component this port does not model
// refuses when it is the one matched.

use super::container::{copy_with_count, is_empty, Stack};
use crate::level::refuse;
use crate::loot::ItemStack;
use crate::nbt::Tag;
use crate::registry::{Recipe, Registry};

// CraftingInput: the non-empty part of the grid (CraftingInput.ofPositioned)
// with the stacks as values.
pub struct Input {
    pub width: usize,
    pub height: usize,
    pub items: Vec<ItemStack>,
    pub ingredient_count: usize,
}

impl Input {
    // CraftingInput.of(3, 3, stacks).
    pub fn of(stacks: &[Stack]) -> Input {
        let (width, height) = (3usize, 3usize);
        let empty = |x: usize, y: usize| is_empty(&stacks[x + y * width]);
        let (mut min_x, mut max_x, mut min_y, mut max_y) = (width as i32 - 1, 0i32, height as i32 - 1, 0i32);
        for y in 0..height {
            let mut empty_row = true;
            for x in 0..width {
                if !empty(x, y) {
                    min_x = min_x.min(x as i32);
                    max_x = max_x.max(x as i32);
                    empty_row = false;
                }
            }
            if !empty_row {
                min_y = min_y.min(y as i32);
                max_y = max_y.max(y as i32);
            }
        }
        let (w, h) = (max_x - min_x + 1, max_y - min_y + 1);
        if w <= 0 || h <= 0 {
            return Input { width: 0, height: 0, items: Vec::new(), ingredient_count: 0 };
        }
        let mut items = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let stack = stacks[(x + min_x) as usize + (y + min_y) as usize * width].borrow().clone();
                items.push(if stack.count <= 0 { ItemStack::empty() } else { stack });
            }
        }
        let ingredient_count = items.iter().filter(|s| s.count > 0).count();
        Input { width: w as usize, height: h as usize, items, ingredient_count }
    }

    // CraftingInput.getItem(row, column): x across, y down.
    fn at(&self, x: usize, y: usize) -> &ItemStack {
        &self.items[x + y * self.width]
    }

    pub fn is_empty(&self) -> bool {
        self.ingredient_count == 0
    }

    // The non-empty stacks in order.
    fn present(&self) -> impl Iterator<Item = &ItemStack> {
        self.items.iter().filter(|s| s.count > 0)
    }
}

fn item_in_tag(reg: &Registry, stack: &ItemStack, tag: &str) -> bool {
    stack.count > 0 && reg.item_tags[tag][stack.item as usize]
}

fn is_item(reg: &Registry, stack: &ItemStack, key: &str) -> bool {
    stack.count > 0 && reg.items[stack.item as usize].key == key
}

// Ingredient.test: the item is one the ingredient accepts.
fn accepts(ingredient: &[u16], stack: &ItemStack) -> bool {
    stack.count > 0 && ingredient.contains(&stack.item)
}

// ShapedRecipePattern.matches. A pattern that is not symmetrical is also
// tried mirrored; for a symmetrical one the mirror is the same.
fn shaped_matches(input: &Input, width: usize, height: usize, cells: &[Option<Vec<u16>>]) -> bool {
    let ingredient_count = cells.iter().flatten().count();
    if input.ingredient_count != ingredient_count || input.width != width || input.height != height {
        return false;
    }
    [true, false].into_iter().any(|mirrored| {
        (0..height).all(|y| {
            (0..width).all(|x| {
                let cell = &cells[if mirrored { width - x - 1 + y * width } else { x + y * width }];
                let stack = input.at(x, y);
                match cell {
                    Some(accepted) => accepts(accepted, stack),
                    None => stack.count <= 0,
                }
            })
        })
    })
}

// Whether every ingredient can be given its own stack of the input (what
// StackedContents.canCraft decides): a matching, by augmenting paths.
fn can_craft(ingredients: &[Vec<u16>], items: &[u16]) -> bool {
    fn augment(i: usize, ingredients: &[Vec<u16>], items: &[u16], seen: &mut [bool], taken: &mut [Option<usize>]) -> bool {
        for (j, item) in items.iter().enumerate() {
            if ingredients[i].contains(item) && !seen[j] {
                seen[j] = true;
                if taken[j].is_none() || augment(taken[j].unwrap(), ingredients, items, seen, taken) {
                    taken[j] = Some(i);
                    return true;
                }
            }
        }
        false
    }
    let mut taken = vec![None; items.len()];
    (0..ingredients.len()).all(|i| augment(i, ingredients, items, &mut vec![false; items.len()], &mut taken))
}

// ShapelessRecipe.matches.
fn shapeless_matches(input: &Input, ingredients: &[Vec<u16>]) -> bool {
    if input.ingredient_count != ingredients.len() {
        return false;
    }
    if input.items.len() == 1 && ingredients.len() == 1 {
        return accepts(&ingredients[0], &input.items[0]);
    }
    let items: Vec<u16> = input.present().map(|s| s.item).collect();
    can_craft(ingredients, &items)
}

// TransmuteRecipe.matches. The result is what the recipe makes of the input
// ingredient; one that leaves it as it is (the same item and components in a
// count of one) does not match.
fn transmute_matches(reg: &Registry, input: &Input, ingredients: &[Vec<u16>], result: &Option<ItemStack>) -> bool {
    if input.ingredient_count != 2 {
        return false;
    }
    let (mut flag, mut flag1) = (false, false);
    for stack in input.present() {
        if !flag && accepts(&ingredients[0], stack) {
            if result_unchanged(reg, result, stack) {
                return false;
            }
            flag = true;
        } else {
            if flag1 || !accepts(&ingredients[1], stack) {
                return false;
            }
            flag1 = true;
        }
    }
    flag && flag1
}

// TransmuteResult.apply: the copy of the stack with the result's item and
// count, and the component changes applied.
fn transmute_apply(reg: &Registry, result: &Option<ItemStack>, stack: &ItemStack) -> ItemStack {
    let Some(result) = result else { refuse("a transmute recipe whose result has a data component this port does not model") };
    let mut copy = stack.transmute_copy(reg, result.item, result.count);
    for (key, value) in result.patch.entries() {
        match value {
            Some(value) => copy.set_component(reg, key, value.clone()),
            None => copy.remove_component(reg, key),
        }
    }
    copy
}

fn result_unchanged(reg: &Registry, result: &Option<ItemStack>, stack: &ItemStack) -> bool {
    let applied = transmute_apply(reg, result, stack);
    applied.count == 1 && super::container::same_item_same_components(stack, &applied)
}

// The recipe the grid makes (RecipeManager.getRecipeFor: the last one that
// matches, in the recipe manager's order), as its index among the registry's
// recipes.
pub fn find(reg: &Registry, input: &Input) -> Option<usize> {
    // RecipeCache.get: an empty input has no recipe without reading any.
    if input.is_empty() {
        return None;
    }
    let mut found = None;
    for (index, recipe) in reg.recipes.iter().enumerate() {
        let matched = match recipe {
            Recipe::Shaped { width, height, cells, .. } => shaped_matches(input, *width, *height, cells),
            Recipe::Shapeless { ingredients, .. } => shapeless_matches(input, ingredients),
            Recipe::Transmute { ingredients, result, .. } => transmute_matches(reg, input, ingredients, result),
            Recipe::Other { class, .. } => custom_matches(reg, class, input),
        };
        if matched {
            found = Some(index);
        }
    }
    found
}

// Recipe.assemble: the item the recipe makes of the input (an empty stack
// where the recipe gives none).
pub fn assemble(reg: &Registry, recipe: usize, input: &Input) -> ItemStack {
    match &reg.recipes[recipe] {
        Recipe::Shaped { result, .. } | Recipe::Shapeless { result, .. } => match result {
            Some(result) => result.clone(),
            None => refuse(format!("the result of the recipe {} (a data component this port does not model)", reg.recipes[recipe].id())),
        },
        Recipe::Transmute { ingredients, result, .. } => {
            for stack in &input.items {
                if stack.count > 0 && accepts(&ingredients[0], stack) {
                    return transmute_apply(reg, result, stack);
                }
            }
            ItemStack::empty()
        }
        Recipe::Other { class, id, .. } => custom_assemble(reg, class, id, input),
    }
}

// CraftingRecipe.getRemainingItems: one stack per cell of the input.
pub fn remaining_items(reg: &Registry, recipe: usize, input: &Input) -> Vec<ItemStack> {
    if let Recipe::Other { class, .. } = &reg.recipes[recipe] {
        match class.as_str() {
            "BannerDuplicateRecipe" => return banner_duplicate_remaining(reg, input),
            "BookCloningRecipe" => return book_cloning_remaining(reg, input),
            _ => {}
        }
    }
    default_remaining(reg, input)
}

// CraftingRecipe.defaultCraftingReminder: each item's crafting remainder.
fn default_remaining(reg: &Registry, input: &Input) -> Vec<ItemStack> {
    input.items.iter().map(|stack| crafting_remainder(reg, stack)).collect()
}

// stack.getItem().getCraftingRemainder(): by item, so for an empty stack
// (air) none.
fn crafting_remainder(reg: &Registry, stack: &ItemStack) -> ItemStack {
    let item = if stack.count <= 0 { 0 } else { stack.item };
    match reg.remainders.get(&item) {
        Some(&(remainder, count)) => ItemStack::new(remainder, count),
        None => ItemStack::empty(),
    }
}

// --- CustomRecipe classes ---

// The dye color of a DyeItem, by its item (the color's name).
fn dye_color(reg: &Registry, stack: &ItemStack) -> Option<usize> {
    if stack.count <= 0 {
        return None;
    }
    let key = &reg.items[stack.item as usize].key;
    let name = key.strip_prefix("minecraft:")?.strip_suffix("_dye")?;
    DYES.iter().position(|dye| dye.0 == name)
}

// DyeColor: name, textureDiffuseColor (opaque) and fireworkColor.
const DYES: [(&str, i32, i32); 16] = [
    ("white", 16383998, 15790320),
    ("orange", 16351261, 15435844),
    ("magenta", 13061821, 12801229),
    ("light_blue", 3847130, 6719955),
    ("yellow", 16701501, 14602026),
    ("lime", 8439583, 4312372),
    ("pink", 15961002, 14188952),
    ("gray", 4673362, 4408131),
    ("light_gray", 10329495, 11250603),
    ("cyan", 1481884, 2651799),
    ("purple", 8991416, 8073150),
    ("blue", 3949738, 2437522),
    ("brown", 8606770, 5320730),
    ("green", 6192150, 3887386),
    ("red", 11546150, 11743532),
    ("black", 1908001, 1973019),
];

// The color of a BannerItem (a banner that is not a wall banner has an item).
fn banner_color(reg: &Registry, stack: &ItemStack) -> Option<usize> {
    if stack.count <= 0 {
        return None;
    }
    let key = &reg.items[stack.item as usize].key;
    let name = key.strip_prefix("minecraft:")?.strip_suffix("_banner")?;
    DYES.iter().position(|dye| dye.0 == name)
}

// The number of layers of a stack's banner patterns (empty when it has the
// component not).
fn banner_layers(reg: &Registry, stack: &ItemStack) -> usize {
    match stack.component(reg, "minecraft:banner_patterns") {
        Some(Tag::List(layers)) => layers.len(),
        Some(_) => unreachable!("banner patterns are a list"),
        None => 0,
    }
}

fn has_component(reg: &Registry, stack: &ItemStack, key: &str) -> bool {
    stack.component(reg, key).is_some()
}

// The class's matches(). A class this port does not know refuses when it
// might match (every custom recipe fails for fewer than two items... except
// where a class is ported).
fn custom_matches(reg: &Registry, class: &str, input: &Input) -> bool {
    match class {
        "ArmorDyeRecipe" => {
            if input.ingredient_count < 2 {
                return false;
            }
            let (mut flag, mut flag1) = (false, false);
            for stack in input.present() {
                if item_in_tag(reg, stack, "minecraft:dyeable") {
                    if flag {
                        return false;
                    }
                    flag = true;
                } else {
                    if dye_color(reg, stack).is_none() {
                        return false;
                    }
                    flag1 = true;
                }
            }
            flag1 && flag
        }
        "BannerDuplicateRecipe" => {
            if input.ingredient_count != 2 {
                return false;
            }
            let mut color = None;
            let (mut flag, mut flag1) = (false, false);
            for stack in input.present() {
                let Some(banner) = banner_color(reg, stack) else { return false };
                match color {
                    None => color = Some(banner),
                    Some(c) if c != banner => return false,
                    Some(_) => {}
                }
                let size = banner_layers(reg, stack);
                if size > 6 {
                    return false;
                }
                if size > 0 {
                    if flag1 {
                        return false;
                    }
                    flag1 = true;
                } else {
                    if flag {
                        return false;
                    }
                    flag = true;
                }
            }
            flag1 && flag
        }
        "BookCloningRecipe" => {
            if input.ingredient_count < 2 {
                return false;
            }
            let (mut flag, mut flag1) = (false, false);
            for stack in input.present() {
                if has_component(reg, stack, "minecraft:written_book_content") {
                    if flag1 {
                        return false;
                    }
                    flag1 = true;
                } else {
                    if !item_in_tag(reg, stack, "minecraft:book_cloning_target") {
                        return false;
                    }
                    flag = true;
                }
            }
            flag1 && flag
        }
        "DecoratedPotRecipe" => {
            input.width == 3
                && input.height == 3
                && input.ingredient_count == 4
                && [(1, 0), (0, 1), (2, 1), (1, 2)].iter().all(|&(x, y)| item_in_tag(reg, input.at(x, y), "minecraft:decorated_pot_ingredients"))
        }
        "FireworkRocketRecipe" => {
            if input.ingredient_count < 2 {
                return false;
            }
            let mut flag = false;
            let mut gunpowder = 0;
            for stack in input.present() {
                if is_item(reg, stack, "minecraft:paper") {
                    if flag {
                        return false;
                    }
                    flag = true;
                } else if is_item(reg, stack, "minecraft:gunpowder") {
                    gunpowder += 1;
                    if gunpowder > 3 {
                        return false;
                    }
                } else if !is_item(reg, stack, "minecraft:firework_star") {
                    return false;
                }
            }
            flag && gunpowder >= 1
        }
        "FireworkStarRecipe" => {
            if input.ingredient_count < 2 {
                return false;
            }
            let (mut flag, mut flag1, mut flag2, mut flag3, mut flag4) = (false, false, false, false, false);
            for stack in input.present() {
                if firework_shape(reg, stack).is_some() {
                    if flag2 {
                        return false;
                    }
                    flag2 = true;
                } else if is_item(reg, stack, "minecraft:glowstone_dust") {
                    if flag4 {
                        return false;
                    }
                    flag4 = true;
                } else if is_item(reg, stack, "minecraft:diamond") {
                    if flag3 {
                        return false;
                    }
                    flag3 = true;
                } else if is_item(reg, stack, "minecraft:gunpowder") {
                    if flag {
                        return false;
                    }
                    flag = true;
                } else {
                    if dye_color(reg, stack).is_none() {
                        return false;
                    }
                    flag1 = true;
                }
            }
            flag && flag1
        }
        "FireworkStarFadeRecipe" => {
            if input.ingredient_count < 2 {
                return false;
            }
            let (mut flag, mut flag1) = (false, false);
            for stack in input.present() {
                if dye_color(reg, stack).is_some() {
                    flag = true;
                } else {
                    if !is_item(reg, stack, "minecraft:firework_star") {
                        return false;
                    }
                    if flag1 {
                        return false;
                    }
                    flag1 = true;
                }
            }
            flag1 && flag
        }
        "MapCloningRecipe" => {
            if input.ingredient_count < 2 {
                return false;
            }
            let (mut flag, mut flag1) = (false, false);
            for stack in input.present() {
                if has_component(reg, stack, "minecraft:map_id") {
                    if flag1 {
                        return false;
                    }
                    flag1 = true;
                } else {
                    if !is_item(reg, stack, "minecraft:map") {
                        return false;
                    }
                    flag = true;
                }
            }
            flag1 && flag
        }
        // The pattern of paper around a filled map, and the map found with its
        // saved data: a map has the map_id component, which no stack here has
        // (no stack of this port holds it), so no map is found.
        "MapExtendingRecipe" => {
            let pattern_matches = input.width == 3
                && input.height == 3
                && input.ingredient_count == 9
                && (0..9).all(|at| if at == 4 { is_item(reg, &input.items[at], "minecraft:filled_map") } else { is_item(reg, &input.items[at], "minecraft:paper") });
            if pattern_matches && input.present().any(|s| has_component(reg, s, "minecraft:map_id")) {
                refuse("a map extended in a crafter (map data)");
            }
            false
        }
        "RepairItemRecipe" => items_to_combine(reg, input).is_some(),
        "ShieldDecorationRecipe" => {
            if input.ingredient_count != 2 {
                return false;
            }
            let (mut flag, mut flag1) = (false, false);
            for stack in input.present() {
                if banner_color(reg, stack).is_some() {
                    if flag1 {
                        return false;
                    }
                    flag1 = true;
                } else {
                    if !is_item(reg, stack, "minecraft:shield") {
                        return false;
                    }
                    if flag {
                        return false;
                    }
                    if banner_layers(reg, stack) > 0 {
                        return false;
                    }
                    flag = true;
                }
            }
            flag && flag1
        }
        "TippedArrowRecipe" => {
            if input.width == 3 && input.height == 3 && input.ingredient_count == 9 {
                for y in 0..3 {
                    for x in 0..3 {
                        let stack = input.at(x, y);
                        if stack.count <= 0 {
                            return false;
                        }
                        if x == 1 && y == 1 {
                            if !is_item(reg, stack, "minecraft:lingering_potion") {
                                return false;
                            }
                        } else if !is_item(reg, stack, "minecraft:arrow") {
                            return false;
                        }
                    }
                }
                true
            } else {
                false
            }
        }
        other => refuse(format!("the crafting recipe class {other}")),
    }
}

// FireworkStarRecipe.SHAPE_BY_ITEM.
fn firework_shape(reg: &Registry, stack: &ItemStack) -> Option<&'static str> {
    if stack.count <= 0 {
        return None;
    }
    Some(match reg.items[stack.item as usize].key.as_str() {
        "minecraft:fire_charge" => "large_ball",
        "minecraft:feather" => "burst",
        "minecraft:gold_nugget" => "star",
        "minecraft:skeleton_skull" | "minecraft:wither_skeleton_skull" | "minecraft:creeper_head" | "minecraft:player_head" | "minecraft:dragon_head" | "minecraft:zombie_head" | "minecraft:piglin_head" => "creeper",
        _ => return None,
    })
}

// RepairItemRecipe.getItemsToCombine: the two stacks, if they combine.
fn items_to_combine<'a>(reg: &Registry, input: &'a Input) -> Option<(&'a ItemStack, &'a ItemStack)> {
    if input.ingredient_count != 2 {
        return None;
    }
    let mut first: Option<&ItemStack> = None;
    for stack in input.present() {
        if let Some(first) = first {
            return if can_combine(reg, first, stack) { Some((first, stack)) } else { None };
        }
        first = Some(stack);
    }
    None
}

fn can_combine(reg: &Registry, a: &ItemStack, b: &ItemStack) -> bool {
    b.item == a.item && a.count == 1 && b.count == 1 && ["minecraft:max_damage", "minecraft:damage"].iter().all(|key| has_component(reg, a, key) && has_component(reg, b, key))
}

fn int_component(reg: &Registry, stack: &ItemStack, key: &str) -> i32 {
    match stack.component(reg, key) {
        Some(Tag::Int(v)) => v,
        other => panic!("{key} is an int: {other:?}"),
    }
}

fn custom_assemble(reg: &Registry, class: &str, id: &str, input: &Input) -> ItemStack {
    match class {
        "ArmorDyeRecipe" => {
            let mut dyes = Vec::new();
            let mut target: Option<&ItemStack> = None;
            for stack in input.present() {
                if item_in_tag(reg, stack, "minecraft:dyeable") {
                    if target.is_some() {
                        return ItemStack::empty();
                    }
                    target = Some(stack);
                } else {
                    match dye_color(reg, stack) {
                        Some(color) => dyes.push(color),
                        None => return ItemStack::empty(),
                    }
                }
            }
            match target {
                Some(target) if !dyes.is_empty() => apply_dyes(reg, target, &dyes),
                _ => ItemStack::empty(),
            }
        }
        "BannerDuplicateRecipe" => {
            for stack in input.present() {
                let size = banner_layers(reg, stack);
                if size > 0 && size <= 6 {
                    return copy_stack_with_count(stack, 1);
                }
            }
            ItemStack::empty()
        }
        "BookCloningRecipe" => {
            let mut count = 0;
            let mut book: Option<&ItemStack> = None;
            for stack in input.present() {
                if has_component(reg, stack, "minecraft:written_book_content") {
                    if book.is_some() {
                        return ItemStack::empty();
                    }
                    book = Some(stack);
                } else {
                    if !item_in_tag(reg, stack, "minecraft:book_cloning_target") {
                        return ItemStack::empty();
                    }
                    count += 1;
                }
            }
            let Some(book) = book else { return ItemStack::empty() };
            if count < 1 {
                return ItemStack::empty();
            }
            // WrittenBookContent.tryCraftCopy: a generation of two or more
            // cannot be copied; else the generation goes up by one.
            let content = book.component(reg, "minecraft:written_book_content").expect("a written book has its content");
            let Tag::Compound(mut content) = content else { unreachable!() };
            let generation = match content.get("generation") {
                Some(Tag::Int(g)) => *g,
                None => 0,
                Some(_) => unreachable!("the generation is an int"),
            };
            if generation >= 2 {
                return ItemStack::empty();
            }
            content.put("generation", Tag::Int(generation + 1));
            let mut copy = copy_stack_with_count(book, count);
            copy.set_component(reg, "minecraft:written_book_content", Tag::Compound(content));
            copy
        }
        "DecoratedPotRecipe" => {
            // DecoratedPotBlockEntity.createDecoratedPotItem.
            let keys: Vec<Tag> = [(1, 0), (0, 1), (2, 1), (1, 2)].iter().map(|&(x, y)| Tag::string(&reg.items[input.at(x, y).item as usize].key)).collect();
            decorated_pot_item(reg, keys)
        }
        "FireworkRocketRecipe" => {
            let mut explosions = Vec::new();
            let mut gunpowder = 0;
            for stack in input.present() {
                if is_item(reg, stack, "minecraft:gunpowder") {
                    gunpowder += 1;
                } else if is_item(reg, stack, "minecraft:firework_star") {
                    if let Some(explosion) = stack.component(reg, "minecraft:firework_explosion") {
                        explosions.push(explosion);
                    }
                }
            }
            if explosions.len() > 256 {
                refuse("a firework rocket with more than 256 explosions (an exception)");
            }
            let mut rocket = ItemStack::new(reg.item_by_key["minecraft:firework_rocket"], 3);
            let mut fireworks = crate::nbt::Compound::new();
            // UNSIGNED_BYTE encoder: more than 255 is an error (at most three gunpowder here).
            if gunpowder != 0 {
                fireworks.put("flight_duration", Tag::Byte(gunpowder as i8));
            }
            if !explosions.is_empty() {
                fireworks.put("explosions", Tag::List(explosions));
            }
            rocket.set_component(reg, "minecraft:fireworks", Tag::Compound(fireworks));
            rocket
        }
        "FireworkStarRecipe" => {
            let mut shape = "small_ball";
            let (mut twinkle, mut trail) = (false, false);
            let mut colors = Vec::new();
            for stack in input.present() {
                if let Some(s) = firework_shape(reg, stack) {
                    shape = s;
                } else if is_item(reg, stack, "minecraft:glowstone_dust") {
                    twinkle = true;
                } else if is_item(reg, stack, "minecraft:diamond") {
                    trail = true;
                } else if let Some(color) = dye_color(reg, stack) {
                    colors.push(Tag::Int(DYES[color].2));
                }
            }
            let mut explosion = crate::nbt::Compound::new();
            explosion.put("shape", Tag::string(shape));
            if !colors.is_empty() {
                explosion.put("colors", Tag::List(colors));
            }
            if trail {
                explosion.put("has_trail", Tag::Byte(1));
            }
            if twinkle {
                explosion.put("has_twinkle", Tag::Byte(1));
            }
            let mut star = ItemStack::new(reg.item_by_key["minecraft:firework_star"], 1);
            star.set_component(reg, "minecraft:firework_explosion", Tag::Compound(explosion));
            star
        }
        "FireworkStarFadeRecipe" => {
            let mut colors = Vec::new();
            let mut star: Option<ItemStack> = None;
            for stack in &input.items {
                if let Some(color) = dye_color(reg, stack) {
                    colors.push(Tag::Int(DYES[color].2));
                } else if is_item(reg, stack, "minecraft:firework_star") {
                    star = Some(copy_stack_with_count(stack, 1));
                }
            }
            match star {
                Some(mut star) if !colors.is_empty() => {
                    // update(FIREWORK_EXPLOSION, DEFAULT, colors, withFadeColors).
                    let mut explosion = match star.component(reg, "minecraft:firework_explosion") {
                        Some(Tag::Compound(c)) => c,
                        None => {
                            let mut c = crate::nbt::Compound::new();
                            c.put("shape", Tag::string("small_ball"));
                            c
                        }
                        Some(_) => unreachable!(),
                    };
                    explosion.put("fade_colors", Tag::List(colors));
                    star.set_component(reg, "minecraft:firework_explosion", Tag::Compound(explosion));
                    star
                }
                _ => ItemStack::empty(),
            }
        }
        "MapCloningRecipe" => {
            let mut count = 0;
            let mut map: Option<&ItemStack> = None;
            for stack in input.present() {
                if has_component(reg, stack, "minecraft:map_id") {
                    if map.is_some() {
                        return ItemStack::empty();
                    }
                    map = Some(stack);
                } else {
                    if !is_item(reg, stack, "minecraft:map") {
                        return ItemStack::empty();
                    }
                    count += 1;
                }
            }
            match map {
                Some(map) if count >= 1 => copy_stack_with_count(map, count + 1),
                _ => ItemStack::empty(),
            }
        }
        "RepairItemRecipe" => {
            let Some((a, b)) = items_to_combine(reg, input) else { return ItemStack::empty() };
            let (a_max, b_max) = (int_component(reg, a, "minecraft:max_damage"), int_component(reg, b, "minecraft:max_damage"));
            let (a_damage, b_damage) = (int_component(reg, a, "minecraft:damage"), int_component(reg, b, "minecraft:damage"));
            let max = a_max.max(b_max);
            let remaining = (a_max - a_damage) + (b_max - b_damage) + max * 5 / 100;
            let mut result = ItemStack::new(a.item, 1);
            result.set_component(reg, "minecraft:max_damage", Tag::Int(max));
            // setDamageValue: Mth.clamp(damage, 0, getMaxDamage()).
            let damage = (max - remaining).max(0).clamp(0, max);
            result.set_component(reg, "minecraft:damage", Tag::Int(damage));
            // The curses of both, at the higher level.
            let key = if is_item(reg, a, "minecraft:enchanted_book") { "minecraft:stored_enchantments" } else { "minecraft:enchantments" };
            let level_of = |stack: &ItemStack, curse: &str| -> i32 {
                match stack.component(reg, key) {
                    Some(Tag::Compound(c)) => match c.get(curse) {
                        Some(Tag::Int(level)) => *level,
                        _ => 0,
                    },
                    _ => 0,
                }
            };
            let mut curses = crate::nbt::Compound::new();
            for curse in reg.enchantment_curses.iter() {
                let level = level_of(a, curse).max(level_of(b, curse));
                if level > 0 {
                    curses.put(curse, Tag::Int(level.min(255)));
                }
            }
            // updateEnchantments: the new item's enchantments (it has the
            // component) with the curses upgraded to those levels, set back.
            result.set_component(reg, key, Tag::Compound(curses));
            result
        }
        "ShieldDecorationRecipe" => {
            let mut banner: Option<&ItemStack> = None;
            let mut shield: Option<ItemStack> = None;
            for stack in input.present() {
                if banner_color(reg, stack).is_some() {
                    banner = Some(stack);
                } else if is_item(reg, stack, "minecraft:shield") {
                    shield = Some(stack.clone());
                }
            }
            let (Some(banner), Some(mut shield)) = (banner, shield) else { return ItemStack::empty() };
            // set(BANNER_PATTERNS, banner.get(BANNER_PATTERNS)): the banner's
            // layers, none being modeled; set(BASE_COLOR, the banner's color).
            let patterns = banner.component(reg, "minecraft:banner_patterns").expect("a banner holds its patterns");
            shield.set_component(reg, "minecraft:banner_patterns", patterns);
            let color = banner_color(reg, banner).expect("a banner has a color");
            shield.set_component(reg, "minecraft:base_color", Tag::string(DYES[color].0));
            shield
        }
        "TippedArrowRecipe" => {
            let potion = input.at(1, 1);
            if !is_item(reg, potion, "minecraft:lingering_potion") {
                return ItemStack::empty();
            }
            let mut arrow = ItemStack::new(reg.item_by_key["minecraft:tipped_arrow"], 8);
            if let Some(contents) = potion.component(reg, "minecraft:potion_contents") {
                arrow.set_component(reg, "minecraft:potion_contents", contents);
            }
            arrow
        }
        _ => refuse(format!("assembling the recipe {id}")),
    }
}

fn copy_stack_with_count(stack: &ItemStack, count: i32) -> ItemStack {
    ItemStack { item: stack.item, count, patch: stack.patch.clone() }
}

// DyedItemColor.applyDyes.
fn apply_dyes(reg: &Registry, stack: &ItemStack, dyes: &[usize]) -> ItemStack {
    let mut result = copy_stack_with_count(stack, 1);
    let (mut r, mut g, mut b, mut total, mut n) = (0i32, 0i32, 0i32, 0i32, 0i32);
    if let Some(Tag::Int(rgb)) = stack.component(reg, "minecraft:dyed_color") {
        let (red, green, blue) = ((rgb >> 16) & 255, (rgb >> 8) & 255, rgb & 255);
        total += red.max(green.max(blue));
        r += red;
        g += green;
        b += blue;
        n += 1;
    }
    for &dye in dyes {
        let color = DYES[dye].1;
        let (red, green, blue) = ((color >> 16) & 255, (color >> 8) & 255, color & 255);
        total += red.max(green.max(blue));
        r += red;
        g += green;
        b += blue;
        n += 1;
    }
    let (mut i5, mut i6, mut i7) = (r / n, g / n, b / n);
    let f = total as f32 / n as f32;
    let f1 = i5.max(i6.max(i7)) as f32;
    i5 = (i5 as f32 * f / f1) as i32;
    i6 = (i6 as f32 * f / f1) as i32;
    i7 = (i7 as f32 * f / f1) as i32;
    // ARGB.color(0, r, g, b).
    let color = (i5 << 16) | (i6 << 8) | i7;
    result.set_component(reg, "minecraft:dyed_color", Tag::Int(color));
    result
}

fn decorated_pot_item(reg: &Registry, decorations: Vec<Tag>) -> ItemStack {
    let mut pot = ItemStack::new(reg.item_by_key["minecraft:decorated_pot"], 1);
    pot.set_component(reg, "minecraft:pot_decorations", Tag::List(decorations));
    pot
}

// BannerDuplicateRecipe.getRemainingItems.
fn banner_duplicate_remaining(reg: &Registry, input: &Input) -> Vec<ItemStack> {
    input
        .items
        .iter()
        .map(|stack| {
            if stack.count <= 0 {
                return ItemStack::empty();
            }
            let remainder = crafting_remainder(reg, stack);
            if remainder.count > 0 {
                remainder
            } else if banner_layers(reg, stack) > 0 {
                copy_stack_with_count(stack, 1)
            } else {
                ItemStack::empty()
            }
        })
        .collect()
}

// BookCloningRecipe.getRemainingItems.
fn book_cloning_remaining(reg: &Registry, input: &Input) -> Vec<ItemStack> {
    let mut list = vec![ItemStack::empty(); input.items.len()];
    for (at, stack) in input.items.iter().enumerate() {
        let remainder = crafting_remainder(reg, stack);
        if remainder.count > 0 {
            list[at] = remainder;
        } else if stack.count > 0 && has_component(reg, stack, "minecraft:written_book_content") {
            list[at] = copy_stack_with_count(stack, 1);
            break;
        }
    }
    list
}

// Used by the crafter to make a stack object of a value.
pub fn to_stack(stack: ItemStack) -> Stack {
    super::container::stack_of(stack)
}

#[allow(dead_code)]
fn unused(stack: &Stack) -> Stack {
    copy_with_count(stack, 1)
}
