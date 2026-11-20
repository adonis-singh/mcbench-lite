//! Anthropic's Messages API (`llm.js` `anthropicRequest`, `anthropicBody`,
//! `anthropicCompletion`, `anthropicAdaptive`).
//!
//! A model with effort levels thinks adaptively and returns a readable
//! summary of it (the API default leaves the text empty); a chosen effort
//! sets `output_config.effort` (the shared effort scale is Anthropic's own),
//! and without one the API default effort applies. A model without effort
//! levels runs with its API defaults. The history only grows between
//! requests, so the prompt cache follows the conversation, and a thinking
//! block replayed after an edited history is an error, never quietly
//! dropped.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};

use serde_json::{Map, Value, json};
use tokio::sync::OnceCell;
use tokio_util::sync::CancellationToken;

use super::{Api, Reply, chat, js, provider_error_message, sse, tools_json};
use crate::{
    ApiMessage, CompletionRequest, Delta, ProviderConfig, ProviderError, ProviderId, ToolCall,
};

/// `dataImage`: a base64 data URL's media type and data
/// (`/^data:([^;,]+);base64,(.+)$/`).
pub fn data_image(url: &str) -> Result<(String, String), ProviderError> {
    let refuse = || ProviderError::new("Anthropic images must be base64 data URLs");
    let rest = url.strip_prefix("data:").ok_or_else(refuse)?;
    let end = rest.find([';', ',']).ok_or_else(refuse)?;
    let (media_type, after) = rest.split_at(end);
    let data = after.strip_prefix(";base64,").ok_or_else(refuse)?;
    // `.` stops at a line terminator, and `$` is the end of the text.
    if media_type.is_empty()
        || data.is_empty()
        || data.contains(['\n', '\r', '\u{2028}', '\u{2029}'])
    {
        return Err(refuse());
    }
    Ok((media_type.to_owned(), data.to_owned()))
}

/// `anthropicContent`: text and data-URL images as Anthropic's blocks.
fn content_blocks(content: &Value) -> Result<Vec<Value>, ProviderError> {
    if let Value::String(text) = content {
        return Ok(vec![json!({"type": "text", "text": text})]);
    }
    let parts = content
        .as_array()
        .expect("message content is a string or a list of parts");
    parts
        .iter()
        .map(|part| match part.get("type").and_then(Value::as_str) {
            Some("text") => {
                let mut block = Map::new();
                block.insert("type".into(), "text".into());
                if let Some(text) = part.get("text") {
                    block.insert("text".into(), text.clone());
                }
                Ok(Value::Object(block))
            }
            Some("image_url") => {
                let url = js::get(part.get("image_url"), "url")
                    .and_then(Value::as_str)
                    .expect("an image part has its URL");
                let (media_type, data) = data_image(url)?;
                Ok(json!({"type": "image", "source": {"type": "base64", "media_type": media_type, "data": data}}))
            }
            other => Err(ProviderError::new(format!(
                "Unsupported Anthropic content type: {}",
                match other {
                    Some(name) => name.to_owned(),
                    None => part.get("type").map(js::to_string).unwrap_or_else(|| "undefined".into()),
                }
            ))),
        })
        .collect()
}

/// `toolInput`: Messages needs tool input as an object. A call whose
/// arguments were not valid JSON was answered with an error; it is replayed
/// with the raw text so the model sees what it sent.
fn tool_input(text: Option<&Value>) -> Value {
    let source = match text {
        Some(value) if js::truthy(Some(value)) => js::to_string(value),
        _ => "{}".to_owned(),
    };
    if let Ok(input @ Value::Object(_)) = super::v8json::parse(&source) {
        return input;
    }
    let raw = match text {
        None => "undefined".to_owned(),
        Some(value) => js::to_string(value),
    };
    json!({"invalid_arguments": raw})
}

