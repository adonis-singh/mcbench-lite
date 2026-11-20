//! What a request sends (`server/agent.js` `apiMessages`): the system prompt,
//! then the history ([`crate::context`]) with images inlined as data URLs,
//! selections and attachments worded, a text-only model's images replaced,
//! and the fields the page alone reads removed. Each message is the JSON
//! object today's code makes, in its key order, so a request body is
//! byte-identical to Node's (the `prompts` and `stopped` fields that reach
//! chat-completions providers today included).

use base64::Engine as _;
use serde_json::{Map, Value};

use mcbench_providers::ApiMessage;
use mcbench_providers::api::js;
use mcbench_store::{Store, StoredMessage};

use crate::context::{self, Archives, WorkFiles};

/// `image-input.js` `NATIVE_IMAGE_TYPES`.
const NATIVE_IMAGE_TYPES: [&str; 4] = ["image/jpeg", "image/png", "image/gif", "image/webp"];

/// `supportsImageType(type)`.
pub fn supports_image_type(mime: &str) -> bool {
    NATIVE_IMAGE_TYPES.contains(&mime.to_lowercase().as_str())
}

/// `supportsImageData(data)`: a base64 data URL of a native image type.
pub fn supports_image_data(data: &str) -> bool {
    let Some(rest) = data.get(..5).filter(|p| p.eq_ignore_ascii_case("data:")) else {
        return false;
    };
    let rest = &data[rest.len()..];
    let end = rest.find([';', ',']).unwrap_or(rest.len());
    let mime = &rest[..end];
    !mime.is_empty()
        && rest[end..]
            .get(..8)
            .is_some_and(|p| p.eq_ignore_ascii_case(";base64,"))
        && supports_image_type(mime)
}

/// The messages of a chat's branch as `server/store.js` holds them in memory.
pub fn in_memory(messages: &[StoredMessage]) -> Result<Vec<Value>, String> {
    messages
        .iter()
        .map(|message| mcbench_json::parse(&message.in_memory_text()))
        .collect()
}

/// Where a request's history comes from and what it is cut to.
pub struct History<'a> {
    /// The chat's id (its `/work` holds the archives).
    pub chat: &'a str,
    /// Its messages in memory ([`in_memory`]).
    pub messages: &'a [Value],
    /// `MCBENCH_CONTEXT_MAX_BYTES`.
    pub max_bytes: usize,
    /// `keepExchanges`.
    pub keep_exchanges: usize,
}

fn text_part(text: String) -> Value {
    let mut part = Map::new();
    part.insert("type".into(), Value::String("text".into()));
    part.insert("text".into(), Value::String(text));
    Value::Object(part)
}

fn image_part(url: String) -> Value {
    let mut image = Map::new();
    image.insert("url".into(), Value::String(url));
    let mut part = Map::new();
    part.insert("type".into(), Value::String("image_url".into()));
    part.insert("image_url".into(), Value::Object(image));
    Value::Object(part)
}

fn base64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn get<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    js::get(Some(value), key)
}

fn string(value: Option<&Value>) -> String {
    value
        .map(js::to_string)
        .unwrap_or_else(|| "undefined".into())
}

/// The fields `apiMessages` removes: `messagePayload`'s and the page's.
const STRIPPED: [&str; 10] = [
    "id",
    "parentId",
    "position",
    "telemetry",
    "createdAt",
    "notices",
    "plan",
    "rateLimit",
    "round",
    "reply",
];

