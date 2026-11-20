//! Claude Code in headless print mode as a harness (`server/claude-code.js`):
//! Claude Code's own loop, system prompt and context management, with
//! mcbench's tools as its only tools over MCP (`--tools ""` removes every
//! built-in; the init event's tool list is checked). It runs on the host
//! under the Claude Code login, with no user, project or local settings.
//! Each chat keeps one Claude Code session that follow-up turns resume, a
//! turn that was stopped or failed included.
//!
//! One Claude Code process per chat outlives its turn: the next turn's
//! request is written to the same process (its stream-json input stays
//! open), which skips the CLI's start-up. A process is replaced when its
//! turn failed or was stopped (the next process resumes the session), when
//! the turn cannot continue the session or runs with other settings, and it
//! exits after sitting idle for ten minutes. A one-shot run ends after its
//! result.

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use async_trait::async_trait;
use serde_json::{Map, Value, json};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

use super::completion;
use super::process::{self, JsonLinesRun, Launch};
use super::relay::{Relay, State as RelayState, Waiter};
use super::{BridgeCommand, CommandSpec, Env, HarnessRequest, Record, Terms, text};
use crate::api::Api;
use crate::bridge::{self, BridgeHost, BridgeServer, McpContent, js_string, truthy};
use crate::catalog::Catalog;
use crate::{
    AssistantMessage, CompletionRequest, Delta, ModelFacts, ModelInfo, Provider, ProviderConfig,
    ProviderError, ProviderId, ToolSchema,
};

/// The harness's name in records and folders.
pub const NAME: &str = "claude-code";
/// The prefix of mcbench's tools as Claude Code names them.
pub const PREFIX: &str = "mcp__mcbench__";
/// Claude Code's task tools, left on in chats so its task list reaches the
/// interface. They only record a list.
pub const CHAT_BUILTINS: [&str; 4] = ["TaskCreate", "TaskUpdate", "TaskList", "TaskGet"];
/// How long a finished chat's process waits for its next turn.
pub const IDLE: Duration = Duration::from_millis(10 * 60_000);

/// Where and how Claude Code runs.
#[derive(Clone, Debug)]
pub struct ClaudeConfig {
    /// The data folder; session records are under `harness/`.
    pub data_dir: PathBuf,
    /// The Claude Code executable (`MCBENCH_CLAUDE_BINARY`, else `claude`
    /// on PATH, else the tools folder's: [`super::install::cli_program`]).
    pub binary: OsString,
    /// Claude Code's config folder when Settings names one; else the CLI's
    /// own default (`CLAUDE_CONFIG_DIR` from this process, or none).
    pub home: Option<PathBuf>,
    /// How the CLI reaches the bridge.
    pub bridge: BridgeCommand,
}

/// The one working folder of every Claude Code run (`claude-code.js`
/// `CWD`): outside any git work tree, so every chat gets the same system
/// prompt, and private to this account.
pub fn cwd() -> PathBuf {
    super::home_dir()
        .join(".cache")
        .join("mcbench")
        .join("claude-cwd")
}

