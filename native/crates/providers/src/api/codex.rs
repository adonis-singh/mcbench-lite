//! The ChatGPT sign-in on the Responses API (`server/codex.js`): the request
//! as the Codex CLI sends it, and its stream.
//!
//! Images go as the Codex CLI sends them: with detail "high", and a tool
//! result's screenshots inside that result. In three chat turns that sent a
//! new screenshot without detail, the request that added it read 0 tokens
//! from the cache; with detail "high" in the function output it read 6,912
//! of 8,349. A user message that ends with an image (an upload) gets
//! `[End of images]` after it, in every request, so the message is the same
//! whether or not it is the last. A chat's requests carry its id where the
//! Codex CLI puts its session id: `prompt_cache_key`, and the session-id and
//! thread-id headers [`super::oauth`] adds. A one-shot has no chat and no
//! history to share.

use std::collections::HashMap;

use serde_json::{Map, Value, json};
use tokio_util::sync::CancellationToken;

use super::{Api, Reply, js, provider_error_message, sse, tools_json};
use crate::{ApiMessage, CompletionRequest, Delta, ProviderConfig, ProviderError, ToolCall};

/// `contentParts`: a message's parts as Responses input parts; a closed
/// message that ends with an image gets `[End of images]` after it.
fn content_parts(parts: &[Value], closed: bool) -> Result<Vec<Value>, ProviderError> {
    let mut content = parts
        .iter()
        .map(|part| match part.get("type").and_then(Value::as_str) {
            Some("text") => {
                let mut out = Map::new();
                out.insert("type".into(), "input_text".into());
                if let Some(text) = part.get("text") {
                    out.insert("text".into(), text.clone());
                }
                Ok(Value::Object(out))
            }
            Some("image_url") => {
                let mut out = Map::new();
                out.insert("type".into(), "input_image".into());
                if let Some(url) = js::get(part.get("image_url"), "url") {
                    out.insert("image_url".into(), url.clone());
                }
                out.insert("detail".into(), "high".into());
                Ok(Value::Object(out))
            }
            _ => Err(ProviderError::new(format!(
                "Unsupported Codex content type: {}",
                part.get("type")
                    .map(js::to_string)
                    .unwrap_or_else(|| "undefined".into())
            ))),
        })
        .collect::<Result<Vec<_>, _>>()?;
    if closed
        && parts
            .last()
            .and_then(|p| p.get("type"))
            .and_then(Value::as_str)
            == Some("image_url")
    {
        content.push(json!({"type": "input_text", "text": "[End of images]"}));
    }
    Ok(content)
}