/// `apiMessages(conversation, { imageInput })` with the system prompt given.
pub fn api_messages(
    store: &Store,
    history: &History<'_>,
    image_input: bool,
    system_prompt: &str,
    archives: &Archives,
    work: &dyn WorkFiles,
) -> Result<Vec<ApiMessage>, String> {
    let mut system = Map::new();
    system.insert("role".into(), Value::String("system".into()));
    system.insert("content".into(), Value::String(system_prompt.into()));
    let mut out = vec![ApiMessage(system)];
    let messages = context::context_messages(
        history.chat,
        history.messages,
        history.max_bytes,
        history.keep_exchanges,
        archives,
        work,
    )?;
    let mut inlined: Vec<Value> = Vec::with_capacity(messages.len());
    for message in messages.iter().rev() {
        if get(message, "role").and_then(Value::as_str) == Some("error") {
            continue;
        }
        let Some(Value::Array(parts)) = get(message, "content") else {
            inlined.push(message.clone());
            continue;
        };
        let see_build = js::truthy(get(message, "seeBuild"));
        let mut rebuilt = Map::new();
        rebuilt.insert(
            "role".into(),
            get(message, "role").cloned().unwrap_or(Value::Null),
        );
        if !image_input && see_build {
            rebuilt.insert("seeBuild".into(), Value::Bool(true));
            rebuilt.insert(
                "content".into(),
                Value::Array(vec![text_part(
                    "[Visual tool result omitted because the selected model accepts text input only.]"
                        .into(),
                )]),
            );
            inlined.push(Value::Object(rebuilt));
            continue;
        }
        if see_build {
            rebuilt.insert("seeBuild".into(), Value::Bool(true));
        }
        let picked = parts
            .iter()
            .filter(|part| get(part, "type").and_then(Value::as_str) == Some("selection"))
            .count();
        let mut order = 0;
        let mut content = Vec::new();
        for part in parts {
            match get(part, "type").and_then(Value::as_str) {
                Some("selection") => {
                    order += 1;
                    let selection = get(part, "selection");
                    let field = |key: &str| js::get(selection, key);
                    let at = |key: &str, i: usize| {
                        string(field(key).and_then(|v| v.as_array()).and_then(|a| a.get(i)))
                    };
                    let of = if picked > 1 {
                        format!(" of {picked}")
                    } else {
                        String::new()
                    };
                    let region = text_part(format!(
                        "[Selection {order}{of} ({}): x {}..{}, y {}..{}, z {}..{}; {} blocks]",
                        string(field("label")),
                        at("min", 0),
                        at("max", 0),
                        at("min", 1),
                        at("max", 1),
                        at("min", 2),
                        at("max", 2),
                        string(field("blocks")),
                    ));
                    if order == 1 && picked > 1 {
                        content.push(text_part(format!(
                            "[The user picked {picked} regions in the viewer, numbered in the order picked]"
                        )));
                    }
                    content.push(region);
                }
                Some("attachment") => {
                    let file = get(part, "attachment");
                    let field = |key: &str| js::get(file, key);
                    let mime = string(field("type"));
                    content.push(text_part(format!(
                        "[Uploaded file: {} | {} | {} bytes | {}]",
                        string(field("name")),
                        mime,
                        string(field("size")),
                        string(field("path")),
                    )));
                    if supports_image_type(&mime) && image_input {
                        let url = string(field("url"));
                        let bytes = store.read_attachment(&url).map_err(|e| e.message)?;
                        content.push(image_part(format!("data:{mime};base64,{}", base64(&bytes))));
                    }
                }
                Some("image_url") if !image_input => {
                    content.push(text_part(
                        "[Image content omitted because the selected model accepts text input only.]"
                            .into(),
                    ));
                }
                Some("image_url") => {
                    let url = get(part, "image_url")
                        .and_then(|i| get(i, "url"))
                        .and_then(Value::as_str)
                        .ok_or("Cannot read properties of undefined (reading 'startsWith')")?;
                    if !url.starts_with("/api/images/") {
                        content.push(part.clone());
                        continue;
                    }
                    let name = url.rsplit('/').next().unwrap_or("");
                    let mime = if url.ends_with(".jpg") {
                        "image/jpeg".to_string()
                    } else {
                        format!("image/{}", url.rsplit('.').next().unwrap_or(""))
                    };
                    let bytes = store.read_image(url).map_err(|e| e.message)?;
                    let image = image_part(format!("data:{mime};base64,{}", base64(&bytes)));
                    if !name.starts_with("view-") {
                        content.push(text_part(format!("[/work/images/{name}]")));
                    }
                    content.push(image);
                }
                _ => content.push(part.clone()),
            }
        }
        rebuilt.insert("content".into(), Value::Array(content));
        inlined.push(Value::Object(rebuilt));
    }
    inlined.reverse();
    for message in inlined {
        let Value::Object(map) = message else {
            return Err("a message is not an object".into());
        };
        out.push(ApiMessage(
            map.into_iter()
                .filter(|(key, _)| !STRIPPED.contains(&key.as_str()))
                .collect(),
        ));
    }
    Ok(out)
}
