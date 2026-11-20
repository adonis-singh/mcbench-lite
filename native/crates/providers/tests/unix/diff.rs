//! The Rust side of the differential (`tests/diff/node-driver.js` is the
//! Node side): one case of `cases.json` at a time, the same turn loop, the
//! same output shape.

use std::path::{Path, PathBuf};

use mcbench_providers::harness::claude_code::cwd;
use mcbench_providers::harness::codex_cli::{Launch, SandboxFiles};
use mcbench_providers::harness::{self, session_terms};
use serde_json::{Map, Value, json};
use tokio_util::sync::CancellationToken;

use super::{
    Deltas, Fake, Req, as_json, claude, codex, complete_claude, complete_codex, sleep, tool,
};

pub async fn run(dir: PathBuf, filter: Option<String>) {
    let cases: Vec<Value> =
        serde_json::from_str(&std::fs::read_to_string(dir.join("cases.json")).expect("cases.json"))
            .unwrap();
    let login = dir.join("codex-login");
    assert!(
        login.join("auth.json").exists(),
        "{} has no auth.json",
        login.display()
    );
    // MCBENCH_DIFF_SANDBOXED=1: Codex in its box (Launch::Sandboxed) with
    // the root filesystem and the static agent the sandbox's tests name.
    let sandbox = (std::env::var("MCBENCH_DIFF_SANDBOXED").as_deref() == Ok("1"))
        .then(|| sandbox_files(&dir));
    for case in &cases {
        let name = case["name"].as_str().unwrap();
        if filter.as_ref().is_some_and(|f| !name.contains(f.as_str())) {
            continue;
        }
        run_case(case, &dir.join("rust").join(name), &login, sandbox.clone()).await;
        println!("rust: {name}");
    }
}

#[cfg(target_os = "linux")]
fn sandbox_files(dir: &Path) -> SandboxFiles {
    let var = |key: &str| {
        std::env::var(key)
            .unwrap_or_else(|_| panic!("{key} is needed with MCBENCH_DIFF_SANDBOXED=1"))
    };
    let unpacked = mcbench_sandbox::linux::rootfs::unpack(
        Path::new(&var("MCBENCH_SANDBOX_ROOTFS")),
        &var("MCBENCH_SANDBOX_ROOTFS_SHA256"),
        &dir.join("rootfs-unpacked"),
    )
    .unwrap();
    SandboxFiles {
        rootfs: unpacked.rootfs,
        image_env: unpacked.image.env,
        relay: var("MCBENCH_SANDBOX_AGENT_STATIC").into(),
    }
}

#[cfg(not(target_os = "linux"))]
fn sandbox_files(_: &Path) -> SandboxFiles {
    panic!("MCBENCH_DIFF_SANDBOXED needs Linux")
}

fn tree(root: &Path) -> Map<String, Value> {
    fn walk(root: &Path, relative: &Path, out: &mut Vec<(String, Value)>) {
        let Ok(entries) = std::fs::read_dir(root.join(relative)) else {
            return;
        };
        for entry in entries.flatten() {
            let name = relative.join(entry.file_name());
            let key = name.to_string_lossy().into_owned();
            if key.contains("fake-record") {
                continue;
            }
            let meta = std::fs::symlink_metadata(entry.path()).unwrap();
            if meta.file_type().is_symlink() {
                out.push((
                    key,
                    json!({ "link": std::fs::read_link(entry.path()).unwrap().to_string_lossy() }),
                ));
            } else if meta.is_dir() {
                out.push((key, json!({ "dir": true })));
                walk(root, &name, out);
            } else {
                out.push((key, json!({ "text": String::from_utf8_lossy(&std::fs::read(entry.path()).unwrap()) })));
            }
        }
    }
    let mut out = Vec::new();
    walk(root, Path::new(""), &mut out);
    out.sort_by(|a, b| a.0.encode_utf16().cmp(b.0.encode_utf16()));
    out.into_iter().collect()
}

