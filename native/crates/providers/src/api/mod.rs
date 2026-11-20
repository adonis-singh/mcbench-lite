//! The API providers (`server/llm.js`, `codex.js`, `qwencloud.js` and the
//! `openai-oauth` package the ChatGPT sign-in runs in-process): OpenRouter,
//! OpenAI, Groq, Google and QwenCloud on Chat Completions ([`chat`]),
//! Anthropic on Messages ([`anthropic`]), and the ChatGPT sign-in on the
//! Responses API ([`codex`], through [`oauth`], which holds what the OAuth
//! bridge did: the login file, its token refresh, the Codex catalogue's
//! model defaults and the request's headers).
//!
//! Every request is today's to the byte: the same address, method, headers
//! (see [`http`] for the few that cannot be, and why), and a body written as
//! `JSON.stringify` writes it. The streams are read as `readSse` reads them
//! ([`sse`]) and become today's deltas and message; failures read as today's
//! sentences. [`Tracker`] is `chatCompletion`'s telemetry, which the
//! harnesses use too.

pub mod anthropic;
pub mod chat;
pub mod codex;
pub mod decode;
pub mod http;
pub mod js;
pub mod oauth;
pub mod qwencloud;
pub mod sse;
pub mod v8json;

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use crate::catalog::Catalog;
use crate::{
    ApiMessage, AssistantMessage, CompletionRequest, Delta, ModelFacts, ModelInfo, Outcome,
    Provider, ProviderConfig, ProviderError, ProviderId, Telemetry, ToolCall, Usage,
};

/// Every address the API providers reach. [`Endpoints::today`] is today's;
/// [`Endpoints::under`] puts each under another origin, as the differential
/// test's replay server serves them (`https://host/path` becomes
/// `origin/host/path`).
#[derive(Clone, Debug)]
pub struct Endpoints {
    /// OpenRouter's API (`https://openrouter.ai/api/v1`).
    pub openrouter: String,
    /// OpenAI's.
    pub openai: String,
    /// Anthropic's.
    pub anthropic: String,
    /// Groq's.
    pub groq: String,
    /// Gemini's OpenAI-compatible API.
    pub google: String,
    /// QwenCloud's compatible mode.
    pub qwencloud: String,
    /// The OpenRouter catalog (`CATALOG_URL`).
    pub catalog: String,
    /// Gemini's own model list (`GOOGLE_MODELS_URL`).
    pub google_models: String,
    /// The ChatGPT sign-in's backend (`DEFAULT_CODEX_BASE_URL`).
    pub codex: String,
    /// OpenAI's token endpoint (`DEFAULT_OPENAI_OAUTH_ISSUER/oauth/token`).
    pub codex_token: String,
    /// npm's latest `@openai/codex`, whose version the backend's catalogue
    /// is asked for (`CODEX_REGISTRY_URL`).
    pub codex_registry: String,
}

impl Endpoints {
    /// Today's addresses.
    pub fn today() -> Endpoints {
        Endpoints {
            openrouter: "https://openrouter.ai/api/v1".into(),
            openai: "https://api.openai.com/v1".into(),
            anthropic: "https://api.anthropic.com/v1".into(),
            groq: "https://api.groq.com/openai/v1".into(),
            google: "https://generativelanguage.googleapis.com/v1beta/openai".into(),
            qwencloud: "https://dashscope-intl.aliyuncs.com/compatible-mode/v1".into(),
            catalog: "https://openrouter.ai/api/v1/models".into(),
            google_models: "https://generativelanguage.googleapis.com/v1beta/models".into(),
            codex: "https://chatgpt.com/backend-api/codex".into(),
            codex_token: "https://auth.openai.com/oauth/token".into(),
            codex_registry: "https://registry.npmjs.org/@openai/codex/latest".into(),
        }
    }

    /// Today's addresses under `origin` (`http://127.0.0.1:PORT`).
    pub fn under(origin: &str) -> Endpoints {
        let at = |url: String| url.replacen("https://", &format!("{origin}/"), 1);
        let today = Endpoints::today();
        Endpoints {
            openrouter: at(today.openrouter),
            openai: at(today.openai),
            anthropic: at(today.anthropic),
            groq: at(today.groq),
            google: at(today.google),
            qwencloud: at(today.qwencloud),
            catalog: at(today.catalog),
            google_models: at(today.google_models),
            codex: at(today.codex),
            codex_token: at(today.codex_token),
            codex_registry: at(today.codex_registry),
        }
    }

