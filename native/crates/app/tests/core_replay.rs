//! The core's chat route replayed against today's (`mcbench-agent`'s
//! `tests/replay_diff.rs` and its Node driver `tests/node/replay.js`, run
//! unchanged): every case through today's `handleApi` chat route on one
//! clone, and through [`mcbench_lite::core::Core`] (`Core::chat`, its
//! events taken from what it publishes to a recording `Window` as
//! `Update::Turn`) on another, with the same replay model and tools, clock
//! and ids. Both sides start as their app starts: Node's index.js purges
//! the archive at load, and `tests/node/startup.js` then ends the turns the
//! last run left and advances every bench (`startHttpServer`), as
//! `Core::start` does. Compared: each case's events as NDJSON lines
//! (through `TurnEvent::to_json`), the requests the model was given,
//! refusals, every row of both databases and the files beside them.
//!
//! Needs `MCBENCH_DIFF_DATA`, `MCBENCH_NODE_DIFF`, `MCBENCH_DIFF_SCRATCH`
//! (`mcbench-agent`'s `tests/common`); `MCBENCH_REPLAY_TOOLS=real` with
//! `MCBENCH_REPLAY_TOOLS_DIR`, `MCBENCH_TABLES`, `MCBENCH_ENGINE_PROGRAM` and
//! `MCBENCH_BUILD_CHILD` (the built `mcbench-build-child`) runs the real
//! tools on both sides but `bash` and `see_build`.

#[path = "../../agent/tests/common/mod.rs"]
mod common;

use std::path::{Path, PathBuf};
use std::process::Command as Process;
use std::sync::{Arc, Mutex, OnceLock};

use async_trait::async_trait;
use serde_json::{Map, Value, json};
use tokio_util::sync::CancellationToken;

use mcbench_agent::lifecycle::{ChatMessage, Outgoing, Turns};
use mcbench_lite::core::backend::Backend;
use mcbench_lite::core::{Core, Parts};
use mcbench_providers::retry;
use mcbench_providers::{
    AssistantMessage, CompletionRequest, Delta, ModelFacts, ModelInfo, Outcome, Provider,
    ProviderError, ProviderId, Telemetry, ToolCall, Usage,
};
use mcbench_store::NewConversation;
use mcbench_tools::{Chat, ImageRef, Progress, Screenshots, Tool, ToolOutcome};
use mcbench_ui::{Update, Window};

#[derive(Default)]
struct Current {
    steps: Vec<Value>,
    tools: Vec<Value>,
    requests: Vec<Value>,
    image_input: bool,
    chat: String,
}

#[derive(Default)]
struct Replay {
    current: Mutex<Current>,
    turns: OnceLock<Turns>,
}

fn telemetry(value: &Value) -> Telemetry {
    let number = |key: &str| value.get(key).and_then(Value::as_f64);
    let failed = value.get("failed") == Some(&Value::Bool(true));
    Telemetry {
        provider: serde_json::from_value::<ProviderId>(value["provider"].clone()).unwrap(),
        model: value["model"].as_str().unwrap().into(),
        effort: value["effort"].as_str().map(str::to_string),
        started_at: value["startedAt"].as_str().unwrap().into(),
        wall_ms: value["wallMs"].as_u64().unwrap(),
        first_delta_ms: value["firstDeltaMs"].as_u64(),
        context_bytes: value["contextBytes"].as_u64().unwrap(),
        message_count: value["messageCount"].as_u64().unwrap(),
        usage: match &value["usage"] {
            Value::Object(u) => {
                let n = |k: &str| u.get(k).and_then(Value::as_f64);
                Some(Usage {
                    input_tokens: n("inputTokens"),
                    output_tokens: n("outputTokens"),
                    cached_input_tokens: n("cachedInputTokens"),
                    cache_write_tokens: n("cacheWriteTokens"),
                    reasoning_tokens: n("reasoningTokens"),
                })
            }
            _ => None,
        },
        outcome: if failed {
            Outcome::Failed {
                cancelled: value["cancelled"] == Value::Bool(true),
            }
        } else {
            Outcome::Completed {
                context_tokens: number("contextTokens"),
            }
        },
    }
}

