// BlockEntity.loadWithComponents: a block entity's fields read from saved
// data (the compound the verifier imports), for each kind of block entity this
// port has. A field of the wrong type is a problem Minecraft reports (the
// import then fails), a value its codec rejects is an error: both are a Fail
// here, which the caller refuses. Data this port does not model (a loot
// table, a lock, a banner's patterns, a block entity's own data components,
// Bukkit's persistent data, a jukebox's record) is a Fail too.

use super::container::{empty_slots, stack_of, Container, Kind};
use super::entities::{BrewingStand, Entity, Furnace, ItemShelf, Jukebox, Lectern, Pot, Shelf, Sign, SignText};
use super::BlockEntity;
use crate::codec::{self, fail, items, map, required, Fail, Input, R};
use crate::components::decode_stack;
use crate::nbt::{Compound, Tag};
use crate::registry::Registry;

pub fn load(reg: &Registry, entity: &mut BlockEntity, tag: &Compound) -> R<super::Extras> {
    let input = Input(tag);
    // BlockEntity.loadAdditional: Bukkit's persistent data (a compound).
    let mut extras = super::Extras::default();
    if let Some(bukkit) = tag.get("PublicBukkitValues") {
        match bukkit {
            Tag::Compound(values) => extras.bukkit = values.clone(),
            _ => return fail("Not a compound tag"),
        }
    }
    match entity {
        BlockEntity::Comparator { output } => *output = input.int_or("OutputSignal", 0)?,
        // The special blocks' block entities (spawners, beacons, command
        // blocks, sculk...): their loading is not ported.
        BlockEntity::Plain(plain) => load_plain(reg, &input, plain)?,
        BlockEntity::Special(special) => load_special(reg, &input, special)?,
        BlockEntity::Sign(sign) | BlockEntity::HangingSign(sign) => load_sign(reg, &input, sign)?,
        BlockEntity::Banner { name, patterns } => {
            *name = custom_name(reg, &input)?;
            // setPatterns(read("patterns", BannerPatternLayers.CODEC)): none
            // where the field is absent.
            *patterns = match input.get("patterns") {
                None => Vec::new(),
                Some(tag) => match crate::components::banner_patterns(reg, tag)? {
                    Tag::List(layers) => layers,
                    _ => unreachable!("banner patterns are a list"),
                },
            };
        }
        BlockEntity::Container(container) => load_container(reg, &input, container)?,
        BlockEntity::Entity(entity) => load_entity(reg, &input, entity)?,
        BlockEntity::Piston(_) => return fail("a moving piston"),
    }
    // loadWithComponents: the block entity's own components, a map of
    // component types to their values.
    if let Some(components) = tag.get("components") {
        extras.components = crate::components::components_map(reg, components)?;
    }
    Ok(extras)
}

// The special blocks' block entities (loadAdditional of each that is read).
fn load_special(reg: &Registry, input: &Input, special: &mut super::special::Special) -> R<()> {
    use super::special::Special;
    match special {
        Special::CommandBlock(command) => command.load(reg, input),
        Special::Beacon(beacon) => beacon.load(reg, input),
        Special::Jigsaw(jigsaw) => jigsaw.load(input),
        Special::TestBlock(test) => test.load(input),
        Special::EndGateway(gateway) => gateway.load(input),
        Special::StructureBlock(structure) => structure.load(input),
        // BrushableBlockEntity.loadAdditional: the item (a loot table is not modeled).
        Special::BrushableBlock { item } => {
            if input.get("LootTable").is_some() {
                return fail("a brushable block's loot table");
            }
            input.long_or("LootTableSeed", 0)?;
            *item = None;
            if let Some(tag) = input.get("item") {
                let stack = decode_stack(reg, tag)?;
                if stack.count > 0 {
                    *item = Some(crate::components::stack_fields(reg, &stack)?);
                }
            }
            // The hit direction is read (it must be a number) and not saved.
            if let Some(direction) = input.get("hit_direction") {
                codec::int(direction)?;
            }
            Ok(())
        }
        Special::EndPortal => Ok(()),
        // SculkSensorBlockEntity.loadAdditional.
        Special::SculkSensor(sensor) => {
            sensor.last_vibration_frequency = input.int_or("last_vibration_frequency", 0)?;
            sensor.vibration = super::special::vibration::Data::load(input)?;
            Ok(())
        }
        // SculkShriekerBlockEntity.loadAdditional.
        Special::SculkShrieker(shrieker) => {
            shrieker.warning_level = input.int_or("warning_level", 0)?;
            shrieker.vibration = super::special::vibration::Data::load(input)?;
            Ok(())
        }
        // SculkCatalystBlockEntity.loadAdditional: the spreader's cursors (none are modeled).
        Special::SculkCatalyst => match input.get("cursors") {
            Some(Tag::List(cursors)) if !cursors.is_empty() => fail("a sculk catalyst's cursors"),
            _ => Ok(()),
        },
        Special::EnchantingTable { name } => {
            *name = input.read("CustomName", |tag| crate::text::component(reg, tag))?;
            Ok(())
        }
        _ => fail("block entity data of a special block (spawner, beacon, sculk...)"),
    }
}