    /// A Chat Completions or Messages provider's base address.
    pub fn base(&self, id: ProviderId) -> &str {
        match id {
            ProviderId::OpenRouter => &self.openrouter,
            ProviderId::OpenAi => &self.openai,
            ProviderId::Anthropic => &self.anthropic,
            ProviderId::Groq => &self.groq,
            ProviderId::Google => &self.google,
            ProviderId::QwenCloud => &self.qwencloud,
            other => panic!("{} has no base address", other.key()),
        }
    }
}

/// What the API providers share for the app's life: the HTTP client, the
/// addresses, the OpenRouter catalog and model lists, the Anthropic effort
/// lookups and the ChatGPT sign-in's caches (all of them per process today).
pub struct Api {
    /// The HTTP client.
    pub http: reqwest::Client,
    /// The addresses.
    pub endpoints: Endpoints,
    /// The OpenRouter catalog and the model lists.
    pub catalog: Arc<Catalog>,
    pub(crate) adaptive: anthropic::Adaptive,
    pub(crate) oauth: oauth::Bridge,
}

impl Api {
    /// The shared state, reaching `endpoints`.
    pub fn new(endpoints: Endpoints) -> Arc<Api> {
        let http = http::client();
        Arc::new(Api {
            catalog: Arc::new(Catalog::new(http.clone(), endpoints.clone())),
            adaptive: anthropic::Adaptive::default(),
            oauth: oauth::Bridge::default(),
            http,
            endpoints,
        })
    }

    /// The provider `config` describes: an API provider, or a panic for a
    /// harness or Cursor, which this module does not build.
    pub fn provider(self: &Arc<Api>, config: ProviderConfig) -> Arc<dyn Provider> {
        assert!(
            !config.id.is_harness(),
            "{} is not an API provider",
            config.id.key()
        );
        Arc::new(ApiProvider {
            api: self.clone(),
            config,
        })
    }
}

/// `resolveProvider`'s refusals: off in Settings, or not connected.
pub fn resolve(config: &ProviderConfig) -> Result<(), ProviderError> {
    let label = config.id.label();
    if !config.enabled {
        return Err(ProviderError::new(format!(
            "{label} is disabled in Settings"
        )));
    }
    if !config.connected {
        return Err(ProviderError::new(if config.id.cli_login() {
            format!("{label} is not signed in: sign in to it in Settings")
        } else {
            format!("Connect {label} in Settings")
        }));
    }
    Ok(())
}

/// An API provider.
pub struct ApiProvider {
    api: Arc<Api>,
    config: ProviderConfig,
}

#[async_trait]
impl Provider for ApiProvider {
    fn id(&self) -> ProviderId {
        self.config.id
    }

    async fn complete(
        &self,
        request: &CompletionRequest,
        on_delta: &mut (dyn FnMut(Delta) + Send),
        cancel: &CancellationToken,
    ) -> Result<AssistantMessage, ProviderError> {
        resolve(&self.config)?;
        let mut tracker = Tracker::start(self.config.id, request, true);
        let reply = {
            let mut tracked = |delta: Delta| {
                tracker.delta(&delta);
                on_delta(delta);
            };
            match self.config.id {
                ProviderId::Anthropic => {
                    anthropic::complete(&self.api, &self.config, request, &mut tracked, cancel)
                        .await
                }
                ProviderId::Codex => {
                    codex::complete(&self.api, &self.config, request, &mut tracked, cancel).await
                }
                _ => chat::complete(&self.api, &self.config, request, &mut tracked, cancel).await,
            }
        };
        tracker.finish(reply, cancel)
    }

    async fn models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        self.api.catalog.list_models(&self.api, &self.config).await
    }

    async fn model_info(&self, model: &str) -> Result<ModelFacts, ProviderError> {
        self.api
            .catalog
            .model_info(&self.api, &self.config, model)
            .await
    }

    async fn close_session(&self, _: &str) {}
}

/// What a protocol's completion returns before the telemetry is added.
#[derive(Clone, Debug, Default)]
pub struct Reply {
    /// The answer's text.
    pub content: String,
    /// The reasoning, if any.
    pub reasoning: Option<String>,
    /// The reasoning items kept for replay.
    pub reasoning_details: Option<Vec<Value>>,
    /// The finish reason.
    pub finish_reason: Option<String>,
    /// The provider's usage object as it sent it, or none.
    pub usage: Option<Value>,
    /// The calls.
    pub tool_calls: Vec<ToolCall>,
    /// A harness's notices.
    pub notices: Option<Vec<Value>>,
    /// A harness's task list.
    pub plan: Option<Value>,
    /// Claude Code's rate-limit object.
    pub rate_limit: Option<Value>,
    /// A harness's context tokens, when it can tell them.
    pub context_tokens: Option<f64>,
}

