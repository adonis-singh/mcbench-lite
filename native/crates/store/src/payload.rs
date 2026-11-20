//! A stored message: the typed, read-only [`Message`] decoded from the row's
//! JSON text, the text itself (the source of truth, never rewritten from a
//! typed value), and [`NewMessage`], one constructor per place today's
//! server writes a message, each in that place's key order.
//!
//! The types are those of `scratchpad/payload/INVENTORY.md` section 4 (they
//! parse and re-serialise all 12,318 stored payloads of the fixture and its
//! backups to their exact bytes with unknown fields refused), as agreed with
//! the interface's owner (`transcript-seam.md`).

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

use crate::json::{self, Json, Object};

fn is_false(value: &bool) -> bool {
    !*value
}

/// A field that is absent, `null`, or a value: the outer `None` is absent,
/// `Some(None)` is `null`.
mod present {
    use super::*;
    pub fn serialize<S: Serializer, T: Serialize>(
        value: &Option<Option<T>>,
        s: S,
    ) -> Result<S::Ok, S::Error> {
        match value {
            Some(inner) => inner.serialize(s),
            None => s.serialize_none(),
        }
    }
    pub fn deserialize<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
        d: D,
    ) -> Result<Option<Option<T>>, D::Error> {
        Option::<T>::deserialize(d).map(Some)
    }
}

/// A stored message, by `role`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "role", rename_all = "lowercase")]
// Most stored messages are replies (assistant) and tool results; boxing the
// largest variant would put almost every message behind a second pointer.
#[allow(clippy::large_enum_variant)]
pub enum Message {
    /// A prompt the user sent, or the screenshots after a tool result.
    User(User),
    /// A model's reply.
    Assistant(Assistant),
    /// A tool call's result.
    Tool(Tool),
    /// A failed turn's error.
    Error(ErrorMessage),
}

/// `role: "user"`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct User {
    /// `seeBuild`: the screenshots after a `see_build` or `view_image`
    /// result, not a prompt (only ever written `true`).
    #[serde(rename = "seeBuild", default, skip_serializing_if = "is_false")]
    pub see_build: bool,
    /// `content`.
    pub content: Content,
    /// `round`: the bench round this prompt answers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub round: Option<String>,
    /// `prompts`: the library prompts it was sent from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompts: Option<Vec<PromptLink>>,
    /// `createdAt`.
    #[serde(rename = "createdAt")]
    pub created_at: String,
}

/// A user message's `content`: text, or parts.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Content {
    /// A string.
    Text(String),
    /// Parts.
    Parts(Vec<Part>),
}

/// A part of a user message's content.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Part {
    /// `{type: "text", text}`.
    Text {
        /// `text`.
        text: String,
    },
    /// `{type: "image_url", image_url: {url}}`: a stored image.
    ImageUrl {
        /// `image_url`.
        image_url: ImageUrl,
    },
    /// `{type: "attachment", attachment}`.
    Attachment {
        /// `attachment`.
        attachment: Attachment,
    },
    /// `{type: "selection", selection}`.
    Selection {
        /// `selection`.
        selection: Selection,
    },
}

/// `image_url`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageUrl {
    /// `/api/images/<chat>/<name>` ([`crate::Store::file_path`] gives its file).
    pub url: String,
}

/// A stored attachment (`saveAttachments`' record).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Attachment {
    /// `name`: the file's name as the user gave it.
    pub name: String,
    /// `type`: its MIME type.
    #[serde(rename = "type")]
    pub mime: String,
    /// `size` in bytes.
    pub size: u64,
    /// `url`: `/api/attachments/<chat>/<stored name>`.
    pub url: String,
    /// `path`: where the sandbox sees it, `/work/attachments/<stored name>`.
    pub path: String,
}

/// A selection the viewer made (`server/selection.js`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    /// `letter`.
    pub letter: String,
    /// `label`.
    pub label: String,
    /// `min`.
    pub min: [i64; 3],
    /// `max`.
    pub max: [i64; 3],
    /// `blocks`.
    pub blocks: u64,
    /// `block`: its main block, or `null` for empty space.
    pub block: Option<String>,
}

/// A library prompt a message or round was sent from.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PromptLink {
    /// The prompt's id.
    pub id: String,
    /// Its title when it was sent.
    pub title: String,
}

