// DFU's optic finders and Typed (OpticFinder, FieldFinder, NamedChoiceFinder,
// TypedOptic, Typed): how a fix finds a part of a typed value by its type or
// field name and reads or replaces it. An optic is a path of steps through a
// value, with DFU's bounds: what an operation needs of them (a lens to get,
// an affine to get optionally or set) is checked as DFU's upcast checks it.

use super::ops::Dy;
use super::types::{self as t, Prim, Ty, TyId, V};
use crate::level::refuse;
use std::rc::Rc;

// The profunctor classes an optic's elements prove (TypedOptic.bounds).
pub const PROFUNCTOR: u8 = 1;
pub const CARTESIAN: u8 = 2;
pub const COCARTESIAN: u8 = 4;
pub const AFFINE: u8 = 8;
pub const TRAVERSAL: u8 = 16;

#[derive(Clone, Debug)]
pub enum Step {
    Proj1,
    Proj2,
    Inj1,
    Inj2,
    Each,
    Tagged(Rc<str>),
    // A tagged choice's key (FieldFinder on the choice's field): proj1.
    ChoiceKey,
    // A tagged choice whose several choices hold the part.
    ChoiceMerge(Rc<Vec<(Rc<str>, Optic)>>),
    // A sum whose both sides hold it.
    SumMerge(Rc<Optic>, Rc<Optic>),
}

#[derive(Clone, Debug)]
pub struct Optic {
    pub bounds: u8,
    pub steps: Vec<Step>,
    pub s: TyId,
    pub t: TyId,
    pub a: TyId,
    pub b: TyId,
}

impl Optic {
    fn leaf(bound: u8, s: TyId, t: TyId, a: TyId, b: TyId, steps: Vec<Step>) -> Optic {
        Optic { bounds: bound, steps, s, t, a, b }
    }

    // outer.compose(inner): this step outside the inner optic.
    fn outer(bound: u8, step: Option<Step>, s: TyId, t: TyId, inner: Optic) -> Optic {
        let mut steps = Vec::with_capacity(inner.steps.len() + 1);
        steps.extend(step);
        steps.extend(inner.steps);
        Optic { bounds: bound | inner.bounds, steps, s, t, a: inner.a, b: inner.b }
    }

    // castOuter: the same path, other outer types.
    fn cast(self, s: TyId, t: TyId) -> Optic {
        Optic { s, t, ..self }
    }

    // TypedOptic.upCast(proof): every bound is a supertype of the proof.
    fn check(&self, allowed: u8) {
        if self.bounds & !allowed != 0 {
            refuse("a data fixer optic that does not upcast (IllegalArgumentException: Couldn't upcast)");
        }
    }
}

// What a lens (Forget, for Typed.get) and an affine (ForgetOpt, ReForgetC)
// accept.
const LENS: u8 = PROFUNCTOR | CARTESIAN;
const AFFINE_OK: u8 = PROFUNCTOR | CARTESIAN | COCARTESIAN | AFFINE;

fn foci<'a>(steps: &[Step], v: &'a V, out: &mut Vec<&'a V>) {
    let Some((step, rest)) = steps.split_first() else {
        out.push(v);
        return;
    };
    match (step, v) {
        (Step::Proj1, V::Pair(a, _)) => foci(rest, a, out),
        (Step::Proj2, V::Pair(_, b)) => foci(rest, b, out),
        (Step::Inj1, V::Left(a)) => foci(rest, a, out),
        (Step::Inj1, V::Right(_)) => {}
        (Step::Inj2, V::Right(b)) => foci(rest, b, out),
        (Step::Inj2, V::Left(_)) => {}
        (Step::Each, V::List(items)) => {
            for x in items {
                foci(rest, x, out);
            }
        }
        (Step::Tagged(key), V::Pair(k, x)) => {
            if k.str() == &**key {
                foci(rest, x, out);
            }
        }
        (Step::ChoiceKey, V::Pair(k, _)) => foci(rest, k, out),
        (Step::ChoiceMerge(optics), V::Pair(k, x)) => {
            if let Some((_, o)) = optics.iter().find(|(key, _)| &**key == k.str()) {
                foci(&o.steps, x, out);
            }
        }
        (Step::SumMerge(l, _), V::Left(x)) => foci(&l.steps, x, out),
        (Step::SumMerge(_, r), V::Right(x)) => foci(&r.steps, x, out),
        (step, v) => refuse(format!("a data fixer optic step {step:?} on {v:?}")),
    }
}

