// NbtOps, the DynamicOps the fixer runs on, and com.mojang.serialization's
// Dynamic and OptionalDynamic over it: what the fixes read and write saved
// data with. A value is a tag or EndTag (ops.empty(), None here).
//
// NbtOps never changes a tag in place: every write makes a copy (a shallow
// one in Java, whose nested tags nothing changes later), so tags are values
// here.

use super::dr::Dr;
use crate::level::refuse;
use crate::nbt::{Compound, Tag};

pub type Nbt = Option<Tag>;

// A boxed Number as NumericTag.box() gives it, with Number's conversions.
#[derive(Clone, Copy, Debug)]
pub enum Num {
    B(i8),
    S(i16),
    I(i32),
    L(i64),
    F(f32),
    D(f64),
}

impl Num {
    pub fn int_value(self) -> i32 {
        match self {
            Num::B(v) => v as i32,
            Num::S(v) => v as i32,
            Num::I(v) => v,
            Num::L(v) => v as i32,
            Num::F(v) => v as i32,
            Num::D(v) => v as i32,
        }
    }

    pub fn long_value(self) -> i64 {
        match self {
            Num::B(v) => v as i64,
            Num::S(v) => v as i64,
            Num::I(v) => v as i64,
            Num::L(v) => v,
            Num::F(v) => v as i64,
            Num::D(v) => v as i64,
        }
    }

    // Byte, Short, Integer and Long narrow their value; Float and Double go
    // through intValue().
    pub fn byte_value(self) -> i8 {
        match self {
            Num::B(v) => v,
            Num::S(v) => v as i8,
            Num::I(v) => v as i8,
            Num::L(v) => v as i8,
            Num::F(v) => (v as i32) as i8,
            Num::D(v) => (v as i32) as i8,
        }
    }

    pub fn short_value(self) -> i16 {
        match self {
            Num::B(v) => v as i16,
            Num::S(v) => v,
            Num::I(v) => v as i16,
            Num::L(v) => v as i16,
            Num::F(v) => (v as i32) as i16,
            Num::D(v) => (v as i32) as i16,
        }
    }

    pub fn float_value(self) -> f32 {
        match self {
            Num::B(v) => v as f32,
            Num::S(v) => v as f32,
            Num::I(v) => v as f32,
            Num::L(v) => v as f32,
            Num::F(v) => v,
            Num::D(v) => v as f32,
        }
    }

    pub fn double_value(self) -> f64 {
        match self {
            Num::B(v) => v as f64,
            Num::S(v) => v as f64,
            Num::I(v) => v as f64,
            Num::L(v) => v as f64,
            Num::F(v) => v as f64,
            Num::D(v) => v,
        }
    }
}

// Tag.equals: records compare their components, a float or a double by
// Float.compare (so NaN equals itself and 0.0 differs from -0.0); a compound
// is a map, a list a list.
pub fn tag_eq(a: &Tag, b: &Tag) -> bool {
    match (a, b) {
        (Tag::Float(x), Tag::Float(y)) => float_bits(*x) == float_bits(*y),
        (Tag::Double(x), Tag::Double(y)) => double_bits(*x) == double_bits(*y),
        (Tag::List(x), Tag::List(y)) => x.len() == y.len() && x.iter().zip(y).all(|(p, q)| tag_eq(p, q)),
        (Tag::Compound(x), Tag::Compound(y)) => x.0.len() == y.0.len() && x.0.iter().all(|(k, v)| y.get(k).is_some_and(|w| tag_eq(v, w))),
        _ => a == b,
    }
}

pub fn nbt_eq(a: &Nbt, b: &Nbt) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(x), Some(y)) => tag_eq(x, y),
        _ => false,
    }
}

// Float.floatToIntBits: every NaN is one.
fn float_bits(v: f32) -> u32 {
    if v.is_nan() {
        0x7fc00000
    } else {
        v.to_bits()
    }
}

fn double_bits(v: f64) -> u64 {
    if v.is_nan() {
        0x7ff8000000000000
    } else {
        v.to_bits()
    }
}

pub fn get_number(v: &Nbt) -> Option<Num> {
    Some(match v.as_ref()? {
        Tag::Byte(x) => Num::B(*x),
        Tag::Short(x) => Num::S(*x),
        Tag::Int(x) => Num::I(*x),
        Tag::Long(x) => Num::L(*x),
        Tag::Float(x) => Num::F(*x),
        Tag::Double(x) => Num::D(*x),
        _ => return None,
    })
}