/// `claude-code.js` `claudeArgs`. `session` is the id and whether it is
/// resumed, for a saved session; a one-shot run (`one_shot`) is streamed as
/// a chat turn is, never saved, and its system prompt replaces Claude
/// Code's own.
#[allow(clippy::too_many_arguments)]
pub fn claude_args(
    model: &str,
    effort: Option<&str>,
    system_prompt: &str,
    allowed_tools: &[String],
    bridge: (&str, &[String]),
    session: Option<(&str, bool)>,
    one_shot: bool,
    builtins: &[String],
) -> Vec<String> {
    let mcp = json!({ "mcpServers": { "mcbench": { "type": "stdio", "command": bridge.0, "args": bridge.1 } } });
    let mut args: Vec<String> = [
        "-p",
        "--output-format",
        "stream-json",
        "--verbose",
        "--model",
        model,
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .collect();
    if let Some(effort) = effort.filter(|e| !e.is_empty()) {
        args.extend(["--effort".to_owned(), effort.to_owned()]);
    }
    args.extend(["--thinking-display".to_owned(), "summarized".to_owned()]);
    args.extend([
        "--tools".to_owned(),
        builtins.join(","),
        "--strict-mcp-config".to_owned(),
    ]);
    args.extend(["--mcp-config".to_owned(), mcbench_json::stringify(&mcp)]);
    let allowed: Vec<String> = allowed_tools.iter().chain(builtins).cloned().collect();
    args.extend([
        "--allowedTools".to_owned(),
        allowed.join(","),
        "--permission-mode".to_owned(),
        "dontAsk".to_owned(),
    ]);
    args.extend([
        "--setting-sources".to_owned(),
        String::new(),
        "--disable-slash-commands".to_owned(),
    ]);
    if session.is_some() || one_shot {
        args.extend([
            "--include-partial-messages".to_owned(),
            "--input-format".to_owned(),
            "stream-json".to_owned(),
        ]);
    }
    match session {
        Some((id, resume)) => args.extend([
            if resume { "--resume" } else { "--session-id" }.to_owned(),
            id.to_owned(),
        ]),
        None => args.push("--no-session-persistence".to_owned()),
    }
    args.push(
        if one_shot {
            "--system-prompt"
        } else {
            "--append-system-prompt"
        }
        .to_owned(),
    );
    args.push(system_prompt.to_owned());
    args
}

impl ClaudeConfig {
    /// `configEnv`: the config folder the CLI is given, when there is one.
    fn config_env(&self) -> Vec<(&'static str, OsString)> {
        match self
            .home
            .clone()
            .map(OsString::from)
            .or_else(|| std::env::var_os("CLAUDE_CONFIG_DIR"))
        {
            Some(dir) => vec![("CLAUDE_CONFIG_DIR", dir)],
            None => Vec::new(),
        }
    }

    /// `configDir`: where the CLI's account is recorded.
    pub fn config_dir(&self) -> PathBuf {
        self.home
            .clone()
            .or_else(|| std::env::var_os("CLAUDE_CONFIG_DIR").map(PathBuf::from))
            .unwrap_or_else(super::home_dir)
    }

    /// `claudeEnv`: the login pays, not an API key; auto memory is off;
    /// tool arguments stream; the app names itself as the entrypoint.
    pub fn env(&self) -> Env {
        let mut extra = self.config_env();
        for (key, value) in [
            ("MCP_TOOL_TIMEOUT", "2147483647"),
            ("MCP_TIMEOUT", "120000"),
            ("MAX_MCP_OUTPUT_TOKENS", "200000"),
            ("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC", "1"),
            ("DISABLE_AUTOUPDATER", "1"),
            ("CLAUDE_CODE_ENABLE_FINE_GRAINED_TOOL_STREAMING", "1"),
            ("CLAUDE_CODE_ENTRYPOINT", "mcbench-lite"),
            ("CLAUDE_CODE_DISABLE_AUTO_MEMORY", "1"),
        ] {
            extra.push((key, OsString::from(value)));
        }
        super::cli_env(&extra)
    }

    /// `authCommand`: `claude auth login` and `claude auth logout`, whose
    /// `BROWSER` opens nothing.
    pub fn auth_command(&self, args: &[&str]) -> CommandSpec {
        let mut extra = self.config_env();
        extra.push(("BROWSER", OsString::from("true")));
        CommandSpec {
            file: self.binary.clone(),
            args: std::iter::once("auth")
                .chain(args.iter().copied())
                .map(str::to_owned)
                .collect(),
            env: super::cli_env(&extra),
        }
    }

    /// `transcriptFile`: where Claude Code files a session's transcript,
    /// under its config folder (the one Settings names, else `~/.claude`)
    /// and the working folder's name with every character but a letter or
    /// digit made a dash.
    pub fn transcript_file(&self, session_id: &str) -> PathBuf {
        let dashed: String = cwd()
            .to_string_lossy()
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect();
        self.home
            .clone()
            .unwrap_or_else(|| super::home_dir().join(".claude"))
            .join("projects")
            .join(dashed)
            .join(format!("{session_id}.jsonl"))
    }

    /// `oauthAccount`: the account `claude auth login` recorded.
    fn oauth_account(&self) -> Result<Option<Value>, String> {
        let file = self.config_dir().join(".claude.json");
        match std::fs::read_to_string(&file) {
            Ok(text) => Ok(crate::api::v8json::parse(&text)?
                .get("oauthAccount")
                .filter(|a| truthy(Some(a)))
                .cloned()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(format!("{}: {e}", file.display())),
        }
    }

    /// `storedLogin`.
    pub fn stored_login(&self) -> Result<bool, String> {
        Ok(self.oauth_account()?.is_some())
    }

    /// `account`: who is signed in and on what plan (`claude_max` with
    /// `default_claude_max_20x` reads `Claude Max 20x`), as `{ email, plan }`.
    pub fn account(&self) -> Result<Option<Value>, String> {
        let Some(entry) = self.oauth_account()? else {
            return Ok(None);
        };
        let organization = entry
            .get("organizationType")
            .and_then(Value::as_str)
            .unwrap_or("");
        let tier = organization
            .strip_prefix("claude_")
            .filter(|t| !t.is_empty() && t.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'));
        let limit = entry
            .get("organizationRateLimitTier")
            .and_then(Value::as_str)
            .unwrap_or("");
        let multiplier = limit.rsplit_once('_').map(|(_, m)| m).filter(|m| {
            m.len() > 1 && m.ends_with('x') && m[..m.len() - 1].chars().all(|c| c.is_ascii_digit())
        });
        let plan = tier.map(|tier| {
            let mut words = vec!["Claude".to_owned()];
            let mut chars = tier.chars();
            let first = chars.next().unwrap();
            words.push(first.to_uppercase().collect::<String>() + chars.as_str());
            if let Some(m) = multiplier {
                words.push(m.to_owned());
            }
            words.join(" ")
        });
        let email = entry
            .get("emailAddress")
            .filter(|e| truthy(Some(e)))
            .cloned()
            .unwrap_or(Value::Null);
        Ok(Some(json!({ "email": email, "plan": plan })))
    }
}

/// `checkInit`: the init event lists only mcbench's tools (and the chat's
/// built-ins), served by a connected server.
pub fn check_init(event: &Value, allowed: &[String], builtins: &[String]) -> Result<(), String> {
    let tools = event
        .get("tools")
        .and_then(Value::as_array)
        .ok_or_else(|| "Cannot read properties of undefined (reading 'filter')".to_owned())?;
    let extra: Vec<String> = tools
        .iter()
        .map(js_string)
        .filter(|t| !allowed.contains(t) && !builtins.contains(t))
        .collect();
    if !extra.is_empty() {
        return Err(format!("{}{}", text::OUTSIDE_MCBENCH, extra.join(", ")));
    }
    let servers = event
        .get("mcp_servers")
        .and_then(Value::as_array)
        .ok_or_else(|| "Cannot read properties of undefined (reading 'find')".to_owned())?;
    let server = servers
        .iter()
        .find(|s| s.get("name") == Some(&json!("mcbench")));
    let status = server.and_then(|s| s.get("status"));
    if status != Some(&json!("connected")) {
        let status = status
            .map(js_string)
            .unwrap_or_else(|| "undefined".to_owned());
        return Err(format!("{}{status}", text::SERVER_STATUS));
    }
    Ok(())
}

/// `usageOf`: Claude Code's thinking tokens where the app's telemetry reads
/// reasoning tokens.
pub fn usage_of(result: &Value) -> Result<Value, String> {
    let usage = match result.get("usage") {
        None => return Err("Cannot destructure property 'output_tokens_details' of 'result.usage' as it is undefined.".to_owned()),
        Some(Value::Null) => return Err("Cannot destructure property 'output_tokens_details' of 'result.usage' as it is null.".to_owned()),
        Some(usage) => usage,
    };
    let mut out = Map::new();
    let mut details = None;
    if let Value::Object(usage) = usage {
        for (key, value) in usage {
            if key == "output_tokens_details" {
                details = Some(value);
            } else {
                out.insert(key.clone(), value.clone());
            }
        }
    }
    if let Some(thinking) = details.and_then(|d| d.get("thinking_tokens")) {
        out.insert(
            "output_tokens_details".to_owned(),
            json!({ "reasoning_tokens": thinking }),
        );
    }
    Ok(Value::Object(out))
}

/// `contextTokensOf`: one request's fresh, cache-written and cache-read input.
pub fn context_tokens_of(usage: &Value) -> Value {
    super::js_sum(&[
        usage.get("input_tokens"),
        usage.get("cache_creation_input_tokens"),
        usage.get("cache_read_input_tokens"),
    ])
}

/// `streamInput`: one stream-json user message, the prompt text and images.
pub fn stream_input(text: &str, images: &[super::PromptImage]) -> String {
    let mut content = vec![json!({ "type": "text", "text": text })];
    for image in images {
        content.push(json!({ "type": "image", "source": { "type": "base64", "media_type": image.mime_type, "data": image.data } }));
    }
    mcbench_json::stringify(
        &json!({ "type": "user", "message": { "role": "user", "content": content } }),
    ) + "\n"
}

/// JavaScript's `a.b` where `a` may be undefined or null (a TypeError).
fn read<'a>(base: Option<&'a Value>, key: &str) -> Result<Option<&'a Value>, String> {
    match base {
        None => Err(format!(
            "Cannot read properties of undefined (reading '{key}')"
        )),
        Some(Value::Null) => Err(format!("Cannot read properties of null (reading '{key}')")),
        Some(value) => Ok(value.get(key)),
    }
}

/// JavaScript's `for (const x of value)`.
fn iterate<'a>(value: Option<&'a Value>, what: &str) -> Result<Vec<&'a Value>, String> {
    match value {
        Some(Value::Array(items)) => Ok(items.iter().collect()),
        Some(Value::String(_)) => Ok(Vec::new()),
        None => Err(format!("{what} is not iterable")),
        Some(other) => Err(format!("{what} is not iterable ({other})")),
    }
}

