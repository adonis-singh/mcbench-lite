// Data components and the item stacks that carry them: DataComponentPatch
// (what a stack changes of its item's default components), the component
// types this port models with their codecs, and ItemStack.CODEC.
//
// A component's value is kept as the tag its codec writes (decoding checks
// and normalizes it: encode(decode(tag))), so two values are equal as
// Objects.equals has them when their tags are. A component type that is not
// modeled here, in saved data, fails: the caller refuses.

use crate::codec::{self, fail, items, location, map, required, string, Fail, R};
use crate::loot::ItemStack;
use crate::nbt::{self, Compound, Tag};
use crate::registry::Registry;
use crate::text;
use std::rc::Rc;

// A stack's component patch, by component type key (sorted); None is a
// component the stack removes ("!minecraft:...").
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Patch(Option<Rc<Vec<(String, Option<Tag>)>>>);

impl Patch {
    pub fn empty() -> Patch {
        Patch(None)
    }

    pub fn from_entries(mut entries: Vec<(String, Option<Tag>)>) -> Patch {
        if entries.is_empty() {
            return Patch(None);
        }
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        Patch(Some(Rc::new(entries)))
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_none()
    }

    pub fn entries(&self) -> &[(String, Option<Tag>)] {
        match &self.0 {
            Some(entries) => entries,
            None => &[],
        }
    }

    // The patch's entry for a component: Some(Some(value)) set, Some(None)
    // removed.
    pub fn get(&self, key: &str) -> Option<&Option<Tag>> {
        self.entries().iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }
}

// The component types this port reads and writes.
pub const MODELED: &[&str] = &[
    "minecraft:banner_patterns",
    "minecraft:base_color",
    "minecraft:block_state",
    "minecraft:bundle_contents",
    "minecraft:bucket_entity_data",
    "minecraft:charged_projectiles",
    "minecraft:container",
    "minecraft:custom_data",
    "minecraft:custom_name",
    "minecraft:damage",
    "minecraft:dyed_color",
    "minecraft:enchantment_glint_override",
    "minecraft:enchantments",
    "minecraft:firework_explosion",
    "minecraft:fireworks",
    "minecraft:item_model",
    "minecraft:item_name",
    "minecraft:lock",
    "minecraft:lore",
    "minecraft:max_damage",
    "minecraft:max_stack_size",
    "minecraft:pot_decorations",
    "minecraft:potion_contents",
    "minecraft:rarity",
    "minecraft:repair_cost",
    "minecraft:stored_enchantments",
    "minecraft:suspicious_stew_effects",
    "minecraft:tooltip_display",
    "minecraft:unbreakable",
    "minecraft:writable_book_content",
    "minecraft:written_book_content",
];

// The page length limit of WrittenBookContent.CONTENT_CODEC (32767 characters
// of the component's JSON) and of a writable page (1024).
const WRITTEN_PAGE_LENGTH: usize = 32767;