pub fn get_string(v: &Nbt) -> Option<&str> {
    match v {
        Some(Tag::String(s)) => Some(s),
        _ => None,
    }
}

pub fn get_map(v: &Nbt) -> Option<&Compound> {
    match v {
        Some(Tag::Compound(c)) => Some(c),
        _ => None,
    }
}

// getStream / getList: a CollectionTag (a list or an array).
pub fn get_stream(v: &Nbt) -> Option<Vec<Tag>> {
    Some(match v.as_ref()? {
        Tag::List(l) => l.clone(),
        Tag::ByteArray(a) => a.iter().map(|x| Tag::Byte(*x)).collect(),
        Tag::IntArray(a) => a.iter().map(|x| Tag::Int(*x)).collect(),
        Tag::LongArray(a) => a.iter().map(|x| Tag::Long(*x)).collect(),
        _ => return None,
    })
}

// A tag that a map or a list is given: EndTag cannot be one here.
pub fn value(v: Nbt) -> Tag {
    match v {
        Some(t) => t,
        None => refuse("an EndTag put into a compound or a list (the data fixer)"),
    }
}

// mergeToMap(map, key, value): a copy of the compound (or a new one for
// EndTag) with the entry.
pub fn merge_to_map(map: &Nbt, key: &str, val: Tag) -> Dr<Nbt> {
    match map {
        None => {
            let mut c = Compound::new();
            c.put(key, val);
            Dr::Ok(Some(Tag::Compound(c)))
        }
        Some(Tag::Compound(c)) => {
            let mut c = c.clone();
            c.put(key, val);
            Dr::Ok(Some(Tag::Compound(c)))
        }
        _ => Dr::Err(Some(map.clone())),
    }
}

// mergeToMap(map, entries) and RecordBuilder.build onto a prefix: the
// entries put into a copy of the prefix.
pub fn merge_to_map_all(map: &Nbt, entries: Vec<(String, Tag)>) -> Dr<Nbt> {
    let mut c = match map {
        None => Compound::new(),
        Some(Tag::Compound(c)) => c.clone(),
        _ => return Dr::Err(Some(map.clone())),
    };
    for (k, v) in entries {
        c.put(&k, v);
    }
    Dr::Ok(Some(Tag::Compound(c)))
}

// mergeToList: NbtOps' list collectors. An array prefix keeps being an
// array while the values are of its element type.
pub fn merge_to_list(list: &Nbt, values: Vec<Tag>) -> Dr<Nbt> {
    enum Collector {
        Generic(Vec<Tag>),
        Bytes(Vec<i8>),
        Ints(Vec<i32>),
        Longs(Vec<i64>),
    }
    let mut collector = match list {
        None => Collector::Generic(Vec::new()),
        Some(Tag::List(l)) => Collector::Generic(l.clone()),
        Some(Tag::ByteArray(a)) if a.is_empty() => Collector::Generic(Vec::new()),
        Some(Tag::IntArray(a)) if a.is_empty() => Collector::Generic(Vec::new()),
        Some(Tag::LongArray(a)) if a.is_empty() => Collector::Generic(Vec::new()),
        Some(Tag::ByteArray(a)) => Collector::Bytes(a.clone()),
        Some(Tag::IntArray(a)) => Collector::Ints(a.clone()),
        Some(Tag::LongArray(a)) => Collector::Longs(a.clone()),
        _ => return Dr::Err(Some(list.clone())),
    };
    for v in values {
        collector = match (collector, v) {
            (Collector::Bytes(mut a), Tag::Byte(x)) => {
                a.push(x);
                Collector::Bytes(a)
            }
            (Collector::Ints(mut a), Tag::Int(x)) => {
                a.push(x);
                Collector::Ints(a)
            }
            (Collector::Longs(mut a), Tag::Long(x)) => {
                a.push(x);
                Collector::Longs(a)
            }
            (Collector::Bytes(a), v) => Collector::Generic(a.into_iter().map(Tag::Byte).chain(std::iter::once(v)).collect()),
            (Collector::Ints(a), v) => Collector::Generic(a.into_iter().map(Tag::Int).chain(std::iter::once(v)).collect()),
            (Collector::Longs(a), v) => Collector::Generic(a.into_iter().map(Tag::Long).chain(std::iter::once(v)).collect()),
            (Collector::Generic(mut l), v) => {
                l.push(v);
                Collector::Generic(l)
            }
        };
    }
    Dr::Ok(Some(match collector {
        Collector::Generic(l) => Tag::List(l),
        Collector::Bytes(a) => Tag::ByteArray(a),
        Collector::Ints(a) => Tag::IntArray(a),
        Collector::Longs(a) => Tag::LongArray(a),
    }))
}

