//! What this crate's output takes from JavaScript: `JSON.stringify`'s
//! strings, an object's key order, the default `sort`'s order (UTF-16 code
//! units) and `String.prototype.localeCompare` for printable ASCII.

use std::cmp::Ordering;
use std::fmt::Write;

/// Appends `text` as `JSON.stringify` writes a string: `"` and `\` escaped,
/// `\b \f \n \r \t`, other characters below U+0020 as `\u00xx`, everything
/// else as it is (a Rust string has no lone surrogates to escape).
pub fn push_json_string(out: &mut String, text: &str) {
    out.push('"');
    let mut start = 0;
    for (at, byte) in text.bytes().enumerate() {
        let escape = match byte {
            b'"' => "\\\"",
            b'\\' => "\\\\",
            0x08 => "\\b",
            0x0c => "\\f",
            b'\n' => "\\n",
            b'\r' => "\\r",
            b'\t' => "\\t",
            0..=0x1f => "",
            _ => continue,
        };
        out.push_str(&text[start..at]);
        if escape.is_empty() {
            write!(out, "\\u{byte:04x}").expect("writing to a String");
        } else {
            out.push_str(escape);
        }
        start = at + 1;
    }
    out.push_str(&text[start..]);
    out.push('"');
}

/// `text` as `JSON.stringify` writes it.
pub fn json_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    push_json_string(&mut out, text);
    out
}

/// Whether `key` is an array index to JavaScript (a canonical decimal below
/// 2^32 - 1): such keys come first in an object, ascending.
fn array_index(key: &str) -> Option<u32> {
    if key.is_empty() || key.len() > 10 || (key.len() > 1 && key.starts_with('0')) {
        return None;
    }
    if !key.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let value: u64 = key.parse().ok()?;
    (value < u32::MAX as u64).then_some(value as u32)
}

/// `pairs` in the order a JavaScript object made by setting them in turn
/// keeps its keys: a key set again keeps its first place and takes the last
/// value; array-index keys come first, ascending.
pub fn object_order(pairs: Vec<(String, String)>) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::with_capacity(pairs.len());
    for (key, value) in pairs {
        match out.iter_mut().find(|(existing, _)| *existing == key) {
            Some(slot) => slot.1 = value,
            None => out.push((key, value)),
        }
    }
    if out.iter().any(|(key, _)| array_index(key).is_some()) {
        let mut indexed: Vec<(u32, (String, String))> = Vec::new();
        let mut named = Vec::new();
        for pair in out {
            match array_index(&pair.0) {
                Some(index) => indexed.push((index, pair)),
                None => named.push(pair),
            }
        }
        indexed.sort_by_key(|(index, _)| *index);
        out = indexed
            .into_iter()
            .map(|(_, pair)| pair)
            .chain(named)
            .collect();
    }
    out
}

/// The default `Array.prototype.sort` order of two strings: by UTF-16 code
/// units.
pub fn code_unit_cmp(a: &str, b: &str) -> Ordering {
    if a.is_ascii() && b.is_ascii() {
        return a.cmp(b);
    }
    a.encode_utf16().cmp(b.encode_utf16())
}

/// Printable ASCII in `localeCompare`'s order (Node 24's ICU, root
/// collation, read from Node: `scripts` of this crate's differential
/// harness hold it to Node). Letters of either case share a primary weight.
const COLLATION: &[u8] = b" _-,;:!?.'\"()[]{}@*/\\&#%`^+<=>|~$0123456789aAbBcCdDeEfFgGhHiIjJkKlLmMnNoOpPqQrRsStTuUvVwWxXyYzZ";

fn primary(byte: u8) -> Result<u8, String> {
    if byte.is_ascii_alphabetic() {
        let lower = byte.to_ascii_lowercase();
        let at = COLLATION
            .iter()
            .position(|&c| c == lower)
            .expect("letters are in the table");
        return Ok(at as u8);
    }
    COLLATION
        .iter()
        .position(|&c| c == byte)
        .map(|at| at as u8)
        .ok_or_else(|| {
            format!(
                "localeCompare is ported for printable ASCII only, not {:?}",
                byte as char
            )
        })
}

/// `a.localeCompare(b)` for printable ASCII: primary weights first (case
/// ignored), then case (lower before upper).
pub fn locale_cmp(a: &str, b: &str) -> Result<Ordering, String> {
    let (left, right) = (a.as_bytes(), b.as_bytes());
    for pass in 0..2 {
        let mut i = 0;
        loop {
            match (left.get(i), right.get(i)) {
                (None, None) => break,
                (None, Some(_)) => return Ok(Ordering::Less),
                (Some(_), None) => return Ok(Ordering::Greater),
                (Some(&x), Some(&y)) => {
                    let order = if pass == 0 {
                        primary(x)?.cmp(&primary(y)?)
                    } else {
                        x.is_ascii_uppercase().cmp(&y.is_ascii_uppercase())
                    };
                    if order != Ordering::Equal {
                        return Ok(order);
                    }
                }
            }
            i += 1;
        }
    }
    Ok(Ordering::Equal)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strings_are_escaped_as_json_stringify_escapes_them() {
        assert_eq!(
            json_string("a\"b\\c\n\u{1}\u{7f}é\u{2028}"),
            "\"a\\\"b\\\\c\\n\\u0001\u{7f}é\u{2028}\""
        );
    }

    #[test]
    fn index_keys_come_first_and_a_repeated_key_keeps_its_place() {
        let pairs = |list: &[(&str, &str)]| {
            list.iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            object_order(pairs(&[
                ("b", "1"),
                ("10", "x"),
                ("a", "2"),
                ("2", "y"),
                ("b", "3"),
                ("01", "z")
            ])),
            pairs(&[("2", "y"), ("10", "x"), ("b", "3"), ("a", "2"), ("01", "z")])
        );
    }

    #[test]
    fn locale_order_puts_punctuation_before_digits_before_letters() {
        assert_eq!(locale_cmp("a_", "a1"), Ok(Ordering::Less));
        assert_eq!(locale_cmp("B", "a"), Ok(Ordering::Greater));
        assert_eq!(locale_cmp("a", "A"), Ok(Ordering::Less));
        assert_eq!(locale_cmp("ab", "Ab"), Ok(Ordering::Less));
        assert_eq!(locale_cmp("Ab", "ac"), Ok(Ordering::Less));
        assert!(locale_cmp("é", "e").is_err());
    }
}
