// What several fixes share: LegacyComponentDataFixUtils, ExtraDataFixUtils,
// the fix families (EntityRenameFix, DataComponentRemainderFix,
// AbstractBlockPropertyFix, AttributesRenameFix), and the iteration order of
// a java.util.HashSet of strings where a fix lists one.

use super::super::gson::{self, Je};
use super::*;
use std::collections::HashMap;

// LegacyComponentDataFixUtils.createTextComponentJson: {"text": ...} as
// GsonHelper.toStableString writes it.
pub fn text_component_json(text: &str) -> String {
    gson::to_stable_string(&Je::Obj(vec![("text".into(), Je::Str(text.into()))]))
}

pub fn translatable_component_json(key: &str) -> String {
    gson::to_stable_string(&Je::Obj(vec![("translate".into(), Je::Str(key.into()))]))
}

pub fn plain_text_component(text: &str) -> Dy {
    Dy::of(Tag::String(text_component_json(text)))
}

pub fn empty_component() -> Dy {
    plain_text_component("")
}

pub fn translatable_component(key: &str) -> Dy {
    Dy::of(Tag::String(translatable_component_json(key)))
}

// LegacyComponentDataFixUtils.extractTranslationString.
pub fn extract_translation_string(data: &str) -> Option<String> {
    match gson::parse_lenient(data) {
        Ok(element @ Je::Obj(_)) => match element.get("translate") {
            Some(t) if t.is_primitive() => Some(t.as_string()),
            _ => None,
        },
        _ => None,
    }
}

// String.hashCode.
pub fn string_hash(s: &str) -> i32 {
    s.encode_utf16().fold(0i32, |h, c| h.wrapping_mul(31).wrapping_add(c as i32))
}

// The iteration order of a new HashSet<String> given these adds (in order,
// repeats ignored): the table index of each, at the capacity the set grew
// to, and within an index the order of the adds.
pub fn java_hash_set_order(adds: &[String]) -> Vec<String> {
    let mut items: Vec<String> = Vec::new();
    for a in adds {
        if !items.contains(a) {
            items.push(a.clone());
        }
    }
    let mut capacity = 16usize;
    for size in 1..=items.len() {
        if size > capacity * 3 / 4 {
            capacity *= 2;
        }
    }
    let index = |s: &str| {
        let h = string_hash(s);
        ((h ^ ((h as u32) >> 16) as i32) as u32 as usize) & (capacity - 1)
    };
    let mut buckets: HashMap<usize, usize> = HashMap::new();
    for s in &items {
        *buckets.entry(index(s)).or_default() += 1;
    }
    if buckets.values().any(|n| *n >= 8) {
        refuse("a hash set bin long enough to be treeified (the data fixer's order)");
    }
    let mut ordered: Vec<(usize, usize, String)> = items.into_iter().enumerate().map(|(i, s)| (index(&s), i, s)).collect();
    ordered.sort();
    ordered.into_iter().map(|(_, _, s)| s).collect()
}

// java.util.UUID of four ints (AttributeModifierIdFix.uuidFromIntArray).
pub fn uuid_from_ints(ints: &[i32]) -> Option<(u64, u64)> {
    (ints.len() == 4).then(|| (((ints[0] as u64) << 32) | (ints[1] as u32 as u64), ((ints[2] as u64) << 32) | (ints[3] as u32 as u64)))
}

// UUID.toString.
pub fn uuid_string((most, least): (u64, u64)) -> String {
    format!("{:08x}-{:04x}-{:04x}-{:04x}-{:012x}", most >> 32, (most >> 16) & 0xffff, most & 0xffff, least >> 48, least & 0xffff_ffff_ffff)
}

// UUID.fromString of a canonical UUID.
pub fn uuid_parse(text: &str) -> (u64, u64) {
    let hex: String = text.chars().filter(|c| *c != '-').collect();
    (u64::from_str_radix(&hex[..16], 16).unwrap(), u64::from_str_radix(&hex[16..], 16).unwrap())
}

// ExtraDataFixUtils.createBlockPos: an int array.
pub fn create_block_pos(d: &Dy, x: i32, y: i32, z: i32) -> Dy {
    d.create_int_list([x, y, z])
}

// ExtraDataFixUtils.fixInlineBlockPos.
pub fn fix_inline_block_pos(data: Dy, x: &str, y: &str, z: &str, field: &str) -> Dy {
    let (ox, oy, oz) = (data.get(x).as_number().result(), data.get(y).as_number().result(), data.get(z).as_number().result());
    match (ox, oy, oz) {
        (Some(a), Some(b), Some(c)) => {
            let pos = create_block_pos(&data, a.int_value(), b.int_value(), c.int_value());
            data.remove(x).remove(y).remove(z).set(field, pos)
        }
        _ => data,
    }
}

// ExtraDataFixUtils.fixBlockPos: {X, Y, Z} as an int array.
pub fn fix_block_pos(data: Dy) -> Dy {
    match (data.get("X").as_number().result(), data.get("Y").as_number().result(), data.get("Z").as_number().result()) {
        (Some(a), Some(b), Some(c)) => create_block_pos(&data, a.int_value(), b.int_value(), c.int_value()),
        _ => data,
    }
}