fn modify(steps: &[Step], v: V, f: &mut dyn FnMut(V) -> V) -> V {
    let Some((step, rest)) = steps.split_first() else { return f(v) };
    match (step, v) {
        (Step::Proj1, V::Pair(a, b)) => V::Pair(Box::new(modify(rest, *a, f)), b),
        (Step::Proj2, V::Pair(a, b)) => V::Pair(a, Box::new(modify(rest, *b, f))),
        (Step::Inj1, V::Left(a)) => V::Left(Box::new(modify(rest, *a, f))),
        (Step::Inj1, v @ V::Right(_)) => v,
        (Step::Inj2, V::Right(b)) => V::Right(Box::new(modify(rest, *b, f))),
        (Step::Inj2, v @ V::Left(_)) => v,
        (Step::Each, V::List(items)) => V::List(items.into_iter().map(|x| modify(rest, x, f)).collect()),
        (Step::Tagged(key), V::Pair(k, x)) => {
            if k.str() == &**key {
                V::Pair(k, Box::new(modify(rest, *x, f)))
            } else {
                V::Pair(k, x)
            }
        }
        (Step::ChoiceKey, V::Pair(k, x)) => V::Pair(Box::new(modify(rest, *k, f)), x),
        (Step::ChoiceMerge(optics), V::Pair(k, x)) => match optics.iter().find(|(key, _)| &**key == k.str()) {
            Some((_, o)) => V::Pair(k, Box::new(modify(&o.steps, *x, f))),
            None => V::Pair(k, x),
        },
        (Step::SumMerge(l, _), V::Left(x)) => V::Left(Box::new(modify(&l.steps, *x, f))),
        (Step::SumMerge(_, r), V::Right(x)) => V::Right(Box::new(modify(&r.steps, *x, f))),
        (step, v) => refuse(format!("a data fixer optic step {step:?} on {v:?}")),
    }
}

// An OpticFinder.
#[derive(Clone, Debug)]
pub enum Finder {
    // FieldFinder: a field of a name and type, or (no name) the type itself.
    Field(Option<Rc<str>>, TyId),
    NamedChoice(Rc<str>, TyId),
    // finder.inField(name, type).
}

pub fn type_finder(ty: TyId) -> Finder {
    Finder::Field(None, ty)
}

pub fn field_finder(name: &str, ty: TyId) -> Finder {
    Finder::Field(Some(name.into()), ty)
}

pub fn named_choice(name: &str, ty: TyId) -> Finder {
    Finder::NamedChoice(name.into(), ty)
}

pub fn remainder_finder() -> Finder {
    type_finder(t::remainder())
}

// Type.findField(name): a finder of the field with the type the type has
// for it.
pub fn find_field(ty: TyId, name: &str) -> Finder {
    match t::find_field_type(ty, name) {
        Some(f) => field_finder(name, f),
        None => refuse(format!("the data fixer found no field {name} (IllegalArgumentException)")),
    }
}

impl Finder {
    pub fn ty(&self) -> TyId {
        match self {
            Finder::Field(_, ty) | Finder::NamedChoice(_, ty) => *ty,
        }
    }

    pub fn find(&self, container: TyId, result: TyId) -> Option<Optic> {
        find_type(container, self, result)
    }
}

enum Match {
    Found(Optic),
    NotFound,
    Continue,
}