fn same(a: &Value, b: &Value) -> bool {
    match (a.as_f64(), b.as_f64()) {
        (Some(x), Some(y)) => x == y,
        _ => a == b,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProcState {
    Spawning,
    Busy,
    Idle,
    Dead,
    Exited,
}

impl ProcState {
    fn name(self) -> &'static str {
        match self {
            ProcState::Spawning => "spawning",
            ProcState::Busy => "busy",
            ProcState::Idle => "idle",
            ProcState::Dead => "dead",
            ProcState::Exited => "exited",
        }
    }
}

struct Session {
    relay: Relay,
    process: Option<u64>,
    allowed: Vec<String>,
    builtins: Vec<String>,
    /// The chat's id when the turn has a transcript (records are written).
    conversation: Option<String>,
    request: Option<String>,
    terms: Terms,
    tasks: Option<Vec<(Value, Value)>>,
    pending_task_updates: Option<Vec<(Value, Value)>>,
    closed: CancellationToken,
}

struct Proc {
    conversation_id: Option<String>,
    key: String,
    state: ProcState,
    session: Option<u64>,
    session_id: Option<Value>,
    idle: u64,
    tool_uses: HashMap<String, watch::Sender<bool>>,
    blocks: Vec<(Value, String)>,
    run: Option<JsonLinesRun>,
    server: Option<BridgeServer>,
    runtime_dir: Option<PathBuf>,
    exited: watch::Sender<bool>,
}

impl Proc {
    fn to(&mut self, state: ProcState) -> Result<(), String> {
        use ProcState::*;
        let legal = match self.state {
            Spawning => matches!(state, Busy | Dead | Exited),
            Busy => matches!(state, Idle | Dead | Exited),
            Idle => matches!(state, Busy | Dead | Exited),
            Dead => matches!(state, Exited),
            Exited => false,
        };
        if !legal {
            return Err(format!(
                "A Claude Code process that is {} cannot become {}",
                self.state.name(),
                state.name()
            ));
        }
        self.state = state;
        Ok(())
    }

    /// `toolUse`: resolved once stdout shows the call's `tool_use` block.
    fn tool_use(&mut self, id: &str) -> watch::Receiver<bool> {
        self.tool_uses
            .entry(id.to_owned())
            .or_insert_with(|| watch::channel(false).0)
            .subscribe()
    }

    fn resolve_tool_use(&mut self, id: &str) {
        self.tool_uses
            .entry(id.to_owned())
            .or_insert_with(|| watch::channel(false).0)
            .send_replace(true);
    }
}

#[derive(Default)]
struct Core {
    /// The run of each chat (`sessionRelay`'s map).
    by_chat: HashMap<String, u64>,
    sessions: HashMap<u64, Session>,
    procs: HashMap<u64, Proc>,
    /// The chat's process (`processes`).
    warm: HashMap<String, u64>,
    next: u64,
}

/// Where Claude Code's model list and facts are read: the public catalogue
/// (`llm.js` `listModels` and `modelInfo` for `claude-code`).
#[derive(Clone)]
pub struct Models {
    /// The catalogue.
    pub catalog: Arc<Catalog>,
    /// The API side it reads with.
    pub api: Arc<Api>,
    /// The provider's resolved settings.
    pub config: ProviderConfig,
}

struct Inner {
    config: ClaudeConfig,
    core: Mutex<Core>,
    models: Option<Models>,
}

/// The Claude Code provider.
pub struct ClaudeCode {
    inner: Arc<Inner>,
}

impl ClaudeCode {
    /// The provider; `models` is where its model list is read from.
    pub fn new(config: ClaudeConfig, models: Option<Models>) -> Self {
        ClaudeCode {
            inner: Arc::new(Inner {
                config,
                core: Mutex::new(Core::default()),
                models,
            }),
        }
    }

    fn models_source(&self) -> Result<&Models, ProviderError> {
        self.inner.models.as_ref().ok_or_else(|| {
            completion::failure("Claude Code's models come from the model catalogue, which this provider was not given".to_owned())
        })
    }

    /// The configuration it runs with.
    pub fn config(&self) -> &ClaudeConfig {
        &self.inner.config
    }

    /// `claude --version`.
    pub async fn version(&self) -> Result<String, String> {
        let out = tokio::time::timeout(
            Duration::from_millis(5000),
            tokio::process::Command::from(mcbench_spawn::command(&self.inner.config.binary))
                .arg("--version")
                .kill_on_drop(true)
                .output(),
        )
        .await
        .map_err(|_| {
            format!(
                "Command failed: {} --version (timed out)",
                self.inner.config.binary.to_string_lossy()
            )
        })?
        .map_err(|e| {
            format!(
                "Command failed: {} --version: {e}",
                self.inner.config.binary.to_string_lossy()
            )
        })?;
        if !out.status.success() {
            return Err(format!(
                "Command failed: {} --version\n{}",
                self.inner.config.binary.to_string_lossy(),
                String::from_utf8_lossy(&out.stderr)
            ));
        }
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
    }

    /// One completion of a turn (`completion`): delivers the stored result
    /// of the chat's pending call, or ends any previous run and starts one.
    /// The message is today's module's (before `chatCompletion`'s
    /// telemetry).
    pub async fn complete_turn(
        &self,
        request: &CompletionRequest,
        on_delta: &mut (dyn FnMut(Delta) + Send),
        cancel: &CancellationToken,
    ) -> Result<Map<String, Value>, ProviderError> {
        let request = HarnessRequest::from_completion(request).map_err(completion::failure)?;
        self.run_turn(&request, on_delta, cancel).await
    }

    async fn run_turn(
        &self,
        request: &HarnessRequest,
        on_delta: &mut (dyn FnMut(Delta) + Send),
        cancel: &CancellationToken,
    ) -> Result<Map<String, Value>, ProviderError> {
        let (waiter, deltas, done) = Waiter::new(true);
        let fresh = {
            let mut core = self.inner.core.lock().unwrap();
            let key = request.conversation_id().map(str::to_owned);
            let existing = key.as_ref().and_then(|k| core.by_chat.get(k).copied());
            let pending = existing
                .and_then(|sid| core.sessions.get(&sid))
                .and_then(|s| s.relay.pending_id().map(str::to_owned));
            let result = match pending {
                Some(id) => {
                    super::pending_result(&request.values(), &id).map_err(completion::failure)?
                }
                None => None,
            };
            if let (Some(result), Some(sid)) = (result, existing) {
                let delivered = core.sessions.get_mut(&sid).unwrap().relay.deliver(result);
                settle(&mut core, sid);
                delivered.map_err(completion::failure)?;
                core.sessions.get_mut(&sid).unwrap().relay.next(waiter);
                settle(&mut core, sid);
                None
            } else {
                if let Some(sid) = existing {
                    if let Some(session) = core.sessions.get_mut(&sid) {
                        session.relay.close();
                    }
                    settle(&mut core, sid);
                }
                let relay = Relay::new(
                    "Claude Code",
                    cancel.is_cancelled().then_some(text::ABORTED),
                )
                .map_err(completion::failure)?;
                core.next += 1;
                let sid = core.next;
                let closed = CancellationToken::new();
                core.sessions.insert(
                    sid,
                    Session {
                        relay,
                        process: None,
                        allowed: Vec::new(),
                        builtins: Vec::new(),
                        conversation: None,
                        request: None,
                        terms: Terms {
                            instructions: String::new(),
                            resolution: String::new(),
                        },
                        tasks: None,
                        pending_task_updates: None,
                        closed: closed.clone(),
                    },
                );
                if let Some(key) = &key {
                    core.by_chat.insert(key.clone(), sid);
                }
                let weak = Arc::downgrade(&self.inner);
                completion::watch_abort(cancel, closed, move || {
                    if let Some(inner) = weak.upgrade() {
                        let mut core = inner.core.lock().unwrap();
                        if let Some(session) = core.sessions.get_mut(&sid) {
                            session.relay.abort(text::ABORTED);
                        }
                        settle(&mut core, sid);
                    }
                });
                // The completion waits from the run's start: what the CLI
                // prints as soon as it runs (a warm process answers the
                // request written to it at once) streams to it, as Node's
                // `next` is set before any of the CLI's output is read.
                core.sessions.get_mut(&sid).unwrap().relay.next(waiter);
                Some(sid)
            }
        };
        if let Some(sid) = fresh {
            let started = start(&self.inner, sid, request, cancel).await;
            let mut core = self.inner.core.lock().unwrap();
            if let Err(error) = started {
                if let Some(session) = core.sessions.get_mut(&sid) {
                    session.relay.close();
                }
                settle(&mut core, sid);
                return Err(completion::failure(error));
            }
            settle(&mut core, sid);
        }
        completion::read_answer(deltas, done, on_delta)
            .await
            .map_err(completion::failure)
    }

    /// `closeSession`: ends the chat's run, if any.
    pub fn close_chat(&self, conversation_id: &str) {
        let mut core = self.inner.core.lock().unwrap();
        if let Some(sid) = core.by_chat.get(conversation_id).copied() {
            if let Some(session) = core.sessions.get_mut(&sid) {
                session.relay.close();
            }
            settle(&mut core, sid);
        }
    }

    /// The chat's warm process exits; its session record goes, then (once
    /// the process has exited) its transcript in Claude Code's folder
    /// (`harness.destroy` for this harness).
    pub async fn destroy(&self, conversation_id: &str) -> Result<(), String> {
        let config = &self.inner.config;
        let record = super::read_session(&config.data_dir, conversation_id, NAME)?;
        let exits = {
            let mut core = self.inner.core.lock().unwrap();
            let ids: Vec<u64> = core
                .procs
                .iter()
                .filter(|(_, p)| p.conversation_id.as_deref() == Some(conversation_id))
                .map(|(id, _)| *id)
                .collect();
            ids.into_iter()
                .filter_map(|id| kill(&mut core, id))
                .collect::<Vec<_>>()
        };
        let dir = super::conversation_dir(&config.data_dir, conversation_id);
        let _ = std::fs::remove_dir_all(&dir);
        for mut exit in exits {
            let _ = exit.wait_for(|e| *e).await;
        }
        if let Some(record) = record {
            let session = record
                .get("sessionId")
                .map(js_string)
                .unwrap_or_else(|| "undefined".to_owned());
            let file = config.transcript_file(&session);
            if let Err(e) = std::fs::remove_file(&file)
                && e.kind() != std::io::ErrorKind::NotFound
            {
                return Err(format!("rm {}: {e}", file.display()));
            }
        }
        Ok(())
    }

    /// Kills every process; resolves once all have exited.
    pub async fn shutdown(&self) {
        let exits = {
            let mut core = self.inner.core.lock().unwrap();
            let ids: Vec<u64> = core.procs.keys().copied().collect();
            ids.into_iter()
                .filter_map(|id| kill(&mut core, id))
                .collect::<Vec<_>>()
        };
        for mut exit in exits {
            let _ = exit.wait_for(|e| *e).await;
        }
    }

    /// The chat's warm process's state, for tests: `idle`, `busy`, ...
    #[doc(hidden)]
    pub fn process_state(&self, conversation_id: &str) -> Option<&'static str> {
        let core = self.inner.core.lock().unwrap();
        core.warm
            .get(conversation_id)
            .and_then(|id| core.procs.get(id))
            .map(|p| p.state.name())
    }

    /// How many processes have not exited, for tests.
    #[doc(hidden)]
    pub fn live_processes(&self) -> usize {
        self.inner.core.lock().unwrap().procs.len()
    }
}

