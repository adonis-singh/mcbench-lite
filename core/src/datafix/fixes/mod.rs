// The fixes of DataFixers.addFixers, in their order, as the pinned server
// registers them. A fix whose rule changes none of the types the engines
// upgrade (block entities, entities, item stacks, block states: the dump's
// F lines say which) is irrelevant here; one that does is ported or refused.

mod util;
mod v1;
mod v2;
mod v3;
mod v4;

use super::optic::{field_finder, find_field, named_choice, namespaced_string, remainder_finder, string, type_finder, Finder, Typed};
use super::ops::{self, Dy, ODy};
use super::rewrite::{self, RuleRef, RR};
use super::schemas::{self, Schema};
use super::types::{self as t, Ty, TyId, V};
use crate::level::refuse;
use crate::nbt::{Compound, Tag};
use std::cell::RefCell;
use std::rc::Rc;

// A fix's schemas: its output schema's version key, and whether it changes
// the type (its input schema is then the one before).
pub struct Fx {
    pub key: i32,
    pub changes: bool,
}

impl Fx {
    pub fn output(&self) -> Rc<Schema> {
        schemas::schema(self.key)
    }

    pub fn input(&self) -> Rc<Schema> {
        if self.changes {
            schemas::parent(self.key)
        } else {
            self.output()
        }
    }
}

pub enum Imp {
    // Its rule changes none of the upgraded types.
    I,
    // Not ported: an upgrade through it is refused.
    U,
    // changesType, and the rule.
    R(bool, fn(&Fx) -> RuleRef),
}

use Imp::{I, R, U};

pub struct Def {
    pub key: i32,
    pub class: &'static str,
    pub imp: Imp,
}

pub const BLOCK_ENTITY: &str = "block_entity";
pub const ENTITY: &str = "entity";
pub const ITEM_STACK: &str = "item_stack";
pub const BLOCK_STATE: &str = "block_state";
pub const BLOCK_NAME: &str = "block_name";
pub const ITEM_NAME: &str = "item_name";
pub const FLAT_BLOCK_STATE: &str = "flat_block_state";
pub const TEXT_COMPONENT: &str = "text_component";
pub const DATA_COMPONENTS: &str = "data_components";

// DataFix.fixTypeEverywhere(name, type, newType, function).
pub fn fix_fn(ty: TyId, new_ty: TyId, f: impl Fn(V) -> V + 'static) -> RuleRef {
    rewrite::fix_everywhere(ty, RR::new(ty, new_ty, Rc::new(f)))
}

// DataFix.fixTypeEverywhereTyped(name, type, newType, function), with
// DataFix.checked's type check.
pub fn fix_typed(ty: TyId, new_ty: TyId, f: impl Fn(Typed) -> Typed + 'static) -> RuleRef {
    fix_fn(ty, new_ty, move |v| {
        let r = f(Typed::new(ty, v));
        if !t::eq(new_ty, r.ty, true, false) {
            refuse("a data fixer's dynamic type check failed (IllegalStateException)");
        }
        r.v
    })
}

// DataFix.fixTypeEverywhereTyped on the remainder: typed.update(remainderFinder(), f).
pub fn fix_remainder(ty: TyId, f: impl Fn(Dy) -> Dy + 'static) -> RuleRef {
    let f = Rc::new(f);
    fix_typed(ty, ty, move |typed| {
        let f = f.clone();
        typed.update_remainder(move |d| f(d))
    })
}

// DataFix.convertUnchecked: the identity, as a view that is no nop.
pub fn convert_unchecked(ty: TyId, new_ty: TyId) -> RuleRef {
    fix_fn(ty, new_ty, |v| v)
}

// DataFix.writeFixAndRead: the value written as the type with this rule
// applied below it, fixed, and read as the new type.
pub fn write_fix_and_read(ty: TyId, new_ty: TyId, fix: impl Fn(Dy) -> Dy + 'static) -> RuleRef {
    let patched: Rc<std::cell::Cell<Option<TyId>>> = Rc::new(std::cell::Cell::new(None));
    let p = patched.clone();
    let view = RR::new(
        ty,
        new_ty,
        Rc::new(move |v| {
            let written = match t::write(p.get().expect("the patched type"), &v).result_or_partial() {
                Some(w) => w,
                None => refuse("a data fixer could not write the object (RuntimeException)"),
            };
            match t::decode(new_ty, &fix(Dy(written)).0).result_or_partial() {
                Some((value, _)) => value,
                None => refuse("a data fixer could not read the new object (RuntimeException)"),
            }
        }),
    );
    let rule = rewrite::fix_everywhere(ty, view);
    patched.set(Some(rewrite::all(ty, &rule, true, false).new_ty));
    rule
}