// A component value, decoded and written again.
pub fn normalize(reg: &Registry, key: &str, value: &Tag) -> R<Tag> {
    Ok(match key {
        "minecraft:custom_data" | "minecraft:bucket_entity_data" => custom_data(value)?,
        "minecraft:max_stack_size" => Tag::Int(codec::int_range(value, 1, 99)?),
        "minecraft:max_damage" => Tag::Int(codec::int_range(value, 1, i32::MAX)?),
        "minecraft:damage" | "minecraft:repair_cost" => Tag::Int(codec::int_range(value, 0, i32::MAX)?),
        "minecraft:unbreakable" => {
            map(value)?;
            Tag::Compound(Compound::new())
        }
        "minecraft:enchantment_glint_override" => Tag::Byte(codec::boolean(value)? as i8),
        "minecraft:rarity" => {
            let name = string(value)?;
            if !["common", "uncommon", "rare", "epic"].contains(&name) {
                return fail(format!("Unknown rarity {name}"));
            }
            Tag::string(name)
        }
        "minecraft:item_model" => Tag::String(location(value)?),
        "minecraft:lock" => item_predicate(reg, value)?,
        "minecraft:custom_name" | "minecraft:item_name" => text::component(reg, value)?,
        "minecraft:lore" => {
            let lines = items(value)?;
            if lines.len() > 256 {
                return fail("List is longer than 256 entries");
            }
            Tag::List(lines.iter().map(|line| text::component(reg, line)).collect::<R<Vec<Tag>>>()?)
        }
        "minecraft:enchantments" | "minecraft:stored_enchantments" => enchantments(reg, value)?,
        "minecraft:container" => container(reg, value)?,
        "minecraft:suspicious_stew_effects" => suspicious_stew_effects(reg, value)?,
        "minecraft:dyed_color" => {
            // RGB_COLOR_CODEC: an int, or a vector of three floats (red, green, blue)
            // that ARGB.colorFromFloat packs with an alpha of 1.
            match codec::int(value) {
                Ok(color) => Tag::Int(color),
                Err(_) => {
                    let elements = items(value)?;
                    if elements.len() != 3 {
                        return fail("Input is not a list of 3 elements");
                    }
                    let floats = elements.iter().map(codec::float).collect::<R<Vec<f32>>>()?;
                    Tag::Int(text::color_from_float(1.0, floats[0], floats[1], floats[2]))
                }
            }
        }
        "minecraft:base_color" => {
            let name = string(value)?;
            if !DYE_COLORS.contains(&name) {
                return fail(format!("Unknown dye color {name}"));
            }
            Tag::string(name)
        }
        "minecraft:banner_patterns" => banner_patterns(reg, value)?,
        "minecraft:pot_decorations" => pot_decorations(reg, value)?,
        "minecraft:fireworks" => fireworks(value)?,
        "minecraft:firework_explosion" => firework_explosion(value)?,
        "minecraft:potion_contents" => potion_contents(reg, value)?,
        "minecraft:bundle_contents" | "minecraft:charged_projectiles" => {
            let stacks = items(value)?;
            Tag::List(stacks.iter().map(|s| Ok(encode_stack(reg, &decode_stack(reg, s)?)?)).collect::<R<Vec<Tag>>>()?)
        }
        "minecraft:written_book_content" => written_book_content(reg, value)?,
        "minecraft:writable_book_content" => writable_book_content(value)?,
        "minecraft:tooltip_display" => tooltip_display(reg, value)?,
        // BlockItemStateProperties.CODEC: Codec.unboundedMap(STRING, STRING).
        "minecraft:block_state" => {
            let mut out = Compound::new();
            for (property, state) in &map(value)?.0 {
                out.put(property, Tag::string(string(state)?));
            }
            Tag::Compound(out)
        }
        other => return fail(format!("the data component {other}")),
    })
}

// TooltipDisplay.CODEC: hide_tooltip (Codec.BOOL, optionalFieldOf with the
// default false) and hidden_components (DataComponentType.CODEC's list into a
// ReferenceLinkedOpenHashSet, which keeps the first of a repeated type, with
// the default of an empty set); a field at its default is not written.
//
// The component type caches its encoding (DataComponents.ENCODER_CACHE: 512
// values by equals, held softly), and a display's equals compares the hidden
// components as a set: a display that hides several is written in the order
// of the first equal one the cache held, which depends on what the process
// encoded before and what its collector cleared, so it refuses. A display
// that hides one or none has one encoding (and its tag's equality is the
// record's).
fn tooltip_display(reg: &Registry, value: &Tag) -> R<Tag> {
    let m = map(value)?;
    let mut out = Compound::new();
    if let Some(hide) = m.get("hide_tooltip") {
        if codec::boolean(hide)? {
            out.put("hide_tooltip", Tag::Byte(1));
        }
    }
    if let Some(hidden) = m.get("hidden_components") {
        let mut keys: Vec<String> = Vec::new();
        for entry in items(hidden)? {
            let key = location(&entry)?;
            if !reg.component_types.contains(&key) {
                return fail(format!("Unknown registry key in ResourceKey[minecraft:root / minecraft:data_component_type]: {key}"));
            }
            if !keys.contains(&key) {
                keys.push(key);
            }
        }
        if keys.len() > 1 {
            crate::level::refuse(format!("a tooltip display hiding {} components (Java writes the order of the first equal display its encoder cache holds)", keys.len()));
        }
        if !keys.is_empty() {
            out.put("hidden_components", Tag::List(keys.into_iter().map(Tag::String).collect()));
        }
    }
    Ok(Tag::Compound(out))
}

