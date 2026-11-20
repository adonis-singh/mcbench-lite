//! `mcbench-lite headless turn`: the model loop for the private benchmark
//! (`PROTOCOL.md`, family `turn`), replacing its `require`s of
//! `server/agent` (`runTurn`, `executeToolCall`, `toolMessageContent`,
//! `toolsFor`) and `server/prompt` (`systemPrompt`).
//!
//! Requests, one JSON object a line, each with an `id` and an `op`:
//! `create` (a chat in a data folder), `tools` (the tools and system
//! prompt of a form), `format` (a tool
//! message's text), `call` (one tool call executed in a chat of a data
//! folder, saved), `turn` (one turn of a chat with a provider, its events as
//! `{id, event}` lines first). The tool set and the provider a data folder's
//! process uses come from [`adapter`].

use std::io::{BufRead, Write};
use std::sync::{Arc, Mutex};

use serde_json::{Map, Value, json};
use tokio_util::sync::CancellationToken;

use mcbench_agent::context::{self, Archives};
use mcbench_agent::contract::{self, Budgets, Form, Placement};
use mcbench_agent::turn::SharedSink;
use mcbench_agent::{TurnEvent, TurnSetup, TurnSink, execute_tool_call, message_content};
use mcbench_store::{DataFolder, Limits, Options, Store, SystemHost};
use mcbench_tools::{Chat, Progress, WorkFolders, Workspace};

/// The tool set and the providers of a data folder's process.
pub mod adapter {
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    use mcbench_providers::Provider;
    use mcbench_store::Store;

    use mcbench_setup::Steps;
    use mcbench_setup::steps::native_steps;
    use mcbench_tools::engines::{Engines, registry};
    use mcbench_tools::{Services, Tools};
    use tokio_util::sync::CancellationToken;

    use crate::providers::Providers;
    use crate::shell::PlatformSandbox;

    /// Minecraft's tables folder, `MCBENCH_TABLES` (`minecraft/<id>` of a
    /// data folder, as `headless setup` prints it).
    fn tables() -> Result<PathBuf, String> {
        std::env::var_os("MCBENCH_TABLES").map(PathBuf::from).ok_or_else(|| {
            "MCBENCH_TABLES names no folder: the tools need Minecraft's tables (core/minecraft/<version>)"
                .to_string()
        })
    }

    /// The process's engines and build child: made at the first call that
    /// needs them, on Minecraft's tables at `MCBENCH_TABLES`, with this
    /// program as the engine children (`mcbench-lite headless engine`) and
    /// `mcbench-build-child` beside it as the build child
    /// (`mcbench_build::child_program`).
    pub(crate) fn engines() -> Result<Arc<Engines>, String> {
        static ENGINES: Mutex<Option<Arc<Engines>>> = Mutex::new(None);
        let mut engines = ENGINES.lock().expect("the engines' lock");
        if let Some(engines) = &*engines {
            return Ok(engines.clone());
        }
        let tables = tables()?;
        let engine = std::env::current_exe().map_err(|e| format!("this program: {e}"))?;
        let started = Arc::new(Engines::start(
            &tables,
            engine,
            mcbench_build::child_program()?,
        )?);
        *engines = Some(started.clone());
        Ok(started)
    }

    /// The tools of a data folder's process (`build`, `test_build`,
    /// `see_build`, `bash`, `view_image`, and `prompt_library` for a chat):
    /// [`Tools::new`] over the store, the engines, the engine verifier
    /// archiving in `<data>/minecraft-verify`, the data folder's
    /// `blocks.json`, and the platform's `bash` sandbox
    /// ([`PlatformSandbox`], made of the files setup names,
    /// `mcbench_setup::shell::files`, with this program's `sandbox-agent` as
    /// its helper and the store's budget; on macOS the VM boots in the
    /// background), answered beside the tools: the request shuts it down
    /// when it ends.
    pub async fn tools(store: &Arc<Store>) -> Result<(Tools, Arc<PlatformSandbox>), String> {
        let engines = engines()?;
        let data = store.folder().root.clone();
        let exe = std::env::current_exe().map_err(|e| format!("this program: {e}"))?;
        let sandbox = Arc::new(PlatformSandbox::new(
            data.join("sandbox"),
            &exe,
            Arc::new(mcbench_tools::budget::StoreBudget(Arc::clone(
                store.storage(),
            ))),
        ));
        let files = mcbench_setup::shell::files(
            Arc::new(Steps::new(native_steps(), Box::new(|_| {}))),
            data.clone(),
            mcbench_setup::fetch::client(),
            CancellationToken::new(),
        )
        .await;
        let verifier = engines.verifier(data.join("minecraft-verify"))?;
        let registry = Arc::new(registry(&tables()?, &data)?);
        crate::shell::write_rootfs(of_folder(&data)?.shell(), &files);
        // Without the files every call answers the shell step's sentence.
        let _ = sandbox.make(files);
        let services = Services::on(
            &engines,
            Arc::clone(store),
            Arc::clone(&sandbox) as Arc<dyn mcbench_sandbox::Sandbox>,
            verifier,
            registry,
            crate::renderer::for_turn(),
        );
        Ok((Tools::new(services), sandbox))
    }