fn matcher(finder: &Finder, target: TyId, result: TyId) -> Match {
    match finder {
        Finder::Field(name, ft) => {
            if name.is_none() && t::eq(*ft, target, true, false) {
                return Match::Found(Optic::leaf(PROFUNCTOR, target, result, target, result, Vec::new()));
            }
            match t::ty(target) {
                Ty::Field(n, el) => {
                    if Some(&n) != name.as_ref() || !t::same(*ft, el) {
                        Match::NotFound
                    } else {
                        Match::Found(Optic::leaf(PROFUNCTOR, target, t::field(&n, result), *ft, result, Vec::new()))
                    }
                }
                Ty::Choice(cname, key, _) if Some(&cname) == name.as_ref() => {
                    if !t::same(*ft, t::prim(key)) || !t::same(*ft, result) {
                        Match::NotFound
                    } else {
                        Match::Found(Optic::leaf(CARTESIAN, target, target, *ft, *ft, vec![Step::ChoiceKey]))
                    }
                }
                _ => Match::Continue,
            }
        }
        Finder::NamedChoice(name, ft) => match t::ty(target) {
            Ty::Choice(_, _, types) => match t::choice_get(&types, name) {
                Some(el) => {
                    if !t::same(*ft, el) {
                        Match::NotFound
                    } else {
                        let new = super::rewrite::replace_tagged(target, name, *ft, result);
                        Match::Found(Optic::leaf(COCARTESIAN, target, new, *ft, result, vec![Step::Tagged(name.clone())]))
                    }
                }
                None => Match::Continue,
            },
            Ty::Field(_, _) => Match::NotFound,
            _ => Match::Continue,
        },
    }
}

// Type.findType.
fn find_type(target: TyId, finder: &Finder, result: TyId) -> Option<Optic> {
    match matcher(finder, target, result) {
        Match::Found(o) => Some(o),
        Match::NotFound => None,
        Match::Continue => find_in_children(target, finder, result),
    }
}

