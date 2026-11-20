//! V8's outputs against this crate's, case by case (`tests/cases.js` writes
//! the cases; the kinds are described there). `golden.jsonl` always runs; a
//! file of generated cases runs with
//! `MCBENCH_JSON_CASES=<file> cargo test -p mcbench-json -- --ignored`.

use std::collections::BTreeMap;
use std::io::BufRead;

use serde_json::{Map, Number, Value};

fn double(bits: &str) -> f64 {
    f64::from_bits(u64::from_str_radix(bits, 16).expect("16 hex digits"))
}

fn integer(text: &str) -> Value {
    match text.parse::<u64>() {
        Ok(u) => Value::from(u),
        Err(_) => Value::from(text.parse::<i64>().expect("a 64-bit integer")),
    }
}

/// The value a case's tree stands for, built in the order JavaScript built it.
fn tree(t: &Value) -> Value {
    match t {
        Value::Null | Value::Bool(_) | Value::String(_) => t.clone(),
        Value::Array(items) => Value::Array(items.iter().map(tree).collect()),
        Value::Object(o) => {
            if let Some(Value::String(bits)) = o.get("n") {
                return Value::Number(Number::from_f64(double(bits)).expect("a finite double"));
            }
            if let Some(Value::String(i)) = o.get("i") {
                return integer(i);
            }
            let mut map = Map::new();
            for entry in o["o"].as_array().expect("entries") {
                let key = entry[0].as_str().expect("a key").to_owned();
                map.insert(key, tree(&entry[1]));
            }
            Value::Object(map)
        }
        Value::Number(_) => panic!("a tree holds numbers as bits"),
    }
}

fn text(v: &Value) -> &str {
    v.as_str().expect("a string")
}

/// Checks one case; the differences it found.
fn check(case: &Value) -> Vec<String> {
    let mut differences = Vec::new();
    let mut same = |what: &str, ours: &str, v8: &str| {
        if ours != v8 {
            differences.push(format!("{what}: ours {ours:?}, V8 {v8:?}"));
        }
    };
    match text(&case["k"]) {
        "num" => {
            let x = double(text(&case["i"]));
            same(
                "String(x)",
                &mcbench_json::number_to_string(x),
                text(&case["o"]),
            );
            let json = match Number::from_f64(x) {
                Some(n) => mcbench_json::stringify(&Value::Number(n)),
                None => mcbench_json::to_string(&x).unwrap(),
            };
            same("JSON.stringify(x)", &json, text(&case["j"]));
        }
        "int" => same(
            "JSON.stringify(Number(n))",
            &mcbench_json::stringify(&integer(text(&case["i"]))),
            text(&case["o"]),
        ),
        "str" => {
            let s: String = case["i"]
                .as_array()
                .unwrap()
                .iter()
                .map(|c| char::from_u32(c.as_u64().unwrap() as u32).expect("a scalar value"))
                .collect();
            same(
                "JSON.stringify(s)",
                &mcbench_json::to_string(&s).unwrap(),
                text(&case["o"]),
            );
        }
        "val" => {
            let value = tree(&case["i"]);
            let compact = mcbench_json::stringify(&value);
            same("JSON.stringify(v)", &compact, text(&case["o"]));
            let indent = case["p"][0].as_u64().unwrap() as usize;
            let pretty = mcbench_json::stringify_pretty(&value, indent);
            same("JSON.stringify(v, null, n)", &pretty, text(&case["p"][1]));
            for v8 in [text(&case["o"]), text(&case["p"][1])] {
                match mcbench_json::parse(v8) {
                    Ok(back) => {
                        let again = if v8.contains('\n') {
                            mcbench_json::stringify_pretty(&back, indent)
                        } else {
                            mcbench_json::stringify(&back)
                        };
                        same("round trip", &again, v8);
                    }
                    Err(error) => same("round trip", &format!("error {error}"), v8),
                }
            }
        }
        "parse" => match mcbench_json::parse(text(&case["i"])) {
            Ok(value) => same(
                "JSON.stringify(JSON.parse(t))",
                &mcbench_json::stringify(&value),
                text(&case["o"]),
            ),
            Err(error) => same(
                "JSON.stringify(JSON.parse(t))",
                &format!("error {error}"),
                text(&case["o"]),
            ),
        },
        "lone" => {
            if let Ok(value) = mcbench_json::parse(text(&case["i"])) {
                differences.push(format!("a lone surrogate was read: {value}"));
            }
        }
        other => differences.push(format!("unknown kind {other}")),
    }
    differences
}

/// Runs every case of `lines`; the totals by kind.
fn run(lines: impl Iterator<Item = String>) -> BTreeMap<String, (usize, usize)> {
    let mut totals: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    let mut shown = 0;
    for (n, line) in lines.enumerate() {
        let case: Value = serde_json::from_str(&line).expect("a case line");
        let differences = check(&case);
        let total = totals.entry(text(&case["k"]).to_owned()).or_default();
        total.0 += 1;
        if differences.is_empty() {
            total.1 += 1;
        } else if shown < 20 {
            shown += 1;
            eprintln!(
                "case {} ({}): {}",
                n + 1,
                text(&case["k"]),
                differences.join("; ")
            );
        }
    }
    totals
}

fn assert_all_identical(totals: &BTreeMap<String, (usize, usize)>) {
    for (kind, (cases, identical)) in totals {
        eprintln!("{kind}: {cases} cases, {identical} identical");
    }
    let differences: usize = totals.values().map(|(c, i)| c - i).sum();
    assert_eq!(differences, 0, "cases that differ from V8");
}

#[test]
fn golden_cases_are_identical() {
    let golden = include_str!("golden.jsonl");
    let totals = run(golden.lines().map(str::to_owned));
    assert!(totals.values().map(|(c, _)| c).sum::<usize>() > 300);
    assert_all_identical(&totals);
}

#[test]
#[ignore = "needs MCBENCH_JSON_CASES=<file written by tests/cases.js>"]
fn generated_cases_are_identical() {
    let path = std::env::var("MCBENCH_JSON_CASES")
        .expect("MCBENCH_JSON_CASES names the file tests/cases.js wrote");
    let file = std::fs::File::open(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let totals = run(std::io::BufReader::new(file)
        .lines()
        .map(|l| l.expect("a line")));
    assert!(
        totals.values().map(|(c, _)| c).sum::<usize>() >= 200_000,
        "{path} holds fewer than 200,000 cases"
    );
    assert_all_identical(&totals);
}
