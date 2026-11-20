//! [`TurnEvent`]: what a turn emits, and its NDJSON form.

use serde_json::{Map, Value};

use mcbench_providers::api::js;
use mcbench_tools::{CallShown, ToolShown};

/// What the transcript shows of a test's progress (`tool_progress`).
#[derive(Clone, Debug, PartialEq)]
pub struct ProgressShown {
    /// `message`.
    pub message: String,
}

/// What a turn emits as it runs (`agent.js` `emit`, and `server/index.js`
/// `executeTurn` for `turn_snapshot`, `done` and `error`), in order. The
/// interface shows these; the store keeps the messages they come from.
/// [`TurnEvent::to_json`] is the event as today's NDJSON stream writes it.
#[derive(Clone, Debug)]
pub enum TurnEvent {
    /// `assistant_delta`: text of the answer as it streams.
    AssistantDelta(String),
    /// `reasoning_delta`.
    ReasoningDelta(String),
    /// `tool_call_delta`: part of a call as it streams.
    ToolCallDelta {
        /// The call's index.
        index: u32,
        /// Its id so far (`''` until the provider sent it).
        id: Option<String>,
        /// Its name so far (`''` until the provider sent it).
        name: Option<String>,
        /// More of its arguments.
        arguments: String,
    },
    /// `harness_status`: a harness's activity line, or none.
    HarnessStatus(Option<String>),
    /// `notice`: a harness's notice.
    Notice {
        /// `info`, `warning` or `error`.
        level: String,
        /// The text.
        text: String,
    },
    /// `plan`: a harness's task list.
    Plan(Value),
    /// `completion_retry`: an API request is retried.
    CompletionRetry {
        /// The attempt about to run.
        attempt: u32,
        /// The wait before it.
        delay_ms: u64,
        /// Why the last one failed.
        error: String,
    },
    /// `reasoning`: a message's whole reasoning.
    Reasoning(String),
    /// `assistant`: a message's text; `final` when no tool call follows.
    Assistant {
        /// The text.
        text: String,
        /// The turn's last message.
        last: bool,
    },
    /// `tool_start`.
    ToolStart {
        /// The call's id.
        call_id: String,
        /// The tool.
        name: String,
        /// The summary line and the program.
        shown: Box<CallShown>,
    },
    /// `tool_progress`.
    ToolProgress {
        /// The call's id.
        call_id: String,
        /// The tool.
        name: String,
        /// What the transcript shows.
        progress: ProgressShown,
        /// The tool's progress object as it gave it (the NDJSON line's
        /// fields).
        raw: Value,
    },
    /// `tool_result`: what the transcript shows of a result.
    ToolResult {
        /// The call's id.
        call_id: String,
        /// The tool.
        name: String,
        /// What the transcript shows.
        shown: Box<ToolShown>,
        /// The result as the model reads it (the NDJSON line's `ok`,
        /// `cancelled`, `error`, `diff`, `totalBlocks` and `output` come
        /// from it).
        result: Value,
        /// The line's `images`: the shown images' addresses, for an
        /// answered `see_build` or `view_image`.
        image_urls: Option<Vec<String>>,
    },
    /// The chat's world changed (`onWorldChange`): the live view reloads it.
    WorldChanged,
    /// `turn_snapshot`: the core publishes the transcript.
    Snapshot,
    /// `done`.
    Done,
    /// `error`: the turn failed with this message.
    Error {
        /// The message.
        message: String,
        /// The stored error message's id, when it was stored.
        message_id: Option<String>,
    },
}

fn event(kind: &str, fields: Vec<(&str, Value)>) -> Value {
    let mut out = Map::new();
    out.insert("type".into(), Value::String(kind.into()));
    for (key, value) in fields {
        out.insert(key.into(), value);
    }
    Value::Object(out)
}

fn text(value: &str) -> Value {
    Value::String(value.into())
}

fn optional(value: &Option<String>) -> Value {
    value.clone().map(Value::String).unwrap_or(Value::Null)
}