impl Drop for ClaudeCode {
    fn drop(&mut self) {
        let mut core = self.inner.core.lock().unwrap();
        let ids: Vec<u64> = core.procs.keys().copied().collect();
        for id in ids {
            kill(&mut core, id);
        }
    }
}

#[async_trait]
impl Provider for ClaudeCode {
    fn id(&self) -> ProviderId {
        ProviderId::ClaudeCode
    }

    async fn complete(
        &self,
        request: &CompletionRequest,
        on_delta: &mut (dyn FnMut(Delta) + Send),
        cancel: &CancellationToken,
    ) -> Result<AssistantMessage, ProviderError> {
        let tracked = completion::Tracked::start(ProviderId::ClaudeCode, request);
        let reply = {
            let mut noted = |delta: Delta| {
                tracked.note(&delta);
                on_delta(delta);
            };
            self.complete_turn(request, &mut noted, cancel).await
        };
        tracked.finish(reply, cancel)
    }

    async fn models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        let models = self.models_source()?;
        models
            .catalog
            .list_models(&models.api, &models.config)
            .await
    }

    async fn model_info(&self, model: &str) -> Result<ModelFacts, ProviderError> {
        let models = self.models_source()?;
        models
            .catalog
            .model_info(&models.api, &models.config, model)
            .await
    }

    async fn close_session(&self, conversation_id: &str) {
        self.close_chat(conversation_id);
    }
}

/// After a relay operation: a run that closed stops its process when it was
/// running (`ClaudeCodeSession.stop`), and is forgotten.
fn settle(core: &mut Core, sid: u64) {
    let Some(session) = core.sessions.get_mut(&sid) else {
        return;
    };
    let Some(was_running) = session.relay.take_close() else {
        return;
    };
    session.closed.cancel();
    let process = session.process;
    core.sessions.remove(&sid);
    core.by_chat.retain(|_, s| *s != sid);
    if was_running && let Some(pid) = process {
        kill(core, pid);
    }
}

