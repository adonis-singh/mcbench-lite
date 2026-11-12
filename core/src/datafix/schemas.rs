// The fixer's schemas (DataFixers' addSchema calls), read from the dump
// scripts/DataFixReference.java writes (core/minecraft/<version>/datafix.txt):
// every schema's type templates, by version key, and the tables the hooks
// and fixes read.
//
//   T <id> <template>      a template node (children before parents)
//   S <key> <class> <plain|namespaced> recursive <name=index>... types <name=template>...
//   X <table> <key> [<value>]
//   F <index> <key> <class> <0|1>   the fixes in order, and whether they change the types the engines upgrade

use super::hooks::Hook;
use super::types::{self as t, Prim, Tpl, TplId, TyId};
use crate::level::refuse;
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

pub const DUMP: &str = include_str!("../../minecraft/1.21.10/datafix.txt");

pub struct Schema {
    pub key: i32,
    pub namespaced: bool,
    // Schema.TYPES: a recursive type is its recursion point.
    pub raw: HashMap<String, TyId>,
}

struct Schemas {
    by_key: BTreeMap<i32, Rc<Schema>>,
}

thread_local! {
    static SCHEMAS: RefCell<Option<Rc<Schemas>>> = const { RefCell::new(None) };
}

fn prim(name: &str) -> Prim {
    match name {
        "prim:Bool" => Prim::Bool,
        "prim:Int" => Prim::Int,
        "prim:Long" => Prim::Long,
        "prim:Byte" => Prim::Byte,
        "prim:Short" => Prim::Short,
        "prim:Float" => Prim::Float,
        "prim:Double" => Prim::Double,
        "prim:String" => Prim::String,
        "prim:NamespacedString" => Prim::NamespacedString,
        other => refuse(format!("the data fixer's primitive type {other}")),
    }
}

fn const_type(name: &str) -> TyId {
    match name {
        "EmptyPart" => t::empty_part(),
        "EmptyPartPassthrough" => t::remainder(),
        other => t::prim(prim(other)),
    }
}

fn load() -> Rc<Schemas> {
    let mut nodes: Vec<TplId> = Vec::new();
    let mut by_key = BTreeMap::new();
    for line in DUMP.lines() {
        let p: Vec<&str> = line.split(' ').collect();
        match p[0] {
            "T" => {
                let id: usize = p[1].parse().unwrap();
                assert_eq!(id, nodes.len());
                let n = |i: usize| nodes[p[i].parse::<usize>().unwrap()];
                let node = match p[2] {
                    "const" => Tpl::Const(const_type(p[3])),
                    "named" => Tpl::Named(p[3].into(), n(4)),
                    "field" => Tpl::Field(p[3].into(), n(4)),
                    "and" => Tpl::And(n(3), n(4)),
                    "or" => Tpl::Or(n(3), n(4)),
                    "list" => Tpl::List(n(3)),
                    "clist" => Tpl::CList(n(3), n(4)),
                    "hook" => Tpl::Hook(n(3), Hook::parse(p[4]), Hook::parse(p[5])),
                    "check" => Tpl::Check(p[3].into(), p[4].parse().unwrap(), n(5)),
                    "id" => Tpl::Id(p[3].parse().unwrap()),
                    "choice" => {
                        let key = prim(p[4]);
                        let count: usize = p[5].parse().unwrap();
                        let mut choices = Vec::with_capacity(count);
                        for i in 0..count {
                            choices.push((Rc::from(p[6 + 2 * i]), n(7 + 2 * i)));
                        }
                        choices.sort_by(|a: &(Rc<str>, TplId), b| a.0.cmp(&b.0));
                        Tpl::Choice(p[3].into(), key, Rc::new(choices))
                    }
                    other => panic!("datafix.txt: unknown template {other}"),
                };
                nodes.push(t::mk_tpl(node));
            }
            "S" => {
                let key: i32 = p[1].parse().unwrap();
                let namespaced = p[3] == "namespaced";
                let types_at = p.iter().position(|x| *x == "types").unwrap();
                let recursive: Vec<(&str, i32)> = p[5..types_at]
                    .iter()
                    .map(|x| {
                        let (n, i) = x.split_once('=').unwrap();
                        (n, i.parse().unwrap())
                    })
                    .collect();
                let templates: HashMap<&str, TplId> = p[types_at + 1..]
                    .iter()
                    .map(|x| {
                        let (n, i) = x.split_once('=').unwrap();
                        (n, nodes[i.parse::<usize>().unwrap()])
                    })
                    .collect();
                // Schema.buildTypes: the recursive types' checks reduced into
                // one sum, in the map's order.
                let named = |name: &str| t::mk_tpl(Tpl::Named(name.into(), templates[name]));
                let mut family: Option<TplId> = None;
                for (name, index) in &recursive {
                    let check = t::mk_tpl(Tpl::Check((*name).into(), *index, named(name)));
                    family = Some(match family {
                        None => check,
                        Some(f) => t::mk_tpl(Tpl::Or(f, check)),
                    });
                }
                let fam = family.expect("a schema with recursive types");
                let mut raw = HashMap::new();
                for name in templates.keys() {
                    let ty = match recursive.iter().find(|(n, _)| n == name) {
                        Some((_, index)) => t::mk(t::Ty::Mu(fam, *index)),
                        None => t::apply(named(name), fam, -1),
                    };
                    raw.insert(name.to_string(), ty);
                }
                by_key.insert(key, Rc::new(Schema { key, namespaced, raw }));
            }
            _ => {}
        }
    }
    Rc::new(Schemas { by_key })
}