// The skulls, beds and copper golem statues (loadAdditional of each).
fn load_plain(reg: &Registry, input: &Input, plain: &mut super::block_entities::Plain) -> R<()> {
    use super::block_entities::Plain;
    match plain {
        // SkullBlockEntity.loadAdditional: the profile (resolved by the server, not
        // modeled), the note block sound and the custom name.
        Plain::Skull { note_block_sound, custom_name } => {
            if input.get("profile").is_some() {
                return fail("a skull's profile (the server resolves it)");
            }
            *note_block_sound = input.read("note_block_sound", |tag| codec::location(tag))?;
            *custom_name = input.read("custom_name", |tag| crate::text::component(reg, tag))?;
        }
        Plain::Bed | Plain::CopperGolemStatue => {}
    }
    Ok(())
}

// LockCode.fromTag: the lock key, an item predicate (as the tag its codec
// writes), none where the field is absent.
fn lock(reg: &Registry, input: &Input) -> R<Option<Tag>> {
    input.read("lock", |tag| crate::components::item_predicate(reg, tag))
}

// BlockEntity.parseCustomNameSafe: CustomName read with the component codec.
fn custom_name(reg: &Registry, input: &Input) -> R<Option<Tag>> {
    input.read("CustomName", |tag| crate::text::component(reg, tag))
}

// ContainerHelper.loadAllItems: the "Items" list into the slots, each element
// an ItemStackWithSlot (a stack with its Slot, read as an unsigned byte and 0
// where it does not read). A slot the container lacks is ignored.
pub fn load_all_items(reg: &Registry, input: &Input, items_out: &mut [super::container::Stack]) -> R<()> {
    for element in input.list_or_empty("Items")? {
        let m = map(element)?;
        let slot = match m.get("Slot") {
            None => 0,
            Some(slot) => codec::unsigned_byte(slot).unwrap_or(0),
        };
        let stack = decode_stack(reg, element)?;
        if (slot as usize) < items_out.len() {
            items_out[slot as usize] = stack_of(stack);
        }
    }
    Ok(())
}

// BaseContainerBlockEntity.loadAdditional (lock, custom name) and
// RandomizableContainer.tryLoadLootTable, then the kind's own fields.
fn load_container(reg: &Registry, input: &Input, container: &mut Container) -> R<()> {
    if matches!(container.kind, Kind::EnderChest) {
        return Ok(());
    }
    container.lock = lock(reg, input)?;
    container.name = custom_name(reg, input)?;
    // A loot table is unpacked from the loot tables when the container is
    // first used; its seed is read either way.
    if input.get("LootTable").is_some() {
        return fail("a container's loot table");
    }
    input.long_or("LootTableSeed", 0)?;
    container.items = empty_slots(&container.kind);
    // The kinds read their items, and their own fields around them.
    match &mut container.kind {
        Kind::Hopper { cooldown_time, .. } => {
            load_all_items(reg, input, &mut container.items)?;
            *cooldown_time = input.int_or("TransferCooldown", -1)?;
        }
        Kind::Crafter { crafting_ticks_remaining, triggered, disabled } => {
            *crafting_ticks_remaining = input.int_or("crafting_ticks_remaining", 0)?;
            load_all_items(reg, input, &mut container.items)?;
            *disabled = [false; 9];
            if let Some(slots) = input.int_array("disabled_slots")? {
                for &slot in slots {
                    // slotCanBeDisabled: a slot of the grid holding nothing.
                    if (0..9).contains(&slot) && super::container::is_empty(&container.items[slot as usize]) {
                        disabled[slot as usize] = true;
                    }
                }
            }
            *triggered = input.int_or("triggered", 0)?;
        }
        _ => load_all_items(reg, input, &mut container.items)?,
    }
    Ok(())
}