fn find_in_children(target: TyId, finder: &Finder, result: TyId) -> Option<Optic> {
    match t::ty(target) {
        Ty::Product(a, b) => match find_type(a, finder, result) {
            Some(o) => Some(Optic::outer(CARTESIAN, Some(Step::Proj1), t::product(o.s, b), t::product(o.t, b), o)),
            None => find_type(b, finder, result).map(|o| Optic::outer(CARTESIAN, Some(Step::Proj2), t::product(a, o.s), t::product(a, o.t), o)),
        },
        Ty::Sum(a, b) => {
            let first = find_type(a, finder, result);
            let second = find_type(b, finder, result);
            match (first, second) {
                (Some(l), Some(r)) => {
                    let (s, tt, fa, fb) = (t::sum(l.s, r.s), t::sum(l.t, r.t), l.a, l.b);
                    Some(Optic::leaf(TRAVERSAL, s, tt, fa, fb, vec![Step::SumMerge(Rc::new(l), Rc::new(r))]))
                }
                (Some(o), None) => Some(Optic::outer(COCARTESIAN, Some(Step::Inj1), t::sum(o.s, b), t::sum(o.t, b), o)),
                (None, Some(o)) => Some(Optic::outer(COCARTESIAN, Some(Step::Inj2), t::sum(a, o.s), t::sum(a, o.t), o)),
                (None, None) => None,
            }
        }
        Ty::List(e) => find_type(e, finder, result).map(|o| Optic::outer(TRAVERSAL, Some(Step::Each), t::list(o.s), t::list(o.t), o)),
        Ty::Field(n, e) => find_type(e, finder, result).map(|o| {
            let (s, tt) = (t::field(&n, o.s), t::field(&n, o.t));
            o.cast(s, tt)
        }),
        Ty::Named(n, e) => find_type(e, finder, result).map(|o| Optic::outer(CARTESIAN, Some(Step::Proj2), t::named(&n, o.s), t::named(&n, o.t), o)),
        Ty::Choice(name, key, types) => {
            let found: Vec<(Rc<str>, Optic)> = types.iter().filter_map(|(k, e)| find_type(*e, finder, result).map(|o| (k.clone(), o))).collect();
            match found.len() {
                0 => None,
                1 => {
                    let (k, o) = found.into_iter().next().unwrap();
                    let new = super::rewrite::replace_tagged(target, &k, o.s, o.t);
                    Some(Optic::outer(COCARTESIAN, Some(Step::Tagged(k)), target, new, o))
                }
                n => {
                    let bounds = found.iter().fold(0, |b, (_, o)| b | o.bounds);
                    let bound = if bounds & !LENS == 0 && n == types.len() {
                        CARTESIAN
                    } else if bounds & !AFFINE_OK == 0 {
                        AFFINE
                    } else {
                        TRAVERSAL
                    };
                    let mut new: Vec<(Rc<str>, TyId)> = types.iter().cloned().collect();
                    for e in new.iter_mut() {
                        if let Some((_, o)) = found.iter().find(|(k, _)| *k == e.0) {
                            e.1 = o.t;
                        }
                    }
                    let tt = t::choice(&name, key, new);
                    let (fa, fb) = (found[0].1.a, found[0].1.b);
                    Some(Optic::leaf(bound, target, tt, fa, fb, vec![Step::ChoiceMerge(Rc::new(found))]))
                }
            }
        }
        Ty::CList(k, e) => match find_type(k, finder, result) {
            Some(o) => Some(Optic::outer(TRAVERSAL, None, t::mk(Ty::CList(o.s, e)), t::mk(Ty::CList(o.t, e)), Optic { steps: [vec![Step::Each, Step::Proj1], o.steps].concat(), ..o })),
            None => find_type(e, finder, result).map(|o| Optic::outer(TRAVERSAL, None, t::mk(Ty::CList(k, o.s)), t::mk(Ty::CList(k, o.t)), Optic { steps: [vec![Step::Each, Step::Proj2], o.steps].concat(), ..o })),
        },
        Ty::Hook(e, pre, post) => find_type(e, finder, result).map(|o| {
            let (s, tt) = (t::mk(Ty::Hook(o.s, pre.clone(), post.clone())), t::mk(Ty::Hook(o.t, pre, post)));
            o.cast(s, tt)
        }),
        Ty::Check(n, index, expected, d) => {
            if index != expected {
                return None;
            }
            find_type(d, finder, result).map(|o| {
                let tt = t::mk(Ty::Check(n, index, expected, o.t));
                o.cast(target, tt)
            })
        }
        // RecursiveTypeFamily.findType without recursion: the unfolded type
        // searched, the new type the recursion point of the family of its
        // template.
        Ty::Mu(fam, i) => find_type(t::unfold(fam, i), finder, result).map(|o| {
            let new_fam = t::template(o.t);
            o.cast(t::mk(Ty::Mu(fam, i)), t::mk(Ty::Mu(new_fam, i)))
        }),
        Ty::Prim(_) | Ty::EmptyPart | Ty::Passthrough => None,
    }
}

fn found(container: TyId, finder: &Finder, result: TyId) -> Optic {
    match finder.find(container, result) {
        Some(o) => o,
        None => refuse("a data fixer optic that finds nothing (Typed throws)"),
    }
}

// Typed: a value with its type.
#[derive(Clone, Debug)]
pub struct Typed {
    pub ty: TyId,
    pub v: V,
}

impl Typed {
    pub fn new(ty: TyId, v: V) -> Typed {
        Typed { ty, v }
    }

    pub fn get(&self, finder: &Finder) -> V {
        let o = found(self.ty, finder, finder.ty());
        o.check(LENS);
        let mut out = Vec::new();
        foci(&o.steps, &self.v, &mut out);
        match out.first() {
            Some(v) => (*v).clone(),
            None => refuse("a data fixer lens without its part"),
        }
    }

