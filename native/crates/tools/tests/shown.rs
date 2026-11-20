//! `call_shown` and `tool_shown` against today's server and page over every
//! stored tool call and tool result of a data folder: the three-way check
//! of `transcript-seam.md`. `MCBENCH_TOOLS_SHOWN=<out.jsonl>` names the file
//! `tests/node/shown.js` wrote from a clone of the folder; without it the
//! test is ignored. [`call_shown`] must equal the server's `tool_start` for
//! every call and [`tool_shown`] the page's reading of every result; the
//! other side's differences are counted by tool and by which fields differ,
//! and printed.

use std::collections::BTreeMap;

use mcbench_tools::{call_shown, tool_shown};
use serde_json::{Value, json};

fn opt(value: Option<String>) -> Value {
    value.map_or(Value::Null, Value::String)
}

#[derive(Default)]
struct Tally {
    total: usize,
    same_server: usize,
    same_page: usize,
    all_three: usize,
    // Fields differing, against the other side, with the first example's
    // values.
    classes: BTreeMap<String, (usize, String)>,
}

#[test]
#[ignore = "needs MCBENCH_TOOLS_SHOWN=<tests/node/shown.js output>"]
fn calls_and_results_show_as_today() {
    let file = std::env::var("MCBENCH_TOOLS_SHOWN").expect("MCBENCH_TOOLS_SHOWN");
    let text = std::fs::read_to_string(&file).unwrap_or_else(|e| panic!("{file}: {e}"));
    let mut calls: BTreeMap<String, Tally> = BTreeMap::new();
    let mut results: BTreeMap<String, Tally> = BTreeMap::new();
    let mut failures = Vec::new();
    for line in text.lines() {
        let row: Value = serde_json::from_str(line).unwrap();
        let name = row["name"].as_str().unwrap_or("").to_string();
        let differing = |a: &Value, b: &Value| -> Vec<String> {
            a.as_object()
                .unwrap()
                .iter()
                .filter(|(key, value)| b.get(key.as_str()) != Some(value))
                .map(|(key, _)| key.clone())
                .collect()
        };
        if row["kind"] == "call" {
            let shown = call_shown(&name, row["arguments"].as_str().unwrap_or(""));
            let ours =
                json!({"summary": shown.summary, "program": shown.program.unwrap_or(Value::Null)});
            let tally = calls.entry(name.clone()).or_default();
            tally.total += 1;
            let server = differing(&ours, &row["server"]);
            let page = differing(&ours, &row["page"]);
            if server.is_empty() {
                tally.same_server += 1;
            } else {
                failures.push(format!("call {name}: {server:?} differ from the server's"));
            }
            if page.is_empty() {
                tally.same_page += 1;
            }
            if server.is_empty() && page.is_empty() {
                tally.all_three += 1;
            }
            if !page.is_empty() {
                let class = format!("page differs in {}", page.join("+"));
                let entry = tally.classes.entry(class).or_insert((
                    0,
                    format!("server {} / page {}", row["server"], row["page"]),
                ));
                entry.0 += 1;
            }
        } else {
            let shown = tool_shown(&name, row["content"].as_str().unwrap());
            let status = if shown.cancelled {
                "cancelled"
            } else if shown.ok {
                "ok"
            } else {
                "failed"
            };
            // The counts as JavaScript's numbers (2266, not 2266.0).
            let diff = shown.diff.map_or(Value::Null, |d| {
                let counts = json!({"added": d.added, "removed": d.removed, "changed": d.changed});
                mcbench_json::parse(&mcbench_json::stringify(&counts)).unwrap()
            });
            let detail = if shown.ok {
                String::new()
            } else {
                match &shown.error {
                    Some(error) if !error.is_empty() => error.clone(),
                    _ if shown.output.as_deref().is_some_and(|o| !o.is_empty()) => String::new(),
                    _ => "failed".into(),
                }
            };
            let page_view = json!({"status": status, "diff": diff, "output": opt(shown.output.clone()), "detail": detail});
            // The page's diff object as stored, compared by its counts.
            let mut page = row["page"].clone();
            if let Some(d) = page.get("diff").filter(|d| !d.is_null()).cloned() {
                page["diff"] =
                    json!({"added": d["added"], "removed": d["removed"], "changed": d["changed"]});
            }
            let tally = results.entry(name.clone()).or_default();
            tally.total += 1;
            let against_page = differing(&page_view, &page);
            if against_page.is_empty() {
                tally.same_page += 1;
            } else {
                failures.push(format!(
                    "result {name}: {against_page:?} differ from the page's"
                ));
            }
            let server_view = json!({
                "ok": shown.ok, "cancelled": shown.cancelled, "error": opt(shown.error.clone()),
                "diff": diff, "output": opt(shown.output.clone()),
            });
            let server = &row["server"];
            let mut normalized = server.clone();
            if !server.is_null() {
                normalized["ok"] = json!(server["ok"] == json!(true));
                normalized["cancelled"] = json!(server["cancelled"] == json!(true));
                if !server["ok"].as_bool().unwrap_or(false) {
                    normalized["diff"] = Value::Null;
                }
                if let Some(d) = server
                    .get("diff")
                    .filter(|d| !d.is_null() && normalized["diff"] != Value::Null)
                {
                    normalized["diff"] = json!({"added": d["added"], "removed": d["removed"], "changed": d["changed"]});
                }
            }
            let against_server = if server.is_null() {
                vec!["all".to_string()]
            } else {
                differing(&server_view, &normalized)
            };
            if against_server.is_empty() {
                tally.same_server += 1;
            }
            if against_page.is_empty() && against_server.is_empty() {
                tally.all_three += 1;
            }
            if !against_server.is_empty() {
                let class = format!(
                    "server's tool_result differs in {}",
                    against_server.join("+")
                );
                let entry = tally
                    .classes
                    .entry(class)
                    .or_insert((0, format!("server {} / ours {}", normalized, server_view)));
                entry.0 += 1;
            }
        }
    }
    for (label, table) in [("call", &calls), ("result", &results)] {
        for (name, tally) in table {
            println!(
                "{label} {name}: {} stored, {} identical to the server, {} to the page, {} all three",
                tally.total, tally.same_server, tally.same_page, tally.all_three
            );
            for (class, (count, example)) in &tally.classes {
                let example: String = example.chars().take(400).collect();
                println!("    {count} x {class}; first: {example}");
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} differences:\n{}",
        failures.len(),
        failures
            .iter()
            .take(20)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}
