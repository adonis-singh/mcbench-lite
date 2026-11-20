//! Settings against today's routes (`tests/node/settings.js`): on two
//! copies of a fixture's database and prompt library, each with the made-up
//! `settings.json` of `fixture::MADE_UP_SETTINGS` (`light_clone`; a
//! fixture's own settings file is never read; the copies are deleted after
//! the run, and the files are compared by their sha256 only), the same
//! Settings session through
//! today's `handleApi` and through the core's commands: every field of
//! `SettingsView` against `GET /api/settings` (the summaries byte for
//! byte), the stored file after each `SaveSettings` against the one after
//! the page's `PUT /api/settings` (a title model, an enabled flag, hidden
//! models and a model order changed, keys left empty, a key typed, the
//! share repository), each refusal's sentence, and the vote count before
//! and after `ClearVotes` (`GET /api/leaderboard`, `DELETE /api/votes`).
//! Only equal or not is printed, with the field's name.
//!
//! Needs `MCBENCH_DIFF_DATA`, `MCBENCH_NODE_DIFF`, `MCBENCH_DIFF_SCRATCH`
//! (`mcbench-agent`'s `tests/common`), and, as `core_reads.rs`, `HOME`,
//! `CLAUDE_CONFIG_DIR` and `CODEX_HOME` inside `MCBENCH_DIFF_SCRATCH` and
//! `MCBENCH_CLAUDE_BINARY` and `MCBENCH_CODEX_BINARY` naming no program.

#[path = "../../agent/tests/common/mod.rs"]
mod common;

use std::path::Path;
use std::process::Command as Process;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde_json::{Map, Value, json};

use mcbench_lite::core::backend::Backend;
use mcbench_lite::core::{Core, Parts};
use mcbench_ui::{
    Auth, Command, KeySource, ProviderChange, SettingsChange, SettingsView, TitleModel, Update,
    Window,
};

struct NoModel;

