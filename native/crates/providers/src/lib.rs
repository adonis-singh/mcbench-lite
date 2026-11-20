//! # mcbench-providers
//!
//! Model providers behind ONE trait, [`Provider`]: the API providers
//! (OpenRouter, OpenAI, the ChatGPT sign-in's `codex`, Anthropic, Groq,
//! Google, QwenCloud) with today's request shapes ([`api`]), and the
//! harnesses run as the providers' own CLIs in child processes (Claude Code
//! and Codex CLI). Cursor is left out of the native app for now by the
//! owner's decision of 2026-11-17 (docs/native-app.md, Cursor, deferred). A
//! CLI reaches mcbench's tools through the app's own tool bridge
//! ([`bridge`]): the app's binary started as
//! `mcbench-lite tool-bridge <address>`, an ordinary stdio MCP server, relaying to the main process
//! over a Unix socket (macOS, Linux) or a named pipe (Windows). Nothing
//! listens on a TCP port.
//!
//! It replaces `server/llm.js`, `codex.js` (with the `openai-oauth` 2.0.0
//! package it runs in-process: `@openai-oauth/core` and `local`),
//! `qwencloud.js`, `provider-login.js`, `harness.js`, `claude-code.js`,
//! `codex-cli.js`, `cursor.js` (`@cursor/sdk`, which has no Rust
//! equivalent), `mcp.js`, `cli-install.js` and `mcp-bridge.py`, and
//! `completion-retry.js`.
//!
//! | Module | Today's source |
//! |---|---|
//! | [`api`] | `llm.js` (the API providers, `chatCompletion`'s telemetry), `codex.js`, `qwencloud.js`, `openai-oauth` |
//! | [`catalog`] | `llm.js` (the OpenRouter catalog, `listModels`, `modelInfo`), `codex-cli.js` `listModels` |
//! | [`retry`] | `completion-retry.js` |
//! | [`login`] | `provider-login.js` (the sign-in flows, without the HTTP routes) |
//! | [`changes`] | none: the page polled a sign-in and an install; the app is told of each change |
//!
//! Text that reaches models here is today's, copied byte for byte from its
//! sources: `server/harness.js` 59-122, `claude-code.js` 79-90,
//! `codex-cli.js` 94-127, `cursor.js` 134 and `mcp.js` 41-45 (the
//! inventory's list), and the request bodies of [`api`] (the Anthropic and
//! Codex conversions' fixed text, `[End of images]`). The benchmark's
//! tool-contract hash does not cover it; each is pinned by a golden test or
//! by the differential test against the text `main`'s files hold.
//!
//! ## Owner
//!
//! The stage-three session.
//!
//! ## Seams
//!
//! | Seam | What crosses | Who calls whom | Thread or process |
//! |---|---|---|---|
//! | agent | [`CompletionRequest`] in; [`Delta`]s while it streams; [`AssistantMessage`] or [`ProviderError`] out; [`ModelInfo`] for the turn's tools | `mcbench-agent`'s turn loop calls [`Provider::complete`] (through [`retry::complete_with_retry`] for an API provider) and [`Provider::model_info`] | the async runtime; a CLI provider's session is a child process kept across the calls of one turn |
//! | the bridge | MCP JSON-RPC 2.0, newline-delimited (`initialize`, `ping`, `tools/list`, `tools/call`, `notifications/cancelled`), as `server/mcp.js` serves it | the CLI starts `mcbench-lite tool-bridge`, which relays bytes to [`bridge::serve`] in the main process, which calls a [`bridge::BridgeHost`] | a child process per CLI session; the relay is a socket or pipe |
//! | store | provider keys and sign-ins (`mcbench-store`'s `Settings` JSON), resolved by the core | the core passes them in [`ProviderConfig`] | the async runtime |
//! | setup | the CLIs' pinned downloads (`server/tools.js` pins Claude Code 2.1.311 and Codex 0.169.0 today) | `mcbench-setup` installs, this crate runs | setup tasks |
//! | interface | a sign-in's [`login::SignInView`] and an install's [`harness::install::InstallView`], each change of either pushed to the watcher of [`changes::Changes`] | the core calls [`login::SignIns`] and [`harness::install::CliInstaller`], watches them (`watch`) and shows each view; the window opens the address only on the person's click | the async runtime; the watcher is called on the thread that made the change |
//!
//! ## Behaviour defined by
//!
//! Class 8 and the provider half of two class-1 and class-9 files
//! (docs/native-app.md, Appendix A), 141 tests in 16 files:
//! `claude-code.test.js` 21, `claude-code-transcript.test.js` 2,
//! `codex-cli.test.js` 14, `codex-cli-shims.test.js` 2, `codex.test.js` 13,
//! `cursor.test.js` 13 (not ported: Cursor is deferred), `harness.test.js`
//! 16, `llm.test.js` 20,
//! `llm-catalog.test.js` 2, `llm-catalog-refresh.test.js` 1,
//! `llm-google.test.js` 1, `qwencloud.test.js` 5, `completion-retry.test.js`
//! 3, `cli-install.test.js` 5 (with `mcbench-setup`),
//! `provider-login.test.js` 12 (the login flows) and `settings.test.js` 11
//! (with `mcbench-store`). Their fakes (fake HTTP providers, fake CLIs) are
//! pointed at the Rust binary in step 3b.
//!
//! The API providers are also held by a differential test
//! (`tests/differential.rs`, `tests/node/`): the same conversations through
//! today's `llm.js` and through [`api`] against one replay server, whose
//! request logs, deltas and assistant messages must be identical.
//!
//! ## Decided
//!
//! Cursor: the native app has no Cursor provider at launch (the owner,
//! 2026-11-17: "leave cursor out for now"; docs/native-app.md, Cursor,
//! deferred, holds what was checked and the routes back). `cursor.js` and
//! its tests are not ported. [`ProviderId::Cursor`] stays as a name, because
//! stored chats and settings carry it: no [`Provider`] is built for it, and
//! a turn in such a chat is refused with a sentence saying Cursor is not in
//! the native app. [`Provider`] makes no assumption that a provider is an
//! HTTP API, so a provider run as an external program can be added later
//! without changing the seam.
//!
//! [`ApiMessage`] is an order-preserving JSON object, the one `agent.js`
//! `apiMessages` makes (the stage-three session's decision, `native/README.md`,
//! The typed message): request bodies are byte-identical to Node's, so the
//! `prompts` and `stopped` fields that reach chat-completions providers today
//! still do.
//!
//! [`Provider`]'s calls and [`bridge::BridgeHost::call`] are `async fn` on
//! the core's tokio runtime (`native/README.md`, Decisions of stage three),
//! cancelled by a [`CancellationToken`]. The HTTP client is `reqwest` on
//! `rustls` with ring's crypto ([`api`] says why and what Node's fetch does
//! that it repeats); the bridge listens with tokio's Unix sockets and
//! Windows named pipes.
//!
//! JSON that Node wrote (request bodies, `contextBytes`, the Codex login
//! file) is written with `mcbench-json`, as `JSON.stringify` writes it, and
//! JSON a provider sends is read with `mcbench-json`'s `JSON.parse`.
//!
//! Where the API providers differ from Node, each in a case no provider
//! produces or a user notices: header names go out in lower case (undici
//! keeps a caller's case); a JavaScript string cut inside a surrogate pair
//! (a 400-character error excerpt, a V8 `JSON.parse` message quoting one
//! code unit) holds U+FFFD where Node holds the half; `Date.parse` is read
//! for ISO 8601 text only (Anthropic's `created_at` and the login's
//! `last_refresh`; a missing `created_at` is 0 here where V8 reads
//! `Date.parse(0)` as 2000-01-01 local time); a Chat Completions call index
//! that skips one, and Codex arguments for a call that was never added,
//! fail with an error where Node streams a hole or an index-less delta; the
//! OAuth bridge's own requests (npm, the catalogue, the refresh), which have
//! no timeout today, time out after a year.
//!
//! The harnesses ([`harness`]) use [`api::Tracker`] for their telemetry
//! (through `Provider::complete`; `complete_turn` gives today's module's
//! message before it), [`catalog::CodexModels`] for the Codex CLI's
//! catalogue, [`catalog::Catalog`] for Claude Code's list and facts,
//! [`api::v8json`] for the CLIs' stdout lines (a line that is not JSON fails
//! with V8's message), and implement [`login::LoginPrograms`]
//! ([`harness::Logins`]): one codepath with the API side. They are held by a
//! differential of their own (`tests/diff/`, 55 cases, 83 turns, through
//! today's `claude-code.js` and `codex-cli.js` and through the Rust
//! providers against one fake CLI) and by `tests/golden/` (Node's answers
//! for the pure functions and for the bridge's protocol steps).
//!
//! [`CompletionRequest::transcript`] carries the stored transcript a
//! harness resumes from, so [`Provider::complete`] is the one call the agent
//! loop makes for every provider; the CLI's start reaches the turn as
//! [`Delta::Started`]. One [`catalog::CodexModels`] (the catalog's) serves
//! the ChatGPT sign-in and the Codex CLI.
//!
//! ## Open
//!
//! On Linux, Codex's sandboxed launch is `mcbench-sandbox`'s
//! (`harness::codex_cli`'s one `todo!`, `SandboxedCodex`).
//! `cargo check` for the Windows targets stops in ring's build script on a
//! Linux host without the MSVC tools and the Windows SDK's C headers; the
//! Windows VM or a CI runner checks it (the bridge's named pipes and the
//! harnesses' Windows paths were compiled for no Windows target).

