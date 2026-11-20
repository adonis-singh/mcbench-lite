//! What the verifier takes from JavaScript to read a model's arguments as
//! `minecraft-verify.js` reads them: `Object.keys`' order, `trim`, numbers.

use serde_json::{Map, Number, Value};

/// `String.prototype.trim`: JavaScript's white space and line terminators,
/// Unicode's White_Space without U+0085, and U+FEFF.
pub(crate) fn trim(text: &str) -> &str {
    text.trim_matches(|c: char| (c.is_whitespace() && c != '\u{85}') || c == '\u{feff}')
}

/// A double as a JSON number (`JSON.stringify` writes it as JavaScript
/// does; integral values within 2^53 are kept as integers).
pub(crate) fn number(x: f64) -> Value {
    if x.fract() == 0.0 && x.abs() < 9_007_199_254_740_992.0 {
        Value::Number(Number::from(x as i64))
    } else {
        Number::from_f64(x).map_or(Value::Null, Value::Number)
    }
}

/// `Math.round`: halves go up.
pub(crate) fn round(x: f64) -> f64 {
    (x + 0.5).floor()
}

/// `String(x)` of a number.
pub(crate) fn text(x: f64) -> String {
    mcbench_json::number_to_string(x)
}

/// `Object.keys(object)`: the array-index keys first, ascending, then the
/// others in insertion order.
pub(crate) fn keys(object: &Map<String, Value>) -> Vec<&str> {
    let index = |key: &str| -> Option<u32> {
        let bytes = key.as_bytes();
        if bytes.is_empty()
            || bytes.len() > 10
            || !bytes.iter().all(u8::is_ascii_digit)
            || (bytes[0] == b'0' && bytes.len() > 1)
        {
            return None;
        }
        let value: u64 = key.parse().ok()?;
        (value <= 4_294_967_294).then_some(value as u32)
    };
    let mut indices: Vec<(u32, &str)> = object
        .keys()
        .filter_map(|key| index(key).map(|i| (i, key.as_str())))
        .collect();
    indices.sort();
    let mut out: Vec<&str> = indices.into_iter().map(|(_, key)| key).collect();
    out.extend(
        object
            .keys()
            .filter(|key| index(key).is_none())
            .map(String::as_str),
    );
    out
}

/// Whether `value` is an object holding exactly the keys `names`
/// (`Object.keys(value).sort().join(',') === names.join(',')`, which only an
/// object with those keys meets).
pub(crate) fn has_exactly(value: &Value, names: &[&str]) -> bool {
    value.as_object().is_some_and(|object| {
        object.len() == names.len() && names.iter().all(|name| object.contains_key(*name))
    })
}

/// JavaScript's truthiness of a value (`!value` is its negation).
pub(crate) fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|x| x != 0.0 && !x.is_nan()),
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(_) | Value::Object(_)) => true,
    }
}

/// `value ?? fallback` on a property: absent and `null` take the fallback.
pub(crate) fn or_default<'a>(value: Option<&'a Value>, fallback: &'a Value) -> &'a Value {
    match value {
        None | Some(Value::Null) => fallback,
        Some(value) => value,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_go_in_javascripts_property_order() {
        let object: Map<String, Value> =
            serde_json::from_str(r#"{"b":1,"2":1,"a":1,"10":1,"01":1,"0":1}"#).unwrap();
        assert_eq!(keys(&object), ["0", "2", "10", "b", "a", "01"]);
        assert_eq!(number(5.0), Value::from(5));
        assert_eq!(number(-0.0), Value::from(0));
        assert_eq!(round(2.5), 3.0);
        assert_eq!(round(0.4), 0.0);
        assert_eq!(trim("\u{feff} x\u{a0}"), "x");
        assert_eq!(trim("\u{85}x"), "\u{85}x");
    }
}
