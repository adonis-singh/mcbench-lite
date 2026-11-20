//! # mcbench-json
//!
//! JavaScript's `JSON.stringify` from Rust, for every place Rust writes JSON
//! that today's JavaScript wrote: text a model reads (tool schemas, tool
//! results, the budgets) and rows Node reads back. `serde_json` writes `2.0`
//! where JavaScript writes `2`, `1e21` where it writes `1e+21`, and keeps
//! insertion order where JavaScript puts index-like keys first; this crate
//! writes what V8 writes.
//!
//! | Item | What it does |
//! |---|---|
//! | [`number_to_string`] | ECMAScript `Number::toString(10)` |
//! | [`stringify()`] | `JSON.stringify(value)` |
//! | [`stringify_pretty`] | `JSON.stringify(value, null, indent)` |
//! | [`to_string`] | `JSON.stringify` of any `Serialize` value, without building a `Value` |
//! | [`parse`] | `JSON.parse` where its result differs from `serde_json`'s |
//!
//! ## Owner
//!
//! The stage-three session.
//!
//! ## Seams
//!
//! | Seam | What crosses | Who calls whom | Thread or process |
//! |---|---|---|---|
//! | every stage-three crate that writes JSON JavaScript wrote | `serde_json::Value`s or `Serialize` values in, text out; text in, a `Value` out | they call this crate | the caller's |
//!
//! ## Behaviour defined by
//!
//! V8's `String(number)`, `JSON.stringify` and `JSON.parse` (Node 24.13.1,
//! V8 13.6), by a differential test: `tests/cases.js` writes random inputs
//! with V8's outputs (doubles of every exponent, subnormals, integers around
//! 2^53 and 1e21, strings over all of Unicode, nested objects with index-like
//! and ordinary keys, deep arrays, JSON text JavaScript did not write), and
//! `tests/differential.rs` requires every output identical: the committed
//! `tests/golden.jsonl` always, a file of 200,000 or more cases when
//! `MCBENCH_JSON_CASES=<file>` names one.
//!
//! ## Decided
//!
//! The digits of a double are [`zmij`]'s (Schubfach: the shortest digits
//! that read back to the same double, the closest of them to its value),
//! which is what ECMAScript asks for; this crate only places the point and
//! the exponent by `Number::toString`'s rule, so it has no float printer of
//! its own.
//!
//! Integers are written as JavaScript writes the double they become: a
//! `u64` or `i64` beyond 2^53 is rounded to the nearest double first, since
//! that is the number JavaScript holds. `f32` is written as the double it
//! widens to, the number JavaScript would hold. NaN and the infinities are
//! `null`, as `JSON.stringify` writes them.
//!
//! Object keys go out in JavaScript's property order: keys that are array
//! indices (the canonical decimal of an integer in 0..=4294967294) first, in
//! ascending order, then the others in insertion order (a typed struct's
//! field order, a `serde_json::Map`'s order).
//!
//! Strings are escaped as V8 escapes them: `"` and `\`, `\b \f \n \r \t`,
//! the other controls below U+0020 as lowercase `\u00xx`, nothing else (`/`,
//! U+007F, U+2028 and U+2029 and all non-ASCII stay raw). V8 writes a lone
//! surrogate as `\udxxx`; a Rust string cannot hold one, so that case does
//! not arise when writing.
//!
//! [`parse`] is `serde_json`'s parser with three of `JSON.parse`'s results:
//! a number is the nearest double (integers within 2^53 stay integers, which
//! are the same numbers; larger ones become the double JavaScript holds), a
//! repeated key keeps its last value at its first position, and `-0` and
//! `0` are both written `0`. It refuses what a `serde_json::Value` cannot
//! hold, with `serde_json`'s message: a lone surrogate escape (`"\ud800"`,
//! which JavaScript writes when a string was cut inside a pair), a number
//! beyond the largest double (JavaScript reads `1e400` as `Infinity`, and
//! never writes it), and nesting deeper than 128. For text JavaScript wrote
//! without lone surrogates, `stringify(&parse(text)?) == text`, and the same
//! for `stringify_pretty` of text JavaScript indented.

use std::fmt::Write as _;