fn schemas() -> Rc<Schemas> {
    if let Some(s) = SCHEMAS.with(|s| s.borrow().clone()) {
        return s;
    }
    let s = load();
    SCHEMAS.with(|x| *x.borrow_mut() = Some(s.clone()));
    s
}

// DataFixerUpper.getSchema: the last schema at or before a version key (the
// first for keys before it).
pub fn schema(key: i32) -> Rc<Schema> {
    let s = schemas();
    match s.by_key.range(..=key).next_back() {
        Some((_, schema)) => schema.clone(),
        None => s.by_key.values().next().unwrap().clone(),
    }
}

// Schema.getParent: the schema before this one.
pub fn parent(key: i32) -> Rc<Schema> {
    let s = schemas();
    s.by_key.range(..key).next_back().map(|(_, v)| v.clone()).expect("a schema with a parent")
}

impl Schema {
    pub fn type_raw(&self, name: &str) -> TyId {
        match self.raw.get(name) {
            Some(t) => *t,
            None => refuse(format!("the data fixer has no type {name} in schema {} (IllegalArgumentException)", self.key)),
        }
    }

    // Schema.getType: a recursive type's checked (named) type.
    pub fn ty(&self, name: &str) -> TyId {
        let raw = self.type_raw(name);
        match t::ty(raw) {
            t::Ty::Mu(_, _) => t::find_checked_type(raw, -1).unwrap_or_else(|| refuse("Could not find choice type in the recursive type")),
            _ => raw,
        }
    }

    // Schema.findChoiceType: the type's "id" choice.
    pub fn find_choice_type(&self, name: &str) -> TyId {
        t::find_choice_type(self.ty(name), "id", -1).unwrap_or_else(|| refuse("Not a choice type"))
    }

    // Schema.getChoiceType (a namespaced schema namespaces the name).
    pub fn choice_type(&self, name: &str, choice: &str) -> TyId {
        let choice = if self.namespaced { t::ensure_namespaced(choice) } else { choice.to_string() };
        let t::Ty::Choice(_, _, types) = t::ty(self.find_choice_type(name)) else { unreachable!() };
        t::choice_get(&types, &choice).unwrap_or_else(|| refuse(format!("Data fixer not registered for: {choice} in {name}")))
    }
}
