//! The one place that adapts the core's seam types to the transcript's own
//! item model: a chat's stored messages (`mcbench_store`'s typed
//! [`mcbench_store::Message`] of each [`mcbench_store::StoredMessage`]) and a
//! turn's events ([`mcbench_agent::TurnEvent`]: a call's [`CallShown`], a
//! result's [`ToolShown`], a test's [`ProgressShown`]). A stored call's line
//! and a stored result's view come from the same functions of the core that
//! make the events' (`mcbench_agent::call_shown`, `mcbench_agent::tool_shown`),
//! since the stored message holds only the call's arguments and the
//! result's content as the model read them.

use mcbench_agent::{CallShown, ProgressShown, ToolShown};
use mcbench_store::{Content, Message, Part};
use serde_json::Value;

/// A part of a user message's content.
#[derive(Clone, Debug, PartialEq)]
pub enum UserPart {
    /// Text.
    Text(String),
    /// An image's address.
    ImageUrl(String),
    /// An attached file.
    Attachment(mcbench_store::Attachment),
    /// A selection in the viewer.
    Selection(mcbench_store::Selection),
}

/// A tool call of an assistant message, with what the core computes for the
/// transcript (its summary and program).
#[derive(Clone, Debug, PartialEq)]
pub struct StoredToolCall {
    /// The call's id.
    pub id: String,
    /// The tool's name.
    pub name: String,
    /// The arguments, as the model wrote them.
    pub arguments: String,
    /// The line the transcript shows.
    pub summary: String,
    /// The program or command (for `test_build` its arguments indented).
    pub program: Option<String>,
}

/// The world's change a tool made.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Diff {
    /// Blocks added.
    pub added: u64,
    /// Blocks removed.
    pub removed: u64,
    /// Blocks changed.
    pub changed: u64,
}

/// What the transcript shows of a tool's result.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Shown {
    /// It succeeded.
    pub ok: bool,
    /// It was cancelled.
    pub cancelled: bool,
    /// Why it failed.
    pub error: Option<String>,
    /// The world's change.
    pub diff: Option<Diff>,
    /// Its output.
    pub output: Option<String>,
    /// Pictures it returned.
    pub images: Vec<std::path::PathBuf>,
}

/// A tool's progress while it runs.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Progress {
    /// The line to show.
    pub message: String,
}

/// A stored message, by role.
#[derive(Clone, Debug, PartialEq)]
pub enum StoredMessage {
    /// What the user sent.
    User {
        /// The message's id.
        id: Option<String>,
        /// Its content.
        parts: Vec<UserPart>,
        /// The pictures the app fed back after `see_build`.
        see_build: bool,
    },
    /// The model's message.
    Assistant {
        /// The message's id.
        id: Option<String>,
        /// Its text.
        content: Option<String>,
        /// Its reasoning.
        reasoning: Option<String>,
        /// A harness's notices: level and text.
        notices: Vec<(String, String)>,
        /// A harness's task list.
        plan: Option<Value>,
        /// Its tool calls.
        tool_calls: Vec<StoredToolCall>,
    },
    /// A tool's result.
    Tool {
        /// The call it answers.
        tool_call_id: String,
        /// What the transcript shows of it.
        shown: Shown,
    },
    /// A turn's error.
    Error {
        /// The message's id.
        id: Option<String>,
        /// The text.
        content: String,
        /// Dismissed by the user.
        dismissed: bool,
    },
}

/// A chat's stored messages (`Update::Transcript`'s), oldest first.
pub fn from_stored(messages: &[mcbench_store::StoredMessage]) -> Vec<StoredMessage> {
    // A tool message names only its call; its view depends on the call's tool.
    let mut names: std::collections::HashMap<&str, &str> = Default::default();
    messages
        .iter()
        .filter_map(|stored| {
            let id = Some(stored.id.0.clone());
            Some(match &stored.message {
                Message::User(user) => StoredMessage::User {
                    id,
                    parts: match &user.content {
                        Content::Text(text) => vec![UserPart::Text(text.clone())],
                        Content::Parts(parts) => parts
                            .iter()
                            .map(|part| match part {
                                Part::Text { text } => UserPart::Text(text.clone()),
                                Part::ImageUrl { image_url } => {
                                    UserPart::ImageUrl(image_url.url.clone())
                                }
                                Part::Attachment { attachment } => {
                                    UserPart::Attachment(attachment.clone())
                                }
                                Part::Selection { selection } => {
                                    UserPart::Selection(selection.clone())
                                }
                            })
                            .collect(),
                    },
                    see_build: user.see_build,
                },
                Message::Assistant(assistant) => {
                    let tool_calls: Vec<StoredToolCall> = assistant
                        .tool_calls
                        .iter()
                        .flatten()
                        .map(|call| {
                            names.insert(&call.id, &call.function.name);
                            let shown = mcbench_agent::call_shown(
                                &call.function.name,
                                &call.function.arguments,
                            );
                            StoredToolCall {
                                id: call.id.clone(),
                                name: call.function.name.clone(),
                                arguments: call.function.arguments.clone(),
                                program: program_of(&shown),
                                summary: shown.summary,
                            }
                        })
                        .collect();
                    StoredMessage::Assistant {
                        id,
                        content: Some(assistant.content.clone()).filter(|c| !c.is_empty()),
                        reasoning: assistant.reasoning.clone().filter(|r| !r.is_empty()),
                        notices: assistant
                            .notices
                            .iter()
                            .flatten()
                            .map(|n| (n.level.clone(), n.text.clone()))
                            .collect(),
                        plan: assistant.plan.as_ref().map(mcbench_store::to_json),
                        tool_calls,
                    }
                }
                Message::Tool(tool) => {
                    // A result whose call no earlier message made is
                    // skipped, as the page skips it (`toolsById` finds
                    // nothing).
                    let name = names.get(tool.tool_call_id.as_str()).copied()?;
                    StoredMessage::Tool {
                        tool_call_id: tool.tool_call_id.clone(),
                        shown: shown_from(&mcbench_agent::tool_shown(name, &tool.content)),
                    }
                }
                Message::Error(error) => StoredMessage::Error {
                    id,
                    content: error.content.clone(),
                    dismissed: error.dismissed,
                },
            })
        })
        .collect()
}