use std::path::PathBuf;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tokio_util::sync::CancellationToken;

pub mod api;
pub mod catalog;
pub mod changes;
pub mod login;
pub mod retry;

#[cfg(test)]
mod fake;
#[cfg(test)]
mod scratch;
#[cfg(test)]
mod tests;

/// A provider's key, as today's settings and stored chats name it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ProviderId {
    /// `openrouter`.
    #[serde(rename = "openrouter")]
    OpenRouter,
    /// `openai`.
    #[serde(rename = "openai")]
    OpenAi,
    /// `codex`: the ChatGPT sign-in's Responses API.
    #[serde(rename = "codex")]
    Codex,
    /// `anthropic`.
    #[serde(rename = "anthropic")]
    Anthropic,
    /// `groq`.
    #[serde(rename = "groq")]
    Groq,
    /// `google`.
    #[serde(rename = "google")]
    Google,
    /// `qwencloud`.
    #[serde(rename = "qwencloud")]
    QwenCloud,
    /// `cursor`: a name only; no provider is built for it (see Decided).
    #[serde(rename = "cursor")]
    Cursor,
    /// `claude-code`: the Claude Code CLI.
    #[serde(rename = "claude-code")]
    ClaudeCode,
    /// `codex-cli`: the Codex CLI.
    #[serde(rename = "codex-cli")]
    CodexCli,
}