/// `kill`: the process is stopped; resolves (the receiver) once it has
/// exited and its files are gone.
fn kill(core: &mut Core, pid: u64) -> Option<watch::Receiver<bool>> {
    let proc = core.procs.get_mut(&pid)?;
    let exited = proc.exited.subscribe();
    if matches!(proc.state, ProcState::Dead | ProcState::Exited) {
        return Some(exited);
    }
    let started = proc.state != ProcState::Spawning;
    proc.to(ProcState::Dead).expect("any live process can die");
    proc.idle += 1;
    let conversation = proc.conversation_id.clone();
    let run = proc.run.clone();
    if let Some(conversation) = conversation
        && core.warm.get(&conversation) == Some(&pid)
    {
        core.warm.remove(&conversation);
    }
    if started && let Some(run) = run {
        run.stop();
    }
    Some(exited)
}

/// `exit`: the process is gone, its bridge stopped and its runtime folder
/// removed.
fn exit(core: &mut Core, pid: u64) {
    let Some(mut proc) = core.procs.remove(&pid) else {
        return;
    };
    let _ = proc.to(ProcState::Exited);
    proc.idle += 1;
    if let Some(conversation) = &proc.conversation_id
        && core.warm.get(conversation) == Some(&pid)
    {
        core.warm.remove(conversation);
    }
    if let Some(server) = proc.server.take() {
        server.stop();
    }
    if let Some(dir) = &proc.runtime_dir {
        let _ = std::fs::remove_dir_all(dir);
    }
    proc.exited.send_replace(true);
}

fn record_session(
    config: &ClaudeConfig,
    conversation: &str,
    session_id: Option<&Value>,
    session: &Session,
) -> Result<(), String> {
    let mut record: Record = Map::new();
    if let Some(id) = session_id {
        record.insert("sessionId".to_owned(), id.clone());
    }
    record.insert("cwd".to_owned(), json!(cwd().to_string_lossy()));
    record.insert("instructions".to_owned(), json!(session.terms.instructions));
    record.insert("resolution".to_owned(), json!(session.terms.resolution));
    if let Some(request) = &session.request {
        record.insert("lastSeen".to_owned(), json!(request));
    }
    let tasks: Vec<Value> = session
        .tasks
        .iter()
        .flatten()
        .map(|(k, v)| json!([k, v]))
        .collect();
    record.insert("tasks".to_owned(), Value::Array(tasks));
    super::write_session(&config.data_dir, conversation, NAME, &record)
}

/// `ClaudeCodeSession.start`.
async fn start(
    inner: &Arc<Inner>,
    sid: u64,
    request: &HarnessRequest,
    cancel: &CancellationToken,
) -> Result<(), String> {
    let config = &inner.config;
    let messages = request.values();
    let allowed: Vec<String> = request
        .tool_names()
        .iter()
        .map(|n| format!("{PREFIX}{n}"))
        .collect();
    let builtins: Vec<String> = if request.chat {
        CHAT_BUILTINS.iter().map(|s| (*s).to_owned()).collect()
    } else {
        Vec::new()
    };
    let conversation_id = request.conversation_id().map(str::to_owned);
    let transcript = request.transcript();
    let request_id = transcript.and_then(|t| t.last()).map(|m| m.id.clone());
    let system_prompt = super::system_text(&messages);
    let terms = super::session_terms(&messages);
    let record = match &conversation_id {
        Some(id) => super::read_session(&config.data_dir, id, NAME)?,
        None => None,
    };
    let cwd_text = cwd().to_string_lossy().into_owned();
    let resume = match (transcript, &record) {
        (Some(transcript), Some(record)) => {
            record.get("cwd") == Some(&json!(cwd_text))
                && record.get("instructions") == Some(&json!(terms.instructions))
                && super::session_continues(Some(record), transcript)
        }
        _ => false,
    };
    if let Some(id) = &conversation_id {
        super::forget_session(&config.data_dir, id, NAME)?;
    }
    // Rendered before anything is opened, so a bad message leaks nothing.
    let (prompt, images) = if resume {
        let record = record.as_ref().unwrap();
        let transcript = transcript.unwrap();
        let mut notes = super::stopped_reply_note(record, transcript);
        notes.extend(super::resolution_note(record, &messages));
        super::harness_prompt(&messages, false, notes)?
    } else {
        super::harness_prompt(&messages, true, Vec::new())?
    };
    let input = stream_input(&prompt, &images);
    let key = mcbench_json::stringify(&json!([
        request.model,
        request.effort,
        terms.instructions,
        allowed,
        builtins
    ]));
    let record_session_id = record.as_ref().and_then(|r| r.get("sessionId").cloned());
    let tasks: Option<Vec<(Value, Value)>> = if resume {
        Some(match record.as_ref().unwrap().get("tasks") {
            Some(Value::Array(entries)) => entries
                .iter()
                .map(|e| {
                    (
                        e.get(0).cloned().unwrap_or(Value::Null),
                        e.get(1).cloned().unwrap_or(Value::Null),
                    )
                })
                .collect(),
            _ => Vec::new(),
        })
    } else {
        None
    };
    let pid = {
        let mut core = inner.core.lock().unwrap();
        let Some(session) = core.sessions.get_mut(&sid) else {
            return Err(text::ABORTED.to_owned());
        };
        session.allowed = allowed.clone();
        session.builtins = builtins.clone();
        session.conversation = if transcript.is_some() {
            conversation_id.clone()
        } else {
            None
        };
        session.request = request_id;
        session.terms = terms.clone();
        if resume {
            session.tasks = tasks;
        }
        let warm = conversation_id
            .as_ref()
            .and_then(|c| core.warm.get(c).copied());
        if let Some(wid) = warm {
            let reuse = core.procs.get(&wid).is_some_and(|w| {
                w.state == ProcState::Idle
                    && resume
                    && w.key == key
                    && w.session_id == record_session_id
            });
            if reuse {
                core.sessions.get_mut(&sid).unwrap().process = Some(wid);
                let proc = core.procs.get_mut(&wid).unwrap();
                proc.to(ProcState::Busy)?;
                proc.idle += 1;
                proc.session = Some(sid);
                // Blocks from the last turn whose MCP call never came.
                proc.tool_uses.clear();
                proc.blocks.clear();
                if let Some(run) = &proc.run {
                    run.write(input.into_bytes());
                }
                return Ok(());
            }
            kill(&mut core, wid);
        }
        create_private_dir(&cwd())?;
        core.next += 1;
        let pid = core.next;
        core.procs.insert(
            pid,
            Proc {
                conversation_id: conversation_id.clone(),
                key,
                state: ProcState::Spawning,
                session: Some(sid),
                session_id: None,
                idle: 0,
                tool_uses: HashMap::new(),
                blocks: Vec::new(),
                run: None,
                server: None,
                runtime_dir: None,
                exited: watch::channel(false).0,
            },
        );
        if let Some(c) = &conversation_id {
            core.warm.insert(c.clone(), pid);
        }
        core.sessions.get_mut(&sid).unwrap().process = Some(pid);
        pid
    };
    let session_arg = conversation_id.as_ref().map(|_| {
        if resume {
            (
                record_session_id
                    .as_ref()
                    .map(js_string)
                    .unwrap_or_else(|| "undefined".to_owned()),
                true,
            )
        } else {
            (super::random_uuid(), false)
        }
    });
    let spawned = spawn(
        inner,
        pid,
        cancel,
        &request.tools,
        |socket| {
            let (command, args) = config.bridge.for_address(socket);
            claude_args(
                &request.model,
                request.effort.as_deref(),
                &system_prompt,
                &allowed,
                (&command, &args),
                session_arg
                    .as_ref()
                    .map(|(id, resume)| (id.as_str(), *resume)),
                conversation_id.is_none(),
                &builtins,
            )
        },
        input,
    )
    .await;
    if let Err(error) = spawned {
        let mut core = inner.core.lock().unwrap();
        exit(&mut core, pid);
        return Err(error);
    }
    Ok(())
}