fn load_entity(reg: &Registry, input: &Input, entity: &mut Entity) -> R<()> {
    match entity {
        Entity::Lectern(lectern) => {
            // The book as a stack (resolving its pages is off, as Paper's
            // default for item validation has it), the page clamped to its
            // pages.
            let book = input.read("Book", |tag| decode_stack(reg, tag))?;
            let page = input.int_or("Page", 0)?;
            *lectern = Lectern::new();
            if let Some(book) = book {
                lectern.page_count = page_count(reg, &book);
                lectern.book = stack_of(book);
            }
            // Mth.clamp(page, 0, pageCount - 1): Math.min(Math.max(page, 0), max),
            // so a book of no pages (max -1) gives -1.
            let max = lectern.page_count - 1;
            lectern.page = page.max(0).min(max);
        }
        Entity::Furnace(_, furnace) => {
            let lock = lock(reg, input)?;
            let name = custom_name(reg, input)?;
            // (The furnace keeps its quickCheck.)
            let last_recipe = furnace.last_recipe;
            *furnace = Furnace::new();
            furnace.last_recipe = last_recipe;
            furnace.name = name;
            furnace.lock = lock;
            load_all_items(reg, input, &mut furnace.items)?;
            furnace.cooking_timer = input.short_or("cooking_time_spent", 0)? as i32;
            furnace.cooking_total_time = input.short_or("cooking_total_time", 0)? as i32;
            furnace.lit_time_remaining = input.short_or("lit_time_remaining", 0)? as i32;
            furnace.lit_total_time = input.short_or("lit_total_time", 0)? as i32;
            if let Some(recipes) = input.read("RecipesUsed", |tag| {
                let mut used: Vec<(String, i32)> = Vec::new();
                for (key, count) in &map(tag)?.0 {
                    let key = codec::resource_location(key)?;
                    if used.iter().any(|(k, _)| *k == key) {
                        return fail("a recipe used twice");
                    }
                    used.push((key, codec::int(count)?));
                }
                Ok(used)
            })? {
                furnace.recipes_used = recipes;
            }
            furnace.cook_speed_multiplier = input.double_or("Paper.CookSpeedMultiplier", 1.0)?;
        }
        Entity::BrewingStand(stand) => {
            let lock = lock(reg, input)?;
            let name = custom_name(reg, input)?;
            *stand = BrewingStand { last_potion_count: stand.last_potion_count, name, lock, ..BrewingStand::new() };
            load_all_items(reg, input, &mut stand.items)?;
            stand.brew_time = input.short_or("BrewTime", 0)? as i32;
            // A brew in progress remembers the item of its ingredient slot.
            if stand.brew_time > 0 {
                stand.ingredient = Some(if super::container::is_empty(&stand.items[3]) { 0 } else { stand.items[3].borrow().item });
            }
            stand.fuel = input.byte_or("Fuel", 0)? as i32;
        }
        Entity::Jukebox(jukebox) => {
            // JukeboxBlockEntity.loadAdditional: the record, and the song it
            // plays from where it was when the item is a record.
            let record = input.read("RecordItem", |tag| decode_stack(reg, tag))?;
            let ticks = input.long("ticks_since_song_started")?;
            *jukebox = Jukebox::new();
            if let Some(record) = record {
                jukebox.items[0] = stack_of(record);
            }
            if let Some(ticks) = ticks {
                let item = jukebox.items[0].borrow().clone();
                if let Some(song) = super::jukebox::song_of(reg, &item) {
                    // setSongWithoutPlaying: only a song that has not finished.
                    if !super::jukebox::has_finished(reg, &song, ticks) {
                        jukebox.song = Some(song);
                        jukebox.ticks_since_song_started = ticks;
                    }
                }
            }
        }
        Entity::ChiseledBookShelf(shelf) => {
            *shelf = Shelf::new();
            load_all_items(reg, input, &mut shelf.items)?;
            shelf.last_interacted_slot = input.int_or("last_interacted_slot", -1)?;
        }
        // ShelfBlockEntity.loadAdditional: the slots cleared and read, then
        // the alignment.
        Entity::Shelf(shelf) => {
            *shelf = ItemShelf::new();
            load_all_items(reg, input, &mut shelf.items)?;
            shelf.align_items_to_bottom = input.bool_or("align_items_to_bottom", false)?;
        }
        // DecoratedPotBlockEntity.loadAdditional: the sherds, then the item (a
        // loot table is not modeled; tryLoadLootTable reads the seed whether
        // or not there is a table).
        Entity::DecoratedPot(pot) => {
            *pot = Pot::new();
            if let Some(tag) = input.get("sherds") {
                match crate::components::normalize(reg, "minecraft:pot_decorations", tag)? {
                    Tag::List(keys) => pot.sherds = keys.iter().map(|key| key.as_str().expect("a sherd key").to_string()).collect(),
                    _ => unreachable!("pot decorations are a list"),
                }
            }
            if input.get("LootTable").is_some() {
                return fail("a decorated pot's loot table");
            }
            input.long_or("LootTableSeed", 0)?;
            if let Some(item) = input.read("item", |tag| decode_stack(reg, tag))? {
                pot.items[0] = stack_of(item);
            }
        }
        Entity::Bell(_) | Entity::DaylightDetector => {}
    }
    Ok(())
}