impl TurnEvent {
    /// The event as `agent.js` emits it (`JSON.stringify` of it is the NDJSON
    /// line), or `None` for [`TurnEvent::WorldChanged`], a callback today,
    /// and [`TurnEvent::Snapshot`], whose messages the core adds.
    pub fn to_json(&self) -> Option<Value> {
        Some(match self {
            TurnEvent::AssistantDelta(t) => event("assistant_delta", vec![("text", text(t))]),
            TurnEvent::ReasoningDelta(t) => event("reasoning_delta", vec![("text", text(t))]),
            TurnEvent::ToolCallDelta {
                index,
                id,
                name,
                arguments,
            } => event(
                "tool_call_delta",
                vec![
                    ("index", Value::from(*index)),
                    ("id", optional(id)),
                    ("name", optional(name)),
                    ("args", text(arguments)),
                ],
            ),
            TurnEvent::HarnessStatus(t) => event("harness_status", vec![("text", optional(t))]),
            TurnEvent::Notice { level, text: t } => {
                event("notice", vec![("level", text(level)), ("text", text(t))])
            }
            TurnEvent::Plan(items) => event("plan", vec![("items", items.clone())]),
            TurnEvent::CompletionRetry {
                attempt,
                delay_ms,
                error,
            } => event(
                "completion_retry",
                vec![
                    ("attempt", Value::from(*attempt)),
                    ("delayMs", Value::from(*delay_ms)),
                    ("error", text(error)),
                ],
            ),
            TurnEvent::Reasoning(t) => event("reasoning", vec![("text", text(t))]),
            TurnEvent::Assistant { text: t, last } => event(
                "assistant",
                vec![("text", text(t)), ("final", Value::Bool(*last))],
            ),
            TurnEvent::ToolStart {
                call_id,
                name,
                shown,
            } => {
                let mut fields = vec![
                    ("callId", text(call_id)),
                    ("name", text(name)),
                    ("summary", text(&shown.summary)),
                ];
                if let Some(program) = &shown.program {
                    fields.push(("program", program.clone()));
                }
                event("tool_start", fields)
            }
            TurnEvent::ToolProgress {
                call_id, name, raw, ..
            } => {
                let mut fields = vec![("callId", text(call_id)), ("name", text(name))];
                if let Value::Object(map) = raw {
                    let mut out = match event("tool_progress", fields) {
                        Value::Object(out) => out,
                        _ => unreachable!("an event is an object"),
                    };
                    for (key, value) in map {
                        out.insert(key.clone(), value.clone());
                    }
                    return Some(Value::Object(out));
                }
                fields.truncate(2);
                event("tool_progress", fields)
            }
            TurnEvent::ToolResult {
                call_id,
                name,
                result,
                image_urls,
                ..
            } => {
                let mut fields = vec![("callId", text(call_id)), ("name", text(name))];
                for key in ["ok", "cancelled", "error", "diff", "totalBlocks"] {
                    if let Some(value) = result.get(key) {
                        fields.push((key, value.clone()));
                    }
                }
                if let Some(output) = result_output(name, result) {
                    fields.push(("output", output));
                }
                if let Some(urls) = image_urls {
                    fields.push((
                        "images",
                        Value::Array(urls.iter().map(|u| text(u)).collect()),
                    ));
                }
                event("tool_result", fields)
            }
            TurnEvent::WorldChanged | TurnEvent::Snapshot => return None,
            TurnEvent::Done => event("done", vec![]),
            TurnEvent::Error {
                message,
                message_id,
            } => {
                let mut fields = vec![("message", text(message))];
                if let Some(id) = message_id {
                    fields.push(("messageId", text(id)));
                }
                event("error", fields)
            }
        })
    }
}

/// `tool_result`'s `output` (`agent.js` 740): `bash`'s transcript, or
/// `test_build`'s and `prompt_library`'s result indented by 2.
fn result_output(name: &str, result: &Value) -> Option<Value> {
    match name {
        "bash" => js::coalesce(
            js::get(Some(result), "text"),
            js::get(Some(result), "output"),
        )
        .cloned(),
        "test_build" | "prompt_library" => {
            Some(Value::String(mcbench_json::stringify_pretty(result, 2)))
        }
        _ => None,
    }
}

/// Where a turn's events go.
pub trait TurnSink: Send {
    /// Takes one event.
    fn emit(&mut self, event: TurnEvent);
}