// CustomData.CODEC: a compound, or a string of SNBT that reads as one.
fn custom_data(value: &Tag) -> R<Tag> {
    match value {
        Tag::Compound(_) => Ok(value.clone()),
        Tag::String(text) => match nbt::parse(text) {
            Ok(Tag::Compound(c)) => Ok(Tag::Compound(c)),
            Ok(_) | Err(nbt::ParseError::Syntax) => fail("Expected compound tag"),
            Err(nbt::ParseError::Unsupported(what)) => fail(format!("SNBT with {what}")),
        },
        _ => fail("Not a map"),
    }
}

// ItemEnchantments.CODEC: enchantments (by key) to levels from 1 to 255.
fn enchantments(reg: &Registry, value: &Tag) -> R<Tag> {
    let m = map(value)?;
    let mut out = Compound::new();
    for (key, level) in &m.0 {
        let key = codec::resource_location(key)?;
        if !reg.enchantments.contains(&key) {
            return fail(format!("Unknown enchantment {key}"));
        }
        if out.contains(&key) {
            return fail(format!("Duplicate entry for key {key}"));
        }
        out.put(&key, Tag::Int(codec::int_range(level, 1, 255)?));
    }
    Ok(Tag::Compound(out))
}

// ItemContainerContents.CODEC: the stacks by slot (0 to 255), a later one in a
// slot replacing an earlier; written as the non-empty slots in order.
fn container(reg: &Registry, value: &Tag) -> R<Tag> {
    let entries = items(value)?;
    if entries.len() > 256 {
        return fail("List is longer than 256 entries");
    }
    let mut slots: Vec<(i32, Tag)> = Vec::new();
    for entry in &entries {
        let m = map(entry)?;
        let slot = codec::int_range(required(m, "slot")?, 0, 255)?;
        let stack = encode_stack(reg, &decode_stack(reg, required(m, "item")?)?)?;
        match slots.iter_mut().find(|(s, _)| *s == slot) {
            Some(existing) => existing.1 = stack,
            None => slots.push((slot, stack)),
        }
    }
    slots.sort_by_key(|(slot, _)| *slot);
    Ok(Tag::List(slots.into_iter().map(|(slot, stack)| Tag::compound(vec![("item", stack), ("slot", Tag::Int(slot))])).collect()))
}

// SuspiciousStewEffects.CODEC: a list of entries, each a mob effect (by key) and
// a duration that defaults to 160 (and is left out when it is).
fn suspicious_stew_effects(reg: &Registry, value: &Tag) -> R<Tag> {
    let mut out = Vec::new();
    for entry in &items(value)? {
        let m = map(entry)?;
        let id = codec::resource_location(string(required(m, "id")?)?)?;
        if !reg.mob_effects.contains(&id) {
            return fail(format!("Unknown mob effect {id}"));
        }
        // lenientOptionalFieldOf: a duration that does not read is the default.
        let duration = match m.get("duration") {
            None => 160,
            Some(duration) => codec::int(duration).unwrap_or(160),
        };
        let mut effect = Compound::new();
        effect.put("id", Tag::String(id));
        if duration != 160 {
            effect.put("duration", Tag::Int(duration));
        }
        out.push(Tag::Compound(effect));
    }
    Ok(Tag::List(out))
}

