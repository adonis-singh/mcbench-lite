//! The Rust side of the replay: each call `tests/node/prepare.js` wrote, run
//! through [`mcbench_tools::Tools::execute`] with the real V8 build child
//! (`mcbench-build-child`, which `MCBENCH_BUILD_CHILD` names), the
//! in-process engine for the
//! settles, the store on a data folder clone, and the same fakes as
//! `tests/node/replay.js`. Writes the same JSON line per call.
//!
//! ```text
//! MCBENCH_TABLES=<core/minecraft/1.21.10> replay <data clone> <calls.jsonl> <out.jsonl>
//! ```

use std::io::Write;
use std::path::PathBuf;

use mcbench_tools::testing::{harness, manifest_of, world_of};
use mcbench_tools::{Chat, Tools, Workspace, call_shown};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let [_, data, calls, out] = &args[..] else {
        panic!("usage: replay <data clone> <calls.jsonl> <out.jsonl>");
    };
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(replay(
        PathBuf::from(data),
        PathBuf::from(calls),
        PathBuf::from(out),
    ));
}

async fn replay(data: PathBuf, calls: PathBuf, out: PathBuf) {
    let tables = PathBuf::from(std::env::var("MCBENCH_TABLES").expect("MCBENCH_TABLES"));
    let harness = harness(&data, &tables).unwrap();
    let tools = Tools::new(harness.services.clone());
    let mut file = std::fs::File::create(&out).unwrap();
    let text = std::fs::read_to_string(&calls).unwrap();
    let mut done = 0;
    for line in text.lines() {
        let call: Value = serde_json::from_str(line).unwrap();
        *harness.record.lock().unwrap() = call["record"].clone();
        *harness.sandbox.exec.lock().unwrap() = None;
        let name = call["name"].as_str().unwrap();
        let arguments = call["arguments"].as_str().unwrap_or("");
        let conversation = call["conversation"].as_str().unwrap().to_string();
        let world = world_of(
            call["before"].as_str().unwrap(),
            &PathBuf::from(call["chunks"].as_str().unwrap()),
        )
        .unwrap();
        let mut chat = Chat {
            id: conversation.clone(),
            model: call["model"].as_str().unwrap_or("").to_string(),
            world,
            workspace: Workspace {
                chat: conversation.clone(),
                host: data.join("sandbox").join(&conversation),
            },
            image_input: call["imageInput"].as_bool().unwrap_or(true),
            chat: call["chat"].as_bool().unwrap_or(false),
        };
        let start = call_shown(name, arguments);
        let parsed = mcbench_json::parse(if arguments.is_empty() {
            "{}"
        } else {
            arguments
        });
        let mut progress = Vec::new();
        let (outcome, threw) = match parsed {
            Ok(Value::Object(args)) => {
                match tools
                    .execute(
                        &mut chat,
                        name,
                        &args,
                        &mut |p| progress.push(p.message),
                        &CancellationToken::new(),
                    )
                    .await
                {
                    Ok(outcome) => (Some(outcome), None),
                    Err(error) => (None, Some(error.0)),
                }
            }
            // The loop refuses arguments that are not an object, in V8's words.
            _ => (None, Some("invalid arguments".to_string())),
        };
        let store = harness.services.store.clone();
        let line = json!({
            "key": call["key"],
            "content": outcome.as_ref().map(|o| o.content(name)),
            "screenshots": outcome.as_ref().and_then(|o| o.screenshots.as_ref()).map(|s| json!({
                "text": s.text, "images": s.images.iter().map(|i| i.url.clone()).collect::<Vec<_>>(),
            })),
            "images": outcome.as_ref().map(|o| o.images.iter().map(|i| i.url.clone()).collect::<Vec<_>>()).unwrap_or_default(),
            "manifest": manifest_of(&chat.world),
            "start": {"summary": start.summary, "program": start.program},
            "result": outcome.as_ref().map(|o| {
                let shown = o.shown(name, |url| store.file_path(url).unwrap_or_default());
                json!({
                    "ok": shown.ok, "cancelled": shown.cancelled, "error": shown.error,
                    "diff": shown.diff.map(|d| json!({"added": d.added, "removed": d.removed, "changed": d.changed})),
                    "output": shown.output,
                    "images": shown.images.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
                })
            }),
            "progress": progress,
            "exec": harness.sandbox.exec.lock().unwrap().clone(),
            "threw": threw,
        });
        writeln!(file, "{}", mcbench_json::stringify(&line)).unwrap();
        done += 1;
        if done % 50 == 0 {
            eprintln!("replay: {done} of {}", text.lines().count());
        }
    }
    eprintln!("replay: {done} calls");
}