/// `chatCompletion`'s telemetry around one completion: the start time, the
/// first delta's time, the reply text streamed so far, and the request's
/// size. A harness uses it the same way, with `api` false (its context
/// tokens are its own report, not the usage's input).
pub struct Tracker {
    provider: ProviderId,
    model: String,
    effort: Option<String>,
    started_ms: i64,
    started: tokio::time::Instant,
    first_delta_ms: Option<u64>,
    reply: String,
    context_bytes: u64,
    message_count: u64,
    api: bool,
}

impl Tracker {
    /// Starts the clock for `request`.
    pub fn start(provider: ProviderId, request: &CompletionRequest, api: bool) -> Tracker {
        let messages = Value::Array(
            request
                .messages
                .iter()
                .map(|ApiMessage(message)| Value::Object(message.clone()))
                .collect(),
        );
        Tracker {
            provider,
            model: request.model.clone(),
            effort: request.effort.clone().filter(|e| !e.is_empty()),
            started_ms: js::now_ms(),
            started: tokio::time::Instant::now(),
            first_delta_ms: None,
            reply: String::new(),
            context_bytes: mcbench_json::stringify(&messages).len() as u64,
            message_count: request.messages.len() as u64,
            api,
        }
    }

    /// Notes a delta (call it for every one, before passing it on).
    pub fn delta(&mut self, delta: &Delta) {
        if self.first_delta_ms.is_none() {
            self.first_delta_ms = Some(self.elapsed());
        }
        if let Delta::Content(text) = delta {
            self.reply.push_str(text);
        }
    }

    fn elapsed(&self) -> u64 {
        self.started.elapsed().as_millis() as u64
    }

    fn telemetry(&self, usage: Option<Usage>, outcome: Outcome) -> Telemetry {
        Telemetry {
            provider: self.provider,
            model: self.model.clone(),
            effort: self.effort.clone(),
            started_at: js::iso_string(self.started_ms),
            wall_ms: self.elapsed(),
            first_delta_ms: self.first_delta_ms,
            context_bytes: self.context_bytes,
            message_count: self.message_count,
            usage,
            outcome,
        }
    }

    /// The message with its telemetry, or the error with the failure's
    /// telemetry and the reply streamed before it.
    pub fn finish(
        self,
        reply: Result<Reply, ProviderError>,
        cancel: &CancellationToken,
    ) -> Result<AssistantMessage, ProviderError> {
        match reply {
            Ok(reply) => {
                let usage = reply.usage.as_ref().map(normalize_usage);
                let context_tokens = if self.api {
                    usage.as_ref().and_then(|u| u.input_tokens)
                } else {
                    reply.context_tokens
                };
                Ok(AssistantMessage {
                    content: reply.content,
                    reasoning: reply.reasoning,
                    reasoning_details: reply.reasoning_details,
                    finish_reason: reply.finish_reason,
                    telemetry: self.telemetry(usage, Outcome::Completed { context_tokens }),
                    notices: reply.notices,
                    plan: reply.plan,
                    rate_limit: reply.rate_limit,
                    tool_calls: reply.tool_calls,
                })
            }
            Err(mut error) => {
                error.telemetry = Some(Box::new(self.telemetry(
                    None,
                    Outcome::Failed {
                        cancelled: cancel.is_cancelled(),
                    },
                )));
                error.reply = self.reply;
                Err(error)
            }
        }
    }
}