fn create_private_dir(dir: &Path) -> Result<(), String> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder
        .create(dir)
        .map_err(|e| format!("mkdir {}: {e}", dir.display()))
}

/// The bridge's host for one process: a call waits for its `tool_use`
/// block on stdout, then goes to the process's running turn.
struct Host {
    inner: Weak<Inner>,
    pid: u64,
    tools: Vec<ToolSchema>,
}

/// Cancels a call that is dropped before it was answered.
struct CallGuard {
    inner: Weak<Inner>,
    sid: u64,
    id: String,
    serial: u64,
    answered: bool,
}

impl Drop for CallGuard {
    fn drop(&mut self) {
        if self.answered {
            return;
        }
        if let Some(inner) = self.inner.upgrade() {
            let mut core = inner.core.lock().unwrap();
            if let Some(session) = core.sessions.get_mut(&self.sid) {
                session.relay.cancel_call(&self.id, self.serial);
            }
        }
    }
}

#[async_trait]
impl BridgeHost for Host {
    fn tools(&self) -> Vec<ToolSchema> {
        self.tools.clone()
    }

    async fn call(
        &self,
        name: &str,
        arguments: Value,
        _call_id: &str,
        tool_use_id: Option<&str>,
        _cancel: &CancellationToken,
    ) -> Result<Vec<McpContent>, String> {
        let id = tool_use_id.ok_or_else(|| text::NO_TOOL_USE_ID.to_owned())?;
        let inner = self
            .inner
            .upgrade()
            .ok_or_else(|| text::NO_TURN.to_owned())?;
        let mut seen = {
            let mut core = inner.core.lock().unwrap();
            match core.procs.get_mut(&self.pid) {
                Some(proc) => proc.tool_use(id),
                None => watch::channel(false).1,
            }
        };
        if seen.wait_for(|seen| *seen).await.is_err() {
            // Its entry was cleared (a later turn began): it never resolves.
            std::future::pending::<()>().await;
        }
        let (answer, guard) = {
            let mut core = inner.core.lock().unwrap();
            let Some(proc) = core.procs.get_mut(&self.pid) else {
                return Err(text::NO_TURN.to_owned());
            };
            proc.tool_uses.remove(id);
            if proc.state != ProcState::Busy {
                return Err(text::NO_TURN.to_owned());
            }
            let sid = proc.session.expect("a busy process has a turn");
            let session = core
                .sessions
                .get_mut(&sid)
                .ok_or_else(|| text::TURN_ENDED.to_owned())?;
            let (answer, serial) = session.relay.call(id, name, arguments);
            (
                answer,
                CallGuard {
                    inner: self.inner.clone(),
                    sid,
                    id: id.to_owned(),
                    serial,
                    answered: false,
                },
            )
        };
        let mut guard = guard;
        let answer = answer
            .await
            .unwrap_or_else(|_| Err(text::TURN_ENDED.to_owned()));
        guard.answered = true;
        answer.map(|blocks| blocks.into_iter().map(McpContent).collect())
    }
}

/// `ClaudeProcess.spawn`: its bridge, then the CLI. A kill while the bridge
/// starts ends it before the CLI is spawned.
async fn spawn(
    inner: &Arc<Inner>,
    pid: u64,
    cancel: &CancellationToken,
    tools: &[ToolSchema],
    args: impl FnOnce(&str) -> Vec<String>,
    input: String,
) -> Result<(), String> {
    let runtime_dir = super::runtime_dir()?;
    let socket = runtime_dir.join("mcp.sock");
    {
        let mut core = inner.core.lock().unwrap();
        match core.procs.get_mut(&pid) {
            Some(proc) => proc.runtime_dir = Some(runtime_dir.clone()),
            // Gone already: its exit cannot remove a folder it never had.
            None => {
                let _ = std::fs::remove_dir_all(&runtime_dir);
                return Err(text::STOPPED_WHILE_STARTING.to_owned());
            }
        }
    }
    let address = super::bridge_address(&socket);
    let host = Arc::new(Host {
        inner: Arc::downgrade(inner),
        pid,
        tools: tools.to_vec(),
    });
    let server = bridge::serve(&address, host)?;
    {
        let mut core = inner.core.lock().unwrap();
        if let Some(proc) = core.procs.get_mut(&pid) {
            proc.server = Some(server);
        }
    }
    // As `await serveMcp` does: a stop that came meanwhile is seen here.
    tokio::task::yield_now().await;
    let mut core = inner.core.lock().unwrap();
    let proc = core
        .procs
        .get_mut(&pid)
        .ok_or_else(|| text::STOPPED_WHILE_STARTING.to_owned())?;
    if proc.state == ProcState::Dead || cancel.is_cancelled() {
        return Err(text::STOPPED_WHILE_STARTING.to_owned());
    }
    proc.to(ProcState::Busy)?;
    let env = inner.config.env();
    let args = args(&address.to_arg());
    let weak = Arc::downgrade(inner);
    let on_line = {
        let weak = weak.clone();
        Box::new(move |event: Value| -> Result<(), String> {
            let Some(inner) = weak.upgrade() else {
                return Ok(());
            };
            let mut core = inner.core.lock().unwrap();
            line(&inner, &mut core, pid, event)
        })
    };
    let done = Box::new(move |outcome: Result<process::Exit, String>| {
        let Some(inner) = weak.upgrade() else { return };
        let mut core = inner.core.lock().unwrap();
        let error = match outcome {
            Ok(exit) => format!(
                "Claude Code exited {} without a result: {}",
                exit.code_or_signal(),
                exit.stderr
            ),
            Err(error) => error,
        };
        // Only a process that dies mid-turn fails its turn; a killed one's
        // turn has already ended.
        let busy = core
            .procs
            .get(&pid)
            .filter(|p| p.state == ProcState::Busy)
            .and_then(|p| p.session);
        if let Some(sid) = busy {
            if let Some(session) = core.sessions.get_mut(&sid) {
                session.relay.finish(Some(error), None);
            }
            settle(&mut core, sid);
        }
        exit(&mut core, pid);
    });
    let run = process::run_json_lines(
        Launch {
            name: "Claude Code",
            command: &inner.config.binary,
            args: &args,
            cwd: &cwd(),
            env: &env,
        },
        input.into_bytes(),
        true,
        on_line,
        Box::new(|| {}),
        done,
    )?;
    core.procs.get_mut(&pid).unwrap().run = Some(run);
    Ok(())
}