use serde::Serialize;
use serde::ser::{self, Impossible};
use serde_json::Value;

/// ECMAScript `Number::toString(10)` (ECMA-262, Number::toString): `2`, `0`
/// for `-0`, `0.1`, `1e+21`, `1e-7`, `123456789012345680000`, `5e-324`,
/// `NaN`, `Infinity`, `-Infinity`.
pub fn number_to_string(x: f64) -> String {
    if x.is_nan() {
        return "NaN".to_owned();
    }
    if x == 0.0 {
        return "0".to_owned();
    }
    if x.is_infinite() {
        return if x < 0.0 { "-Infinity" } else { "Infinity" }.to_owned();
    }
    let mut out = String::new();
    write_number(&mut out, x);
    out
}

/// Appends a finite, nonzero `x` as `Number::toString` writes it.
fn write_number(out: &mut String, x: f64) {
    if x < 0.0 {
        out.push('-');
    }
    let mut buffer = zmij::Buffer::new();
    let (digits, n) = digits_and_point(buffer.format_finite(x.abs()));
    let k = digits.len() as i32;
    if k <= n && n <= 21 {
        // Digits, then n - k zeros.
        out.push_str(&digits);
        out.extend(std::iter::repeat_n('0', (n - k) as usize));
    } else if 0 < n && n <= 21 {
        out.push_str(&digits[..n as usize]);
        out.push('.');
        out.push_str(&digits[n as usize..]);
    } else if -6 < n && n <= 0 {
        out.push_str("0.");
        out.extend(std::iter::repeat_n('0', (-n) as usize));
        out.push_str(&digits);
    } else {
        out.push_str(&digits[..1]);
        if k > 1 {
            out.push('.');
            out.push_str(&digits[1..]);
        }
        let e = n - 1;
        write!(out, "e{}{}", if e < 0 { '-' } else { '+' }, e.abs()).expect("writing to a String");
    }
}

/// The shortest digits of a positive double as zmij prints them, and the
/// position of the decimal point: the value is `0.digits × 10^n`, digits
/// without leading or trailing zeros (ECMA-262's s, k and n).
fn digits_and_point(printed: &str) -> (String, i32) {
    let (mantissa, exponent) = match printed.find(['e', 'E']) {
        Some(at) => (
            &printed[..at],
            printed[at + 1..]
                .parse::<i32>()
                .expect("zmij writes a decimal exponent"),
        ),
        None => (printed, 0),
    };
    let point = mantissa.find('.').unwrap_or(mantissa.len()) as i32;
    let all: String = mantissa.chars().filter(|c| *c != '.').collect();
    let leading = all.len() - all.trim_start_matches('0').len();
    let digits = all.trim_matches('0').to_owned();
    assert!(
        !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()),
        "zmij printed {printed}"
    );
    (digits, point - leading as i32 + exponent)
}

/// `JSON.stringify(value)`.
pub fn stringify(value: &Value) -> String {
    to_string(value).expect("a serde_json::Value always serializes")
}

/// `JSON.stringify(value, null, indent)`: `indent` spaces per level, at most
/// 10 as JavaScript clamps it; 0 is [`stringify()`].
pub fn stringify_pretty(value: &Value, indent: usize) -> String {
    let gap = " ".repeat(indent.min(10));
    value
        .serialize(Writer {
            gap: &gap,
            indent: String::new(),
        })
        .expect("a serde_json::Value always serializes")
}

/// `JSON.stringify` of a `Serialize` value, in serde's data model as
/// `serde_json` maps it (a unit variant is its name, a newtype variant is
/// `{"Name": value}`, `None` is `null`, bytes are an array of numbers). Map
/// keys must be strings, chars, booleans or numbers (written as JavaScript
/// writes a number used as a key).
pub fn to_string<T: Serialize + ?Sized>(value: &T) -> Result<String, Error> {
    value.serialize(Writer {
        gap: "",
        indent: String::new(),
    })
}

/// `JSON.parse(text)` (see Decided for where it refuses what JavaScript
/// reads).
pub fn parse(text: &str) -> Result<Value, String> {
    let mut value: Value = serde_json::from_str(text).map_err(|error| error.to_string())?;
    to_doubles(&mut value);
    Ok(value)
}