// LecternBlockEntity.getPageCount: the pages of written or writable content.
pub fn page_count(reg: &Registry, book: &crate::loot::ItemStack) -> i32 {
    for key in ["minecraft:written_book_content", "minecraft:writable_book_content"] {
        if let Some(content) = book.component(reg, key) {
            return match content.as_compound().and_then(|c| c.get("pages")) {
                Some(Tag::List(pages)) => pages.len() as i32,
                _ => 0,
            };
        }
    }
    0
}

// SignBlockEntity.loadAdditional: both sides' text and the wax.
fn load_sign(reg: &Registry, input: &Input, sign: &mut Sign) -> R<()> {
    sign.front = input.read("front_text", |tag| sign_text(reg, tag))?.unwrap_or_else(SignText::blank);
    sign.back = input.read("back_text", |tag| sign_text(reg, tag))?.unwrap_or_else(SignText::blank);
    sign.waxed = input.bool_or("is_waxed", false)?;
    Ok(())
}

// SignText.DIRECT_CODEC: required messages (four lines), optional filtered
// ones (ignored if unreadable), the color (black unless it reads) and the
// glow (false unless it reads).
fn sign_text(reg: &Registry, tag: &Tag) -> R<SignText> {
    let m = map(tag)?;
    let lines = |tag: &Tag| -> R<[Tag; 4]> {
        let lines = items(tag)?.iter().map(|line| crate::text::sign_line(reg, line)).collect::<R<Vec<Tag>>>()?;
        let n = lines.len();
        <[Tag; 4]>::try_from(lines).map_err(|_| Fail(format!("List must have 4 elements, but it had {n}")))
    };
    let messages = lines(required(m, "messages")?)?;
    // lenientOptionalFieldOf: a value that does not read is none; one this
    // port cannot read cannot be told from one that does not.
    let filtered = match m.get("filtered_messages") {
        None => messages.clone(),
        Some(filtered) => lines(filtered)?,
    };
    let color = match m.get("color") {
        Some(Tag::String(name)) if DYE_COLORS.contains(&name.as_str()) => name.clone(),
        _ => "black".to_string(),
    };
    let glowing = match m.get("has_glowing_text") {
        Some(t) => codec::boolean(t).unwrap_or(false),
        None => false,
    };
    Ok(SignText { messages, filtered, color, glowing })
}

const DYE_COLORS: [&str; 16] = [
    "white", "orange", "magenta", "light_blue", "yellow", "lime", "pink", "gray", "light_gray", "cyan", "purple", "blue", "brown", "green", "red", "black",
];