/// `anthropicRequest`: `{system, messages, tools}`.
pub fn request_parts(
    messages: &[ApiMessage],
    tools: &[Value],
) -> Result<Map<String, Value>, ProviderError> {
    let system = messages
        .iter()
        .filter(|m| m.0.get("role").and_then(Value::as_str) == Some("system"))
        .map(|m| m.0.get("content").map(js::to_string).unwrap_or_default())
        .collect::<Vec<_>>()
        .join("\n\n");
    let mut converted = Vec::new();
    for ApiMessage(message) in messages {
        let role = message
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if role == "system" {
            continue;
        }
        if role == "tool" {
            let mut block = Map::new();
            block.insert("type".into(), "tool_result".into());
            if let Some(id) = message.get("tool_call_id") {
                block.insert("tool_use_id".into(), id.clone());
            }
            if let Some(content) = message.get("content") {
                block.insert("content".into(), content.clone());
            }
            converted.push(json!({"role": "user", "content": [Value::Object(block)]}));
            continue;
        }
        // Anthropic rejects empty text blocks (an assistant turn that was
        // pure tool calls, or an image-only user message).
        let source = match message.get("content") {
            Some(content) if js::truthy(Some(content)) => content.clone(),
            _ => Value::String(String::new()),
        };
        let mut content: Vec<Value> = content_blocks(&source)?
            .into_iter()
            .filter(|block| block["type"] != "text" || js::truthy(block.get("text")))
            .collect();
        if role == "assistant" {
            // Thinking blocks are replayed verbatim (signatures included) or
            // the API rejects the next tool_result turn when thinking is on.
            let thinking: Vec<Value> = message
                .get("reasoning_details")
                .and_then(Value::as_array)
                .map(|details| {
                    details
                        .iter()
                        .filter(|block| {
                            matches!(
                                block.get("type").and_then(Value::as_str),
                                Some("thinking" | "redacted_thinking")
                            )
                        })
                        .cloned()
                        .collect()
                })
                .unwrap_or_default();
            content.splice(0..0, thinking);
            for call in message
                .get("tool_calls")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let function = call.get("function");
                let mut block = Map::new();
                block.insert("type".into(), "tool_use".into());
                if let Some(id) = call.get("id") {
                    block.insert("id".into(), id.clone());
                }
                if let Some(name) = js::get(function, "name") {
                    block.insert("name".into(), name.clone());
                }
                block.insert("input".into(), tool_input(js::get(function, "arguments")));
                content.push(Value::Object(block));
            }
        }
        if !content.is_empty() {
            converted.push(json!({"role": role, "content": content}));
        }
    }
    let mut out = Map::new();
    out.insert("system".into(), Value::String(system));
    out.insert("messages".into(), Value::Array(converted));
    // Arguments stream as the model writes them, not in one burst at the
    // end; they can arrive cut off, and the agent turns those into an error.
    let tools = tools
        .iter()
        .map(|tool| {
            let function = tool.get("function");
            let mut out = Map::new();
            for (to, from) in [
                ("name", "name"),
                ("description", "description"),
                ("input_schema", "parameters"),
            ] {
                if let Some(value) = js::get(function, from) {
                    out.insert(to.into(), value.clone());
                }
            }
            out.insert("eager_input_streaming".into(), Value::Bool(true));
            Value::Object(out)
        })
        .collect();
    out.insert("tools".into(), Value::Array(tools));
    Ok(out)
}

/// `anthropicBody`.
pub fn body(request: &CompletionRequest, adaptive: bool) -> Result<Value, ProviderError> {
    let mut body = Map::new();
    body.insert("model".into(), Value::String(request.model.clone()));
    for (key, value) in request_parts(&request.messages, &tools_json(request))? {
        body.insert(key, value);
    }
    body.insert("max_tokens".into(), Value::from(32768));
    body.insert("stream".into(), Value::Bool(true));
    body.insert("cache_control".into(), json!({"type": "ephemeral"}));
    if adaptive {
        body.insert(
            "thinking".into(),
            json!({"type": "adaptive", "display": "summarized", "block_binding": {"prefix_mismatch_behavior": "error"}}),
        );
    }
    if let Some(effort) = &request.effort {
        body.insert("output_config".into(), json!({"effort": effort}));
    }
    Ok(Value::Object(body))
}