/// Integers beyond 2^53 become the double JavaScript holds for them.
fn to_doubles(value: &mut Value) {
    const SAFE: u64 = (1 << 53) - 1;
    match value {
        Value::Number(number) => {
            let unsafe_integer = match (number.as_u64(), number.as_i64()) {
                (Some(u), _) => u > SAFE,
                (None, Some(i)) => i.unsigned_abs() > SAFE,
                (None, None) => false,
            };
            if unsafe_integer {
                let x = number.as_f64().expect("every number has a double");
                *number = serde_json::Number::from_f64(x).expect("a 64-bit integer is finite");
            }
        }
        Value::Array(items) => items.iter_mut().for_each(to_doubles),
        Value::Object(map) => map.values_mut().for_each(to_doubles),
        Value::Null | Value::Bool(_) | Value::String(_) => {}
    }
}

/// Appends `s` quoted and escaped as V8's `JSON.stringify` writes it.
fn quote_into(out: &mut String, s: &str) {
    out.push('"');
    let mut start = 0;
    for (at, byte) in s.bytes().enumerate() {
        let escape = match byte {
            b'"' => "\\\"",
            b'\\' => "\\\\",
            0x08 => "\\b",
            0x0c => "\\f",
            b'\n' => "\\n",
            b'\r' => "\\r",
            b'\t' => "\\t",
            0x00..=0x1f => "",
            _ => continue,
        };
        out.push_str(&s[start..at]);
        if escape.is_empty() {
            write!(out, "\\u{byte:04x}").expect("writing to a String");
        } else {
            out.push_str(escape);
        }
        start = at + 1;
    }
    out.push_str(&s[start..]);
    out.push('"');
}

fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    quote_into(&mut out, s);
    out
}

/// A number as `JSON.stringify` writes it: `null` when not finite.
fn json_number(x: f64) -> String {
    if x.is_finite() {
        number_to_string(x)
    } else {
        "null".to_owned()
    }
}

/// Whether `key` is an array index, which JavaScript orders first: the
/// canonical decimal of an integer in 0..=4294967294.
fn array_index(key: &str) -> Option<u32> {
    let bytes = key.as_bytes();
    if bytes.is_empty()
        || bytes.len() > 10
        || !bytes.iter().all(u8::is_ascii_digit)
        || (bytes[0] == b'0' && bytes.len() > 1)
    {
        return None;
    }
    let value: u64 = key.parse().expect("ten ASCII digits fit a u64");
    (value <= 4_294_967_294).then_some(value as u32)
}

/// The error of [`to_string`]: a map key that is not a string or a number,
/// or an error a `Serialize` implementation raised.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error(pub String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

impl ser::Error for Error {
    fn custom<T: std::fmt::Display>(message: T) -> Self {
        Error(message.to_string())
    }
}

/// Serializes one value to its text at the indentation `indent`, with `gap`
/// added per level (empty for the compact form).
#[derive(Clone)]
struct Writer<'a> {
    gap: &'a str,
    indent: String,
}

impl<'a> Writer<'a> {
    fn child(&self) -> Writer<'a> {
        Writer {
            gap: self.gap,
            indent: format!("{}{}", self.indent, self.gap),
        }
    }

    /// `[a,b]`, or indented: `[\n  a,\n  b\n]`; `[]` when empty.
    fn array(&self, items: Vec<String>) -> String {
        if items.is_empty() {
            return "[]".to_owned();
        }
        if self.gap.is_empty() {
            return format!("[{}]", items.join(","));
        }
        let inner = format!("{}{}", self.indent, self.gap);
        format!(
            "[\n{inner}{}\n{}]",
            items.join(&format!(",\n{inner}")),
            self.indent
        )
    }

    /// An object in JavaScript's property order: array indices ascending,
    /// then the other keys in the order given.
    fn object(&self, mut entries: Vec<(String, String)>) -> String {
        if entries.is_empty() {
            return "{}".to_owned();
        }
        let indices = entries
            .iter()
            .filter(|(key, _)| array_index(key).is_some())
            .count();
        if indices > 0 {
            // A stable sort keeps the other keys in insertion order.
            entries.sort_by_key(|(key, _)| array_index(key).map_or((1, 0), |i| (0, i)));
        }
        let colon = if self.gap.is_empty() { ":" } else { ": " };
        let items = entries
            .into_iter()
            .map(|(key, value)| format!("{}{colon}{value}", quote(&key)))
            .collect::<Vec<_>>();
        if self.gap.is_empty() {
            return format!("{{{}}}", items.join(","));
        }
        let inner = format!("{}{}", self.indent, self.gap);
        format!(
            "{{\n{inner}{}\n{}}}",
            items.join(&format!(",\n{inner}")),
            self.indent
        )
    }
}