    pub fn get_optional(&self, finder: &Finder) -> Option<V> {
        let o = found(self.ty, finder, finder.ty());
        o.check(AFFINE_OK);
        let mut out = Vec::new();
        foci(&o.steps, &self.v, &mut out);
        out.first().map(|v| (*v).clone())
    }

    pub fn get_optional_typed(&self, finder: &Finder) -> Option<Typed> {
        let o = found(self.ty, finder, finder.ty());
        self.get_optional(finder).map(|v| Typed::new(o.a, v))
    }

    pub fn get_or_create(&self, finder: &Finder) -> V {
        self.get_optional(finder).or_else(|| t::point(finder.ty())).unwrap_or_else(|| refuse("a data fixer default value for a type that has none (IllegalStateException)"))
    }

    pub fn get_or_create_typed(&self, finder: &Finder) -> Typed {
        match self.get_optional_typed(finder) {
            Some(x) => x,
            None => match t::point(finder.ty()) {
                Some(v) => Typed::new(finder.ty(), v),
                None => refuse("a data fixer default value for a type that has none (IllegalStateException)"),
            },
        }
    }

    pub fn get_all_typed(&self, finder: &Finder) -> Vec<Typed> {
        let o = found(self.ty, finder, finder.ty());
        let mut out = Vec::new();
        foci(&o.steps, &self.v, &mut out);
        out.into_iter().map(|v| Typed::new(finder.ty(), v.clone())).collect()
    }

    // Typed.set(optic, newValue): the part replaced (where the affine finds it).
    pub fn set(&self, finder: &Finder, value: Typed) -> Typed {
        let o = found(self.ty, finder, value.ty);
        o.check(AFFINE_OK);
        let new = modify(&o.steps, self.v.clone(), &mut |_| value.v.clone());
        Typed::new(o.t, new)
    }

    pub fn set_value(&self, finder: &Finder, v: V) -> Typed {
        self.set(finder, Typed::new(finder.ty(), v))
    }

    pub fn update(&self, finder: &Finder, f: impl Fn(V) -> V) -> Typed {
        self.update_to(finder, finder.ty(), f)
    }

    pub fn update_to(&self, finder: &Finder, new_ty: TyId, f: impl Fn(V) -> V) -> Typed {
        let o = found(self.ty, finder, new_ty);
        let new = modify(&o.steps, self.v.clone(), &mut |x| f(x));
        Typed::new(o.t, new)
    }

    // Typed.update of the remainder: the Dynamic function on it.
    pub fn update_remainder(&self, f: impl Fn(Dy) -> Dy) -> Typed {
        self.update(&remainder_finder(), |v| V::Dyn(f(v.dy()).0))
    }

    pub fn remainder(&self) -> Dy {
        self.get(&remainder_finder()).dy()
    }

    pub fn update_typed(&self, finder: &Finder, f: impl Fn(Typed) -> Typed) -> Typed {
        self.update_typed_to(finder, finder.ty(), f)
    }

    pub fn update_typed_to(&self, finder: &Finder, new_ty: TyId, f: impl Fn(Typed) -> Typed) -> Typed {
        let o = found(self.ty, finder, new_ty);
        let ft = finder.ty();
        let b = o.b;
        let new = modify(&o.steps, self.v.clone(), &mut |x| {
            let r = f(Typed::new(ft, x));
            if !t::eq(b, r.ty, true, true) {
                refuse("a data fixer function that did not update to the expected type (IllegalArgumentException)");
            }
            r.v
        });
        Typed::new(o.t, new)
    }

    pub fn write(&self) -> super::dr::Dr<Dy> {
        t::write(self.ty, &self.v).map(Dy)
    }

}

pub fn namespaced_string() -> TyId {
    t::prim(Prim::NamespacedString)
}

pub fn string() -> TyId {
    t::prim(Prim::String)
}