/// Whether each Anthropic model has effort levels, from the catalog,
/// looked up once per model (so later requests do not depend on
/// OpenRouter); a lookup that failed is forgotten.
#[derive(Default)]
pub struct Adaptive {
    models: Mutex<HashMap<String, Lookup>>,
}

/// One model's lookup, shared by the requests that wait for it.
type Lookup = Arc<OnceCell<Result<bool, String>>>;

/// `anthropicAdaptive`: a failed lookup reads as a catalog error, not as an
/// HTTP status or network failure of the Anthropic request, so the retry
/// does not retry it.
async fn adaptive(api: &Api, model: &str) -> Result<bool, ProviderError> {
    let cell = api
        .adaptive
        .models
        .lock()
        .expect("the lookups")
        .entry(model.to_owned())
        .or_default()
        .clone();
    let result = cell
        .get_or_init(|| async {
            api.catalog
                .facts(ProviderId::Anthropic, model)
                .await
                .map(|facts| !facts.efforts.is_empty())
                .map_err(|message| {
                    format!("The OpenRouter model catalog could not describe {model}: {message}")
                })
        })
        .await
        .clone();
    if result.is_err() {
        let mut models = api.adaptive.models.lock().expect("the lookups");
        if models
            .get(model)
            .is_some_and(|held| Arc::ptr_eq(held, &cell))
        {
            models.remove(model);
        }
    }
    result.map_err(ProviderError::new)
}

/// `providerHeaders` for Anthropic.
fn headers(api_key: &str) -> Vec<(String, String)> {
    vec![
        ("x-api-key".into(), api_key.into()),
        ("anthropic-version".into(), "2023-06-01".into()),
        (
            "anthropic-beta".into(),
            "thinking-binding-controls-2026-08-01".into(),
        ),
    ]
}

/// A thinking block as it streams in.
enum Thinking {
    Signed { thinking: String, signature: String },
    Redacted { data: Option<Value> },
}

