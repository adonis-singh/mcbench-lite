//! JSON as the store reads and writes it: `serde_json`'s value (with
//! `preserve_order`, so an object keeps JavaScript's insertion order), read
//! with `mcbench_json::parse` (`JSON.parse`) and written with
//! `mcbench_json::stringify` (`JSON.stringify`); and the few JavaScript
//! readings of a value the store's ports make (truthiness, `String(value)`).

pub use mcbench_json::{parse, stringify};
pub use serde_json::{Value as Json, to_value};

/// A JSON object, keys in insertion order.
pub type Object = serde_json::Map<String, Json>;

/// `JSON.stringify(value, null, 2)`.
pub fn stringify_indented(value: &Json) -> String {
    mcbench_json::stringify_pretty(value, 2)
}

/// JavaScript's truthiness of a value.
pub trait Truthy {
    /// Whether JavaScript reads it as true.
    fn truthy(&self) -> bool;
}

impl Truthy for Json {
    fn truthy(&self) -> bool {
        match self {
            Json::Null => false,
            Json::Bool(b) => *b,
            Json::Number(n) => n.as_f64().is_some_and(|n| n != 0.0 && !n.is_nan()),
            Json::String(s) => !s.is_empty(),
            Json::Array(_) | Json::Object(_) => true,
        }
    }
}

/// JavaScript's truthiness of `value.key`, `undefined` being false.
pub fn truthy(value: Option<&Json>) -> bool {
    value.is_some_and(Truthy::truthy)
}

/// JavaScript's `String(value)` (`undefined` for `None`).
pub fn to_js_string(value: Option<&Json>) -> String {
    match value {
        None => "undefined".to_string(),
        Some(Json::Null) => "null".to_string(),
        Some(Json::Bool(b)) => b.to_string(),
        Some(Json::Number(n)) => mcbench_json::number_to_string(n.as_f64().expect("a JSON number")),
        Some(Json::String(s)) => s.clone(),
        Some(Json::Array(items)) => items
            .iter()
            .map(|item| match item {
                Json::Null => String::new(),
                other => to_js_string(Some(other)),
            })
            .collect::<Vec<_>>()
            .join(","),
        Some(Json::Object(_)) => "[object Object]".to_string(),
    }
}

/// A JavaScript number as a value: an integer within 2^53 is held as one,
/// so it equals the value `JSON.parse` gives for the same text.
pub fn number(value: f64) -> Json {
    if value.fract() == 0.0 && value.abs() <= 9_007_199_254_740_991.0 {
        Json::from(value as i64)
    } else {
        Json::from(value)
    }
}

/// A JSON string value.
pub fn str(text: impl Into<String>) -> Json {
    Json::String(text.into())
}

/// An object from `key => value` pairs in the order JavaScript's literal
/// gives them; each value is anything `serde` serialises.
#[macro_export]
#[doc(hidden)]
macro_rules! object {
    ($($key:expr => $value:expr),* $(,)?) => {{
        #[allow(unused_mut)]
        let mut object = $crate::json::Object::new();
        $(object.insert(($key).to_string(), $crate::json::to_value($value).expect("a JSON value"));)*
        $crate::json::Json::Object(object)
    }};
}
