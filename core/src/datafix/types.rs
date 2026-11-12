// DataFixerUpper's types and type templates (com.mojang.datafixers.types):
// what a schema says saved data holds, and the values the fixer reads it
// into. Types and templates are interned, so structurally equal ones share
// an id; a recursive type family is its template's id, as DFU's families are
// equal when their interned templates are.
//
// A value mirrors DFU's: a product is a Pair, a sum an Either, a named type
// a Pair of its name and value, a tagged choice a Pair of its key and value,
// the remainder a Dynamic.

use super::dr::{ap2, Dr};
use super::hooks::Hook;
use super::ops::{self, Dy, Nbt};
use crate::level::refuse;
use crate::nbt::{Compound, Tag};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

pub type TyId = u32;
pub type TplId = u32;
// A recursive type family: the id of its (interned) template.
pub type Fam = TplId;

// The primitive types (Const.PrimitiveType), each its own instance: DSL's and
// NamespacedSchema's namespaced string.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Prim {
    Bool,
    Int,
    Long,
    Byte,
    Short,
    Float,
    Double,
    String,
    NamespacedString,
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum Ty {
    Prim(Prim),
    EmptyPart,
    // EmptyPartPassthrough: the remainder.
    Passthrough,
    Product(TyId, TyId),
    Sum(TyId, TyId),
    List(TyId),
    // Tag.TagType: a field of a map.
    Field(Rc<str>, TyId),
    Named(Rc<str>, TyId),
    // TaggedChoiceType: name, key type, the type of each key (sorted).
    Choice(Rc<str>, Prim, Rc<Vec<(Rc<str>, TyId)>>),
    CList(TyId, TyId),
    Hook(TyId, Hook, Hook),
    // CheckType: name, index, expected index, delegate.
    Check(Rc<str>, i32, i32, TyId),
    // RecursivePointType: family, index.
    Mu(Fam, i32),
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum Tpl {
    Const(TyId),
    Named(Rc<str>, TplId),
    Field(Rc<str>, TplId),
    And(TplId, TplId),
    Or(TplId, TplId),
    List(TplId),
    CList(TplId, TplId),
    Choice(Rc<str>, Prim, Rc<Vec<(Rc<str>, TplId)>>),
    Hook(TplId, Hook, Hook),
    Check(Rc<str>, i32, TplId),
    Id(i32),
}

#[derive(Default)]
struct Arena {
    tys: Vec<Ty>,
    ty_ids: HashMap<Ty, TyId>,
    tpls: Vec<Tpl>,
    tpl_ids: HashMap<Tpl, TplId>,
    applied: HashMap<(TplId, Fam, i32), TyId>,
    templates: HashMap<TyId, TplId>,
    sizes: HashMap<TplId, i32>,
    eqs: HashMap<(TyId, TyId, bool, bool), bool>,
    has_id: HashMap<TplId, bool>,
}

thread_local! {
    static ARENA: RefCell<Arena> = RefCell::new(Arena::default());
}

pub fn mk(t: Ty) -> TyId {
    ARENA.with(|a| {
        let mut a = a.borrow_mut();
        if let Some(&id) = a.ty_ids.get(&t) {
            return id;
        }
        let id = a.tys.len() as TyId;
        a.tys.push(t.clone());
        a.ty_ids.insert(t, id);
        id
    })
}

pub fn ty(id: TyId) -> Ty {
    ARENA.with(|a| a.borrow().tys[id as usize].clone())
}

pub fn mk_tpl(t: Tpl) -> TplId {
    ARENA.with(|a| {
        let mut a = a.borrow_mut();
        if let Some(&id) = a.tpl_ids.get(&t) {
            return id;
        }
        let id = a.tpls.len() as TplId;
        a.tpls.push(t.clone());
        a.tpl_ids.insert(t, id);
        id
    })
}

pub fn tpl(id: TplId) -> Tpl {
    ARENA.with(|a| a.borrow().tpls[id as usize].clone())
}

pub fn prim(p: Prim) -> TyId {
    mk(Ty::Prim(p))
}

pub fn product(a: TyId, b: TyId) -> TyId {
    mk(Ty::Product(a, b))
}

pub fn sum(a: TyId, b: TyId) -> TyId {
    mk(Ty::Sum(a, b))
}

pub fn list(e: TyId) -> TyId {
    mk(Ty::List(e))
}

pub fn field(name: &str, e: TyId) -> TyId {
    mk(Ty::Field(name.into(), e))
}

pub fn named(name: &str, e: TyId) -> TyId {
    mk(Ty::Named(name.into(), e))
}

pub fn remainder() -> TyId {
    mk(Ty::Passthrough)
}

pub fn empty_part() -> TyId {
    mk(Ty::EmptyPart)
}

// DSL.optional: (type | EmptyPart).
pub fn optional(e: TyId) -> TyId {
    sum(e, empty_part())
}

pub fn choice(name: &str, key: Prim, types: Vec<(Rc<str>, TyId)>) -> TyId {
    let mut types = types;
    types.sort_by(|a, b| a.0.cmp(&b.0));
    mk(Ty::Choice(name.into(), key, Rc::new(types)))
}

pub fn choice_get(types: &[(Rc<str>, TyId)], key: &str) -> Option<TyId> {
    types.binary_search_by(|e| (*e.0).cmp(key)).ok().map(|at| types[at].1)
}

// TypeTemplate.size.
pub fn size(t: TplId) -> i32 {
    if let Some(s) = ARENA.with(|a| a.borrow().sizes.get(&t).copied()) {
        return s;
    }
    let s = match tpl(t) {
        Tpl::Const(_) => 0,
        Tpl::Named(_, e) | Tpl::Field(_, e) | Tpl::List(e) | Tpl::Hook(e, _, _) => size(e),
        Tpl::And(a, b) | Tpl::Or(a, b) | Tpl::CList(a, b) => size(a).max(size(b)),
        Tpl::Choice(_, _, ts) => ts.iter().map(|(_, e)| size(*e)).max().unwrap_or(0),
        Tpl::Check(_, index, e) => (index + 1).max(size(e)),
        Tpl::Id(i) => i + 1,
    };
    ARENA.with(|a| a.borrow_mut().sizes.insert(t, s));
    s
}

// Whether a template holds a recursion point.
pub fn has_id(t: TplId) -> bool {
    if let Some(s) = ARENA.with(|a| a.borrow().has_id.get(&t).copied()) {
        return s;
    }
    let s = match tpl(t) {
        Tpl::Const(_) => false,
        Tpl::Named(_, e) | Tpl::Field(_, e) | Tpl::List(e) | Tpl::Hook(e, _, _) | Tpl::Check(_, _, e) => has_id(e),
        Tpl::And(a, b) | Tpl::Or(a, b) | Tpl::CList(a, b) => has_id(a) || has_id(b),
        Tpl::Choice(_, _, ts) => ts.iter().any(|(_, e)| has_id(*e)),
        Tpl::Id(_) => true,
    };
    ARENA.with(|a| a.borrow_mut().has_id.insert(t, s));
    s
}

// TypeTemplate.apply(family).apply(index).
pub fn apply(t: TplId, fam: Fam, index: i32) -> TyId {
    if let Some(id) = ARENA.with(|a| a.borrow().applied.get(&(t, fam, index)).copied()) {
        return id;
    }
    let id = match tpl(t) {
        Tpl::Const(c) => c,
        Tpl::Named(n, e) => mk(Ty::Named(n, apply(e, fam, index))),
        Tpl::Field(n, e) => mk(Ty::Field(n, apply(e, fam, index))),
        Tpl::And(a, b) => product(apply(a, fam, index), apply(b, fam, index)),
        Tpl::Or(a, b) => sum(apply(a, fam, index), apply(b, fam, index)),
        Tpl::List(e) => list(apply(e, fam, index)),
        Tpl::CList(k, e) => mk(Ty::CList(apply(k, fam, index), apply(e, fam, index))),
        Tpl::Choice(n, k, ts) => mk(Ty::Choice(n, k, Rc::new(ts.iter().map(|(key, e)| (key.clone(), apply(*e, fam, index))).collect()))),
        Tpl::Hook(e, pre, post) => mk(Ty::Hook(apply(e, fam, index), pre, post)),
        Tpl::Check(n, expected, e) => {
            assert!(index >= 0, "IndexOutOfBoundsException: a check type at index {index}");
            mk(Ty::Check(n, index, expected, apply(e, fam, index)))
        }
        Tpl::Id(i) => mk(Ty::Mu(fam, i)),
    };
    ARENA.with(|a| a.borrow_mut().applied.insert((t, fam, index), id));
    id
}

// RecursivePointType.unfold: the family's template at the point's index.
pub fn unfold(fam: Fam, index: i32) -> TyId {
    apply(fam, fam, index)
}

// Type.template (buildTemplate).
pub fn template(t: TyId) -> TplId {
    if let Some(id) = ARENA.with(|a| a.borrow().templates.get(&t).copied()) {
        return id;
    }
    let id = match ty(t) {
        Ty::Prim(_) | Ty::EmptyPart | Ty::Passthrough => mk_tpl(Tpl::Const(t)),
        Ty::Product(a, b) => mk_tpl(Tpl::And(template(a), template(b))),
        Ty::Sum(a, b) => mk_tpl(Tpl::Or(template(a), template(b))),
        Ty::List(e) => mk_tpl(Tpl::List(template(e))),
        Ty::Field(n, e) => mk_tpl(Tpl::Field(n, template(e))),
        Ty::Named(n, e) => mk_tpl(Tpl::Named(n, template(e))),
        Ty::Choice(n, k, ts) => mk_tpl(Tpl::Choice(n, k, Rc::new(ts.iter().map(|(key, e)| (key.clone(), template(*e))).collect()))),
        Ty::CList(k, e) => mk_tpl(Tpl::CList(template(k), template(e))),
        Ty::Hook(e, pre, post) => mk_tpl(Tpl::Hook(template(e), pre, post)),
        Ty::Check(n, _, expected, e) => mk_tpl(Tpl::Check(n, expected, template(e))),
        Ty::Mu(_, i) => mk_tpl(Tpl::Id(i)),
    };
    ARENA.with(|a| a.borrow_mut().templates.insert(t, id));
    id
}

// Type.equals(other, ignoreRecursionPoints, checkIndex).
pub fn eq(a: TyId, b: TyId, ignore: bool, check: bool) -> bool {
    if a == b {
        return true;
    }
    if let Some(r) = ARENA.with(|x| x.borrow().eqs.get(&(a, b, ignore, check)).copied()) {
        return r;
    }
    let r = match (ty(a), ty(b)) {
        (Ty::Product(a1, a2), Ty::Product(b1, b2)) | (Ty::Sum(a1, a2), Ty::Sum(b1, b2)) | (Ty::CList(a1, a2), Ty::CList(b1, b2)) => eq(a1, b1, ignore, check) && eq(a2, b2, ignore, check),
        (Ty::List(x), Ty::List(y)) => eq(x, y, ignore, check),
        (Ty::Field(n, x), Ty::Field(m, y)) | (Ty::Named(n, x), Ty::Named(m, y)) => n == m && eq(x, y, ignore, check),
        (Ty::Choice(n, k, xs), Ty::Choice(m, l, ys)) => n == m && k == l && xs.len() == ys.len() && xs.iter().all(|(key, x)| choice_get(&ys, key).is_some_and(|y| eq(*x, y, ignore, check))),
        (Ty::Hook(x, p, q), Ty::Hook(y, r, s)) => eq(x, y, ignore, check) && p == r && q == s,
        (Ty::Check(_, i, e, x), Ty::Check(_, j, f, y)) => i == j && e == f && (!check || eq(x, y, ignore, check)),
        (Ty::Mu(f, i), Ty::Mu(g, j)) => (ignore || f == g) && i == j,
        _ => false,
    };
    ARENA.with(|x| x.borrow_mut().eqs.insert((a, b, ignore, check), r));
    r
}

// Objects.equals of two types: equals(other, false, true).
pub fn same(a: TyId, b: TyId) -> bool {
    eq(a, b, false, true)
}

// RecursiveTypeFamily.buildMuType: the recursion point of the family (made
// from the new type's template when none is given) whose unfolded type is
// the new type.
pub fn build_mu_type(this_family: Fam, new_type: TyId, new_family: Option<Fam>) -> TyId {
    let fam = new_family.unwrap_or_else(|| template(new_type));
    for i in 0..size(fam) {
        if eq(new_type, unfold(fam, i), true, false) {
            return mk(Ty::Mu(fam, i));
        }
    }
    let _ = this_family;
    refuse("the data fixer could not determine a rewritten recursive type (IllegalStateException)")
}

// Type.findChoiceType(name, index).
pub fn find_choice_type(t: TyId, name: &str, index: i32) -> Option<TyId> {
    match ty(t) {
        Ty::Choice(n, _, _) => (&*n == name).then_some(t),
        Ty::Product(a, b) | Ty::Sum(a, b) => find_choice_type(a, name, index).or_else(|| find_choice_type(b, name, index)),
        Ty::Named(_, e) | Ty::Hook(e, _, _) => find_choice_type(e, name, index),
        Ty::Check(_, _, expected, e) => (index == expected).then(|| find_choice_type(e, name, index)).flatten(),
        Ty::Mu(fam, i) => find_choice_type(unfold(fam, i), name, i),
        _ => None,
    }
}

// Type.findCheckedType(index).
pub fn find_checked_type(t: TyId, index: i32) -> Option<TyId> {
    match ty(t) {
        Ty::Product(a, b) | Ty::Sum(a, b) => find_checked_type(a, index).or_else(|| find_checked_type(b, index)),
        Ty::Named(_, e) | Ty::Hook(e, _, _) => find_checked_type(e, index),
        Ty::Check(_, _, expected, e) => (index == expected).then_some(e),
        Ty::Mu(fam, i) => find_checked_type(unfold(fam, i), i),
        Ty::Choice(_, _, ts) => ts.iter().find_map(|(_, e)| find_checked_type(*e, index)),
        _ => None,
    }
}

// Type.findFieldTypeOpt(name). A tagged choice looks in its choices in its
// map's order; this port takes the answer only when every choice that has
// the field gives the same type.
pub fn find_field_type(t: TyId, name: &str) -> Option<TyId> {
    match ty(t) {
        Ty::Product(a, b) | Ty::Sum(a, b) => find_field_type(a, name).or_else(|| find_field_type(b, name)),
        Ty::Field(n, e) => (&*n == name).then_some(e),
        Ty::Named(_, e) | Ty::Hook(e, _, _) => find_field_type(e, name),
        Ty::Check(_, i, expected, e) => (i == expected).then(|| find_field_type(e, name)).flatten(),
        Ty::Mu(fam, i) => find_field_type(unfold(fam, i), name),
        Ty::Choice(_, _, ts) => {
            let found: Vec<TyId> = ts.iter().filter_map(|(_, e)| find_field_type(*e, name)).collect();
            if found.windows(2).any(|w| !same(w[0], w[1])) {
                refuse("a field found with different types in a tagged choice's choices (their order is a hash map's)");
            }
            found.first().copied()
        }
        _ => None,
    }
}

// The values the fixer reads saved data into.
#[derive(Clone, Debug)]
pub enum V {
    Unit,
    Dyn(Nbt),
    Bool(bool),
    Int(i32),
    Long(i64),
    Byte(i8),
    Short(i16),
    Float(f32),
    Double(f64),
    Str(String),
    Pair(Box<V>, Box<V>),
    Left(Box<V>),
    Right(Box<V>),
    List(Vec<V>),
}

impl V {
    pub fn pair(a: V, b: V) -> V {
        V::Pair(Box::new(a), Box::new(b))
    }

    pub fn str(&self) -> &str {
        match self {
            V::Str(s) => s,
            other => refuse(format!("the data fixer expected a string value, not {other:?}")),
        }
    }

    pub fn dy(self) -> Dy {
        match self {
            V::Dyn(n) => Dy(n),
            other => refuse(format!("the data fixer expected a dynamic value, not {other:?}")),
        }
    }

    pub fn into_pair(self) -> (V, V) {
        match self {
            V::Pair(a, b) => (*a, *b),
            other => refuse(format!("the data fixer expected a pair, not {other:?}")),
        }
    }
}

// Type.point: a type's default value.
pub fn point(t: TyId) -> Option<V> {
    match ty(t) {
        Ty::EmptyPart => Some(V::Unit),
        Ty::Passthrough => Some(V::Dyn(None)),
        Ty::Prim(_) => None,
        Ty::Product(a, b) => Some(V::pair(point(a)?, point(b)?)),
        Ty::Sum(a, b) => point(b).map(|v| V::Right(Box::new(v))).or_else(|| point(a).map(|v| V::Left(Box::new(v)))),
        Ty::List(_) | Ty::CList(_, _) => Some(V::List(Vec::new())),
        Ty::Field(_, e) | Ty::Hook(e, _, _) => point(e),
        Ty::Named(n, e) => point(e).map(|v| V::pair(V::Str(n.to_string()), v)),
        Ty::Check(_, i, expected, e) => (i == expected).then(|| point(e)).flatten(),
        Ty::Mu(fam, i) => point(unfold(fam, i)),
        Ty::Choice(_, _, ts) => {
            let with: Vec<_> = ts.iter().filter_map(|(k, e)| point(*e).map(|v| (k.clone(), v))).collect();
            match with.len() {
                0 => None,
                1 => Some(V::pair(V::Str(with[0].0.to_string()), with[0].1.clone())),
                _ => {
                    let keys: Vec<&str> = ts.iter().map(|(k, _)| &**k).collect();
                    match super::tables::choice_point(&keys.join(",")) {
                        Some(first) => with.iter().find(|(k, _)| &**k == first).map(|(k, v)| V::pair(V::Str(k.to_string()), v.clone())),
                        None => refuse("the default value of a tagged choice (the first of its hash map that has one)"),
                    }
                }
            }
        }
    }
}

// NamespacedSchema.ensureNamespaced: ResourceLocation.tryParse's form, or
// the text when it is no resource location.
pub fn ensure_namespaced(text: &str) -> String {
    crate::codec::resource_location(text).unwrap_or_else(|_| text.to_string())
}

fn read_prim(p: Prim, input: &Nbt) -> Dr<V> {
    let number = || ops::get_number(input);
    let v = match p {
        Prim::Bool => number().map(|n| V::Bool(n.byte_value() != 0)),
        Prim::Int => number().map(|n| V::Int(n.int_value())),
        Prim::Long => number().map(|n| V::Long(n.long_value())),
        Prim::Byte => number().map(|n| V::Byte(n.byte_value())),
        Prim::Short => number().map(|n| V::Short(n.short_value())),
        Prim::Float => number().map(|n| V::Float(n.float_value())),
        Prim::Double => number().map(|n| V::Double(n.double_value())),
        Prim::String => ops::get_string(input).map(|s| V::Str(s.to_string())),
        Prim::NamespacedString => ops::get_string(input).map(|s| V::Str(ensure_namespaced(s))),
    };
    match v {
        Some(v) => Dr::Ok(v),
        None => Dr::err(),
    }
}

fn write_prim(p: Prim, v: &V) -> Tag {
    match (p, v) {
        (Prim::Bool, V::Bool(b)) => Tag::Byte(*b as i8),
        (Prim::Int, V::Int(x)) => Tag::Int(*x),
        (Prim::Long, V::Long(x)) => Tag::Long(*x),
        (Prim::Byte, V::Byte(x)) => Tag::Byte(*x),
        (Prim::Short, V::Short(x)) => Tag::Short(*x),
        (Prim::Float, V::Float(x)) => Tag::Float(*x),
        (Prim::Double, V::Double(x)) => Tag::Double(*x),
        (Prim::String | Prim::NamespacedString, V::Str(s)) => Tag::String(s.clone()),
        (p, v) => refuse(format!("the data fixer wrote a {v:?} as a {p:?} (ClassCastException)")),
    }
}

// Codec.parse: the decoded value without its rest.
pub fn parse(t: TyId, input: &Nbt) -> Dr<V> {
    decode(t, input).map(|(v, _)| v)
}

// Type.codec().decode(ops, input): the value and the rest of the input.
pub fn decode(t: TyId, input: &Nbt) -> Dr<(V, Nbt)> {
    match ty(t) {
        Ty::Prim(p) => read_prim(p, input).map(|v| (v, None)),
        // Codec.EMPTY.codec(): a map, which it leaves whole.
        Ty::EmptyPart => match ops::get_map(input) {
            Some(_) => Dr::Ok((V::Unit, input.clone())),
            None => Dr::err(),
        },
        // Codec.PASSTHROUGH.
        Ty::Passthrough => Dr::Ok((V::Dyn(input.clone()), None)),
        Ty::Product(a, b) => decode(a, input).flat_map(|(va, rest)| decode(b, &rest).map(|(vb, rest2)| (V::pair(va, vb), rest2))),
        // Codec.either.
        Ty::Sum(a, b) => {
            let first = decode(a, input).map(|(v, r)| (V::Left(Box::new(v)), r));
            if first.is_ok() {
                return first;
            }
            let second = decode(b, input).map(|(v, r)| (V::Right(Box::new(v)), r));
            if second.is_ok() {
                second
            } else if first.has_result_or_partial() {
                first
            } else if second.has_result_or_partial() {
                second
            } else {
                Dr::err()
            }
        }
        Ty::List(e) => decode_list(e, input),
        // element.fieldOf(name).codec(): the input stays the rest.
        Ty::Field(name, e) => match ops::get_map(input) {
            None => Dr::err(),
            Some(m) => match m.get(&name) {
                None => Dr::err(),
                Some(value) => parse(e, &Some(value.clone())).map(|v| (v, input.clone())),
            },
        },
        Ty::Named(name, e) => decode(e, input).map(|(v, r)| (V::pair(V::Str(name.to_string()), v), r)),
        Ty::Choice(name, key, types) => decode_choice(&name, key, &types, input),
        Ty::CList(k, e) => decode_clist(k, e, input),
        Ty::Hook(e, pre, _) => decode(e, &pre.apply(input.clone())),
        Ty::Check(_, index, expected, e) => {
            if index != expected {
                Dr::err()
            } else {
                decode(e, input)
            }
        }
        Ty::Mu(fam, i) => decode(unfold(fam, i), input),
    }
}

// ListCodec.decode: every element read (an element read in part counts with
// its partial value); any error makes the whole an error whose partial
// result is the list read, and whose rest lists the elements that failed.
fn decode_list(e: TyId, input: &Nbt) -> Dr<(V, Nbt)> {
    let Some(items) = ops::get_stream(input) else { return Dr::err() };
    let mut elements = Vec::new();
    let mut failed = Vec::new();
    let mut ok = true;
    for item in items {
        let r = decode(e, &Some(item.clone()));
        if !r.is_ok() {
            failed.push(item);
            ok = false;
        }
        if let Some((v, _)) = r.result_or_partial() {
            elements.push(v);
        }
    }
    let pair = (V::List(elements), Some(Tag::List(failed)));
    if ok {
        Dr::Ok(pair)
    } else {
        Dr::Err(Some(pair))
    }
}

// KeyDispatchCodec through MapCodec.assumeMapUnsafe: the key read from the
// field, then the key's type read from the whole map (copied).
fn decode_choice(name: &str, key: Prim, types: &[(Rc<str>, TyId)], input: &Nbt) -> Dr<(V, Nbt)> {
    let Some(map) = ops::get_map(input) else { return Dr::err() };
    let Some(element_name) = map.get(name) else { return Dr::err() };
    let key_value = match read_prim(key, &Some(element_name.clone())) {
        Dr::Ok(V::Str(s)) => s,
        Dr::Ok(_) => refuse("a tagged choice whose key is no string"),
        Dr::Err(_) => return Dr::err(),
    };
    let Some(t) = choice_get(types, &key_value) else { return Dr::err() };
    let copy = Some(Tag::Compound(map.clone()));
    parse(t, &copy).map(|v| (V::pair(V::Str(key_value), v), input.clone()))
}

// CompoundListCodec.decode: every entry, each key and value read; an entry
// that fails without a partial value stops the later ones being kept (the
// running result has none), which makes the kept entries depend on the
// compound's (hash map) order: this port refuses that case.
fn decode_clist(k: TyId, e: TyId, input: &Nbt) -> Dr<(V, Nbt)> {
    let Some(map) = ops::get_map(input) else { return Dr::err() };
    let mut read = Vec::new();
    let mut failed = Compound::new();
    let mut ok = true;
    let mut stopped = false;
    for (key, value) in &map.0 {
        let kr = parse(k, &Some(Tag::String(key.clone())));
        let vr = parse(e, &Some(value.clone()));
        let entry = ap2(kr, vr, V::pair);
        if !entry.is_ok() {
            failed.put(key, value.clone());
            ok = false;
        }
        match entry.result_or_partial() {
            Some(pair) if !stopped => read.push(pair),
            Some(_) => {}
            None => {
                if map.0.len() > 1 {
                    refuse("a compound list entry that fails to read among others (which ones are kept follows a hash map's order)");
                }
                stopped = true;
            }
        }
    }
    let pair = (V::List(read), Some(Tag::Compound(failed)));
    if ok {
        Dr::Ok(pair)
    } else {
        Dr::Err(Some(pair))
    }
}

// Codec.encode(value, ops, prefix).
pub fn encode(t: TyId, v: &V, prefix: Nbt) -> Dr<Nbt> {
    match ty(t) {
        Ty::Prim(p) => ops::merge_to_primitive(&prefix, write_prim(p, v)),
        // MapCodecCodec of Codec.EMPTY: an empty builder built onto the prefix.
        Ty::EmptyPart => ops::merge_to_map_all(&prefix, Vec::new()),
        Ty::Passthrough => {
            let V::Dyn(value) = v else { refuse("the data fixer wrote a non-dynamic value as the remainder") };
            match value {
                None => Dr::Ok(prefix),
                Some(value) => {
                    if prefix.is_none() {
                        return Dr::Ok(Some(value.clone()));
                    }
                    let value = Some(value.clone());
                    if let (Some(m), Some(Tag::Compound(_))) = (ops::get_map(&value), &prefix) {
                        return ops::merge_to_map_all(&prefix, m.0.clone());
                    }
                    if let Some(items) = ops::get_stream(&value) {
                        if let Dr::Ok(r) = ops::merge_to_list(&prefix, items) {
                            return Dr::Ok(r);
                        }
                    }
                    Dr::Err(Some(prefix))
                }
            }
        }
        Ty::Product(a, b) => {
            let V::Pair(va, vb) = v else { refuse("the data fixer wrote a non-pair as a product") };
            encode(b, vb, prefix).flat_map(|r| encode(a, va, r))
        }
        Ty::Sum(a, b) => match v {
            V::Left(x) => encode(a, x, prefix),
            V::Right(x) => encode(b, x, prefix),
            _ => refuse("the data fixer wrote a non-either as a sum"),
        },
        Ty::List(e) => {
            let V::List(items) = v else { refuse("the data fixer wrote a non-list as a list") };
            // ListBuilder: each element encoded on its own, then merged onto the prefix.
            let mut builder: Dr<Vec<Tag>> = Dr::Ok(Vec::new());
            for item in items {
                let r = encode(e, item, None).map(ops::value);
                builder = ap2(builder, r, |mut b, x| {
                    b.push(x);
                    b
                });
            }
            builder.flat_map(|b| ops::merge_to_list(&prefix, b))
        }
        Ty::Field(name, e) => {
            let element = encode(e, v, None).map(ops::value);
            record_build(element.map(|x| vec![(name.to_string(), x)]), &prefix)
        }
        Ty::Named(name, e) => {
            let V::Pair(n, x) = v else { refuse("the data fixer wrote a non-pair as a named type") };
            if n.str() != &*name {
                return Dr::Err(Some(prefix));
            }
            encode(e, x, prefix)
        }
        Ty::Choice(name, key, types) => encode_choice(&name, key, &types, v, prefix),
        Ty::CList(k, e) => {
            let V::List(items) = v else { refuse("the data fixer wrote a non-list as a compound list") };
            let mut builder: Dr<Vec<(String, Tag)>> = Dr::Ok(Vec::new());
            for item in items {
                let V::Pair(kv, ev) = item else { refuse("a compound list entry that is no pair") };
                let value = encode(e, ev, None).map(ops::value);
                // AbstractStringBuilder.add(key, value): the key read as a
                // string, flatMapped into the value added (apply2stable).
                let key = encode(k, kv, None).flat_map(|n| match n {
                    Some(Tag::String(s)) => Dr::Ok(s),
                    _ => Dr::err(),
                });
                builder = key.flat_map(|s| {
                    ap2(builder, value, |mut b, x| {
                        b.push((s, x));
                        b
                    })
                });
            }
            record_build(builder, &prefix)
        }
        Ty::Hook(e, _, post) => encode(e, v, prefix).map(|r| post.apply(r)),
        Ty::Check(_, _, _, e) => encode(e, v, prefix),
        Ty::Mu(fam, i) => encode(unfold(fam, i), v, prefix),
    }
}

// NbtRecordBuilder.build(prefix): the builder's entries over a copy of the
// prefix (a compound or EndTag).
fn record_build(builder: Dr<Vec<(String, Tag)>>, prefix: &Nbt) -> Dr<Nbt> {
    builder.flat_map(|entries| ops::merge_to_map_all(prefix, entries))
}

// KeyDispatchCodec.encode through MapCodec.assumeMapUnsafe: the key's type
// encoded on its own, its entries added to the builder, then the key.
fn encode_choice(name: &str, key: Prim, types: &[(Rc<str>, TyId)], v: &V, prefix: Nbt) -> Dr<Nbt> {
    let V::Pair(k, x) = v else { refuse("the data fixer wrote a non-pair as a tagged choice") };
    let key_value = k.str().to_string();
    let Some(t) = choice_get(types, &key_value) else {
        // withErrorsFrom an error without a partial value.
        return Dr::err();
    };
    let encoded = encode(t, x, None);
    let mut builder: Dr<Vec<(String, Tag)>> = match encoded {
        Dr::Ok(Some(Tag::Compound(c))) => Dr::Ok(c.0),
        // withErrorsFrom(encoded.flatMap(getMap)): the builder keeps its
        // entries (none yet) as a partial value when the error has a map.
        Dr::Err(Some(Some(Tag::Compound(_)))) => Dr::Err(Some(Vec::new())),
        _ => Dr::err(),
    };
    let key_tag = write_prim(key, &V::Str(key_value));
    builder = builder.map(|mut b| {
        b.push((name.to_string(), key_tag));
        b
    });
    record_build(builder, &prefix)
}

// Type.write: encoded onto EndTag.
pub fn write(t: TyId, v: &V) -> Dr<Nbt> {
    encode(t, v, None)
}
