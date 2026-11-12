// Minecraft's data fixer (DataFixers.getDataFixer(): Mojang's DataFixerUpper
// with the server's schemas and fixes), ported for what the engines import
// at an older data version: block entities, entities, item stacks and block
// states. DataFixer.update reads the data as the type of its version's
// schema, rewrites the type through every fix after that version (each
// fix's rule, which says where in the type its function runs), applies the
// functions and writes the result as the new type onto what was read.
//
// The schemas come from the server itself (core/minecraft/<version>/
// datafix.txt, scripts/DataFixReference.java --dump); the fixes are ported
// one by one (fixes/). An update through a fix this port does not hold is
// refused ("unported: ..."), and so is what DFU would only log or throw on
// in a way this port does not model.

mod dr;
mod fixes;
mod gson;
mod hooks;
pub mod ops;
mod optic;
mod rewrite;
mod schemas;
mod tables;
mod types;

use crate::level::refuse;
use crate::nbt::Tag;
use dr::Dr;
use rewrite::{RuleRef, RR};
use std::cell::RefCell;
use std::collections::HashMap;
use types::TyId;

pub use crate::chunk_save::DATA_VERSION;

// The types an import upgrades (References).
pub const BLOCK_ENTITY: &str = "block_entity";
pub const ENTITY: &str = "entity";
pub const ENTITY_TREE: &str = "entity_tree";
pub const ITEM_STACK: &str = "item_stack";
pub const BLOCK_STATE: &str = "block_state";

struct Fixer {
    defs: Vec<fixes::Def>,
    // DataFixerUpper.fixerVersions.
    keys: Vec<i32>,
    rules: HashMap<usize, RuleRef>,
    chains: HashMap<(i32, i32), RuleRef>,
    rewritten: HashMap<(TyId, u64), Option<RR>>,
}

thread_local! {
    static FIXER: RefCell<Option<Fixer>> = const { RefCell::new(None) };
}

fn with_fixer<R>(f: impl FnOnce(&mut Fixer) -> R) -> R {
    FIXER.with(|x| {
        if x.borrow().is_none() {
            let defs = fixes::registry();
            let mut keys: Vec<i32> = defs.iter().map(|d| d.key).collect();
            keys.sort();
            keys.dedup();
            *x.borrow_mut() = Some(Fixer { defs, keys, rules: HashMap::new(), chains: HashMap::new(), rewritten: HashMap::new() });
        }
        // Rules are built outside the borrow (they read schemas and types).
        let mut taken = x.borrow_mut().take().unwrap();
        let r = f(&mut taken);
        *x.borrow_mut() = Some(taken);
        r
    })
}

// DataFixerUpper.getRule(version, newVersion): every fix after the last fix
// key at or before the version's key, up to the new version, in order.
fn rule(fixer: &mut Fixer, version: i32, new_version: i32) -> RuleRef {
    if let Some(r) = fixer.chains.get(&(version, new_version)) {
        return r.clone();
    }
    let key = version * 10;
    let expanded = if key < fixer.keys[0] { fixer.keys[0] - 1 } else { *fixer.keys.iter().filter(|k| **k <= key).last().unwrap() };
    let mut rules = Vec::new();
    for index in 0..fixer.defs.len() {
        let def = &fixer.defs[index];
        if def.key <= expanded || def.key / 10 > new_version {
            continue;
        }
        match def.imp {
            fixes::Imp::I => {}
            fixes::Imp::U => refuse(format!("data of version {version}, which the data fixer's {} (data version {}) changes, not ported", def.class, def.key / 10)),
            fixes::Imp::R(changes, make) => {
                let r = match fixer.rules.get(&index) {
                    Some(r) => r.clone(),
                    None => {
                        let r = make(&fixes::Fx { key: def.key, changes });
                        fixer.rules.insert(index, r.clone());
                        r
                    }
                };
                rules.push(r);
            }
        }
    }
    let r = rewrite::seq(rules);
    fixer.chains.insert((version, new_version), r.clone());
    r
}

// DataFixer.update(type, input, version, DATA_VERSION).
pub fn update(reference: &str, input: Tag, version: i32) -> Tag {
    let new_version = DATA_VERSION as i32;
    if version >= new_version {
        return input;
    }
    with_fixer(|fixer| {
        let data_type = schemas::schema(version * 10).type_raw(reference);
        let expected = schemas::schema(new_version * 10).type_raw(reference);
        let rule = rule(fixer, version, new_version);
        let rr = match fixer.rewritten.get(&(data_type, rule.id)) {
            Some(rr) => rr.clone(),
            None => {
                let rr = rewrite::rewrite(&rule, data_type);
                fixer.rewritten.insert((data_type, rule.id), rr.clone());
                rr
            }
        };
        // Type.readAndWrite: a type whose rewrite is a nop is returned as it
        // is; a failure leaves the input (DataFixerUpper logs it).
        let Some(rr) = rr else { return input };
        if rr.is_nop() {
            return input;
        }
        let input_nbt = Some(input.clone());
        let result: Dr<ops::Nbt> = types::decode(data_type, &input_nbt).flat_map(|(value, rest)| {
            if !types::eq(expected, rr.new_ty, true, true) {
                return Dr::err();
            }
            let value = rr.apply(value);
            types::encode(rr.new_ty, &value, rest)
        });
        match result.result_or_partial() {
            Some(Some(tag)) => tag,
            Some(None) => refuse("the data fixer wrote EndTag"),
            None => input,
        }
    })
}

// update of a compound the caller casts back to CompoundTag (a fixer that
// wrote anything else is a ClassCastException on the server).
pub fn update_compound(reference: &str, input: crate::nbt::Compound, version: i32) -> crate::nbt::Compound {
    match update(reference, Tag::Compound(input), version) {
        Tag::Compound(c) => c,
        other => refuse(format!("the data fixer upgraded a {reference} to a {} (a ClassCastException on the server)", other.type_name())),
    }
}

// Each line "<type> <version> <snbt>" upgraded, as DataFixReference
// --upgrade answers it: "ok <snbt>", or the refusal (scripts/datafix-parity.js).
pub fn run_lines(text: &str) -> String {
    let mut out = String::new();
    for line in text.lines() {
        let mut p = line.splitn(3, ' ');
        let (reference, version, snbt) = (p.next().unwrap().to_string(), p.next().unwrap().parse::<i32>().unwrap(), p.next().unwrap().to_string());
        let answer = std::panic::catch_unwind(move || match crate::nbt::parse_compound(&snbt) {
            Err(e) => format!("unparsed {e:?}"),
            Ok(c) => format!("ok {}", update(&reference, Tag::Compound(c), version)),
        });
        match answer {
            Ok(a) => out.push_str(&a),
            Err(payload) => out.push_str(&crate::failure(&*payload)),
        }
        out.push('\n');
    }
    out
}