// ItemPredicate.CODEC, as a lock holds it: the items it names (a tag, or the
// items by key: one is written as a string, none or several as a list), a
// count or its bounds, and the components it requires. A predicate on a
// component's value (`predicates`) is not modeled but when it is empty.
pub fn item_predicate(reg: &Registry, value: &Tag) -> R<Tag> {
    let m = map(value)?;
    let mut out = Compound::new();
    if let Some(items) = m.get("items") {
        let item = |tag: &Tag| -> R<String> {
            let key = location(tag)?;
            if !reg.item_by_key.contains_key(&key) {
                return fail(format!("Failed to get element {key}"));
            }
            Ok(key)
        };
        let written = match items {
            Tag::String(text) if text.starts_with('#') => {
                let key = codec::resource_location(&text[1..])?;
                if !reg.item_tags.contains_key(&key) {
                    return fail(format!("Missing tag: '{key}' in 'minecraft:item'"));
                }
                Tag::String(format!("#{key}"))
            }
            Tag::String(_) => Tag::String(item(items)?),
            other => {
                let keys = codec::items(other)?.iter().map(item).collect::<R<Vec<String>>>()?;
                if keys.len() == 1 {
                    Tag::String(keys.into_iter().next().unwrap())
                } else {
                    Tag::List(keys.into_iter().map(Tag::String).collect())
                }
            }
        };
        out.put("items", written);
    }
    if let Some(count) = m.get("count") {
        // MinMaxBounds.Ints: a number (exactly it) or a map of the bounds
        // (written as the number when they are the same, not at all when
        // there are none).
        match count {
            Tag::Compound(bounds) => {
                let min = bounds.get("min").map(codec::int).transpose()?;
                let max = bounds.get("max").map(codec::int).transpose()?;
                match (min, max) {
                    (Some(min), Some(max)) if min > max => return fail("Swapped bounds in range"),
                    (Some(min), Some(max)) if min == max => out.put("count", Tag::Int(min)),
                    (None, None) => {}
                    (min, max) => {
                        let mut written = Compound::new();
                        if let Some(min) = min {
                            written.put("min", Tag::Int(min));
                        }
                        if let Some(max) = max {
                            written.put("max", Tag::Int(max));
                        }
                        out.put("count", Tag::Compound(written));
                    }
                }
            }
            number => out.put("count", Tag::Int(codec::int(number)?)),
        }
    }
    if let Some(components) = m.get("components") {
        let components = components_map(reg, components)?;
        if !components.is_empty() {
            out.put("components", Tag::Compound(components));
        }
    }
    if let Some(predicates) = m.get("predicates") {
        if !map(predicates)?.is_empty() {
            return fail("a predicate on a component");
        }
    }
    Ok(Tag::Compound(out))
}

// DataComponentMap.CODEC (and DataComponentExactPredicate's): component types
// to their values.
pub fn components_map(reg: &Registry, value: &Tag) -> R<Compound> {
    let mut out = Compound::new();
    for (key, component) in &map(value)?.0 {
        let key = codec::resource_location(key)?;
        if !MODELED.contains(&key.as_str()) {
            return fail(format!("the data component {key}"));
        }
        if out.contains(&key) {
            return fail(format!("Duplicate entry for key: '{key}'"));
        }
        out.put(&key, normalize(reg, &key, component)?);
    }
    Ok(out)
}

// BannerPatternLayers.CODEC: a list of layers, each a pattern (a banner
// pattern's key, or one defined in place by its asset and translation key)
// and a dye color.
pub fn banner_patterns(reg: &Registry, value: &Tag) -> R<Tag> {
    let mut layers = Vec::new();
    for layer in &items(value)? {
        let m = map(layer)?;
        let pattern = match required(m, "pattern")? {
            Tag::Compound(direct) => Tag::compound(vec![
                ("asset_id", Tag::String(codec::location(required(direct, "asset_id")?)?)),
                ("translation_key", Tag::string(string(required(direct, "translation_key")?)?)),
            ]),
            tag => {
                let key = codec::location(tag)?;
                if !reg.banner_patterns.contains(&key) {
                    return fail(format!("Failed to get element ResourceKey[minecraft:banner_pattern / {key}]"));
                }
                Tag::String(key)
            }
        };
        let color = string(required(m, "color")?)?;
        if !DYE_COLORS.contains(&color) {
            return fail(format!("Unknown element name:{color}"));
        }
        layers.push(Tag::compound(vec![("color", Tag::string(color)), ("pattern", pattern)]));
    }
    Ok(Tag::List(layers))
}

const DYE_COLORS: [&str; 16] = ["white", "orange", "magenta", "light_blue", "yellow", "lime", "pink", "gray", "light_gray", "cyan", "purple", "blue", "brown", "green", "red", "black"];

// PotDecorations.CODEC: up to four items (a brick is none), written as all
// four with bricks for none.
fn pot_decorations(reg: &Registry, value: &Tag) -> R<Tag> {
    let list = items(value)?;
    if list.len() > 4 {
        return fail("List is longer than 4 entries");
    }
    let mut out = Vec::new();
    for at in 0..4 {
        let key = match list.get(at) {
            None => "minecraft:brick".to_string(),
            Some(tag) => {
                let key = codec::resource_location(string(tag)?)?;
                if !reg.item_by_key.contains_key(&key) {
                    return fail(format!("Unknown registry key {key}"));
                }
                key
            }
        };
        out.push(Tag::String(key));
    }
    Ok(Tag::List(out))
}