async fn run_case(case: &Value, dir: &Path, login: &Path, sandbox: Option<SandboxFiles>) {
    let cli = case["cli"].as_str().unwrap();
    let sandbox = sandbox.filter(|_| cli == "codex");
    let _ = std::fs::remove_dir_all(dir);
    let fake = if sandbox.is_some() {
        let fake = Fake::new(&dir.join("install"), cli, &case["script"]);
        std::fs::write(
            dir.join("install/codex-package.json"),
            r#"{"layoutVersion":1,"version":"0.169.0","variant":"codex","entrypoint":"bin/codex"}"#,
        )
        .unwrap();
        fake
    } else {
        Fake::new(dir, cli, &case["script"])
    };
    let records = match &sandbox {
        Some(_) => dir.join("data/harness/c1/codex-home/fake-record/record.jsonl"),
        None => dir.join("bin/record.jsonl"),
    };
    let data = dir.join("data");
    let binary = if case["missing_binary"].as_bool().unwrap_or(false) {
        fake.program.with_file_name("missing")
    } else {
        fake.program.clone()
    };
    let claude = claude(&data, &binary);
    let launch = match sandbox {
        Some(files) => Launch::sandboxed(files),
        None => Launch::Host,
    };
    let codex = codex(&data, &binary, login, Some(launch));
    let harness_name = if cli == "claude" {
        "claude-code"
    } else {
        "codex-cli"
    };
    let first: Vec<&Value> = case["turns"][0]["messages"]
        .as_array()
        .unwrap()
        .iter()
        .collect();
    let terms = session_terms(&first);
    for preset in case["records"].as_array().into_iter().flatten() {
        let text = mcbench_json::stringify(&preset["record"])
            .replace(
                "\"$CWD\"",
                &mcbench_json::stringify(&json!(cwd().to_string_lossy())),
            )
            .replace(
                "\"$INSTRUCTIONS\"",
                &mcbench_json::stringify(&json!(terms.instructions)),
            )
            .replace(
                "\"$RESOLUTION\"",
                &mcbench_json::stringify(&json!(terms.resolution)),
            );
        let Value::Object(record) = mcbench_json::parse(&text).unwrap() else {
            panic!("a record")
        };
        let which = preset["harness"].as_str().unwrap_or(harness_name);
        harness::write_session(&data, preset["conv"].as_str().unwrap(), which, &record).unwrap();
    }
    let mut turns = Vec::new();
    for turn in case["turns"].as_array().unwrap() {
        let mut completions = Vec::new();
        let mut messages = turn["messages"].clone();
        let cancel = CancellationToken::new();
        let conversation = turn["conversation"].as_object().map(|c| {
            (
                c["id"].as_str().unwrap().to_owned(),
                c.get("transcript").cloned(),
            )
        });
        let conversation_id = conversation.as_ref().map(|c| c.0.clone());
        let on = turn["cancel"]["on"].as_str().map(str::to_owned);
        let mut answer = 0;
        loop {
            let deltas = Deltas::default();
            let mut req = Req::new(messages.clone());
            req.conversation = conversation.clone();
            req.tools = turn["tools"]
                .as_array()
                .unwrap()
                .iter()
                .map(|t| tool(t.as_str().unwrap()))
                .collect();
            req.model = turn["model"].as_str().unwrap().to_owned();
            req.effort = turn["effort"]
                .as_str()
                .filter(|e| !e.is_empty())
                .map(str::to_owned);
            req.chat = turn["chat"].as_bool().unwrap();
            let request = req.build();
            // The cancel rules of the Node driver: at the start, or at the
            // first text.
            let sink = deltas.0.clone();
            let watch = cancel.clone();
            let rule = on.clone();
            let mut on_delta = move |d: mcbench_providers::Delta| {
                let value = harness::delta_json(&d);
                let started = value.get("started").is_some();
                let content = value.get("content").is_some();
                sink.lock().unwrap().push(value);
                if (rule.as_deref() == Some("started") && started)
                    || (rule.as_deref() == Some("content") && content)
                {
                    watch.cancel();
                }
            };
            let outcome = if cli == "claude" {
                claude.complete_turn(&request, &mut on_delta, &cancel).await
            } else {
                codex.complete_turn(&request, &mut on_delta, &cancel).await
            };
            let streamed: Vec<Value> = deltas
                .take()
                .iter()
                .map(|d| json!(mcbench_json::stringify(d)))
                .collect();
            let message = match outcome {
                Err(error) => {
                    completions.push(json!({ "deltas": streamed, "error": error.message }));
                    break;
                }
                Ok(message) => message,
            };
            let text = as_json(&message);
            completions
                .push(json!({ "deltas": streamed, "message": mcbench_json::stringify(&text) }));
            if text["tool_calls"].is_null() {
                break;
            }
            if on.as_deref() == Some("tool_call") {
                cancel.cancel();
                completions.push(json!({ "stopped": true }));
                break;
            }
            let given = &turn["answers"][answer];
            answer += 1;
            if given.is_null() {
                completions.push(json!({ "unanswered": true }));
                break;
            }
            let calls = messages.as_array_mut().unwrap();
            calls.push(json!({ "role": "assistant", "content": text["content"], "tool_calls": text["tool_calls"] }));
            calls.push(json!({ "role": "tool", "tool_call_id": text["tool_calls"][0]["id"], "content": given["content"] }));
            if let Some(shots) = given.get("screenshots") {
                calls.push(json!({ "role": "user", "seeBuild": true, "content": shots }));
            }
            if let Some(delay) = given["delay"].as_u64() {
                sleep(delay).await;
            }
        }
        if let Some(id) = &conversation_id {
            claude.close_chat(id);
            codex.close_chat(id);
        }
        turns.push(Value::Array(completions));
    }
    claude.shutdown().await;
    sleep(300).await;
    let fake_lines: Vec<Value> = std::fs::read_to_string(&records)
        .map(|t| t.lines().map(|l| json!(l)).collect())
        .unwrap_or_default();
    let out = json!({ "turns": turns, "harness": tree(&data.join("harness")), "fake": fake_lines });
    std::fs::write(
        dir.join("out.json"),
        mcbench_json::stringify_pretty(&out, 1),
    )
    .unwrap();
    let _ = complete_claude;
    let _ = complete_codex;
}
