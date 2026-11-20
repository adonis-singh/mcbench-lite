//! `mc-dsl.js` `createBlockNormalizer`: a block spec checked against the
//! block registry the request carries (`{ name: [properties, defaults] }`,
//! the game's own data), with today's checks and error text. The parent
//! runs it on every palette entry the child sends back (`applyChanges`),
//! so a forged entry fails exactly as it fails today.

use mcbench_world::BlockState;
use serde_json::{Map, Value};

use crate::format::js_trim;

/// `Object.prototype`'s own properties: a registry or properties lookup of
/// one of these names finds the prototype's member in JavaScript, which
/// changes the error (`entry is not iterable`, `allowed.includes is not a
/// function`).
const OBJECT_PROTOTYPE: [&str; 12] = [
    "constructor",
    "__defineGetter__",
    "__defineSetter__",
    "hasOwnProperty",
    "__lookupGetter__",
    "__lookupSetter__",
    "isPrototypeOf",
    "propertyIsEnumerable",
    "toString",
    "valueOf",
    "__proto__",
    "toLocaleString",
];

/// The canonical decimal of an integer in 0..=4294967294 (an array index).
fn array_index(key: &str) -> Option<u32> {
    if key.is_empty()
        || (key.len() > 1 && key.starts_with('0'))
        || !key.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let value: u64 = key.parse().ok()?;
    (value < u32::MAX as u64).then_some(value as u32)
}

/// `Object.keys(object)`: array-index keys ascending, then the others in
/// insertion order.
fn js_keys(object: &Map<String, Value>) -> Vec<&String> {
    let mut indexed: Vec<(u32, &String)> = Vec::new();
    let mut named = Vec::new();
    for key in object.keys() {
        match array_index(key) {
            Some(index) => indexed.push((index, key)),
            None => named.push(key),
        }
    }
    indexed.sort_by_key(|(index, _)| *index);
    indexed
        .into_iter()
        .map(|(_, key)| key)
        .chain(named)
        .collect()
}

/// `String(value)` of a JSON value.
fn js_string(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Number(number) => {
            mcbench_json::number_to_string(number.as_f64().expect("a JSON number"))
        }
        Value::Bool(b) => b.to_string(),
        Value::Null => "null".into(),
        // Array.prototype.join: null and undefined elements are empty.
        Value::Array(items) => items
            .iter()
            .map(|item| {
                if item.is_null() {
                    String::new()
                } else {
                    js_string(item)
                }
            })
            .collect::<Vec<_>>()
            .join(","),
        Value::Object(_) => "[object Object]".into(),
    }
}

/// The registry's block names, as `Object.keys(blockRegistry)` lists them.
pub struct Normalizer<'a> {
    registry: &'a Map<String, Value>,
    names: Vec<&'a String>,
}

