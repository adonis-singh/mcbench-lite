//! Today's tests that reach a provider over HTTP, ported against
//! [`crate::fake`]: `test/llm.test.js` (the eight that stub `fetch`),
//! `llm-catalog.test.js`, `llm-catalog-refresh.test.js`,
//! `llm-google.test.js`, `qwencloud.test.js` (the two with HTTP) and
//! `codex.test.js` (the nine with HTTP or a login file). Each keeps its
//! cases and expected values; where Node stubbed a module this goes through
//! the same code over the fake's HTTP.

use std::time::Duration;

use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use crate::api::{Api, oauth};
use crate::fake::{Answer, Chunk, Fake, Seen};
use crate::{
    ApiMessage, AssistantMessage, CliRuntime, CompletionRequest, Delta, Provider, ProviderConfig,
    ProviderError, ProviderId, ToolSchema,
};

fn config(id: ProviderId, key: &str) -> ProviderConfig {
    ProviderConfig {
        id,
        api_key: key.into(),
        enabled: true,
        connected: !key.is_empty(),
        cli: None,
    }
}

fn messages(value: Value) -> Vec<ApiMessage> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|m| ApiMessage(m.as_object().unwrap().clone()))
        .collect()
}

fn tools(value: Value) -> Vec<ToolSchema> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|t| ToolSchema(mcbench_json::stringify(t)))
        .collect()
}

fn request(
    model: &str,
    effort: Option<&str>,
    messages: Vec<ApiMessage>,
    tools: Vec<ToolSchema>,
) -> CompletionRequest {
    CompletionRequest {
        conversation_id: None,
        model: model.into(),
        effort: effort.map(String::from),
        messages,
        tools,
        chat: false,
        transcript: None,
    }
}

fn sse(events: &[Value]) -> String {
    events
        .iter()
        .map(|e| format!("data: {}\n\n", mcbench_json::stringify(e)))
        .collect()
}

async fn complete(
    provider: &dyn Provider,
    request: &CompletionRequest,
) -> (Result<AssistantMessage, ProviderError>, Vec<Delta>) {
    let mut deltas = Vec::new();
    let result = provider
        .complete(request, &mut |d| deltas.push(d), &CancellationToken::new())
        .await;
    (result, deltas)
}

const CHAT: &str = "POST /openrouter.ai/api/v1/chat/completions";
const CATALOG: &str = "GET /openrouter.ai/api/v1/models";

#[tokio::test]
async fn chat_completion_routes_an_explicit_provider_and_low_reasoning_effort() {
    let fake = Fake::start().await;
    fake.on(CHAT, Answer::sse("data: {\"choices\":[{\"delta\":{\"content\":\"Horse Build\"}}]}\n\ndata: {\"choices\":[{\"finish_reason\":\"stop\",\"delta\":{}}]}\n"));
    let api = Api::new(fake.endpoints());
    let provider = api.provider(config(ProviderId::OpenRouter, "test-key"));
    let (result, _) = complete(
        &*provider,
        &request(
            "openai/gpt-5.6-luna",
            Some("low"),
            messages(json!([{"role": "user", "content": "Name a horse build"}])),
            Vec::new(),
        ),
    )
    .await;
    assert_eq!(result.unwrap().content, "Horse Build");
    let seen = &fake.seen()[0];
    assert_eq!(seen.header("authorization"), Some("Bearer test-key"));
    let body = seen.json();
    assert_eq!(body["model"], "openai/gpt-5.6-luna");
    assert_eq!(body["reasoning_effort"], "low");
    assert!(body.get("tools").is_none());
}

