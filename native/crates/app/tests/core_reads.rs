//! The reading routes' bodies against today's (`tests/node/reads.js`): for
//! every chat and bench of a fixture clone and the folder's lists, the JSON
//! body today's `handleApi` answers and the core's method's JSON
//! (`ConversationList::to_json`, `Transcript::to_json`, `Core::bench`,
//! `archive`, `leaderboard`, `prompts`, `busy`, `config`, `settings`,
//! `search`), each side on its own clone with the same clock and ids, both
//! started as their app starts.
//!
//! Needs `MCBENCH_DIFF_DATA`, `MCBENCH_NODE_DIFF`, `MCBENCH_DIFF_SCRATCH`
//! (`mcbench-agent`'s `tests/common`), and, so neither side reads anyone's
//! sign-in or keys, `HOME`, `CLAUDE_CONFIG_DIR` and `CODEX_HOME` inside
//! `MCBENCH_DIFF_SCRATCH` and `MCBENCH_CLAUDE_BINARY` and
//! `MCBENCH_CODEX_BINARY` naming no program (the test checks them).

#[path = "../../agent/tests/common/mod.rs"]
mod common;

use std::path::Path;
use std::process::Command as Process;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{Map, Value, json};

use mcbench_agent::lifecycle::Refusal;
use mcbench_lite::core::backend::Backend;
use mcbench_lite::core::{Core, Parts};

struct NoModel;

#[async_trait]
impl Backend for NoModel {
    fn provider(&self, _: &str) -> Result<Arc<dyn mcbench_providers::Provider>, String> {
        Err("no model in the reads".into())
    }
    fn tools(&self) -> mcbench_tools::Tools {
        mcbench_tools::Tools::of(Vec::new())
    }
    fn sync_attachments(&self, _: &str) -> Result<(), String> {
        Ok(())
    }
    async fn reset_sandbox(&self, _: &str) -> Result<(), String> {
        Ok(())
    }
    fn network_access(&self) -> bool {
        true
    }
    async fn shutdown(&self) {}
}

/// Synthetic search words.
const QUERIES: [&str; 6] = ["house", "red stone", "tower", "a", "build the", "zzzz"];

fn answer(result: Result<Value, Refusal>) -> Value {
    match result {
        Ok(body) => json!({ "status": 200, "body": body }),
        Err(refusal) => json!({ "status": refusal.status, "body": { "error": refusal.message } }),
    }
}