impl<'a> Normalizer<'a> {
    /// The normalizer of `registry` (`createBlockNormalizer(blockRegistry)`).
    pub fn new(registry: &'a Value) -> Normalizer<'a> {
        let registry = registry
            .as_object()
            .expect("the block registry is an object");
        Normalizer {
            registry,
            names: js_keys(registry),
        }
    }

    /// `normalize(spec, label)`: the block state the world keeps, or the
    /// error message.
    pub fn normalize(&self, spec: &Value, label: &str) -> Result<BlockState, String> {
        let mut name: String;
        let mut owned_states = Map::new();
        let mut given: Option<&Map<String, Value>> = None;
        match spec {
            Value::String(text) => {
                name = text.clone();
                if let Some(bracket) = text.find('[') {
                    if !text.ends_with(']') {
                        return Err(format!("{label}: malformed block states in \"{text}\""));
                    }
                    name = text[..bracket].to_string();
                    for pair in text[bracket + 1..text.len() - 1].split(',') {
                        if pair.is_empty() {
                            continue;
                        }
                        let Some(eq) = pair.find('=') else {
                            return Err(format!(
                                "{label}: malformed block state \"{pair}\" in \"{text}\""
                            ));
                        };
                        owned_states.insert(
                            js_trim(&pair[..eq]).to_string(),
                            Value::String(js_trim(&pair[eq + 1..]).to_string()),
                        );
                    }
                }
            }
            Value::Object(object) if object.get("id").is_some_and(Value::is_string) => {
                name = object["id"].as_str().unwrap().to_string();
                match object.get("states") {
                    None | Some(Value::Null) => {}
                    Some(Value::Object(states)) => given = Some(states),
                    Some(_) => return Err(format!("{label}: states must be an object")),
                }
            }
            _ => {
                return Err(format!(
                    "{label}: block must be a string like \"oak_planks\" or \"oak_stairs[facing=north]\", or {{ id, states }}"
                ));
            }
        }

        let states = given.unwrap_or(&owned_states);
        let name = js_trim(name.strip_prefix("minecraft:").unwrap_or(&name)).to_string();
        if name.is_empty() {
            return Err(format!("{label}: empty block id"));
        }
        if name == "air" {
            return Ok(BlockState {
                id: "minecraft:air".into(),
                properties: Vec::new(),
            });
        }
        let Some(entry) = self.registry.get(&name) else {
            if OBJECT_PROTOTYPE.contains(&name.as_str()) {
                return Err("entry is not iterable".into());
            }
            let close: Vec<&str> = self
                .names
                .iter()
                .filter(|k| k.contains(name.as_str()) || name.contains(k.as_str()))
                .take(6)
                .map(|k| k.as_str())
                .collect();
            let hint = if close.is_empty() {
                String::new()
            } else {
                format!(" (closest matches: {})", close.join(", "))
            };
            return Err(format!("{label}: unknown block \"{name}\"{hint}"));
        };
        let properties = entry[0]
            .as_object()
            .expect("a registry entry's properties are an object");

        let mut normalized = Vec::new();
        for key in js_keys(states) {
            let Some(allowed) = properties.get(key) else {
                if OBJECT_PROTOTYPE.contains(&key.as_str()) {
                    return Err("allowed.includes is not a function".into());
                }
                let valid: Vec<&str> = js_keys(properties)
                    .into_iter()
                    .map(String::as_str)
                    .collect();
                let valid = if valid.is_empty() {
                    "none".to_string()
                } else {
                    valid.join(", ")
                };
                return Err(format!(
                    "{label}: block \"{name}\" has no state \"{key}\" (valid: {valid})"
                ));
            };
            let allowed: Vec<&str> = allowed
                .as_array()
                .expect("a registry state lists its values")
                .iter()
                .map(|v| v.as_str().expect("a registry state value is a string"))
                .collect();
            let value = js_string(&states[key.as_str()]);
            if !allowed.contains(&value.as_str()) {
                return Err(format!(
                    "{label}: \"{name}\" state {key}={value} is invalid (valid: {})",
                    allowed.join(", ")
                ));
            }
            normalized.push((key.clone(), value));
        }
        Ok(BlockState {
            id: format!("minecraft:{name}"),
            properties: normalized,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every spec of `harness/normalize-cases.js` gives the block state or
    /// the error text `createBlockNormalizer` gave in Node.
    #[test]
    fn every_spec_is_normalised_as_mc_dsl_js_does() {
        let text = include_str!("../tests/golden/normalize.jsonl");
        let lines: Vec<Value> = text
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let registry = &lines.last().unwrap()["registry"];
        let normalizer = Normalizer::new(registry);
        let mut checked = 0;
        for line in &lines[..lines.len() - 1] {
            let got = normalizer.normalize(&line["spec"], "palette entry");
            match (&got, line.get("ok"), line.get("error")) {
                (Ok(state), Some(ok), None) => {
                    assert_eq!(state.id, ok["id"], "{}", line["spec"]);
                    let states: Vec<(String, String)> = ok["states"]
                        .as_object()
                        .unwrap()
                        .iter()
                        .map(|(k, v)| (k.clone(), v.as_str().unwrap().to_string()))
                        .collect();
                    assert_eq!(state.properties, states, "{}", line["spec"]);
                }
                (Err(message), None, Some(error)) => {
                    assert_eq!(message, error, "{}", line["spec"])
                }
                _ => panic!("{}: got {got:?}, Node gave {line}", line["spec"]),
            }
            checked += 1;
        }
        assert_eq!(checked, 87);
    }
}