#[tokio::test]
async fn streamed_tool_deltas_retain_stable_call_ids() {
    let fake = Fake::start().await;
    fake.on(CHAT, Answer::sse(concat!(
        "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call-one\",\"function\":{\"name\":\"build\",\"arguments\":\"{\\\"summary\\\":\\\"one\\\"}\"}}]}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":1,\"id\":\"call-two\",\"function\":{\"name\":\"bash\",\"arguments\":\"{\\\"command\\\":\\\"pwd\\\"}\"}}]}}]}\n\n",
        "data: {\"choices\":[{\"finish_reason\":\"tool_calls\",\"delta\":{}}]}\n",
    )));
    let api = Api::new(fake.endpoints());
    let provider = api.provider(config(ProviderId::OpenRouter, "test-key"));
    let (_, deltas) = complete(
        &*provider,
        &request(
            "test/tool-model",
            None,
            messages(json!([{"role": "user", "content": "Use tools"}])),
            tools(json!([{"type": "function", "function": {"name": "build", "parameters": {}}}])),
        ),
    )
    .await;
    let ids: Vec<String> = deltas
        .iter()
        .filter_map(|d| match d {
            Delta::ToolCall { id, .. } => Some(id.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(ids, ["call-one", "call-two"]);
}

#[tokio::test]
async fn gemini_tool_calls_come_apart_without_an_index_and_send_their_thought_signatures_back() {
    // As Gemini's OpenAI-compatible endpoint streams them: each call whole in
    // one chunk with no index, a thought signature on the first call of a
    // parallel batch, and finish_reason "stop" even with tool calls.
    let stream = concat!(
        "data: {\"choices\":[{\"delta\":{\"role\":\"assistant\",\"tool_calls\":[{\"extra_content\":{\"google\":{\"thought_signature\":\"sig-one\"}},\"function\":{\"arguments\":\"{\\\"city\\\":\\\"Paris\\\"}\",\"name\":\"get_weather\"},\"id\":\"call-one\",\"type\":\"function\"}]},\"index\":0}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"role\":\"assistant\",\"tool_calls\":[{\"function\":{\"arguments\":\"{\\\"city\\\":\\\"Tokyo\\\"}\",\"name\":\"get_weather\"},\"id\":\"call-two\",\"type\":\"function\"}]},\"index\":0}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"role\":\"assistant\"},\"finish_reason\":\"stop\",\"index\":0}],\"usage\":{\"completion_tokens\":32,\"prompt_tokens\":60,\"total_tokens\":92}}\n\n",
    );
    let route = "POST /generativelanguage.googleapis.com/v1beta/openai/chat/completions";
    let fake = Fake::start().await;
    fake.on(route, Answer::sse(stream));
    fake.on(route, Answer::sse("data: {\"choices\":[{\"delta\":{\"content\":\"18 in both\"},\"finish_reason\":\"stop\",\"index\":0}]}\n\n"));
    let api = Api::new(fake.endpoints());
    let provider = api.provider(config(ProviderId::Google, "test-key"));
    let tools =
        tools(json!([{"type": "function", "function": {"name": "get_weather", "parameters": {}}}]));
    let question = json!({"role": "user", "content": "Weather in Paris and Tokyo?"});
    let (first, _) = complete(
        &*provider,
        &request(
            "gemini-3.8-flash",
            Some("low"),
            messages(json!([question])),
            tools.clone(),
        ),
    )
    .await;
    let first = first.unwrap();
    assert_eq!(
        fake.seen()[0].header("authorization"),
        Some("Bearer test-key")
    );
    assert_eq!(fake.seen()[0].json()["reasoning_effort"], "low");
    let calls: Vec<Value> = first.tool_calls.iter().map(|c| c.to_json()).collect();
    assert_eq!(
        Value::Array(calls.clone()),
        json!([
            {"id": "call-one", "type": "function", "function": {"name": "get_weather", "arguments": "{\"city\":\"Paris\"}"}, "extra_content": {"google": {"thought_signature": "sig-one"}}},
            {"id": "call-two", "type": "function", "function": {"name": "get_weather", "arguments": "{\"city\":\"Tokyo\"}"}},
        ])
    );
    let mut assistant = first.to_json();
    assistant.shift_remove("telemetry");
    let mut history = vec![question, Value::Object(assistant)];
    history.extend(
        first
            .tool_calls
            .iter()
            .map(|c| json!({"role": "tool", "tool_call_id": c.id, "content": "{\"c\":18}"})),
    );
    let (second, _) = complete(
        &*provider,
        &request(
            "gemini-3.8-flash",
            None,
            messages(Value::Array(history)),
            tools,
        ),
    )
    .await;
    assert_eq!(second.unwrap().content, "18 in both");
    assert_eq!(
        fake.seen()[1].json()["messages"][1]["tool_calls"],
        Value::Array(calls)
    );
}

#[tokio::test]
async fn a_provider_error_that_quotes_the_api_key_reaches_the_caller_with_key_in_its_place() {
    let key = ["sk", "proj", "FAKEfixture0123456789abcdefFAKE"].join("-");
    let fake = Fake::start().await;
    // OpenAI's 401 quotes the key it was sent (it shows only the ends of a
    // real one; the whole key is quoted here to test every copy).
    fake.on("POST /api.openai.com/v1/chat/completions", Answer::json(401, &json!({"error": {
        "message": format!("Incorrect API key provided: {key}. You can find your API key at https://platform.openai.com/account/api-keys. ({key})"),
        "type": "invalid_request_error", "code": "invalid_api_key"}})));
    let api = Api::new(fake.endpoints());
    let provider = api.provider(config(ProviderId::OpenAi, &key));
    let (result, _) = complete(
        &*provider,
        &request(
            "gpt-fixture",
            None,
            messages(json!([{"role": "user", "content": "hi"}])),
            Vec::new(),
        ),
    )
    .await;
    assert_eq!(
        result.unwrap_err().message,
        "LLM request failed (HTTP 401): Incorrect API key provided: [key]. You can find your API key at https://platform.openai.com/account/api-keys. ([key])"
    );
}

#[tokio::test]
async fn request_telemetry_survives_success_failure_and_incomplete_tool_streams_never_execute() {
    let fake = Fake::start().await;
    fake.on(CHAT, Answer::sse("data: {\"choices\":[{\"delta\":{\"content\":\"ok\"},\"finish_reason\":\"stop\"}]}\n\ndata: {\"choices\":[],\"usage\":{\"prompt_tokens\":123,\"completion_tokens\":4,\"prompt_tokens_details\":{\"cached_tokens\":99}}}\n\n"));
    fake.on(CHAT, Answer::sse("data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"unfinished\",\"function\":{\"name\":\"build\",\"arguments\":\"{}\"}}]}}]}\n\n"));
    // A completion cut off after streaming text keeps that text as `reply`.
    fake.on(CHAT, Answer::sse("data: {\"choices\":[{\"delta\":{\"content\":\"First the \"}}]}\n\ndata: {\"choices\":[{\"delta\":{\"content\":\"floor\"}}]}\n\n"));
    let api = Api::new(fake.endpoints());
    let provider = api.provider(config(ProviderId::OpenRouter, "test-key"));
    let run = request(
        "fixture",
        Some("max"),
        messages(json!([{"role": "user", "content": "Run"}])),
        Vec::new(),
    );
    let (result, _) = complete(&*provider, &run).await;
    assert_eq!(
        fake.seen()[0].json()["stream_options"]["include_usage"],
        true
    );
    let telemetry = result.unwrap().telemetry;
    let usage = telemetry.usage.unwrap();
    assert_eq!(usage.input_tokens, Some(123.0));
    assert_eq!(usage.cached_input_tokens, Some(99.0));
    assert_eq!(telemetry.effort.as_deref(), Some("max"));
    assert!(telemetry.wall_ms >= telemetry.first_delta_ms.unwrap());
    let (failed, _) = complete(&*provider, &run).await;
    let failed = failed.unwrap_err();
    assert!(
        failed.message.contains("without a finish reason"),
        "{}",
        failed.message
    );
    assert_eq!(failed.telemetry.unwrap().to_json()["failed"], true);
    let (cut, _) = complete(&*provider, &run).await;
    assert_eq!(cut.unwrap_err().reply, "First the floor");
}

#[tokio::test]
async fn anthropic_collects_input_cache_output_usage_and_requires_its_completion_event() {
    let mut events = vec![
        json!({"type": "message_start", "message": {"usage": {"input_tokens": 10, "cache_creation_input_tokens": 20, "cache_read_input_tokens": 30, "output_tokens": 0}}}),
        json!({"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": "ok"}}),
        json!({"type": "message_delta", "delta": {"stop_reason": "end_turn"}, "usage": {"output_tokens": 4}}),
        json!({"type": "message_stop"}),
    ];
    let fake = Fake::start().await;
    // The catalog lists the model's effort levels, so its thinking is summarized.
    fake.on(CATALOG, Answer::json(200, &json!({"data": [{"id": "anthropic/fixture", "reasoning": {"supported_efforts": ["low", "high"]}}]})));
    fake.on(
        "POST /api.anthropic.com/v1/messages",
        Answer::sse(&sse(&events)),
    );
    events.pop();
    fake.on(
        "POST /api.anthropic.com/v1/messages",
        Answer::sse(&sse(&events)),
    );
    let api = Api::new(fake.endpoints());
    let provider = api.provider(config(ProviderId::Anthropic, "test-key"));
    let reply = request(
        "fixture",
        None,
        messages(json!([{"role": "user", "content": "Reply"}])),
        Vec::new(),
    );
    let (result, _) = complete(&*provider, &reply).await;
    let sent = fake
        .seen()
        .into_iter()
        .find(|s| s.target.ends_with("/messages"))
        .unwrap();
    assert_eq!(
        sent.json()["thinking"],
        json!({"type": "adaptive", "display": "summarized", "block_binding": {"prefix_mismatch_behavior": "error"}})
    );
    let usage = result.unwrap().telemetry.usage.unwrap();
    assert_eq!(usage.input_tokens, Some(60.0));
    assert_eq!(usage.output_tokens, Some(4.0));
    assert_eq!(usage.cached_input_tokens, Some(30.0));
    let (result, _) = complete(&*provider, &reply).await;
    assert!(result.unwrap_err().message.contains("before message_stop"));
}

#[tokio::test]
async fn concurrent_catalog_lookups_share_one_fetch() {
    let fake = Fake::start().await;
    fake.on(
        CATALOG,
        Answer {
            status: 502,
            headers: Vec::new(),
            chunks: vec![Chunk::Wait(20)],
        },
    );
    let api = Api::new(fake.endpoints());
    let provider = api.provider(config(ProviderId::OpenRouter, "k"));
    let (a, b) = tokio::join!(
        provider.model_info("openai/gpt-5.5"),
        provider.model_info("openai/gpt-5.5")
    );
    assert!(a.is_err() && b.is_err());
    assert_eq!(fake.seen().len(), 1);
}

#[tokio::test]
async fn an_openrouter_outage_is_not_a_transient_anthropic_error_and_a_model_is_looked_up_once() {
    let stream = sse(&[
        json!({"type": "message_start", "message": {"usage": {"input_tokens": 1, "output_tokens": 0}}}),
        json!({"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": "ok"}}),
        json!({"type": "message_delta", "delta": {"stop_reason": "end_turn"}, "usage": {"output_tokens": 1}}),
        json!({"type": "message_stop"}),
    ]);
    let messages_route = "POST /api.anthropic.com/v1/messages";
    let fake = Fake::start().await;
    fake.on(CATALOG, Answer::text(503, "unavailable"));
    let api = Api::new(fake.endpoints());
    let provider = api.provider(config(ProviderId::Anthropic, "test-key"));
    let reply = request(
        "fixture-2",
        None,
        messages(json!([{"role": "user", "content": "Reply"}])),
        Vec::new(),
    );
    let error = complete(&*provider, &reply).await.0.unwrap_err();
    assert!(
        error
            .message
            .contains("catalog could not describe fixture-2"),
        "{}",
        error.message
    );
    assert!(!error.retryable);
    assert!(!fake.seen().iter().any(|s| s.target.ends_with("/messages")));
    fake.on(CATALOG, Answer::json(200, &json!({"data": [{"id": "anthropic/fixture-2", "reasoning": {"supported_efforts": ["low", "high"]}}]})));
    fake.on(messages_route, Answer::sse(&stream));
    assert_eq!(complete(&*provider, &reply).await.0.unwrap().content, "ok");
    // Later requests skip the catalog, even after it goes down again.
    fake.on(CATALOG, Answer::text(503, "unavailable"));
    fake.on(messages_route, Answer::sse(&stream));
    let before = fake.seen().len();
    assert_eq!(complete(&*provider, &reply).await.0.unwrap().content, "ok");
    let after: Vec<String> = fake.seen()[before..]
        .iter()
        .map(|s| s.target.clone())
        .collect();
    assert_eq!(after, ["/api.anthropic.com/v1/messages"]);
}

#[tokio::test]
async fn the_catalog_is_fetched_at_start_and_on_a_timer_turns_read_the_last_one_and_a_failed_refresh_leaves_it()
 {
    let entry = |vision: bool| json!({"data": [{"id": "openai/fixture", "architecture": {"input_modalities": if vision { json!(["text", "image"]) } else { json!(["text"]) }}, "supported_parameters": ["tools"]}]});
    let fake = Fake::start().await;
    fake.on(CATALOG, Answer::json(200, &entry(true)));
    fake.on(CATALOG, Answer::json(200, &entry(false)));
    fake.on(CATALOG, Answer::text(503, "unavailable"));
    let api = Api::new(fake.endpoints());
    let provider = api.provider(config(ProviderId::OpenRouter, "k"));
    // The timer's period is CATALOG_REFRESH; here a shorter one stands in
    // for Node's mocked setInterval, long enough that a slow machine does
    // the steps between two fetches inside it. Each fetch is waited for.
    let period = Duration::from_secs(3);
    api.catalog.start_every(period);
    let image_input = || async {
        provider
            .model_info("openai/fixture")
            .await
            .unwrap()
            .image_input
    };
    let started = std::time::Instant::now();
    let bounded = |what: &str| {
        assert!(started.elapsed() < Duration::from_secs(60), "{what}");
    };
    // A turn before the first fetch has finished waits for that fetch.
    assert_eq!(image_input().await, Some(true));
    assert_eq!(image_input().await, Some(true));
    assert_eq!(fake.seen().len(), 1);
    // The timer's next fetch, read once its answer has come.
    while image_input().await != Some(false) {
        bounded("the timer's second fetch");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(fake.seen().len(), 2);
    // The one after it fails, and the catalog stays.
    while fake.seen().len() < 3 {
        bounded("the timer's third fetch");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(image_input().await, Some(false));
    api.catalog.stop();
    assert_eq!(
        fake.seen().len(),
        3,
        "a turn does not fetch while a catalog is held"
    );
}

#[tokio::test]
async fn google_lists_the_models_its_key_can_chat_with_described_by_the_catalog_newest_first() {
    let fake = Fake::start().await;
    fake.on(CATALOG, Answer::json(200, &json!({"data": [
        {"id": "google/gemini-3.8-flash", "created": 300, "context_length": 1048576, "architecture": {"input_modalities": ["text", "image"], "output_modalities": ["text"]}, "supported_parameters": ["tools", "reasoning"], "reasoning": {"mandatory": true, "supported_efforts": ["high", "medium", "low"], "default_effort": "medium"}},
        {"id": "google/gemini-2.5-flash", "created": 100, "context_length": 1048576, "architecture": {"input_modalities": ["text", "image"], "output_modalities": ["text"]}, "supported_parameters": ["tools"]},
        {"id": "google/gemini-3.1-flash-image", "created": 200, "architecture": {"input_modalities": ["text", "image"], "output_modalities": ["image", "text"]}, "supported_parameters": []},
    ]})));
    // Gemini's own model list, as models.list returns it: two pages, and
    // models that cannot chat beside the ones that can.
    fake.on("GET /generativelanguage.googleapis.com/v1beta/models?pageSize=1000", Answer::json(200, &json!({"models": [
        {"name": "models/gemini-2.5-flash", "displayName": "Gemini 2.5 Flash", "inputTokenLimit": 1048576, "supportedGenerationMethods": ["generateContent", "countTokens"]},
        {"name": "models/gemini-embedding-2", "displayName": "Gemini Embedding 2", "inputTokenLimit": 8192, "supportedGenerationMethods": ["embedContent"]},
    ], "nextPageToken": "page-2"})));
    fake.on("GET /generativelanguage.googleapis.com/v1beta/models?pageSize=1000&pageToken=page-2", Answer::json(200, &json!({"models": [
        {"name": "models/gemini-3.8-flash", "displayName": "Gemini 3.8 Flash", "inputTokenLimit": 1048576, "supportedGenerationMethods": ["generateContent"]},
        {"name": "models/gemini-3.1-flash-image", "displayName": "Nano Banana 2", "supportedGenerationMethods": ["generateContent"]},
        {"name": "models/gemini-3.8-live", "displayName": "Gemini 3.8 Live", "supportedGenerationMethods": ["bidiGenerateContent"]},
    ]})));
    let api = Api::new(fake.endpoints());
    let provider = api.provider(config(ProviderId::Google, "test-key"));
    let models = provider.models().await.unwrap();
    let pages: Vec<Seen> = fake
        .seen()
        .into_iter()
        .filter(|s| s.target.contains("pageSize"))
        .collect();
    assert_eq!(pages.len(), 2);
    for page in &pages {
        // Gemini's own API takes its key in this header; a Bearer key is refused there.
        assert_eq!(page.header("x-goog-api-key"), Some("test-key"));
        assert_eq!(page.header("authorization"), None);
    }
    let rows: Vec<(String, String, f64)> = models
        .iter()
        .map(|m| (m.id.clone(), m.name.clone(), m.created))
        .collect();
    assert_eq!(
        rows,
        [
            ("gemini-3.8-flash".into(), "Gemini 3.8 Flash".into(), 300.0),
            ("gemini-2.5-flash".into(), "Gemini 2.5 Flash".into(), 100.0)
        ]
    );
    let facts = &models[0].facts;
    assert_eq!(
        (
            facts.image_input,
            facts.tools,
            facts.efforts.clone(),
            facts.default_effort.clone(),
            facts.mandatory_reasoning,
            facts.context_window
        ),
        (
            Some(true),
            Some(true),
            vec!["low".to_string(), "medium".into(), "high".into()],
            Some("medium".to_string()),
            true,
            Some(Some(1_048_576.0))
        )
    );
    assert_eq!(
        provider
            .model_info("gemini-3.8-flash")
            .await
            .unwrap()
            .efforts,
        ["low", "medium", "high"]
    );
}

#[tokio::test]
async fn claude_code_lists_anthropics_models_from_the_catalog_and_asks_no_provider() {
    let fake = Fake::start().await;
    fake.on(CATALOG, Answer::json(200, &json!({"data": [
        {"id": "anthropic/claude-haiku-5-5", "name": "Anthropic: Claude Haiku 5.5", "created": 400, "supported_parameters": ["tools"]},
        {"id": "anthropic/claude-opus-5.5", "name": "Anthropic: Claude Opus 5.5", "created": 500, "supported_parameters": ["tools"]},
        {"id": "anthropic/claude-opus-5.5:batch", "name": "Anthropic: Claude Opus 5.5 (batch)", "created": 500},
        {"id": "openai/gpt-5.5", "name": "OpenAI: GPT-5.5", "created": 900},
    ]})));
    let api = Api::new(fake.endpoints());
    let models = api
        .catalog
        .list_models(&api, &config(ProviderId::ClaudeCode, "signed-in"))
        .await
        .unwrap();
    let rows: Vec<(&str, &str, Option<bool>)> = models
        .iter()
        .map(|m| (m.id.as_str(), m.name.as_str(), m.facts.tools))
        .collect();
    // Anthropic's native ids, newest first, without the catalog's variants
    // or another vendor's models, each described by its catalog entry.
    assert_eq!(
        rows,
        [
            ("claude-opus-5-5", "Anthropic: Claude Opus 5.5", Some(true)),
            (
                "claude-haiku-5-5",
                "Anthropic: Claude Haiku 5.5",
                Some(true)
            ),
        ]
    );
    // Only the catalog was asked: a CLI has no models address.
    for seen in fake.seen() {
        assert!(
            seen.target.contains("openrouter.ai/api/v1/models"),
            "{}",
            seen.target
        );
    }
}

#[tokio::test]
async fn qwencloud_authenticates_its_native_catalog() {
    let fake = Fake::start().await;
    fake.on("GET /dashscope-intl.aliyuncs.com/compatible-mode/v1/models", Answer::json(200, &json!({"data": [{"id": "qwen3.8-flash", "created": 100}, {"id": "qwen-turbo", "created": 999}]})));
    let api = Api::new(fake.endpoints());
    let provider = api.provider(config(ProviderId::QwenCloud, "qwen-test-key"));
    let ids: Vec<String> = provider
        .models()
        .await
        .unwrap()
        .into_iter()
        .map(|m| m.id)
        .collect();
    assert_eq!(ids, ["qwen3.8-flash"]);
    assert_eq!(
        fake.seen()[0].header("authorization"),
        Some("Bearer qwen-test-key")
    );
}

#[tokio::test]
async fn qwencloud_streams_thinking_and_tool_calls_then_replays_reasoning_with_tool_results_and_images()
 {
    let route = "POST /dashscope-intl.aliyuncs.com/compatible-mode/v1/chat/completions";
    let fake = Fake::start().await;
    fake.on(route, Answer::sse(&sse(&[
        json!({"choices": [{"delta": {"reasoning_content": "Inspect the shape."}}]}),
        json!({"choices": [{"delta": {"tool_calls": [{"index": 0, "id": "call-build", "function": {"name": "build", "arguments": "{\"program\":"}}]}}]}),
        json!({"choices": [{"delta": {"tool_calls": [{"index": 0, "function": {"arguments": "\"sample\"}"}}]}, "finish_reason": "tool_calls"}]}),
    ])));
    fake.on(
        route,
        Answer::sse(&sse(&[
            json!({"choices": [{"delta": {"content": "Verified."}, "finish_reason": "stop"}]}),
        ])),
    );
    let api = Api::new(fake.endpoints());
    let provider = api.provider(config(ProviderId::QwenCloud, "qwen-test-key"));
    let user = json!({"role": "user", "content": [{"type": "text", "text": "Build this"}, {"type": "image_url", "image_url": {"url": "data:image/png;base64,YWJj"}}]});
    let tool_list = json!([{"type": "function", "function": {"name": "build", "parameters": {"type": "object"}}}]);
    let (first, deltas) = complete(
        &*provider,
        &request(
            "qwen3.8-flash",
            Some("low"),
            messages(json!([user])),
            tools(tool_list.clone()),
        ),
    )
    .await;
    let first = first.unwrap();
    assert_eq!(first.reasoning.as_deref(), Some("Inspect the shape."));
    assert_eq!(first.finish_reason.as_deref(), Some("tool_calls"));
    assert_eq!(
        serde_json::from_str::<Value>(&first.tool_calls[0].arguments).unwrap(),
        json!({"program": "sample"})
    );
    assert_eq!(deltas[0], Delta::Reasoning("Inspect the shape.".into()));
    assert!(matches!(&deltas[2], Delta::ToolCall { id, .. } if id == "call-build"));
    let body = fake.seen()[0].json();
    assert_eq!(body["reasoning_effort"], "low");
    assert!(body.get("thinking_budget").is_none());
    assert_eq!(body["messages"][0], user);
    assert_eq!(body["tools"], tool_list);
    let tool_result =
        json!({"role": "tool", "tool_call_id": "call-build", "content": "{\"ok\":true}"});
    let history = json!([user, Value::Object(first.to_json()), tool_result]);
    let (second, _) = complete(
        &*provider,
        &request("qwen3.8-flash", Some("none"), messages(history), Vec::new()),
    )
    .await;
    assert_eq!(second.unwrap().content, "Verified.");
    let body = fake.seen()[1].json();
    assert_eq!(body["enable_thinking"], false);
    assert!(body.get("reasoning_effort").is_none());
    assert_eq!(
        body["messages"][1]["reasoning_content"],
        "Inspect the shape."
    );
    assert!(body["messages"][1].get("reasoning").is_none());
    assert!(body["messages"][1].get("finish_reason").is_none());
    assert_eq!(body["messages"][2], tool_result);
}

// ---- the ChatGPT sign-in ---------------------------------------------------

const RESPONSES: &str = "POST /chatgpt.com/backend-api/codex/responses";

fn scratch(name: &str) -> crate::scratch::Scratch {
    crate::scratch::Scratch::new(name)
}

fn jwt(claims: Value) -> String {
    use base64::Engine as _;
    let b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD;
    format!(
        "{}.{}.sig",
        b64.encode("{}"),
        b64.encode(mcbench_json::stringify(&claims))
    )
}

const ACCOUNT: &str = "acct-fixture";

/// A Codex login with a token valid until 2100.
fn signed_in(home: &std::path::Path) {
    let claims = json!({"exp": 4_102_444_800u64, "https://api.openai.com/auth": {"chatgpt_account_id": ACCOUNT}});
    std::fs::write(home.join("auth.json"), mcbench_json::stringify(&json!({"tokens": {"access_token": jwt(claims), "refresh_token": "rt", "account_id": ACCOUNT}}))).unwrap();
}

fn codex(home: &std::path::Path, connected: bool, program: &str) -> ProviderConfig {
    ProviderConfig {
        id: ProviderId::Codex,
        api_key: String::new(),
        enabled: true,
        connected,
        cli: Some(CliRuntime {
            program: program.into(),
            args: Vec::new(),
            home: home.into(),
            env: vec![("PATH".into(), std::env::var("PATH").unwrap_or_default())],
        }),
    }
}

fn completed() -> Value {
    json!({"type": "response.completed", "response": {"output": [], "usage": {"input_tokens": 7, "output_tokens": 2}}})
}

#[test]
fn chatgpt_sign_in_resolves_to_a_keyless_provider_with_no_network_address() {
    let home = scratch("keyless");
    let config = codex(&home, true, "codex");
    assert_eq!(config.api_key, "");
    crate::api::resolve(&config).unwrap();
    // The ChatGPT sign-in has no base address of a key provider.
    assert!(
        std::panic::catch_unwind(|| crate::api::Endpoints::today().base(ProviderId::Codex).len())
            .is_err()
    );
}

#[tokio::test]
async fn chatgpt_requests_go_to_the_backend_with_the_login_never_an_api_key() {
    let home = scratch("backend");
    signed_in(&home);
    let fake = Fake::start().await;
    fake.on(RESPONSES, Answer::sse(&format!(
        "data: {{\"type\":\"response.output_text.delta\",\"delta\":\"ok\"}}\n\ndata: {{\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{{\"type\":\"message\",\"content\":[{{\"type\":\"output_text\",\"text\":\"ok\"}}]}}}}\n\n{}",
        sse(&[completed()])
    )));
    let api = Api::new(fake.endpoints());
    let provider = api.provider(codex(&home, true, "codex"));
    let (result, _) = complete(
        &*provider,
        &request(
            "gpt-6-astra",
            Some("high"),
            messages(json!([{"role": "user", "content": "Reply with ok"}])),
            Vec::new(),
        ),
    )
    .await;
    let result = result.unwrap();
    assert_eq!(result.content, "ok");
    assert_eq!(result.telemetry.usage.unwrap().input_tokens, Some(7.0));
    let sent: Vec<Seen> = fake
        .seen()
        .into_iter()
        .filter(|s| s.target.ends_with("/responses"))
        .collect();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].method, "POST");
    assert!(
        sent[0]
            .header("authorization")
            .unwrap()
            .starts_with("Bearer e30.")
    );
    assert_eq!(sent[0].header("chatgpt-account-id"), Some(ACCOUNT));
    let body = sent[0].json();
    assert_eq!(body["model"], "gpt-6-astra");
    assert_eq!(
        body["reasoning"],
        json!({"effort": "high", "summary": "auto"})
    );
    assert_eq!(body["store"], false);
}

#[tokio::test]
async fn a_streamed_chatgpt_response_reaches_the_turn_intact_chunk_by_chunk() {
    let home = scratch("chunks");
    signed_in(&home);
    let call = json!({"type": "function_call", "id": "fc_1", "call_id": "call_1", "name": "build", "arguments": "{\"program\":\"mc.set([0,0,0], \\\"stone\\\")\"}"});
    let arguments = call["arguments"].as_str().unwrap().to_owned();
    let mut added = call.clone();
    added["arguments"] = "".into();
    let text = format!(
        "{}{}",
        sse(&[
            json!({"type": "response.output_text.delta", "delta": "Building "}),
            json!({"type": "response.output_text.delta", "delta": "now"}),
            json!({"type": "response.output_item.added", "output_index": 1, "item": added}),
            json!({"type": "response.function_call_arguments.delta", "output_index": 1, "delta": arguments[..12]}),
            json!({"type": "response.function_call_arguments.delta", "output_index": 1, "delta": arguments[12..]}),
            json!({"type": "response.output_item.done", "output_index": 0, "item": {"type": "message", "content": [{"type": "output_text", "text": "Building now"}]}}),
            json!({"type": "response.output_item.done", "output_index": 1, "item": call}),
        ]),
        sse(&[completed()])
    );
    // Split mid-record and released one piece at a time.
    let chars: Vec<char> = text.chars().collect();
    let mut chunks = Vec::new();
    for piece in chars.chunks(37) {
        chunks.push(Chunk::Text(piece.iter().collect()));
        chunks.push(Chunk::Wait(1));
    }
    let fake = Fake::start().await;
    fake.on(
        RESPONSES,
        Answer {
            status: 200,
            headers: vec![("content-type".into(), "text/event-stream".into())],
            chunks,
        },
    );
    let api = Api::new(fake.endpoints());
    let provider = api.provider(codex(&home, true, "codex"));
    let (result, deltas) = complete(
        &*provider,
        &request(
            "gpt-6-astra",
            None,
            messages(json!([{"role": "user", "content": "Build"}])),
            Vec::new(),
        ),
    )
    .await;
    let result = result.unwrap();
    assert_eq!(result.content, "Building now");
    let content: Vec<&str> = deltas
        .iter()
        .filter_map(|d| match d {
            Delta::Content(t) => Some(t.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(content, ["Building ", "now"]);
    let args: String = deltas
        .iter()
        .filter_map(|d| match d {
            Delta::ToolCall { arguments, .. } => Some(arguments.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(args, arguments);
    assert_eq!(result.tool_calls[0].id, "call_1");
    assert_eq!(result.tool_calls[0].arguments, arguments);
}

#[tokio::test]
async fn without_a_codex_login_chatgpt_is_disconnected_and_says_to_sign_in_from_settings() {
    let home = scratch("no-login");
    let fake = Fake::start().await;
    let api = Api::new(fake.endpoints());
    let provider = api.provider(codex(&home, false, "codex"));
    let message = "ChatGPT is not signed in: sign in to it in Settings";
    let (result, _) = complete(
        &*provider,
        &request(
            "gpt-6-astra",
            None,
            messages(json!([{"role": "user", "content": "hi"}])),
            Vec::new(),
        ),
    )
    .await;
    assert_eq!(result.unwrap_err().message, message);
    assert_eq!(provider.models().await.unwrap_err().message, message);
    assert!(fake.seen().is_empty());
}

#[tokio::test]
async fn the_login_is_read_from_codex_home_and_one_without_tokens_fails_before_any_request() {
    let home = scratch("empty-login");
    std::fs::write(home.join("auth.json"), "{}").unwrap();
    let fake = Fake::start().await;
    let api = Api::new(fake.endpoints());
    let provider = api.provider(codex(&home, true, "codex"));
    let (result, _) = complete(
        &*provider,
        &request(
            "gpt-6-astra",
            None,
            messages(json!([{"role": "user", "content": "hi"}])),
            Vec::new(),
        ),
    )
    .await;
    assert!(
        result
            .unwrap_err()
            .message
            .contains("ChatGPT access token not found")
    );
    assert!(fake.seen().is_empty());
}

/// Writes `text` as the program `path`, executable, through a shell of its
/// own: this process never holds the file open for writing, so another
/// test's fork (which keeps this process's descriptors until it execs) can
/// never leave the program busy (`ETXTBSY`) when the app starts it.
#[cfg(unix)]
fn write_program(path: &std::path::Path, text: &str) {
    use std::io::Write as _;
    let mut shell = std::process::Command::new("/bin/sh")
        .args(["-c", "cat > \"$1\" && chmod 755 \"$1\"", "sh"])
        .arg(path)
        .stdin(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    shell
        .stdin
        .take()
        .unwrap()
        .write_all(text.as_bytes())
        .unwrap();
    assert!(
        shell.wait().unwrap().success(),
        "{} is written",
        path.display()
    );
}

#[cfg(unix)]
#[tokio::test]
async fn chatgpt_lists_the_live_codex_catalogue_not_a_fixed_model_list() {
    let home = scratch("catalogue");
    signed_in(&home);
    let catalogue = json!({"models": [
        {"slug": "gpt-7-nova", "display_name": "GPT-7 Nova", "visibility": "list", "priority": 1, "supported_reasoning_levels": [{"effort": "low"}, {"effort": "high"}], "default_reasoning_level": "high", "input_modalities": ["text", "image"], "context_window": 400000},
        {"slug": "gpt-6-astra", "display_name": "GPT-6 Astra", "visibility": "list", "priority": 2, "supported_reasoning_levels": [{"effort": "high"}], "default_reasoning_level": "high", "input_modalities": ["text"], "context_window": 272000},
        {"slug": "codex-internal", "display_name": "Internal", "visibility": "hide", "priority": 3},
    ]});
    let program = home.join("codex");
    write_program(
        &program,
        &format!(
            "#!/bin/sh\nprintf '%s' '{}'\n",
            mcbench_json::stringify(&catalogue)
        ),
    );
    let fake = Fake::start().await;
    let api = Api::new(fake.endpoints());
    let provider = api.provider(codex(&home, true, &program.display().to_string()));
    let models = provider.models().await.unwrap();
    let ids: Vec<&str> = models.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(ids, ["gpt-7-nova", "gpt-6-astra"]);
    assert_eq!(models[0].facts.efforts, ["low", "high"]);
    assert_eq!(models[0].facts.image_input, Some(true));
    assert_eq!(
        provider
            .model_info("gpt-7-nova")
            .await
            .unwrap()
            .context_window,
        Some(Some(400_000.0))
    );
    assert!(fake.seen().is_empty());
}

#[tokio::test]
async fn a_chats_codex_requests_carry_its_id_in_the_session_headers_a_one_shot_and_the_token_refresh_carry_none()
 {
    let id = "6f1c2a40-1111-4c3e-9d2b-0a5c8e7f6b21";
    let home = scratch("session");
    // An expired token: the first request refreshes it.
    let expired = json!({"exp": 1_700_000_000u64, "https://api.openai.com/auth": {"chatgpt_account_id": ACCOUNT}});
    std::fs::write(home.join("auth.json"), mcbench_json::stringify(&json!({"tokens": {"access_token": jwt(expired), "refresh_token": "rt", "account_id": ACCOUNT}}))).unwrap();
    let fake = Fake::start().await;
    let fresh = json!({"exp": 4_102_444_800u64, "https://api.openai.com/auth": {"chatgpt_account_id": ACCOUNT}});
    fake.on(
        "POST /auth.openai.com/oauth/token",
        Answer::json(
            200,
            &json!({"access_token": jwt(fresh), "refresh_token": "rt2"}),
        ),
    );
    fake.on(RESPONSES, Answer::sse(&sse(&[completed()])));
    fake.on(RESPONSES, Answer::sse(&sse(&[completed()])));
    let api = Api::new(fake.endpoints());
    let provider = api.provider(codex(&home, true, "codex"));
    let list = messages(
        json!([{"role": "system", "content": "Name this chat"}, {"role": "user", "content": "Build a platform"}]),
    );
    let mut chat = request("gpt-6-astra", None, list.clone(), Vec::new());
    chat.conversation_id = Some(id.into());
    complete(&*provider, &chat).await.0.unwrap();
    complete(&*provider, &request("gpt-6-astra", None, list, Vec::new()))
        .await
        .0
        .unwrap();
    let seen = fake.seen();
    let token = seen
        .iter()
        .find(|s| s.target == "/auth.openai.com/oauth/token")
        .unwrap();
    assert_eq!(token.header("session-id"), None);
    assert_eq!(
        token.json(),
        json!({"grant_type": "refresh_token", "refresh_token": "rt", "client_id": oauth::CLIENT_ID})
    );
    let sent: Vec<&Seen> = seen
        .iter()
        .filter(|s| s.target.ends_with("/responses"))
        .collect();
    assert_eq!(
        (sent[0].header("session-id"), sent[0].header("thread-id")),
        (Some(id), Some(id))
    );
    assert_eq!(
        (sent[1].header("session-id"), sent[1].header("thread-id")),
        (None, None)
    );
    let written: Value =
        serde_json::from_str(&std::fs::read_to_string(home.join("auth.json")).unwrap()).unwrap();
    assert_eq!(written["tokens"]["refresh_token"], "rt2");
    assert_eq!(written["auth_mode"], "chatgpt");
}

async fn codex_events(
    name: &str,
    events: &[Value],
) -> ((Result<AssistantMessage, ProviderError>, Vec<Delta>), Fake) {
    let home = scratch(name);
    signed_in(&home);
    let fake = Fake::start().await;
    fake.on(RESPONSES, Answer::sse(&sse(events)));
    let api = Api::new(fake.endpoints());
    let provider = api.provider(codex(&home, true, "codex"));
    let out = complete(
        &*provider,
        &request("gpt-6-astra", None, Vec::new(), Vec::new()),
    )
    .await;
    (out, fake)
}

#[tokio::test]
async fn codex_streams_tool_arguments_with_stable_ids_and_retains_completed_reasoning_for_replay() {
    let reasoning = json!({"type": "reasoning", "id": "rs_1", "encrypted_content": "opaque", "summary": [{"type": "summary_text", "text": "Preparing"}]});
    let call = json!({"type": "function_call", "id": "fc_1", "call_id": "call_1", "name": "build", "arguments": "{\"program\":\"code\"}"});
    let mut added = call.clone();
    added["arguments"] = "".into();
    let ((result, deltas), _) = codex_events("stable", &[
        json!({"type": "response.reasoning_summary_text.delta", "item_id": "rs_1", "summary_index": 0, "delta": "Preparing"}),
        json!({"type": "response.output_item.added", "output_index": 1, "item": added}),
        json!({"type": "response.function_call_arguments.delta", "output_index": 1, "delta": "{\"program\":"}),
        json!({"type": "response.function_call_arguments.delta", "output_index": 1, "delta": "\"code\"}"}),
        json!({"type": "response.output_item.done", "output_index": 0, "item": reasoning}),
        json!({"type": "response.output_item.done", "output_index": 1, "item": call}),
        json!({"type": "response.completed", "response": {"output": [], "usage": {"input_tokens": 123, "output_tokens": 45, "input_tokens_details": {"cached_tokens": 100}}}}),
    ]).await;
    let message = result.unwrap();
    let usage = message.telemetry.usage.clone().unwrap();
    assert_eq!(
        (usage.input_tokens, usage.cached_input_tokens),
        (Some(123.0), Some(100.0))
    );
    assert_eq!(message.tool_calls[0].id, "call_1");
    assert_eq!(
        message.tool_calls[0].arguments,
        call["arguments"].as_str().unwrap()
    );
    assert_eq!(message.finish_reason.as_deref(), Some("tool_calls"));
    assert_eq!(message.reasoning_details, Some(vec![reasoning]));
    assert_eq!(message.reasoning.as_deref(), Some("Preparing"));
    let mut args = String::new();
    for delta in &deltas {
        if let Delta::ToolCall {
            index,
            id,
            name,
            arguments,
        } = delta
        {
            assert_eq!((index, id.as_str(), name.as_str()), (&0, "call_1", "build"));
            args.push_str(arguments);
        }
    }
    assert_eq!(args, call["arguments"].as_str().unwrap());
}

#[tokio::test]
async fn codex_separates_summary_parts_without_splitting_tokens_or_repeating_completed_text() {
    let items = [
        json!({"type": "reasoning", "id": "rs_1", "summary": [{"type": "summary_text", "text": "**Planning the layout**"}, {"type": "summary_text", "text": "**Sizing the canvas**"}]}),
        json!({"type": "reasoning", "id": "rs_2", "summary": [{"type": "summary_text", "text": "**Choosing materials**"}]}),
    ];
    let mut events = Vec::new();
    for item in &items {
        for (index, part) in item["summary"].as_array().unwrap().iter().enumerate() {
            let text = part["text"].as_str().unwrap();
            events.push(json!({"type": "response.reasoning_summary_part.added", "item_id": item["id"], "summary_index": index}));
            for delta in [&text[..8], &text[8..]] {
                events.push(json!({"type": "response.reasoning_summary_text.delta", "item_id": item["id"], "summary_index": index, "delta": delta}));
            }
            events.push(json!({"type": "response.reasoning_summary_text.done", "item_id": item["id"], "summary_index": index, "text": text}));
            events.push(json!({"type": "response.reasoning_summary_part.done", "item_id": item["id"], "summary_index": index, "part": part}));
        }
        events.push(json!({"type": "response.output_item.done", "item": item}));
    }
    events.push(json!({"type": "response.completed", "response": {"output": []}}));
    let ((result, deltas), _) = codex_events("summary", &events).await;
    let streamed: String = deltas
        .iter()
        .filter_map(|d| match d {
            Delta::Reasoning(t) => Some(t.as_str()),
            _ => None,
        })
        .collect();
    let expected = "**Planning the layout**\n\n**Sizing the canvas**\n\n**Choosing materials**";
    assert_eq!(streamed, expected);
    let message = result.unwrap();
    assert_eq!(message.reasoning.as_deref(), Some(expected));
    assert_eq!(message.reasoning_details, Some(items.to_vec()));
}

#[tokio::test]
async fn codex_fails_on_upstream_errors_and_truncated_streams() {
    let ((failed, _), _) = codex_events(
        "failed",
        &[json!({"type": "response.failed", "response": {"error": {"message": "Quota exceeded"}}})],
    )
    .await;
    assert!(failed.unwrap_err().message.contains("Quota exceeded"));
    let ((truncated, _), _) = codex_events(
        "truncated",
        &[json!({"type": "response.output_text.delta", "delta": "Partial answer"})],
    )
    .await;
    assert!(
        truncated
            .unwrap_err()
            .message
            .contains("ended before response.completed")
    );
}

#[tokio::test]
async fn a_degenerate_tool_stream_is_cancelled_rather_than_waiting_for_all_remaining_output() {
    let home = scratch("degenerate");
    signed_in(&home);
    let item = json!({"type": "function_call", "call_id": "call_stalled", "name": "test_build", "arguments": ""});
    let head = sse(&[
        json!({"type": "response.output_item.added", "output_index": 0, "item": item}),
        json!({"type": "response.function_call_arguments.delta", "output_index": 0, "delta": "{\"probes\":["}),
        json!({"type": "response.function_call_arguments.delta", "output_index": 0, "delta": " ".repeat(16385)}),
    ]);
    let fake = Fake::start().await;
    fake.on(RESPONSES, Answer {
        status: 200,
        headers: vec![("content-type".into(), "text/event-stream".into())],
        chunks: vec![Chunk::Text(head), Chunk::Forever(sse(&[json!({"type": "response.function_call_arguments.delta", "output_index": 0, "delta": " "})]))],
    });
    let api = Api::new(fake.endpoints());
    let provider = api.provider(codex(&home, true, "codex"));
    let (result, _) = complete(
        &*provider,
        &request("gpt-6-astra", None, Vec::new(), Vec::new()),
    )
    .await;
    assert!(
        result
            .unwrap_err()
            .message
            .contains("tool argument stream stalled")
    );
    // The stream was dropped, so the fake's endless answer ends (a minute,
    // in steps of 20 ms: a hang fails, a slow machine does not).
    for _ in 0..3000 {
        if *fake.dropped.lock().unwrap() == 1 {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("the stream was not cancelled");
}

#[test]
fn provider_names_inherited_from_object_prototype_are_unknown_providers() {
    for name in ["constructor", "toString", "__proto__"] {
        assert!(serde_json::from_value::<ProviderId>(Value::String(name.into())).is_err());
    }
}

/// Not one of today's tests: the sentences of a stream past its timeout
/// (`AbortSignal.timeout`) and of a connection that fails before its answer.
#[tokio::test]
async fn a_stream_past_its_timeout_and_a_refused_connection_read_as_nodes_fetch_does() {
    use crate::api::http::{self, Deadline, Request};
    let fake = Fake::start().await;
    fake.on(
        "GET /slow",
        Answer {
            status: 200,
            headers: Vec::new(),
            // Far past the deadline, so the deadline comes first however
            // slowly the machine runs.
            chunks: vec![Chunk::Text("data: {}\n\n".into()), Chunk::Wait(60_000)],
        },
    );
    let client = http::client();
    let deadline = Deadline::new(&CancellationToken::new(), Duration::from_millis(200));
    let url = format!("{}/slow", fake.origin);
    let mut answer = http::send(
        &client,
        Request {
            method: reqwest::Method::GET,
            url: &url,
            headers: Vec::new(),
            body: None,
        },
        &deadline,
    )
    .await
    .unwrap();
    let error = crate::api::sse::read(&mut answer, &mut |_| Ok(()))
        .await
        .unwrap_err();
    assert_eq!(error.message, "The operation was aborted due to timeout");
    assert!(!error.retryable);
    // A port nothing listens on: bound, then let go.
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let url = format!("http://127.0.0.1:{port}/");
    let deadline = Deadline::new(&CancellationToken::new(), Duration::from_secs(10));
    let failed = http::send(
        &client,
        Request {
            method: reqwest::Method::GET,
            url: &url,
            headers: Vec::new(),
            body: None,
        },
        &deadline,
    )
    .await
    .err()
    .unwrap();
    assert_eq!(failed.message(), "fetch failed");
    assert!(crate::retry::is_transient(failed.message(), None));
}