fn assistant(message: &Value) -> AssistantMessage {
    let string = |key: &str| message.get(key).and_then(Value::as_str).map(str::to_string);
    AssistantMessage {
        content: string("content").unwrap_or_default(),
        reasoning: string("reasoning"),
        reasoning_details: message
            .get("reasoning_details")
            .and_then(Value::as_array)
            .cloned(),
        finish_reason: string("finish_reason"),
        telemetry: telemetry(&message["telemetry"]),
        notices: message.get("notices").and_then(Value::as_array).cloned(),
        plan: message.get("plan").cloned(),
        rate_limit: message.get("rateLimit").cloned(),
        tool_calls: message
            .get("tool_calls")
            .and_then(Value::as_array)
            .map(|calls| {
                calls
                    .iter()
                    .map(|call| ToolCall {
                        id: call["id"].as_str().unwrap().into(),
                        kind: call["type"].as_str().unwrap_or("function").into(),
                        name: call["function"]["name"].as_str().unwrap().into(),
                        arguments: call["function"]["arguments"].as_str().unwrap().into(),
                        extra_content: call.get("extra_content").cloned(),
                    })
                    .collect()
            })
            .unwrap_or_default(),
    }
}

fn joined(texts: &[mcbench_providers::ToolSchema]) -> String {
    format!(
        "[{}]",
        texts
            .iter()
            .map(|t| t.0.as_str())
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn empty_telemetry() -> Value {
    json!({ "provider": "openrouter", "model": "", "effort": null, "startedAt": "", "wallMs": 0,
        "firstDeltaMs": null, "contextBytes": 0, "messageCount": 0, "usage": null, "contextTokens": null })
}

#[async_trait]
impl Provider for Replay {
    fn id(&self) -> ProviderId {
        ProviderId::OpenRouter
    }
    async fn complete(
        &self,
        request: &CompletionRequest,
        on_delta: &mut (dyn FnMut(Delta) + Send),
        _: &CancellationToken,
    ) -> Result<AssistantMessage, ProviderError> {
        let messages = mcbench_json::stringify(&Value::Array(
            request
                .messages
                .iter()
                .map(|m| Value::Object(m.0.clone()))
                .collect(),
        ));
        let step = {
            let mut current = self.current.lock().unwrap();
            current.requests.push(json!({
                "conversationId": request.conversation_id,
                "model": request.model,
                "effort": request.effort,
                "chat": request.chat,
                "tools": common::sha256(&joined(&request.tools)),
                "messages": common::sha256(&wall_free(&messages)),
                "bytes": wall_free(&messages).len(),
            }));
            if request.tools.is_empty() {
                None
            } else if current.steps.is_empty() {
                Some(
                    json!({ "kind": "message", "message": { "content": "", "telemetry": empty_telemetry() } }),
                )
            } else {
                Some(current.steps.remove(0))
            }
        };
        let Some(step) = step else {
            let chat = self.current.lock().unwrap().chat.clone();
            self.turns.get().unwrap().free(&chat).await;
            return Ok(AssistantMessage {
                content: "A title".into(),
                ..assistant(&json!({ "telemetry": empty_telemetry() }))
            });
        };
        match step["kind"].as_str().unwrap() {
            "message" => {
                let reply = assistant(&step["message"]);
                if !reply.content.is_empty() {
                    on_delta(Delta::Content(reply.content.clone()));
                }
                if let Some(reasoning) = reply.reasoning.as_ref().filter(|r| !r.is_empty()) {
                    on_delta(Delta::Reasoning(reasoning.clone()));
                }
                for (index, call) in reply.tool_calls.iter().enumerate() {
                    on_delta(Delta::ToolCall {
                        index: index as u32,
                        id: call.id.clone(),
                        name: call.name.clone(),
                        arguments: call.arguments.clone(),
                    });
                }
                Ok(reply)
            }
            "error" => {
                let message = step["message"].as_str().unwrap().to_string();
                Err(ProviderError {
                    retryable: retry::is_transient(&message, None),
                    message,
                    telemetry: (!step["telemetry"].is_null())
                        .then(|| Box::new(telemetry(&step["telemetry"]))),
                    reply: String::new(),
                })
            }
            _ => {
                let reply = step["reply"].as_str().unwrap_or("").to_string();
                if !reply.is_empty() {
                    on_delta(Delta::Content(reply.clone()));
                }
                let chat = self.current.lock().unwrap().chat.clone();
                self.turns.get().unwrap().stop(&chat);
                Err(ProviderError {
                    message: retry::ABORTED.into(),
                    retryable: false,
                    telemetry: (!step["telemetry"].is_null())
                        .then(|| Box::new(telemetry(&step["telemetry"]))),
                    reply,
                })
            }
        }
    }
    async fn models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        unimplemented!("the replay lists no models")
    }
    async fn model_info(&self, _: &str) -> Result<ModelFacts, ProviderError> {
        Ok(ModelFacts {
            image_input: Some(self.current.lock().unwrap().image_input),
            ..ModelFacts::unknown()
        })
    }
    async fn close_session(&self, _: &str) {}
}

struct ReplayTool {
    name: &'static str,
    replay: Arc<Replay>,
}

#[async_trait]
impl Tool for ReplayTool {
    fn name(&self) -> &'static str {
        self.name
    }
    async fn call(
        &self,
        _: &mut Chat,
        _: &Map<String, Value>,
        progress: &mut (dyn FnMut(Progress) + Send),
        _: &CancellationToken,
    ) -> Result<ToolOutcome, mcbench_tools::ToolError> {
        let entry = {
            let mut current = self.replay.current.lock().unwrap();
            assert!(
                !current.tools.is_empty(),
                "{} was called where the case has no call",
                self.name
            );
            current.tools.remove(0)
        };
        assert_eq!(
            entry["name"], self.name,
            "the case's calls are in another order"
        );
        let urls = |list: &Value| -> Vec<ImageRef> {
            list.as_array()
                .unwrap()
                .iter()
                .map(|u| ImageRef {
                    url: u.as_str().unwrap().into(),
                })
                .collect()
        };
        let mut outcome = ToolOutcome {
            result: entry["result"].clone(),
            screenshots: None,
            images: Vec::new(),
        };
        let ok = entry["result"]["ok"] == Value::Bool(true);
        match self.name {
            "test_build" if entry["progress"] == Value::Bool(true) => {
                progress(Progress {
                    message: "Test 1: replayed".into(),
                    raw: json!({ "phase": "replay", "message": "Test 1: replayed" })
                        .as_object()
                        .unwrap()
                        .clone(),
                });
            }
            "see_build" if ok => {
                if let Some(shots) = entry.get("screenshots") {
                    outcome.screenshots = Some(Screenshots {
                        text: shots["text"].as_str().unwrap().into(),
                        images: urls(&shots["images"]),
                    });
                } else {
                    outcome.images = urls(&entry["images"]);
                }
            }
            "view_image" if ok => {
                let shots = &entry["screenshots"];
                outcome.result =
                    json!({ "ok": true, "note": "image attached in the next message" });
                outcome.screenshots = Some(Screenshots {
                    text: shots["text"].as_str().unwrap().into(),
                    images: urls(&shots["images"]),
                });
            }
            "prompt_library" => {
                outcome.result = match entry.get("error") {
                    Some(error) => json!({ "ok": false, "error": error }),
                    None => json!({ "ok": true, "prompts": entry["result"]["prompts"] }),
                };
            }
            _ => {}
        }
        Ok(outcome)
    }
}