/// `anthropicCompletion`.
pub async fn complete(
    api: &Api,
    config: &ProviderConfig,
    request: &CompletionRequest,
    on_delta: &mut (dyn FnMut(Delta) + Send),
    cancel: &CancellationToken,
) -> Result<Reply, ProviderError> {
    let adaptive = adaptive(api, &request.model).await?;
    let body = body(request, adaptive)?;
    let url = format!("{}/messages", api.endpoints.anthropic);
    let mut answer = chat::post(
        api,
        &url,
        headers(&config.api_key),
        &config.api_key,
        &body,
        cancel,
    )
    .await?;
    let status = answer.status;
    let mut reply = Reply::default();
    let mut usage: Option<Map<String, Value>> = None;
    let mut calls: BTreeMap<i64, ToolCall> = BTreeMap::new();
    let mut thinking: BTreeMap<i64, Thinking> = BTreeMap::new();
    let mut complete = false;
    let mut reasoning = String::new();
    sse::read(&mut answer, &mut |payload| {
        let kind = payload
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let index = payload.get("index").and_then(Value::as_i64).unwrap_or(-1);
        if kind == "error" {
            return Err(ProviderError::new(provider_error_message(
                &config.api_key,
                status,
                &mcbench_json::stringify(&payload),
            )));
        }
        if kind == "message_start"
            && let Some(start) =
                js::get(payload.get("message"), "usage").filter(|u| js::truthy(Some(u)))
        {
            usage = start.as_object().cloned();
        }
        if kind == "message_delta"
            && let Some(more) = payload.get("usage").filter(|u| js::truthy(Some(u)))
        {
            let merged = usage.get_or_insert_with(Map::new);
            for (key, value) in more.as_object().into_iter().flatten() {
                merged.insert(key.clone(), value.clone());
            }
        }
        if kind == "message_stop" {
            complete = true;
        }
        if kind == "content_block_start" {
            let block = payload.get("content_block");
            match js::get(block, "type").and_then(Value::as_str) {
                Some("tool_use") => {
                    let call = ToolCall {
                        id: js::get(block, "id").map(js::to_string).unwrap_or_default(),
                        kind: "function".into(),
                        name: js::get(block, "name")
                            .map(js::to_string)
                            .unwrap_or_default(),
                        arguments: String::new(),
                        extra_content: None,
                    };
                    on_delta(Delta::ToolCall {
                        index: index as u32,
                        id: call.id.clone(),
                        name: call.name.clone(),
                        arguments: String::new(),
                    });
                    calls.insert(index, call);
                }
                Some("thinking") => {
                    thinking.insert(
                        index,
                        Thinking::Signed {
                            thinking: String::new(),
                            signature: String::new(),
                        },
                    );
                }
                Some("redacted_thinking") => {
                    thinking.insert(
                        index,
                        Thinking::Redacted {
                            data: js::get(block, "data").cloned(),
                        },
                    );
                }
                _ => {}
            }
            return Ok(());
        }
        if kind == "content_block_delta" {
            let delta = payload.get("delta");
            let text = |key: &str| {
                js::get(delta, key)
                    .map(js::to_string)
                    .unwrap_or_else(|| "undefined".into())
            };
            match js::get(delta, "type").and_then(Value::as_str) {
                Some("text_delta") => {
                    let piece = text("text");
                    reply.content.push_str(&piece);
                    on_delta(Delta::Content(piece));
                }
                Some("thinking_delta") => {
                    let piece = text("thinking");
                    reasoning.push_str(&piece);
                    if let Some(Thinking::Signed { thinking, .. }) = thinking.get_mut(&index) {
                        thinking.push_str(&piece);
                    }
                    on_delta(Delta::Reasoning(piece));
                }
                Some("signature_delta") => {
                    if let Some(Thinking::Signed { signature, .. }) = thinking.get_mut(&index) {
                        signature.push_str(&text("signature"));
                    }
                }
                Some("input_json_delta") => {
                    let Some(call) = calls.get_mut(&index) else {
                        return Err(ProviderError::new(
                            "Cannot read properties of undefined (reading 'function')",
                        ));
                    };
                    let piece = text("partial_json");
                    call.arguments.push_str(&piece);
                    on_delta(Delta::ToolCall {
                        index: index as u32,
                        id: call.id.clone(),
                        name: call.name.clone(),
                        arguments: piece,
                    });
                }
                _ => {}
            }
            return Ok(());
        }
        if kind == "message_delta" {
            let reason = js::get(payload.get("delta"), "stop_reason");
            if js::truthy(reason) {
                reply.finish_reason = Some(js::to_string(reason.expect("truthy")));
            }
        }
        Ok(())
    })
    .await?;
    if !reasoning.is_empty() {
        reply.reasoning = Some(reasoning);
    }
    if !complete {
        return Err(ProviderError::new(
            "Anthropic stream ended before message_stop",
        ));
    }
    if reply.finish_reason.as_deref() == Some("max_tokens") {
        return Err(ProviderError::new("Model response incomplete: max_tokens"));
    }
    let details: Vec<Value> = thinking
        .into_values()
        .map(|block| match block {
            Thinking::Signed {
                thinking,
                signature,
            } => {
                json!({"type": "thinking", "thinking": thinking, "signature": signature})
            }
            Thinking::Redacted { data } => {
                let mut out = Map::new();
                out.insert("type".into(), "redacted_thinking".into());
                if let Some(data) = data {
                    out.insert("data".into(), data);
                }
                Value::Object(out)
            }
        })
        .collect();
    if !details.is_empty() {
        reply.reasoning_details = Some(details);
    }
    reply.usage = usage.map(Value::Object);
    reply.tool_calls = calls.into_values().collect();
    Ok(reply)
}

#[cfg(test)]
mod tests {
    //! The request half of `test/llm.test.js`'s Anthropic tests.

    use super::*;

    fn messages(value: Value) -> Vec<ApiMessage> {
        value
            .as_array()
            .unwrap()
            .iter()
            .map(|m| ApiMessage(m.as_object().unwrap().clone()))
            .collect()
    }

    fn request(model: &str, messages: Vec<ApiMessage>, effort: Option<&str>) -> CompletionRequest {
        CompletionRequest {
            conversation_id: None,
            model: model.into(),
            effort: effort.map(String::from),
            messages,
            tools: Vec::new(),
            chat: false,
            transcript: None,
        }
    }

