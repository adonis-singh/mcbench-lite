//! Chat Completions (`llm.js` `openAiCompletion`, `remoteMessages`): the
//! protocol of OpenRouter, OpenAI, Groq, Google (Gemini's OpenAI
//! compatibility) and QwenCloud.

use serde_json::{Map, Value};
use tokio_util::sync::CancellationToken;

use super::http::{self, Deadline, Request};
use super::{Api, Reply, js, provider_error_message, qwencloud, sse};
use crate::{
    ApiMessage, CompletionRequest, Delta, ProviderConfig, ProviderError, ProviderId, ToolCall,
};

/// The messages as a Chat Completions API gets them (`remoteMessages`):
/// `seeBuild` (which marks screenshots for the Responses API) is removed
/// from every message; an assistant message loses `reasoning`,
/// `reasoning_content`, `reasoning_details` and `finish_reason`, gets
/// `reasoning_content` back last when `preserve_thinking` (QwenCloud) and it
/// had reasoning, and keeps its calls' `extra_content` only with
/// `signatures` (Gemini's thought signatures, which must go back to it).
/// Every other field goes as it is, in its order.
pub fn remote_messages(
    messages: &[ApiMessage],
    preserve_thinking: bool,
    signatures: bool,
) -> Vec<Value> {
    messages
        .iter()
        .map(|ApiMessage(message)| {
            let mut entry: Map<String, Value> = message
                .iter()
                .filter(|(key, _)| key.as_str() != "seeBuild")
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            if message.get("role").and_then(Value::as_str) != Some("assistant") {
                return Value::Object(entry);
            }
            let reasoning_content = entry.shift_remove("reasoning_content");
            let reasoning = entry.shift_remove("reasoning");
            entry.shift_remove("reasoning_details");
            entry.shift_remove("finish_reason");
            if preserve_thinking {
                let kept = if js::truthy(reasoning_content.as_ref()) {
                    reasoning_content
                } else {
                    reasoning
                };
                if js::truthy(kept.as_ref()) {
                    entry.insert("reasoning_content".into(), kept.expect("truthy"));
                }
            }
            if !signatures && js::truthy(entry.get("tool_calls")) {
                let calls = entry["tool_calls"]
                    .as_array()
                    .expect("tool_calls is an array")
                    .iter()
                    .map(|call| {
                        let mut call = call.as_object().expect("a tool call is an object").clone();
                        call.shift_remove("extra_content");
                        Value::Object(call)
                    })
                    .collect();
                entry.insert("tool_calls".into(), Value::Array(calls));
            }
            Value::Object(entry)
        })
        .collect()
}

/// The request body, in `openAiCompletion`'s key order.
pub fn body(id: ProviderId, request: &CompletionRequest) -> Result<Value, ProviderError> {
    let mut body = Map::new();
    body.insert("model".into(), Value::String(request.model.clone()));
    body.insert(
        "messages".into(),
        Value::Array(remote_messages(
            &request.messages,
            id == ProviderId::QwenCloud,
            id == ProviderId::Google,
        )),
    );
    body.insert("stream".into(), Value::Bool(true));
    let mut options = Map::new();
    options.insert("include_usage".into(), Value::Bool(true));
    body.insert("stream_options".into(), Value::Object(options));
    if !request.tools.is_empty() {
        body.insert("tools".into(), Value::Array(super::tools_json(request)));
    }
    if id == ProviderId::QwenCloud {
        for (key, value) in qwencloud::request_options(&request.model, request.effort.as_deref())? {
            body.insert(key, value);
        }
    } else if let Some(effort) = &request.effort {
        body.insert("reasoning_effort".into(), Value::String(effort.clone()));
    }
    Ok(Value::Object(body))
}

