// DataFixerUpper's rewriting (TypeRewriteRule, RewriteResult, View, the
// types' all/one/everywhere and RecursiveTypeFamily.everywhere with its
// fold): a rule rewrites a type into a new type and a function on values.
// DFU builds the function as a PointFree term and optimizes it; this port
// builds it as a closure (the optimizations do not change what it computes).
// A view is a nop exactly when DFU's is (Functions.isId): an identity made
// any other way (convertUnchecked, a recursion point's `in`) is not one.

use super::types::{self as t, Fam, Tpl, TplId, Ty, TyId, V};
use crate::level::refuse;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

pub type F = Rc<dyn Fn(V) -> V>;

// RewriteResult: a view from a type to a new type (recData is DFU's own
// bookkeeping for its optimizations).
#[derive(Clone)]
pub struct RR {
    pub ty: TyId,
    pub new_ty: TyId,
    pub f: Option<F>,
}

impl RR {
    pub fn nop(ty: TyId) -> RR {
        RR { ty, new_ty: ty, f: None }
    }

    pub fn is_nop(&self) -> bool {
        self.f.is_none()
    }

    pub fn new(ty: TyId, new_ty: TyId, f: F) -> RR {
        RR { ty, new_ty, f: Some(f) }
    }

    // View.compose: this view after that one.
    pub fn compose(&self, that: &RR) -> RR {
        match (&self.f, &that.f) {
            (None, _) => that.clone(),
            (_, None) => self.clone(),
            (Some(f), Some(g)) => {
                let (f, g) = (f.clone(), g.clone());
                RR::new(that.ty, self.new_ty, Rc::new(move |v| f(g(v))))
            }
        }
    }

    pub fn apply(&self, v: V) -> V {
        match &self.f {
            None => v,
            Some(f) => f(v),
        }
    }
}

pub type RuleRef = Rc<Rule>;

pub struct Rule {
    pub id: u64,
    pub kind: Kind,
}

pub enum Kind {
    Seq(Vec<RuleRef>),
    Everywhere(RuleRef, bool, bool),
    IfSame(TyId, RR),
}

thread_local! {
    static NEXT: Cell<u64> = const { Cell::new(1) };
    static EVERYWHERE_RULES: RefCell<HashMap<(u64, bool, bool), RuleRef>> = RefCell::new(HashMap::new());
    static EVERYWHERE: RefCell<HashMap<(TyId, u64, bool, bool), Option<RR>>> = RefCell::new(HashMap::new());
    static ALL: RefCell<HashMap<(TyId, u64, bool, bool), RR>> = RefCell::new(HashMap::new());
}

pub fn rule(kind: Kind) -> RuleRef {
    let id = NEXT.with(|n| {
        let id = n.get();
        n.set(id + 1);
        id
    });
    Rc::new(Rule { id, kind })
}

pub fn seq(rules: Vec<RuleRef>) -> RuleRef {
    rule(Kind::Seq(rules))
}

pub fn if_same(target: TyId, value: RR) -> RuleRef {
    rule(Kind::IfSame(target, value))
}

// TypeRewriteRule.everywhere(rule, ...), one object per rule and flags (DFU's
// are equal, which its caches rely on).
pub fn everywhere(r: &RuleRef, recurse: bool, check_index: bool) -> RuleRef {
    let key = (r.id, recurse, check_index);
    if let Some(e) = EVERYWHERE_RULES.with(|m| m.borrow().get(&key).cloned()) {
        return e;
    }
    let e = rule(Kind::Everywhere(r.clone(), recurse, check_index));
    EVERYWHERE_RULES.with(|m| m.borrow_mut().insert(key, e.clone()));
    e
}

// DataFix.fixTypeEverywhere(type, view): everywhere(ifSame(type, view)).
pub fn fix_everywhere(ty: TyId, view: RR) -> RuleRef {
    everywhere(&if_same(ty, view), true, true)
}

// TypeRewriteRule.rewrite.
pub fn rewrite(r: &RuleRef, ty: TyId) -> Option<RR> {
    match &r.kind {
        Kind::Seq(rules) => {
            let mut result = RR::nop(ty);
            for r in rules {
                let s = rewrite(r, result.new_ty)?;
                result = s.compose(&result);
            }
            Some(result)
        }
        Kind::Everywhere(inner, recurse, check) => type_everywhere(ty, inner, *recurse, *check),
        Kind::IfSame(target, value) => t::eq(ty, *target, true, true).then(|| value.clone()),
    }
}

pub fn rewrite_or_nop(r: &RuleRef, ty: TyId) -> RR {
    rewrite(r, ty).unwrap_or_else(|| RR::nop(ty))
}

