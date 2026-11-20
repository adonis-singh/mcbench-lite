//! Retrying an API provider's completion (`server/completion-retry.js`).
//!
//! A completion has no tool side effects: the caller runs tools only after
//! it has the complete message, so the request is retried, never the turn.
//! A provider still failing after [`MAX_ATTEMPTS`] requests (about 90 s of
//! backoff) fails the turn with its last error. Harnesses are not retried
//! (the agent calls them once).

use std::time::Duration;

use tokio_util::sync::CancellationToken;

use crate::{AssistantMessage, CompletionRequest, Delta, Provider, ProviderError};

/// Requests a completion gets at most (`MAX_ATTEMPTS`).
pub const MAX_ATTEMPTS: u32 = 8;

/// The error of a completion asked for after the turn was stopped
/// (`signal.throwIfAborted()`, a DOMException without a reason).
pub const ABORTED: &str = "This operation was aborted";
/// The error of a backoff the turn's stop interrupted (`timers/promises`
/// `setTimeout` with an aborted signal).
pub const WAIT_ABORTED: &str = "The operation was aborted";

/// The network error codes `transientCompletionError` retries (undici's
/// `error.cause.code`).
const CODES: [&str; 8] = [
    "ECONNRESET",
    "ECONNREFUSED",
    "EPIPE",
    "ETIMEDOUT",
    "UND_ERR_SOCKET",
    "UND_ERR_CONNECT_TIMEOUT",
    "UND_ERR_HEADERS_TIMEOUT",
    "UND_ERR_BODY_TIMEOUT",
];

/// Whether an error is a rate limit or a transient failure
/// (`transientCompletionError`): its network `code`, a message that is
/// exactly `terminated`, `fetch failed` or `socket hang up` (any case), or a
/// message containing `HTTP 429` (500, 502, 503, 504, 529) as a whole word,
/// an overload, or one of the four sentences of a stream that ended early.
pub fn is_transient(message: &str, code: Option<&str>) -> bool {
    if code.is_some_and(|code| CODES.contains(&code)) {
        return true;
    }
    if ["terminated", "fetch failed", "socket hang up"]
        .iter()
        .any(|exact| message.eq_ignore_ascii_case(exact))
    {
        return true;
    }
    http_status_word(message)
        || [
            "servers are currently overloaded",
            "overloaded_error",
            "Overloaded",
            "Codex tool argument stream stalled",
            "Anthropic stream ended before message_stop",
            "Model stream ended without a finish reason",
        ]
        .iter()
        .any(|part| message.contains(part))
        || any_char_between(message, "Codex stream ended before response", "completed")
}

/// `/HTTP (429|500|502|503|504|529)\b/`.
fn http_status_word(message: &str) -> bool {
    message.match_indices("HTTP ").any(|(at, _)| {
        let rest = &message[at + 5..];
        ["429", "500", "502", "503", "504", "529"]
            .iter()
            .any(|code| {
                rest.starts_with(code)
                    && !rest[3..]
                        .chars()
                        .next()
                        .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
            })
    })
}

/// `before.after` where `.` is any character but a line terminator, as a
/// JavaScript regular expression reads it.
fn any_char_between(message: &str, before: &str, after: &str) -> bool {
    message.match_indices(before).any(|(at, _)| {
        let mut rest = message[at + before.len()..].chars();
        match rest.next() {
            Some(c) if !matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}') => {
                rest.as_str().starts_with(after)
            }
            _ => false,
        }
    })
}

/// A retry about to happen (`completion_retry`'s fields).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RetryEvent {
    /// The retry's number, 1 for the first.
    pub attempt: u32,
    /// The wait before it.
    pub delay_ms: u64,
    /// Why the last request failed.
    pub error: String,
}

/// The wait before retry number `retry + 1`: 1, 2, 4, 8, 16, 30, 30 s.
pub fn delay_ms(retry: u32) -> u64 {
    30_000.min(1000 * 2u64.pow(retry.min(5)))
}