impl ProviderId {
    /// Whether the provider is a harness: a whole agent session in a child
    /// process that retries its own requests, so its completion runs once
    /// (`agent.js` `runTurn`).
    pub fn is_harness(self) -> bool {
        matches!(
            self,
            ProviderId::Cursor | ProviderId::ClaudeCode | ProviderId::CodexCli
        )
    }

    /// The key settings and chats store (`openrouter`, `claude-code`, ...).
    pub fn key(self) -> &'static str {
        match self {
            ProviderId::OpenRouter => "openrouter",
            ProviderId::OpenAi => "openai",
            ProviderId::Codex => "codex",
            ProviderId::Anthropic => "anthropic",
            ProviderId::Groq => "groq",
            ProviderId::Google => "google",
            ProviderId::QwenCloud => "qwencloud",
            ProviderId::Cursor => "cursor",
            ProviderId::ClaudeCode => "claude-code",
            ProviderId::CodexCli => "codex-cli",
        }
    }

    /// The name Settings shows (`settings.js` `PROVIDERS[id].label`), which
    /// the errors of a provider that is off or not connected name.
    pub fn label(self) -> &'static str {
        match self {
            ProviderId::OpenRouter => "OpenRouter",
            ProviderId::OpenAi => "OpenAI",
            ProviderId::Codex => "ChatGPT",
            ProviderId::Anthropic => "Anthropic",
            ProviderId::Groq => "Groq",
            ProviderId::Google => "Google",
            ProviderId::QwenCloud => "QwenCloud",
            ProviderId::Cursor => "Cursor",
            ProviderId::ClaudeCode => "Claude Code",
            ProviderId::CodexCli => "Codex CLI",
        }
    }

    /// Whether it signs in with a CLI's login rather than a key
    /// (`settings.js` `auth: 'cli-login'`).
    pub fn cli_login(self) -> bool {
        matches!(
            self,
            ProviderId::Codex | ProviderId::ClaudeCode | ProviderId::CodexCli
        )
    }
}

/// One message of the request: the JSON object `agent.js` `apiMessages`
/// makes, in its key order (see Decided). Each protocol reads what
/// `docs`' inventory lists (`$S/payload/INVENTORY.md` section 5): chat
/// completions send it as it is minus `seeBuild` and the assistant's
/// `reasoning`, `reasoning_content`, `reasoning_details` and `finish_reason`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ApiMessage(pub Map<String, Value>);

/// One tool offered, the exact JSON text of today's schema (`agent.js`
/// `toolsFor`), kept as text so its bytes are the ones the tool-contract
/// hash covers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolSchema(pub String);