// Fireworks.CODEC: a flight duration (an unsigned byte, 0 by default and left
// out then) and up to 256 explosions (left out when none).
fn fireworks(value: &Tag) -> R<Tag> {
    let m = map(value)?;
    let flight = match m.get("flight_duration") {
        None => 0,
        Some(tag) => codec::byte(tag)? as u8 as i32,
    };
    let explosions = match m.get("explosions") {
        None => Vec::new(),
        Some(tag) => {
            let list = items(tag)?;
            if list.len() > 256 {
                return fail("List is longer than 256 entries");
            }
            list.iter().map(firework_explosion).collect::<R<Vec<Tag>>>()?
        }
    };
    let mut out = Compound::new();
    if flight != 0 {
        out.put("flight_duration", Tag::Byte(flight as u8 as i8));
    }
    if !explosions.is_empty() {
        out.put("explosions", Tag::List(explosions));
    }
    Ok(Tag::Compound(out))
}

// FireworkExplosion.CODEC: the shape, the colors and fade colors (left out
// when empty), the trail and twinkle (left out when false).
fn firework_explosion(value: &Tag) -> R<Tag> {
    let m = map(value)?;
    let shape = string(required(m, "shape")?)?;
    if !["small_ball", "large_ball", "star", "creeper", "burst"].contains(&shape) {
        return fail(format!("Unknown shape {shape}"));
    }
    let mut out = Compound::new();
    out.put("shape", Tag::string(shape));
    for key in ["colors", "fade_colors"] {
        if let Some(tag) = m.get(key) {
            let colors = items(tag)?.iter().map(|c| codec::int(c).map(Tag::Int)).collect::<R<Vec<Tag>>>()?;
            if !colors.is_empty() {
                out.put(key, Tag::List(colors));
            }
        }
    }
    for key in ["has_trail", "has_twinkle"] {
        if let Some(tag) = m.get(key) {
            if codec::boolean(tag)? {
                out.put(key, Tag::Byte(1));
            }
        }
    }
    Ok(Tag::Compound(out))
}

// PotionContents.CODEC: a potion alone (by key), or a record of a potion, a
// custom color, custom effects and a custom name (each left out when it is
// none, an empty effect list too).
fn potion_contents(reg: &Registry, value: &Tag) -> R<Tag> {
    let potion = |tag: &Tag| -> R<String> {
        let key = codec::resource_location(string(tag)?)?;
        if !reg.potions.contains(&key) {
            return fail(format!("Unknown potion {key}"));
        }
        Ok(key)
    };
    let mut out = Compound::new();
    match value {
        Tag::String(_) => out.put("potion", Tag::String(potion(value)?)),
        _ => {
            let m = map(value)?;
            if let Some(tag) = m.get("potion") {
                out.put("potion", Tag::String(potion(tag)?));
            }
            if let Some(color) = m.get("custom_color") {
                out.put("custom_color", Tag::Int(codec::int(color)?));
            }
            if let Some(effects) = m.get("custom_effects") {
                let effects = items(effects)?.iter().map(|effect| mob_effect_instance(reg, effect)).collect::<R<Vec<Tag>>>()?;
                if !effects.is_empty() {
                    out.put("custom_effects", Tag::List(effects));
                }
            }
            if let Some(name) = m.get("custom_name") {
                out.put("custom_name", Tag::string(string(name)?));
            }
        }
    }
    Ok(Tag::Compound(out))
}

// MobEffectInstance.CODEC: the effect, and its details (MobEffectInstance.Details):
// an amplifier (a byte, 0 by default, left out then), a duration (an int, 0
// by default, left out then), ambient (false, left out then), show_particles
// (true, left out then) and show_icon (as show_particles unless given, always
// written). The hidden effect is read (it must be a map of details) and not
// written.
fn mob_effect_instance(reg: &Registry, value: &Tag) -> R<Tag> {
    let m = map(value)?;
    let id = codec::resource_location(string(codec::required(m, "id")?)?)?;
    if !reg.mob_effects.contains(&id) {
        return fail(format!("Unknown registry key in ResourceKey[minecraft:root / minecraft:mob_effect]: {id}"));
    }
    let mut out = Compound::new();
    out.put("id", Tag::String(id));
    mob_effect_details(m, &mut out, 0)?;
    Ok(Tag::Compound(out))
}