/// A result's shown fields (`TurnEvent::ToolResult::shown`).
pub fn shown_from(shown: &ToolShown) -> Shown {
    // A diff's counts are whole numbers of blocks; the result holds them as
    // JSON numbers.
    let count = |n: f64| n as u64;
    Shown {
        ok: shown.ok,
        cancelled: shown.cancelled,
        error: shown.error.clone().filter(|e| !e.is_empty()),
        diff: shown.diff.map(|diff| Diff {
            added: count(diff.added),
            removed: count(diff.removed),
            changed: count(diff.changed),
        }),
        output: shown.output.clone(),
        // Paths stay paths to the picture element: a data folder whose path
        // is not UTF-8 works.
        images: shown.images.clone(),
    }
}

/// A call's program as the transcript shows it (`TurnEvent::ToolStart`'s
/// `shown.program`): a string as it is, `null` as none (`event.program ??`
/// keeps the streamed one), anything else as its JSON indented by two.
pub fn program_of(shown: &CallShown) -> Option<String> {
    match &shown.program {
        None | Some(Value::Null) => None,
        Some(Value::String(program)) => Some(program.clone()),
        // As JavaScript's JSON.stringify(value, null, 2) prints it.
        Some(other) => Some(mcbench_agent::stringify_pretty(other, 2)),
    }
}

/// A test's progress line (`TurnEvent::ToolProgress::progress`).
pub fn progress_of(progress: &ProgressShown) -> Progress {
    Progress {
        message: progress.message.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn call(program: Option<Value>) -> CallShown {
        CallShown {
            summary: "build".into(),
            program,
        }
    }

    #[test]
    fn a_program_shows_a_string_as_it_is_and_other_json_indented() {
        assert_eq!(
            program_of(&call(Some(json!("place(0,0,0)")))).as_deref(),
            Some("place(0,0,0)")
        );
        assert_eq!(
            program_of(&call(Some(json!({"a": [1]})))).as_deref(),
            Some("{\n  \"a\": [\n    1\n  ]\n}")
        );
        assert_eq!(program_of(&call(Some(Value::Null))), None);
        assert_eq!(program_of(&call(None)), None);
    }
}

/// A search snippet split at the store's marks (`\u{1}` opens a match,
/// `\u{2}` closes it) into text and whether it matched.
pub fn snippet_parts(snippet: &str) -> Vec<(String, bool)> {
    let mut parts = Vec::new();
    let mut matched = false;
    let mut current = String::new();
    for ch in snippet.chars() {
        match ch {
            '\u{1}' | '\u{2}' => {
                if !current.is_empty() {
                    parts.push((std::mem::take(&mut current), matched));
                }
                matched = ch == '\u{1}';
            }
            other => current.push(other),
        }
    }
    if !current.is_empty() {
        parts.push((current, matched));
    }
    parts
}

#[cfg(test)]
mod snippet_tests {
    use super::snippet_parts;

    #[test]
    fn a_snippet_splits_at_its_marks() {
        assert_eq!(
            snippet_parts("…the \u{1}portcullis\u{2} between"),
            vec![
                ("…the ".to_string(), false),
                ("portcullis".to_string(), true),
                (" between".to_string(), false)
            ]
        );
    }
}

#[cfg(test)]
mod review_tests {
    use super::*;

    fn row(id: &str, json: &str) -> mcbench_store::StoredMessage {
        mcbench_store::StoredMessage::from_payload(id, None, 0, json.to_string())
            .expect("a stored message")
    }

    #[test]
    fn a_tool_result_without_its_call_is_skipped() {
        let messages = [
            row(
                "u",
                r#"{"role":"user","content":"hi","createdAt":"2026-11-17T10:00:00.000Z"}"#,
            ),
            row(
                "t",
                r#"{"role":"tool","tool_call_id":"nowhere","content":"done","createdAt":"2026-11-17T10:00:00.000Z"}"#,
            ),
        ];
        let typed = from_stored(&messages);
        assert_eq!(typed.len(), 1, "the orphan result is skipped: {typed:?}");
        assert!(matches!(typed[0], StoredMessage::User { .. }));
    }

    #[test]
    fn a_program_that_is_not_a_string_prints_as_javascript_does() {
        let shown = CallShown {
            summary: "Test build".into(),
            program: Some(serde_json::json!({ "ticks": 2.0, "big": 1e21 })),
        };
        let printed = program_of(&shown).expect("a program");
        assert!(printed.contains("\"ticks\": 2"), "{printed}");
        assert!(!printed.contains("2.0"), "{printed}");
        assert!(printed.contains("1e+21"), "{printed}");
    }

    #[cfg(unix)]
    #[test]
    fn a_picture_path_that_is_not_utf8_stays_a_path() {
        use std::os::unix::ffi::OsStrExt;
        let path =
            std::path::PathBuf::from(std::ffi::OsStr::from_bytes(b"/data/\xff\xfe/shot.png"));
        let shown = ToolShown {
            ok: true,
            cancelled: false,
            error: None,
            diff: None,
            output: None,
            images: vec![path.clone()],
        };
        assert_eq!(shown_from(&shown).images, vec![path]);
    }
}