#[test]
#[ignore = "needs MCBENCH_DIFF_DATA, MCBENCH_NODE_DIFF and MCBENCH_DIFF_SCRATCH (mcbench-agent's tests/common)"]
fn the_reading_routes_answer_as_todays() {
    let env = common::env();
    for name in ["HOME", "CLAUDE_CONFIG_DIR", "CODEX_HOME"] {
        let value = std::env::var(name).unwrap_or_default();
        assert!(
            Path::new(&value).starts_with(&env.scratch),
            "{name} must be inside MCBENCH_DIFF_SCRATCH, so no sign-in is read"
        );
    }
    for name in ["MCBENCH_CLAUDE_BINARY", "MCBENCH_CODEX_BINARY"] {
        let value = std::env::var(name).unwrap_or_default();
        assert!(
            value.contains('/') && !Path::new(&value).exists(),
            "{name} must name a path that does not exist"
        );
    }
    let node_data = common::clone(&env, &env.fixture, "core-reads-node");
    let rust_data = common::clone(&env, &env.fixture, "core-reads-rust");
    // The library read through the typed `LibraryPrompt`: the clone's own
    // prompts and synthetic ones with every field `clean` reads in the
    // shapes it converts (numbers as text, tags to trim and fold, an
    // attachment's stored type replaced by its data URL's, extra fields
    // dropped), the same bytes on both sides.
    let library = {
        let path = node_data.join("prompts.json");
        let mut prompts: Vec<Value> = match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text).unwrap(),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(e) => panic!("{}: {e}", path.display()),
        };
        prompts.push(json!({ "id": "syn-1", "title": " Ref ", "text": "x", "tags": ["A", "a", 1, " b "],
            "favorite": true, "usedAt": "17",
            "attachments": [{ "name": "ref.png", "type": "text/html", "size": "3", "data": "data:image/png;base64,cG5n", "url": "javascript:alert(1)" },
                            { "name": "notes.txt", "data": "data:text/plain;base64,aGk=" }],
            "addedBy": { "chat": "c", "model": "m", "extra": 1 } }));
        prompts.push(
            json!({ "id": 7, "title": null, "text": 5, "favorite": "yes", "usedAt": 0,
            "attachments": [], "addedBy": { "chat": 1 } }),
        );
        serde_json::to_string(&prompts).unwrap()
    };
    for data in [&node_data, &rust_data] {
        std::fs::write(data.join("prompts.json"), &library).unwrap();
    }
    let queries = env.scratch.join("core-reads-queries.json");
    std::fs::write(&queries, serde_json::to_string(&QUERIES).unwrap()).unwrap();
    let node_out = env.scratch.join("core-reads-node.json");
    let ours_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/node");
    let agent = Path::new(env!("CARGO_MANIFEST_DIR")).join("../agent/tests/node");
    let output = Process::new(std::env::var("MCBENCH_NODE").unwrap_or_else(|_| "node".into()))
        .arg("-r")
        .arg(agent.join("det.js"))
        .arg("-r")
        .arg(ours_dir.join("startup.js"))
        .arg(ours_dir.join("reads.js"))
        .arg(&node_out)
        .arg(&queries)
        .env("MCBENCH_NODE_DIFF", &env.checkout)
        .env("MCBENCH_DATA_DIR", &node_data)
        .env("MCBENCH_SETTINGS_FILE", node_data.join("settings.json"))
        .env("MCBENCH_TOOLS_DIR", env.scratch.join("tools-empty"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "node reads.js: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let node: Map<String, Value> =
        serde_json::from_str(&std::fs::read_to_string(&node_out).unwrap()).unwrap();

    let core = Core::start(Parts {
        data: rust_data.clone(),
        program: std::env::current_exe().unwrap(),
        host: common::DetHost::new(common::DET_NOW),
        shipped: Box::new(|| Err("unused".into())),
        client: mcbench_setup::fetch::client(),
        backend: Some(Arc::new(NoModel)),
        installer: None,
        share: None,
        live: Default::default(),
    })
    .unwrap();
    let handle = core.handle().clone();
    handle.block_on(core.started());
    let ours: Map<String, Value> = handle
        .block_on({
            let core = core.clone();
            handle.spawn(async move {
                let mut out = Map::new();
                let list = core.conversations().await.map(|list| list.to_json());
                out.insert("/api/conversations/stream".into(), answer(list));
                let store = Arc::clone(core.store());
                for (id, _) in store.list_conversation_ids().unwrap() {
                    let got = core.conversation(&id).await.and_then(|t| {
                        t.to_json().map_err(|e| Refusal {
                            status: 500,
                            message: e,
                        })
                    });
                    out.insert(format!("/api/conversations/{id}"), answer(got));
                }
                for (id, _) in store.list_bench_ids().unwrap() {
                    out.insert(
                        format!("/api/benches/{id}"),
                        answer(core.bench(&id).await.map(|details| details.to_json())),
                    );
                }
                out.insert("/api/archive".into(), answer(core.archive().await));
                out.insert("/api/leaderboard".into(), answer(core.leaderboard().await));
                out.insert(
                    "/api/prompts".into(),
                    answer(core.prompts().await.map(|prompts| {
                        let typed: Vec<Value> = prompts.iter().map(|p| p.to_json()).collect();
                        json!({ "prompts": typed })
                    })),
                );
                out.insert(
                    "/api/activity".into(),
                    answer(core.busy().await.map(|busy| json!({ "busy": busy }))),
                );
                out.insert("/api/config".into(), answer(core.config().await));
                out.insert("/api/settings".into(), answer(core.settings(false).await));
                for q in QUERIES {
                    for scope in ["build", "bench"] {
                        let route =
                            format!("/api/search?scope={scope}&q={}", q.replace(' ', "%20"));
                        out.insert(route, answer(core.search(scope, q).await));
                    }
                }
                out
            })
        })
        .unwrap();
    core.shutdown();
    // `provider-login.test.js`: Settings show no terminal command for a
    // sign-in.
    for (id, entry) in ours["/api/settings"]["body"]["providers"]
        .as_object()
        .unwrap()
    {
        assert!(entry.get("login").is_none(), "{id} shows a login command");
    }

    // The one allowed difference: Cursor is not in the native app, so its
    // Settings entry has no SDK version and no sign-in (today's reads
    // `@cursor/sdk`'s). It is compared without those two fields.
    let mut node = node;
    let mut ours = ours;
    for side in [&mut node, &mut ours] {
        if let Some(Value::Object(cursor)) = side
            .get_mut("/api/settings")
            .and_then(|s| s.pointer_mut("/body/providers/cursor"))
        {
            cursor.shift_remove("version");
            cursor.shift_remove("signIn");
        }
    }
    let mut differences = Vec::new();
    let mut same = 0;
    for (route, theirs) in &node {
        match ours.get(route) {
            Some(mine) if mine == theirs => same += 1,
            Some(mine) => {
                let (a, b) = (theirs.to_string(), mine.to_string());
                let at = a.chars().zip(b.chars()).take_while(|(x, y)| x == y).count();
                let cut = |s: &str| {
                    s.chars()
                        .skip(at.saturating_sub(80))
                        .take(240)
                        .collect::<String>()
                };
                differences.push(format!("{route}:\n  node {}\n  rust {}", cut(&a), cut(&b)));
            }
            None => differences.push(format!("{route}: only node answered it")),
        }
    }
    for route in ours.keys().filter(|r| !node.contains_key(*r)) {
        differences.push(format!("{route}: only rust answered it"));
    }
    println!(
        "core reads: {} routes, {same} identical, {} differences",
        node.len(),
        differences.len()
    );
    for line in &differences {
        eprintln!("{line}\n");
    }
    assert!(differences.is_empty(), "{} differences", differences.len());
    for path in [&node_data, &rust_data] {
        std::fs::remove_dir_all(path).unwrap();
    }
    for file in [&queries, &node_out] {
        std::fs::remove_file(file).unwrap();
    }
}