/// What the provider is given for one completion (`llm.js`
/// `chatCompletion`'s options).
#[derive(Clone, Debug)]
pub struct CompletionRequest {
    /// The chat: a harness resumes its own session for it, and the ChatGPT
    /// sign-in sends it as the prompt cache key and session headers. `None`
    /// for a one-shot (a title, a prompt rewrite).
    pub conversation_id: Option<String>,
    /// The model id.
    pub model: String,
    /// The reasoning effort, when one is chosen (`conversation.effort ||
    /// undefined`: an empty effort is `None`).
    pub effort: Option<String>,
    /// The messages, oldest first.
    pub messages: Vec<ApiMessage>,
    /// The tools offered.
    pub tools: Vec<ToolSchema>,
    /// A chat turn (the user's prompt library is offered) rather than a
    /// benchmark attempt.
    pub chat: bool,
    /// The chat's stored transcript (`chatCompletion`'s `conversation`),
    /// oldest first, whose last message is this turn's request: a harness
    /// resumes its session from it and records the session against it
    /// (`harness.js` `sessionContinues`, `stoppedReplyNote`). `None` runs a
    /// harness without resuming or recording one, as today with no
    /// `conversation`; it needs `conversation_id`. An API provider ignores
    /// it.
    pub transcript: Option<Vec<harness::TranscriptEntry>>,
}

/// One call the model made, as `llm.js` returns it and `agent.js` stores it:
/// `{id, type, function: {name, arguments}, extra_content?}`.
#[derive(Clone, Debug, PartialEq)]
pub struct ToolCall {
    /// The provider's call id.
    pub id: String,
    /// `type`: `function` for every provider today.
    pub kind: String,
    /// The tool's name.
    pub name: String,
    /// The arguments as the model wrote them (JSON text, parsed by the
    /// agent, which words the error a model reads when it is not an object).
    pub arguments: String,
    /// Gemini's thought signature (`extra_content`), which must go back with
    /// the call on the next request.
    pub extra_content: Option<Value>,
}

impl ToolCall {
    /// The call as today's object, in its key order.
    pub fn to_json(&self) -> Value {
        let mut function = Map::new();
        function.insert("name".into(), Value::String(self.name.clone()));
        function.insert("arguments".into(), Value::String(self.arguments.clone()));
        let mut call = Map::new();
        call.insert("id".into(), Value::String(self.id.clone()));
        call.insert("type".into(), Value::String(self.kind.clone()));
        call.insert("function".into(), Value::Object(function));
        if let Some(extra) = &self.extra_content {
            call.insert("extra_content".into(), extra.clone());
        }
        Value::Object(call)
    }
}

/// What streams while a completion runs (`agent.js` `onDelta`).
#[derive(Clone, Debug, PartialEq)]
pub enum Delta {
    /// The model is under way (a harness's CLI reported its start).
    ///
    /// Today's rule (`agent.js` `onStarted`, a call of its own beside
    /// `onDelta`): it is not a delta of the reply, so `chatCompletion`'s
    /// `firstDeltaMs` does not count it ([`api::Tracker`] and the harnesses'
    /// telemetry skip it).
    Started,
    /// Text of the answer (`{content}`).
    Content(String),
    /// Reasoning text (`{reasoning}`).
    Reasoning(String),
    /// Part of a tool call as it streams (`{toolCall: {index, id, name,
    /// args}}`).
    ToolCall {
        /// The call's index in the message.
        index: u32,
        /// Its id so far (`''` until the provider sent it).
        id: String,
        /// Its name so far (`''` until the provider sent it).
        name: String,
        /// More of its arguments.
        arguments: String,
    },
    /// A harness's activity line, or none.
    Status(Option<String>),
    /// A harness's notice (`harness.js` `notice`): level and text.
    Notice {
        /// `info`, `warning` or `error`.
        level: String,
        /// The text.
        text: String,
    },
    /// A harness's task list.
    Plan(Value),
}

impl Delta {
    /// The delta as the object `llm.js` passes to `onDelta` (`{content}`,
    /// `{reasoning}`, `{toolCall}`, `{status}`, `{notice}`, `{plan}`), or
    /// `None` for [`Delta::Started`], which is a call of its own today.
    pub fn to_json(&self) -> Option<Value> {
        let mut out = Map::new();
        match self {
            Delta::Started => return None,
            Delta::Content(text) => {
                out.insert("content".into(), Value::String(text.clone()));
            }
            Delta::Reasoning(text) => {
                out.insert("reasoning".into(), Value::String(text.clone()));
            }
            Delta::ToolCall {
                index,
                id,
                name,
                arguments,
            } => {
                let mut call = Map::new();
                call.insert("index".into(), Value::from(*index));
                call.insert("id".into(), Value::String(id.clone()));
                call.insert("name".into(), Value::String(name.clone()));
                call.insert("args".into(), Value::String(arguments.clone()));
                out.insert("toolCall".into(), Value::Object(call));
            }
            Delta::Status(text) => {
                out.insert(
                    "status".into(),
                    text.clone().map(Value::String).unwrap_or(Value::Null),
                );
            }
            Delta::Notice { level, text } => {
                let mut notice = Map::new();
                notice.insert("level".into(), Value::String(level.clone()));
                notice.insert("text".into(), Value::String(text.clone()));
                out.insert("notice".into(), Value::Object(notice));
            }
            Delta::Plan(plan) => {
                out.insert("plan".into(), plan.clone());
            }
        }
        Some(Value::Object(out))
    }
}