fn mob_effect_details(m: &Compound, out: &mut Compound, depth: usize) -> R<()> {
    if depth > 64 {
        return fail("a hidden effect nested deeper than this port reads");
    }
    let amplifier = match m.get("amplifier") {
        None => 0,
        Some(tag) => codec::byte(tag)?,
    };
    let duration = match m.get("duration") {
        None => 0,
        Some(tag) => codec::int(tag)?,
    };
    let ambient = match m.get("ambient") {
        None => false,
        Some(tag) => codec::boolean(tag)?,
    };
    let show_particles = match m.get("show_particles") {
        None => true,
        Some(tag) => codec::boolean(tag)?,
    };
    let show_icon = match m.get("show_icon") {
        None => show_particles,
        Some(tag) => codec::boolean(tag)?,
    };
    if let Some(hidden) = m.get("hidden_effect") {
        mob_effect_details(map(hidden)?, &mut Compound::new(), depth + 1)?;
    }
    if depth == 0 {
        if amplifier != 0 {
            out.put("amplifier", Tag::Byte(amplifier));
        }
        if duration != 0 {
            out.put("duration", Tag::Int(duration));
        }
        if ambient {
            out.put("ambient", Tag::Byte(1));
        }
        if !show_particles {
            out.put("show_particles", Tag::Byte(0));
        }
        out.put("show_icon", Tag::Byte(show_icon as i8));
    }
    Ok(())
}

// Filterable.codec: {raw, filtered?}, or the bare value.
fn filterable(value: &Tag, inner: &dyn Fn(&Tag) -> R<Tag>) -> R<Tag> {
    if let Tag::Compound(m) = value {
        if let Some(raw) = m.get("raw") {
            if let Ok(raw) = inner(raw) {
                match m.get("filtered") {
                    None => return Ok(Tag::compound(vec![("raw", raw)])),
                    Some(filtered) => {
                        if let Ok(filtered) = inner(filtered) {
                            return Ok(Tag::compound(vec![("filtered", filtered), ("raw", raw)]));
                        }
                    }
                }
            }
        }
    }
    Ok(Tag::compound(vec![("raw", inner(value)?)]))
}

fn written_page(reg: &Registry, value: &Tag) -> R<Tag> {
    let component = text::component(reg, value)?;
    // flatRestrictedCodec: the component's JSON may not exceed the limit.
    match text::json_length(&component) {
        text::JsonLength::Exact(length) if length > WRITTEN_PAGE_LENGTH => return fail("a page over the length limit"),
        text::JsonLength::Exact(_) => {}
        text::JsonLength::AtMost(bound) if bound > WRITTEN_PAGE_LENGTH => return fail("a page near the length limit"),
        text::JsonLength::AtMost(_) => {}
    }
    Ok(component)
}

// WrittenBookContent.CODEC.
fn written_book_content(reg: &Registry, value: &Tag) -> R<Tag> {
    let m = map(value)?;
    let title = filterable(required(m, "title")?, &|t| Ok(Tag::String(codec::string_length(t, 0, 32)?)))?;
    let author = Tag::string(string(required(m, "author")?)?);
    let generation = match m.get("generation") {
        None => 0,
        Some(g) => codec::int_range(g, 0, 3)?,
    };
    let pages = match m.get("pages") {
        None => Vec::new(),
        Some(p) => items(p)?.iter().map(|page| filterable(page, &|tag| written_page(reg, tag))).collect::<R<Vec<Tag>>>()?,
    };
    let resolved = match m.get("resolved") {
        None => false,
        Some(r) => codec::boolean(r)?,
    };
    let mut out = Compound::new();
    out.put("title", title);
    out.put("author", author);
    if generation != 0 {
        out.put("generation", Tag::Int(generation));
    }
    if !pages.is_empty() {
        out.put("pages", Tag::List(pages));
    }
    if resolved {
        out.put("resolved", Tag::Byte(1));
    }
    Ok(Tag::Compound(out))
}