/// `codexRequest`: the Responses body.
pub fn request_body(request: &CompletionRequest) -> Result<Value, ProviderError> {
    let mut input: Vec<Value> = Vec::new();
    for ApiMessage(message) in &request.messages {
        let role = message
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if role == "system" {
            continue;
        }
        if role == "tool" {
            let mut out = Map::new();
            out.insert("type".into(), "function_call_output".into());
            if let Some(id) = message.get("tool_call_id") {
                out.insert("call_id".into(), id.clone());
            }
            if let Some(content) = message.get("content") {
                out.insert("output".into(), content.clone());
            }
            input.push(Value::Object(out));
            continue;
        }
        // Screenshots a tool result showed go inside that result, after its
        // text.
        if js::truthy(message.get("seeBuild")) {
            let Some(result) = input.last_mut().filter(|item| {
                item.get("type").and_then(Value::as_str) == Some("function_call_output")
            }) else {
                return Err(ProviderError::new("Screenshots must follow a tool result"));
            };
            let mut output = match result.get("output") {
                Some(Value::String(text)) => vec![json!({"type": "input_text", "text": text})],
                Some(Value::Array(items)) => items.clone(),
                _ => Vec::new(),
            };
            let parts = message
                .get("content")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            output.extend(content_parts(&parts, false)?);
            result["output"] = Value::Array(output);
            continue;
        }
        if role == "assistant" {
            for item in message
                .get("reasoning_details")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                if item.get("type").and_then(Value::as_str) == Some("reasoning") {
                    input.push(item.clone());
                }
            }
        }
        let content = match message.get("content") {
            Some(Value::String(text)) => Value::String(text.clone()),
            Some(Value::Array(parts)) => Value::Array(content_parts(parts, true)?),
            other => other.cloned().unwrap_or(Value::Null),
        };
        let has = match &content {
            Value::String(text) => !text.is_empty(),
            Value::Array(items) => !items.is_empty(),
            _ => false,
        };
        if has {
            input.push(json!({"role": role, "content": content}));
        }
        for call in message
            .get("tool_calls")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let function = call.get("function");
            let mut out = Map::new();
            out.insert("type".into(), "function_call".into());
            if let Some(id) = call.get("id") {
                out.insert("call_id".into(), id.clone());
            }
            if let Some(name) = js::get(function, "name") {
                out.insert("name".into(), name.clone());
            }
            if let Some(arguments) = js::get(function, "arguments") {
                out.insert("arguments".into(), arguments.clone());
            }
            input.push(Value::Object(out));
        }
    }
    let instructions = request
        .messages
        .iter()
        .filter(|m| m.0.get("role").and_then(Value::as_str) == Some("system"))
        .map(|m| m.0.get("content").map(js::to_string).unwrap_or_default())
        .collect::<Vec<_>>()
        .join("\n\n");
    let tools: Vec<Value> = tools_json(request)
        .into_iter()
        .map(|tool| {
            let mut out = Map::new();
            out.insert("type".into(), "function".into());
            for (key, value) in tool
                .get("function")
                .and_then(Value::as_object)
                .into_iter()
                .flatten()
            {
                out.insert(key.clone(), value.clone());
            }
            out.insert("strict".into(), Value::Bool(false));
            Value::Object(out)
        })
        .collect();
    let mut body = Map::new();
    body.insert("model".into(), Value::String(request.model.clone()));
    body.insert("instructions".into(), Value::String(instructions));
    body.insert("input".into(), Value::Array(input));
    body.insert("tools".into(), Value::Array(tools));
    body.insert("stream".into(), Value::Bool(true));
    body.insert("store".into(), Value::Bool(false));
    body.insert("include".into(), json!(["reasoning.encrypted_content"]));
    if let Some(id) = request.conversation_id.as_ref().filter(|id| !id.is_empty()) {
        body.insert("prompt_cache_key".into(), Value::String(id.clone()));
    }
    if let Some(effort) = &request.effort {
        body.insert(
            "reasoning".into(),
            json!({"effort": effort, "summary": "auto"}),
        );
    }
    Ok(Value::Object(body))
}

/// The most formatting whitespace in a row outside JSON strings a tool's
/// arguments may stream (`toolArgumentProgress`).
const WHITESPACE_LIMIT: u32 = 16384;

/// `toolArgumentProgress`: repeated formatting whitespace outside JSON
/// strings carries no tool content. It recovers a degenerate stream without
/// bounding valid argument size, code inside strings, reasoning time, model
/// turns, or tool calls.
#[derive(Default)]
pub struct ToolArgumentProgress {
    in_string: bool,
    escaped: bool,
    whitespace: u32,
}

impl ToolArgumentProgress {
    /// Takes the next piece of the arguments.
    pub fn push(&mut self, chunk: &str) -> Result<(), ProviderError> {
        for c in chunk.chars() {
            if self.in_string {
                self.whitespace = 0;
                if self.escaped {
                    self.escaped = false;
                } else if c == '\\' {
                    self.escaped = true;
                } else if c == '"' {
                    self.in_string = false;
                }
            } else if c == '"' {
                self.in_string = true;
                self.whitespace = 0;
            } else if matches!(c, ' ' | '\n' | '\r' | '\t') {
                self.whitespace += 1;
                if self.whitespace > WHITESPACE_LIMIT {
                    return Err(ProviderError::new(
                        "Codex tool argument stream stalled: repeated JSON whitespace without progress",
                    ));
                }
            } else {
                self.whitespace = 0;
            }
        }
        Ok(())
    }
}

/// A call as it streams in: its index, id and name.
struct Call {
    index: u32,
    id: String,
    name: String,
}