/// `telemetry.usage` (`llm.js` `normalizeUsage`): each count, or `None`
/// where the provider did not report it (never read as zero). Numbers are
/// JavaScript numbers, written with `mcbench-json`.
#[derive(Clone, Debug, PartialEq)]
pub struct Usage {
    /// `inputTokens` (Anthropic's adds its cache reads and writes).
    pub input_tokens: Option<f64>,
    /// `outputTokens`.
    pub output_tokens: Option<f64>,
    /// `cachedInputTokens`.
    pub cached_input_tokens: Option<f64>,
    /// `cacheWriteTokens`.
    pub cache_write_tokens: Option<f64>,
    /// `reasoningTokens`.
    pub reasoning_tokens: Option<f64>,
}

impl Usage {
    /// `{inputTokens, outputTokens, cachedInputTokens, cacheWriteTokens,
    /// reasoningTokens}`.
    pub fn to_json(&self) -> Value {
        let number = |value: Option<f64>| match value {
            Some(x) => serde_json::Number::from_f64(x)
                .map(Value::Number)
                .expect("a finite count"),
            None => Value::Null,
        };
        let mut out = Map::new();
        out.insert("inputTokens".into(), number(self.input_tokens));
        out.insert("outputTokens".into(), number(self.output_tokens));
        out.insert("cachedInputTokens".into(), number(self.cached_input_tokens));
        out.insert("cacheWriteTokens".into(), number(self.cache_write_tokens));
        out.insert("reasoningTokens".into(), number(self.reasoning_tokens));
        Value::Object(out)
    }
}

/// How a completion ended, the telemetry's last fields.
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    /// It answered: `contextTokens`, how much of the model's context the
    /// request filled (the usage's input for an API provider; what a
    /// harness reports, or null).
    Completed {
        /// `contextTokens`.
        context_tokens: Option<f64>,
    },
    /// It failed: `failed: true` and `cancelled` (the turn was stopped).
    Failed {
        /// `cancelled`.
        cancelled: bool,
    },
}

/// A completion's telemetry (`llm.js` `chatCompletion`), which the agent
/// stores on the assistant message and the core on an error row.
#[derive(Clone, Debug, PartialEq)]
pub struct Telemetry {
    /// `provider`.
    pub provider: ProviderId,
    /// `model`.
    pub model: String,
    /// `effort`, or null.
    pub effort: Option<String>,
    /// `startedAt`, as `Date.prototype.toISOString` writes it.
    pub started_at: String,
    /// `wallMs`.
    pub wall_ms: u64,
    /// `firstDeltaMs`: when the first delta came, or null.
    pub first_delta_ms: Option<u64>,
    /// `contextBytes`: the UTF-8 bytes of `JSON.stringify(messages)`.
    pub context_bytes: u64,
    /// `messageCount`.
    pub message_count: u64,
    /// `usage`, or null (always null for a failure).
    pub usage: Option<Usage>,
    /// How it ended.
    pub outcome: Outcome,
}

impl Telemetry {
    /// The object in today's key order: `{provider, model, effort,
    /// startedAt, wallMs, firstDeltaMs, contextBytes, messageCount, usage,
    /// contextTokens}`, or `{..., usage: null, failed: true, cancelled}`.
    pub fn to_json(&self) -> Value {
        let mut out = Map::new();
        out.insert("provider".into(), Value::String(self.provider.key().into()));
        out.insert("model".into(), Value::String(self.model.clone()));
        out.insert(
            "effort".into(),
            self.effort
                .clone()
                .map(Value::String)
                .unwrap_or(Value::Null),
        );
        out.insert("startedAt".into(), Value::String(self.started_at.clone()));
        out.insert("wallMs".into(), Value::from(self.wall_ms));
        out.insert(
            "firstDeltaMs".into(),
            self.first_delta_ms.map(Value::from).unwrap_or(Value::Null),
        );
        out.insert("contextBytes".into(), Value::from(self.context_bytes));
        out.insert("messageCount".into(), Value::from(self.message_count));
        out.insert(
            "usage".into(),
            self.usage
                .as_ref()
                .map(Usage::to_json)
                .unwrap_or(Value::Null),
        );
        match &self.outcome {
            Outcome::Completed { context_tokens } => {
                out.insert(
                    "contextTokens".into(),
                    context_tokens
                        .and_then(serde_json::Number::from_f64)
                        .map(Value::Number)
                        .unwrap_or(Value::Null),
                );
            }
            Outcome::Failed { cancelled } => {
                out.insert("failed".into(), Value::Bool(true));
                out.insert("cancelled".into(), Value::Bool(*cancelled));
            }
        }
        Value::Object(out)
    }
}

