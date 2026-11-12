// The static tables of the schemas (X lines of datafix.txt): maps of strings,
// by "Class.FIELD".

use std::collections::HashMap;
use std::sync::OnceLock;

fn tables() -> &'static HashMap<String, HashMap<String, String>> {
    static TABLES: OnceLock<HashMap<String, HashMap<String, String>>> = OnceLock::new();
    TABLES.get_or_init(|| {
        let mut maps: HashMap<String, HashMap<String, String>> = HashMap::new();
        for rest in super::schemas::DUMP.lines().filter_map(|l| l.strip_prefix("X ")) {
            let mut p = rest.splitn(3, ' ');
            let (name, key, value) = (p.next().unwrap(), p.next().unwrap(), p.next().unwrap());
            maps.entry(name.to_string()).or_default().insert(key.to_string(), value.to_string());
        }
        maps
    })
}

pub fn map(name: &str) -> &'static HashMap<String, String> {
    tables().get(name).unwrap_or_else(|| panic!("datafix.txt has no table {name}"))
}

// The first choice (in Java's map order) that has a default value, of the
// schemas' entity and block entity choice types, by their sorted keys (P lines).
pub fn choice_point(sorted_keys: &str) -> Option<&'static str> {
    static POINTS: OnceLock<HashMap<String, String>> = OnceLock::new();
    POINTS
        .get_or_init(|| {
            let mut points: HashMap<String, String> = HashMap::new();
            for l in super::schemas::DUMP.lines().filter_map(|l| l.strip_prefix("P ")) {
                let (first, keys) = l.split_once(' ').unwrap();
                // Schemas that disagree on the first: none is known.
                let e = points.entry(keys.to_string()).or_insert_with(|| first.to_string());
                if e != first {
                    e.clear();
                }
            }
            points
        })
        .get(sorted_keys)
        .map(String::as_str)
        .filter(|s| !s.is_empty())
}