// Util.writeAndReadTypedOrThrow: written (a success only), fixed, read back
// (a partial result is taken).
pub fn write_and_read_typed(typed: &Typed, ty: TyId, f: impl FnOnce(Dy) -> Dy) -> Typed {
    let written = match typed.write() {
        super::dr::Dr::Ok(d) => d,
        _ => refuse("a data fixer could not write a typed value (IllegalStateException)"),
    };
    read_typed_or_throw(ty, &f(written), true)
}

// Util.readTypedOrThrow.
pub fn read_typed_or_throw(ty: TyId, data: &Dy, partial: bool) -> Typed {
    let r = t::decode(ty, &data.0);
    let v = if partial { r.result_or_partial() } else { r.result() };
    match v {
        Some((v, _)) => Typed::new(ty, v),
        None => refuse("a data fixer could not read a type (ReportedException)"),
    }
}

// ExtraDataFixUtils.patchSubType: the type with each part of the old sub
// type made the new one.
pub fn patch_sub_type(ty: TyId, old: TyId, new: TyId) -> TyId {
    let patcher = rewrite::everywhere(&rewrite::if_same(old, RR::new(old, new, Rc::new(|_| refuse("the data fixer's type patcher ran (UnsupportedOperationException)")))), true, true);
    rewrite::all(ty, &patcher, true, false).new_ty
}

// AddNewChoices: the choice type of the type becomes the output schema's.
pub fn add_new_choices(fx: &Fx, reference: &str) -> RuleRef {
    let input = fx.input().find_choice_type(reference);
    let output = fx.output().find_choice_type(reference);
    let (Ty::Choice(_, k1, _), Ty::Choice(_, k2, out_types)) = (t::ty(input), t::ty(output)) else { unreachable!() };
    if k1 != k2 {
        refuse("Could not inject: key type is not the same");
    }
    fix_fn(input, output, move |v| {
        if let V::Pair(k, _) = &v {
            if t::choice_get(&out_types, k.str()).is_none() {
                refuse("AddNewChoices: Unknown type (IllegalArgumentException)");
            }
        }
        v
    })
}

// NamedEntityFix: a fix of one choice of a type (an entity or block entity).
pub fn named_entity_fix(fx: &Fx, reference: &'static str, entity: &'static str, fix: impl Fn(Typed) -> Typed + 'static) -> RuleRef {
    let finder = named_choice(entity, fx.input().choice_type(reference, entity));
    let out_choice = fx.output().choice_type(reference, entity);
    fix_typed(fx.input().ty(reference), fx.output().ty(reference), move |typed| typed.update_typed_to(&finder, out_choice, &fix))
}

// NamedEntityFix on the choice's remainder.
pub fn named_entity_remainder(fx: &Fx, reference: &'static str, entity: &'static str, fix: impl Fn(Dy) -> Dy + 'static) -> RuleRef {
    let fix = Rc::new(fix);
    named_entity_fix(fx, reference, entity, move |typed| {
        let fix = fix.clone();
        typed.update_remainder(move |d| fix(d))
    })
}

// NamedEntityWriteReadFix: the choice written, fixed as a Dynamic and read
// as the output type.
pub fn named_entity_write_read(fx: &Fx, reference: &'static str, entity: &'static str, fix: impl Fn(Dy) -> Dy + 'static) -> RuleRef {
    let input = fx.input().ty(reference);
    let choice = fx.input().choice_type(reference, entity);
    let output = fx.output().ty(reference);
    let finder = named_choice(entity, choice);
    let patched = patch_sub_type(input, input, output);
    let fix = Rc::new(fix);
    fix_typed(input, output, move |typed| {
        if typed.get_optional(&finder).is_none() {
            return Typed::new(output, typed.v);
        }
        let cast = Typed::new(patched, typed.v);
        let fix = fix.clone();
        write_and_read_typed(&cast, output, move |d| fix(d))
    })
}

// NamedEntityConvertUncheckedFix: the choice's value as the output choice type.
pub fn named_entity_convert_unchecked(fx: &Fx, reference: &'static str, entity: &'static str) -> RuleRef {
    let out_choice = fx.output().choice_type(reference, entity);
    named_entity_fix(fx, reference, entity, move |typed| Typed::new(out_choice, typed.v))
}