/// `role: "assistant"`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Assistant {
    /// `content`.
    pub content: String,
    /// `stopped`: the user stopped the turn.
    #[serde(default, skip_serializing_if = "is_false")]
    pub stopped: bool,
    /// `reply`: the text that had streamed when it was stopped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply: Option<String>,
    /// `reasoning`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,
    /// `reasoning_details`: vendor blocks replayed verbatim.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_details: Option<Vec<Value>>,
    /// `finish_reason`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
    /// `telemetry`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub telemetry: Option<Telemetry>,
    /// `notices`: a harness's notices.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notices: Option<Vec<Notice>>,
    /// `plan`: a harness's task list.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<Vec<PlanItem>>,
    /// `rateLimit`: Claude Code's `rate_limit_info`, as the CLI sent it.
    #[serde(rename = "rateLimit", default, skip_serializing_if = "Option::is_none")]
    pub rate_limit: Option<Value>,
    /// `tool_calls`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    /// `createdAt`.
    #[serde(rename = "createdAt")]
    pub created_at: String,
}

/// A request's telemetry (`llm.js`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Telemetry {
    /// `provider`.
    pub provider: String,
    /// `model`.
    pub model: String,
    /// `effort`.
    pub effort: Option<String>,
    /// `startedAt`.
    #[serde(rename = "startedAt")]
    pub started_at: String,
    /// `wallMs`.
    #[serde(rename = "wallMs")]
    pub wall_ms: u64,
    /// `firstDeltaMs`.
    #[serde(rename = "firstDeltaMs")]
    pub first_delta_ms: Option<u64>,
    /// `contextBytes`.
    #[serde(rename = "contextBytes")]
    pub context_bytes: u64,
    /// `messageCount`.
    #[serde(rename = "messageCount")]
    pub message_count: u64,
    /// `usage`.
    pub usage: Option<Usage>,
    /// `contextTokens`: absent, `null`, or a count.
    #[serde(
        rename = "contextTokens",
        default,
        skip_serializing_if = "Option::is_none",
        with = "present"
    )]
    pub context_tokens: Option<Option<u64>>,
    /// `failed`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failed: Option<bool>,
    /// `cancelled`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cancelled: Option<bool>,
}

/// Token counts.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Usage {
    /// `inputTokens`.
    #[serde(rename = "inputTokens")]
    pub input_tokens: Option<u64>,
    /// `outputTokens`.
    #[serde(rename = "outputTokens")]
    pub output_tokens: Option<u64>,
    /// `cachedInputTokens`.
    #[serde(rename = "cachedInputTokens")]
    pub cached_input_tokens: Option<u64>,
    /// `cacheWriteTokens`.
    #[serde(rename = "cacheWriteTokens")]
    pub cache_write_tokens: Option<u64>,
    /// `reasoningTokens`.
    #[serde(rename = "reasoningTokens")]
    pub reasoning_tokens: Option<u64>,
}

/// A harness notice.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Notice {
    /// `level`.
    pub level: String,
    /// `text`.
    pub text: String,
}

/// An item of a harness's task list.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanItem {
    /// `text`.
    pub text: String,
    /// `status`: pending, active, done.
    pub status: String,
}

/// A stored tool call: its id, name and the model's raw arguments.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolCall {
    /// `id`.
    pub id: String,
    /// `type` (`function`).
    #[serde(rename = "type")]
    pub kind: String,
    /// `function`: name and arguments.
    pub function: FunctionCall,
    /// `extra_content`: Gemini's thought signature.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extra_content: Option<Value>,
}

/// A tool call's function.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FunctionCall {
    /// `name`.
    pub name: String,
    /// `arguments`: the model's JSON text, as written.
    pub arguments: String,
}

/// `role: "tool"`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tool {
    /// `tool_call_id`.
    pub tool_call_id: String,
    /// `telemetry`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub telemetry: Option<ToolTelemetry>,
    /// `content`: what the model read (JSON text, or bash's transcript).
    pub content: String,
    /// `createdAt`.
    #[serde(rename = "createdAt")]
    pub created_at: String,
}

/// A tool call's telemetry (`agent.js` 750).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolTelemetry {
    /// `name`.
    pub name: String,
    /// `startedAt`.
    #[serde(rename = "startedAt")]
    pub started_at: String,
    /// `wallMs`.
    #[serde(rename = "wallMs")]
    pub wall_ms: u64,
    /// `ok`.
    pub ok: bool,
    /// `cancelled`.
    pub cancelled: bool,
}

/// `role: "error"`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ErrorMessage {
    /// `content`.
    pub content: String,
    /// `telemetry`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub telemetry: Option<Telemetry>,
    /// `createdAt`.
    #[serde(rename = "createdAt")]
    pub created_at: String,
    /// `dismissed`: the user dismissed it.
    #[serde(default, skip_serializing_if = "is_false")]
    pub dismissed: bool,
}

/// Decodes a row's text; the error names `label` (`Conversation <id>
/// message <id>`).
pub fn decode(text: &str, label: &str) -> Result<Message, String> {
    serde_json::from_str(text).map_err(|error| format!("{label}: {error}"))
}