/// A sequence, map or struct being written; `variant` wraps it as
/// `{"Variant": ...}`.
struct Compound<'a> {
    outer: Writer<'a>,
    body: Writer<'a>,
    variant: Option<&'static str>,
    items: Vec<String>,
    entries: Vec<(String, String)>,
    key: Option<String>,
}

impl<'a> Compound<'a> {
    fn new(outer: Writer<'a>, variant: Option<&'static str>) -> Self {
        let body = if variant.is_some() {
            outer.child()
        } else {
            outer.clone()
        };
        Compound {
            outer,
            body,
            variant,
            items: Vec::new(),
            entries: Vec::new(),
            key: None,
        }
    }

    fn item<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        self.items.push(value.serialize(self.body.child())?);
        Ok(())
    }

    fn entry<T: Serialize + ?Sized>(&mut self, key: String, value: &T) -> Result<(), Error> {
        let value = value.serialize(self.body.child())?;
        self.entries.push((key, value));
        Ok(())
    }

    fn wrap(self, body: String) -> String {
        match self.variant {
            Some(name) => self.outer.object(vec![(name.to_owned(), body)]),
            None => body,
        }
    }

    fn end_array(mut self) -> Result<String, Error> {
        let body = self.body.array(std::mem::take(&mut self.items));
        Ok(self.wrap(body))
    }

    fn end_object(mut self) -> Result<String, Error> {
        let body = self.body.object(std::mem::take(&mut self.entries));
        Ok(self.wrap(body))
    }
}

impl<'a> ser::Serializer for Writer<'a> {
    type Ok = String;
    type Error = Error;
    type SerializeSeq = Compound<'a>;
    type SerializeTuple = Compound<'a>;
    type SerializeTupleStruct = Compound<'a>;
    type SerializeTupleVariant = Compound<'a>;
    type SerializeMap = Compound<'a>;
    type SerializeStruct = Compound<'a>;
    type SerializeStructVariant = Compound<'a>;

