//! The replay's comparison: for every call of `calls.jsonl`
//! (`tests/node/prepare.js`), today's `executeToolCall` (`node.jsonl`,
//! `tests/node/replay.js`) against `mcbench-tools` (`rust.jsonl`,
//! `examples/replay.rs`): the tool message's content byte for byte, the
//! screenshots message, the saved images, the world's manifest after the
//! call (its chunk bytes by their hashes), the `tool_start` line, the
//! `tool_result` fields, the progress messages, the command the sandbox was
//! given and whether the call threw. And both against what is stored.
//!
//! Masked, compared by presence and shape only: the 8 hex digits of a saved
//! image's name (`crypto.randomUUID().slice(0, 8)`) and a test report's
//! UUID (`/work/verification/<uuid>.json`). Nothing compared carries a wall
//! time: bash's transcript (which has one) is the recorded run's on both
//! sides.
//!
//! `MCBENCH_TOOLS_REPLAY=<folder with the three files>`; ignored without it.

use std::collections::BTreeMap;

use serde_json::Value;

/// Every number under a key ending in `Ms`, and `reportBytes` (the size of
/// a report that holds those times), written `"<ms>"`, inside a JSON
/// value and inside the JSON texts it holds (a tool message's content, a
/// result's output).
fn wall_free(value: &Value) -> Value {
    fn walk(value: &mut Value) {
        match value {
            Value::Object(map) => {
                for (key, value) in map.iter_mut() {
                    if (key.ends_with("Ms") || key == "reportBytes") && value.is_number() {
                        *value = Value::String("<ms>".into());
                    } else {
                        walk(value);
                    }
                }
            }
            Value::Array(items) => items.iter_mut().for_each(walk),
            Value::String(text) if text.starts_with('{') => {
                if let Ok(mut inner) = serde_json::from_str::<Value>(text) {
                    walk(&mut inner);
                    *text = mcbench_json::stringify(&inner);
                }
            }
            _ => {}
        }
    }
    let mut value = value.clone();
    walk(&mut value);
    value
}

fn mask(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    let hex = |b: u8| b.is_ascii_digit() || (b'a'..=b'f').contains(&b);
    while i < bytes.len() {
        // `/api/images/<chat>/<prefix><8 hex>.<ext>`
        if text[i..].starts_with("/api/images/") {
            let end = text[i..]
                .find(['"', ' ', '\n'])
                .map_or(text.len(), |e| i + e);
            let url = &text[i..end];
            let (head, name) = url.rsplit_once('/').unwrap_or((url, ""));
            let stem = name.split('.').next().unwrap_or("");
            let keep = stem.len().saturating_sub(8);
            out.push_str(head);
            out.push('/');
            out.push_str(&stem[..keep]);
            out.push_str("########");
            out.push_str(&name[stem.len()..]);
            i = end;
            continue;
        }
        // A random UUID (a test report's file name, a new prompt's id).
        let uuid = |at: usize| {
            at + 36 <= bytes.len()
                && bytes[at..at + 36].iter().enumerate().all(|(k, &b)| {
                    if [8, 13, 18, 23].contains(&k) {
                        b == b'-'
                    } else {
                        hex(b)
                    }
                })
        };
        if (i == 0 || !hex(bytes[i - 1])) && uuid(i) {
            out.push_str("<uuid>");
            i += 36;
            continue;
        }
        let c = text[i..].chars().next().unwrap();
        out.push(c);
        i += c.len_utf8();
    }
    out
}

fn read(path: &std::path::Path) -> BTreeMap<String, Value> {
    std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
        .lines()
        .map(|line| {
            let value: Value = serde_json::from_str(line).unwrap();
            (value["key"].as_str().unwrap().to_string(), value)
        })
        .collect()
}

#[derive(Default)]
struct Tally {
    calls: usize,
    same_node: usize,
    timed: usize,
    untyped: usize,
    polled: usize,
    stored: usize,
    same_stored_node: usize,
    same_stored_rust: usize,
    differences: Vec<String>,
    stored_differences: BTreeMap<String, (usize, String)>,
}

