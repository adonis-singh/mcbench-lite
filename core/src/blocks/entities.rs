// The block entities of the redstone-adjacent blocks (lectern, furnaces,
// brewing stand, jukebox, chiseled bookshelf, shelf, decorated pot, bell,
// daylight detector). A
// block placed by a command has no NBT, so its entity starts with empty
// slots; saved data (blocks/load.rs) can fill them. What an entity does with
// its contents is ported where the contents need no data this port lacks
// (the recipes, the brewing rules, the songs); code that would need it
// refuses when the contents reach it.

use super::bell::Bell;
use super::container::{empty_slots_n, empty_stack, is_empty, save_all_items, Stack};
use crate::dir::Pos;
use crate::level::Level;
use crate::nbt::{Compound, Tag};
use crate::registry::{Registry, State};

#[derive(Clone, Debug)]
pub enum Entity {
    // LecternBlockEntity.
    Lectern(Lectern),
    // FurnaceBlockEntity, BlastFurnaceBlockEntity and SmokerBlockEntity.
    Furnace(FurnaceKind, Furnace),
    // BrewingStandBlockEntity; lastPotionCount (null until the first tick)
    // decides the state it sets.
    BrewingStand(BrewingStand),
    // JukeboxBlockEntity.
    Jukebox(Jukebox),
    // ChiseledBookShelfBlockEntity.
    ChiseledBookShelf(Shelf),
    // ShelfBlockEntity, a ListBackedContainer of three slots.
    Shelf(ItemShelf),
    // DecoratedPotBlockEntity, a ContainerSingleItem.
    DecoratedPot(Pot),
    Bell(Bell),
    // DaylightDetectorBlockEntity.
    DaylightDetector,
}

// LecternBlockEntity's book (ItemStack.EMPTY without one), page and
// pageCount. A lectern made by a command has page 0 and pageCount 0; one
// loaded from saved data has the page clamped to the book's pages.
#[derive(Clone, Debug)]
pub struct Lectern {
    pub book: Stack,
    pub page: i32,
    pub page_count: i32,
}