/// `codexCompletion`.
pub async fn complete(
    api: &Api,
    config: &ProviderConfig,
    request: &CompletionRequest,
    on_delta: &mut (dyn FnMut(Delta) + Send),
    cancel: &CancellationToken,
) -> Result<Reply, ProviderError> {
    let body = request_body(request)?;
    let home = &config.cli.as_ref().expect("the Codex CLI's runtime").home;
    let mut answer = super::oauth::send(api, &home.join("auth.json"), &body, cancel).await?;
    if !answer.ok() {
        let status = answer.status;
        let text = answer
            .text()
            .await
            .map_err(|failure| ProviderError::new(failure.message()))?;
        return Err(ProviderError::new(provider_error_message(
            "", status, &text,
        )));
    }
    let mut calls: HashMap<String, Call> = HashMap::new();
    let mut progress: HashMap<String, ToolArgumentProgress> = HashMap::new();
    let mut output: Vec<Value> = Vec::new();
    let mut completed: Option<Value> = None;
    let mut reasoning_part: Option<String> = None;
    sse::read(&mut answer, &mut |event| {
        let key = |event: &Value| {
            event
                .get("output_index")
                .map(mcbench_json::stringify)
                .unwrap_or_default()
        };
        let text = |value: Option<&Value>| {
            value
                .map(js::to_string)
                .unwrap_or_else(|| "undefined".into())
        };
        match event
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default()
        {
            "response.output_text.delta" => on_delta(Delta::Content(text(event.get("delta")))),
            "response.reasoning_summary_text.delta" => {
                let part = format!(
                    "{}:{}",
                    text(event.get("item_id")),
                    text(event.get("summary_index"))
                );
                let separator = if reasoning_part.as_ref().is_some_and(|last| *last != part) {
                    "\n\n"
                } else {
                    ""
                };
                on_delta(Delta::Reasoning(format!(
                    "{separator}{}",
                    text(event.get("delta"))
                )));
                reasoning_part = Some(part);
            }
            "response.output_item.added" => {
                let item = event.get("item");
                if js::get(item, "type").and_then(Value::as_str) == Some("function_call") {
                    let call = Call {
                        index: calls.len() as u32,
                        id: js::get(item, "call_id")
                            .map(js::to_string)
                            .unwrap_or_default(),
                        name: js::get(item, "name").map(js::to_string).unwrap_or_default(),
                    };
                    let arguments = js::get(item, "arguments")
                        .map(js::to_string)
                        .unwrap_or_default();
                    let mut guard = ToolArgumentProgress::default();
                    guard.push(&arguments)?;
                    progress.insert(key(&event), guard);
                    on_delta(Delta::ToolCall {
                        index: call.index,
                        id: call.id.clone(),
                        name: call.name.clone(),
                        arguments,
                    });
                    calls.insert(key(&event), call);
                }
            }
            "response.function_call_arguments.delta" => {
                let piece = event.get("delta").map(js::to_string).unwrap_or_default();
                if let Some(guard) = progress.get_mut(&key(&event)) {
                    guard.push(&piece)?;
                }
                let call = calls.get(&key(&event)).ok_or_else(|| {
                    ProviderError::new("A tool call's arguments came before the call")
                })?;
                on_delta(Delta::ToolCall {
                    index: call.index,
                    id: call.id.clone(),
                    name: call.name.clone(),
                    arguments: piece,
                });
            }
            "response.output_item.done" => {
                output.push(event.get("item").cloned().unwrap_or(Value::Null))
            }
            "response.completed" => {
                completed = Some(event.get("response").cloned().unwrap_or(Value::Null))
            }
            "error" | "response.failed" => {
                let message = [
                    js::get(js::get(event.get("response"), "error"), "message"),
                    js::get(event.get("error"), "message"),
                    event.get("message"),
                ]
                .into_iter()
                .find(|v| js::truthy(*v))
                .flatten()
                .map(js::to_string)
                .unwrap_or_else(|| "Codex response failed".into());
                return Err(ProviderError::new(message));
            }
            "response.incomplete" => {
                let reason = js::get(
                    js::get(event.get("response"), "incomplete_details"),
                    "reason",
                );
                return Err(ProviderError::new(format!(
                    "Codex response incomplete: {}",
                    text(reason)
                )));
            }
            _ => {}
        }
        Ok(())
    })
    .await?;
    let Some(completed) = completed else {
        return Err(ProviderError::new(
            "Codex stream ended before response.completed",
        ));
    };
    let mut reply = Reply {
        finish_reason: Some("stop".into()),
        usage: completed
            .get("usage")
            .filter(|u| js::truthy(Some(u)))
            .cloned(),
        ..Reply::default()
    };
    let mut details: Option<Vec<Value>> = None;
    // Codex sends complete items here and leaves response.completed.output
    // empty.
    for item in output {
        match item.get("type").and_then(Value::as_str) {
            Some("message") => {
                for part in item
                    .get("content")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    match part.get("type").and_then(Value::as_str) {
                        Some("output_text") => reply.content.push_str(&text_of(part.get("text"))),
                        Some("refusal") => reply.content.push_str(&text_of(part.get("refusal"))),
                        _ => {}
                    }
                }
            }
            Some("function_call") => {
                reply.tool_calls.push(ToolCall {
                    id: item.get("call_id").map(js::to_string).unwrap_or_default(),
                    kind: "function".into(),
                    name: item.get("name").map(js::to_string).unwrap_or_default(),
                    arguments: item.get("arguments").map(js::to_string).unwrap_or_default(),
                    extra_content: None,
                });
                reply.finish_reason = Some("tool_calls".into());
            }
            Some("reasoning") => details.get_or_insert_with(Vec::new).push(item),
            _ => {}
        }
    }
    if let Some(details) = &details {
        let summary: Vec<String> = details
            .iter()
            .flat_map(|item| {
                item.get("summary")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .map(|part| text_of(part.get("text")))
                    .collect::<Vec<_>>()
            })
            .collect();
        reply.reasoning = Some(summary.join("\n\n"));
    }
    reply.reasoning_details = details;
    Ok(reply)
}

