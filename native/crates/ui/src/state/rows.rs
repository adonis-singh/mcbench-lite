//! The transcript's rows as pieces: what each entry shows given which folds
//! are open, in reading order. The view draws exactly these pieces, and the
//! selection and the accessibility tree count text in the same order, so
//! copied text never depends on which rows the virtual list has laid out.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use super::branches::Branch;
use super::doc::{self, Block, Boundary, Leaf};
use super::transcript::{Entry, Item, ShownFile, StackedCall, ToolCall, ToolStatus};

/// What a row is, for its accessibility node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowKind {
    /// The user's message.
    User,
    /// The model's message.
    Assistant,
    /// The model's work: a fold, reasoning, a tool stack.
    Work,
    /// A turn's error.
    Error,
    /// A harness's notice or task list.
    Notice,
}

/// One piece of a row.
#[derive(Clone, Debug, PartialEq)]
pub enum Piece {
    /// Plain text, as typed (a user message, an error, a notice, a plan line).
    Text(String),
    /// Markdown; `note` is the quieter register inside a fold.
    Markdown {
        /// The parsed blocks.
        blocks: Arc<Vec<Block>>,
        /// Drawn as a log note.
        note: bool,
    },
    /// A fold's summary row (activity, reasoning, previous tool calls).
    Fold {
        /// The fold's key in the open set.
        key: String,
        /// What it says.
        label: String,
        /// Whether it is open.
        open: bool,
        // par-chat: the summary's `title` (`Fold`'s), a hover tip.
        /// Its title: `Show reasoning`, `Show previous tool calls`; none
        /// for the activity fold.
        title: Option<&'static str>,
    },
    /// A tool call's row; it opens when it has a program.
    Tool {
        /// The call.
        call: ToolCall,
        /// Whether it is open.
        open: bool,
    },
    /// A tool call's code, verbatim, under its open row (`pre.code`).
    // par-chat: with its call and whether it is still being written, for
    // the box's follow of its bottom; `awaiting` is the page's
    // `Streaming tool input…` (a call being written with no program yet).
    Code {
        /// The call's id.
        call: String,
        /// The program, or the awaiting line.
        text: String,
        /// The call is still being written (`writing`).
        writing: bool,
        /// `text` is `Streaming tool input…` (`.code-awaiting`).
        awaiting: bool,
    },
    /// A tool call's output under its open row.
    Output(String),
    /// Pictures.
    Shots(Vec<std::path::PathBuf>),
    /// A user message's files (`.msg-attachments`).
    Files(Vec<ShownFile>),
    /// A user message's selections (`.msg-selections`).
    Selections(Vec<crate::Selection>),
    /// A user message's tools (`.msg-tools`): branch arrows, Copy message,
    /// Edit and branch.
    Tools(MessageTools),
    /// A harness's task list (`PlanCard`): each task's status and text.
    Plan(Vec<(String, String)>),
    /// A harness's notice (`NoticeLine`).
    Notice {
        /// `info`, `warning` or `error`.
        level: String,
        /// The text.
        text: String,
    },
}

/// What a user message's tools show.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MessageTools {
    /// The stored message's id (none until the stored transcript has it).
    pub message_id: Option<String>,
    /// Its place among its versions, when it has others.
    pub branch: Option<Branch>,
    /// The arrows are disabled: a turn runs, or a change is under way.
    pub arrows_disabled: bool,
    /// Edit and branch is shown: a stored message, more than one item, no
    /// turn running, not a bench model's chat.
    pub can_edit: bool,
    /// Edit and branch is disabled: a branch switch or an edit is under way.
    pub edit_disabled: bool,
    /// Copied in the last 1.2 s (its icon is a check).
    pub copied: bool,
}