impl Message {
    /// A row's text decoded; unknown fields are refused, the error naming
    /// `label` (`Conversation <id> message <id>`).
    pub fn decode(text: &str, label: &str) -> Result<Message, String> {
        decode(text, label)
    }

    /// `role === 'user' && !seeBuild`: a prompt the user sent.
    pub fn is_prompt(&self) -> bool {
        matches!(self, Message::User(user) if !user.see_build)
    }

    /// `messageText(payload)`: what a message contributes to search.
    pub fn search_text(&self) -> String {
        match self {
            Message::Assistant(assistant) => assistant.content.clone(),
            Message::User(user) if !user.see_build => user.content.text(),
            _ => String::new(),
        }
    }

    /// `turnEnd(message)`: `final`, `stopped`, `failed`, or `None` for a
    /// turn still under way.
    pub fn turn_end(&self) -> Option<&'static str> {
        match self {
            Message::Error(_) => Some("failed"),
            Message::Assistant(assistant)
                if assistant.tool_calls.as_ref().is_none_or(Vec::is_empty) =>
            {
                Some(if assistant.stopped {
                    "stopped"
                } else {
                    "final"
                })
            }
            _ => None,
        }
    }

    /// The library prompts a user message was sent from.
    pub fn prompts(&self) -> &[PromptLink] {
        match self {
            Message::User(User {
                prompts: Some(prompts),
                ..
            }) => prompts,
            _ => &[],
        }
    }
}

impl Content {
    /// `messageText`'s text of a prompt: the string, or its text parts
    /// joined by newlines.
    pub fn text(&self) -> String {
        match self {
            Content::Text(text) => text.clone(),
            Content::Parts(parts) => parts
                .iter()
                .filter_map(|part| match part {
                    Part::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n"),
        }
    }
}

/// A typed value as JavaScript's `JSON.stringify` writes it, fields in the
/// struct's order.
pub fn to_json(value: &impl Serialize) -> Json {
    serde_json::to_value(value).expect("the store's types serialise")
}

/// A message about to be appended: the payload a writer of today's server
/// builds, in its key order, before `appendMessage` adds `createdAt`.
#[derive(Clone, Debug, PartialEq)]
pub struct NewMessage(pub(crate) Object);

/// What a model's reply holds (`agent.js` 662-673's `message`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Reply {
    /// `content` (`''` when the provider gave none).
    pub content: String,
    /// `reasoning`, kept when not empty.
    pub reasoning: Option<String>,
    /// `reasoning_details`.
    pub reasoning_details: Option<Vec<Value>>,
    /// `finish_reason`, kept when not empty.
    pub finish_reason: Option<String>,
    /// `telemetry`.
    pub telemetry: Option<Telemetry>,
    /// `notices`.
    pub notices: Option<Vec<Notice>>,
    /// `plan`.
    pub plan: Option<Vec<PlanItem>>,
    /// `rateLimit`.
    pub rate_limit: Option<Value>,
    /// `tool_calls`, kept when there are any.
    pub tool_calls: Vec<ToolCall>,
}

fn object(pairs: Vec<(&str, Json)>) -> NewMessage {
    let mut object = Object::new();
    for (key, value) in pairs {
        object.insert((key).to_string(), value);
    }
    NewMessage(object)
}

impl NewMessage {
    /// A chat prompt (`index.js` 472): `role, content, prompts?`.
    pub fn prompt(content: &Content, prompts: &[PromptLink]) -> NewMessage {
        let mut pairs = vec![("role", json::str("user")), ("content", to_json(content))];
        if !prompts.is_empty() {
            pairs.push(("prompts", to_json(&prompts)));
        }
        object(pairs)
    }

    /// A bench model's prompt (`index.js` 536, 544): `role, round,
    /// prompts?, content`.
    pub fn bench_prompt(
        round: &str,
        prompts: Option<&[PromptLink]>,
        content: &Content,
    ) -> NewMessage {
        let mut pairs = vec![("role", json::str("user")), ("round", json::str(round))];
        if let Some(prompts) = prompts {
            pairs.push(("prompts", to_json(&prompts)));
        }
        pairs.push(("content", to_json(content)));
        object(pairs)
    }