    /// The provider a chat names (`llm.js` `resolveProvider`), from the
    /// data folder's providers (`crate::providers::Providers`): an API
    /// provider with the key the settings save, or else its environment
    /// variable, and its switch; Claude Code and the Codex CLI run by the
    /// program and with the sign-in the app's core uses
    /// (`MCBENCH_CLAUDE_BINARY`, else `claude` on `PATH`, else the tools
    /// folder's; the CLI's own sign-in), one of each per data folder for the
    /// process, so their sessions last across the process's turns.
    pub fn provider(store: &Arc<Store>, key: &str) -> Result<Arc<dyn Provider>, String> {
        of_folder(&store.folder().root)?.provider(key)
    }

    /// The providers of data folder `data` for this process.
    pub fn of_folder(data: &std::path::Path) -> Result<Arc<Providers>, String> {
        static ALL: Mutex<Vec<(PathBuf, Arc<Providers>)>> = Mutex::new(Vec::new());
        let mut all = ALL.lock().expect("the providers' lock");
        if let Some((_, providers)) = all.iter().find(|(folder, _)| folder == data) {
            return Ok(Arc::clone(providers));
        }
        let file = mcbench_store::SettingsFile::path(data, |name| std::env::var(name).ok());
        let settings = Arc::new(mcbench_store::SettingsFile::open(file).map_err(|e| e.message)?);
        let program = std::env::current_exe().map_err(|e| format!("this program: {e}"))?;
        let providers = Arc::new(Providers::new(
            data.to_path_buf(),
            settings,
            mcbench_setup::tools_folder(data)?,
            program,
            Default::default(),
        ));
        all.push((data.to_path_buf(), Arc::clone(&providers)));
        Ok(providers)
    }
}

fn fail(message: &str) -> ! {
    eprintln!("mcbench-lite headless turn: {message}");
    std::process::exit(2)
}

fn field<'a>(request: &'a Map<String, Value>, key: &str) -> &'a Value {
    request
        .get(key)
        .unwrap_or_else(|| fail(&format!("a request needs {key}")))
}

fn text<'a>(request: &'a Map<String, Value>, key: &str) -> &'a str {
    field(request, key)
        .as_str()
        .unwrap_or_else(|| fail(&format!("{key} is a string")))
}

fn flag(request: &Map<String, Value>, key: &str) -> bool {
    field(request, key)
        .as_bool()
        .unwrap_or_else(|| fail(&format!("{key} is true or false")))
}

fn placement(request: &Map<String, Value>) -> Placement {
    match text(request, "placement") {
        "updates" => Placement::Updates,
        "strict" => Placement::Strict,
        other => fail(&format!("placement is updates or strict, not {other}")),
    }
}

fn open(data: &str) -> Result<Arc<Store>, String> {
    Store::open(
        &DataFolder { root: data.into() },
        Options {
            host: Arc::new(SystemHost),
            limits: Limits::from_env(|name| std::env::var(name).ok())?,
            chat_removed: Box::new(|_| {}),
        },
    )
    .map(Arc::new)
    .map_err(|e| e.message)
}

struct Lines {
    id: Value,
    out: Arc<Mutex<std::io::Stdout>>,
}

impl Lines {
    fn write(&self, value: Value) {
        let mut out = self.out.lock().expect("stdout");
        writeln!(out, "{}", mcbench_json::stringify(&value)).expect("stdout");
        out.flush().expect("stdout");
    }
}

