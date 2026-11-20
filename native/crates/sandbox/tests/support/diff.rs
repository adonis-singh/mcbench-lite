//! What the differential tests share: the cases of `tests/diff/cases.json`
//! run through a sandbox, each answer normalised (wall times, curl's and
//! dd's timings and bash's job pids masked) into a map by field.

use std::collections::BTreeMap;

use mcbench_sandbox::Budget;
use serde_json::Value;

use super::*;

/// The cases.
pub fn cases() -> Vec<Value> {
    let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/diff/cases.json");
    serde_json::from_str(&std::fs::read_to_string(file).unwrap()).unwrap()
}

/// Every case by id, normalised.
pub fn by_id(lines: &[Value]) -> BTreeMap<String, BTreeMap<String, String>> {
    lines
        .iter()
        .map(|l| (l["id"].as_str().unwrap().to_string(), normalised(l)))
        .collect()
}

pub fn mask(text: &str) -> String {
    let mut out = String::new();
    for line in text.split_inclusive('\n') {
        if let Some(rest) = line.strip_prefix("Wall time: ") {
            let end = rest.find(" seconds").map(|at| &rest[at..]).unwrap_or(rest);
            out.push_str("Wall time: N");
            out.push_str(end);
            continue;
        }
        // dd's own timing.
        if line.contains(" copied, ") && line.trim_end().ends_with("/s") {
            out.push_str("<dd's copy line>\n");
            continue;
        }
        let mut masked = line.to_string();
        // A job's pid in bash's notice (`bash: line 3:    88 Killed`): pids
        // count the box's earlier processes.
        if let Some(rest) = masked.strip_prefix("bash: line ")
            && let Some((number, after)) = rest.split_once(':')
            && let Some(pid) = after.trim_start().split(' ').next()
            && !pid.is_empty()
            && pid.bytes().all(|b| b.is_ascii_digit())
        {
            let tail = &after.trim_start()[pid.len()..];
            masked = format!("bash: line {number}: PID{tail}");
        }
        if let Some(at) = masked.find(" after ")
            && let Some(ms) = masked[at + 7..].find(" ms")
            && masked[at + 7..at + 7 + ms]
                .bytes()
                .all(|b| b.is_ascii_digit())
        {
            masked.replace_range(at + 7..at + 7 + ms, "N");
        }
        out.push_str(&masked);
    }
    out
}

pub fn normalised(line: &Value) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for key in [
        "exitCode",
        "formatted",
        "storage",
        "error",
        "png",
        "loneSurrogates",
    ] {
        match &line[key] {
            Value::Null => {}
            Value::String(text) => {
                out.insert(key.to_string(), mask(text));
            }
            other => {
                out.insert(key.to_string(), other.to_string());
            }
        }
    }
    out
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

pub async fn ours(
    cases: &[Value],
    scratch: &Scratch,
    chat: &str,
    which: &str,
    max: u64,
) -> Vec<Value> {
    let budget = budget(scratch, max);
    let any = sandbox(budget.clone()).await;
    let sandbox = any.sandbox();
    let ws = ws(scratch, chat);
    let big = "x".repeat(60000);
    let mut out = Vec::new();
    for case in cases
        .iter()
        .filter(|c| c["chat"].as_str().unwrap_or("main") == which)
    {
        let mut line = serde_json::Map::new();
        line.insert("id".into(), case["id"].clone());
        if case["measure"] == Value::Bool(true) {
            budget.measure(chat).await.unwrap();
        }
        let result: Result<(), String> = match case["op"].as_str() {
            Some("view_image") => sandbox
                .view_image(&ws, case["path"].as_str().unwrap())
                .await
                .map(|png| {
                    line.insert(
                        "png".into(),
                        Value::String(format!("{} bytes, sha256 {}", png.len(), sha256_hex(&png))),
                    );
                })
                .map_err(|e| e.to_string()),
            Some("remove") => {
                let paths: Vec<String> = case["paths"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|p| p.as_str().unwrap().to_string())
                    .collect();
                sandbox
                    .remove_work_files(&ws, &paths)
                    .await
                    .map_err(|e| e.to_string())
            }
            Some("disable_network") => {
                sandbox.disable_network();
                Ok(())
            }
            _ => {
                let command = case["command"]
                    .as_str()
                    .unwrap()
                    .replace("REPLACED_BY_BIG", &big);
                let ms = case["timeout_ms"].as_f64().unwrap_or(10_000.0);
                run(
                    sandbox,
                    &ws,
                    &command,
                    ms,
                    case["network"] == Value::Bool(true),
                )
                .await
                .map(|run| {
                    line.insert("exitCode".into(), run.exit_code.into());
                    line.insert("formatted".into(), run.formatted.into());
                    if let Some(storage) = run.storage {
                        line.insert("storage".into(), storage.into());
                    }
                })
                .map_err(|e| e.to_string())
            }
        };
        if let Err(error) = result {
            line.insert("error".into(), error.into());
        }
        out.push(Value::Object(line));
    }
    sandbox.destroy(&ws).await.unwrap();
    sandbox.shutdown().await;
    out
}

pub fn cell(text: &str) -> String {
    let flat = text.replace('|', "\\|").replace('\n', "⏎");
    if flat.chars().count() > 160 {
        format!("{}…", flat.chars().take(160).collect::<String>())
    } else {
        flat
    }
}