/// What the rows need besides the entries (`App`'s `branches`, `busy`,
/// `changing`, `editedId`, `copied`, `readOnly`).
pub struct Context<'a> {
    /// Each branchable message's place among its versions.
    pub branches: &'a HashMap<String, Branch>,
    /// A turn runs.
    pub busy: bool,
    /// A branch switch waits, or an edit is in the composer.
    pub changing: bool,
    /// The message the composer edits.
    pub edited: Option<&'a str>,
    /// The row key of the message copied in the last 1.2 s.
    pub copied: Option<&'a str>,
    /// A bench model's chat: it reads, never edits.
    pub read_only: bool,
    /// The transcript has more than one item.
    pub several: bool,
}

impl Piece {
    /// The selectable text this piece adds.
    pub fn leaves(&self) -> Vec<Leaf> {
        let line = |text: &str| {
            vec![Leaf {
                boundary: Boundary::Line,
                text: text.to_string(),
            }]
        };
        match self {
            Piece::Text(text) | Piece::Code { text, .. } | Piece::Output(text) => line(text),
            Piece::Notice { text, .. } => line(text),
            Piece::Plan(tasks) => tasks
                .iter()
                .map(|(_, text)| Leaf {
                    boundary: Boundary::Line,
                    text: text.clone(),
                })
                .collect(),
            Piece::Markdown { blocks, .. } => doc::leaves(blocks),
            // A failed call's error line is text under its row.
            Piece::Tool { call, .. }
                if call.status == ToolStatus::Failed && !call.detail.is_empty() =>
            {
                line(&call.detail)
            }
            Piece::Fold { .. }
            | Piece::Tool { .. }
            | Piece::Shots(_)
            | Piece::Files(_)
            | Piece::Selections(_)
            | Piece::Tools(_) => Vec::new(),
        }
    }
}

/// A row of the transcript.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    /// Its key (`entryKeys`), stable while rows around it change.
    pub key: String,
    /// What it is.
    pub kind: RowKind,
    /// Its pieces in order.
    pub pieces: Vec<Piece>,
    /// Still streaming.
    pub live: bool,
    /// The message the composer edits (`.user-block.editing`).
    pub editing: bool,
    /// An error row's dismiss is shown (not a bench model's chat, no turn
    /// running).
    pub dismissable: bool,
}

impl Row {
    /// Its selectable text in order.
    pub fn leaves(&self) -> Vec<Leaf> {
        self.pieces.iter().flat_map(Piece::leaves).collect()
    }
}

/// Parsed markdown, kept per text so a streamed token parses only the text
/// it lands in.
#[derive(Default)]
pub struct Parsed(HashMap<String, Arc<Vec<Block>>>);

impl Parsed {
    /// The blocks of `text`.
    pub fn get(&mut self, text: &str) -> Arc<Vec<Block>> {
        if let Some(blocks) = self.0.get(text) {
            return blocks.clone();
        }
        let blocks = Arc::new(doc::parse(text));
        self.0.insert(text.to_string(), blocks.clone());
        blocks
    }

    /// Keeps only the texts of these rows (after a chat changes or a turn
    /// streams), so the cache does not grow with every streamed token.
    pub fn retain_texts(&mut self, keep: &HashSet<String>) {
        self.0.retain(|text, _| keep.contains(text));
    }
}

/// The rows of `entries` with `open` folds and tool rows.
pub fn rows(
    entries: &[Entry],
    keys: &[String],
    open: &HashSet<String>,
    parsed: &mut Parsed,
    context: &Context,
) -> Vec<Row> {
    entries
        .iter()
        .zip(keys)
        .map(|(entry, key)| row(entry, key, open, parsed, context))
        .collect()
}