/// The assistant message a completion ends with: every field a provider
/// returns today. The agent stores it with [`AssistantMessage::to_json`], in
/// `agent.js` 662-673's key order: `role`, `content`, `reasoning`,
/// `reasoning_details`, `finish_reason`, `telemetry`, `notices`, `plan`,
/// `rateLimit`, `tool_calls` (each optional one only when present and, as
/// there, `reasoning` only when not empty).
#[derive(Clone, Debug, PartialEq)]
pub struct AssistantMessage {
    /// The answer's text.
    pub content: String,
    /// The reasoning text, when there was any.
    pub reasoning: Option<String>,
    /// The provider's reasoning items kept for replay (Anthropic's signed
    /// thinking blocks, the Responses API's reasoning items), verbatim.
    pub reasoning_details: Option<Vec<Value>>,
    /// Why the model stopped, as the provider said it.
    pub finish_reason: Option<String>,
    /// The telemetry.
    pub telemetry: Telemetry,
    /// A harness's notices.
    pub notices: Option<Vec<Value>>,
    /// A harness's task list.
    pub plan: Option<Value>,
    /// Claude Code's `rate_limit_info`, verbatim.
    pub rate_limit: Option<Value>,
    /// The calls the model made.
    pub tool_calls: Vec<ToolCall>,
}

impl AssistantMessage {
    /// The message as `agent.js` stores it (without `createdAt`, which the
    /// store adds), in that order.
    pub fn to_json(&self) -> Map<String, Value> {
        let mut out = Map::new();
        out.insert("role".into(), Value::String("assistant".into()));
        out.insert("content".into(), Value::String(self.content.clone()));
        if let Some(reasoning) = self.reasoning.as_ref().filter(|r| !r.is_empty()) {
            out.insert("reasoning".into(), Value::String(reasoning.clone()));
        }
        if let Some(details) = &self.reasoning_details {
            out.insert("reasoning_details".into(), Value::Array(details.clone()));
        }
        if let Some(reason) = self.finish_reason.as_ref().filter(|r| !r.is_empty()) {
            out.insert("finish_reason".into(), Value::String(reason.clone()));
        }
        out.insert("telemetry".into(), self.telemetry.to_json());
        if let Some(notices) = &self.notices {
            out.insert("notices".into(), Value::Array(notices.clone()));
        }
        if let Some(plan) = &self.plan {
            out.insert("plan".into(), plan.clone());
        }
        if let Some(rate_limit) = &self.rate_limit {
            out.insert("rateLimit".into(), rate_limit.clone());
        }
        if !self.tool_calls.is_empty() {
            out.insert(
                "tool_calls".into(),
                Value::Array(self.tool_calls.iter().map(ToolCall::to_json).collect()),
            );
        }
        out
    }
}

/// Why a completion failed: the message the user and the transcript show,
/// whether `completion-retry.js` may retry it, and what `llm.js` hangs on
/// the error for the stored error row (`error.telemetry`, `error.reply`).
#[derive(Clone, Debug, PartialEq)]
pub struct ProviderError {
    /// The message.
    pub message: String,
    /// Retryable ([`retry::is_transient`]: a rate limit or a transient
    /// failure of an API provider).
    pub retryable: bool,
    /// The failed completion's telemetry; `None` when it failed before it
    /// started (the provider is off or not connected, an unknown model list).
    pub telemetry: Option<Box<Telemetry>>,
    /// The reply text streamed before it failed (a stopped turn stores what
    /// the user saw), `''` when none.
    pub reply: String,
}

impl ProviderError {
    /// An error with `message`, retryable as [`retry::is_transient`] judges
    /// it without a network error code.
    pub fn new(message: impl Into<String>) -> ProviderError {
        let message = message.into();
        ProviderError {
            retryable: retry::is_transient(&message, None),
            message,
            telemetry: None,
            reply: String::new(),
        }
    }
}

