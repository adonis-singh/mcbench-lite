//! A chat's transcript as the screen shows it: items from the stored
//! messages, live turn events applied to them, and the entries they group
//! into. A port of `web/transcript.mjs` (`applyEvent`, `groupItems`,
//! `entryKeys`), `web/tool-stream.mjs` and `web/app.js` `itemsFromMessages`;
//! no GPUI types.

use mcbench_agent::TurnEvent;
use serde_json::Value;

use super::seam::{Diff, Shown, StoredMessage, UserPart, program_of, progress_of, shown_from};

/// Where a tool call is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolStatus {
    /// Its arguments still stream in.
    Writing,
    /// It runs.
    Running,
    /// It finished.
    Ok,
    /// It failed.
    Failed,
    /// It was cancelled.
    Cancelled,
}

/// A tool call row.
#[derive(Clone, Debug, PartialEq)]
pub struct ToolCall {
    /// The call's id.
    pub call_id: String,
    /// The tool's name.
    pub tool: String,
    /// The line it shows.
    pub summary: String,
    /// The program or command, when it has one.
    pub program: Option<String>,
    /// Where it is.
    pub status: ToolStatus,
    /// The arguments as they streamed.
    pub args_raw: String,
    /// The output (bash's transcript, `test_build`'s result).
    pub output: Option<String>,
    /// Why it failed.
    pub detail: String,
    /// The world's change.
    pub diff: Option<Diff>,
    /// Its progress line while it runs.
    pub progress: Option<String>,
}

/// Where a user message's file is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileSource {
    /// Stored: its `/api/attachments/...` address (`Core::attachment_file`).
    Stored(String),
    /// Just sent: its bytes, until the stored transcript replaces it.
    Sent(std::sync::Arc<[u8]>),
}

/// A file a user message carries, as the transcript shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShownFile {
    /// Its name.
    pub name: String,
    /// Its type (`image/...` shows a thumbnail that opens the lightbox).
    pub content_type: String,
    /// Its size in bytes.
    pub size: u64,
    /// Where its bytes are.
    pub source: FileSource,
}

impl ShownFile {
    /// Whether it is shown as a picture.
    pub fn is_image(&self) -> bool {
        self.content_type.starts_with("image/")
    }
}

/// The transcript of the chat on screen (`LOADING`, `SHOWN`, `readFailed`):
/// not read yet, so nothing claims it is empty; shown; or its read failed,
/// with why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReadState {
    /// Not on screen yet.
    Loading,
    /// The items are the chat's (none for a new chat).
    Shown,
    /// Reading it failed.
    Failed(String),
}

/// An item of the transcript (`web/app.js` items).
#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    /// What the user sent.
    User {
        /// The text.
        text: String,
        /// The stored message's id.
        message_id: Option<String>,
        /// Its files.
        files: Vec<ShownFile>,
        /// Its selections in the viewer.
        selections: Vec<crate::Selection>,
    },
    /// The model's text.
    Assistant {
        /// The text (markdown).
        text: String,
        /// The turn's last message (`final`).
        last: bool,
        /// Still streaming.
        live: bool,
    },
    /// The model's reasoning.
    Reasoning {
        /// The text (markdown).
        text: String,
        /// Still streaming.
        live: bool,
    },
    /// A tool call.
    Tool(ToolCall),
    /// Pictures a tool returned.
    Shots {
        /// Their addresses.
        images: Vec<std::path::PathBuf>,
    },
    /// A turn's error.
    Error {
        /// The message.
        text: String,
        /// The stored message's id.
        message_id: Option<String>,
    },
    /// A harness's notice.
    Notice {
        /// `info`, `warning` or `error`.
        level: String,
        /// The text.
        text: String,
    },
    /// A harness's task list.
    Plan(Value),
}

impl Item {
    fn is_activity(&self) -> bool {
        matches!(
            self,
            Item::Reasoning { .. }
                | Item::Tool(_)
                | Item::Shots { .. }
                | Item::Assistant { .. }
                | Item::Notice { .. }
                | Item::Plan(_)
        )
    }
}

/// The items of stored messages, as `web/app.js` `itemsFromMessages`
/// rebuilds them.
pub fn items_from_messages(messages: &[StoredMessage]) -> Vec<Item> {
    let mut items: Vec<Item> = Vec::new();
    for message in messages {
        match message {
            StoredMessage::User {
                id,
                parts,
                see_build,
            } => {
                if *see_build {
                    items.push(Item::Shots {
                        images: parts
                            .iter()
                            .filter_map(|p| match p {
                                UserPart::ImageUrl(url) => Some(std::path::PathBuf::from(url)),
                                _ => None,
                            })
                            .collect(),
                    });
                } else {
                    items.push(Item::User {
                        text: parts
                            .iter()
                            .filter_map(|p| match p {
                                UserPart::Text(text) => Some(text.as_str()),
                                _ => None,
                            })
                            .collect::<Vec<_>>()
                            .join("\n"),
                        message_id: id.clone(),
                        files: parts
                            .iter()
                            .filter_map(|p| match p {
                                UserPart::Attachment(file) => Some(ShownFile {
                                    name: file.name.clone(),
                                    content_type: file.mime.clone(),
                                    size: file.size,
                                    source: FileSource::Stored(file.url.clone()),
                                }),
                                _ => None,
                            })
                            .collect(),
                        selections: parts
                            .iter()
                            .filter_map(|p| match p {
                                UserPart::Selection(s) => Some(crate::Selection {
                                    letter: s.letter.clone(),
                                    label: s.label.clone(),
                                    min: s.min,
                                    max: s.max,
                                    blocks: s.blocks as i64,
                                    block: s.block.clone(),
                                }),
                                _ => None,
                            })
                            .collect(),
                    });
                }
            }
            StoredMessage::Error {
                id,
                content,
                dismissed,
            } => {
                if !dismissed {
                    items.push(Item::Error {
                        text: content.clone(),
                        message_id: id.clone(),
                    });
                }
            }
            StoredMessage::Assistant {
                content,
                reasoning,
                notices,
                plan,
                tool_calls,
                ..
            } => {
                for (level, text) in notices {
                    items.push(Item::Notice {
                        level: level.clone(),
                        text: text.clone(),
                    });
                }
                if let Some(plan) = plan {
                    items.push(Item::Plan(plan.clone()));
                }
                if let Some(text) = reasoning {
                    items.push(Item::Reasoning {
                        text: text.clone(),
                        live: false,
                    });
                }
                if let Some(text) = content {
                    items.push(Item::Assistant {
                        text: text.clone(),
                        last: tool_calls.is_empty(),
                        live: false,
                    });
                }
                for call in tool_calls {
                    items.push(Item::Tool(ToolCall {
                        call_id: call.id.clone(),
                        tool: call.name.clone(),
                        summary: call.summary.clone(),
                        program: call.program.clone(),
                        status: ToolStatus::Ok,
                        args_raw: String::new(),
                        output: None,
                        detail: String::new(),
                        diff: None,
                        progress: None,
                    }));
                }
            }
            StoredMessage::Tool {
                tool_call_id,
                shown,
            } => {
                let found = items.iter_mut().rev().find_map(|item| match item {
                    Item::Tool(call) if &call.call_id == tool_call_id => Some(call),
                    _ => None,
                });
                if let Some(call) = found {
                    finish(call, shown);
                }
            }
        }
    }
    items
}