// WritableBookContent.CODEC.
fn writable_book_content(value: &Tag) -> R<Tag> {
    let m = map(value)?;
    let pages = match m.get("pages") {
        None => Vec::new(),
        Some(p) => {
            let pages = items(p)?;
            if pages.len() > 100 {
                return fail("List is longer than 100 entries");
            }
            pages.iter().map(|page| filterable(page, &|t| Ok(Tag::String(codec::string_length(t, 0, 1024)?)))).collect::<R<Vec<Tag>>>()?
        }
    };
    let mut out = Compound::new();
    if !pages.is_empty() {
        out.put("pages", Tag::List(pages));
    }
    Ok(Tag::Compound(out))
}

// DataComponentPatch.CODEC, and PatchedDataComponentMap.fromPatch against the
// item's own components: a component set to the item's value is no change,
// one removed that the item lacks is none.
pub fn decode_patch(reg: &Registry, item: u16, value: &Tag) -> R<Patch> {
    let m = map(value)?;
    let mut entries: Vec<(String, Option<Tag>)> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    for (key, component) in &m.0 {
        let (removed, name) = match key.strip_prefix('!') {
            Some(name) => (true, name),
            None => (false, key.as_str()),
        };
        let name = codec::resource_location(name)?;
        if !MODELED.contains(&name.as_str()) {
            return fail(format!("the data component {name}"));
        }
        // Two keys naming one component (with and without the namespace)
        // are a duplicate entry; a component both set and removed has no
        // order this port knows.
        if seen.contains(&name) {
            return fail(format!("the component {name} twice"));
        }
        seen.push(name.clone());
        let prototype = reg.item_component(item, &name);
        if removed {
            // Unit.CODEC: any map.
            map(component)?;
            if prototype.is_some() {
                entries.push((name, None));
            }
        } else {
            let value = normalize(reg, &name, component)?;
            if prototype.as_ref() != Some(&value) {
                entries.push((name, Some(value)));
            }
        }
    }
    Ok(Patch::from_entries(entries))
}

// DataComponentPatch.CODEC's encoder.
pub fn encode_patch(patch: &Patch) -> Tag {
    let mut out = Compound::new();
    for (key, value) in patch.entries() {
        match value {
            Some(value) => out.put(key, value.clone()),
            None => out.put(&format!("!{key}"), Tag::Compound(Compound::new())),
        }
    }
    Tag::Compound(out)
}

// ItemStack.CODEC's decoder.
pub fn decode_stack(reg: &Registry, value: &Tag) -> R<ItemStack> {
    let m = map(value)?;
    let id = location(required(m, "id")?)?;
    if id == "minecraft:air" {
        return fail("Item must not be minecraft:air");
    }
    let Some(&item) = reg.item_by_key.get(&id) else { return fail(format!("Unknown item {id}")) };
    // count: intRange(1, 99), orElse(1).
    let count = match m.get("count") {
        None => 1,
        Some(count) => codec::int_range(count, 1, 99).unwrap_or(1),
    };
    let patch = match m.get("components") {
        None => Patch::empty(),
        Some(components) => decode_patch(reg, item, components)?,
    };
    Ok(ItemStack { item, count, patch })
}

// ItemStack.CODEC's encoder (an empty stack, or a count outside 1 to 99, does
// not encode).
pub fn encode_stack(reg: &Registry, stack: &ItemStack) -> R<Tag> {
    if stack.item == 0 || stack.count < 1 || stack.count > 99 {
        return fail(format!("a stack that does not encode ({} x{})", reg.items[stack.item as usize].key, stack.count));
    }
    let mut out = Compound::new();
    out.put("count", Tag::Int(stack.count));
    out.put("id", Tag::string(&reg.items[stack.item as usize].key));
    if !stack.patch.is_empty() {
        out.put("components", encode_patch(&stack.patch));
    }
    Ok(Tag::Compound(out))
}

// ItemStack.MAP_CODEC's fields, merged into another compound (ItemStackWithSlot).
pub fn stack_fields(reg: &Registry, stack: &ItemStack) -> R<Compound> {
    match encode_stack(reg, stack)? {
        Tag::Compound(c) => Ok(c),
        _ => unreachable!(),
    }
}

impl ItemStack {
    pub fn new(item: u16, count: i32) -> ItemStack {
        ItemStack { item, count, patch: Patch::empty() }
    }

    // ItemStack.EMPTY.
    pub fn empty() -> ItemStack {
        ItemStack::new(0, 0)
    }