// Type.opticView(type, view, optic): the view lifted through an optic from
// `s` to `t`, or a nop of the type.
fn optic_view(ty: TyId, view: &RR, s: TyId, new: TyId, lift: impl FnOnce(F) -> F) -> RR {
    match &view.f {
        None => RR::nop(ty),
        Some(f) => RR::new(s, new, lift(f.clone())),
    }
}

fn proj1(f: F) -> F {
    Rc::new(move |v| match v {
        V::Pair(a, b) => V::Pair(Box::new(f(*a)), b),
        other => refuse(format!("the data fixer's proj1 on {other:?}")),
    })
}

fn proj2(f: F) -> F {
    Rc::new(move |v| match v {
        V::Pair(a, b) => V::Pair(a, Box::new(f(*b))),
        other => refuse(format!("the data fixer's proj2 on {other:?}")),
    })
}

fn inj1(f: F) -> F {
    Rc::new(move |v| match v {
        V::Left(a) => V::Left(Box::new(f(*a))),
        other @ V::Right(_) => other,
        other => refuse(format!("the data fixer's inj1 on {other:?}")),
    })
}

fn inj2(f: F) -> F {
    Rc::new(move |v| match v {
        V::Right(b) => V::Right(Box::new(f(*b))),
        other @ V::Left(_) => other,
        other => refuse(format!("the data fixer's inj2 on {other:?}")),
    })
}

fn each(f: F) -> F {
    Rc::new(move |v| match v {
        V::List(items) => V::List(items.into_iter().map(|x| f(x)).collect()),
        other => refuse(format!("the data fixer's list traversal on {other:?}")),
    })
}

fn tagged(key: Rc<str>, f: F) -> F {
    Rc::new(move |v| match v {
        V::Pair(k, x) if k.str() == &*key => V::Pair(k, Box::new(f(*x))),
        other @ V::Pair(_, _) => other,
        other => refuse(format!("the data fixer's tagged choice on {other:?}")),
    })
}

// Product.mergeViews and Sum.mergeViews: the left view, then the right.
fn merge_views(ty: TyId, first: TyId, second: TyId, left: &RR, right: &RR, sum: bool) -> RR {
    let mk = |a, b| if sum { t::sum(a, b) } else { t::product(a, b) };
    let v1 = optic_view(ty, left, mk(first, second), mk(left.new_ty, second), |f| if sum { inj1(f) } else { proj1(f) });
    let v2 = optic_view(v1.new_ty, right, mk(left.new_ty, second), mk(left.new_ty, right.new_ty), |f| if sum { inj2(f) } else { proj2(f) });
    v2.compose(&v1)
}

// TaggedChoiceType.elementResult: one key's view through TypedOptic.tagged.
fn element_result(choice: TyId, key: &Rc<str>, view: &RR) -> RR {
    let new = replace_tagged(choice, key, view.ty, view.new_ty);
    let key = key.clone();
    optic_view(choice, view, choice, new, move |f| tagged(key, f))
}

// TypedOptic.replaceTagged.
pub fn replace_tagged(choice: TyId, key: &str, a: TyId, b: TyId) -> TyId {
    if t::same(a, b) {
        return choice;
    }
    let Ty::Choice(name, k, types) = t::ty(choice) else { unreachable!() };
    match t::choice_get(&types, key) {
        Some(e) if t::same(e, a) => {}
        _ => refuse("the data fixer's focused choice type does not match (IllegalArgumentException)"),
    }
    let mut new: Vec<(Rc<str>, TyId)> = types.iter().cloned().collect();
    for e in new.iter_mut() {
        if &*e.0 == key {
            e.1 = b;
        }
    }
    t::choice(&name, k, new)
}

// Type.all(rule, recurse, checkIndex).
pub fn all(ty: TyId, r: &RuleRef, recurse: bool, check: bool) -> RR {
    let key = (ty, r.id, recurse, check);
    if let Some(rr) = ALL.with(|m| m.borrow().get(&key).cloned()) {
        return rr;
    }
    let rr = all_uncached(ty, r, recurse, check);
    ALL.with(|m| m.borrow_mut().insert(key, rr.clone()));
    rr
}