/// `request`: a POST of `body` to the provider's `endpoint` with
/// `content-type` and the provider's headers, within four hours.
pub async fn post(
    api: &Api,
    url: &str,
    headers: Vec<(String, String)>,
    api_key: &str,
    body: &Value,
    cancel: &CancellationToken,
) -> Result<http::Answer, ProviderError> {
    let mut all = vec![("content-type".to_owned(), "application/json".to_owned())];
    all.extend(headers);
    let deadline = Deadline::new(cancel, http::COMPLETION_TIMEOUT);
    let answer = http::send(
        &api.http,
        Request {
            method: reqwest::Method::POST,
            url,
            headers: all,
            body: Some(mcbench_json::stringify(body)),
        },
        &deadline,
    )
    .await
    .map_err(|failure| ProviderError::new(failure.message()))?;
    if !answer.ok() {
        let status = answer.status;
        let text = answer
            .text()
            .await
            .map_err(|failure| ProviderError::new(failure.message()))?;
        return Err(ProviderError::new(provider_error_message(
            api_key, status, &text,
        )));
    }
    Ok(answer)
}

/// `providerHeaders` of a key provider on Chat Completions.
pub fn bearer(api_key: &str) -> Vec<(String, String)> {
    vec![("authorization".into(), format!("Bearer {api_key}"))]
}

/// A call as it streams in.
struct Streaming {
    id: String,
    name: String,
    arguments: String,
    extra_content: Option<Value>,
}

