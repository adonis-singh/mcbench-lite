//! What the tool bodies take from JavaScript when they read a model's
//! arguments: `String(value)`, truthiness, `String.prototype.trim`,
//! `Number(value)`, `Number.isInteger`, UTF-16 lengths and slices, and the
//! `JSON.stringify` of a result (`mcbench_json`). Each follows the ECMAScript
//! operation it names for the JSON values a tool call can hold.

use serde_json::Value;

/// JavaScript's truthiness of a value (`undefined` is `None`): `null`,
/// `false`, `0`, `-0`, `NaN` and `""` are falsy.
pub fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|x| x != 0.0 && !x.is_nan()),
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(_)) | Some(Value::Object(_)) => true,
    }
}

/// ECMAScript `ToString` of a JSON value: a number as `Number::toString`
/// writes it, an array as `Array.prototype.join(',')` (null elements
/// empty), an object as `[object Object]`.
pub fn to_string(value: &Value) -> String {
    match value {
        Value::Null => "null".into(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => mcbench_json::number_to_string(n.as_f64().expect("a JSON number")),
        Value::String(s) => s.clone(),
        Value::Array(items) => items
            .iter()
            .map(|item| match item {
                Value::Null => String::new(),
                other => to_string(other),
            })
            .collect::<Vec<_>>()
            .join(","),
        Value::Object(_) => "[object Object]".into(),
    }
}

/// `String(value || '')`.
pub fn string_or_empty(value: Option<&Value>) -> String {
    if truthy(value) {
        to_string(value.expect("truthy"))
    } else {
        String::new()
    }
}

/// `String(value)` where `undefined` reads `undefined`.
pub fn string_of(value: Option<&Value>) -> String {
    value.map_or_else(|| "undefined".into(), to_string)
}

/// JavaScript's white space and line terminators (`String.prototype.trim`):
/// Unicode's White_Space without U+0085, and U+FEFF.
pub fn is_space(c: char) -> bool {
    (c.is_whitespace() && c != '\u{85}') || c == '\u{feff}'
}

/// `text.trim()`.
pub fn trim(text: &str) -> &str {
    text.trim_matches(is_space)
}

/// The length of `text` in UTF-16 code units (`.length`).
pub fn utf16_len(text: &str) -> usize {
    text.chars().map(char::len_utf16).sum()
}

/// `text.slice(0, units)` in UTF-16 code units. A cut inside a surrogate
/// pair keeps a lone high surrogate in JavaScript; a Rust string cannot
/// hold one, so it is U+FFFD here (what `Buffer` writes for it).
pub fn utf16_prefix(text: &str, units: usize) -> String {
    let mut out = String::new();
    let mut taken = 0;
    for c in text.chars() {
        let width = c.len_utf16();
        if taken + width > units {
            if taken < units {
                out.push('\u{fffd}');
            }
            break;
        }
        out.push(c);
        taken += width;
    }
    out
}

/// ECMAScript `StringToNumber`: NaN where JavaScript gives NaN.
pub fn string_to_number(text: &str) -> f64 {
    let text = trim(text);
    if text.is_empty() {
        return 0.0;
    }
    for (prefix, radix) in [
        ("0x", 16),
        ("0X", 16),
        ("0o", 8),
        ("0O", 8),
        ("0b", 2),
        ("0B", 2),
    ] {
        if let Some(digits) = text.strip_prefix(prefix) {
            if digits.is_empty() || !digits.chars().all(|c| c.is_digit(radix)) {
                return f64::NAN;
            }
            return digits.chars().fold(0.0, |x, c| {
                x * f64::from(radix) + f64::from(c.to_digit(radix).expect("a digit"))
            });
        }
    }
    match text {
        "Infinity" | "+Infinity" => return f64::INFINITY,
        "-Infinity" => return f64::NEG_INFINITY,
        _ => {}
    }
    if !text
        .bytes()
        .all(|b| b.is_ascii_digit() || matches!(b, b'+' | b'-' | b'.' | b'e' | b'E'))
    {
        return f64::NAN;
    }
    text.parse().unwrap_or(f64::NAN)
}

/// `Number(value)` of a JSON value (`undefined` is `None`, NaN).
pub fn to_number(value: Option<&Value>) -> f64 {
    match value {
        None => f64::NAN,
        Some(Value::Null) => 0.0,
        Some(Value::Bool(b)) => f64::from(u8::from(*b)),
        Some(Value::Number(n)) => n.as_f64().expect("a JSON number"),
        Some(Value::String(s)) => string_to_number(s),
        // ToPrimitive of an array is its join.
        Some(array @ Value::Array(_)) => string_to_number(&to_string(array)),
        Some(Value::Object(_)) => f64::NAN,
    }
}

/// `Number.isInteger(value)`: a number with no fraction.
pub fn integer(value: &Value) -> Option<f64> {
    let x = value.as_f64()?;
    (x.is_finite() && x.fract() == 0.0).then_some(x)
}

/// `Array.isArray(value) ? value : null`.
pub fn array(value: Option<&Value>) -> Option<&Vec<Value>> {
    value.and_then(Value::as_array)
}

/// `JSON.stringify(value)`.
pub fn stringify(value: &Value) -> String {
    mcbench_json::stringify(value)
}

/// `Buffer.byteLength(JSON.stringify(value))`.
pub fn json_bytes(value: &Value) -> usize {
    stringify(value).len()
}

/// `path.replace(/^\/?work(?:\/|$)/, '')`.
pub fn strip_work(path: &str) -> &str {
    let rest = path.strip_prefix('/').unwrap_or(path);
    match rest.strip_prefix("work") {
        Some("") => "",
        Some(after) if after.starts_with('/') => &after[1..],
        _ => path,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn strings_numbers_and_truthiness_are_javascripts() {
        assert_eq!(to_string(&json!([1, null, [2, 3], "a"])), "1,,2,3,a");
        assert_eq!(to_string(&json!({"a": 1})), "[object Object]");
        assert_eq!(to_string(&json!(1e21)), "1e+21");
        assert_eq!(to_string(&json!(2.0)), "2");
        assert_eq!(string_or_empty(Some(&json!(0))), "");
        assert_eq!(string_or_empty(Some(&json!(false))), "");
        assert_eq!(string_or_empty(Some(&json!(true))), "true");
        assert_eq!(string_or_empty(None), "");
        assert_eq!(trim("\u{feff} a\u{a0}\n"), "a");
        assert_eq!(trim("\u{85}a"), "\u{85}a");
        assert_eq!(to_number(Some(&json!("5000"))), 5000.0);
        assert_eq!(to_number(Some(&json!([7]))), 7.0);
        assert!(to_number(Some(&json!([1, 2]))).is_nan());
        assert_eq!(to_number(Some(&json!(null))), 0.0);
        assert!(to_number(Some(&json!({}))).is_nan());
        assert_eq!(utf16_len("a😀"), 3);
        assert_eq!(utf16_prefix("a😀b", 2), "a\u{fffd}");
        assert_eq!(utf16_prefix("a😀b", 3), "a😀");
        assert_eq!(strip_work("/work/a"), "a");
        assert_eq!(strip_work("work"), "");
        assert_eq!(strip_work("workshop"), "workshop");
    }
}