impl Lectern {
    pub fn new() -> Lectern {
        Lectern { book: empty_stack(), page: 0, page_count: 0 }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FurnaceKind {
    Furnace,
    BlastFurnace,
    Smoker,
}

// AbstractFurnaceBlockEntity's saved fields: the three slots (ingredient,
// fuel, result), cookingTimer, cookingTotalTime, litTimeRemaining and
// litTotalTime (read and written as shorts, held as ints), the recipes used
// with their counts and the Paper cook speed multiplier.
#[derive(Clone, Debug)]
pub struct Furnace {
    pub items: Vec<Stack>,
    pub name: Option<Tag>,
    pub lock: Option<Tag>,
    pub cooking_timer: i32,
    pub cooking_total_time: i32,
    pub lit_time_remaining: i32,
    pub lit_total_time: i32,
    pub recipes_used: Vec<(String, i32)>,
    pub cook_speed_multiplier: f64,
    // The cooking recipe the furnace's quickCheck last found (RecipeManager.createCheck
    // remembers it, and uses it first while it still matches), as its index among
    // the registry's cooking recipes. Not saved.
    pub last_recipe: Option<usize>,
}

impl Furnace {
    pub fn new() -> Furnace {
        Furnace {
            items: empty_slots_n(3),
            name: None,
            lock: None,
            cooking_timer: 0,
            cooking_total_time: 0,
            lit_time_remaining: 0,
            lit_total_time: 0,
            recipes_used: Vec::new(),
            cook_speed_multiplier: 1.0,
            last_recipe: None,
        }
    }
}

// BrewingStandBlockEntity: the five slots (three bottles, the ingredient, the
// fuel), brewTime, fuel, and lastPotionCount. The ingredient it brews is only
// read to finish a brew, which needs the brewing recipes.
#[derive(Clone, Debug)]
pub struct BrewingStand {
    pub items: Vec<Stack>,
    pub name: Option<Tag>,
    pub lock: Option<Tag>,
    pub brew_time: i32,
    pub fuel: i32,
    pub last_potion_count: Option<[bool; 3]>,
    // The item of the ingredient the current brew started with (null until
    // one starts, or a stand with brew time that was loaded).
    pub ingredient: Option<u16>,
}

impl BrewingStand {
    pub fn new() -> BrewingStand {
        BrewingStand { items: empty_slots_n(5), name: None, lock: None, brew_time: 0, fuel: 0, last_potion_count: None, ingredient: None }
    }
}

// JukeboxBlockEntity: the record it holds (a one slot container), the song
// its player plays (the song's key) and how long it has played.
#[derive(Clone, Debug)]
pub struct Jukebox {
    pub items: Vec<Stack>,
    pub song: Option<String>,
    pub ticks_since_song_started: i64,
}

impl Jukebox {
    pub fn new() -> Jukebox {
        Jukebox { items: empty_slots_n(1), song: None, ticks_since_song_started: 0 }
    }
}

// ChiseledBookShelfBlockEntity: the six slots and lastInteractedSlot.
#[derive(Clone, Debug)]
pub struct Shelf {
    pub items: Vec<Stack>,
    pub last_interacted_slot: i32,
}

impl Shelf {
    pub fn new() -> Shelf {
        Shelf { items: empty_slots_n(6), last_interacted_slot: -1 }
    }
}

// ShelfBlockEntity: the three slots and alignItemsToBottom.
#[derive(Clone, Debug)]
pub struct ItemShelf {
    pub items: Vec<Stack>,
    pub align_items_to_bottom: bool,
}

impl ItemShelf {
    pub fn new() -> ItemShelf {
        ItemShelf { items: empty_slots_n(3), align_items_to_bottom: false }
    }
}

// DecoratedPotBlockEntity: its item (the one slot, ItemStack.EMPTY without
// one; a loot table is not modeled) and its four sherds (item keys, ordered
// back, left, right, front, bricks for none).
#[derive(Clone, Debug)]
pub struct Pot {
    pub items: Vec<Stack>,
    pub sherds: Vec<String>,
}

impl Pot {
    pub fn new() -> Pot {
        Pot { items: empty_slots_n(1), sherds: vec!["minecraft:brick".to_string(); 4] }
    }
}

impl Entity {
    // Container.getContainerSize, or none for an entity that is no Container.
    pub fn container_size(&self) -> Option<usize> {
        match self {
            Entity::Lectern(_) | Entity::Bell(_) | Entity::DaylightDetector => None,
            Entity::Furnace(..) => Some(3),
            Entity::BrewingStand(_) => Some(5),
            Entity::Jukebox(_) => Some(1),
            Entity::ChiseledBookShelf(_) => Some(6),
            Entity::Shelf(_) => Some(3),
            Entity::DecoratedPot(_) => Some(1),
        }
    }

    // The slots of an entity that holds them in a list.
    pub fn items(&self) -> Option<&Vec<Stack>> {
        match self {
            Entity::Furnace(_, furnace) => Some(&furnace.items),
            Entity::BrewingStand(stand) => Some(&stand.items),
            Entity::Jukebox(jukebox) => Some(&jukebox.items),
            Entity::ChiseledBookShelf(shelf) => Some(&shelf.items),
            Entity::Shelf(shelf) => Some(&shelf.items),
            Entity::DecoratedPot(pot) => Some(&pot.items),
            _ => None,
        }
    }

    pub fn items_mut(&mut self) -> Option<&mut Vec<Stack>> {
        match self {
            Entity::Furnace(_, furnace) => Some(&mut furnace.items),
            Entity::BrewingStand(stand) => Some(&mut stand.items),
            Entity::Jukebox(jukebox) => Some(&mut jukebox.items),
            Entity::ChiseledBookShelf(shelf) => Some(&mut shelf.items),
            Entity::Shelf(shelf) => Some(&mut shelf.items),
            Entity::DecoratedPot(pot) => Some(&mut pot.items),
            _ => None,
        }
    }

    // Container.getMaxStackSize: the chiseled bookshelf holds one of an item
    // in a slot, the others 99 (Paper's maxStack of the shelf and the pot).
    pub fn max_stack(&self) -> i32 {
        match self {
            Entity::ChiseledBookShelf(_) => 1,
            _ => super::container::MAX_STACK,
        }
    }

    // The class of the one kind of block the entity's type is valid for
    // (BlockEntityType.isValid).
    fn block_class(&self) -> &'static str {
        match self {
            Entity::Lectern(_) => "LecternBlock",
            Entity::Furnace(FurnaceKind::Furnace, _) => "FurnaceBlock",
            Entity::Furnace(FurnaceKind::BlastFurnace, _) => "BlastFurnaceBlock",
            Entity::Furnace(FurnaceKind::Smoker, _) => "SmokerBlock",
            Entity::BrewingStand(_) => "BrewingStandBlock",
            Entity::Jukebox(_) => "JukeboxBlock",
            Entity::ChiseledBookShelf(_) => "ChiseledBookShelfBlock",
            Entity::Shelf(_) => "ShelfBlock",
            Entity::DecoratedPot(_) => "DecoratedPotBlock",
            Entity::Bell(_) => "BellBlock",
            Entity::DaylightDetector => "DaylightDetectorBlock",
        }
    }
}

// BlockEntityType.isValid.
pub fn valid(l: &Level, entity: &Entity, state: State) -> bool {
    l.reg.blocks[l.block(state) as usize].class == entity.block_class()
}

// The entity's type key and saved data (see BlockEntity::saved).
pub fn saved(reg: &Registry, entity: &Entity) -> (&'static str, Compound) {
    let mut out = Compound::new();
    let key = match entity {
        Entity::Lectern(lectern) => {
            // LecternBlockEntity.saveAdditional: a book and the page.
            if !is_empty(&lectern.book) {
                out.put("Book", Tag::Compound(crate::components::stack_fields(reg, &lectern.book.borrow()).unwrap_or_else(|f| crate::level::refuse(format!("saving a book: {}", f.0)))));
                out.put_int("Page", lectern.page);
            }
            "minecraft:lectern"
        }
        Entity::Furnace(kind, furnace) => {
            out.put("cooking_time_spent", Tag::Short(furnace.cooking_timer as i16));
            out.put("cooking_total_time", Tag::Short(furnace.cooking_total_time as i16));
            out.put("lit_time_remaining", Tag::Short(furnace.lit_time_remaining as i16));
            out.put("lit_total_time", Tag::Short(furnace.lit_total_time as i16));
            out.put("Paper.CookSpeedMultiplier", Tag::double(furnace.cook_speed_multiplier));
            save_all_items(reg, &furnace.items, true, &mut out);
            let mut recipes = Compound::new();
            for (recipe, count) in &furnace.recipes_used {
                recipes.put(recipe, Tag::Int(*count));
            }
            out.put("RecipesUsed", Tag::Compound(recipes));
            if let Some(name) = &furnace.name {
                out.put("CustomName", name.clone());
            }
            if let Some(lock) = &furnace.lock {
                out.put("lock", lock.clone());
            }
            match kind {
                FurnaceKind::Furnace => "minecraft:furnace",
                FurnaceKind::BlastFurnace => "minecraft:blast_furnace",
                FurnaceKind::Smoker => "minecraft:smoker",
            }
        }
        Entity::BrewingStand(stand) => {
            out.put("BrewTime", Tag::Short(stand.brew_time as i16));
            save_all_items(reg, &stand.items, true, &mut out);
            out.put("Fuel", Tag::Byte(stand.fuel as i8));
            if let Some(name) = &stand.name {
                out.put("CustomName", name.clone());
            }
            if let Some(lock) = &stand.lock {
                out.put("lock", lock.clone());
            }
            "minecraft:brewing_stand"
        }
        Entity::Jukebox(jukebox) => {
            // JukeboxBlockEntity.saveAdditional.
            if !is_empty(&jukebox.items[0]) {
                out.put("RecordItem", Tag::Compound(crate::components::stack_fields(reg, &jukebox.items[0].borrow()).unwrap_or_else(|f| crate::level::refuse(format!("saving a record: {}", f.0)))));
            }
            if jukebox.song.is_some() {
                out.put("ticks_since_song_started", Tag::Long(jukebox.ticks_since_song_started));
            }
            "minecraft:jukebox"
        }
        Entity::ChiseledBookShelf(shelf) => {
            save_all_items(reg, &shelf.items, true, &mut out);
            out.put_int("last_interacted_slot", shelf.last_interacted_slot);
            "minecraft:chiseled_bookshelf"
        }
        // ShelfBlockEntity.saveAdditional: every slot list, empty or not.
        Entity::Shelf(shelf) => {
            save_all_items(reg, &shelf.items, true, &mut out);
            out.put_boolean("align_items_to_bottom", shelf.align_items_to_bottom);
            "minecraft:shelf"
        }
        // DecoratedPotBlockEntity.saveAdditional: the sherds unless they are
        // four bricks (PotDecorations.EMPTY), the item unless it is empty.
        Entity::DecoratedPot(pot) => {
            if pot.sherds.iter().any(|sherd| sherd != "minecraft:brick") {
                out.put("sherds", Tag::List(pot.sherds.iter().map(|sherd| Tag::string(sherd)).collect()));
            }
            if !is_empty(&pot.items[0]) {
                out.put("item", Tag::Compound(crate::components::stack_fields(reg, &pot.items[0].borrow()).unwrap_or_else(|f| crate::level::refuse(format!("saving a pot's item: {}", f.0)))));
            }
            "minecraft:decorated_pot"
        }
        Entity::Bell(_) => "minecraft:bell",
        Entity::DaylightDetector => "minecraft:daylight_detector",
    };
    out.put("components", Tag::Compound(Compound::new()));
    (key, out)
}

// BlockEntity.preRemoveSideEffects. A Container's contents drop
// (Containers.dropContents: dropItemStack draws the item's three position
// doubles from the level's random for every slot, empty or not). The jukebox
// pops out its item (none) and the lectern drops its book as an ItemEntity
// when the old state has one. A furnace that used recipes also gives their
// experience as orbs.
pub fn pre_remove_side_effects(l: &mut Level, entity: &Entity, pos: Pos, old: State) {
    match entity {
        // LecternBlockEntity.preRemoveSideEffects: with the old state having a
        // book, an item entity of a copy of the book (addFreshEntity rejects
        // one that is empty, after the constructor drew from the shared
        // random).
        Entity::Lectern(lectern) => {
            if l.reg.get_bool(old, l.reg.property_of(old, "has_book")) {
                let direction = l.reg.get_dir(old, l.reg.property_of(old, "facing"));
                let (dx, _, dz) = direction.offset();
                let (f, f1) = (0.25f32 * dx as f32, 0.25f32 * dz as f32);
                let (x, y, z) = (pos.x() as f64 + 0.5 + f as f64, (pos.y() + 1) as f64, pos.z() as f64 + 0.5 + f1 as f64);
                let book = super::container::copy(&lectern.book);
                if is_empty(&book) {
                    crate::entity::item::reject_empty_item(l, x, y, z);
                } else {
                    let stack = book.borrow().clone();
                    crate::entity::item::spawn_with_default_delay(l, x, y, z, stack);
                }
            }
        }
        // JukeboxBlockEntity.preRemoveSideEffects: popOutTheItem.
        Entity::Jukebox(_) => super::jukebox::pop_out_the_item(l, pos, old),
        Entity::Bell(_) | Entity::DaylightDetector => {}
        Entity::Furnace(_, furnace) => {
            for stack in &furnace.items {
                crate::entity::item::drop_item_stack(l, pos.x() as f64, pos.y() as f64, pos.z() as f64, stack);
            }
            // getRecipesToAwardAndPopExperience: the experience of each cooking
            // recipe used (createExperience: Mth.floor and Mth.frac of the uses
            // times the recipe's experience, BlockExpEvent with no listener), as
            // orbs at the block's center. A fraction is a chance taken with
            // Math.random, which no seed fixes; and the recipes are visited in
            // the order of a map keyed by identity hash, which differs from
            // run to run, so more than one recipe that gives orbs refuses.
            let mut awards = Vec::new();
            for (id, used) in &furnace.recipes_used {
                let Some(recipe) = l.reg.cooking.iter().find(|recipe| &recipe.id == id) else { continue };
                let product = *used as f32 * recipe.experience;
                let floor = if product < (product as i32) as f32 { product as i32 - 1 } else { product as i32 };
                let fraction = product - floor as f32;
                if fraction != 0.0f32 {
                    crate::level::refuse("a furnace removed with a fraction of experience to give (Math.random)");
                }
                if floor > 0 {
                    awards.push(floor);
                }
            }
            if awards.len() > 1 {
                crate::level::refuse("a furnace removed with experience of more than one recipe to give (identity hash order)");
            }
            for floor in awards {
                crate::entity::orb::award(l, crate::entity::orb::center_of(pos), floor, "FURNACE", None);
            }
        }
        Entity::BrewingStand(_) | Entity::ChiseledBookShelf(_) | Entity::Shelf(_) | Entity::DecoratedPot(_) => {
            for stack in entity.items().expect("a container") {
                crate::entity::item::drop_item_stack(l, pos.x() as f64, pos.y() as f64, pos.z() as f64, stack);
            }
        }
    }
}

// AbstractContainerMenu.getRedstoneSignalFromBlockEntity of a block whose
// signal reads its block entity: the entity (Level.getBlockEntity makes it if
// missing) if it is a Container.
pub fn container_signal(l: &mut Level, pos: Pos) -> i32 {
    let reg = l.reg;
    let Some(super::BlockEntity::Entity(entity)) = l.block_entity(pos) else { return 0 };
    let Some(items) = entity.items() else { return 0 };
    redstone_signal(reg, items, entity.max_stack())
}

// AbstractContainerMenu.getRedstoneSignalFromContainer.
pub fn redstone_signal(reg: &Registry, items: &[Stack], container_max: i32) -> i32 {
    let mut f = 0.0f32;
    for item in items {
        if !is_empty(item) {
            f += super::container::count(item) as f32 / container_max.min(super::container::max_stack(reg, item)) as f32;
        }
    }
    f /= items.len() as f32;
    super::container::lerp_discrete(f)
}

// SignText: the four lines (as the text components their codec writes), the
// filtered lines (a copy of the lines unless saved data gave others), the dye
// color's name and whether the text glows.
#[derive(Clone, Debug, PartialEq)]
pub struct SignText {
    pub messages: [Tag; 4],
    pub filtered: [Tag; 4],
    pub color: String,
    pub glowing: bool,
}

impl SignText {
    pub fn blank() -> SignText {
        let empty = || Tag::string("");
        SignText { messages: [empty(), empty(), empty(), empty()], filtered: [empty(), empty(), empty(), empty()], color: "black".into(), glowing: false }
    }

    // SignText.DIRECT_CODEC's encoder.
    fn encode(&self) -> Tag {
        let mut out = Compound::new();
        out.put("messages", Tag::List(self.messages.to_vec()));
        // filteredMessages(): none when they are the lines.
        if self.messages != self.filtered {
            out.put("filtered_messages", Tag::List(self.filtered.to_vec()));
        }
        out.put("color", Tag::string(&self.color));
        out.put_boolean("has_glowing_text", self.glowing);
        Tag::Compound(out)
    }
}

// SignBlockEntity and HangingSignBlockEntity.
#[derive(Clone, Debug)]
pub struct Sign {
    pub front: SignText,
    pub back: SignText,
    pub waxed: bool,
}

impl Sign {
    pub fn new() -> Sign {
        Sign { front: SignText::blank(), back: SignText::blank(), waxed: false }
    }

    pub fn saved(&self) -> Compound {
        let mut out = Compound::new();
        out.put("front_text", self.front.encode());
        out.put("back_text", self.back.encode());
        out.put_boolean("is_waxed", self.waxed);
        out.put("components", Tag::Compound(Compound::new()));
        out
    }
}
