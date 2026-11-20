//! The CLI harnesses (`server/harness.js`): Claude Code ([`claude_code`])
//! and Codex CLI ([`codex_cli`]) run as child processes with mcbench's tools
//! as their only tools, reached through the [`crate::bridge`]. Each tool call
//! a CLI makes is relayed to the agent's turn loop as an ordinary completion
//! tool call, and the next completion request delivers the loop's stored
//! result back to the waiting call: one turn loop, one transcript, one tool
//! executor. [`install`] is the CLIs' pinned download.
//!
//! This module is what both share: the prompt a CLI is given (the earlier
//! transcript or the notes a resumed session lacks, then the request), the
//! session records under `<data>/harness/<chat>/<harness>.json` and the
//! rules for resuming one, the environment a CLI starts with, the relay
//! (`relay`) and the JSON-lines child process (`process`).

pub mod claude_code;
pub mod codex_cli;
pub mod completion;
pub mod install;
pub(crate) mod process;
pub(crate) mod relay;
pub(crate) mod text;
pub(crate) mod tree;

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use crate::bridge::{js_string, truthy};

use crate::{ApiMessage, CompletionRequest, ToolSchema};
pub use completion::{delta_json, message_json, reply_of};

/// One message of the stored transcript, what a harness reads of it to
/// resume its session (`harness.js` 114-117 and 160-166, `claude-code.js`
/// 390): its id, its role, whether it is `see_build`'s screenshots, whether
/// the user stopped it and the reply text they saw then.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TranscriptEntry {
    /// The message's id.
    pub id: String,
    /// `user`, `assistant`, `tool` or `error`.
    pub role: String,
    /// `seeBuild`: a user message holding screenshots, not a request.
    pub see_build: bool,
    /// `stopped`: the user stopped the turn this message ended.
    pub stopped: bool,
    /// `reply`: the text the user saw streamed when they stopped it.
    pub reply: Option<String>,
}

/// The chat a completion belongs to.
#[derive(Clone, Debug)]
pub(crate) struct Conversation {
    /// The chat's id: the session folder's name and the key of its run.
    pub id: String,
    /// The stored transcript, oldest first, whose last message is this
    /// turn's request; `None` runs without resuming or recording a session.
    pub transcript: Option<Vec<TranscriptEntry>>,
}

/// One completion of a harness (`llm.js` `chatCompletion`'s options as the
/// harnesses read them).
#[derive(Clone, Debug)]
pub(crate) struct HarnessRequest {
    /// The chat, or `None` for a one-shot completion (a title, a prompt
    /// rewrite), which keeps nothing.
    pub conversation: Option<Conversation>,
    /// The model id.
    pub model: String,
    /// The reasoning effort.
    pub effort: Option<String>,
    /// The messages, oldest first (`agent.js` `apiMessages`), each a JSON
    /// object.
    pub messages: Vec<Value>,
    /// The tools offered.
    pub tools: Vec<ToolSchema>,
    /// A chat turn rather than a benchmark attempt.
    pub chat: bool,
}

impl HarnessRequest {
    /// The request as the [`crate::Provider`] seam gives it: a chat (with
    /// its transcript, or none), or a one-shot completion. A transcript with
    /// no chat is refused (today `recordSession` throws on it).
    pub(crate) fn from_completion(request: &CompletionRequest) -> Result<Self, String> {
        if request.conversation_id.is_none() && request.transcript.is_some() {
            return Err(
                "A harness completion with a transcript needs its conversation id".to_owned(),
            );
        }
        Ok(HarnessRequest {
            conversation: request.conversation_id.as_ref().map(|id| Conversation {
                id: id.clone(),
                transcript: request.transcript.clone(),
            }),
            model: request.model.clone(),
            effort: request.effort.clone(),
            messages: request
                .messages
                .iter()
                .map(|ApiMessage(m)| Value::Object(m.clone()))
                .collect(),
            tools: request.tools.clone(),
            chat: request.chat,
        })
    }

    pub(crate) fn conversation_id(&self) -> Option<&str> {
        self.conversation.as_ref().map(|c| c.id.as_str())
    }