fn row(
    entry: &Entry,
    key: &str,
    open: &HashSet<String>,
    parsed: &mut Parsed,
    context: &Context,
) -> Row {
    let mut pieces = Vec::new();
    let mut editing = false;
    let (kind, live) = match entry {
        Entry::Item(Item::User {
            text,
            message_id,
            files,
            selections,
        }) => {
            pieces.push(Piece::Text(text.clone()));
            if !files.is_empty() {
                pieces.push(Piece::Files(files.clone()));
            }
            if !selections.is_empty() {
                pieces.push(Piece::Selections(selections.clone()));
            }
            editing = message_id.is_some() && message_id.as_deref() == context.edited;
            pieces.push(Piece::Tools(MessageTools {
                message_id: message_id.clone(),
                branch: message_id
                    .as_ref()
                    .filter(|_| !context.read_only)
                    .and_then(|id| context.branches.get(id))
                    .cloned(),
                arrows_disabled: context.busy || context.changing,
                can_edit: message_id.is_some()
                    && context.several
                    && !context.busy
                    && !context.read_only,
                edit_disabled: context.changing,
                copied: context.copied == Some(key),
            }));
            (RowKind::User, false)
        }
        Entry::Item(Item::Assistant { text, live, .. }) => {
            pieces.push(Piece::Markdown {
                blocks: parsed.get(text),
                note: false,
            });
            (RowKind::Assistant, *live)
        }
        Entry::Item(Item::Reasoning { text, live }) => {
            reasoning(key, text, *live, open, parsed, &mut pieces);
            (RowKind::Work, *live)
        }
        Entry::Item(Item::Error { text, .. }) => {
            pieces.push(Piece::Text(text.clone()));
            (RowKind::Error, false)
        }
        Entry::Item(item @ (Item::Notice { .. } | Item::Plan(_))) => {
            log_item(key, 0, item, open, parsed, &mut pieces);
            (RowKind::Notice, false)
        }
        Entry::Item(item @ (Item::Tool(_) | Item::Shots { .. })) => {
            log_item(key, 0, item, open, parsed, &mut pieces);
            (RowKind::Work, false)
        }
        Entry::Activity(children) => {
            let steps = children
                .iter()
                .filter(|c| matches!(c, Item::Tool(_)))
                .count();
            let label = if steps > 0 {
                format!(
                    "Worked \u{b7} {steps} step{}",
                    if steps == 1 { "" } else { "s" }
                )
            } else {
                "Worked".to_string()
            };
            let is_open = open.contains(key);
            pieces.push(Piece::Fold {
                key: key.to_string(),
                label,
                open: is_open,
                title: None,
            });
            if is_open {
                // A burst of reasoning folds into one "Reasoning" row.
                let mut merged: Vec<Item> = Vec::new();
                for child in children {
                    match (merged.last_mut(), child) {
                        (
                            Some(Item::Reasoning { text, .. }),
                            Item::Reasoning { text: more, .. },
                        ) => {
                            text.push_str("\n\n");
                            text.push_str(more);
                        }
                        _ => merged.push(child.clone()),
                    }
                }
                for (index, child) in merged.iter().enumerate() {
                    log_item(key, index, child, open, parsed, &mut pieces);
                }
            }
            (RowKind::Work, false)
        }
        Entry::ToolStack(calls) => {
            let (latest, hidden) = calls.split_last().expect("a tool stack holds a call");
            stacked(key, latest, open, parsed, &mut pieces);
            if !hidden.is_empty() {
                let fold = format!("{key}/previous");
                let is_open = open.contains(&fold);
                pieces.push(Piece::Fold {
                    key: fold,
                    label: format!(
                        "+{} previous tool call{}",
                        hidden.len(),
                        if hidden.len() == 1 { "" } else { "s" }
                    ),
                    open: is_open,
                    title: Some("Show previous tool calls"),
                });
                if is_open {
                    for call in hidden {
                        stacked(key, call, open, parsed, &mut pieces);
                    }
                }
            }
            (RowKind::Work, true)
        }
    };
    Row {
        key: key.to_string(),
        kind,
        pieces,
        live,
        editing,
        dismissable: kind == RowKind::Error && !context.read_only && !context.busy,
    }
}

fn stacked(
    key: &str,
    call: &StackedCall,
    open: &HashSet<String>,
    parsed: &mut Parsed,
    pieces: &mut Vec<Piece>,
) {
    log_item(key, 0, &Item::Tool(call.call.clone()), open, parsed, pieces);
    for output in &call.outputs {
        log_item(key, 0, output, open, parsed, pieces);
    }
}

