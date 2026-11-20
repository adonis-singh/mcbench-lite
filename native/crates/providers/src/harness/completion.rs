//! What both harnesses' completions share: reading a relay's answer while
//! its deltas stream, the message as the agent gets it, the abort watcher,
//! and the telemetry `chatCompletion` adds (papi's `api::Tracker`).

use serde_json::{Map, Value, json};
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;

use crate::api::{Reply, Tracker};
use crate::{AssistantMessage, CompletionRequest, Delta, ProviderError, ProviderId, ToolCall};

/// Forwards the waiter's deltas to `on_delta` until its message (or error)
/// arrives, then the deltas sent before it.
pub(crate) async fn read_answer(
    mut deltas: mpsc::UnboundedReceiver<Delta>,
    mut done: oneshot::Receiver<Result<Map<String, Value>, String>>,
    on_delta: &mut (dyn FnMut(Delta) + Send),
) -> Result<Map<String, Value>, String> {
    loop {
        tokio::select! {
            biased;
            delta = deltas.recv() => match delta {
                Some(delta) => on_delta(delta),
                None => {
                    // No more deltas: only the answer is left.
                    return (&mut done).await.unwrap_or_else(|_| Err("The harness run was dropped".to_owned()));
                }
            },
            answer = &mut done => {
                while let Ok(delta) = deltas.try_recv() {
                    on_delta(delta);
                }
                return answer.unwrap_or_else(|_| Err("The harness run was dropped".to_owned()));
            }
        }
    }
}

/// The relay's message as `api::Tracker` takes it: `content`,
/// `reasoning`, `finish_reason`, `usage`, `tool_calls`, `notices`, `plan`,
/// `rateLimit` and `contextTokens` (`chatCompletion` moves the last two into
/// the telemetry).
pub fn reply_of(message: &Map<String, Value>) -> Reply {
    let text = |key: &str| message.get(key).and_then(Value::as_str).map(str::to_owned);
    let tool_calls = match message.get("tool_calls") {
        Some(Value::Array(calls)) => calls
            .iter()
            .map(|call| ToolCall {
                id: call["id"].as_str().unwrap_or_default().to_owned(),
                kind: call["type"].as_str().unwrap_or_default().to_owned(),
                name: call["function"]["name"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned(),
                arguments: call["function"]["arguments"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned(),
                extra_content: None,
            })
            .collect(),
        _ => Vec::new(),
    };
    Reply {
        content: text("content").unwrap_or_default(),
        reasoning: text("reasoning"),
        reasoning_details: None,
        finish_reason: text("finish_reason"),
        usage: message.get("usage").cloned(),
        tool_calls,
        notices: message.get("notices").and_then(Value::as_array).cloned(),
        plan: message.get("plan").cloned(),
        rate_limit: message.get("rateLimit").cloned(),
        context_tokens: message.get("contextTokens").and_then(Value::as_f64),
    }
}

/// `chatCompletion`'s telemetry around a harness completion: every delta
/// but `Started` (today's `onStarted`, which no delta counts) is noted, and
/// the message or error gets its telemetry.
pub(crate) struct Tracked(std::sync::Mutex<Tracker>);

impl Tracked {
    pub(crate) fn start(provider: ProviderId, request: &CompletionRequest) -> Tracked {
        Tracked(std::sync::Mutex::new(Tracker::start(
            provider, request, false,
        )))
    }

    pub(crate) fn note(&self, delta: &Delta) {
        if !matches!(delta, Delta::Started) {
            self.0.lock().unwrap().delta(delta);
        }
    }

    pub(crate) fn finish(
        self,
        reply: Result<Map<String, Value>, ProviderError>,
        cancel: &CancellationToken,
    ) -> Result<AssistantMessage, ProviderError> {
        self.0
            .into_inner()
            .unwrap()
            .finish(reply.map(|m| reply_of(&m)), cancel)
    }
}

/// The message as today's harness completion returns it (`role`,
/// `content`, the rest, `tool_calls` last): what the differential compares.
pub fn message_json(message: &Map<String, Value>) -> Value {
    Value::Object(message.clone())
}

/// A delta as today's `onDelta` receives it (`{ content }`, `{ toolCall }`,
/// ...); `Started` is `{ started: true }`, which today is the `onStarted`
/// call.
pub fn delta_json(delta: &Delta) -> Value {
    match delta {
        Delta::Started => json!({ "started": true }),
        Delta::Content(text) => json!({ "content": text }),
        Delta::Reasoning(text) => json!({ "reasoning": text }),
        Delta::ToolCall {
            index,
            id,
            name,
            arguments,
        } => {
            json!({ "toolCall": { "index": index, "id": id, "name": name, "args": arguments } })
        }
        Delta::Status(text) => json!({ "status": text }),
        Delta::Notice { level, text } => json!({ "notice": { "level": level, "text": text } }),
        Delta::Plan(items) => json!({ "plan": items }),
    }
}

/// A harness's failure as the seam carries it: never retried (a harness
/// retries its own requests, `agent.js` `runTurn`).
pub(crate) fn failure(message: String) -> ProviderError {
    ProviderError {
        message,
        retryable: false,
        telemetry: None,
        reply: String::new(),
    }
}

/// Runs `on_abort` when `cancel` is cancelled, unless `closed` is first
/// (the run closed: `removeEventListener`).
pub(crate) fn watch_abort(
    cancel: &CancellationToken,
    closed: CancellationToken,
    on_abort: impl FnOnce() + Send + 'static,
) {
    let cancel = cancel.clone();
    tokio::spawn(async move {
        tokio::select! {
            _ = closed.cancelled() => {}
            _ = cancel.cancelled() => on_abort(),
        }
    });
}
