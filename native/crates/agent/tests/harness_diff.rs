//! The harness differential, the Rust side (`tests/node/harness.sh` runs
//! both): the cases of `tests/node/harness_cases.py` (one chat per CLI, six
//! turns: tool calls, a batch, a stop, a resume, warm follow-ups) through
//! [`Turns`] with the real Claude Code and Codex CLI providers against
//! mcbench-providers' fake CLI, compared with what today's chat route did
//! (`tests/node/harness.js`): every turn's NDJSON lines, what the fake CLI
//! saw (its record, the scratch folder's path replaced), and every row of
//! the two databases (`tests/node/dump.js`).
//!
//! Its own `main` (`harness = false`): started as `<this> tool-bridge
//! <socket>` it is the bridge's relay, which the fake CLI starts as a real
//! one starts `mcbench-lite tool-bridge`. Otherwise, with
//! `MCBENCH_HARNESS_DIFF=<dir>` it runs; without it, it says it was skipped.
//!
//! Telemetry's clock fields (`startedAt`, `wallMs`, `firstDeltaMs`) of the
//! completions are compared as present or absent only: Node's run fixes
//! `Date` (det.js), `mcbench-providers`' tracker reads the real clock.

#[cfg(unix)]
mod common;

#[cfg(unix)]
mod unix {
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex, OnceLock};

    use async_trait::async_trait;
    use serde_json::{Map, Value, json};
    use tokio_util::sync::CancellationToken;

    use crate::common;
    use mcbench_agent::context;
    use mcbench_agent::contract::Placement;
    use mcbench_agent::lifecycle::{ChatMessage, Outgoing, Services, TurnConfig, Turns};
    use mcbench_agent::{TurnEvent, TurnSink};
    use mcbench_providers::harness::BridgeCommand;
    use mcbench_providers::harness::claude_code::{ClaudeCode, ClaudeConfig};
    use mcbench_providers::harness::codex_cli::{CodexCli, CodexConfig, Launch};
    use mcbench_providers::{
        AssistantMessage, CompletionRequest, Delta, ModelFacts, ModelInfo, Outcome, Provider,
        ProviderError, ProviderId, Telemetry,
    };
    use mcbench_store::NewConversation;
    use mcbench_tools::WorkFolders as SandboxWork;
    use mcbench_tools::verify::Budgets;
    use mcbench_tools::{Chat, Progress, Tool, ToolOutcome};

    #[derive(Default)]
    struct Current {
        tools: Vec<Value>,
        chat: String,
    }

    #[derive(Default)]
    struct State {
        current: Mutex<Current>,
        turns: OnceLock<Turns>,
    }

    /// The title model: an API provider answering once the turn has ended.
    struct Titles(Arc<State>);

    #[async_trait]
    impl Provider for Titles {
        fn id(&self) -> ProviderId {
            ProviderId::OpenRouter
        }
        async fn complete(
            &self,
            request: &CompletionRequest,
            _: &mut (dyn FnMut(Delta) + Send),
            _: &CancellationToken,
        ) -> Result<AssistantMessage, ProviderError> {
            let chat = self.0.current.lock().unwrap().chat.clone();
            self.0.turns.get().unwrap().free(&chat).await;
            Ok(AssistantMessage {
                content: "A title".into(),
                reasoning: None,
                reasoning_details: None,
                finish_reason: None,
                telemetry: Telemetry {
                    provider: ProviderId::OpenRouter,
                    model: request.model.clone(),
                    effort: request.effort.clone(),
                    started_at: String::new(),
                    wall_ms: 0,
                    first_delta_ms: None,
                    context_bytes: 0,
                    message_count: 0,
                    usage: None,
                    outcome: Outcome::Completed {
                        context_tokens: None,
                    },
                },
                notices: None,
                plan: None,
                rate_limit: None,
                tool_calls: Vec::new(),
            })
        }
        async fn models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
            unimplemented!("not asked")
        }
        async fn model_info(&self, _: &str) -> Result<ModelFacts, ProviderError> {
            unimplemented!("not asked")
        }
        async fn close_session(&self, _: &str) {}
    }

    /// A harness with today's `modelInfo` stand-in (`llm.modelInfo` is
    /// patched on the Node side): the model takes images, no CLI is asked.
    struct Patched(Arc<dyn Provider>);

    #[async_trait]
    impl Provider for Patched {
        fn id(&self) -> ProviderId {
            self.0.id()
        }
        async fn complete(
            &self,
            request: &CompletionRequest,
            on_delta: &mut (dyn FnMut(Delta) + Send),
            cancel: &CancellationToken,
        ) -> Result<AssistantMessage, ProviderError> {
            self.0.complete(request, on_delta, cancel).await
        }
        async fn models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
            unimplemented!("not asked")
        }
        async fn model_info(&self, _: &str) -> Result<ModelFacts, ProviderError> {
            Ok(ModelFacts {
                image_input: Some(true),
                ..ModelFacts::unknown()
            })
        }
        async fn close_session(&self, conversation_id: &str) {
            self.0.close_session(conversation_id).await
        }
    }

    struct ReplayTool {
        name: &'static str,
        state: Arc<State>,
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
            _: &mut (dyn FnMut(Progress) + Send),
            _: &CancellationToken,
        ) -> Result<ToolOutcome, mcbench_tools::ToolError> {
            let (entry, chat) = {
                let mut current = self.state.current.lock().unwrap();
                assert!(!current.tools.is_empty(), "{} has no call left", self.name);
                (current.tools.remove(0), current.chat.clone())
            };
            assert_eq!(entry["name"], self.name);
            if entry["stop"] == Value::Bool(true) {
                self.state.turns.get().unwrap().stop(&chat);
            }
            let result = match self.name {
                "prompt_library" => json!({ "ok": true, "prompts": entry["result"]["prompts"] }),
                _ => entry["result"].clone(),
            };
            Ok(ToolOutcome {
                result,
                screenshots: None,
                images: Vec::new(),
            })
        }
    }

    struct Harnesses {
        state: Arc<State>,
        claude: Arc<ClaudeCode>,
        codex: Arc<CodexCli>,
    }

    #[async_trait]
    impl Services for Harnesses {
        fn provider(&self, key: &str) -> Result<Arc<dyn Provider>, String> {
            Ok(match key {
                "claude-code" => Arc::new(Patched(self.claude.clone())),
                "codex-cli" => Arc::new(Patched(self.codex.clone())),
                "openrouter" => Arc::new(Titles(Arc::clone(&self.state))),
                other => return Err(format!("no provider {other} here")),
            })
        }
        fn tools(&self) -> mcbench_tools::Tools {
            mcbench_tools::Tools::of(
                [
                    "build",
                    "test_build",
                    "see_build",
                    "bash",
                    "view_image",
                    "prompt_library",
                ]
                .into_iter()
                .map(|name| {
                    Arc::new(ReplayTool {
                        name,
                        state: Arc::clone(&self.state),
                    }) as Arc<dyn Tool>
                })
                .collect(),
            )
        }
        fn title_model(&self) -> Option<(String, String, Option<String>)> {
            Some((
                "openrouter".into(),
                "title/model".into(),
                Some("low".into()),
            ))
        }
        fn sync_attachments(&self, _: &str) -> Result<(), String> {
            Ok(())
        }
        async fn reset_sandbox(&self, _: &str) -> Result<(), String> {
            Ok(())
        }
        fn conversations_changed(&self) {}
    }

    #[derive(Clone, Default)]
    struct Lines(Arc<Mutex<Vec<String>>>);

    impl TurnSink for Lines {
        fn emit(&mut self, event: TurnEvent) {
            if let Some(value) = event.to_json() {
                self.0.lock().unwrap().push(mcbench_json::stringify(&value));
            }
        }
    }

    fn fake(dir: &Path, cli: &str, script: &Value, checkout: &Path) -> PathBuf {
        let bin = dir.join(cli).join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let program = bin.join(cli);
        std::fs::copy(
            checkout.join("native/crates/providers/tests/fakes/fake_cli.py"),
            &program,
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::write(bin.join("script.json"), mcbench_json::stringify(script)).unwrap();
        program
    }

    /// A row's text with the completion telemetry's clock fields reduced to
    /// whether they are there.
    fn normalized(cell: &Value) -> Value {
        let Some(text) = cell.as_str() else {
            return cell.clone();
        };
        let Ok(Value::Object(mut payload)) = serde_json::from_str::<Value>(text) else {
            return cell.clone();
        };
        if payload.get("role").and_then(Value::as_str) != Some("tool")
            && let Some(Value::Object(telemetry)) = payload.get_mut("telemetry")
        {
            for key in ["startedAt", "wallMs", "firstDeltaMs"] {
                if let Some(value) = telemetry.get_mut(key) {
                    *value = Value::Bool(!value.is_null());
                }
            }
        }
        Value::String(mcbench_json::stringify(&Value::Object(payload)))
    }

    pub fn run(dir: PathBuf) {
        let checkout =
            PathBuf::from(std::env::var("MCBENCH_NODE_DIFF").expect("MCBENCH_NODE_DIFF"));
        let cases: Vec<Value> =
            serde_json::from_str(&std::fs::read_to_string(dir.join("cases.json")).unwrap())
                .unwrap();
        let node: Vec<Value> =
            serde_json::from_str(&std::fs::read_to_string(dir.join("node/out.json")).unwrap())
                .unwrap();
        let side = dir.join("rust");
        let data = side.join("data");
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        let mut differences = Vec::new();
        let ours = runtime.block_on(async {
            let host = common::DetHost::new(common::DET_NOW);
            let store = common::open_with(&data, host.clone());
            store.clear_expired_archive(common::DET_NOW).unwrap();
            let me = std::env::current_exe().unwrap();
            let claude = Arc::new(ClaudeCode::new(
                ClaudeConfig {
                    data_dir: data.clone(),
                    binary: fake(&side, "claude", &cases[0]["script"], &checkout).into(),
                    home: None,
                    bridge: BridgeCommand {
                        program: me.clone(),
                    },
                },
                None,
            ));
            let codex = Arc::new(CodexCli::new(
                CodexConfig {
                    data_dir: data.clone(),
                    binary: fake(&side, "codex", &cases[1]["script"], &checkout).into(),
                    prefix: Vec::new(),
                    home: Some(side.join("codex-login")),
                    bridge: BridgeCommand { program: me },
                    launch: Some(Launch::Host),
                },
                Arc::new(mcbench_providers::catalog::CodexModels::default()),
            ));
            let state = Arc::new(State::default());
            let turns = Turns::new(
                Arc::clone(&store),
                Arc::new(Harnesses {
                    state: Arc::clone(&state),
                    claude: Arc::clone(&claude),
                    codex,
                }),
                TurnConfig {
                    placement: Placement::Updates,
                    network_access: true,
                    budgets: Budgets::DEFAULT,
                    max_context_bytes: context::DEFAULT_MAX_BYTES,
                    keep_exchanges: context::KEEP_EXCHANGES,
                    work: Arc::new(SandboxWork {
                        root: data.join("sandbox"),
                    }),
                },
            );
            assert!(state.turns.set(turns.clone()).is_ok());
            let mut out = Vec::new();
            for case in &cases {
                let (provider, model) = (
                    case["provider"].as_str().unwrap(),
                    case["model"].as_str().unwrap(),
                );
                let id = {
                    let live = store
                        .create_conversation(provider, model, NewConversation::default())
                        .unwrap();
                    let id = live.lock().unwrap().id.0.clone();
                    drop(live);
                    id
                };
                let mut done = Vec::new();
                for turn in case["turns"].as_array().unwrap() {
                    {
                        let mut current = state.current.lock().unwrap();
                        current.tools = turn["tools"].as_array().unwrap().clone();
                        current.chat = id.clone();
                    }
                    let lines = Lines::default();
                    let sent = turns
                        .send(
                            &id,
                            ChatMessage {
                                provider: provider.into(),
                                model: model.into(),
                                effort: None,
                                resolution: 1,
                                message: Outgoing {
                                    text: turn["text"].as_str().unwrap().into(),
                                    ..Outgoing::default()
                                },
                                edit_of: None,
                            },
                            Box::new(lines.clone()),
                        )
                        .await;
                    let (status, error) = match sent {
                        Ok(stream) => {
                            stream.done().await;
                            (200, Value::Null)
                        }
                        Err(refusal) => (u64::from(refusal.status), Value::String(refusal.message)),
                    };
                    turns.free(&id).await;
                    turns.titles_settled().await;
                    let left = state.current.lock().unwrap().tools.len();
                    let events = lines.0.lock().unwrap().clone();
                    done.push(
                        json!({ "status": status, "error": error, "events": events, "left": left }),
                    );
                }
                let record = side
                    .join(case["cli"].as_str().unwrap())
                    .join("bin/record.jsonl");
                let fake: Vec<String> = std::fs::read_to_string(&record)
                    .map(|t| t.lines().map(str::to_string).collect())
                    .unwrap_or_default();
                out.push(json!({ "chat": id, "turns": done, "fake": fake }));
            }
            claude.shutdown().await;
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            out
        });
        // The fake's record names each side's folder.
        let unsided = |value: &Value, side: &str| -> Value {
            let text = mcbench_json::stringify(value)
                .replace(&dir.join(side).display().to_string(), "<side>");
            serde_json::from_str(&text).unwrap()
        };
        // The bridge the CLI is told to start is today's `mcp-bridge.py`
        // under `python3` in Node and the app's own binary as `tool-bridge`
        // here (the native app's design); the socket's folder is random.
        let exe = std::env::current_exe().unwrap().display().to_string();
        let script = checkout.join("server/mcp-bridge.py").display().to_string();
        let temporary = regex::Regex::new("mcb-[A-Za-z0-9]{6}").unwrap();
        let bridged = |fake: &Value, side: &str| -> Value {
            Value::Array(
                fake.as_array()
                    .unwrap()
                    .iter()
                    .map(|line| {
                        let line = line.as_str().unwrap();
                        let line = if side == "node" {
                            line.replace("\\\"python3\\\"", "\\\"<bridge>\\\"")
                                .replace(&script, "tool-bridge")
                        } else {
                            line.replace(&format!("\\\"{exe}\\\""), "\\\"<bridge>\\\"")
                        };
                        Value::String(temporary.replace_all(&line, "mcb-XXXXXX").into_owned())
                    })
                    .collect(),
            )
        };
        // The chats and turns are named together, each fake record apart:
        // the records also hold ids of the harness's own making.
        let parts = |cases: &[Value], side: &str| -> (Value, Vec<Value>) {
            let turns: Vec<Value> = cases
                .iter()
                .map(|c| json!({ "chat": c["chat"], "turns": c["turns"] }))
                .collect();
            let fakes = cases
                .iter()
                .map(|c| canonical(&unsided(&bridged(&c["fake"], side), side)))
                .collect();
            (canonical(&Value::Array(turns)), fakes)
        };
        let (node_turns, node_fakes) = parts(&node, "node");
        let (rust_turns, rust_fakes) = parts(&ours, "rust");
        for (i, (a, b)) in node_turns
            .as_array()
            .unwrap()
            .iter()
            .zip(rust_turns.as_array().unwrap())
            .enumerate()
        {
            if a != b {
                differences.push(first_difference(&format!("case {i}"), a, b));
            }
        }
        for (i, (a, b)) in node_fakes.iter().zip(&rust_fakes).enumerate() {
            if a != b {
                differences.push(first_difference(&format!("case {i} fake"), a, b));
            }
        }
        let rust_dump = side.join("dump.json");
        let node_binary = std::env::var("MCBENCH_NODE").unwrap();
        let status = std::process::Command::new(node_binary)
            .arg(checkout.join("native/crates/agent/tests/node/dump.js"))
            .arg(&data)
            .arg(&rust_dump)
            .env("MCBENCH_NODE_DIFF", &checkout)
            .status()
            .unwrap();
        assert!(status.success());
        let theirs: Value = canonical(
            &serde_json::from_str(&std::fs::read_to_string(dir.join("node/dump.json")).unwrap())
                .unwrap(),
        );
        let mine: Value = canonical(
            &serde_json::from_str(&std::fs::read_to_string(&rust_dump).unwrap()).unwrap(),
        );
        let mut rows = 0;
        for (table, node_rows) in theirs["tables"].as_object().unwrap() {
            let node_rows = node_rows.as_array().unwrap();
            let rust_rows = mine["tables"][table]
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
                continue;
            }
            for (r, (a, b)) in node_rows.iter().zip(&rust_rows).enumerate() {
                let norm = |row: &Value| {
                    Value::Array(row.as_array().unwrap().iter().map(normalized).collect())
                };
                // FTS5's blobs hold the clock fields too; compared through
                // the vocabulary below.
                if table.starts_with("search_") && table != "search_docs" {
                    continue;
                }
                if norm(a) != norm(b) {
                    differences.push(format!(
                        "{table} row {r}:\n  node {}\n  rust {}",
                        cut(&norm(a)),
                        cut(&norm(b))
                    ));
                }
            }
        }
        if theirs["searchVocabulary"] != mine["searchVocabulary"] {
            differences.push("searchVocabulary differs".into());
        }
        let turns: usize = cases
            .iter()
            .map(|c| c["turns"].as_array().unwrap().len())
            .sum();
        println!(
            "harness: {} chats, {turns} turns, {rows} rows, {} differences",
            cases.len(),
            differences.len()
        );
        for line in differences.iter().take(20) {
            println!("{line}\n");
        }
        if !differences.is_empty() {
            std::process::exit(1);
        }
    }

    /// `value` with each id of det.js's form named by the order it first
    /// appears in: Node's det.js counts every `randomUUID` of the process,
    /// the harness modules' own included, so the store's ids are offset by
    /// those; the order is what is compared.
    fn canonical(value: &Value) -> Value {
        // Every UUID (det.js and the store make them in Node and the
        // store here; Claude Code's session id is random here), and Codex's call ids
        // (`<8 hex of a random UUID>:<n>`), which the harness makes itself.
        let pattern = regex::Regex::new(
            "[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}|\\b[0-9a-f]{8}:[0-9]+\\b",
        )
        .unwrap();
        let text = mcbench_json::stringify(value);
        let mut names: Vec<String> = Vec::new();
        let out = pattern.replace_all(&text, |found: &regex::Captures| {
            let id = found[0].to_string();
            let at = match names.iter().position(|n| *n == id) {
                Some(at) => at,
                None => {
                    names.push(id);
                    names.len() - 1
                }
            };
            format!("id-{at:08}")
        });
        serde_json::from_str(&out).unwrap()
    }

    /// Where two values first differ, by path, with both sides there.
    fn first_difference(path: &str, a: &Value, b: &Value) -> String {
        match (a, b) {
            (Value::Object(x), Value::Object(y)) => {
                for (key, value) in x {
                    match y.get(key) {
                        Some(other) if other == value => {}
                        Some(other) => {
                            return first_difference(&format!("{path}.{key}"), value, other);
                        }
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
                let from = at.saturating_sub(100);
                let cut = |s: &str| s.chars().skip(from).take(300).collect::<String>();
                format!(
                    "{path} (from character {from}):\n  node {}\n  rust {}",
                    cut(&a),
                    cut(&b)
                )
            }
        }
    }

    fn cut(value: &Value) -> String {
        value.to_string().chars().take(1500).collect()
    }

    pub fn main() {
        let args: Vec<String> = std::env::args().collect();
        if args.get(1).map(String::as_str) == Some("tool-bridge") {
            mcbench_providers::bridge::relay_main(
                &mcbench_providers::bridge::BridgeAddress::UnixSocket(PathBuf::from(&args[2])),
            );
        }
        match std::env::var("MCBENCH_HARNESS_DIFF") {
            Ok(dir) => run(PathBuf::from(dir)),
            Err(_) => println!(
                "harness_diff: skipped (MCBENCH_HARNESS_DIFF is not set; tests/node/harness.sh runs it)"
            ),
        }
    }
}

fn main() {
    #[cfg(unix)]
    unix::main();
    #[cfg(not(unix))]
    println!("harness_diff: the CLI harnesses run on macOS and Linux");
}