    // PatchedDataComponentMap.set: a component set to a value (one equal to the
    // item's own is no change).
    pub fn set_component(&mut self, reg: &Registry, key: &str, value: Tag) {
        let mut entries: Vec<(String, Option<Tag>)> = self.patch.entries().iter().filter(|(k, _)| k != key).cloned().collect();
        if reg.item_component(self.item, key).as_ref() != Some(&value) {
            entries.push((key.to_string(), Some(value)));
        }
        self.patch = Patch::from_entries(entries);
    }

    // PatchedDataComponentMap.remove: the component removed from the stack
    // (a removal of a component the item lacks is no change).
    pub fn remove_component(&mut self, reg: &Registry, key: &str) {
        let mut entries: Vec<(String, Option<Tag>)> = self.patch.entries().iter().filter(|(k, _)| k != key).cloned().collect();
        if reg.item_component(self.item, key).is_some() {
            entries.push((key.to_string(), None));
        }
        self.patch = Patch::from_entries(entries);
    }

    // ItemStack.transmuteCopy(item, count): the new item with this stack's
    // component patch, made fit the new item's own components
    // (PatchedDataComponentMap.fromPatch). An empty stack gives an empty one.
    pub fn transmute_copy(&self, reg: &Registry, item: u16, count: i32) -> ItemStack {
        if self.count <= 0 {
            return ItemStack::empty();
        }
        let mut entries = Vec::new();
        for (key, value) in self.patch.entries() {
            let prototype = reg.item_component(item, key);
            match value {
                Some(value) if prototype.as_ref() == Some(value) => {}
                None if prototype.is_none() => {}
                _ => entries.push((key.clone(), value.clone())),
            }
        }
        ItemStack { item, count, patch: Patch::from_entries(entries) }
    }

    // The value of a component this stack has (its patch, else its item's).
    pub fn component(&self, reg: &Registry, key: &str) -> Option<Tag> {
        match self.patch.get(key) {
            Some(value) => value.clone(),
            None => reg.item_component(self.item, key),
        }
    }

    // ItemStack.getMaxStackSize: DataComponents.MAX_STACK_SIZE, 1 without it
    // (an empty stack has no components).
    pub fn max_stack_size(&self, reg: &Registry) -> i32 {
        if self.count <= 0 || self.item == 0 {
            return 1;
        }
        match self.patch.get("minecraft:max_stack_size") {
            Some(Some(Tag::Int(v))) => *v,
            Some(Some(_)) => unreachable!("max_stack_size is an int"),
            Some(None) => 1,
            None => reg.items[self.item as usize].max_stack,
        }
    }

    // The stacks that the container, bundle_contents and charged_projectiles
    // components hold, in that order (the contained items an observation
    // lists).
    pub fn contained(&self, reg: &Registry) -> R<Vec<ItemStack>> {
        let mut out = Vec::new();
        if let Some(Tag::List(slots)) = self.component(reg, "minecraft:container") {
            for slot in &slots {
                let m = map(slot)?;
                out.push(decode_stack(reg, required(m, "item")?)?);
            }
        }
        for key in ["minecraft:bundle_contents", "minecraft:charged_projectiles"] {
            if let Some(Tag::List(stacks)) = self.component(reg, key) {
                for stack in &stacks {
                    out.push(decode_stack(reg, stack)?);
                }
            }
        }
        Ok(out)
    }
}

// ItemContainerContents.fromItems, as the container component's tag: the
// non-empty slots as {item, slot} up to the last.
pub fn container_contents(reg: &Registry, items: &[crate::blocks::container::Stack]) -> R<Tag> {
    let mut slots = Vec::new();
    for (slot, stack) in items.iter().enumerate() {
        let stack = stack.borrow();
        if stack.count > 0 {
            slots.push(Tag::compound(vec![("item", encode_stack(reg, &stack)?), ("slot", Tag::Int(slot as i32))]));
        }
    }
    Ok(Tag::List(slots))
}

pub fn into_fail<T>(result: Result<T, nbt::ParseError>) -> R<T> {
    result.map_err(|e| match e {
        nbt::ParseError::Syntax => Fail("SNBT that the grammar rejects".into()),
        nbt::ParseError::Unsupported(what) => Fail(format!("SNBT with {what}")),
    })
}