    fn serialize_bool(self, v: bool) -> Result<String, Error> {
        Ok(if v { "true" } else { "false" }.to_owned())
    }
    fn serialize_i8(self, v: i8) -> Result<String, Error> {
        Ok(json_number(v.into()))
    }
    fn serialize_i16(self, v: i16) -> Result<String, Error> {
        Ok(json_number(v.into()))
    }
    fn serialize_i32(self, v: i32) -> Result<String, Error> {
        Ok(json_number(v.into()))
    }
    fn serialize_i64(self, v: i64) -> Result<String, Error> {
        Ok(json_number(v as f64))
    }
    fn serialize_i128(self, v: i128) -> Result<String, Error> {
        Ok(json_number(v as f64))
    }
    fn serialize_u8(self, v: u8) -> Result<String, Error> {
        Ok(json_number(v.into()))
    }
    fn serialize_u16(self, v: u16) -> Result<String, Error> {
        Ok(json_number(v.into()))
    }
    fn serialize_u32(self, v: u32) -> Result<String, Error> {
        Ok(json_number(v.into()))
    }
    fn serialize_u64(self, v: u64) -> Result<String, Error> {
        Ok(json_number(v as f64))
    }
    fn serialize_u128(self, v: u128) -> Result<String, Error> {
        Ok(json_number(v as f64))
    }
    fn serialize_f32(self, v: f32) -> Result<String, Error> {
        Ok(json_number(v.into()))
    }
    fn serialize_f64(self, v: f64) -> Result<String, Error> {
        Ok(json_number(v))
    }
    fn serialize_char(self, v: char) -> Result<String, Error> {
        Ok(quote(v.encode_utf8(&mut [0; 4])))
    }
    fn serialize_str(self, v: &str) -> Result<String, Error> {
        Ok(quote(v))
    }
    fn serialize_bytes(self, v: &[u8]) -> Result<String, Error> {
        Ok(self.array(v.iter().map(|b| json_number((*b).into())).collect()))
    }
    fn serialize_none(self) -> Result<String, Error> {
        Ok("null".to_owned())
    }
    fn serialize_some<T: Serialize + ?Sized>(self, value: &T) -> Result<String, Error> {
        value.serialize(self)
    }
    fn serialize_unit(self) -> Result<String, Error> {
        Ok("null".to_owned())
    }
    fn serialize_unit_struct(self, _: &'static str) -> Result<String, Error> {
        Ok("null".to_owned())
    }
    fn serialize_unit_variant(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
    ) -> Result<String, Error> {
        Ok(quote(variant))
    }
    fn serialize_newtype_struct<T: Serialize + ?Sized>(
        self,
        _: &'static str,
        value: &T,
    ) -> Result<String, Error> {
        value.serialize(self)
    }
    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<String, Error> {
        let body = value.serialize(self.child())?;
        Ok(self.object(vec![(variant.to_owned(), body)]))
    }
    fn serialize_seq(self, _: Option<usize>) -> Result<Compound<'a>, Error> {
        Ok(Compound::new(self, None))
    }
    fn serialize_tuple(self, _: usize) -> Result<Compound<'a>, Error> {
        Ok(Compound::new(self, None))
    }
    fn serialize_tuple_struct(self, _: &'static str, _: usize) -> Result<Compound<'a>, Error> {
        Ok(Compound::new(self, None))
    }
    fn serialize_tuple_variant(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
        _: usize,
    ) -> Result<Compound<'a>, Error> {
        Ok(Compound::new(self, Some(variant)))
    }
    fn serialize_map(self, _: Option<usize>) -> Result<Compound<'a>, Error> {
        Ok(Compound::new(self, None))
    }
    fn serialize_struct(self, _: &'static str, _: usize) -> Result<Compound<'a>, Error> {
        Ok(Compound::new(self, None))
    }
    fn serialize_struct_variant(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
        _: usize,
    ) -> Result<Compound<'a>, Error> {
        Ok(Compound::new(self, Some(variant)))
    }
}

impl ser::SerializeSeq for Compound<'_> {
    type Ok = String;
    type Error = Error;
    fn serialize_element<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        self.item(value)
    }
    fn end(self) -> Result<String, Error> {
        self.end_array()
    }
}

impl ser::SerializeTuple for Compound<'_> {
    type Ok = String;
    type Error = Error;
    fn serialize_element<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        self.item(value)
    }
    fn end(self) -> Result<String, Error> {
        self.end_array()
    }
}

impl ser::SerializeTupleStruct for Compound<'_> {
    type Ok = String;
    type Error = Error;
    fn serialize_field<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        self.item(value)
    }
    fn end(self) -> Result<String, Error> {
        self.end_array()
    }
}

impl ser::SerializeTupleVariant for Compound<'_> {
    type Ok = String;
    type Error = Error;
    fn serialize_field<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        self.item(value)
    }
    fn end(self) -> Result<String, Error> {
        self.end_array()
    }
}

impl ser::SerializeMap for Compound<'_> {
    type Ok = String;
    type Error = Error;
    fn serialize_key<T: Serialize + ?Sized>(&mut self, key: &T) -> Result<(), Error> {
        self.key = Some(key.serialize(KeyWriter)?);
        Ok(())
    }
    fn serialize_value<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        let key = self.key.take().expect("serde gives a key before its value");
        self.entry(key, value)
    }
    fn end(self) -> Result<String, Error> {
        self.end_object()
    }
}

impl ser::SerializeStruct for Compound<'_> {
    type Ok = String;
    type Error = Error;
    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), Error> {
        self.entry(key.to_owned(), value)
    }
    fn end(self) -> Result<String, Error> {
        self.end_object()
    }
}