/// `openAiCompletion`.
pub async fn complete(
    api: &Api,
    config: &ProviderConfig,
    request: &CompletionRequest,
    on_delta: &mut (dyn FnMut(Delta) + Send),
    cancel: &CancellationToken,
) -> Result<Reply, ProviderError> {
    let body = body(config.id, request)?;
    let url = format!("{}/chat/completions", api.endpoints.base(config.id));
    let mut answer = post(
        api,
        &url,
        bearer(&config.api_key),
        &config.api_key,
        &body,
        cancel,
    )
    .await?;
    let status = answer.status;
    let mut reply = Reply::default();
    let mut calls: Vec<Option<Streaming>> = Vec::new();
    let mut reasoning = String::new();
    let mut finish_reason = String::new();
    sse::read(&mut answer, &mut |payload| {
        if js::truthy(payload.get("error")) {
            return Err(ProviderError::new(provider_error_message(
                &config.api_key,
                status,
                &mcbench_json::stringify(&payload),
            )));
        }
        if js::truthy(payload.get("usage")) {
            reply.usage = Some(payload["usage"].clone());
        }
        let Some(choice) = js::get(payload.get("choices"), "0")
            .or_else(|| {
                payload
                    .get("choices")
                    .and_then(Value::as_array)
                    .and_then(|a| a.first())
            })
            .filter(|c| !c.is_null())
        else {
            return Ok(());
        };
        if js::truthy(choice.get("finish_reason")) {
            finish_reason = js::to_string(&choice["finish_reason"]);
        }
        let Some(delta) = choice.get("delta").filter(|d| !d.is_null()) else {
            return Ok(());
        };
        if js::truthy(delta.get("content")) {
            let text = js::to_string(&delta["content"]);
            reply.content.push_str(&text);
            on_delta(Delta::Content(text));
        }
        let piece = [delta.get("reasoning"), delta.get("reasoning_content")]
            .into_iter()
            .find(|v| js::truthy(*v))
            .flatten();
        if let Some(piece) = piece {
            let text = js::to_string(piece);
            reasoning.push_str(&text);
            on_delta(Delta::Reasoning(text));
        }
        let parts = delta
            .get("tool_calls")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        for part in &parts {
            let function = part.get("function");
            let name = js::get(function, "name").filter(|v| js::truthy(Some(v)));
            let arguments = js::get(function, "arguments").filter(|v| js::truthy(Some(v)));
            // Gemini sends each call whole, without an index: a part that
            // names a function begins a call, and one that does not goes on
            // with the last.
            let index = match part.get("index").filter(|v| !v.is_null()) {
                Some(index) => {
                    index.as_u64().expect("a tool call index is a whole number") as usize
                }
                None if name.is_some() => calls.len(),
                None => calls.len().saturating_sub(1),
            };
            if calls.len() <= index {
                calls.resize_with(index + 1, || None);
            }
            let call = calls[index].get_or_insert_with(|| Streaming {
                id: String::new(),
                name: String::new(),
                arguments: String::new(),
                extra_content: None,
            });
            if js::truthy(part.get("id")) {
                call.id = js::to_string(&part["id"]);
            }
            if js::truthy(part.get("extra_content")) {
                call.extra_content = Some(part["extra_content"].clone());
            }
            if let Some(name) = name {
                call.name.push_str(&js::to_string(name));
            }
            if let Some(arguments) = arguments {
                call.arguments.push_str(&js::to_string(arguments));
            }
            if name.is_some() || arguments.is_some() {
                on_delta(Delta::ToolCall {
                    index: index as u32,
                    id: call.id.clone(),
                    name: call.name.clone(),
                    arguments: arguments.map(js::to_string).unwrap_or_default(),
                });
            }
        }
        Ok(())
    })
    .await?;
    if !reasoning.is_empty() {
        reply.reasoning = Some(reasoning);
    }
    if finish_reason.is_empty() {
        return Err(ProviderError::new(
            "Model stream ended without a finish reason",
        ));
    }
    if finish_reason == "length" || finish_reason == "content_filter" {
        return Err(ProviderError::new(format!(
            "Model response incomplete: {finish_reason}"
        )));
    }
    reply.finish_reason = Some(finish_reason);
    // A call index the provider skipped would leave a hole today, which
    // JSON.stringify writes as null; no provider does it, and it fails here.
    reply.tool_calls = calls
        .into_iter()
        .map(|call| {
            let call =
                call.ok_or_else(|| ProviderError::new("A tool call's index skipped a call"))?;
            Ok(ToolCall {
                id: call.id,
                kind: "function".into(),
                name: call.name,
                arguments: call.arguments,
                extra_content: call.extra_content,
            })
        })
        .collect::<Result<_, ProviderError>>()?;
    Ok(reply)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn messages(value: Value) -> Vec<ApiMessage> {
        value
            .as_array()
            .unwrap()
            .iter()
            .map(|m| ApiMessage(m.as_object().unwrap().clone()))
            .collect()
    }

    #[test]
    fn remote_messages_strips_provider_private_response_fields() {
        let out = remote_messages(
            &messages(json!([{
                "role": "assistant", "content": "", "reasoning": "visible summary",
                "reasoning_details": [{"type": "reasoning.encrypted", "data": "opaque"}],
                "finish_reason": "tool_calls",
                "tool_calls": [{"id": "call_123", "function": {"name": "build", "arguments": "{}"}}],
            }])),
            false,
            false,
        );
        assert_eq!(
            Value::Array(out),
            json!([{"role": "assistant", "content": "", "tool_calls": [{"id": "call_123", "function": {"name": "build", "arguments": "{}"}}]}])
        );
    }

    #[test]
    fn a_gemini_thought_signature_stays_with_google_other_providers_get_the_bare_call() {
        let call = json!({"id": "call-one", "type": "function", "function": {"name": "build", "arguments": "{}"}});
        let mut signed = call.clone();
        signed["extra_content"] = json!({"google": {"thought_signature": "sig"}});
        let list =
            messages(json!([{"role": "assistant", "content": "", "tool_calls": [signed.clone()]}]));
        assert_eq!(
            remote_messages(&list, false, false)[0]["tool_calls"],
            json!([call])
        );
        assert_eq!(
            remote_messages(&list, false, true)[0]["tool_calls"],
            json!([signed])
        );
    }

    #[test]
    fn see_build_is_removed_for_chat_completions() {
        let list = messages(json!([{"role": "user", "seeBuild": true, "content": "x"}]));
        assert_eq!(
            Value::Array(remote_messages(&list, false, false)),
            json!([{"role": "user", "content": "x"}])
        );
    }
}