impl std::fmt::Display for ProviderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ProviderError {}

/// A CLI's program, its configuration folder and the environment it runs
/// with (the core resolves them as `settings.js` and `harness.js` `cliEnv`
/// do).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CliRuntime {
    /// The program, a path or a command on `PATH` (`MCBENCH_CODEX_BINARY`,
    /// else `codex` on `PATH`, else the tools folder's).
    pub program: String,
    /// Arguments before the CLI's own: none, except Codex on Windows,
    /// which runs in the WSL distro (`codex_cli::CodexConfig::prefix`).
    pub args: Vec<String>,
    /// Its folder: `CODEX_HOME` (default `~/.codex`) or Claude Code's config
    /// folder.
    pub home: PathBuf,
    /// The whole environment it runs with (`hostEnv()`, `claudeEnv()`).
    pub env: Vec<(String, String)>,
}

/// A provider's key, sign-in and switches, from the store's settings as the
/// core resolves them (`llm.js` `resolveProvider`).
#[derive(Clone, Debug)]
pub struct ProviderConfig {
    /// Which provider.
    pub id: ProviderId,
    /// Its API key, saved or from its environment variable, trimmed; `''`
    /// when it has none or takes none.
    pub api_key: String,
    /// Offered at all (`enabled`).
    pub enabled: bool,
    /// Connected (`settings.js` `connected`: a key, or a CLI login with its
    /// CLI installed).
    pub connected: bool,
    /// The CLI whose login or catalogue it reads: Codex's for `codex` and
    /// `codex-cli`, Claude Code's for `claude-code`.
    pub cli: Option<CliRuntime>,
}

/// A model's facts (`llm.js` `modelInfo`, `infoFromCatalogEntry`,
/// `qwencloud.js` `modelInfo`, `codex-cli.js` `modelsFromCache`), in their
/// key order.
#[derive(Clone, Debug, PartialEq)]
pub struct ModelFacts {
    /// `known`: the source describes it.
    pub known: bool,
    /// `imageInput`, or null when the source does not say.
    pub image_input: Option<bool>,
    /// `imageOutput`.
    pub image_output: bool,
    /// `tools`, or null for an unknown model.
    pub tools: Option<bool>,
    /// `reasoning`.
    pub reasoning: bool,
    /// `efforts`, in `EFFORT_ORDER` for the catalog's models.
    pub efforts: Vec<String>,
    /// `defaultEffort`.
    pub default_effort: Option<String>,
    /// `mandatoryReasoning`.
    pub mandatory_reasoning: bool,
    /// `contextWindow`: absent (`None`) for QwenCloud's models, which have
    /// no such key today, else the value or null.
    pub context_window: Option<Option<f64>>,
    /// The Codex catalogue's `fast` and `fastNote`, present only for its
    /// models.
    pub fast: Option<(bool, Option<String>)>,
}

impl ModelFacts {
    /// The facts of a model no source describes (`infoFromCatalogEntry(null)`).
    pub fn unknown() -> ModelFacts {
        ModelFacts {
            known: false,
            image_input: None,
            image_output: false,
            tools: None,
            reasoning: false,
            efforts: Vec::new(),
            default_effort: None,
            mandatory_reasoning: false,
            context_window: Some(None),
            fast: None,
        }
    }

    /// Appends the facts to `out` in today's key order.
    pub fn write(&self, out: &mut Map<String, Value>) {
        let opt_bool = |v: Option<bool>| v.map(Value::Bool).unwrap_or(Value::Null);
        out.insert("known".into(), Value::Bool(self.known));
        out.insert("imageInput".into(), opt_bool(self.image_input));
        out.insert("imageOutput".into(), Value::Bool(self.image_output));
        out.insert("tools".into(), opt_bool(self.tools));
        out.insert("reasoning".into(), Value::Bool(self.reasoning));
        out.insert(
            "efforts".into(),
            Value::Array(self.efforts.iter().cloned().map(Value::String).collect()),
        );
        out.insert(
            "defaultEffort".into(),
            self.default_effort
                .clone()
                .map(Value::String)
                .unwrap_or(Value::Null),
        );
        out.insert(
            "mandatoryReasoning".into(),
            Value::Bool(self.mandatory_reasoning),
        );
        if let Some(window) = self.context_window {
            out.insert(
                "contextWindow".into(),
                window
                    .and_then(serde_json::Number::from_f64)
                    .map(Value::Number)
                    .unwrap_or(Value::Null),
            );
        }
        if let Some((fast, note)) = &self.fast {
            out.insert("fast".into(), Value::Bool(*fast));
            out.insert(
                "fastNote".into(),
                note.clone().map(Value::String).unwrap_or(Value::Null),
            );
        }
    }