impl ser::SerializeStructVariant for Compound<'_> {
    type Ok = String;
    type Error = Error;
    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), Error> {
        self.entry(key.to_owned(), value)
    }
    fn end(self) -> Result<String, Error> {
        self.end_object()
    }
}

/// A map key as the property name JavaScript would use for it.
struct KeyWriter;

fn key_error() -> Error {
    Error("a JSON object key must be a string, a char, a boolean or a number".to_owned())
}

impl ser::Serializer for KeyWriter {
    type Ok = String;
    type Error = Error;
    type SerializeSeq = Impossible<String, Error>;
    type SerializeTuple = Impossible<String, Error>;
    type SerializeTupleStruct = Impossible<String, Error>;
    type SerializeTupleVariant = Impossible<String, Error>;
    type SerializeMap = Impossible<String, Error>;
    type SerializeStruct = Impossible<String, Error>;
    type SerializeStructVariant = Impossible<String, Error>;

    fn serialize_bool(self, v: bool) -> Result<String, Error> {
        Ok(if v { "true" } else { "false" }.to_owned())
    }
    fn serialize_i8(self, v: i8) -> Result<String, Error> {
        Ok(number_to_string(v.into()))
    }
    fn serialize_i16(self, v: i16) -> Result<String, Error> {
        Ok(number_to_string(v.into()))
    }
    fn serialize_i32(self, v: i32) -> Result<String, Error> {
        Ok(number_to_string(v.into()))
    }
    fn serialize_i64(self, v: i64) -> Result<String, Error> {
        Ok(number_to_string(v as f64))
    }
    fn serialize_i128(self, v: i128) -> Result<String, Error> {
        Ok(number_to_string(v as f64))
    }
    fn serialize_u8(self, v: u8) -> Result<String, Error> {
        Ok(number_to_string(v.into()))
    }
    fn serialize_u16(self, v: u16) -> Result<String, Error> {
        Ok(number_to_string(v.into()))
    }
    fn serialize_u32(self, v: u32) -> Result<String, Error> {
        Ok(number_to_string(v.into()))
    }
    fn serialize_u64(self, v: u64) -> Result<String, Error> {
        Ok(number_to_string(v as f64))
    }
    fn serialize_u128(self, v: u128) -> Result<String, Error> {
        Ok(number_to_string(v as f64))
    }
    fn serialize_f32(self, v: f32) -> Result<String, Error> {
        Ok(number_to_string(v.into()))
    }
    fn serialize_f64(self, v: f64) -> Result<String, Error> {
        Ok(number_to_string(v))
    }
    fn serialize_char(self, v: char) -> Result<String, Error> {
        Ok(v.to_string())
    }
    fn serialize_str(self, v: &str) -> Result<String, Error> {
        Ok(v.to_owned())
    }
    fn serialize_bytes(self, _: &[u8]) -> Result<String, Error> {
        Err(key_error())
    }
    fn serialize_none(self) -> Result<String, Error> {
        Err(key_error())
    }
    fn serialize_some<T: Serialize + ?Sized>(self, _: &T) -> Result<String, Error> {
        Err(key_error())
    }
    fn serialize_unit(self) -> Result<String, Error> {
        Err(key_error())
    }
    fn serialize_unit_struct(self, _: &'static str) -> Result<String, Error> {
        Err(key_error())
    }
    fn serialize_unit_variant(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
    ) -> Result<String, Error> {
        Ok(variant.to_owned())
    }
    fn serialize_newtype_struct<T: Serialize + ?Sized>(
        self,
        _: &'static str,
        value: &T,
    ) -> Result<String, Error> {
        value.serialize(self)
    }
    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: &T,
    ) -> Result<String, Error> {
        Err(key_error())
    }
    fn serialize_seq(self, _: Option<usize>) -> Result<Self::SerializeSeq, Error> {
        Err(key_error())
    }
    fn serialize_tuple(self, _: usize) -> Result<Self::SerializeTuple, Error> {
        Err(key_error())
    }
    fn serialize_tuple_struct(
        self,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeTupleStruct, Error> {
        Err(key_error())
    }
    fn serialize_tuple_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeTupleVariant, Error> {
        Err(key_error())
    }
    fn serialize_map(self, _: Option<usize>) -> Result<Self::SerializeMap, Error> {
        Err(key_error())
    }
    fn serialize_struct(self, _: &'static str, _: usize) -> Result<Self::SerializeStruct, Error> {
        Err(key_error())
    }
    fn serialize_struct_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeStructVariant, Error> {
        Err(key_error())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn numbers_print_as_javascript_prints_them() {
        for (x, text) in [
            (2.0, "2"),
            (-0.0, "0"),
            (0.1, "0.1"),
            (1e21, "1e+21"),
            (1e-7, "1e-7"),
            (123456789012345680000.0, "123456789012345680000"),
            (5e-324, "5e-324"),
            (f64::NAN, "NaN"),
            (f64::INFINITY, "Infinity"),
            (f64::NEG_INFINITY, "-Infinity"),
            (1.5e-7, "1.5e-7"),
            (0.000001, "0.000001"),
            (-1234.5, "-1234.5"),
            (2147483647.0 / 1048576.0, "2047.9999990463257"),
        ] {
            assert_eq!(number_to_string(x), text, "{x:e}");
        }
    }

    #[test]
    fn index_keys_come_first_in_ascending_order() {
        let value =
            json!({ "b": 1, "10": 2, "a": 3, "2": 4, "01": 5, "4294967295": 6, "4294967294": 7 });
        assert_eq!(
            stringify(&value),
            r#"{"2":4,"10":2,"4294967294":7,"b":1,"a":3,"01":5,"4294967295":6}"#
        );
    }

    #[test]
    fn pretty_form_is_javascripts() {
        let value = json!({ "a": [1, [], {}, { "b": null }], "c": "x" });
        assert_eq!(
            stringify_pretty(&value, 2),
            "{\n  \"a\": [\n    1,\n    [],\n    {},\n    {\n      \"b\": null\n    }\n  ],\n  \"c\": \"x\"\n}"
        );
        assert_eq!(stringify_pretty(&value, 0), stringify(&value));
        assert_eq!(stringify_pretty(&value, 40), stringify_pretty(&value, 10));
    }

    #[test]
    fn typed_values_print_as_javascript_prints_them() {
        #[derive(Serialize)]
        enum Kind {
            Plain,
            Wrapped(f64),
            Pair(u8, u8),
            Named { x: f32 },
        }
        #[derive(Serialize)]
        struct Row {
            ticks: f64,
            missing: Option<u32>,
            kinds: Vec<Kind>,
            bytes: serde_json::Value,
            big: u64,
            nan: f64,
        }
        let row = Row {
            ticks: 2.0,
            missing: None,
            kinds: vec![
                Kind::Plain,
                Kind::Wrapped(1e21),
                Kind::Pair(1, 2),
                Kind::Named { x: 0.5 },
            ],
            bytes: json!({ "1": true, "0": false }),
            big: 9007199254740993,
            nan: f64::NAN,
        };
        assert_eq!(
            to_string(&row).unwrap(),
            r#"{"ticks":2,"missing":null,"kinds":["Plain",{"Wrapped":1e+21},{"Pair":[1,2]},{"Named":{"x":0.5}}],"bytes":{"0":false,"1":true},"big":9007199254740992,"nan":null}"#
        );
        let mut keys = std::collections::BTreeMap::new();
        keys.insert(3u32, 'a');
        assert_eq!(to_string(&keys).unwrap(), r#"{"3":"a"}"#);
        let mut bad = std::collections::BTreeMap::new();
        bad.insert(vec![1], 1);
        assert!(to_string(&bad).is_err());
    }

    #[test]
    fn parse_keeps_the_last_value_of_a_repeated_key_at_its_first_position() {
        let value = parse(r#"{"a":1,"b":2,"a":3,"big":12345678901234567890,"z":-0}"#).unwrap();
        assert_eq!(
            stringify(&value),
            r#"{"a":3,"b":2,"big":12345678901234567000,"z":0}"#
        );
        assert!(parse(r#""\ud800""#).is_err());
    }
}