// DynamicOps.mergeToPrimitive: a primitive goes onto nothing but EndTag.
pub fn merge_to_primitive(prefix: &Nbt, val: Tag) -> Dr<Nbt> {
    if prefix.is_some() {
        Dr::Err(Some(Some(val)))
    } else {
        Dr::Ok(Some(val))
    }
}

pub fn remove(input: &Nbt, key: &str) -> Nbt {
    match input {
        Some(Tag::Compound(c)) => {
            let mut c = c.clone();
            c.remove(key);
            Some(Tag::Compound(c))
        }
        other => other.clone(),
    }
}

// DynamicOps.set: mergeToMap, or the input when that fails.
pub fn set(input: &Nbt, key: &str, val: Nbt) -> Nbt {
    match input {
        None | Some(Tag::Compound(_)) => merge_to_map(input, key, value(val)).result().expect("a map"),
        other => other.clone(),
    }
}

// Dynamic: a value with NbtOps.
#[derive(Clone, Debug)]
pub struct Dy(pub Nbt);

// OptionalDynamic: a DataResult of a Dynamic.
#[derive(Clone, Debug)]
pub struct ODy(pub Dr<Dy>);

impl PartialEq for Dy {
    fn eq(&self, other: &Dy) -> bool {
        nbt_eq(&self.0, &other.0)
    }
}

impl Dy {
    pub fn of(tag: Tag) -> Dy {
        Dy(Some(tag))
    }

    pub fn empty() -> Dy {
        Dy(None)
    }

    pub fn tag(&self) -> &Tag {
        match &self.0 {
            Some(t) => t,
            None => refuse("an EndTag where the data fixer reads a tag"),
        }
    }

    pub fn into_tag(self) -> Tag {
        value(self.0)
    }

    pub fn get(&self, key: &str) -> ODy {
        match get_map(&self.0) {
            None => ODy(Dr::err()),
            Some(m) => match m.get(key) {
                Some(v) => ODy(Dr::Ok(Dy(Some(v.clone())))),
                None => ODy(Dr::err()),
            },
        }
    }

    pub fn has(&self, key: &str) -> bool {
        get_map(&self.0).is_some_and(|m| m.contains(key))
    }

    pub fn set(self, key: &str, val: Dy) -> Dy {
        Dy(set(&self.0, key, val.0))
    }

    pub fn remove(self, key: &str) -> Dy {
        Dy(remove(&self.0, key))
    }

    // Dynamic.update: the function of the field, when there is one.
    pub fn update(self, key: &str, f: impl FnOnce(Dy) -> Dy) -> Dy {
        match self.get(key).0 {
            Dr::Ok(v) => {
                let new = f(v);
                Dy(set(&self.0, key, new.0))
            }
            _ => self,
        }
    }

    pub fn set_field_if_present(self, key: &str, val: Option<Dy>) -> Dy {
        match val {
            None => self,
            Some(v) => self.set(key, v),
        }
    }

    pub fn rename_field(self, old: &str, new: &str) -> Dy {
        self.rename_and_fix_field(old, new, |d| d)
    }

    pub fn rename_and_fix_field(self, old: &str, new: &str, f: impl FnOnce(Dy) -> Dy) -> Dy {
        let value = self.get(old).result().map(f);
        self.remove(old).set_field_if_present(new, value)
    }

    // renameAndFixField whose fixer may return null (the field then goes).
    pub fn rename_and_fix_field_opt(self, old: &str, new: &str, f: impl FnOnce(Dy) -> Option<Dy>) -> Dy {
        let value = self.get(old).result().and_then(f);
        self.remove(old).set_field_if_present(new, value)
    }

    pub fn replace_field(self, old: &str, new: &str, value: Option<Dy>) -> Dy {
        self.remove(old).set_field_if_present(new, value)
    }

    pub fn as_number(&self) -> Dr<Num> {
        match get_number(&self.0) {
            Some(n) => Dr::Ok(n),
            None => Dr::err(),
        }
    }

    pub fn as_string(&self) -> Dr<String> {
        match get_string(&self.0) {
            Some(s) => Dr::Ok(s.to_string()),
            None => Dr::err(),
        }
    }

    pub fn as_string_or(&self, default: &str) -> String {
        self.as_string().result().unwrap_or_else(|| default.to_string())
    }