const TASK_STATUS: [(&str, &str); 3] = [
    ("pending", "pending"),
    ("in_progress", "active"),
    ("completed", "done"),
];

/// `relayEvent`: Claude Code's stream events onto the relay: activity,
/// notices, its task list and usage-limit state.
fn relay_event(session: &mut Session, event: &Value) -> Result<(), String> {
    let kind = event.get("type").and_then(Value::as_str);
    let relay = &mut session.relay;
    match kind {
        Some("system") => {
            let subtype = event.get("subtype").and_then(Value::as_str).unwrap_or("");
            match subtype {
                "status" => relay.status(
                    (event.get("status") == Some(&json!("compacting")))
                        .then(|| text::COMPACTING.to_owned()),
                ),
                "api_retry" => {
                    let status = event.get("error_status");
                    let after = if truthy(status) {
                        format!("{}{}", text::RETRYING.1, js_string(status.unwrap()))
                    } else {
                        String::new()
                    };
                    let field = |k: &str| {
                        event
                            .get(k)
                            .map(js_string)
                            .unwrap_or_else(|| "undefined".to_owned())
                    };
                    relay.status(Some(format!(
                        "{}{after}{}{}{}{}{}",
                        text::RETRYING.0,
                        text::RETRYING.2,
                        field("attempt"),
                        text::RETRYING.3,
                        field("max_retries"),
                        text::RETRYING.4
                    )));
                }
                "compact_boundary" => relay.notice("info", Some(&json!(text::COMPACTED))),
                "model_fallback" | "model_refusal_fallback" | "informational" => relay.notice(
                    if event.get("level") == Some(&json!("warning")) {
                        "warning"
                    } else {
                        "info"
                    },
                    event.get("content"),
                ),
                "notification" => relay.notice(
                    if event.get("color") == Some(&json!("error")) {
                        "error"
                    } else {
                        "warning"
                    },
                    event.get("text"),
                ),
                _ => {}
            }
        }
        Some("rate_limit_event") => relay.set_rate_limit(event.get("rate_limit_info").cloned()),
        Some("assistant") => {
            let message = event.get("message");
            let usage = read(message, "usage")?;
            let tokens = match usage {
                None => {
                    return Err(
                        "Cannot read properties of undefined (reading 'input_tokens')".to_owned(),
                    );
                }
                Some(Value::Null) => {
                    return Err(
                        "Cannot read properties of null (reading 'input_tokens')".to_owned()
                    );
                }
                Some(usage) => context_tokens_of(usage),
            };
            relay.set_context_tokens(tokens);
            let content = message
                .and_then(|m| m.get("content"))
                .filter(|c| truthy(Some(c)));
            let blocks = match content {
                None => Vec::new(),
                Some(content) => iterate(Some(content), "event.message?.content")?,
            };
            for block in blocks {
                if block.get("type") == Some(&json!("tool_use"))
                    && block.get("name") == Some(&json!("TaskUpdate"))
                {
                    let updates = session.pending_task_updates.get_or_insert_with(Vec::new);
                    let id = block.get("id").cloned().unwrap_or(Value::Null);
                    let input = block.get("input").cloned().unwrap_or(Value::Null);
                    match updates.iter_mut().find(|(k, _)| same(k, &id)) {
                        Some(slot) => slot.1 = input,
                        None => updates.push((id, input)),
                    }
                }
            }
        }
        Some("user") if truthy(event.get("tool_use_result")) => {
            let tasks = session.tasks.get_or_insert_with(Vec::new);
            let result = &event["tool_use_result"];
            let call_id = match event.get("message").and_then(|m| m.get("content")) {
                Some(Value::Array(blocks)) => blocks
                    .iter()
                    .find(|b| b.get("type") == Some(&json!("tool_result")))
                    .and_then(|b| b.get("tool_use_id")),
                _ => None,
            };
            let task = result.get("task");
            let task_id = task.and_then(|t| t.get("id")).filter(|id| truthy(Some(id)));
            if let Some(id) = task_id {
                let mut entry = Map::new();
                if let Some(subject) = task.and_then(|t| t.get("subject")) {
                    entry.insert("text".to_owned(), subject.clone());
                }
                entry.insert("status".to_owned(), json!("pending"));
                match tasks.iter_mut().find(|(k, _)| same(k, id)) {
                    Some(slot) => slot.1 = Value::Object(entry),
                    None => tasks.push((id.clone(), Value::Object(entry))),
                }
            }
            let update = call_id.filter(|c| truthy(Some(c))).and_then(|c| {
                session
                    .pending_task_updates
                    .as_ref()
                    .and_then(|u| u.iter().find(|(k, _)| same(k, c)))
                    .map(|(_, v)| v.clone())
            });
            let update = update.filter(|u| truthy(Some(u)));
            let update_task = update
                .as_ref()
                .and_then(|u| u.get("taskId").cloned())
                .unwrap_or(Value::Null);
            let applies = update.is_some()
                && truthy(result.get("success"))
                && tasks.iter().any(|(k, _)| same(k, &update_task));
            if applies {
                let update = update.unwrap();
                let position = tasks
                    .iter()
                    .position(|(k, _)| same(k, &update_task))
                    .unwrap();
                if update.get("status") == Some(&json!("deleted")) {
                    tasks.remove(position);
                } else {
                    let existing = tasks[position].1.clone();
                    let mut entry = Map::new();
                    let subject = update
                        .get("subject")
                        .filter(|s| truthy(Some(s)))
                        .or_else(|| existing.get("text"));
                    if let Some(subject) = subject {
                        entry.insert("text".to_owned(), subject.clone());
                    }
                    let status = update
                        .get("status")
                        .and_then(Value::as_str)
                        .and_then(|s| TASK_STATUS.iter().find(|(k, _)| *k == s))
                        .map(|(_, v)| json!(v))
                        .or_else(|| existing.get("status").cloned());
                    if let Some(status) = status {
                        entry.insert("status".to_owned(), status);
                    }
                    tasks[position].1 = Value::Object(entry);
                }
            }
            if task_id.is_some() || applies {
                let plan: Vec<Value> = tasks.iter().map(|(_, v)| v.clone()).collect();
                relay.set_plan(Value::Array(plan));
            }
        }
        _ => {}
    }
    Ok(())
}