// ItemRenameFix.
pub fn item_rename(fx: &Fx, rename: impl Fn(&str) -> String + 'static) -> RuleRef {
    let ty = t::named(ITEM_NAME, t::prim(t::Prim::NamespacedString));
    if !t::same(fx.input().ty(ITEM_NAME), ty) {
        refuse("item name type is not what was expected.");
    }
    fix_fn(ty, ty, move |v| {
        let (n, item) = v.into_pair();
        V::pair(n, V::Str(rename(item.str())))
    })
}

// BlockRenameFix: block names, block states' Name and flat block states.
pub fn block_rename(fx: &Fx, rename: impl Fn(&str) -> String + 'static) -> RuleRef {
    let input = fx.input();
    let ty = t::named(BLOCK_NAME, t::prim(t::Prim::NamespacedString));
    if !t::same(input.ty(BLOCK_NAME), ty) {
        refuse("block type is not what was expected.");
    }
    let rename = Rc::new(rename);
    let r1 = rename.clone();
    let names = fix_fn(ty, ty, move |v| {
        let (n, block) = v.into_pair();
        V::pair(n, V::Str(r1(block.str())))
    });
    let r2 = rename.clone();
    let states = fix_remainder(input.ty(BLOCK_STATE), move |d| match d.get("Name").as_string().result() {
        Some(name) => {
            let renamed = d.create_string(&r2(&name));
            d.set("Name", renamed)
        }
        None => d,
    });
    let r3 = rename.clone();
    let flat = fix_remainder(input.ty(FLAT_BLOCK_STATE), move |d| match d.as_string().result() {
        Some(name) => d.create_string(&fix_flat_block_state(&name, &*r3)),
        None => d,
    });
    rewrite::seq(vec![names, rewrite::seq(vec![states, flat])])
}

// BlockRenameFix.fixFlatBlockState: the name before "[" or "{" renamed.
fn fix_flat_block_state(name: &str, rename: &dyn Fn(&str) -> String) -> String {
    let units: Vec<u16> = name.encode_utf16().collect();
    let index = units.iter().position(|&c| c == '[' as u16).map_or(-1, |i| i as i64);
    let index1 = units.iter().position(|&c| c == '{' as u16).map_or(-1, |i| i as i64);
    let mut len = units.len() as i64;
    if index > 0 {
        len = index;
    }
    if index1 > 0 {
        len = len.min(index1);
    }
    let sub = String::from_utf16_lossy(&units[..len as usize]);
    let rest = String::from_utf16_lossy(&units[len as usize..]);
    rename(&sub) + &rest
}

// DataFixers.createRenamer(map): the namespaced name looked up, or the name.
pub fn renamer(map: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> String + 'static {
    move |name| {
        let key = t::ensure_namespaced(name);
        map.iter().find(|(k, _)| *k == key).map_or_else(|| name.to_string(), |(_, v)| v.to_string())
    }
}

// DataFixers.createRenamerNoNamespace.
pub fn renamer_no_namespace(map: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> String + 'static {
    move |name| map.iter().find(|(k, _)| *k == name).map_or_else(|| name.to_string(), |(_, v)| v.to_string())
}

// Every fix, in DataFixers.addFixers' order.
pub fn registry() -> Vec<Def> {
    let mut defs = Vec::new();
    for (key, class, imp) in [v1::defs(), v2::defs(), v3::defs(), v4::defs()].into_iter().flatten() {
        defs.push(Def { key, class, imp });
    }
    defs
}

#[cfg(test)]
mod tests {
    // The registry's fixes are the server's, in order, and those it calls
    // irrelevant change none of the upgraded types (the dump's F lines).
    #[test]
    fn the_registry_is_the_servers_list_of_fixes() {
        let dump: Vec<(i32, String, bool)> = super::schemas::DUMP
            .lines()
            .filter_map(|l| l.strip_prefix("F "))
            .map(|l| {
                let p: Vec<&str> = l.split(' ').collect();
                (p[1].parse().unwrap(), p[2].to_string(), p[3] == "1")
            })
            .collect();
        let defs = super::registry();
        assert_eq!(defs.len(), dump.len());
        for (def, (key, class, relevant)) in defs.iter().zip(&dump) {
            assert_eq!((def.key, def.class), (*key, class.as_str()));
            assert_eq!(matches!(def.imp, super::Imp::I), !relevant, "{key} {class}");
        }
    }
}