/// `message.content += value`: undefined reads `undefined`.
fn text_of(value: Option<&Value>) -> String {
    value
        .map(js::to_string)
        .unwrap_or_else(|| "undefined".into())
}

#[cfg(test)]
mod tests {
    //! The request half of `test/codex.test.js`.

    use super::*;
    use crate::ToolSchema;

    fn messages(value: Value) -> Vec<ApiMessage> {
        value
            .as_array()
            .unwrap()
            .iter()
            .map(|m| ApiMessage(m.as_object().unwrap().clone()))
            .collect()
    }

    fn request(
        messages: Vec<ApiMessage>,
        effort: Option<&str>,
        conversation: Option<&str>,
        tools: Vec<ToolSchema>,
    ) -> CompletionRequest {
        CompletionRequest {
            conversation_id: conversation.map(String::from),
            model: "gpt-6-astra".into(),
            effort: effort.map(String::from),
            messages,
            tools,
            chat: false,
            transcript: None,
        }
    }

    #[test]
    fn codex_replays_tool_results_images_and_encrypted_reasoning_without_remote_state() {
        let reasoning = json!({"type": "reasoning", "id": "rs_1", "encrypted_content": "opaque", "summary": []});
        let body = request_body(&request(
            messages(json!([
                {"role": "system", "content": "Build carefully"},
                {"role": "user", "content": "Build a platform"},
                {"role": "assistant", "content": "", "reasoning_details": [reasoning], "tool_calls": [{"id": "call_1", "function": {"name": "build", "arguments": "{\"program\":\"code\"}"}}]},
                {"role": "tool", "tool_call_id": "call_1", "content": "{\"ok\":true}"},
                {"role": "user", "content": [{"type": "text", "text": "Inspect this"}, {"type": "image_url", "image_url": {"url": "data:image/png;base64,YWJj"}}]},
            ])),
            Some("high"),
            None,
            vec![ToolSchema(r#"{"type":"function","function":{"name":"build","parameters":{"type":"object"}}}"#.into())],
        ))
        .unwrap();
        assert_eq!(
            body["input"],
            json!([
                {"role": "user", "content": "Build a platform"}, reasoning,
                {"type": "function_call", "call_id": "call_1", "name": "build", "arguments": "{\"program\":\"code\"}"},
                {"type": "function_call_output", "call_id": "call_1", "output": "{\"ok\":true}"},
                {"role": "user", "content": [
                    {"type": "input_text", "text": "Inspect this"},
                    {"type": "input_image", "image_url": "data:image/png;base64,YWJj", "detail": "high"},
                    {"type": "input_text", "text": "[End of images]"},
                ]},
            ])
        );
        assert_eq!(body["instructions"], "Build carefully");
        assert_eq!(body["store"], false);
        assert_eq!(
            body["reasoning"],
            json!({"effort": "high", "summary": "auto"})
        );
        assert_eq!(body["tools"][0]["name"], "build");
    }

    #[test]
    fn a_tool_results_screenshots_go_inside_its_function_output_as_the_codex_cli_sends_them_and_a_stray_one_fails()
     {
        let body = request_body(&request(
            messages(json!([
                {"role": "user", "content": "Build and look"},
                {"role": "assistant", "content": "", "tool_calls": [{"id": "call_1", "function": {"name": "see_build", "arguments": "{}"}}]},
                {"role": "tool", "tool_call_id": "call_1", "content": "{\"ok\":true}"},
                {"role": "user", "seeBuild": true, "content": [{"type": "text", "text": "[see_build] Screenshots of the current build:"}, {"type": "image_url", "image_url": {"url": "data:image/jpeg;base64,YWJj"}}]},
            ])),
            None,
            None,
            Vec::new(),
        ))
        .unwrap();
        assert_eq!(
            body["input"].as_array().unwrap().last().unwrap(),
            &json!({"type": "function_call_output", "call_id": "call_1", "output": [
                {"type": "input_text", "text": "{\"ok\":true}"},
                {"type": "input_text", "text": "[see_build] Screenshots of the current build:"},
                {"type": "input_image", "image_url": "data:image/jpeg;base64,YWJj", "detail": "high"},
            ]})
        );
        let stray = request_body(&request(
            messages(
                json!([{"role": "user", "seeBuild": true, "content": [{"type": "text", "text": "x"}]}]),
            ),
            None,
            None,
            Vec::new(),
        ));
        assert!(
            stray
                .unwrap_err()
                .message
                .contains("must follow a tool result")
        );
    }

    #[test]
    fn a_chats_codex_requests_carry_its_id_where_the_codex_cli_puts_its_session_id_a_one_shot_carries_none()
     {
        let id = "6f1c2a40-1111-4c3e-9d2b-0a5c8e7f6b21";
        let list = messages(
            json!([{"role": "system", "content": "Name this chat"}, {"role": "user", "content": "Build a platform"}]),
        );
        assert_eq!(
            request_body(&request(list.clone(), None, Some(id), Vec::new())).unwrap()["prompt_cache_key"],
            id
        );
        assert!(
            request_body(&request(list, None, None, Vec::new()))
                .unwrap()
                .get("prompt_cache_key")
                .is_none()
        );
    }

    #[test]
    fn tool_progress_guard_ignores_arbitrary_whitespace_in_code_strings_and_resets_on_meaningful_json()
     {
        let mut guard = ToolArgumentProgress::default();
        let program = format!("{}\\\"\n\t{}", " ".repeat(100_000), " ".repeat(100_000));
        guard
            .push(&mcbench_json::stringify(&json!({"program": program})))
            .unwrap();
        for _ in 0..100 {
            guard.push(&format!("{},0", " ".repeat(16000))).unwrap();
        }
        let mut stalled = ToolArgumentProgress::default();
        stalled.push("{\"probes\":[").unwrap();
        stalled.push(&" ".repeat(16000)).unwrap();
        assert!(
            stalled
                .push(&"\n".repeat(385))
                .unwrap_err()
                .message
                .contains("tool argument stream stalled")
        );
        let mut split = ToolArgumentProgress::default();
        split.push("{\"program\":\"\\").unwrap();
        split.push(&format!("\"{}", " ".repeat(100_000))).unwrap();
        split.push("\"}").unwrap();
    }
}