    pub fn as_int(&self, default: i32) -> i32 {
        self.as_number().result().map_or(default, Num::int_value)
    }

    pub fn as_long(&self, default: i64) -> i64 {
        self.as_number().result().map_or(default, Num::long_value)
    }

    pub fn as_byte(&self, default: i8) -> i8 {
        self.as_number().result().map_or(default, Num::byte_value)
    }

    pub fn as_short(&self, default: i16) -> i16 {
        self.as_number().result().map_or(default, Num::short_value)
    }

    pub fn as_float(&self, default: f32) -> f32 {
        self.as_number().result().map_or(default, Num::float_value)
    }

    pub fn as_double(&self, default: f64) -> f64 {
        self.as_number().result().map_or(default, Num::double_value)
    }

    // getBooleanValue: the number's byteValue() is not 0.
    pub fn as_boolean_dr(&self) -> Dr<bool> {
        self.as_number().map(|n| n.byte_value() != 0)
    }

    pub fn as_boolean(&self, default: bool) -> bool {
        self.as_boolean_dr().result().unwrap_or(default)
    }

    pub fn as_stream_opt(&self) -> Dr<Vec<Dy>> {
        match get_stream(&self.0) {
            Some(items) => Dr::Ok(items.into_iter().map(Dy::of).collect()),
            None => Dr::err(),
        }
    }

    pub fn as_stream(&self) -> Vec<Dy> {
        self.as_stream_opt().result().unwrap_or_default()
    }

    // getMapValues: the entries in the compound's order. Java iterates a
    // HashMap; a caller whose output shows that order must not use this.
    pub fn as_map_opt(&self) -> Dr<Vec<(Dy, Dy)>> {
        match get_map(&self.0) {
            Some(m) => Dr::Ok(m.0.iter().map(|(k, v)| (Dy::of(Tag::String(k.clone())), Dy::of(v.clone()))).collect()),
            None => Dr::err(),
        }
    }

    // Dynamic.updateMapValues: each entry mapped, into a new compound (the
    // value as it is when it is no map).
    pub fn update_map_values(self, f: impl Fn(Dy, Dy) -> (Dy, Dy)) -> Dy {
        match self.as_map_opt().result() {
            Some(entries) => {
                let mapped: Vec<(Dy, Dy)> = entries.into_iter().map(|(k, v)| f(k, v)).collect();
                self.create_map(mapped)
            }
            None => self,
        }
    }

    pub fn create_string(&self, s: &str) -> Dy {
        Dy::of(Tag::String(s.to_string()))
    }

    pub fn create_int(&self, v: i32) -> Dy {
        Dy::of(Tag::Int(v))
    }

    pub fn create_long(&self, v: i64) -> Dy {
        Dy::of(Tag::Long(v))
    }

    pub fn create_short(&self, v: i16) -> Dy {
        Dy::of(Tag::Short(v))
    }

    pub fn create_byte(&self, v: i8) -> Dy {
        Dy::of(Tag::Byte(v))
    }

    pub fn create_float(&self, v: f32) -> Dy {
        Dy::of(Tag::Float(v))
    }

    pub fn create_double(&self, v: f64) -> Dy {
        Dy::of(Tag::Double(v))
    }

    pub fn create_boolean(&self, v: bool) -> Dy {
        Dy::of(Tag::Byte(v as i8))
    }

    pub fn empty_map(&self) -> Dy {
        Dy::of(Tag::Compound(Compound::new()))
    }

    pub fn empty_list(&self) -> Dy {
        Dy::of(Tag::List(Vec::new()))
    }

    // createList: a ListTag of the values.
    pub fn create_list(&self, items: impl IntoIterator<Item = Dy>) -> Dy {
        Dy::of(Tag::List(items.into_iter().map(Dy::into_tag).collect()))
    }

    // createMap: a compound of the entries; a key that is no string throws.
    pub fn create_map(&self, entries: impl IntoIterator<Item = (Dy, Dy)>) -> Dy {
        let mut c = Compound::new();
        for (k, v) in entries {
            match k.0 {
                Some(Tag::String(s)) => c.put(&s, v.into_tag()),
                _ => refuse("a map made with a key that is no string (the data fixer throws)"),
            }
        }
        Dy::of(Tag::Compound(c))
    }

    pub fn create_int_list(&self, items: impl IntoIterator<Item = i32>) -> Dy {
        Dy::of(Tag::IntArray(items.into_iter().collect()))
    }

    pub fn create_long_list(&self, items: impl IntoIterator<Item = i64>) -> Dy {
        Dy::of(Tag::LongArray(items.into_iter().collect()))
    }