/// `ClaudeProcess.line`: one stdout event, while the process runs a turn.
fn line(inner: &Arc<Inner>, core: &mut Core, pid: u64, event: Value) -> Result<(), String> {
    let Some(proc) = core.procs.get(&pid) else {
        return Ok(());
    };
    if proc.state != ProcState::Busy {
        return Ok(());
    }
    let Some(sid) = proc.session else {
        return Ok(());
    };
    let kind = event.get("type").and_then(Value::as_str);
    if kind == Some("system") && event.get("subtype") == Some(&json!("init")) {
        let session = core
            .sessions
            .get_mut(&sid)
            .ok_or_else(|| text::TURN_ENDED.to_owned())?;
        check_init(&event, &session.allowed, &session.builtins)?;
        session.relay.started();
        // Claude Code has taken this turn's request into its session, so
        // the next turn resumes the session however this one ends.
        if let Some(conversation) = session.conversation.clone() {
            record_session(
                &inner.config,
                &conversation,
                event.get("session_id"),
                session,
            )?;
        }
    }
    if kind == Some("assistant") {
        let content = read(event.get("message"), "content")?;
        for block in iterate(content, "event.message.content")? {
            let name = block.get("name").and_then(Value::as_str);
            if block.get("type") == Some(&json!("tool_use"))
                && name.is_some_and(|n| n.starts_with(PREFIX))
            {
                let id = block
                    .get("id")
                    .map(js_string)
                    .unwrap_or_else(|| "undefined".to_owned());
                core.procs.get_mut(&pid).unwrap().resolve_tool_use(&id);
            }
        }
    }
    let Some(session) = core.sessions.get_mut(&sid) else {
        return Ok(());
    };
    relay_event(session, &event)?;
    if kind == Some("stream_event") {
        let stream = match event.get("event") {
            None => {
                return Err(
                    "Cannot destructure property 'type' of 'event.event' as it is undefined."
                        .to_owned(),
                );
            }
            Some(Value::Null) => {
                return Err(
                    "Cannot destructure property 'type' of 'event.event' as it is null.".to_owned(),
                );
            }
            Some(stream) => stream,
        };
        let stream_type = stream.get("type").and_then(Value::as_str);
        let index = stream.get("index").cloned().unwrap_or(Value::Null);
        let block = stream.get("content_block");
        let delta = stream.get("delta");
        let proc = core.procs.get_mut(&pid).unwrap();
        if stream_type == Some("message_start") {
            proc.blocks.clear();
        }
        if stream_type == Some("content_block_start") {
            let block_type = read(block, "type")?;
            let name = block.and_then(|b| b.get("name")).and_then(Value::as_str);
            if block_type == Some(&json!("tool_use")) {
                let name = name.ok_or_else(|| {
                    "Cannot read properties of undefined (reading 'startsWith')".to_owned()
                })?;
                if let Some(tool) = name.strip_prefix(PREFIX) {
                    let id = block
                        .and_then(|b| b.get("id"))
                        .map(js_string)
                        .unwrap_or_else(|| "undefined".to_owned());
                    match proc.blocks.iter_mut().find(|(k, _)| same(k, &index)) {
                        Some(slot) => slot.1 = id.clone(),
                        None => proc.blocks.push((index.clone(), id.clone())),
                    }
                    let at = index.as_f64().unwrap_or(0.0) as u32;
                    session.relay.draft_tool_call(&id, at, tool);
                }
            }
        }
        let delta_type = delta.and_then(|d| d.get("type")).and_then(Value::as_str);
        match delta_type {
            Some("text_delta") => {
                let text = delta
                    .and_then(|d| d.get("text"))
                    .filter(|t| truthy(Some(t)))
                    .map(js_string);
                session.relay.update(text.as_deref(), None, None);
            }
            Some("thinking_delta") => {
                let text = delta
                    .and_then(|d| d.get("thinking"))
                    .filter(|t| truthy(Some(t)))
                    .map(js_string);
                session.relay.update(None, text.as_deref(), None);
            }
            Some("input_json_delta") => {
                if let Some((_, id)) = proc.blocks.iter().find(|(k, _)| same(k, &index)) {
                    let partial = delta
                        .and_then(|d| d.get("partial_json"))
                        .map(js_string)
                        .unwrap_or_else(|| "undefined".to_owned());
                    session.relay.draft_args(&id.clone(), &partial)?;
                }
            }
            _ => {}
        }
    }
    if kind == Some("result") {
        result(inner, core, pid, sid, &event);
    }
    settle(core, sid);
    Ok(())
}

/// `ClaudeProcess.result`: the turn's result always finishes its run; the
/// process then idles only if the turn completed. Its record is written
/// again with the task list as the turn left it.
fn result(inner: &Arc<Inner>, core: &mut Core, pid: u64, sid: u64, event: &Value) {
    core.procs.get_mut(&pid).unwrap().session = None;
    let Some(session) = core.sessions.get_mut(&sid) else {
        return;
    };
    let checked = if truthy(event.get("is_error")) {
        Err(format!(
            "Claude Code {}: {}",
            event
                .get("subtype")
                .map(js_string)
                .unwrap_or_else(|| "undefined".to_owned()),
            match event.get("result") {
                None | Some(Value::Null) => String::new(),
                Some(r) => js_string(r),
            }
        ))
    } else {
        usage_of(event)
    };
    let usage = match checked {
        Ok(usage) => usage,
        Err(error) => {
            session.relay.finish(Some(error), None);
            settle(core, sid);
            kill(core, pid);
            return;
        }
    };
    session.relay.update(None, None, Some(usage));
    let session_id = event.get("session_id").cloned();
    let conversation = session.conversation.clone();
    let config = inner.config.clone();
    let save = |s: &Session| -> Result<(), String> {
        match &conversation {
            Some(c) => record_session(&config, c, session_id.as_ref(), s),
            None => Ok(()),
        }
    };
    // `save` reads the session as the relay's finish leaves it.
    let snapshot = Session {
        relay: Relay::new("Claude Code", None).unwrap(),
        process: None,
        allowed: Vec::new(),
        builtins: Vec::new(),
        conversation: session.conversation.clone(),
        request: session.request.clone(),
        terms: session.terms.clone(),
        tasks: session.tasks.clone(),
        pending_task_updates: None,
        closed: CancellationToken::new(),
    };
    let has_conversation = conversation.is_some();
    let mut saver = || save(&snapshot);
    session.relay.finish(
        None,
        if has_conversation {
            Some(&mut saver)
        } else {
            None
        },
    );
    let failed =
        session.relay.outcome_error().is_some() || session.relay.state == RelayState::Running;
    settle(core, sid);
    let proc = core.procs.get_mut(&pid).unwrap();
    proc.session_id = session_id;
    if proc.conversation_id.is_none() || failed {
        kill(core, pid);
        return;
    }
    let _ = proc.to(ProcState::Idle);
    proc.idle += 1;
    let generation = proc.idle;
    let weak = Arc::downgrade(inner);
    tokio::spawn(async move {
        tokio::time::sleep(IDLE).await;
        if let Some(inner) = weak.upgrade() {
            let mut core = inner.core.lock().unwrap();
            if core.procs.get(&pid).is_some_and(|p| p.idle == generation) {
                kill(&mut core, pid);
            }
        }
    });
}