impl TurnSink for Lines {
    fn emit(&mut self, event: TurnEvent) {
        if let Some(event) = event.to_json() {
            self.write(json!({ "id": self.id, "event": event }));
        }
    }
}

const OPS: [(&str, &[&str]); 5] = [
    ("create", &["data", "provider", "model"]),
    (
        "tools",
        &[
            "placement",
            "imageInput",
            "networkAccess",
            "chat",
            "resolution",
        ],
    ),
    ("format", &["name", "result"]),
    (
        "call",
        &[
            "data",
            "chat",
            "name",
            "arguments",
            "imageInput",
            "turnChat",
        ],
    ),
    (
        "turn",
        &[
            "data",
            "chat",
            "prompt",
            "effort",
            "placement",
            "networkAccess",
            "turnChat",
        ],
    ),
];

async fn answer(op: &str, request: &Map<String, Value>, lines: Lines) -> Result<Value, String> {
    let budgets = Budgets::from_env()?;
    match op {
        "tools" => {
            let form = Form {
                placement: placement(request),
                image_input: flag(request, "imageInput"),
                network_access: flag(request, "networkAccess"),
                chat: flag(request, "chat"),
                resolution: field(request, "resolution")
                    .as_u64()
                    .unwrap_or_else(|| fail("resolution is a number"))
                    as u32,
            };
            let contract = contract::of(form, &budgets)?;
            let tools: Vec<Value> = contract
                .tools
                .iter()
                .map(|tool| mcbench_json::parse(&tool.0))
                .collect::<Result<_, _>>()?;
            Ok(json!({ "tools": tools, "systemPrompt": contract.system_prompt }))
        }
        "create" => {
            let store = open(text(request, "data"))?;
            let live = store
                .create_conversation(
                    text(request, "provider"),
                    text(request, "model"),
                    mcbench_store::NewConversation::default(),
                )
                .map_err(|e| e.message)?;
            let id = live.lock().expect("a chat's lock").id.0.clone();
            Ok(json!({ "chat": id }))
        }
        "format" => Ok(
            json!({ "content": message_content(text(request, "name"), field(request, "result")) }),
        ),
        "call" => {
            let store = open(text(request, "data"))?;
            let id = text(request, "chat").to_string();
            let live = store
                .conversation(&id)
                .map_err(|e| e.message)?
                .ok_or_else(|| format!("Conversation {id} not found"))?;
            let (model, world) = {
                let conversation = live.lock().expect("a chat's lock");
                (conversation.model.clone(), conversation.world.clone())
            };
            let mut chat = Chat {
                id: id.clone(),
                model,
                world,
                workspace: Workspace {
                    chat: id.clone(),
                    host: store.folder().root.join("sandbox").join(&id),
                },
                image_input: flag(request, "imageInput"),
                chat: flag(request, "turnChat"),
            };
            let Value::Object(arguments) = field(request, "arguments") else {
                fail("arguments is an object")
            };
            let (tools, sandbox) = adapter::tools(&store).await?;
            let name = text(request, "name").to_string();
            let mut progress = |progress: Progress| {
                lines.write(json!({ "id": lines.id, "event": { "type": "tool_progress", "name": name, "progress": Value::Object(progress.raw) } }));
            };
            let before = (chat.world.uid(), chat.world.settle_ticks);
            let outcome = execute_tool_call(
                &tools,
                &mut chat,
                &name,
                arguments,
                &mut progress,
                &CancellationToken::new(),
            )
            .await;
            drop(tools);
            mcbench_sandbox::Sandbox::shutdown(&*sandbox).await;
            if (chat.world.uid(), chat.world.settle_ticks) != before {
                live.lock().expect("a chat's lock").world = chat.world.clone();
            }
            let outcome = outcome.map_err(|error| error.0)?;
            store
                .save(&mut live.lock().expect("a chat's lock"))
                .map_err(|e| e.message)?;
            let urls = |images: &[mcbench_tools::ImageRef]| -> Vec<Value> {
                images
                    .iter()
                    .map(|i| Value::String(i.url.clone()))
                    .collect()
            };
            let mut out = Map::new();
            out.insert(
                "content".into(),
                Value::String(message_content(&name, &outcome.result)),
            );
            out.insert("result".into(), outcome.result.clone());
            if let Some(shots) = &outcome.screenshots {
                out.insert(
                    "screenshots".into(),
                    json!({ "text": shots.text, "images": urls(&shots.images) }),
                );
            }
            if !outcome.images.is_empty() {
                out.insert("images".into(), Value::Array(urls(&outcome.images)));
            }
            Ok(Value::Object(out))
        }
        "turn" => {
            let store = open(text(request, "data"))?;
            let id = text(request, "chat").to_string();
            let live = store
                .conversation(&id)
                .map_err(|e| e.message)?
                .ok_or_else(|| format!("Conversation {id} not found"))?;
            let provider_key = {
                let mut conversation = live.lock().expect("a chat's lock");
                conversation.effort = field(request, "effort").as_str().map(str::to_string);
                let prompt = mcbench_store::Content::Text(text(request, "prompt").to_string());
                store
                    .append_message(
                        &mut conversation,
                        mcbench_store::NewMessage::prompt(&prompt, &[]),
                        None,
                    )
                    .map_err(|e| e.message)?;
                store.save(&mut conversation).map_err(|e| e.message)?;
                conversation.provider.clone()
            };
            // Everything that can refuse the turn first: the sandbox is
            // made last and shut down when the turn ends.
            let provider = adapter::provider(&store, &provider_key)?;
            let max_context_bytes = context::max_bytes_from(
                std::env::var("MCBENCH_CONTEXT_MAX_BYTES").ok().as_deref(),
            )?;
            let (tools, sandbox) = adapter::tools(&store).await?;
            let setup = TurnSetup {
                tools,
                provider,
                conversation: live,
                chat: flag(request, "turnChat"),
                placement: placement(request),
                network_access: flag(request, "networkAccess"),
                budgets,
                max_context_bytes,
                keep_exchanges: context::KEEP_EXCHANGES,
                archives: Arc::new(Archives::default()),
                work: Arc::new(WorkFolders {
                    root: store.folder().root.join("sandbox"),
                }),
                store: Arc::clone(&store),
            };
            let sink: SharedSink = Arc::new(Mutex::new(lines));
            let result =
                mcbench_agent::run_turn(&setup, &sink, &CancellationToken::new(), &mut || {}).await;
            mcbench_sandbox::Sandbox::shutdown(&*sandbox).await;
            store
                .save(&mut setup.conversation.lock().expect("a chat's lock"))
                .map_err(|e| e.message)?;
            match result {
                Ok(()) => Ok(json!({})),
                Err(error) => Ok(json!({
                    "failed": error.message,
                    "telemetry": error.telemetry.as_ref().map(|t| serde_json::to_value(t).expect("telemetry serializes")),
                    "reply": error.reply,
                })),
            }
        }
        other => fail(&format!("unknown op {other}")),
    }
}