/// Runs `provider`'s completion, retrying a transient failure
/// (`completionWithRetry`): before each request a stopped turn fails with
/// [`ABORTED`]; a failure that is not retryable, comes after the turn was
/// stopped, or is the [`MAX_ATTEMPTS`]th is returned; otherwise `on_retry`
/// hears of it and the retry waits [`delay_ms`], or fails with
/// [`WAIT_ABORTED`] when the turn is stopped during the wait. The same
/// request and the same `on_delta` serve every attempt.
pub async fn complete_with_retry(
    provider: &dyn Provider,
    request: &CompletionRequest,
    on_delta: &mut (dyn FnMut(Delta) + Send),
    on_retry: &mut (dyn FnMut(RetryEvent) + Send),
    cancel: &CancellationToken,
) -> Result<AssistantMessage, ProviderError> {
    let mut retry = 0;
    loop {
        if cancel.is_cancelled() {
            return Err(ProviderError::new(ABORTED));
        }
        let error = match provider.complete(request, on_delta, cancel).await {
            Ok(message) => return Ok(message),
            Err(error) => error,
        };
        if cancel.is_cancelled() || !error.retryable || retry + 1 == MAX_ATTEMPTS {
            return Err(error);
        }
        let delay = delay_ms(retry);
        on_retry(RetryEvent {
            attempt: retry + 1,
            delay_ms: delay,
            error: error.message.clone(),
        });
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_millis(delay)) => {}
            _ = cancel.cancelled() => return Err(ProviderError::new(WAIT_ABORTED)),
        }
        retry += 1;
    }
}

#[cfg(test)]
mod tests {
    //! `test/completion-retry.test.js`, 3 tests.

    use std::sync::Mutex;

    use async_trait::async_trait;

    use super::*;
    use crate::{ModelFacts, ModelInfo, Outcome, ProviderId, Telemetry, ToolCall};

    /// A provider whose completions fail with the given errors, in order,
    /// then answer.
    struct Scripted {
        errors: Mutex<Vec<String>>,
        calls: Mutex<u32>,
        on_first: Option<Delta>,
        cancel_on_call: Option<CancellationToken>,
    }

    fn message() -> AssistantMessage {
        AssistantMessage {
            content: String::new(),
            reasoning: None,
            reasoning_details: None,
            finish_reason: None,
            telemetry: Telemetry {
                provider: ProviderId::OpenRouter,
                model: "m".into(),
                effort: None,
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
            tool_calls: vec![ToolCall {
                id: "complete".into(),
                kind: "function".into(),
                name: "build".into(),
                arguments: "{}".into(),
                extra_content: None,
            }],
        }
    }

    #[async_trait]
    impl Provider for Scripted {
        fn id(&self) -> ProviderId {
            ProviderId::OpenRouter
        }
        async fn complete(
            &self,
            _: &CompletionRequest,
            on_delta: &mut (dyn FnMut(Delta) + Send),
            _: &CancellationToken,
        ) -> Result<AssistantMessage, ProviderError> {
            let call = {
                let mut calls = self.calls.lock().unwrap();
                *calls += 1;
                *calls
            };
            if call == 1
                && let Some(delta) = &self.on_first
            {
                on_delta(delta.clone());
            }
            if let Some(cancel) = &self.cancel_on_call {
                cancel.cancel();
            }
            let mut errors = self.errors.lock().unwrap();
            if errors.is_empty() {
                return Ok(message());
            }
            let text = errors.remove(0);
            Err(ProviderError::new(text))
        }
        async fn models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
            unimplemented!()
        }
        async fn model_info(&self, _: &str) -> Result<ModelFacts, ProviderError> {
            unimplemented!()
        }
        async fn close_session(&self, _: &str) {}
    }

    fn request() -> CompletionRequest {
        CompletionRequest {
            conversation_id: None,
            model: "m".into(),
            effort: None,
            messages: Vec::new(),
            tools: Vec::new(),
            chat: false,
            transcript: None,
        }
    }

    fn scripted(errors: &[String]) -> Scripted {
        Scripted {
            errors: Mutex::new(errors.to_vec()),
            calls: Mutex::new(0),
            on_first: None,
            cancel_on_call: None,
        }
    }