fn real_tools() -> bool {
    std::env::var("MCBENCH_REPLAY_TOOLS").as_deref() == Ok("real")
}

fn stubbed(real: bool) -> &'static [&'static str] {
    if real {
        &["see_build", "bash"]
    } else {
        &[
            "build",
            "test_build",
            "see_build",
            "bash",
            "view_image",
            "prompt_library",
        ]
    }
}

fn wall_free(text: &str) -> String {
    if !real_tools() {
        return text.to_string();
    }
    static WALL: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r#"(Ms|reportBytes)((?:\\)*)":(\s*)-?[0-9][0-9.eE+-]*"#).unwrap()
    });
    WALL.replace_all(text, "${1}${2}\":${3}0").into_owned()
}

fn wall_free_value(value: &Value) -> Value {
    if !real_tools() {
        return value.clone();
    }
    serde_json::from_str(&wall_free(&value.to_string())).unwrap()
}

/// The replay as the core's backend.
struct ReplayBackend {
    replay: Arc<Replay>,
    real: Option<Arc<mcbench_tools::Services>>,
}

#[async_trait]
impl Backend for ReplayBackend {
    fn provider(&self, _: &str) -> Result<Arc<dyn Provider>, String> {
        Ok(self.replay.clone())
    }
    fn tools(&self) -> mcbench_tools::Tools {
        let stubs = stubbed(self.real.is_some()).iter().map(|&name| {
            Arc::new(ReplayTool {
                name,
                replay: Arc::clone(&self.replay),
            }) as Arc<dyn Tool>
        });
        match &self.real {
            None => mcbench_tools::Tools::of(stubs.collect()),
            Some(services) => stubs.fold(
                mcbench_tools::Tools::new(Arc::clone(services)),
                mcbench_tools::Tools::with,
            ),
        }
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

/// A window that records every update.
#[derive(Default)]
struct Recording(Mutex<Vec<Update>>);

impl Window for Recording {
    fn publish(&self, update: Update) {
        self.0.lock().unwrap().push(update);
    }
}

impl Recording {
    /// Chat `id`'s turn events as NDJSON lines, from position `from`.
    fn lines(&self, id: &str, from: usize) -> Vec<String> {
        self.0.lock().unwrap()[from..]
            .iter()
            .filter_map(|update| match update {
                Update::Turn { chat, event } if chat == id => {
                    event.to_json().map(|value| mcbench_json::stringify(&value))
                }
                _ => None,
            })
            .collect()
    }
}

fn first_difference(path: &str, a: &Value, b: &Value) -> String {
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => {
            for (key, value) in x {
                match y.get(key) {
                    Some(other) if other == value => {}
                    Some(other) => return first_difference(&format!("{path}.{key}"), value, other),
                    None => return format!("{path}.{key}: only node has it"),
                }
            }
            format!("{path}: rust has more keys")
        }
        (Value::Array(x), Value::Array(y)) => {
            for (i, (p, q)) in x.iter().zip(y).enumerate() {
                if p != q {
                    return first_difference(&format!("{path}[{i}]"), p, q);
                }
            }
            format!("{path}: node {} items, rust {}", x.len(), y.len())
        }
        _ => {
            let (a, b) = (a.to_string(), b.to_string());
            let at = a.chars().zip(b.chars()).take_while(|(x, y)| x == y).count();
            let from = at.saturating_sub(120);
            let cut = |s: &str| s.chars().skip(from).take(400).collect::<String>();
            format!(
                "{path} (from character {from}):\n  node {}\n  rust {}",
                cut(&a),
                cut(&b)
            )
        }
    }
}

/// Runs a Node script of `mcbench-agent`'s `tests/node/` as
/// `common::node` does, `tests/node/startup.js` preloaded after `det.js`.
fn node(env: &common::Env, data: &Path, script: &str, args: &[&str], vars: &[(&str, &str)]) {
    let agent = Path::new(env!("CARGO_MANIFEST_DIR")).join("../agent/tests/node");
    let ours = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/node");
    let mut command =
        Process::new(std::env::var("MCBENCH_NODE").unwrap_or_else(|_| "node".to_string()));
    command
        .arg("-r")
        .arg(agent.join("det.js"))
        .arg("-r")
        .arg(ours.join("startup.js"))
        .arg(agent.join(script))
        .args(args)
        .env("MCBENCH_NODE_DIFF", &env.checkout)
        .env("MCBENCH_DATA_DIR", data)
        .env("MCBENCH_SETTINGS_FILE", data.join("settings.json"))
        .env("MCBENCH_TOOLS_DIR", env.scratch.join("tools-empty"))
        .env_remove("MCBENCH_CONTEXT_MAX_BYTES");
    for (key, value) in vars {
        command.env(key, value);
    }
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "node {script}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
#[ignore = "needs MCBENCH_DIFF_DATA, MCBENCH_NODE_DIFF and MCBENCH_DIFF_SCRATCH (mcbench-agent's tests/common)"]
fn the_cores_chat_route_replays_as_todays() {
    let env = common::env();
    let node_data = common::clone(&env, &env.fixture, "core-replay-node");
    let rust_data = common::clone(&env, &env.fixture, "core-replay-rust");
    let cases_file = env.scratch.join("core-replay-cases.json");
    let node_out = env.scratch.join("core-replay-node.json");
    let real = real_tools();
    let tools_dir = std::env::var("MCBENCH_REPLAY_TOOLS_DIR").unwrap_or_default();
    let real_vars: Vec<(&str, &str)> = if real {
        assert!(
            !tools_dir.is_empty(),
            "MCBENCH_REPLAY_TOOLS=real needs MCBENCH_REPLAY_TOOLS_DIR"
        );
        vec![
            ("MCBENCH_REPLAY_TOOLS", "real"),
            ("MCBENCH_TOOLS_DIR", tools_dir.as_str()),
        ]
    } else {
        Vec::new()
    };
    node(
        &env,
        &node_data,
        "replay.js",
        &["cases", cases_file.to_str().unwrap()],
        &[],
    );
    node(
        &env,
        &node_data,
        "replay.js",
        &[
            "run",
            cases_file.to_str().unwrap(),
            node_out.to_str().unwrap(),
        ],
        &real_vars,
    );
    let cases: Vec<Value> =
        serde_json::from_str(&std::fs::read_to_string(&cases_file).unwrap()).unwrap();
    let node_cases: Vec<Value> =
        serde_json::from_str(&std::fs::read_to_string(&node_out).unwrap()).unwrap();

    let host = common::DetHost::new(common::DET_NOW);
    let replay = Arc::new(Replay::default());
    let program = std::env::var_os("MCBENCH_ENGINE_PROGRAM")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_exe().unwrap());
    // The real tools' services need a store before the core has one: the
    // core opens its own, so they are made on the core's (below).
    let backend = Arc::new(ReplayBackend {
        replay: Arc::clone(&replay),
        real: None,
    });
    let backend_slot: Arc<Mutex<Option<Arc<ReplayBackend>>>> = Arc::new(Mutex::new(None));
    let core = if real {
        // The real tools' services are over the core's store, so the core
        // is made first with a backend that is filled in after.
        struct Late(Arc<Mutex<Option<Arc<ReplayBackend>>>>);
        #[async_trait]
        impl Backend for Late {
            fn provider(&self, key: &str) -> Result<Arc<dyn Provider>, String> {
                self.0.lock().unwrap().as_ref().unwrap().provider(key)
            }
            fn tools(&self) -> mcbench_tools::Tools {
                self.0.lock().unwrap().as_ref().unwrap().tools()
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
        let core = Core::start(Parts {
            data: rust_data.clone(),
            program: program.clone(),
            host: host.clone(),
            shipped: Box::new(|| Err("unused".into())),
            client: mcbench_setup::fetch::client(),
            backend: Some(Arc::new(Late(Arc::clone(&backend_slot)))),
            installer: None,
            share: None,
            live: Default::default(),
        })
        .unwrap();
        let ids: Arc<dyn mcbench_store::Host> = host.clone();
        mcbench_tools::verify::set_uuid_source(Box::new(move || ids.uuid())).unwrap();
        let tables = PathBuf::from(std::env::var("MCBENCH_TABLES").expect("MCBENCH_TABLES"));
        let services = core.handle().block_on({
            let (store, data) = (Arc::clone(core.store()), rust_data.clone());
            core.handle().spawn(async move {
                let engines = mcbench_tools::testing::shared(&tables).unwrap();
                let verifier = engines.verifier(data.join("minecraft-verify")).unwrap();
                let registry = mcbench_tools::testing::registry_of(&data).unwrap();
                mcbench_tools::Services::on(
                    &engines,
                    store,
                    Arc::new(mcbench_tools::testing::RecordedSandbox::new()),
                    verifier,
                    Arc::new(registry),
                    mcbench_tools::shots::SharedRenderer::new(),
                )
            })
        });
        *backend_slot.lock().unwrap() = Some(Arc::new(ReplayBackend {
            replay: Arc::clone(&replay),
            real: Some(services.unwrap()),
        }));
        core
    } else {
        Core::start(Parts {
            data: rust_data.clone(),
            program,
            host: host.clone(),
            shipped: Box::new(|| Err("unused".into())),
            client: mcbench_setup::fetch::client(),
            backend: Some(backend),
            installer: None,
            share: None,
            live: Default::default(),
        })
        .unwrap()
    };
    let window = Arc::new(Recording::default());
    mcbench_ui::Core::attach(&core, window.clone());
    let handle = core.handle().clone();
    handle.block_on(core.started());
    assert!(replay.turns.set(core.turns().clone()).is_ok());

    let mut differences = Vec::new();
    let mut events = 0;
    for (index, case) in cases.iter().enumerate() {
        {
            let mut current = replay.current.lock().unwrap();
            current.steps = case["steps"].as_array().unwrap().clone();
            current.tools = case["tools"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|entry| stubbed(real).contains(&entry["name"].as_str().unwrap()))
                .cloned()
                .collect();
            current.requests = Vec::new();
            current.image_input = case["imageInput"] == Value::Bool(true);
        }
        let provider = case["provider"].as_str().unwrap().to_string();
        let model = case["model"].as_str().unwrap().to_string();
        let store = Arc::clone(core.store());
        let id = match case["chat"].as_str() {
            Some(chat) => {
                let id = mcbench_store::Host::uuid(&*host);
                let source = store.conversation(chat).unwrap().unwrap();
                let source = source.lock().unwrap();
                let prompt = case["prompt"].as_u64().unwrap() as usize;
                let world = source
                    .checkpoint(&source.messages[prompt].id)
                    .unwrap()
                    .clone();
                store
                    .fork_conversation(&source, prompt, &world, Some(id.clone()))
                    .unwrap();
                id
            }
            None => {
                let live = store
                    .create_conversation(&provider, &model, NewConversation::default())
                    .unwrap();
                live.lock().unwrap().id.0.clone()
            }
        };
        replay.current.lock().unwrap().chat = id.clone();
        let from = window.0.lock().unwrap().len();
        let body = ChatMessage {
            provider,
            model,
            effort: Some(case["effort"].as_str().unwrap_or("").into()),
            resolution: case["resolution"].as_u64().unwrap() as u32,
            message: Outgoing {
                text: case["text"].as_str().unwrap().into(),
                ..Outgoing::default()
            },
            edit_of: None,
        };
        let (status, error) = handle
            .block_on({
                let (core, id) = (core.clone(), id.clone());
                handle.spawn(async move {
                    let sent = core.chat(&id, body).await;
                    let outcome = match sent {
                        Ok(stream) => {
                            stream.done().await;
                            (200, Value::Null)
                        }
                        Err(refusal) => (refusal.status as u64, Value::String(refusal.message)),
                    };
                    core.turns().free(&id).await;
                    core.turns().titles_settled().await;
                    outcome
                })
            })
            .unwrap();
        let lines = window.lines(&id, from);
        events += lines.len();
        let current = replay.current.lock().unwrap();
        let ours = json!({
            "chat": id,
            "status": status,
            "error": error,
            "events": lines,
            "requests": current.requests,
            "left": { "steps": current.steps.len(), "tools": current.tools.len() },
        });
        let split = |case: &Value| {
            let mut case = case.clone();
            let requests = case["requests"].as_array().cloned().unwrap_or_default();
            let (titles, turn): (Vec<Value>, Vec<Value>) = requests
                .into_iter()
                .partition(|r| r["conversationId"].is_null());
            case["requests"] = Value::Array(turn);
            case["titles"] = Value::Array(titles);
            case
        };
        let (ours, theirs) = (
            wall_free_value(&split(&ours)),
            wall_free_value(&split(&node_cases[index])),
        );
        if ours != theirs {
            differences.push(first_difference(&format!("case {index}"), &theirs, &ours));
        }
    }
    core.shutdown();
    drop(core);
    let node_dump = env.scratch.join("core-replay-node-dump.json");
    let rust_dump = env.scratch.join("core-replay-rust-dump.json");
    node(
        &env,
        &node_data,
        "dump.js",
        &[node_data.to_str().unwrap(), node_dump.to_str().unwrap()],
        &[],
    );
    node(
        &env,
        &rust_data,
        "dump.js",
        &[rust_data.to_str().unwrap(), rust_dump.to_str().unwrap()],
        &[],
    );
    let node_doc: Value =
        serde_json::from_str(&std::fs::read_to_string(&node_dump).unwrap()).unwrap();
    let rust_doc: Value =
        serde_json::from_str(&std::fs::read_to_string(&rust_dump).unwrap()).unwrap();
    let mut rows = 0;
    for (table, node_rows) in node_doc["tables"].as_object().unwrap() {
        let node_rows = node_rows.as_array().unwrap();
        let rust_rows = rust_doc["tables"][table]
            .as_array()
            .cloned()
            .unwrap_or_default();
        rows += node_rows.len();
        if node_rows.len() != rust_rows.len() {
            differences.push(format!(
                "{table}: node {} rows, rust {}",
                node_rows.len(),
                rust_rows.len()
            ));
        }
        for (i, (a, b)) in node_rows.iter().zip(&rust_rows).enumerate() {
            let (a, b) = (&wall_free_value(a), &wall_free_value(b));
            if a != b {
                differences.push(first_difference(&format!("{table} row {i}"), a, b));
            }
        }
    }
    let files = |doc: &Value| -> Value {
        let mut files = doc["files"].clone();
        if real_tools()
            && let Value::Array(list) = &mut files
        {
            for entry in list.iter_mut() {
                let name = entry.to_string();
                if (name.contains("/verification/") || name.contains("minecraft-verify"))
                    && let Value::Array(pair) = entry
                    && pair.len() == 2
                {
                    pair[1] = Value::String("<report>".into());
                }
            }
        }
        files
    };
    if files(&node_doc) != files(&rust_doc) {
        differences.push("files differ".into());
    }
    for key in ["searchVocabulary", "sqlite_master", "user_version"] {
        if node_doc[key] != rust_doc[key] {
            differences.push(format!("{key} differs"));
        }
    }
    println!(
        "core replay: {} cases, {events} events, {rows} rows, {} differences",
        cases.len(),
        differences.len()
    );
    for line in differences.iter().take(200) {
        eprintln!("{line}\n");
    }
    let allowed = |line: &&String| {
        real_tools() && (line.starts_with("messages row ") || line.contains(".messages (from"))
    };
    let unexplained: Vec<&String> = differences.iter().filter(|line| !allowed(line)).collect();
    assert!(unexplained.is_empty(), "{} differences", unexplained.len());
    for path in [&node_data, &rust_data] {
        std::fs::remove_dir_all(path).unwrap();
    }
    for file in [&cases_file, &node_out, &node_dump, &rust_dump] {
        std::fs::remove_file(file).unwrap();
    }
}