fn reasoning(
    key: &str,
    text: &str,
    live: bool,
    open: &HashSet<String>,
    parsed: &mut Parsed,
    pieces: &mut Vec<Piece>,
) {
    let is_open = open.contains(key);
    pieces.push(Piece::Fold {
        key: key.to_string(),
        label: if live {
            "Reasoning\u{2026}"
        } else {
            "Reasoning"
        }
        .to_string(),
        open: is_open,
        title: Some("Show reasoning"),
    });
    if is_open {
        pieces.push(Piece::Markdown {
            blocks: parsed.get(text),
            note: true,
        });
    }
}

/// `renderLogItem`: an item inside a fold or a stack.
fn log_item(
    key: &str,
    index: usize,
    item: &Item,
    open: &HashSet<String>,
    parsed: &mut Parsed,
    pieces: &mut Vec<Piece>,
) {
    match item {
        Item::Assistant { text, .. } => pieces.push(Piece::Markdown {
            blocks: parsed.get(text),
            note: true,
        }),
        Item::Reasoning { text, .. } => reasoning(
            &format!("{key}/reasoning{index}"),
            text,
            false,
            open,
            parsed,
            pieces,
        ),
        Item::Notice { level, text } => pieces.push(Piece::Notice {
            level: level.clone(),
            text: text.clone(),
        }),
        Item::Plan(plan) => pieces.push(Piece::Plan(plan_tasks(plan))),
        Item::Shots { images } => pieces.push(Piece::Shots(images.clone())),
        Item::Tool(call) => {
            let is_open = open.contains(&call.call_id);
            pieces.push(Piece::Tool {
                call: call.clone(),
                open: is_open,
            });
            if is_open {
                // `program ? <Code> : writing ? 'Streaming tool input…' : null`.
                let writing = call.status == ToolStatus::Writing;
                let program = call.program.clone().unwrap_or_default();
                let awaiting = program.is_empty() && writing;
                pieces.push(Piece::Code {
                    call: call.call_id.clone(),
                    text: if awaiting {
                        "Streaming tool input\u{2026}".to_string()
                    } else {
                        program
                    },
                    writing,
                    awaiting,
                });
                if let Some(output) = &call.output {
                    pieces.push(Piece::Output(output.clone()));
                }
            }
        }
        Item::User { text, .. } | Item::Error { text, .. } => {
            pieces.push(Piece::Text(text.clone()))
        }
    }
}

/// A plan's tasks, status and text (`msg.plan`: `[{ text, status }]`; a
/// task without them is a bug of the core).
pub fn plan_tasks(plan: &serde_json::Value) -> Vec<(String, String)> {
    plan.as_array()
        .unwrap_or_else(|| panic!("a plan is a list of tasks, not {plan}"))
        .iter()
        .map(|task| {
            let field = |name: &str| {
                task[name]
                    .as_str()
                    .unwrap_or_else(|| panic!("a plan's task has a {name}: {task}"))
                    .to_string()
            };
            (field("status"), field("text"))
        })
        .collect()
}

/// How a notice reads: an info's text, else its level then its text.
pub fn notice_text(level: &str, text: &str) -> String {
    let mut chars = level.chars();
    match (level, chars.next()) {
        ("info", _) | (_, None) => text.to_string(),
        (_, Some(first)) => format!("{}{}: {text}", first.to_uppercase(), chars.as_str()),
    }
}

/// How a plan reads: `Tasks, <done> of <total>`, then each task, a done one
/// with `, done` after it.
pub fn plan_spoken(tasks: &[(String, String)]) -> String {
    let done = tasks.iter().filter(|(s, _)| s == "done").count();
    std::iter::once(format!("Tasks, {done} of {}", tasks.len()))
        .chain(tasks.iter().map(|(status, text)| match status.as_str() {
            "done" => format!("{text}, done"),
            "active" => format!("{text}, in progress"),
            _ => text.clone(),
        }))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Whether a tool row opens: it has code to show, or code is streaming.
pub fn tool_expandable(call: &ToolCall) -> bool {
    call.program.is_some() || call.status == ToolStatus::Writing
}
