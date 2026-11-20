//! The `see_build` differential's comparison: for every case of
//! `tests/node/see-build-cases.json`, today's `executeToolCall` with the
//! helper built from this tree's `native/draw` (`node.jsonl`,
//! `tests/node/see-build.js`) against `mcbench-tools` with the app's
//! `mcbench_draw::live::Draw` (`rust.jsonl`, `examples/see_build.rs`): the
//! tool message's content byte for byte, the screenshots message, the
//! sha256 of every JPEG in order, the shots each `shots` request was given
//! (numbers as `JSON.stringify` writes them), a montage's sha256, and
//! whether the call threw.
//!
//! Masked: the 8 hex digits of a saved image's name
//! (`crypto.randomUUID().slice(0, 8)`). A difference that is not this
//! crate's is listed in `KNOWN` exactly as it prints, with why; any other
//! fails.
//!
//! The run on adipc: both sides on the software floor (the released
//! helper's SwiftShader, given to both as `softwareVulkan`, and the
//! helper's `hello` given `adapter: "software"` by `see-build.js`, the
//! Rust side's `Draw` started with `AdapterChoice::Software`), then both on
//! the hardware adapter (each side's own rule: `hello` without `adapter`,
//! `AdapterChoice::Hardware`).
//!
//! `MCBENCH_TOOLS_SEE_BUILD=<folder with node.jsonl and rust.jsonl>`;
//! ignored without it, and failing when a file is missing.

use serde_json::Value;

fn masked(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("/work/images/") {
        let (head, tail) = rest.split_at(at + "/work/images/".len());
        out.push_str(head);
        if tail.get(8..).is_some_and(|after| after.starts_with(".jpg")) {
            out.push_str("########");
            rest = &tail[8..];
        } else {
            rest = tail;
        }
    }
    out.push_str(rest);
    out
}

fn lines(folder: &std::path::Path, name: &str) -> Vec<Value> {
    let text = std::fs::read_to_string(folder.join(name))
        .unwrap_or_else(|error| panic!("{}: {error}", folder.join(name).display()));
    text.lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[test]
#[ignore = "needs MCBENCH_TOOLS_SEE_BUILD=<folder with node.jsonl and rust.jsonl>"]
fn see_build_answers_as_todays_executor_with_the_same_pictures() {
    let folder = std::path::PathBuf::from(
        std::env::var("MCBENCH_TOOLS_SEE_BUILD").expect("MCBENCH_TOOLS_SEE_BUILD"),
    );
    let node = lines(&folder, "node.jsonl");
    let rust = lines(&folder, "rust.jsonl");
    assert_eq!(node.len(), rust.len(), "one line per case on each side");
    let mut differ = Vec::new();
    let (mut pictures, mut identical) = (0, 0);
    for (node, rust) in node.iter().zip(&rust) {
        assert_eq!(node["key"], rust["key"]);
        let key = node["key"].as_str().unwrap();
        let mut fields = Vec::new();
        for field in ["content", "screenshots", "images", "drawn", "card", "threw"] {
            let side = |line: &Value| {
                let value = line.get(field).cloned().unwrap_or(Value::Null);
                match (field, &value) {
                    ("content", Value::String(text)) => Value::String(masked(text)),
                    _ => value,
                }
            };
            let (a, b) = (side(node), side(rust));
            if mcbench_json::stringify(&a) != mcbench_json::stringify(&b) {
                fields.push(format!(
                    "{field}:\n  node {}\n  rust {}",
                    mcbench_json::stringify(&a),
                    mcbench_json::stringify(&b)
                ));
            }
        }
        pictures +=
            node["images"].as_array().map_or(0, Vec::len) + usize::from(node.get("card").is_some());
        let report = format!("{key}\n{}", fields.join("\n"));
        if fields.is_empty() {
            identical += 1;
        } else if KNOWN
            .iter()
            .any(|(case, difference, _)| *case == key && report == format!("{key}\n{difference}"))
        {
            println!("known difference: {report}");
        } else {
            differ.push(report);
        }
    }
    println!(
        "see_build differential: {identical} of {} cases identical, {pictures} pictures compared by sha256",
        node.len()
    );
    for (case, _, why) in KNOWN {
        println!("known difference in {case:?}: {why}");
    }
    assert!(differ.is_empty(), "{}", differ.join("\n\n"));
}

/// The differences that are not this crate's, each exactly as the
/// comparison prints it, and why.
const KNOWN: &[(&str, &str, &str)] = &[(
    "a view inherited from Object.prototype",
    "content:\n  node \"{\\\"ok\\\":false,\\\"error\\\":\\\"Screenshot capture: shots: invalid type: null, expected f64\\\"}\"\n  rust \"{\\\"ok\\\":false,\\\"error\\\":\\\"Screenshot capture: A shot: invalid type: null, expected f64\\\"}\"",
    "a view named after an Object.prototype property gives today's camera of NaN numbers (written null); the helper refuses the shot as `shots: <serde's reason>` (native/draw/src/serve.rs), the in-process Draw::shots as `A shot: <serde's reason>` (native/draw/src/live/mod.rs): the drawing crate's sentence, the renderer session's to make one",
)];