/// A call's end from its shown result (`tool_result` and a stored `tool`
/// message alike).
fn finish(call: &mut ToolCall, shown: &Shown) {
    call.status = if shown.cancelled {
        ToolStatus::Cancelled
    } else if shown.ok {
        ToolStatus::Ok
    } else {
        ToolStatus::Failed
    };
    call.diff = if shown.ok { shown.diff } else { None };
    if let Some(output) = &shown.output {
        call.output = Some(output.clone());
    }
    call.detail = if shown.ok {
        String::new()
    } else if let Some(error) = &shown.error {
        error.clone()
    } else if shown.output.as_deref().is_some_and(|o| !o.is_empty()) {
        String::new()
    } else {
        "failed".to_string()
    };
}

/// Applies one live event to the items (`applyEvent`). An event that names
/// a tool call no earlier event made is a bug, and panics.
pub fn apply_event(items: &mut Vec<Item>, event: &TurnEvent) {
    match event {
        TurnEvent::CompletionRetry { .. } => items.retain(|item| match item {
            Item::Assistant { live, .. } | Item::Reasoning { live, .. } => !live,
            Item::Tool(call) => call.status != ToolStatus::Writing,
            _ => true,
        }),
        TurnEvent::AssistantDelta(text) => append_delta(items, false, text),
        TurnEvent::ReasoningDelta(text) => append_delta(items, true, text),
        TurnEvent::Assistant { text, last } => finalize(items, false, text, *last),
        TurnEvent::Reasoning(text) => finalize(items, true, text, false),
        TurnEvent::ToolCallDelta {
            index,
            id,
            name,
            arguments,
        } => {
            let id = id
                .as_deref()
                .unwrap_or_else(|| panic!("Tool call {index} streamed without its id"));
            let found = items.iter_mut().find_map(|item| match item {
                Item::Tool(call) if call.status == ToolStatus::Writing && call.call_id == id => {
                    Some(call)
                }
                _ => None,
            });
            match found {
                Some(call) => merge_tool_call_delta(call, name.as_deref(), arguments),
                None => {
                    let mut call = ToolCall {
                        call_id: id.to_string(),
                        tool: name.clone().unwrap_or_else(|| "build".to_string()),
                        summary: String::new(),
                        program: None,
                        status: ToolStatus::Writing,
                        args_raw: String::new(),
                        output: None,
                        detail: String::new(),
                        diff: None,
                        progress: None,
                    };
                    merge_tool_call_delta(&mut call, name.as_deref(), arguments);
                    items.push(Item::Tool(call));
                }
            }
        }
        TurnEvent::ToolStart {
            call_id,
            name,
            shown,
        } => {
            let call = find_call(items, call_id, ToolStatus::Writing)
                .unwrap_or_else(|| panic!("Tool call {call_id} started without being written"));
            call.status = ToolStatus::Running;
            call.summary = shown.summary.clone();
            call.tool = name.clone();
            if let Some(program) = program_of(shown) {
                call.program = Some(program);
            }
        }
        TurnEvent::ToolProgress {
            call_id, progress, ..
        } => {
            if let Some(call) = find_call(items, call_id, ToolStatus::Running) {
                call.progress = Some(progress_of(progress).message);
            }
        }
        TurnEvent::ToolResult { call_id, shown, .. } => {
            let call = find_call(items, call_id, ToolStatus::Running)
                .unwrap_or_else(|| panic!("Tool call {call_id} finished without running"));
            let shown = shown_from(shown);
            finish(call, &shown);
            if !shown.images.is_empty() {
                items.push(Item::Shots {
                    images: shown.images,
                });
            }
        }
        TurnEvent::Error {
            message,
            message_id,
        } => items.push(Item::Error {
            text: message.clone(),
            message_id: message_id.clone(),
        }),
        TurnEvent::Notice { level, text } => items.push(Item::Notice {
            level: level.clone(),
            text: text.clone(),
        }),
        TurnEvent::Plan(plan) => {
            let last_user = items
                .iter()
                .rposition(|item| matches!(item, Item::User { .. }))
                .map_or(0, |i| i + 1);
            match items[last_user..]
                .iter()
                .position(|item| matches!(item, Item::Plan(_)))
            {
                Some(at) => items[last_user + at] = Item::Plan(plan.clone()),
                None => items.push(Item::Plan(plan.clone())),
            }
        }
        // The status line, the world and the turn's end are the screen's,
        // not the items'.
        TurnEvent::HarnessStatus(_)
        | TurnEvent::WorldChanged
        | TurnEvent::Snapshot
        | TurnEvent::Done => {}
    }
}

fn find_call<'a>(items: &'a mut [Item], id: &str, status: ToolStatus) -> Option<&'a mut ToolCall> {
    items.iter_mut().find_map(|item| match item {
        Item::Tool(call) if call.status == status && call.call_id == id => Some(call),
        _ => None,
    })
}

fn append_delta(items: &mut Vec<Item>, reasoning: bool, delta: &str) {
    match items.last_mut() {
        Some(Item::Reasoning { text, live: true }) if reasoning => text.push_str(delta),
        Some(Item::Assistant {
            text, live: true, ..
        }) if !reasoning => text.push_str(delta),
        _ => items.push(if reasoning {
            Item::Reasoning {
                text: delta.to_string(),
                live: true,
            }
        } else {
            Item::Assistant {
                text: delta.to_string(),
                last: false,
                live: true,
            }
        }),
    }
}

fn finalize(items: &mut Vec<Item>, reasoning: bool, whole: &str, is_last: bool) {
    for item in items.iter_mut().rev() {
        match item {
            Item::Reasoning { text, live } if reasoning && *live => {
                *text = whole.to_string();
                *live = false;
                return;
            }
            Item::Assistant { text, last, live } if !reasoning && *live => {
                *text = whole.to_string();
                *last = is_last;
                *live = false;
                return;
            }
            _ => {}
        }
    }
    if !whole.is_empty() {
        items.push(if reasoning {
            Item::Reasoning {
                text: whole.to_string(),
                live: false,
            }
        } else {
            Item::Assistant {
                text: whole.to_string(),
                last: is_last,
                live: false,
            }
        });
    }
}