/// Serves requests on stdin until it ends, one at a time, then exits 0.
pub(crate) fn run(arguments: &[String]) -> ! {
    if !arguments.is_empty() {
        fail(&format!("takes no arguments, not {}", arguments.join(" ")));
    }
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("a tokio runtime");
    let out = Arc::new(Mutex::new(std::io::stdout()));
    for line in std::io::stdin().lock().lines() {
        let line = line.unwrap_or_else(|e| fail(&e.to_string()));
        let Ok(Value::Object(request)) = serde_json::from_str::<Value>(&line) else {
            fail("a request line is a JSON object")
        };
        let id = field(&request, "id").clone();
        let op = text(&request, "op").to_string();
        let Some((_, keys)) = OPS.iter().find(|(name, _)| *name == op) else {
            fail(&format!("unknown op {op}"))
        };
        for key in request.keys() {
            if key != "id" && key != "op" && !keys.contains(&key.as_str()) {
                fail(&format!("{op} takes no {key}"));
            }
        }
        let lines = Lines {
            id: id.clone(),
            out: Arc::clone(&out),
        };
        let reply = runtime.block_on(answer(&op, &request, lines));
        let mut line = Map::new();
        line.insert("id".into(), id);
        match reply {
            Ok(Value::Object(fields)) => {
                line.insert("ok".into(), Value::Bool(true));
                line.extend(fields);
            }
            Ok(_) => unreachable!("a reply is an object"),
            Err(error) => {
                line.insert("error".into(), Value::String(error));
            }
        }
        let mut out = out.lock().expect("stdout");
        writeln!(out, "{}", mcbench_json::stringify(&Value::Object(line))).expect("stdout");
        out.flush().expect("stdout");
    }
    std::process::exit(0)
}