    #[test]
    fn data_image_parses_base64_image_urls() {
        assert_eq!(
            data_image("data:image/png;base64,YWJj").unwrap(),
            ("image/png".into(), "YWJj".into())
        );
        assert!(data_image("https://example.com/image.png").is_err());
    }

    #[test]
    fn anthropic_request_translates_system_images_tools_and_tool_history() {
        let out = request_parts(
            &messages(json!([
                {"role": "system", "content": "Build carefully."},
                {"role": "user", "content": [{"type": "text", "text": "Copy this"}, {"type": "image_url", "image_url": {"url": "data:image/jpeg;base64,YWJj"}}]},
                {"role": "assistant", "content": "Starting", "tool_calls": [{"id": "tool_1", "type": "function", "function": {"name": "build", "arguments": "{\"summary\":\"house\"}"}}]},
                {"role": "tool", "tool_call_id": "tool_1", "content": "{\"ok\":true}"},
            ])),
            &[json!({"type": "function", "function": {"name": "build", "description": "Build blocks", "parameters": {"type": "object"}}})],
        )
        .unwrap();
        assert_eq!(out["system"], "Build carefully.");
        assert_eq!(
            out["messages"][0]["content"][1],
            json!({"type": "image", "source": {"type": "base64", "media_type": "image/jpeg", "data": "YWJj"}})
        );
        assert_eq!(
            out["messages"][1]["content"][1],
            json!({"type": "tool_use", "id": "tool_1", "name": "build", "input": {"summary": "house"}})
        );
        assert_eq!(
            out["messages"][2],
            json!({"role": "user", "content": [{"type": "tool_result", "tool_use_id": "tool_1", "content": "{\"ok\":true}"}]})
        );
        assert_eq!(
            out["tools"],
            json!([{"name": "build", "description": "Build blocks", "input_schema": {"type": "object"}, "eager_input_streaming": true}])
        );
    }

    #[test]
    fn anthropic_request_drops_empty_text_blocks_and_replays_signed_thinking() {
        let out = request_parts(
            &messages(json!([
                {"role": "assistant", "content": "", "reasoning": "summary text", "reasoning_details": [{"type": "thinking", "thinking": "plan the roof", "signature": "sig123"}], "tool_calls": [{"id": "t1", "type": "function", "function": {"name": "build", "arguments": "{}"}}]},
                {"role": "tool", "tool_call_id": "t1", "content": "{\"ok\":true}"},
            ])),
            &[],
        )
        .unwrap();
        assert_eq!(
            out["messages"][0]["content"],
            json!([{"type": "thinking", "thinking": "plan the roof", "signature": "sig123"}, {"type": "tool_use", "id": "t1", "name": "build", "input": {}}])
        );
    }

    #[test]
    fn anthropic_requests_summarize_adaptive_thinking_whenever_the_model_has_effort_levels_never_a_token_budget_and_cache_the_conversation()
     {
        let list = messages(
            json!([{"role": "system", "content": "S"}, {"role": "user", "content": "hi"}]),
        );
        let thinking = json!({"type": "adaptive", "display": "summarized", "block_binding": {"prefix_mismatch_behavior": "error"}});
        let with = body(
            &request("claude-opus-5-5", list.clone(), Some("high")),
            true,
        )
        .unwrap();
        assert_eq!(with["thinking"], thinking);
        assert_eq!(with["cache_control"], json!({"type": "ephemeral"}));
        assert_eq!(with["output_config"], json!({"effort": "high"}));
        assert_eq!(with["max_tokens"], 32768);
        // No chosen effort: the API's default effort, still with a summary.
        let unset = body(&request("claude-opus-5-5", list.clone(), None), true).unwrap();
        assert_eq!(unset["thinking"], thinking);
        assert!(unset.get("output_config").is_none());
        // A model without effort levels keeps its API defaults.
        let legacy = body(&request("claude-opus-4-5-20251101", list, None), false).unwrap();
        assert!(legacy.get("thinking").is_none());
        assert!(legacy.get("output_config").is_none());
    }
}
