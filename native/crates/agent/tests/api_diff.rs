//! `api_messages` against today's `apiMessages` (`tests/node/api.js`): the
//! request at every assistant message of the active branch of every stored
//! chat, for a model that takes images and a text-only one, under today's
//! budget and under a small one that forces the context archive, byte for
//! byte (by sha256 of the JSON text), and the archive files both wrote.
//! The environment is `tests/common`'s.

mod common;

use std::collections::BTreeMap;

use serde_json::Value;

use mcbench_agent::api::{self, History};
use mcbench_agent::context::{self, Archives};
use mcbench_agent::contract::{self, Form, Placement};
use mcbench_tools::WorkFolders as SandboxWork;
use mcbench_tools::verify::Budgets;

fn run(budget: Option<usize>) -> (usize, Vec<String>) {
    let env = common::env();
    let tag = budget.map_or("default".to_string(), |b| b.to_string());
    let node_data = common::clone(&env, &env.fixture, &format!("api-node-{tag}"));
    let rust_data = common::clone(&env, &env.fixture, &format!("api-rust-{tag}"));
    let out = env.scratch.join(format!("api-node-{tag}.jsonl"));
    let dump = std::env::var("MCBENCH_DIFF_DUMP").unwrap_or_default();
    let dump_file = env.scratch.join(format!("api-node-{tag}.dump.json"));
    let budget_text = budget.map(|b| b.to_string());
    let mut vars = Vec::new();
    if let Some(text) = &budget_text {
        vars.push(("MCBENCH_CONTEXT_MAX_BYTES", text.as_str()));
    }
    common::node(
        &env,
        &node_data,
        "api.js",
        &[out.to_str().unwrap(), &dump, dump_file.to_str().unwrap()],
        &vars,
    );
    let node: BTreeMap<String, Value> = std::fs::read_to_string(&out)
        .unwrap()
        .lines()
        .map(|line| {
            let value: Value = serde_json::from_str(line).unwrap();
            let key = format!(
                "{}:{}:{}",
                value["chat"].as_str().unwrap(),
                value["step"],
                value["variant"].as_str().unwrap()
            );
            (key, value)
        })
        .collect();
    let store = common::open(&rust_data);
    let archives = Archives::default();
    let work = SandboxWork {
        root: rust_data.join("sandbox"),
    };
    let max_bytes = budget.unwrap_or(context::DEFAULT_MAX_BYTES);
    let mut ids: Vec<String> = store
        .list_conversation_ids()
        .unwrap()
        .into_iter()
        .filter(|(_, archived)| !archived)
        .map(|(id, _)| id)
        .collect();
    ids.sort();
    let mut compared = 0;
    let mut differences = Vec::new();
    for id in ids {
        let live = store.conversation(&id).unwrap().unwrap();
        let conversation = live.lock().unwrap();
        let memory = api::in_memory(&conversation.messages).unwrap();
        for (step, message) in conversation.messages.iter().enumerate() {
            if !matches!(message.message, mcbench_store::Message::Assistant(_)) {
                continue;
            }
            for variant in ["image", "text"] {
                let key = format!("{id}:{step}:{variant}");
                let form = Form {
                    placement: Placement::Updates,
                    image_input: variant == "image",
                    network_access: true,
                    chat: true,
                    resolution: conversation.resolution,
                };
                let ours = contract::of(form, &Budgets::DEFAULT).and_then(|c| {
                    api::api_messages(
                        &store,
                        &History {
                            chat: &id,
                            messages: &memory[..step],
                            max_bytes,
                            keep_exchanges: context::KEEP_EXCHANGES,
                        },
                        variant == "image",
                        &c.system_prompt,
                        &archives,
                        &work,
                    )
                });
                let ours = match ours {
                    Ok(messages) => {
                        let text = mcbench_json::stringify(&Value::Array(
                            messages.into_iter().map(|m| Value::Object(m.0)).collect(),
                        ));
                        if key == dump {
                            std::fs::write(
                                env.scratch.join(format!("api-rust-{tag}.dump.json")),
                                &text,
                            )
                            .unwrap();
                        }
                        serde_json::json!({ "sha256": common::sha256(&text), "bytes": text.len() })
                    }
                    Err(error) => serde_json::json!({ "error": error }),
                };
                let theirs = node
                    .get(&key)
                    .unwrap_or_else(|| panic!("Node has no {key}"));
                compared += 1;
                let same = match (theirs.get("sha256"), theirs.get("error")) {
                    (Some(sha), _) => ours.get("sha256") == Some(sha),
                    (_, Some(error)) => ours.get("error") == Some(error),
                    _ => false,
                };
                if !same {
                    differences.push(format!("{key}: node {theirs} rust {ours}"));
                }
            }
        }
    }
    assert_eq!(
        compared,
        node.len(),
        "Node made requests this crate did not"
    );
    let node_files = common::files(&node_data.join("sandbox"));
    let rust_files = common::files(&rust_data.join("sandbox"));
    let node_context: Vec<_> = node_files
        .iter()
        .filter(|(p, _)| p.contains("/context/"))
        .collect();
    let rust_context: Vec<_> = rust_files
        .iter()
        .filter(|(p, _)| p.contains("/context/"))
        .collect();
    if node_context != rust_context {
        differences.push(format!(
            "archive files differ: node {} files, rust {}",
            node_context.len(),
            rust_context.len()
        ));
    }
    println!(
        "budget {tag}: {compared} requests, {} archive files, {} differences",
        rust_context.len(),
        differences.len()
    );
    // The clones go once compared; a failing run's stay to be looked at.
    if differences.is_empty() {
        drop(store);
        for path in [&node_data, &rust_data] {
            std::fs::remove_dir_all(path).unwrap();
        }
        std::fs::remove_file(&out).unwrap();
    }
    (compared, differences)
}

#[test]
#[ignore = "needs MCBENCH_DIFF_DATA, MCBENCH_NODE_DIFF and MCBENCH_DIFF_SCRATCH (tests/common)"]
fn api_messages_are_todays_for_every_stored_request() {
    let mut all = Vec::new();
    for budget in [None, Some(16_000), Some(60_000)] {
        let (_, differences) = run(budget);
        all.extend(differences);
    }
    for line in all.iter().take(20) {
        eprintln!("{line}");
    }
    assert!(all.is_empty(), "{} differences", all.len());
}