// ExtraDataFixUtils.fixStringField.
pub fn fix_string_field(data: Dy, field: &str, fixer: &dyn Fn(&str) -> String) -> Dy {
    data.update(field, |d| match d.as_string().result() {
        Some(s) => d.create_string(&fixer(&s)),
        None => d,
    })
}

// ExtraDataFixUtils.dyeColorIdToName.
pub fn dye_color_name(id: i32) -> &'static str {
    match id {
        1 => "orange",
        2 => "magenta",
        3 => "light_blue",
        4 => "yellow",
        5 => "lime",
        6 => "pink",
        7 => "gray",
        8 => "light_gray",
        9 => "cyan",
        10 => "purple",
        11 => "blue",
        12 => "brown",
        13 => "green",
        14 => "red",
        15 => "black",
        _ => "white",
    }
}

// EntityRenameFix: an entity's choice (and its value, as the type of its old
// choice with the output's choice type below it) renamed by the function.
pub fn entity_rename(fx: &Fx, fix: impl Fn(&str, Typed) -> (String, Typed) + 'static) -> RuleRef {
    let input = fx.input().find_choice_type(ENTITY);
    let output = fx.output().find_choice_type(ENTITY);
    let Ty::Choice(_, _, in_types) = t::ty(input) else { unreachable!() };
    let Ty::Choice(_, _, out_types) = t::ty(output) else { unreachable!() };
    let patched: std::cell::RefCell<HashMap<String, TyId>> = Default::default();
    fix_fn(input, output, move |v| {
        let (k, x) = v.into_pair();
        let name = k.str().to_string();
        let ty = *patched.borrow_mut().entry(name.clone()).or_insert_with(|| patch_sub_type(t::choice_get(&in_types, &name).expect("the value's choice"), input, output));
        let (new_name, typed) = fix(&name, Typed::new(ty, x));
        match t::choice_get(&out_types, &new_name) {
            Some(t1) if t::eq(t1, typed.ty, true, true) => {}
            _ => refuse("an entity rename's dynamic type check failed (IllegalStateException)"),
        }
        V::pair(V::Str(new_name), typed.v)
    })
}

// DataComponentRemainderFix: one component of the components' remainder
// fixed (None removes it) and maybe renamed.
pub fn component_remainder(fx: &Fx, id: &'static str, new_id: &'static str, fix: impl Fn(Dy) -> Option<Dy> + 'static) -> RuleRef {
    fix_remainder(fx.input().ty(DATA_COMPONENTS), move |d| match d.get(id).result() {
        None => d,
        Some(c) => {
            let fixed = fix(c);
            d.remove(id).set_field_if_present(new_id, fixed)
        }
    })
}

// AbstractBlockPropertyFix: the properties of the block states of a block.
pub fn block_property_fix(fx: &Fx, should_fix: impl Fn(&str) -> bool + 'static, fix: impl Fn(&str, Dy) -> Dy + 'static) -> RuleRef {
    fix_remainder(fx.input().ty(BLOCK_STATE), move |tag| {
        match tag.get("Name").as_string().result().map(|s| t::ensure_namespaced(&s)) {
            Some(name) if should_fix(&name) => tag.update("Properties", |p| fix(&name, p)),
            _ => tag,
        }
    })
}

// BlockPropertyRenameAndFix.
pub fn block_property_rename(fx: &Fx, block: &'static str, old: &'static str, new: &'static str, value: fn(&str) -> String) -> RuleRef {
    block_property_fix(fx, move |name| name == block, move |_, p| p.rename_and_fix_field(old, new, |d| d.create_string(&value(&d.as_string_or("")))))
}

// AttributesRenameFix: data components' attribute modifiers' type and an
// entity's attributes' id renamed.
pub fn attributes_rename(fx: &Fx, rename: fn(&str) -> String) -> RuleRef {
    let components = fix_remainder(fx.input().ty(DATA_COMPONENTS), move |d| {
        d.update("minecraft:attribute_modifiers", |m| {
            m.update("modifiers", |list| match list.as_stream_opt().result() {
                Some(items) => list.create_list(items.into_iter().map(|x| fix_string_field(x, "type", &rename))),
                None => list,
            })
        })
    });
    let entities = fix_remainder(fx.input().ty(ENTITY), move |d| {
        d.update("attributes", |list| match list.as_stream_opt().result() {
            Some(items) => list.create_list(items.into_iter().map(|x| fix_string_field(x, "id", &rename))),
            None => list,
        })
    });
    rewrite::seq(vec![components, entities])
}

// EntityFieldsRenameFix.
pub fn entity_fields_rename(fx: &Fx, entity: &'static str, renames: &'static [(&'static str, &'static str)]) -> RuleRef {
    named_entity_remainder(fx, ENTITY, entity, move |mut d| {
        for (old, new) in renames {
            d = d.rename_field(old, new);
        }
        d
    })
}

// CompoundTag.merge: each tag of the other replaces this one's, compounds merged.
pub fn merge_compounds(a: &Compound, b: &Compound) -> Compound {
    let mut out = a.clone();
    out.merge(b);
    out
}