/// `normalizeUsage`: each count when it is a finite number, else `None`
/// (never zero); Anthropic's input is its input plus its cache reads and
/// writes (a usage is Anthropic's when it has either cache field).
pub fn normalize_usage(usage: &Value) -> Usage {
    let number = |value: Option<&Value>| value.and_then(Value::as_f64).filter(|x| x.is_finite());
    let field = |name: &str| js::get(Some(usage), name);
    let anthropic = field("cache_creation_input_tokens").is_some()
        || field("cache_read_input_tokens").is_some();
    // `(a || 0) + (b || 0) + (c || 0)`: a value that is not a number makes
    // the sum a string or NaN in JavaScript, which is not finite.
    let or_zero = |name: &str| -> Option<f64> {
        let value = field(name);
        if !js::truthy(value) {
            return Some(0.0);
        }
        value.and_then(Value::as_f64)
    };
    let input = if anthropic {
        match (
            or_zero("input_tokens"),
            or_zero("cache_creation_input_tokens"),
            or_zero("cache_read_input_tokens"),
        ) {
            (Some(a), Some(b), Some(c)) => Some(a + b + c).filter(|x| x.is_finite()),
            _ => None,
        }
    } else {
        number(js::coalesce(field("input_tokens"), field("prompt_tokens")))
    };
    let nested = |outer: &str, inner: &str| js::get(field(outer), inner);
    Usage {
        input_tokens: input,
        output_tokens: number(js::coalesce(
            field("output_tokens"),
            field("completion_tokens"),
        )),
        cached_input_tokens: number(js::coalesce(
            js::coalesce(
                nested("input_tokens_details", "cached_tokens"),
                nested("prompt_tokens_details", "cached_tokens"),
            ),
            field("cache_read_input_tokens"),
        )),
        cache_write_tokens: number(field("cache_creation_input_tokens")),
        reasoning_tokens: number(js::coalesce(
            nested("output_tokens_details", "reasoning_tokens"),
            nested("completion_tokens_details", "reasoning_tokens"),
        )),
    }
}

/// `providerErrorMessage`: the answer with every copy of the key made
/// `[key]` (a provider can quote the key it was sent, as OpenAI's 401
/// does), its first 400 characters, or the message its JSON carries
/// (Gemini's comes wrapped in a one-element array).
pub fn provider_error_message(api_key: &str, status: u16, answer: &str) -> String {
    let text = if api_key.is_empty() {
        answer.to_owned()
    } else {
        answer.replace(api_key, "[key]")
    };
    let mut message = js::slice_utf16(&text, 400);
    if let Ok(parsed) = v8json::parse(&text) {
        let payload = match &parsed {
            Value::Array(items) => items.first(),
            other => Some(other),
        };
        // `payload.error` on null or undefined throws, and the cut text stays.
        if let Some(payload) = payload.filter(|p| !p.is_null()) {
            let error = js::get(Some(payload), "error");
            let found = [
                js::get(error, "message"),
                js::get(js::get(error, "error"), "message"),
                js::get(Some(payload), "message"),
            ]
            .into_iter()
            .find(|value| js::truthy(*value));
            if let Some(value) = found {
                message = js::to_string(value.expect("truthy"));
            }
        }
    }
    format!("LLM request failed (HTTP {status}): {message}")
}

/// The tools as parsed JSON, for a protocol that reshapes them.
pub(crate) fn tools_json(request: &CompletionRequest) -> Vec<Value> {
    request
        .tools
        .iter()
        .map(|tool| mcbench_json::parse(&tool.0).expect("a tool schema is JSON"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn usage_normalizes_provider_token_accounting_without_treating_unavailable_values_as_zero() {
        let usage = normalize_usage(
            &json!({"prompt_tokens": 100, "completion_tokens": 20, "prompt_tokens_details": {"cached_tokens": 80}, "completion_tokens_details": {"reasoning_tokens": 10}}),
        );
        assert_eq!(
            usage,
            Usage {
                input_tokens: Some(100.0),
                output_tokens: Some(20.0),
                cached_input_tokens: Some(80.0),
                cache_write_tokens: None,
                reasoning_tokens: Some(10.0)
            }
        );
        assert_eq!(
            normalize_usage(&json!({"input_tokens": 10, "cache_creation_input_tokens": 20, "cache_read_input_tokens": 30})).input_tokens,
            Some(60.0)
        );
    }

    #[test]
    fn gemini_errors_arrive_as_a_one_element_array_and_read_as_their_message() {
        let body = r#"[{"error":{"code":400,"message":"Function call is missing a thought_signature","status":"INVALID_ARGUMENT"}}]"#;
        assert_eq!(
            provider_error_message("fixture-key", 400, body),
            "LLM request failed (HTTP 400): Function call is missing a thought_signature"
        );
    }

    #[test]
    fn a_key_past_the_first_400_characters_is_replaced_before_the_answer_is_cut() {
        let key = ["sk", "proj", "FAKEfixture0123456789abcdefFAKE"].join("-");
        let cut = provider_error_message(&key, 500, &format!("{}{key}", "x".repeat(390)));
        assert!(!cut.contains(&key[..10]), "{cut}");
    }
}