fn all_uncached(ty: TyId, r: &RuleRef, recurse: bool, check: bool) -> RR {
    match t::ty(ty) {
        Ty::Prim(_) | Ty::EmptyPart | Ty::Passthrough => RR::nop(ty),
        Ty::Product(a, b) => merge_views(ty, a, b, &rewrite_or_nop(r, a), &rewrite_or_nop(r, b), false),
        Ty::Sum(a, b) => merge_views(ty, a, b, &rewrite_or_nop(r, a), &rewrite_or_nop(r, b), true),
        Ty::List(e) => list_fix(ty, &rewrite_or_nop(r, e)),
        Ty::Field(name, e) => field_wrap(&name, ty, rewrite_or_nop(r, e)),
        Ty::Named(name, e) => named_fix(&name, ty, &rewrite_or_nop(r, e)),
        Ty::Choice(name, key, types) => {
            let mut results: Vec<(Rc<str>, RR)> = Vec::new();
            for (k, e) in types.iter() {
                if let Some(rr) = rewrite(r, *e) {
                    if !rr.is_nop() {
                        results.push((k.clone(), rr));
                    }
                }
            }
            match results.len() {
                0 => RR::nop(ty),
                1 => element_result(ty, &results[0].0, &results[0].1),
                _ => {
                    let mut new: Vec<(Rc<str>, TyId)> = types.iter().cloned().collect();
                    for (k, rr) in &results {
                        for e in new.iter_mut() {
                            if e.0 == *k {
                                e.1 = rr.new_ty;
                            }
                        }
                    }
                    let new_ty = t::choice(&name, key, new);
                    let map: HashMap<Rc<str>, RR> = results.into_iter().collect();
                    RR::new(
                        ty,
                        new_ty,
                        Rc::new(move |v| match v {
                            V::Pair(k, x) => match map.get(k.str()) {
                                Some(rr) => V::Pair(k, Box::new(rr.apply(*x))),
                                None => V::Pair(k, x),
                            },
                            other => refuse(format!("the data fixer's tagged choice on {other:?}")),
                        }),
                    )
                }
            }
        }
        Ty::CList(k, e) => clist_merge(ty, k, e, &rewrite_or_nop(r, k), &rewrite_or_nop(r, e)),
        Ty::Hook(e, pre, post) => {
            let view = rewrite_or_nop(r, e);
            match &view.f {
                None => RR::nop(ty),
                Some(f) => RR::new(t::mk(Ty::Hook(view.ty, pre.clone(), post.clone())), t::mk(Ty::Hook(view.new_ty, pre, post)), f.clone()),
            }
        }
        Ty::Check(name, index, expected, d) => {
            if check && index != expected {
                return RR::nop(ty);
            }
            check_fix(ty, &name, index, expected, &rewrite_or_nop(r, d))
        }
        Ty::Mu(fam, i) => all(t::unfold(fam, i), r, recurse, check),
    }
}

fn list_fix(ty: TyId, view: &RR) -> RR {
    optic_view(ty, view, t::list(view.ty), t::list(view.new_ty), each)
}

// TagType.wrap: a nop element view is returned as it is.
fn field_wrap(name: &str, ty: TyId, view: RR) -> RR {
    match &view.f {
        None => view,
        Some(f) => RR::new(ty, t::field(name, view.new_ty), f.clone()),
    }
}

// NamedType.fix.
fn named_fix(name: &str, ty: TyId, view: &RR) -> RR {
    optic_view(ty, view, t::named(name, view.ty), t::named(name, view.new_ty), proj2)
}

// CheckType.fix.
fn check_fix(ty: TyId, name: &str, index: i32, expected: i32, view: &RR) -> RR {
    optic_view(ty, view, ty, t::mk(Ty::Check(name.into(), index, expected, view.new_ty)), |f| f)
}

// CompoundListType.mergeViews: the keys' view, then the values'.
fn clist_merge(ty: TyId, k: TyId, e: TyId, keys: &RR, values: &RR) -> RR {
    let v1 = optic_view(ty, keys, t::mk(Ty::CList(k, e)), t::mk(Ty::CList(keys.new_ty, e)), |f| each(proj1(f)));
    let v2 = optic_view(v1.new_ty, values, t::mk(Ty::CList(keys.new_ty, e)), t::mk(Ty::CList(keys.new_ty, values.new_ty)), |f| each(proj2(f)));
    v2.compose(&v1)
}

// Type.everywhere (and its overrides).
pub fn type_everywhere(ty: TyId, r: &RuleRef, recurse: bool, check: bool) -> Option<RR> {
    let key = (ty, r.id, recurse, check);
    if let Some(rr) = EVERYWHERE.with(|m| m.borrow().get(&key).cloned()) {
        return rr;
    }
    let rr = everywhere_uncached(ty, r, recurse, check);
    EVERYWHERE.with(|m| m.borrow_mut().insert(key, rr.clone()));
    rr
}

fn everywhere_uncached(ty: TyId, r: &RuleRef, recurse: bool, check: bool) -> Option<RR> {
    match t::ty(ty) {
        Ty::Mu(fam, index) => {
            if recurse {
                if let Some(rr) = family_everywhere(fam, index, r) {
                    return Some(rr);
                }
            }
            Some(RR::nop(ty))
        }
        Ty::Check(_, index, expected, _) if check && index != expected => None,
        _ => {
            // seq(orElse(rule, nop), all(everywhere(rule), recurse, checkIndex)),
            // each step composed onto the one before (a nop onto a nop of the
            // type keeps the type).
            let r1 = rewrite_or_nop(r, ty).compose(&RR::nop(ty));
            let inner = everywhere(r, recurse, check);
            let r2 = all(r1.new_ty, &inner, recurse, check);
            Some(r2.compose(&r1))
        }
    }
}