    pub(crate) fn transcript(&self) -> Option<&[TranscriptEntry]> {
        self.conversation
            .as_ref()
            .and_then(|c| c.transcript.as_deref())
    }

    pub(crate) fn values(&self) -> Vec<&Value> {
        self.messages.iter().collect()
    }

    /// The tools' names (`t.function.name`).
    pub(crate) fn tool_names(&self) -> Vec<String> {
        self.tools.iter().map(tool_name).collect()
    }
}

pub(crate) fn tool_name(schema: &ToolSchema) -> String {
    let tool = mcbench_json::parse(&schema.0).expect("a tool schema is JSON text");
    tool.get("function")
        .and_then(|f| f.get("name"))
        .map(js_string)
        .unwrap_or_else(|| "undefined".to_owned())
}

/// An image the prompt carries: its type and base64 data.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PromptImage {
    /// The media type (`image/png`).
    pub mime_type: String,
    /// The base64 data.
    pub data: String,
}

/// `harness.js` `dataImage`: `data:<type>;base64,<data>`.
pub(crate) fn data_image(url: &str) -> Result<PromptImage, String> {
    let bad = || text::BAD_IMAGE.to_owned();
    let rest = url.strip_prefix("data:").ok_or_else(bad)?;
    let end = rest.find([';', ',']).ok_or_else(bad)?;
    let (mime, rest) = rest.split_at(end);
    let data = rest.strip_prefix(";base64,").ok_or_else(bad)?;
    // `.+` matches no line terminator.
    if mime.is_empty()
        || data.is_empty()
        || data.contains(['\n', '\r', '\u{2028}', '\u{2029}'])
        || mime.contains(['\n', '\r', '\u{2028}', '\u{2029}'])
    {
        return Err(bad());
    }
    Ok(PromptImage {
        mime_type: mime.to_owned(),
        data: data.to_owned(),
    })
}

/// `harness.js` `parts`: string content is one text part.
fn parts(content: Option<&Value>) -> Vec<Value> {
    match content {
        Some(Value::String(text)) => vec![json!({ "type": "text", "text": text })],
        Some(Value::Array(parts)) => parts.clone(),
        _ => Vec::new(),
    }
}

fn part_type(part: &Value) -> String {
    part.get("type")
        .map(js_string)
        .unwrap_or_else(|| "undefined".to_owned())
}

fn image_url(part: &Value) -> String {
    part.get("image_url")
        .and_then(|u| u.get("url"))
        .map(js_string)
        .unwrap_or_else(|| "undefined".to_owned())
}

/// `harness.js` `promptRenderer`: text of message content, images as
/// numbered placeholders whose bytes are collected.
struct Renderer {
    images: Vec<PromptImage>,
}

impl Renderer {
    fn render(&mut self, content: Option<&Value>) -> Result<String, String> {
        let mut out = Vec::new();
        for part in parts(content) {
            let kind = part_type(&part);
            let text = match kind.as_str() {
                "text" => match part.get("text") {
                    None | Some(Value::Null) => String::new(),
                    Some(text) => js_string(text),
                },
                "image_url" => {
                    self.images.push(data_image(&image_url(&part))?);
                    format!("{}{}{}", text::IMAGE.0, self.images.len(), text::IMAGE.1)
                }
                _ => {
                    return Err(format!(
                        "{}{kind}{}",
                        text::UNSUPPORTED.0,
                        text::UNSUPPORTED.1
                    ));
                }
            };
            // `.filter(Boolean)`.
            if !text.is_empty() {
                out.push(text);
            }
        }
        Ok(out.join("\n"))
    }
}

fn role(message: &Value) -> Option<&str> {
    message.get("role").and_then(Value::as_str)
}