/// `mergeToolCallDelta`: the call's last decoded fields are kept on it.
fn merge_tool_call_delta(call: &mut ToolCall, name: Option<&str>, arguments: &str) {
    call.args_raw.push_str(arguments);
    if let Some(name) = name {
        call.tool = name.to_string();
    }
    let (summary, program) = parse_tool_args(&call.args_raw);
    if let Some(summary) = summary {
        call.summary = summary;
    }
    if program.is_some() {
        call.program = program;
    }
}

/// `parseToolArgs`: a call's summary and program from its arguments, whole
/// or still streaming.
pub fn parse_tool_args(raw: &str) -> (Option<String>, Option<String>) {
    if raw.is_empty() {
        return (None, None);
    }
    match serde_json::from_str::<Value>(raw) {
        Ok(value) => {
            let args = unwrap_arguments(value);
            let text = |key: &str| args[key].as_str().map(str::to_string);
            (text("summary"), text("program").or_else(|| text("command")))
        }
        Err(_) => (
            extract_string(raw, "summary"),
            extract_string(raw, "program").or_else(|| extract_string(raw, "command")),
        ),
    }
}

fn unwrap_arguments(mut args: Value) -> Value {
    for _ in 0..3 {
        let nested = if args["arguments"].is_null() {
            args["function"]["arguments"].clone()
        } else {
            args["arguments"].clone()
        };
        match nested {
            Value::String(text) => match serde_json::from_str(&text) {
                Ok(parsed) => args = parsed,
                Err(_) => break,
            },
            Value::Object(_) => args = nested,
            _ => break,
        }
    }
    args
}

fn string_end(raw: &[char], start: usize) -> Option<usize> {
    let mut index = start + 1;
    while index < raw.len() {
        match raw[index] {
            '\\' => index += 1,
            '"' => return Some(index),
            _ => {}
        }
        index += 1;
    }
    None
}

/// A top-level string field of a JSON object still streaming in.
fn extract_string(raw: &str, key: &str) -> Option<String> {
    let chars: Vec<char> = raw.chars().collect();
    let mut depth = 0i32;
    let mut index = 0;
    while index < chars.len() {
        let c = chars[index];
        if c == '{' || c == '[' {
            depth += 1;
        } else if c == '}' || c == ']' {
            depth -= 1;
        }
        if c != '"' {
            index += 1;
            continue;
        }
        let end = string_end(&chars, index)?;
        let after: String = chars[end + 1..].iter().collect();
        let trimmed = after.trim_start();
        let is_value = trimmed.starts_with(':') && trimmed[1..].trim_start().starts_with('"');
        let name: String = chars[index + 1..end].iter().collect();
        if depth == 1 && is_value && name == key {
            let colon = trimmed[1..].trim_start();
            return Some(streamed_string(&colon[1..]));
        }
        index = end + 1;
    }
    None
}

fn streamed_string(rest: &str) -> String {
    let with_quote: Vec<char> = std::iter::once('"').chain(rest.chars()).collect();
    let mut body: String = match string_end(&with_quote, 0) {
        Some(end) => with_quote[1..end].iter().collect(),
        None => {
            let mut body = rest.to_string();
            let trailing = body.chars().rev().take_while(|c| *c == '\\').count();
            if trailing % 2 == 1 {
                body.pop();
            }
            if let Some(at) = body.rfind("\\u") {
                let tail = &body[at + 2..];
                if tail.len() < 4 && tail.chars().all(|c| c.is_ascii_hexdigit()) {
                    body.truncate(at);
                }
            }
            body
        }
    };
    body = unescape_json(&body);
    body
}

fn unescape_json(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('u') => {
                let hex: String = chars.by_ref().take(4).collect();
                match u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                    Some(decoded) => out.push(decoded),
                    None => out.push_str(&hex),
                }
            }
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('b') => out.push('\u{8}'),
            Some('f') => out.push('\u{c}'),
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

/// One call of a live tool stack with the pictures it returned.
#[derive(Clone, Debug, PartialEq)]
pub struct StackedCall {
    /// The call.
    pub call: ToolCall,
    /// Its pictures.
    pub outputs: Vec<Item>,
}

/// A row of the transcript (`groupItems`' entries). An item is larger than
/// a list of them; entries are few (one per row) and built once per change.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq)]
pub enum Entry {
    /// A shown item: user, assistant, reasoning, error, notice, plan, shots.
    Item(Item),
    /// A settled turn's work, folded ("Worked · N steps").
    Activity(Vec<Item>),
    /// A running turn's uninterrupted tool calls, the latest shown.
    ToolStack(Vec<StackedCall>),
}

impl Entry {
    /// `entryKeys`' kind.
    fn kind(&self) -> &'static str {
        match self {
            Entry::Item(Item::User { .. }) => "user",
            Entry::Item(Item::Assistant { .. }) => "assistant",
            Entry::Item(Item::Reasoning { .. }) => "reasoning",
            Entry::Item(Item::Tool(_)) => "tool",
            Entry::Item(Item::Shots { .. }) => "shots",
            Entry::Item(Item::Error { .. }) => "error",
            Entry::Item(Item::Notice { .. }) => "notice",
            Entry::Item(Item::Plan(_)) => "plan",
            Entry::Activity(_) => "activity",
            Entry::ToolStack(_) => "tool-stack",
        }
    }
}