#[test]
#[ignore = "needs MCBENCH_TOOLS_REPLAY=<folder of calls.jsonl, node.jsonl, rust.jsonl>"]
fn every_call_answers_as_today() {
    let folder = std::path::PathBuf::from(
        std::env::var("MCBENCH_TOOLS_REPLAY").expect("MCBENCH_TOOLS_REPLAY"),
    );
    let calls = std::fs::read_to_string(folder.join("calls.jsonl")).expect("calls.jsonl");
    let node = read(&folder.join("node.jsonl"));
    let rust = read(&folder.join("rust.jsonl"));
    let mut tallies: BTreeMap<String, Tally> = BTreeMap::new();
    for line in calls.lines() {
        let call: Value = serde_json::from_str(line).unwrap();
        let key = call["key"].as_str().unwrap();
        let group = format!(
            "{} {}",
            call["source"].as_str().unwrap(),
            call["name"].as_str().unwrap()
        );
        let tally = tallies.entry(group).or_default();
        tally.calls += 1;
        let (Some(n), Some(r)) = (node.get(key), rust.get(key)) else {
            tally.differences.push(format!(
                "{key}: missing from {}",
                if node.contains_key(key) {
                    "rust"
                } else {
                    "node"
                }
            ));
            continue;
        };
        let mut differing = Vec::new();
        // A program that logs its own clock (Date.now, performance.now, or
        // code it reads from /work and evaluates)
        // logs other numbers on each run: its logs are compared by count.
        let program = call["arguments"].as_str().unwrap_or("");
        let timed = program.contains("Date")
            || program.contains("performance")
            || program.contains("readFile");
        let without_logs = |side: &Value| -> Option<(Value, usize)> {
            let mut content: Value = serde_json::from_str(side["content"].as_str()?).ok()?;
            let logs = content.as_object_mut()?.get_mut("logs")?;
            let count = logs.as_array()?.len();
            *logs = Value::Null;
            Some((content, count))
        };
        if timed
            && n["content"] != r["content"]
            && without_logs(n).is_some()
            && without_logs(n) == without_logs(r)
        {
            tally.timed += 1;
        }
        let timed = timed && without_logs(n).is_some() && without_logs(n) == without_logs(r);
        // A program or command that is not a string is the value `tool_start`
        // carries, on both sides.
        if !n["start"]["program"].is_string() && !n["start"]["program"].is_null() {
            tally.untyped += 1;
        }
        // The verifier's wall times (`metrics.*Ms`, `timings`) are compared
        // by presence and type.
        let (n, r) = (&wall_free(n), &wall_free(r));
        for field in [
            "content",
            "screenshots",
            "images",
            "manifest",
            "start",
            "progress",
            "exec",
            "threw",
        ] {
            let (a, b) = (mask(&n[field].to_string()), mask(&r[field].to_string()));
            // The verifier's progress is read from its progress file as it is
            // written: how many updates a run gives depends on its speed.
            let polled = field == "progress" && call["name"] == "test_build";
            if polled && a != b {
                tally.polled += 1;
            }
            if a != b && !(timed && field == "content") && !polled {
                differing.push(format!(
                    "{field}: node {} / rust {}",
                    a.chars().take(300).collect::<String>(),
                    b.chars().take(300).collect::<String>()
                ));
            }
        }
        // tool_result: the shown fields; images by count (URLs on the
        // server, paths here); output where the server writes one.
        let (nr, rr) = (&n["result"], &r["result"]);
        if !(nr.is_null() && rr.is_null()) {
            for field in ["ok", "cancelled", "error", "diff"] {
                let node_value = match (field, &nr[field]) {
                    ("ok" | "cancelled", Value::Null) => Value::Bool(false),
                    (_, value) => value.clone(),
                };
                let node_value = if field == "diff" && nr["ok"] != Value::Bool(true) {
                    Value::Null
                } else {
                    node_value
                };
                if node_value != rr[field] {
                    differing.push(format!(
                        "result.{field}: node {} / rust {}",
                        node_value, rr[field]
                    ));
                }
            }
            if !nr["output"].is_null()
                && call["name"] != "prompt_library"
                && mask(&nr["output"].to_string()) != mask(&rr["output"].to_string())
            {
                differing.push("result.output".into());
            }
            let count = |v: &Value| v.as_array().map_or(0, Vec::len);
            if count(&nr["images"]) != count(&rr["images"]) {
                differing.push("result.images".into());
            }
        }
        if differing.is_empty() {
            tally.same_node += 1;
        } else {
            tally
                .differences
                .push(format!("{key}: {}", differing.join("; ")));
        }
        if let Some(stored) = call["stored"].as_str() {
            tally.stored += 1;
            let stored = mask(
                wall_free(&Value::String(stored.to_string()))
                    .as_str()
                    .unwrap(),
            );
            let same = |side: &Value| {
                side["content"].as_str().map(mask).as_deref() == Some(stored.as_str())
            };
            if same(n) {
                tally.same_stored_node += 1;
            } else {
                let class = stored_class(&stored, n["content"].as_str().unwrap_or(""));
                let example = key.to_string();
                tally
                    .stored_differences
                    .entry(class)
                    .or_insert((0, example))
                    .0 += 1;
            }
            if same(r) {
                tally.same_stored_rust += 1;
            }
        }
    }
    let mut failures = Vec::new();
    for (group, tally) in &tallies {
        println!(
            "{group}: {} calls, {} identical to Node ({} but the logs of a program that reads the clock; {} but the verifier's polled progress; {} whose program is not text){}",
            tally.calls,
            tally.same_node,
            tally.timed,
            tally.polled,
            tally.untyped,
            if tally.stored > 0 {
                format!(
                    ", stored {}: Node reproduces {}, Rust {}",
                    tally.stored, tally.same_stored_node, tally.same_stored_rust
                )
            } else {
                String::new()
            }
        );
        for (class, (count, example)) in &tally.stored_differences {
            println!("    {count} x stored differs from today's: {class} (first {example})");
        }
        for difference in &tally.differences {
            println!(
                "    DIFFERS {}",
                difference.chars().take(900).collect::<String>()
            );
            failures.push(difference.clone());
        }
    }
    assert!(
        failures.is_empty(),
        "{} calls differ from Node",
        failures.len()
    );
}

/// A short name for how a stored result differs from today's: the first
/// top-level key whose value differs, or the shape.
fn stored_class(stored: &str, today: &str) -> String {
    let (Ok(Value::Object(a)), Ok(Value::Object(b))) = (
        serde_json::from_str::<Value>(stored),
        serde_json::from_str::<Value>(today),
    ) else {
        return "text".into();
    };
    let keys_a: Vec<&String> = a.keys().collect();
    let keys_b: Vec<&String> = b.keys().collect();
    if keys_a != keys_b {
        return format!("keys {keys_a:?} now {keys_b:?}");
    }
    for (key, value) in &a {
        if b.get(key) != Some(value) {
            if key == "error" {
                let s = value.as_str().unwrap_or("");
                return format!(
                    "error (stored starts {:?})",
                    s.chars().take(60).collect::<String>()
                );
            }
            return format!("value of {key}");
        }
    }
    "key order or number form".into()
}