    #[tokio::test(start_paused = true)]
    async fn retry_preserves_request_and_running_work_returns_only_the_complete_tool_message() {
        let mut provider = scripted(&["terminated".into()]);
        provider.on_first = Some(Delta::ToolCall {
            index: 0,
            id: "partial".into(),
            name: String::new(),
            arguments: "{\"program\":".into(),
        });
        let mut deltas = Vec::new();
        let mut retries = Vec::new();
        let request = request();
        let result = complete_with_retry(
            &provider,
            &request,
            &mut |delta| deltas.push(delta),
            &mut |event| retries.push(event),
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(result.tool_calls[0].id, "complete");
        assert_eq!(*provider.calls.lock().unwrap(), 2);
        assert_eq!(deltas.len(), 1);
        assert_eq!(
            retries,
            [RetryEvent {
                attempt: 1,
                delay_ms: 1000,
                error: "terminated".into()
            }]
        );
    }

    #[tokio::test(start_paused = true)]
    async fn transient_recovery_has_bounded_backoff_and_a_capped_attempt_count_permanent_errors_escape()
     {
        let errors: Vec<String> = (1..MAX_ATTEMPTS)
            .map(|_| "HTTP 503: unavailable".to_string())
            .collect();
        let provider = scripted(&errors);
        let mut waits = Vec::new();
        complete_with_retry(
            &provider,
            &request(),
            &mut |_| {},
            &mut |event| waits.push(event.delay_ms),
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(*provider.calls.lock().unwrap(), MAX_ATTEMPTS);
        assert_eq!(waits.iter().max(), Some(&30_000));

        // A provider stuck on 429 fails the turn with its last error.
        let errors: Vec<String> = (1..=MAX_ATTEMPTS + 2)
            .map(|n| format!("HTTP 429: busy {n}"))
            .collect();
        let provider = scripted(&errors);
        let mut waits = Vec::new();
        let error = complete_with_retry(
            &provider,
            &request(),
            &mut |_| {},
            &mut |event| waits.push(event.delay_ms),
            &CancellationToken::new(),
        )
        .await
        .unwrap_err();
        assert_eq!(error.message, format!("HTTP 429: busy {MAX_ATTEMPTS}"));
        assert_eq!(*provider.calls.lock().unwrap(), MAX_ATTEMPTS);
        assert_eq!(waits.len() as u32, MAX_ATTEMPTS - 1);

        for message in [
            "HTTP 401: invalid credentials",
            "HTTP 400: bad request",
            "invalid tool schema",
        ] {
            let provider = scripted(&[message.to_string()]);
            let error = complete_with_retry(
                &provider,
                &request(),
                &mut |_| {},
                &mut |_| {},
                &CancellationToken::new(),
            )
            .await
            .unwrap_err();
            assert_eq!(error.message, message);
            assert_eq!(*provider.calls.lock().unwrap(), 1);
        }
        assert!(is_transient("fetch failed", Some("ECONNRESET")));
    }

    #[tokio::test(start_paused = true)]
    async fn cancellation_interrupts_backoff_without_another_model_request() {
        let cancel = CancellationToken::new();
        let provider = scripted(&["terminated".into(), "terminated".into()]);
        let stop = cancel.clone();
        let result = complete_with_retry(
            &provider,
            &request(),
            &mut |_| {},
            &mut |_| stop.cancel(),
            &cancel,
        )
        .await;
        assert_eq!(result.unwrap_err().message, WAIT_ABORTED);
        assert_eq!(*provider.calls.lock().unwrap(), 1);
    }

    #[test]
    fn the_transient_patterns_are_todays() {
        assert!(is_transient(
            "LLM request failed (HTTP 529): Overloaded",
            None
        ));
        assert!(is_transient(
            "Codex stream ended before response.completed",
            None
        ));
        assert!(is_transient(
            "Codex stream ended before responseXcompleted",
            None
        ));
        assert!(!is_transient(
            "Codex stream ended before response\ncompleted",
            None
        ));
        assert!(!is_transient("LLM request failed (HTTP 5030): x", None));
        assert!(is_transient("HTTP 500.", None));
        assert!(!is_transient("HTTP 400: nope", None));
        assert!(is_transient("Socket Hang Up", None));
        assert!(!is_transient("terminated early", None));
        assert!(!is_transient(
            "The OpenRouter model catalog could not describe m: Model catalog fetch failed with status 503",
            None
        ));
    }
}