    /// The facts as `modelInfo` returns them.
    pub fn to_json(&self) -> Value {
        let mut out = Map::new();
        self.write(&mut out);
        Value::Object(out)
    }
}

/// A model a provider offers (`llm.js` `listModels`): its id, name, release
/// time and facts.
#[derive(Clone, Debug, PartialEq)]
pub struct ModelInfo {
    /// The model id.
    pub id: String,
    /// The name the picker shows.
    pub name: String,
    /// `created`: its release, in seconds since 1970 (0 when unknown); the
    /// list is newest first.
    pub created: f64,
    /// Whether it takes images (`see_build` and `view_image` are offered
    /// only then), its efforts and the rest.
    pub facts: ModelFacts,
}

impl ModelInfo {
    /// `{id, name, created, ...facts}`.
    pub fn to_json(&self) -> Value {
        let mut out = Map::new();
        out.insert("id".into(), Value::String(self.id.clone()));
        out.insert("name".into(), Value::String(self.name.clone()));
        out.insert(
            "created".into(),
            serde_json::Number::from_f64(self.created)
                .map(Value::Number)
                .expect("a finite time"),
        );
        self.facts.write(&mut out);
        Value::Object(out)
    }
}

/// A model provider. Every provider, API or CLI, implements it.
#[async_trait]
pub trait Provider: Send + Sync {
    /// Which provider this is.
    fn id(&self) -> ProviderId;
    /// Runs one completion, streaming its deltas, until the model answers
    /// or `cancel` is cancelled.
    async fn complete(
        &self,
        request: &CompletionRequest,
        on_delta: &mut (dyn FnMut(Delta) + Send),
        cancel: &CancellationToken,
    ) -> Result<AssistantMessage, ProviderError>;
    /// The models it offers (`llm.js` `listModels`), newest first.
    async fn models(&self) -> Result<Vec<ModelInfo>, ProviderError>;
    /// One model's facts (`llm.js` `modelInfo`); a model no source
    /// describes has [`ModelFacts::unknown`].
    async fn model_info(&self, model: &str) -> Result<ModelFacts, ProviderError>;
    /// Ends a harness's session for a chat (`llm.js` `closeHarnessSession`);
    /// nothing for an API provider.
    async fn close_session(&self, conversation_id: &str);
}

pub mod bridge;
pub mod harness;

#[cfg(test)]
mod seam {
    use super::bridge::{BridgeHost, McpContent};
    use super::*;

    struct FakeProvider;
    #[async_trait]
    impl Provider for FakeProvider {
        fn id(&self) -> ProviderId {
            unimplemented!("a fake for the seam test")
        }
        async fn complete(
            &self,
            _: &CompletionRequest,
            _: &mut (dyn FnMut(Delta) + Send),
            _: &CancellationToken,
        ) -> Result<AssistantMessage, ProviderError> {
            unimplemented!("a fake for the seam test")
        }
        async fn models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
            unimplemented!("a fake for the seam test")
        }
        async fn model_info(&self, _: &str) -> Result<ModelFacts, ProviderError> {
            unimplemented!("a fake for the seam test")
        }
        async fn close_session(&self, _: &str) {
            unimplemented!("a fake for the seam test")
        }
    }

    struct FakeHost;
    #[async_trait]
    impl BridgeHost for FakeHost {
        fn tools(&self) -> Vec<ToolSchema> {
            unimplemented!("a fake for the seam test")
        }
        async fn call(
            &self,
            _: &str,
            _: serde_json::Value,
            _: &str,
            _: Option<&str>,
            _: &CancellationToken,
        ) -> Result<Vec<McpContent>, String> {
            unimplemented!("a fake for the seam test")
        }
    }

    #[test]
    fn a_fake_can_be_written_against_each_trait() {
        let _: Box<dyn Provider> = Box::new(FakeProvider);
        let _: Box<dyn BridgeHost> = Box::new(FakeHost);
    }

    #[test]
    fn provider_keys_are_todays() {
        let keys: Vec<String> = [
            ProviderId::OpenRouter,
            ProviderId::ClaudeCode,
            ProviderId::CodexCli,
            ProviderId::QwenCloud,
        ]
        .iter()
        .map(|id| serde_json::to_string(id).unwrap())
        .collect();
        assert_eq!(
            keys,
            [
                "\"openrouter\"",
                "\"claude-code\"",
                "\"codex-cli\"",
                "\"qwencloud\""
            ]
        );
    }
}