#[async_trait]
impl Backend for NoModel {
    fn provider(&self, _: &str) -> Result<Arc<dyn mcbench_providers::Provider>, String> {
        Err("no model in the settings".into())
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

#[derive(Default)]
struct Recording(Mutex<Vec<Update>>);

impl Window for Recording {
    fn publish(&self, update: Update) {
        self.0.lock().unwrap().push(update);
    }
}

/// The edits of each save, in `settings.js`'s shape; `None` is a read.
fn edits() -> Vec<Value> {
    vec![
        json!({
            "titleModel": { "provider": "openrouter", "model": "openai/gpt-5", "effort": "" },
            "providers": {
                "groq": { "enabled": false },
                "openrouter": { "hiddenModels": ["x/a", "x/b", "x/a", ""], "modelOrder": ["x/b", "x/a"] },
            },
        }),
        json!({
            "providers": {
                "groq": { "enabled": true, "apiKey": "  synthetic-groq-key  " },
                "anthropic": { "hiddenModels": ["claude-x"] },
            },
        }),
        json!({ "titleModel": { "provider": "nobody", "model": "x", "effort": "" } }),
        json!({ "titleModel": null, "shareRepo": "owner/name" }),
        json!({ "shareRepo": "not a repository" }),
    ]
}

/// `edits` applied to the draft of `view` (`change_of`: every key empty).
fn change(view: &SettingsView, edits: &Value) -> SettingsChange {
    let mut change = SettingsChange {
        title_model: view.title_model.clone(),
        share_repo: view.share_repo.clone(),
        providers: view
            .providers
            .iter()
            .map(|p| ProviderChange {
                id: p.id.clone(),
                enabled: p.enabled,
                api_key: String::new(),
                hidden_models: p.hidden_models.clone(),
                model_order: p.model_order.clone(),
            })
            .collect(),
    };
    if let Some(title) = edits.get("titleModel") {
        change.title_model = (!title.is_null()).then(|| TitleModel {
            provider: title["provider"].as_str().unwrap().into(),
            model: title["model"].as_str().unwrap().into(),
            effort: title["effort"].as_str().unwrap().into(),
        });
    }
    if let Some(repo) = edits.get("shareRepo") {
        change.share_repo = repo.as_str().map(str::to_string);
    }
    let texts = |v: &Value| -> Vec<String> {
        v.as_array()
            .unwrap()
            .iter()
            .map(|s| s.as_str().unwrap().to_string())
            .collect()
    };
    for (id, fields) in edits
        .get("providers")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
    {
        let p = change.providers.iter_mut().find(|p| &p.id == id).unwrap();
        if let Some(on) = fields.get("enabled") {
            p.enabled = on.as_bool().unwrap();
        }
        if let Some(key) = fields.get("apiKey") {
            p.api_key = key.as_str().unwrap().into();
        }
        if let Some(list) = fields.get("hiddenModels") {
            p.hidden_models = texts(list);
        }
        if let Some(list) = fields.get("modelOrder") {
            p.model_order = texts(list);
        }
    }
    change
}

/// The view in today's body's names, with the presence rules of
/// `settings.js` `details`, for the fields the view carries.
fn render(view: &SettingsView) -> Value {
    let mut providers = Map::new();
    for p in &view.providers {
        let mut entry = Map::new();
        entry.insert("label".into(), p.label.clone().into());
        entry.insert("connected".into(), p.connected.into());
        if let Some(env) = &p.env_key {
            entry.insert("envKey".into(), env.clone().into());
        }
        let cli = p.auth == Auth::CliLogin;
        entry.insert(
            "auth".into(),
            if cli { "cli-login" } else { "api-key" }.into(),
        );
        entry.insert("harness".into(), p.harness.into());
        entry.insert("enabled".into(), p.enabled.into());
        entry.insert("hiddenModels".into(), p.hidden_models.clone().into());
        entry.insert("modelOrder".into(), p.model_order.clone().into());
        if p.env_key.is_some() {
            entry.insert(
                "keySource".into(),
                match p.key_source {
                    Some(KeySource::Saved) => "saved".into(),
                    Some(KeySource::Env) => "env".into(),
                    None => Value::Null,
                },
            );
        }
        if let Some(r) = &p.runtime {
            entry.insert(
                "runtime".into(),
                json!({ "binary": r.binary, "binaryEnv": r.binary_env, "homeEnv": r.home_env,
                        "binaryValue": r.binary_value.clone().unwrap_or_default(), "resolvedHome": r.resolved_home }),
            );
        }
        if cli {
            entry.insert("signedIn".into(), p.signed_in.into());
        }
        if ["codex", "codex-cli", "claude-code"].contains(&p.id.as_str()) {
            entry.insert("account".into(), p.account.clone().into());
            entry.insert("plan".into(), p.plan.clone().into());
            entry.insert("version".into(), p.version.clone().into());
        }
        if let Some(offer) = &p.sign_in {
            let methods: Value = if offer.methods.is_empty() {
                Value::Null
            } else {
                offer.methods.clone().into()
            };
            let bytes: Value = if offer.program.bytes == 0 {
                Value::Null
            } else {
                offer.program.bytes.into()
            };
            entry.insert(
                "signIn".into(),
                json!({
                    "methods": methods,
                    "program": { "name": offer.program.name, "title": offer.program.title, "bytes": bytes },
                    "current": offer.current.as_ref().map_or(Value::Null, |v| v.to_json()),
                    "installed": offer.installed,
                }),
            );
        }
        entry.insert("summary".into(), p.summary.clone().into());
        entry.insert("endpoint".into(), p.endpoint.clone().into());
        entry.insert("protocol".into(), p.protocol.clone().into());
        providers.insert(p.id.clone(), Value::Object(entry));
    }
    json!({
        "titleModel": view.title_model.as_ref().map(|t| json!({ "provider": t.provider, "model": t.model, "effort": t.effort })),
        "shareRepo": view.share_repo,
        "providers": providers,
        "checkedAt": mcbench_store::iso(view.checked_at),
    })
}

/// Today's body without what the view does not carry: Cursor (not in the
/// native app), the starred models (`Update::Favorites` carries them) and
/// each runtime's `home` (its default, which the window does not show).
fn comparable(body: &Value) -> Value {
    let mut body = body.clone();
    let object = body.as_object_mut().unwrap();
    object.shift_remove("modelFavorites");
    let providers = object["providers"].as_object_mut().unwrap();
    providers.shift_remove("cursor");
    for entry in providers.values_mut() {
        if let Some(Value::Object(runtime)) = entry.get_mut("runtime") {
            runtime.shift_remove("home");
        }
    }
    body
}

/// Each field that differs, by name; and how many were compared.
fn differences(at: &str, node: &Value, ours: &Value, out: &mut Vec<String>, compared: &mut usize) {
    let (Value::Object(n), Value::Object(o)) = (node, ours) else {
        *compared += 1;
        if node != ours {
            out.push(at.to_string());
        }
        return;
    };
    for key in n.keys().chain(o.keys().filter(|k| !n.contains_key(*k))) {
        let field = format!("{at}.{key}");
        match (n.get(key), o.get(key)) {
            (Some(a), Some(b)) if key == "providers" || at.ends_with(".providers") => {
                differences(&field, a, b, out, compared)
            }
            (Some(a), Some(b)) => {
                *compared += 1;
                if a != b {
                    out.push(field);
                }
            }
            (Some(_), None) => out.push(format!("{field} (only node)")),
            (None, _) => out.push(format!("{field} (only rust)")),
        }
    }
}

/// What Settings and the votes read of the fixture, copied file to file:
/// the database and the prompt library; the worlds, sandboxes and
/// Minecraft's files are not read by these routes, and leaving them out
/// keeps the two clones small. The settings file is the made-up one.
fn light_clone(env: &common::Env, name: &str) -> std::path::PathBuf {
    let to = env.scratch.join(name);
    if to.exists() {
        std::fs::remove_dir_all(&to).unwrap();
    }
    std::fs::create_dir_all(&to).unwrap();
    common::fixture::write_made_up_settings(&to);
    for file in [
        "prompts.json",
        "mcbench.sqlite3",
        "mcbench.sqlite3-wal",
        "mcbench.sqlite3-shm",
    ] {
        let from = env.fixture.join(file);
        if from.exists() {
            std::fs::copy(&from, to.join(file)).unwrap();
        }
    }
    to
}

#[test]
#[ignore = "needs MCBENCH_DIFF_DATA, MCBENCH_NODE_DIFF and MCBENCH_DIFF_SCRATCH (mcbench-agent's tests/common)"]
fn settings_save_and_votes_answer_as_todays() {
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
    let node_data = light_clone(&env, "core-settings-node");
    let rust_data = light_clone(&env, "core-settings-rust");

    // Today's: a read, then each save and a read, then the votes.
    let mut ops = vec![json!({ "get": "/api/settings" })];
    for edit in edits() {
        ops.push(json!({ "put": edit }));
        ops.push(json!({ "get": "/api/settings" }));
    }
    ops.push(json!({ "get": "/api/leaderboard" }));
    ops.push(json!({ "del": "/api/votes" }));
    ops.push(json!({ "get": "/api/leaderboard" }));
    let ops_file = env.scratch.join("core-settings-ops.json");
    std::fs::write(&ops_file, serde_json::to_string(&ops).unwrap()).unwrap();
    let node_out = env.scratch.join("core-settings-node.json");
    let ours_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/node");
    let agent = Path::new(env!("CARGO_MANIFEST_DIR")).join("../agent/tests/node");
    let output = Process::new(std::env::var("MCBENCH_NODE").unwrap_or_else(|_| "node".into()))
        .arg("-r")
        .arg(agent.join("det.js"))
        .arg("-r")
        .arg(ours_dir.join("startup.js"))
        .arg(ours_dir.join("settings.js"))
        .arg(&ops_file)
        .arg(&node_out)
        .env("MCBENCH_NODE_DIFF", &env.checkout)
        .env("MCBENCH_DATA_DIR", &node_data)
        .env("MCBENCH_SETTINGS_FILE", node_data.join("settings.json"))
        .env("MCBENCH_TOOLS_DIR", env.scratch.join("tools-empty"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "node settings.js failed (its output is not shown: it may name a key)"
    );
    let node: Vec<Value> =
        serde_json::from_str(&std::fs::read_to_string(&node_out).unwrap()).unwrap();

    // Ours: the same through the commands.
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
    let window = Arc::new(Recording::default());
    let handle = core.handle().clone();
    handle.block_on(core.started());
    mcbench_ui::Core::attach(&core, window.clone());
    let run = |command: Command| -> Vec<Update> {
        let from = window.0.lock().unwrap().len();
        let core = core.clone();
        handle
            .block_on(handle.spawn(async move { core.command(command).await }))
            .unwrap();
        window.0.lock().unwrap()[from..].to_vec()
    };
    let read = |updates: Vec<Update>| -> SettingsView {
        updates
            .into_iter()
            .find_map(|u| match u {
                Update::Settings(Ok(view)) => Some(view),
                _ => None,
            })
            .expect("a settings view")
    };
    let file_sha = || {
        use sha2::Digest;
        let bytes = std::fs::read(rust_data.join("settings.json")).unwrap();
        sha2::Sha256::digest(&bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    };

    let mut differ = Vec::new();
    let mut compared = 0;
    let mut views = 0;
    let mut files_same = 0;
    let mut refusals_same = 0;
    let mut view = read(run(Command::ReadSettings { refresh: false }));
    // The made-up file as the first read shows it: three saved keys, one
    // empty, and its title model (by field name only, never a value).
    let key = |id: &str| {
        view.providers
            .iter()
            .find(|p| p.id == id)
            .unwrap()
            .key_source
    };
    for id in ["openrouter", "openai", "groq"] {
        assert!(
            key(id) == Some(KeySource::Saved),
            "providers.{id}.apiKey of the made-up settings is not a saved key"
        );
    }
    assert!(
        key("anthropic") != Some(KeySource::Saved),
        "providers.anthropic.apiKey of the made-up settings is empty, not saved"
    );
    assert!(
        view.title_model
            == Some(TitleModel {
                provider: "openrouter".into(),
                model: "made-up/title".into(),
                effort: "low".into(),
            }),
        "titleModel is not the made-up settings'"
    );
    let mut at = 0;
    let mut compare_view = |view: &SettingsView, at: usize, differ: &mut Vec<String>| {
        assert_eq!(view.settings_file, rust_data.join("settings.json"));
        differences(
            &format!("read {at}"),
            &comparable(&node[at]["body"]),
            &render(view),
            differ,
            &mut compared,
        );
        views += 1;
    };
    compare_view(&view, at, &mut differ);
    for edit in edits() {
        at += 1;
        let saved = run(Command::SaveSettings(change(&view, &edit)));
        let theirs = &node[at];
        // The list's own task may publish after the save's end: the save's
        // answer is its one `SettingsSaved`.
        let answer = saved.iter().find(|u| matches!(u, Update::SettingsSaved(_)));
        match answer {
            Some(Update::SettingsSaved(Ok(()))) => {
                if theirs["status"] != 200 {
                    differ.push(format!("save {at}: node refused it, rust saved"));
                }
            }
            Some(Update::SettingsSaved(Err(error))) => {
                if theirs["body"]["error"] == error.as_str() {
                    refusals_same += 1;
                } else {
                    differ.push(format!("save {at}: the refusal's sentence"));
                }
            }
            _ => differ.push(format!("save {at}: no SettingsSaved")),
        }
        if theirs["settings"] == file_sha() {
            files_same += 1;
        } else {
            differ.push(format!("save {at}: settings.json's bytes"));
        }
        at += 1;
        view = read(run(Command::ReadSettings { refresh: false }));
        compare_view(&view, at, &mut differ);
    }
    let votes = |updates: Vec<Update>| -> u64 {
        match updates.last() {
            Some(Update::Leaderboard(Ok(board))) => board.votes,
            other => panic!("no vote count: {other:?}"),
        }
    };
    let before = votes(run(Command::ReadLeaderboard));
    let cleared = votes(run(Command::ClearVotes));
    let after = votes(run(Command::ReadLeaderboard));
    let mut votes_same = 0;
    for (value, theirs) in [(before, &node[at + 1]), (after, &node[at + 3])] {
        if theirs["body"]["votes"] == value {
            votes_same += 1;
        } else {
            differ.push(format!("leaderboard votes after op {at}"));
        }
    }
    if node[at + 2]["status"] != 200 || cleared != after {
        differ.push("clear votes".into());
    }
    core.shutdown();
    println!(
        "core settings: {views} views ({compared} fields), {files_same} of {} saved files, \
         {refusals_same} refusals, {votes_same} of 2 vote counts ({before} before, {after} after) \
         identical; {} differences",
        edits().len(),
        differ.len()
    );
    for line in &differ {
        eprintln!("differs: {line}");
    }
    assert!(before > 0, "the fixture has votes to clear");
    for path in [&node_data, &rust_data] {
        std::fs::remove_dir_all(path).unwrap();
    }
    for file in [&ops_file, &node_out] {
        std::fs::remove_file(file).unwrap();
    }
    assert!(differ.is_empty(), "{} differences", differ.len());
}