    /// A model's reply (`agent.js` 665-672).
    pub fn reply(reply: &Reply) -> NewMessage {
        let mut pairs = vec![
            ("role", json::str("assistant")),
            ("content", json::str(&reply.content)),
        ];
        if let Some(reasoning) = reply.reasoning.as_ref().filter(|r| !r.is_empty()) {
            pairs.push(("reasoning", json::str(reasoning)));
        }
        if let Some(details) = &reply.reasoning_details {
            pairs.push(("reasoning_details", to_json(details)));
        }
        if let Some(reason) = reply.finish_reason.as_ref().filter(|r| !r.is_empty()) {
            pairs.push(("finish_reason", json::str(reason)));
        }
        if let Some(telemetry) = &reply.telemetry {
            pairs.push(("telemetry", to_json(telemetry)));
        }
        if let Some(notices) = &reply.notices {
            pairs.push(("notices", to_json(notices)));
        }
        if let Some(plan) = &reply.plan {
            pairs.push(("plan", to_json(plan)));
        }
        if let Some(limit) = reply
            .rate_limit
            .as_ref()
            .filter(|limit| !matches!(limit, Value::Null | Value::Bool(false)))
        {
            pairs.push(("rateLimit", to_json(limit)));
        }
        if !reply.tool_calls.is_empty() {
            pairs.push(("tool_calls", to_json(&reply.tool_calls)));
        }
        object(pairs)
    }

    /// A tool call's result (`agent.js` 747-752): `role, tool_call_id,
    /// telemetry, content`.
    pub fn tool_result(call_id: &str, telemetry: &ToolTelemetry, content: &str) -> NewMessage {
        object(vec![
            ("role", json::str("tool")),
            ("tool_call_id", json::str(call_id)),
            ("telemetry", to_json(telemetry)),
            ("content", json::str(content)),
        ])
    }

    /// The result of a call its turn left unanswered (`agent.js` 769,
    /// `store.js` 1314): `role, tool_call_id, content`, the content
    /// `JSON.stringify(unansweredResult(...))`.
    pub fn unanswered(call_id: &str, stopped: bool, reason: &str) -> NewMessage {
        object(vec![
            ("role", json::str("tool")),
            ("tool_call_id", json::str(call_id)),
            (
                "content",
                Json::String(json::stringify(&crate::chats::unanswered_result(
                    stopped, reason,
                ))),
            ),
        ])
    }

    /// The screenshots after a tool result (`agent.js` 774-781): `role,
    /// seeBuild, content: [text, image_url...]`.
    pub fn screenshots(text: &str, image_urls: &[String]) -> NewMessage {
        let mut parts = vec![to_json(&Part::Text {
            text: text.to_string(),
        })];
        parts.extend(image_urls.iter().map(|url| {
            to_json(&Part::ImageUrl {
                image_url: ImageUrl { url: url.clone() },
            })
        }));
        object(vec![
            ("role", json::str("user")),
            ("seeBuild", Json::Bool(true)),
            ("content", Json::Array(parts)),
        ])
    }

    /// A stopped turn's end (`index.js` 337-338): `role, content, stopped,
    /// reply?, telemetry?`.
    pub fn stopped(
        content: &str,
        reply: Option<&str>,
        telemetry: Option<&Telemetry>,
    ) -> NewMessage {
        let mut pairs = vec![
            ("role", json::str("assistant")),
            ("content", json::str(content)),
            ("stopped", Json::Bool(true)),
        ];
        if let Some(reply) = reply.filter(|reply| !reply.is_empty()) {
            pairs.push(("reply", json::str(reply)));
        }
        if let Some(telemetry) = telemetry {
            pairs.push(("telemetry", to_json(telemetry)));
        }
        object(pairs)
    }

    /// A failed turn's error (`index.js` 272, 338, 548; `store.js` 1327):
    /// `role, content, telemetry?`.
    pub fn error(content: &str, telemetry: Option<&Telemetry>) -> NewMessage {
        let mut pairs = vec![
            ("role", json::str("error")),
            ("content", json::str(content)),
        ];
        if let Some(telemetry) = telemetry {
            pairs.push(("telemetry", to_json(telemetry)));
        }
        object(pairs)
    }

    /// `final.stopped`.
    pub(crate) fn is_stopped(&self) -> bool {
        json::truthy(self.0.get("stopped"))
    }

    /// `String(final.content)`.
    pub(crate) fn content_text(&self) -> String {
        json::to_js_string(self.0.get("content"))
    }
}

/// A row's text with `,"<key>":<value>` added before its closing brace:
/// what `JSON.stringify({ ...payload, key: value })` writes for a key the
/// payload lacks, the row's text being what `JSON.stringify` wrote.
pub(crate) fn splice(text: &str, pairs: &[(&str, &Json)]) -> String {
    let body = text
        .strip_suffix('}')
        .expect("a stored payload is an object");
    let mut out = String::with_capacity(text.len() + 64);
    out.push_str(body);
    let empty = body.trim_end() == "{";
    for (i, (key, value)) in pairs.iter().enumerate() {
        if !(empty && i == 0) {
            out.push(',');
        }
        out.push_str(&json::stringify(&Json::String((*key).to_string())));
        out.push(':');
        out.push_str(&json::stringify(value));
    }
    out.push('}');
    out
}