/// `groupItems`: while the last turn runs (`busy`), spoken text stays
/// visible and uninterrupted tool calls stack; once settled, all work but
/// the final answer folds into one activity entry.
pub fn group_items(items: &[Item], busy: bool) -> Vec<Entry> {
    enum Raw<'a> {
        One(&'a Item),
        Activity(Vec<&'a Item>),
    }
    let mut grouped: Vec<Raw> = Vec::new();
    for item in items {
        if item.is_activity() {
            if let Some(Raw::Activity(children)) = grouped.last_mut() {
                children.push(item);
            } else {
                grouped.push(Raw::Activity(vec![item]));
            }
        } else {
            grouped.push(Raw::One(item));
        }
    }
    let count = grouped.len();
    let mut entries = Vec::new();
    for (index, raw) in grouped.into_iter().enumerate() {
        match raw {
            Raw::One(item) => entries.push(Entry::Item(item.clone())),
            Raw::Activity(children) => {
                let last_plan = children.iter().rposition(|i| matches!(i, Item::Plan(_)));
                let children: Vec<Item> = children
                    .into_iter()
                    .enumerate()
                    .filter(|(i, item)| !matches!(item, Item::Plan(_)) || Some(*i) == last_plan)
                    .map(|(_, item)| item.clone())
                    .collect();
                let has_final = children
                    .iter()
                    .any(|i| matches!(i, Item::Assistant { last: true, .. }));
                if busy && index == count - 1 && !has_final {
                    live_entries(children, &mut entries);
                } else {
                    settled_entries(children, &mut entries);
                }
            }
        }
    }
    entries
}

fn live_entries(children: Vec<Item>, entries: &mut Vec<Entry>) {
    let mut calls: Option<Vec<StackedCall>> = None;
    for item in children {
        match item {
            Item::Tool(call) => calls.get_or_insert_with(Vec::new).push(StackedCall {
                call,
                outputs: Vec::new(),
            }),
            Item::Shots { .. } if calls.as_ref().is_some_and(|c| !c.is_empty()) => calls
                .as_mut()
                .and_then(|c| c.last_mut())
                .expect("a stack with a call")
                .outputs
                .push(item),
            other => {
                if let Some(stack) = calls.take() {
                    entries.push(Entry::ToolStack(stack));
                }
                entries.push(Entry::Item(other));
            }
        }
    }
    if let Some(stack) = calls {
        entries.push(Entry::ToolStack(stack));
    }
}

fn settled_entries(children: Vec<Item>, entries: &mut Vec<Entry>) {
    let final_index = children
        .iter()
        .rposition(|i| matches!(i, Item::Assistant { last: true, .. }));
    let surfaced = |item: &Item| match item {
        Item::Plan(_) => true,
        Item::Notice { level, .. } => level != "info",
        _ => false,
    };
    let mut work = Vec::new();
    let mut shown = Vec::new();
    let mut last = None;
    for (index, item) in children.into_iter().enumerate() {
        if Some(index) == final_index {
            last = Some(item);
        } else if surfaced(&item) {
            shown.push(item);
        } else {
            work.push(item);
        }
    }
    if !work.is_empty() {
        entries.push(Entry::Activity(work));
    }
    entries.extend(shown.into_iter().map(Entry::Item));
    if let Some(last) = last {
        entries.push(Entry::Item(last));
    }
}

/// `entryKeys`: a key per entry, stable while the entries around it change,
/// so a fold keeps its open state.
pub fn entry_keys(entries: &[Entry]) -> Vec<String> {
    let mut turn = 0;
    let mut ordinal = 0;
    entries
        .iter()
        .map(|entry| {
            if entry.kind() == "user" {
                turn += 1;
                ordinal = 0;
                format!("{turn}:user")
            } else {
                ordinal += 1;
                format!("{turn}:{ordinal}:{}", entry.kind())
            }
        })
        .collect()
}

// par-chat (stage four's parity pass): `renderLogItem`'s patterns, case for
// case. JavaScript's `\s` and `trim()` are its own space set (no U+0085,
// with U+FEFF), its `.` stops at the four line ends, `\d` is ASCII and the
// `i` flag folds no non-ASCII letter onto an ASCII one (no `ſ` for `s`, no
// Kelvin sign for `k`): each is spelled out below.
/// JavaScript's `\s`.
const JS_SPACE: &str = r"[\t\n\x0B\x0C\r \u{A0}\u{1680}\u{2000}-\u{200A}\u{2028}\u{2029}\u{202F}\u{205F}\u{3000}\u{FEFF}]";

fn js_pattern(pattern: &str) -> regex::Regex {
    let pattern = pattern
        .replace(r"\s", JS_SPACE)
        .replace("(.*)", r"([^\n\r\u{2028}\u{2029}]*)");
    regex::Regex::new(&pattern).unwrap_or_else(|e| panic!("the tool row pattern {pattern}: {e}"))
}

/// The tool row's patterns (`web/app.js` 5982-6002).
struct Patterns {
    /// `/^\$\s*/`.
    bash: regex::Regex,
    /// `/^(?:build|building|built)\s+/i`.
    build: regex::Regex,
    /// `/^(?:inspect|check|verify|dump|read|examine)\s+(.*)$/i`.
    inspection: regex::Regex,
    /// `/^Look at build\s*/i`.
    see_build: regex::Regex,
    /// `/\((\d+) tests\)/`.
    tests: regex::Regex,
    /// `/^Look at\s*/i`.
    view_image: regex::Regex,
}

static PATTERNS: std::sync::LazyLock<Patterns> = std::sync::LazyLock::new(|| Patterns {
    bash: js_pattern(r"^\$\s*"),
    build: js_pattern(r"^(?i-u:build|building|built)\s+"),
    inspection: js_pattern(r"^(?i-u:inspect|check|verify|dump|read|examine)\s+(.*)$"),
    see_build: js_pattern(r"^(?i-u:Look at build)\s*"),
    tests: js_pattern(r"\(([0-9]+) tests\)"),
    view_image: js_pattern(r"^(?i-u:Look at)\s*"),
});

/// JavaScript's `String.prototype.trim()` is empty.
fn js_blank(text: &str) -> bool {
    text.chars().all(|c| {
        matches!(
            c,
            '\t' | '\n' | '\u{0B}' | '\u{0C}' | '\r' | ' ' | '\u{A0}' | '\u{1680}' | '\u{2000}'
                ..='\u{200A}'
                    | '\u{2028}'
                    | '\u{2029}'
                    | '\u{202F}'
                    | '\u{205F}'
                    | '\u{3000}'
                    | '\u{FEFF}'
        )
    })
}

/// How a tool row reads (`renderLogItem`): its action and its detail.
pub fn tool_row_text(call: &ToolCall) -> (String, String) {
    let active = matches!(call.status, ToolStatus::Writing | ToolStatus::Running);
    let failed = call.status == ToolStatus::Failed;
    // `item.summary || item.tool || '…'`.
    let summary = if !call.summary.is_empty() {
        call.summary.clone()
    } else if !call.tool.is_empty() {
        call.tool.clone()
    } else {
        "\u{2026}".to_string()
    };
    let p = &*PATTERNS;
    let replace = |pattern: &regex::Regex, text: &str| pattern.replacen(text, 1, "").into_owned();
    let (action, detail) = match call.tool.as_str() {
        "bash" => (
            if active {
                "Running command"
            } else if failed {
                "Command failed"
            } else {
                "Ran command"
            }
            .to_string(),
            replace(&p.bash, &summary),
        ),
        "build" => {
            let detail = replace(&p.build, &summary);
            match p.inspection.captures(&detail) {
                Some(inspection) => (
                    if active {
                        "Inspecting"
                    } else if failed {
                        "Inspection failed"
                    } else {
                        "Inspected"
                    }
                    .to_string(),
                    inspection[1].to_string(),
                ),
                None => (
                    if active {
                        "Building"
                    } else if failed {
                        "Build failed"
                    } else {
                        "Built"
                    }
                    .to_string(),
                    detail,
                ),
            }
        }
        "see_build" => (
            if active {
                "Inspecting build"
            } else if failed {
                "Inspection failed"
            } else {
                "Inspected build"
            }
            .to_string(),
            replace(&p.see_build, &summary),
        ),
        "test_build" => {
            let tests = p.tests.captures(&summary).map(|c| c[1].to_string());
            (
                if active {
                    "Testing in Minecraft"
                } else if call.status == ToolStatus::Cancelled {
                    "Minecraft test cancelled"
                } else if failed {
                    "Minecraft test failed"
                } else {
                    "Minecraft test completed"
                }
                .to_string(),
                // `active && item.progress ? item.progress : …`: an empty
                // progress is falsy.
                match (call.progress.as_deref().filter(|p| !p.is_empty()), tests) {
                    (Some(progress), _) if active => progress.to_string(),
                    (_, Some(n)) => format!("{n} tests: input and block-state observations"),
                    _ => "Input and block-state observations".to_string(),
                },
            )
        }
        "view_image" => (
            if active {
                "Viewing image"
            } else if failed {
                "Image view failed"
            } else {
                "Viewed image"
            }
            .to_string(),
            replace(&p.view_image, &summary),
        ),
        "" => ("Tool".to_string(), summary),
        other => (other.to_string(), summary),
    };
    let detail = if detail == action || js_blank(&detail) {
        String::new()
    } else {
        detail
    };
    (action, detail)
}

/// The `+<n> lines` a call being written shows (`writing && program`):
/// the program's lines as `program.split('\n').length` counts them, none
/// for an empty or absent program or a call no longer being written.
pub fn writing_lines(call: &ToolCall) -> Option<usize> {
    let program = call.program.as_deref().filter(|p| !p.is_empty())?;
    (call.status == ToolStatus::Writing).then(|| program.split('\n').count())
}
// end of par-chat's block

/// `DiffStat`: `+added+changed` and `-removed+changed`, each shown when not zero.
pub fn diff_stat(diff: &Diff) -> (u64, u64) {
    (diff.added + diff.changed, diff.removed + diff.changed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mcbench_agent::Diff as ShownDiff;
    use mcbench_agent::{CallShown, ToolShown};
    use serde_json::json;

    fn delta(id: &str, args: &str) -> TurnEvent {
        TurnEvent::ToolCallDelta {
            index: 0,
            id: Some(id.into()),
            name: Some("build".into()),
            arguments: args.into(),
        }
    }

    #[test]
    fn deltas_stream_into_a_live_item_and_the_whole_event_seals_it() {
        let mut items = Vec::new();
        apply_event(&mut items, &TurnEvent::AssistantDelta("Hel".into()));
        apply_event(&mut items, &TurnEvent::AssistantDelta("lo".into()));
        assert_eq!(
            items,
            vec![Item::Assistant {
                text: "Hello".into(),
                last: false,
                live: true
            }]
        );
        apply_event(
            &mut items,
            &TurnEvent::Assistant {
                text: "Hello.".into(),
                last: true,
            },
        );
        assert_eq!(
            items,
            vec![Item::Assistant {
                text: "Hello.".into(),
                last: true,
                live: false
            }]
        );
    }

    #[test]
    fn a_streaming_call_shows_its_program_before_its_json_closes() {
        let mut items = Vec::new();
        apply_event(
            &mut items,
            &delta("c1", r#"{"summary":"Build a lamp","program":"place(0,"#),
        );
        apply_event(&mut items, &delta("c1", r#"0,0)\nlight("#));
        let Item::Tool(call) = &items[0] else {
            panic!("a tool item")
        };
        assert_eq!(call.summary, "Build a lamp");
        assert_eq!(call.program.as_deref(), Some("place(0,0,0)\nlight("));
        assert_eq!(call.status, ToolStatus::Writing);
    }

    #[test]
    fn a_call_runs_and_finishes_with_its_diff() {
        let mut items = Vec::new();
        apply_event(&mut items, &delta("c1", r#"{"program":"x"}"#));
        apply_event(
            &mut items,
            &TurnEvent::ToolStart {
                call_id: "c1".into(),
                name: "build".into(),
                shown: Box::new(CallShown {
                    summary: "Build a lamp".into(),
                    program: None,
                }),
            },
        );
        apply_event(
            &mut items,
            &TurnEvent::ToolResult {
                call_id: "c1".into(),
                name: "build".into(),
                shown: Box::new(ToolShown {
                    ok: true,
                    cancelled: false,
                    error: None,
                    diff: Some(ShownDiff {
                        added: 3.,
                        removed: 0.,
                        changed: 1.,
                    }),
                    output: None,
                    images: Vec::new(),
                }),
                result: json!({"ok": true, "diff": {"added": 3, "changed": 1}}),
                image_urls: None,
            },
        );
        let Item::Tool(call) = &items[0] else {
            panic!("a tool item")
        };
        assert_eq!(call.status, ToolStatus::Ok);
        assert_eq!(diff_stat(&call.diff.expect("a diff")), (4, 1));
        assert_eq!(tool_row_text(call), ("Built".into(), "a lamp".into()));
    }

    #[test]
    #[should_panic(expected = "started without being written")]
    fn a_start_for_an_unknown_call_is_a_bug() {
        apply_event(
            &mut Vec::new(),
            &TurnEvent::ToolStart {
                call_id: "x".into(),
                name: "build".into(),
                shown: Box::new(CallShown {
                    summary: String::new(),
                    program: None,
                }),
            },
        );
    }

    #[test]
    fn a_settled_turn_folds_its_work_and_keeps_the_final_answer() {
        let items = vec![
            Item::User {
                text: "hi".into(),
                message_id: None,
                files: Vec::new(),
                selections: Vec::new(),
            },
            Item::Reasoning {
                text: "think".into(),
                live: false,
            },
            Item::Tool(ToolCall {
                call_id: "a".into(),
                tool: "build".into(),
                summary: "Build x".into(),
                program: None,
                status: ToolStatus::Ok,
                args_raw: String::new(),
                output: None,
                detail: String::new(),
                diff: None,
                progress: None,
            }),
            Item::Assistant {
                text: "done".into(),
                last: true,
                live: false,
            },
        ];
        let entries = group_items(&items, false);
        assert_eq!(entries.len(), 3);
        assert!(matches!(&entries[1], Entry::Activity(work) if work.len() == 2));
        assert!(matches!(&entries[2], Entry::Item(Item::Assistant { .. })));
        assert_eq!(
            entry_keys(&entries),
            vec!["1:user", "1:1:activity", "1:2:assistant"]
        );
        let live = group_items(&items[..3], true);
        assert!(matches!(&live[1], Entry::Item(Item::Reasoning { .. })));
        assert!(matches!(&live[2], Entry::ToolStack(calls) if calls.len() == 1));
    }

    #[test]
    fn stored_messages_rebuild_their_items() {
        let at = "2026-11-17T10:00:00.000Z";
        let messages = [
            (
                "m1",
                json!({"role":"user","content":"Make a lamp","createdAt":at}),
            ),
            (
                "m2",
                json!({"role":"assistant","content":"","reasoning":"Plan","tool_calls":[{"id":"t1","type":"function","function":{"name":"bash","arguments":"{\"command\":\"ls -la\\nwc\"}"}}],"createdAt":at}),
            ),
            (
                "m3",
                json!({"role":"tool","tool_call_id":"t1","content":"total 0","createdAt":at}),
            ),
            (
                "m4",
                json!({"role":"assistant","content":"Done.","createdAt":at}),
            ),
        ];
        let stored: Vec<mcbench_store::StoredMessage> = messages
            .iter()
            .enumerate()
            .map(|(n, (id, payload))| {
                let parent = n.checked_sub(1).map(|p| messages[p].0);
                mcbench_store::StoredMessage::from_payload(
                    id,
                    parent,
                    n as u32,
                    payload.to_string(),
                )
                .expect("a stored message")
            })
            .collect();
        let items = items_from_messages(&super::super::seam::from_stored(&stored));
        assert_eq!(items.len(), 4);
        let Item::Tool(call) = &items[2] else {
            panic!("a tool item")
        };
        assert_eq!(call.summary, "$ ls -la");
        assert_eq!(call.output.as_deref(), Some("total 0"));
        assert_eq!(tool_row_text(call), ("Ran command".into(), "ls -la".into()));
        assert!(matches!(&items[3], Item::Assistant { last: true, .. }));
    }
}

#[cfg(test)]
mod page_cases {
    //! `test/transcript.test.js`, the grouping, key and event cases, case
    //! for case (the phase cases are `state::turn`'s tests).
    use super::*;
    use mcbench_agent::{CallShown, ToolShown};

    fn user(text: &str) -> Item {
        Item::User {
            text: text.into(),
            message_id: None,
            files: Vec::new(),
            selections: Vec::new(),
        }
    }

    fn tool(id: &str, summary: &str, status: ToolStatus) -> Item {
        Item::Tool(ToolCall {
            call_id: id.into(),
            tool: "build".into(),
            summary: summary.into(),
            program: None,
            status,
            args_raw: String::new(),
            output: None,
            detail: String::new(),
            diff: None,
            progress: None,
        })
    }

    fn reasoning(text: &str, live: bool) -> Item {
        Item::Reasoning {
            text: text.into(),
            live,
        }
    }

    fn assistant(text: &str, last: bool) -> Item {
        Item::Assistant {
            text: text.into(),
            last,
            live: false,
        }
    }

    fn kinds(entries: &[Entry]) -> Vec<&'static str> {
        entries.iter().map(Entry::kind).collect()
    }

    fn delta(index: u32, id: &str, name: Option<&str>, args: &str) -> TurnEvent {
        TurnEvent::ToolCallDelta {
            index,
            id: Some(id.into()),
            name: name.map(str::to_string),
            arguments: args.into(),
        }
    }

    fn start(id: &str, name: &str, summary: &str) -> TurnEvent {
        TurnEvent::ToolStart {
            call_id: id.into(),
            name: name.into(),
            shown: Box::new(CallShown {
                summary: summary.into(),
                program: None,
            }),
        }
    }

    fn result(
        id: &str,
        name: &str,
        output: Option<&str>,
        images: Vec<std::path::PathBuf>,
    ) -> TurnEvent {
        TurnEvent::ToolResult {
            call_id: id.into(),
            name: name.into(),
            shown: Box::new(ToolShown {
                ok: true,
                cancelled: false,
                error: None,
                diff: None,
                output: output.map(str::to_string),
                images,
            }),
            result: serde_json::json!({ "ok": true }),
            image_urls: None,
        }
    }

    #[test]
    fn live_turns_expose_speech_and_reasoning_while_compacting_consecutive_tools() {
        let mut items = vec![
            user("Build a house"),
            reasoning("Plan the shell", false),
            assistant("Okay, I will build that house for you!", false),
        ];
        for index in 0..5 {
            items.push(tool(
                &format!("call-{}", index + 1),
                &format!("tool {}", index + 1),
                if index == 4 {
                    ToolStatus::Running
                } else {
                    ToolStatus::Ok
                },
            ));
        }
        items.push(reasoning("Inspect the result", true));
        let entries = group_items(&items, true);
        assert_eq!(
            kinds(&entries),
            ["user", "reasoning", "assistant", "tool-stack", "reasoning"]
        );
        assert!(
            matches!(&entries[2], Entry::Item(Item::Assistant { text, .. }) if text == "Okay, I will build that house for you!")
        );
        let Entry::ToolStack(calls) = &entries[3] else {
            panic!("a stack")
        };
        assert_eq!(calls.len(), 5);
        assert_eq!(calls.last().expect("a call").call.summary, "tool 5");
    }

    #[test]
    fn spoken_assistant_content_breaks_one_tool_stack_from_the_next() {
        let entries = group_items(
            &[
                user("Build"),
                tool("a", "first", ToolStatus::Ok),
                tool("b", "second", ToolStatus::Ok),
                assistant("The shell is ready.", false),
                tool("c", "third", ToolStatus::Ok),
            ],
            true,
        );
        assert_eq!(
            kinds(&entries),
            ["user", "tool-stack", "assistant", "tool-stack"]
        );
        assert!(matches!(&entries[1], Entry::ToolStack(c) if c.len() == 2));
        assert!(matches!(&entries[3], Entry::ToolStack(c) if c.len() == 1));
    }

    #[test]
    fn live_screenshot_output_stays_attached_to_its_latest_tool() {
        let shot = Item::Shots {
            images: vec!["/one.jpg".into()],
        };
        let entries = group_items(
            &[
                user("Inspect it"),
                tool("a", "see build", ToolStatus::Ok),
                shot.clone(),
                tool("b", "fix roof", ToolStatus::Ok),
            ],
            true,
        );
        let Entry::ToolStack(calls) = &entries[1] else {
            panic!("a stack")
        };
        assert_eq!(calls[0].outputs, vec![shot]);
        assert!(calls[1].outputs.is_empty());
    }

    #[test]
    fn settled_turns_fold_all_work_and_expose_only_the_explicit_final_response() {
        let entries = group_items(
            &[
                user("Build"),
                reasoning("Plan", false),
                assistant("I am starting the shell.", false),
                tool("a", "build shell", ToolStatus::Ok),
                reasoning("Check the roof", false),
                assistant("The build is complete!", true),
            ],
            false,
        );
        assert_eq!(kinds(&entries), ["user", "activity", "assistant"]);
        assert!(matches!(&entries[1], Entry::Activity(c) if c.len() == 4));
        assert!(
            matches!(&entries[2], Entry::Item(Item::Assistant { text, .. }) if text == "The build is complete!")
        );
    }

    #[test]
    fn a_settled_direct_reply_has_no_empty_activity_dropdown() {
        let items = [user("Hello"), assistant("Hello!", true)];
        assert_eq!(
            group_items(&items, false),
            vec![Entry::Item(items[0].clone()), Entry::Item(items[1].clone())]
        );
    }

    #[test]
    fn a_turn_begun_before_its_prompt_shows_leaves_the_previous_turn_settled() {
        let items = [
            user("Build"),
            reasoning("Plan", false),
            tool("a", "build shell", ToolStatus::Ok),
            assistant("Done!", true),
        ];
        assert_eq!(
            kinds(&group_items(&items, true)),
            kinds(&group_items(&items, false))
        );
    }

    #[test]
    fn entry_keys_hold_while_a_turn_grows_and_change_when_it_settles() {
        let first = vec![user("a"), reasoning("r", false), assistant("ok", true)];
        let mut live = first.clone();
        live.extend([user("b"), reasoning("r2", false)]);
        let mut grown = live.clone();
        grown.push(tool("c", "t", ToolStatus::Ok));
        let keys = entry_keys(&group_items(&live, true));
        let grown_keys = entry_keys(&group_items(&grown, true));
        assert_eq!(grown_keys[..keys.len()], keys[..]);
        assert_eq!(keys[..3], ["1:user", "1:1:activity", "1:2:assistant"]);
        let mut settled_items = grown.clone();
        settled_items.push(assistant("done", true));
        let settled = entry_keys(&group_items(&settled_items, false));
        assert_eq!(settled[4], "2:1:activity");
        assert_ne!(keys[4], settled[4]);
    }

    #[test]
    fn a_failed_transcript_read_carries_why_and_only_it_does() {
        assert_ne!(ReadState::Loading, ReadState::Shown);
        assert_eq!(
            ReadState::Failed("HTTP 500".into()),
            ReadState::Failed("HTTP 500".into())
        );
    }

    #[test]
    fn tool_events_land_on_the_call_they_name() {
        let mut items = vec![user("Build")];
        for event in [
            delta(0, "a", Some("build"), r#"{"summary":"one""#),
            delta(1, "b", Some("bash"), r#"{"command":"ls""#),
            delta(0, "a", None, "}"),
            start("b", "bash", "$ ls"),
        ] {
            apply_event(&mut items, &event);
        }
        let states: Vec<(String, ToolStatus)> = items[1..]
            .iter()
            .map(|i| match i {
                Item::Tool(c) => (c.call_id.clone(), c.status),
                other => panic!("a tool, not {other:?}"),
            })
            .collect();
        assert_eq!(
            states,
            [
                ("a".into(), ToolStatus::Writing),
                ("b".into(), ToolStatus::Running)
            ]
        );
        apply_event(&mut items, &result("b", "bash", Some("x"), Vec::new()));
        let Item::Tool(b) = &items[2] else {
            panic!("a tool")
        };
        assert_eq!((b.status, b.output.as_deref()), (ToolStatus::Ok, Some("x")));
        let mut again = items.clone();
        assert!(
            std::panic::catch_unwind(move || apply_event(
                &mut again,
                &result("a", "build", None, Vec::new())
            ))
            .is_err()
        );
        let mut again = items.clone();
        assert!(
            std::panic::catch_unwind(move || apply_event(&mut again, &start("z", "build", "s")))
                .is_err()
        );
        // Another response's call of the same number is another call.
        apply_event(&mut items, &delta(0, "c", Some("build"), "{"));
        let ids: Vec<&str> = items[1..]
            .iter()
            .map(|i| match i {
                Item::Tool(c) => c.call_id.as_str(),
                _ => "",
            })
            .collect();
        assert_eq!(ids, ["a", "b", "c"]);
    }

    #[test]
    fn a_call_written_in_parts_with_another_calls_result_between_them_is_one_call() {
        let mut items = vec![user("Build")];
        for event in [
            delta(0, "a", Some("see_build"), "{}"),
            delta(1, "b", Some("build"), r#"{"summary":"ro"#),
            start("a", "see_build", "Look at build"),
            result("a", "see_build", None, vec!["/shot.png".into()]),
            // A harness sends the rest of b's text once the next completion waits.
            delta(1, "b", Some("build"), r#"of"}"#),
            start("b", "build", "roof"),
        ] {
            apply_event(&mut items, &event);
        }
        let shape: Vec<(&str, Option<ToolStatus>)> = items[1..]
            .iter()
            .map(|i| match i {
                Item::Tool(c) => (c.call_id.as_str(), Some(c.status)),
                Item::Shots { .. } => ("shots", None),
                _ => ("other", None),
            })
            .collect();
        assert_eq!(
            shape,
            [
                ("a", Some(ToolStatus::Ok)),
                ("b", Some(ToolStatus::Running)),
                ("shots", None)
            ]
        );
        let Item::Tool(b) = &items[2] else {
            panic!("a tool")
        };
        assert_eq!(b.args_raw, r#"{"summary":"roof"}"#);
        let event = TurnEvent::ToolCallDelta {
            index: 0,
            id: None,
            name: Some("build".into()),
            arguments: "{".into(),
        };
        assert!(std::panic::catch_unwind(move || apply_event(&mut items, &event)).is_err());
    }

    #[test]
    fn deltas_stream_into_one_live_item_that_the_full_event_seals() {
        let mut items = Vec::new();
        apply_event(&mut items, &TurnEvent::AssistantDelta("Hel".into()));
        apply_event(&mut items, &TurnEvent::AssistantDelta("lo".into()));
        assert_eq!(
            items,
            vec![Item::Assistant {
                text: "Hello".into(),
                last: false,
                live: true
            }]
        );
        apply_event(
            &mut items,
            &TurnEvent::Assistant {
                text: "Hello!".into(),
                last: true,
            },
        );
        assert_eq!(
            items,
            vec![Item::Assistant {
                text: "Hello!".into(),
                last: true,
                live: false
            }]
        );
        items.push(reasoning("r", true));
        apply_event(
            &mut items,
            &TurnEvent::CompletionRetry {
                attempt: 1,
                delay_ms: 0,
                error: "overloaded".into(),
            },
        );
        assert_eq!(items.len(), 1);
        assert!(matches!(items[0], Item::Assistant { .. }));
    }

    #[test]
    fn an_error_dismissed_in_the_page_is_dismissed_in_the_fetched_transcript() {
        let messages = [
            StoredMessage::User {
                id: Some("1".into()),
                parts: vec![UserPart::Text("Build".into())],
                see_build: false,
            },
            StoredMessage::Error {
                id: Some("2".into()),
                content: "Rate limited".into(),
                dismissed: true,
            },
            StoredMessage::Error {
                id: Some("3".into()),
                content: "Out of credits".into(),
                dismissed: false,
            },
        ];
        let items = items_from_messages(&messages);
        let errors: Vec<&str> = items
            .iter()
            .filter_map(|i| match i {
                Item::Error { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(errors, ["Out of credits"]);
    }

    // par-chat (stage four's parity pass): `renderLogItem`'s rows, case
    // for case against the page's patterns (the page has no test of its
    // own for them).
    fn call(tool: &str, summary: &str, status: ToolStatus) -> ToolCall {
        ToolCall {
            call_id: "c".into(),
            tool: tool.into(),
            summary: summary.into(),
            program: None,
            status,
            args_raw: String::new(),
            output: None,
            detail: String::new(),
            diff: None,
            progress: None,
        }
    }

    fn row(tool: &str, summary: &str) -> (String, String) {
        tool_row_text(&call(tool, summary, ToolStatus::Ok))
    }

    #[test]
    fn a_bash_row_strips_one_dollar_and_the_spaces_after_it() {
        assert_eq!(row("bash", "$ ls"), ("Ran command".into(), "ls".into()));
        assert_eq!(row("bash", "$$ ls"), ("Ran command".into(), "$ ls".into()));
        assert_eq!(row("bash", "$\tls"), ("Ran command".into(), "ls".into()));
        assert_eq!(
            row("bash", "echo $HOME"),
            ("Ran command".into(), "echo $HOME".into())
        );
        assert_eq!(row("bash", "$"), ("Ran command".into(), String::new()));
        let running = tool_row_text(&call("bash", "$ ls", ToolStatus::Running));
        assert_eq!(running.0, "Running command");
        let failed = tool_row_text(&call("bash", "$ ls", ToolStatus::Failed));
        assert_eq!(failed.0, "Command failed");
    }

    #[test]
    fn a_build_row_reads_its_verb_and_an_inspection_by_javascripts_whitespace() {
        assert_eq!(
            row("build", "build a lamp"),
            ("Built".into(), "a lamp".into())
        );
        assert_eq!(
            row("build", "Building  a lamp"),
            ("Built".into(), "a lamp".into())
        );
        assert_eq!(
            row("build", "built\ta lamp"),
            ("Built".into(), "a lamp".into())
        );
        assert_eq!(
            row("build", "buildings"),
            ("Built".into(), "buildings".into())
        );
        assert_eq!(row("build", "inspect  x"), ("Inspected".into(), "x".into()));
        assert_eq!(row("build", "inspect\tx"), ("Inspected".into(), "x".into()));
        assert_eq!(
            row("build", "build CHECK the roof"),
            ("Inspected".into(), "the roof".into())
        );
        // `.` stops at a line end, so a second line is a build.
        assert_eq!(
            row("build", "inspect x\ny"),
            ("Built".into(), "inspect x\ny".into())
        );
        // The `i` flag folds no non-ASCII letter onto an ASCII one.
        assert_eq!(
            row("build", "inſpect x"),
            ("Built".into(), "inſpect x".into())
        );
        // U+0085 is not JavaScript's `\s`; U+FEFF is.
        assert_eq!(
            row("build", "inspect\u{85}x"),
            ("Built".into(), "inspect\u{85}x".into())
        );
        assert_eq!(
            row("build", "inspect\u{FEFF}x"),
            ("Inspected".into(), "x".into())
        );
        // An inspection with nothing after its spaces reads no detail.
        assert_eq!(
            row("build", "inspect  "),
            ("Inspected".into(), String::new())
        );
    }

    #[test]
    fn a_test_build_row_takes_the_first_count_in_brackets_anywhere() {
        let done = "Minecraft test completed";
        assert_eq!(
            row("test_build", "Ran (a) (3 tests)"),
            (
                done.into(),
                "3 tests: input and block-state observations".into()
            )
        );
        assert_eq!(
            row("test_build", "(x tests)"),
            (done.into(), "Input and block-state observations".into())
        );
        let mut running = call("test_build", "(3 tests)", ToolStatus::Running);
        running.progress = Some(String::new());
        assert_eq!(
            tool_row_text(&running),
            (
                "Testing in Minecraft".into(),
                "3 tests: input and block-state observations".into()
            )
        );
        running.progress = Some("2 of 3".into());
        assert_eq!(tool_row_text(&running).1, "2 of 3");
    }

    #[test]
    fn look_rows_strip_their_lead_and_a_detail_equal_to_the_action_is_dropped() {
        assert_eq!(
            row("see_build", "look at build(iso_se)"),
            ("Inspected build".into(), "(iso_se)".into())
        );
        assert_eq!(
            row("view_image", "Look at  a.png"),
            ("Viewed image".into(), "a.png".into())
        );
        assert_eq!(row("", ""), ("Tool".into(), "\u{2026}".into()));
        assert_eq!(row("place", ""), ("place".into(), String::new()));
        assert_eq!(row("place", "\u{3000}"), ("place".into(), String::new()));
    }

    #[test]
    fn a_call_being_written_counts_its_lines_as_split_does() {
        let mut writing = call("build", "", ToolStatus::Writing);
        assert_eq!(writing_lines(&writing), None);
        writing.program = Some(String::new());
        assert_eq!(writing_lines(&writing), None, "an empty program shows none");
        writing.program = Some("a\nb".into());
        assert_eq!(writing_lines(&writing), Some(2));
        writing.program = Some("a\nb\n".into());
        assert_eq!(
            writing_lines(&writing),
            Some(3),
            "a trailing newline is a line"
        );
        writing.status = ToolStatus::Running;
        assert_eq!(writing_lines(&writing), None);
    }
}