/// `harness.js` `splitRequest`: the system prompt, the earlier transcript
/// and the new request. Trailing user messages after a tool result are its
/// screenshots, not a new request.
fn split_request<'a>(messages: &[&'a Value]) -> (String, Option<&'a Value>, Vec<&'a Value>) {
    let system = messages
        .iter()
        .filter(|m| role(m) == Some("system"))
        .map(|m| match m.get("content") {
            None => String::new(),
            Some(content) => js_string(content),
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    let history: Vec<&Value> = messages
        .iter()
        .copied()
        .filter(|m| role(m) != Some("system"))
        .collect();
    let mut start = history.len();
    while start > 0 && role(history[start - 1]) == Some("user") {
        start -= 1;
    }
    let request =
        if start < history.len() && (start == 0 || role(history[start - 1]) != Some("tool")) {
            history.last().copied()
        } else {
            None
        };
    let prior = if request.is_some() {
        history[..history.len() - 1].to_vec()
    } else {
        history
    };
    (system, request, prior)
}

fn render_transcript(prior: &[&Value], renderer: &mut Renderer) -> Result<Vec<String>, String> {
    let mut tool_names: Vec<(String, String)> = Vec::new();
    let mut out = Vec::new();
    for message in prior {
        let content = message.get("content");
        match role(message) {
            Some("user") => out.push(format!("{}{}", text::USER, renderer.render(content)?)),
            Some("tool") => {
                let id = message
                    .get("tool_call_id")
                    .map(js_string)
                    .unwrap_or_else(|| "undefined".to_owned());
                let name = tool_names
                    .iter()
                    .rev()
                    .find(|(call, _)| *call == id)
                    .map(|(_, name)| name.clone())
                    .unwrap_or_else(|| "undefined".to_owned());
                out.push(format!(
                    "{}{name}{}{id}{}{}",
                    text::RESULT.0,
                    text::RESULT.1,
                    text::RESULT.2,
                    renderer.render(content)?
                ));
            }
            _ => {
                let mut lines = vec![if truthy(content) {
                    format!("{}\n{}", text::ASSISTANT, js_string(content.unwrap()))
                } else {
                    text::ASSISTANT.to_owned()
                }];
                if let Some(Value::Array(calls)) = message.get("tool_calls") {
                    for call in calls {
                        let id = call
                            .get("id")
                            .map(js_string)
                            .unwrap_or_else(|| "undefined".to_owned());
                        let function = call.get("function");
                        let name = function
                            .and_then(|f| f.get("name"))
                            .map(js_string)
                            .unwrap_or_else(|| "undefined".to_owned());
                        let arguments = function.and_then(|f| f.get("arguments"));
                        let arguments = if truthy(arguments) {
                            js_string(arguments.unwrap())
                        } else {
                            text::NO_ARGUMENTS.to_owned()
                        };
                        tool_names.retain(|(call, _)| *call != id);
                        tool_names.push((id.clone(), name.clone()));
                        lines.push(format!(
                            "{}{name}{}{id}{}{arguments}",
                            text::CALLED.0,
                            text::CALLED.1,
                            text::CALLED.2
                        ));
                    }
                }
                out.push(lines.join("\n"));
            }
        }
    }
    Ok(out)
}

/// The prompt a CLI harness receives (`harness.js` `harnessPrompt`): a new
/// session gets the earlier transcript first (`history`), a resumed one
/// the `notes` it lacks, then the new request. The system prompt travels
/// separately, as the harness's appended instructions.
pub fn harness_prompt(
    messages: &[&Value],
    history: bool,
    notes: Vec<String>,
) -> Result<(String, Vec<PromptImage>), String> {
    let (_, request, prior) = split_request(messages);
    let mut renderer = Renderer { images: Vec::new() };
    let mut sections = notes;
    if history {
        let transcript = render_transcript(&prior, &mut renderer)?;
        if !transcript.is_empty() {
            sections.push(format!(
                "{}{}{}",
                text::CONVERSATION.0,
                transcript.join("\n\n"),
                text::CONVERSATION.1
            ));
        }
    }
    sections.push(match request {
        Some(request) => renderer.render(request.get("content"))?,
        None => text::CONTINUE.to_owned(),
    });
    Ok((sections.join("\n\n"), renderer.images))
}

/// `harness.js` `systemText`.
pub fn system_text(messages: &[&Value]) -> String {
    split_request(messages).0
}

/// `prompt.js` `splitResolution`: the prompt every chat gets and the
/// resolution section after it ('' at Standard).
pub fn split_resolution(text: &str) -> (&str, &str) {
    let marker = format!("\n\n{}\n", text::RESOLUTION_HEADING);
    match text.find(&marker) {
        None => (text, ""),
        Some(at) => (&text[..at], &text[at + 2..]),
    }
}

/// `prompt.js` `resolutionUpdate`.
pub fn resolution_update(section: &str) -> String {
    if section.is_empty() {
        text::RESOLUTION_STANDARD.to_owned()
    } else {
        format!(
            "{}{section}{}",
            text::RESOLUTION_CHANGE.0,
            text::RESOLUTION_CHANGE.1
        )
    }
}

fn digest(text: &str) -> String {
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The instructions a session was given, as its record keeps them
/// (`harness.js` `sessionTerms`): a digest of the system prompt without its
/// resolution section and one of that section.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Terms {
    /// `instructions`.
    pub instructions: String,
    /// `resolution`.
    pub resolution: String,
}

/// `harness.js` `sessionTerms`.
pub fn session_terms(messages: &[&Value]) -> Terms {
    let system = system_text(messages);
    let (base, resolution) = split_resolution(&system);
    Terms {
        instructions: digest(base),
        resolution: digest(resolution),
    }
}

/// A session record: the JSON object `<harness>.json` holds.
pub type Record = Map<String, Value>;

/// `harness.js` `resolutionNote`.
pub fn resolution_note(record: &Record, messages: &[&Value]) -> Vec<String> {
    let system = system_text(messages);
    let (_, resolution) = split_resolution(&system);
    if record.get("resolution") == Some(&Value::String(digest(resolution))) {
        Vec::new()
    } else {
        vec![resolution_update(resolution)]
    }
}

fn last_seen_index(record: &Record, transcript: &[TranscriptEntry]) -> Option<usize> {
    let last_seen = record.get("lastSeen")?;
    transcript
        .iter()
        .position(|m| Value::String(m.id.clone()) == *last_seen)
}

/// `harness.js` `stoppedReplyNote`: a reply the user stopped while it
/// streamed, in the session's latest turn.
pub fn stopped_reply_note(record: &Record, transcript: &[TranscriptEntry]) -> Vec<String> {
    // `findIndex` gives -1 when the request is gone, and `slice(0, -1)`.
    let from = last_seen_index(record, transcript).map_or(0, |i| i + 1);
    let to = transcript.len().saturating_sub(1);
    let stopped = transcript
        .get(from..to.max(from))
        .unwrap_or(&[])
        .iter()
        .find(|m| m.stopped && m.reply.as_deref().is_some_and(|r| !r.is_empty()));
    match stopped {
        Some(m) => vec![format!(
            "{}{}{}",
            text::STOPPED_REPLY.0,
            m.reply.as_deref().unwrap(),
            text::STOPPED_REPLY.1
        )],
        None => Vec::new(),
    }
}

/// `harness.js` `pendingResult`: the pending call's tool message and any
/// screenshots right after it, as MCP content blocks; `None` when the
/// messages hold no result for it, or something other than screenshots
/// follows it.
pub fn pending_result(messages: &[&Value], call_id: &str) -> Result<Option<Vec<Value>>, String> {
    let Some(index) = messages.iter().position(|m| {
        role(m) == Some("tool") && m.get("tool_call_id") == Some(&Value::String(call_id.to_owned()))
    }) else {
        return Ok(None);
    };
    if messages[index + 1..]
        .iter()
        .any(|m| role(m) != Some("user"))
    {
        return Ok(None);
    }
    let mut blocks = Vec::new();
    for message in &messages[index..] {
        for part in parts(message.get("content")) {
            let kind = part_type(&part);
            match kind.as_str() {
                "text" => {
                    let mut block = Map::new();
                    block.insert("type".to_owned(), json!("text"));
                    if let Some(text) = part.get("text") {
                        block.insert("text".to_owned(), text.clone());
                    }
                    blocks.push(Value::Object(block));
                }
                "image_url" => {
                    let image = data_image(&image_url(&part))?;
                    blocks.push(
                        json!({ "type": "image", "mimeType": image.mime_type, "data": image.data }),
                    );
                }
                _ => {
                    return Err(format!(
                        "{}{kind}{}",
                        text::UNSUPPORTED.0,
                        text::UNSUPPORTED.1
                    ));
                }
            }
        }
    }
    Ok(Some(blocks))
}

fn user_prompt(m: &TranscriptEntry) -> bool {
    m.role == "user" && !m.see_build
}

/// `harness.js` `sessionContinues`: a stored session continues the
/// transcript when the request of its latest turn is still on it and
/// everything between that request and the new one belongs to that turn.
pub fn session_continues(record: Option<&Record>, transcript: &[TranscriptEntry]) -> bool {
    let Some(record) = record else { return false };
    let Some(index) = last_seen_index(record, transcript) else {
        return false;
    };
    if !transcript.last().is_some_and(user_prompt) {
        return false;
    }
    !transcript
        .get(index + 1..transcript.len() - 1)
        .unwrap_or(&[])
        .iter()
        .any(user_prompt)
}

/// Where a chat's harness records are (`harness.js` `conversationDir`).
pub fn conversation_dir(data_dir: &Path, conversation_id: &str) -> PathBuf {
    data_dir.join("harness").join(conversation_id)
}

fn session_file(data_dir: &Path, conversation_id: &str, harness: &str) -> PathBuf {
    conversation_dir(data_dir, conversation_id).join(format!("{harness}.json"))
}

/// `harness.js` `readSession`: the record, or `None` when there is none.
pub fn read_session(
    data_dir: &Path,
    conversation_id: &str,
    harness: &str,
) -> Result<Option<Record>, String> {
    let file = session_file(data_dir, conversation_id, harness);
    match std::fs::read_to_string(&file) {
        Ok(text) => match crate::api::v8json::parse(&text)? {
            Value::Object(record) => Ok(Some(record)),
            other => Err(format!(
                "{} holds {other}, not a session record",
                file.display()
            )),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{}: {e}", file.display())),
    }
}

/// `harness.js` `writeSession`: written whole, then renamed into place, so
/// a record cut short by a crash never fails to parse.
pub fn write_session(
    data_dir: &Path,
    conversation_id: &str,
    harness: &str,
    record: &Record,
) -> Result<(), String> {
    let dir = conversation_dir(data_dir, conversation_id);
    std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir {}: {e}", dir.display()))?;
    let file = session_file(data_dir, conversation_id, harness);
    let temporary = PathBuf::from(format!("{}.{}.tmp", file.display(), std::process::id()));
    std::fs::write(
        &temporary,
        mcbench_json::stringify_pretty(&Value::Object(record.clone()), 2),
    )
    .map_err(|e| format!("write {}: {e}", temporary.display()))?;
    std::fs::rename(&temporary, &file).map_err(|e| format!("rename {}: {e}", file.display()))
}

/// `harness.js` `forgetSession`: a turn that starts forgets the stored
/// session until the harness records it again with this turn's request.
pub fn forget_session(data_dir: &Path, conversation_id: &str, harness: &str) -> Result<(), String> {
    let file = session_file(data_dir, conversation_id, harness);
    match std::fs::remove_file(&file) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("rm {}: {e}", file.display())),
    }
}

/// `harness.js` `CLI_ENV`: what a CLI needs to run (path, home, locale,
/// proxies, certificates) and nothing else. Keys in the app's environment
/// and anything it inherited stay out.
const CLI_ENV: [&str; 20] = [
    "PATH",
    "HOME",
    "USER",
    "LOGNAME",
    "SHELL",
    "TMPDIR",
    "LANG",
    "TERM",
    "TZ",
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "NO_PROXY",
    "ALL_PROXY",
    "http_proxy",
    "https_proxy",
    "no_proxy",
    "all_proxy",
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
    "NODE_EXTRA_CA_CERTS",
];

/// What a Windows program needs besides [`CLI_ENV`] to start and find its
/// account's folders (Windows' own folder, the profile, the temporary
/// folder, the program extensions). Windows' names are matched in any case,
/// as Windows matches them (its `PATH` is usually `Path`).
#[cfg(windows)]
const WINDOWS_ENV: [&str; 14] = [
    "SystemRoot",
    "SystemDrive",
    "windir",
    "ComSpec",
    "PATHEXT",
    "USERPROFILE",
    "HOMEDRIVE",
    "HOMEPATH",
    "APPDATA",
    "LOCALAPPDATA",
    "ProgramData",
    "TEMP",
    "TMP",
    "USERNAME",
];

/// Whether this process's variable `key` is one a CLI gets.
fn cli_key(key: &str) -> bool {
    #[cfg(windows)]
    {
        CLI_ENV
            .iter()
            .chain(WINDOWS_ENV.iter())
            .any(|allowed| allowed.eq_ignore_ascii_case(key))
            || key.starts_with("LC_")
    }
    #[cfg(not(windows))]
    {
        CLI_ENV.contains(&key) || key.starts_with("LC_")
    }
}

/// An environment, in order, as a child is given it.
pub type Env = Vec<(OsString, OsString)>;

/// `harness.js` `cliEnv`: the allowed part of this process's environment
/// and `extra`; a key in both keeps its place with `extra`'s value.
pub fn cli_env(extra: &[(&str, OsString)]) -> Env {
    let mut env: Env = std::env::vars_os()
        .filter(|(key, _)| key.to_str().is_some_and(cli_key))
        .collect();
    for (key, value) in extra {
        match env.iter_mut().find(|(k, _)| k == key) {
            Some(slot) => slot.1 = value.clone(),
            None => env.push((OsString::from(key), value.clone())),
        }
    }
    env
}

/// `harness.js` `expandHome`: a leading `~` is the home folder.
pub fn expand_home(value: &str) -> PathBuf {
    match value.strip_prefix('~') {
        Some(rest) if rest.is_empty() || rest.starts_with('/') => {
            PathBuf::from(format!("{}{rest}", home_dir().display()))
        }
        _ => PathBuf::from(value),
    }
}

/// `os.homedir()`.
pub fn home_dir() -> PathBuf {
    std::env::home_dir().expect("this process has a home folder")
}

/// `os.tmpdir()`.
pub(crate) fn temp_dir() -> PathBuf {
    let dir = std::env::temp_dir();
    // Node strips one trailing separator.
    let text = dir.to_string_lossy();
    match text.strip_suffix('/') {
        Some(stripped) if !stripped.is_empty() => PathBuf::from(stripped),
        _ => dir,
    }
}

/// `fs.mkdtempSync(prefix)`: `prefix` and six random characters, a new
/// folder private to this account.
pub(crate) fn mkdtemp(prefix: &Path) -> Result<PathBuf, String> {
    const CHARS: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    loop {
        let mut bytes = [0u8; 6];
        getrandom::fill(&mut bytes).map_err(|e| format!("random: {e}"))?;
        let suffix: String = bytes
            .iter()
            .map(|b| CHARS[*b as usize % CHARS.len()] as char)
            .collect();
        let dir = PathBuf::from(format!("{}{suffix}", prefix.display()));
        #[cfg_attr(not(unix), allow(unused_mut))]
        let mut builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
        match builder.create(&dir) {
            Ok(()) => return Ok(dir),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("mkdtemp {}: {e}", dir.display())),
        }
    }
}