// RecursiveTypeFamily.everywhere: each member rewritten inside (not through
// its recursion points), the rule tried on the member itself, and the
// result a fold: recursion points first, then the member's view.
fn family_everywhere(fam: Fam, index: i32, r: &RuleRef) -> Option<RR> {
    let source = t::unfold(fam, index);
    let source_view = type_everywhere(source, r, false, false).unwrap_or_else(|| RR::nop(source));
    let new_mu = t::build_mu_type(fam, source_view.new_ty, None);
    let Ty::Mu(new_fam, _) = t::ty(new_mu) else { unreachable!() };
    let size = t::size(fam);
    let mut views: Vec<Option<F>> = Vec::with_capacity(size as usize);
    let mut found_any = false;
    for i in 0..size {
        let unfold = t::unfold(fam, i);
        let view = type_everywhere(unfold, r, false, true).unwrap_or_else(|| RR::nop(unfold));
        let mut nop = view.is_nop();
        let new_mu_i = t::build_mu_type(fam, view.new_ty, Some(new_fam));
        let Ty::Mu(_, j) = t::ty(new_mu_i) else { unreachable!() };
        // in: the unfolded value as the recursion point's (not a nop).
        let new_view = RR::new(t::unfold(new_fam, j), new_mu_i, Rc::new(|v| v)).compose(&view);
        let mut chosen = view;
        if let Some(rw) = rewrite(r, new_view.new_ty) {
            if !rw.is_nop() {
                nop = false;
                chosen = rw.compose(&new_view);
            }
        }
        views.push(chosen.f.clone());
        found_any = found_any || !nop;
    }
    if !found_any {
        return None;
    }
    let data = Rc::new(FoldData { fam, algebra: views });
    Some(RR::new(t::mk(Ty::Mu(fam, index)), t::mk(Ty::Mu(new_fam, index)), Rc::new(move |v| fold(&data, index, v))))
}

struct FoldData {
    fam: Fam,
    algebra: Vec<Option<F>>,
}

// Fold.eval: the family template's hmap (each recursion point folded), then
// the member's algebra.
fn fold(data: &Rc<FoldData>, index: i32, v: V) -> V {
    let v = hmap(data.fam, data, v);
    match &data.algebra[index as usize] {
        Some(f) => f(v),
        None => v,
    }
}

fn hmap(tp: TplId, data: &Rc<FoldData>, v: V) -> V {
    if !t::has_id(tp) {
        return v;
    }
    match t::tpl(tp) {
        Tpl::Const(_) => v,
        Tpl::Named(_, e) => match v {
            V::Pair(n, x) => V::Pair(n, Box::new(hmap(e, data, *x))),
            other => refuse(format!("the data fixer's fold on a named value {other:?}")),
        },
        Tpl::Field(_, e) | Tpl::Hook(e, _, _) | Tpl::Check(_, _, e) => hmap(e, data, v),
        Tpl::And(a, b) => match v {
            V::Pair(x, y) => V::Pair(Box::new(hmap(a, data, *x)), Box::new(hmap(b, data, *y))),
            other => refuse(format!("the data fixer's fold on a product {other:?}")),
        },
        Tpl::Or(a, b) => match v {
            V::Left(x) => V::Left(Box::new(hmap(a, data, *x))),
            V::Right(y) => V::Right(Box::new(hmap(b, data, *y))),
            other => refuse(format!("the data fixer's fold on a sum {other:?}")),
        },
        Tpl::List(e) => match v {
            V::List(items) => V::List(items.into_iter().map(|x| hmap(e, data, x)).collect()),
            other => refuse(format!("the data fixer's fold on a list {other:?}")),
        },
        Tpl::CList(k, e) => match v {
            V::List(items) => V::List(
                items
                    .into_iter()
                    .map(|x| match x {
                        V::Pair(a, b) => V::Pair(Box::new(hmap(k, data, *a)), Box::new(hmap(e, data, *b))),
                        other => refuse(format!("the data fixer's fold on a compound list entry {other:?}")),
                    })
                    .collect(),
            ),
            other => refuse(format!("the data fixer's fold on a compound list {other:?}")),
        },
        Tpl::Choice(_, _, templates) => match v {
            V::Pair(k, x) => match templates.binary_search_by(|e| (*e.0).cmp(k.str())) {
                Ok(at) => V::Pair(k, Box::new(hmap(templates[at].1, data, *x))),
                Err(_) => V::Pair(k, x),
            },
            other => refuse(format!("the data fixer's fold on a tagged choice {other:?}")),
        },
        Tpl::Id(j) => fold(data, j, v),
    }
}