    // getIntStream: an IntArrayTag, or a stream of numbers' intValue().
    pub fn as_int_stream_opt(&self) -> Dr<Vec<i32>> {
        match &self.0 {
            Some(Tag::IntArray(a)) => Dr::Ok(a.clone()),
            _ => match get_stream(&self.0) {
                Some(items) => {
                    if items.iter().all(|t| get_number(&Some(t.clone())).is_some()) {
                        Dr::Ok(items.iter().map(|t| get_number(&Some(t.clone())).unwrap().int_value()).collect())
                    } else {
                        Dr::err()
                    }
                }
                None => Dr::err(),
            },
        }
    }

    pub fn as_long_stream_opt(&self) -> Dr<Vec<i64>> {
        match &self.0 {
            Some(Tag::LongArray(a)) => Dr::Ok(a.clone()),
            _ => match get_stream(&self.0) {
                Some(items) => {
                    if items.iter().all(|t| get_number(&Some(t.clone())).is_some()) {
                        Dr::Ok(items.iter().map(|t| get_number(&Some(t.clone())).unwrap().long_value()).collect())
                    } else {
                        Dr::err()
                    }
                }
                None => Dr::err(),
            },
        }
    }

    pub fn is_map(&self) -> bool {
        get_map(&self.0).is_some()
    }
}

impl ODy {
    pub fn result(self) -> Option<Dy> {
        self.0.result()
    }

    pub fn is_present(&self) -> bool {
        self.0.is_ok()
    }

    pub fn get(self, key: &str) -> ODy {
        match self.0 {
            Dr::Ok(d) => d.get(key),
            Dr::Err(_) => ODy(Dr::err()),
        }
    }

    pub fn map(self, f: impl FnOnce(Dy) -> Dy) -> ODy {
        ODy(self.0.map(f))
    }

    pub fn as_string(&self) -> Dr<String> {
        match &self.0 {
            Dr::Ok(d) => d.as_string(),
            _ => Dr::err(),
        }
    }

    pub fn as_string_or(&self, default: &str) -> String {
        self.as_string().result().unwrap_or_else(|| default.to_string())
    }

    pub fn as_number(&self) -> Dr<Num> {
        match &self.0 {
            Dr::Ok(d) => d.as_number(),
            _ => Dr::err(),
        }
    }

    pub fn as_int(&self, default: i32) -> i32 {
        self.as_number().result().map_or(default, Num::int_value)
    }

    pub fn as_long(&self, default: i64) -> i64 {
        self.as_number().result().map_or(default, Num::long_value)
    }

    pub fn as_byte(&self, default: i8) -> i8 {
        self.as_number().result().map_or(default, Num::byte_value)
    }

    pub fn as_short(&self, default: i16) -> i16 {
        self.as_number().result().map_or(default, Num::short_value)
    }

    pub fn as_float(&self, default: f32) -> f32 {
        self.as_number().result().map_or(default, Num::float_value)
    }

    pub fn as_double(&self, default: f64) -> f64 {
        self.as_number().result().map_or(default, Num::double_value)
    }

    pub fn as_boolean(&self, default: bool) -> bool {
        match &self.0 {
            Dr::Ok(d) => d.as_boolean(default),
            _ => default,
        }
    }

    pub fn as_stream_opt(&self) -> Dr<Vec<Dy>> {
        match &self.0 {
            Dr::Ok(d) => d.as_stream_opt(),
            _ => Dr::err(),
        }
    }

    pub fn as_stream(&self) -> Vec<Dy> {
        self.as_stream_opt().result().unwrap_or_default()
    }

    pub fn as_map_opt(&self) -> Dr<Vec<(Dy, Dy)>> {
        match &self.0 {
            Dr::Ok(d) => d.as_map_opt(),
            _ => Dr::err(),
        }
    }

    // asIntStream: the ints, or none.
    pub fn as_int_stream_opt_or_empty(&self) -> Vec<i32> {
        match &self.0 {
            Dr::Ok(d) => d.as_int_stream_opt().result().unwrap_or_default(),
            _ => Vec::new(),
        }
    }

    // orElseEmptyMap: the value, or an empty compound.
    pub fn or_else_empty_map(self) -> Dy {
        self.0.result().unwrap_or_else(|| Dy::of(Tag::Compound(Compound::new())))
    }

    pub fn or_else_empty_list(self) -> Dy {
        self.0.result().unwrap_or_else(|| Dy::of(Tag::List(Vec::new())))
    }
}