/// A run's runtime folder (`fs.mkdtempSync(path.join(os.tmpdir(),
/// 'mcb-'))`), which holds its Unix sockets: on macOS and Linux it is made
/// in `mcbench_relay::socket_base`, the one rule for where a socket of the
/// app's lives; on Windows, whose bridge is a named pipe, in `os.tmpdir()`.
pub fn runtime_dir() -> Result<PathBuf, String> {
    #[cfg(unix)]
    let base = mcbench_relay::socket_base().map_err(|e| e.to_string())?;
    #[cfg(not(unix))]
    let base = temp_dir();
    mkdtemp(&base.join("mcb-"))
}

/// `crypto.randomUUID()`: a version 4 UUID.
pub(crate) fn random_uuid() -> String {
    let mut b = [0u8; 16];
    getrandom::fill(&mut b).expect("the system's random source answers");
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let hex: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

/// How a CLI reaches the bridge: the app's own binary as
/// `<program> tool-bridge <address>`.
#[derive(Clone, Debug)]
pub struct BridgeCommand {
    /// The program (the app's binary, `std::env::current_exe()`).
    pub program: PathBuf,
}

impl BridgeCommand {
    /// The command and its arguments for `address`.
    pub fn for_address(&self, address: &str) -> (String, Vec<String>) {
        (
            self.program.to_string_lossy().into_owned(),
            vec!["tool-bridge".to_owned(), address.to_owned()],
        )
    }
}

/// The CLIs' sign-in commands for `login::SignIns` (`authCommand`,
/// `loginCommand`, `storedLogin`).
pub struct Logins {
    /// Claude Code's.
    pub claude: claude_code::ClaudeConfig,
    /// Codex's.
    pub codex: codex_cli::CodexConfig,
}

fn command(spec: CommandSpec) -> crate::login::Command {
    crate::login::Command {
        file: spec.file.to_string_lossy().into_owned(),
        args: spec.args,
        env: spec
            .env
            .into_iter()
            .map(|(k, v)| {
                (
                    k.to_string_lossy().into_owned(),
                    v.to_string_lossy().into_owned(),
                )
            })
            .collect(),
    }
}

impl crate::login::LoginPrograms for Logins {
    fn login(&self, login: crate::login::Login, method: Option<&str>) -> crate::login::Command {
        match login {
            crate::login::Login::Claude => {
                let method = format!(
                    "--{}",
                    method.expect("Claude Code's sign-in names its method")
                );
                command(self.claude.auth_command(&["login", &method]))
            }
            crate::login::Login::Codex => {
                command(self.codex.login_command(&["login", "--device-auth"]))
            }
        }
    }

    fn logout(&self, login: crate::login::Login) -> crate::login::Command {
        match login {
            crate::login::Login::Claude => command(self.claude.auth_command(&["logout"])),
            crate::login::Login::Codex => command(self.codex.login_command(&["logout"])),
        }
    }

    fn stored(&self, login: crate::login::Login) -> Result<bool, String> {
        match login {
            crate::login::Login::Claude => self.claude.stored_login(),
            crate::login::Login::Codex => Ok(self.codex.stored_login()),
        }
    }
}

/// A command to run, with its own environment (`authCommand`,
/// `loginCommand`).
#[derive(Clone, Debug)]
pub struct CommandSpec {
    /// The program.
    pub file: OsString,
    /// Its arguments.
    pub args: Vec<String>,
    /// Its whole environment.
    pub env: Env,
}

/// The bridge's address for a run's runtime folder: a Unix socket in it, or
/// on Windows a named pipe named after it.
pub(crate) fn bridge_address(socket: &Path) -> crate::bridge::BridgeAddress {
    #[cfg(unix)]
    {
        crate::bridge::BridgeAddress::UnixSocket(socket.to_path_buf())
    }
    #[cfg(not(unix))]
    {
        let name = socket
            .to_string_lossy()
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect::<String>();
        crate::bridge::BridgeAddress::NamedPipe(format!("\\\\.\\pipe\\mcbench-{name}"))
    }
}

/// JavaScript's `a + b + c` of numbers that may be missing (`undefined`
/// makes NaN, which `JSON.stringify` writes as `null`).
pub(crate) fn js_sum(values: &[Option<&Value>]) -> Value {
    let mut sum = 0.0;
    for value in values {
        match value.and_then(Value::as_f64) {
            Some(x) => sum += x,
            None => return Value::Null,
        }
    }
    json_number(sum)
}

/// A double as a JSON number (`null` for NaN and the infinities, as
/// `JSON.stringify` writes them).
pub(crate) fn json_number(x: f64) -> Value {
    if x.fract() == 0.0 && x.abs() < 9_007_199_254_740_992.0 {
        json!(x as i64)
    } else {
        serde_json::Number::from_f64(x).map_or(Value::Null, Value::Number)
    }
}

#[cfg(test)]
mod tests;
