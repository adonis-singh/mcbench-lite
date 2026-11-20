//! The window's commands against today's routes (`tests/node/commands.js`):
//! the same operations on two clones of a fixture data folder, through
//! today's `handleApi` on one and through `Core::command` with a recording
//! `Window` on the other, with the same clock and ids, both started as
//! their app starts. Compared: each operation's answer (the route's error
//! sentence, or a read's JSON, against the update the command published)
//! and afterwards every row of both databases, the files beside them, the
//! prompt library and the settings' starred models.
//!
//! The operations, on chats and a bench the fixture holds: opening a chat
//! (and one that is not there), which marks nothing seen; mark seen;
//! archive, read the archive, restore (and again: refused), delete one not
//! archived (refused), archive one that is not there (refused), archive and
//! delete; a bench archived, read, restored; an error dismissed (first a
//! message that is not one: refused); a branch switch (and to a message
//! that is not there: refused); the starred models; a search; the prompt
//! library; the models and a model's facts of a provider or model that is
//! not one (refused); and sends the chat route refuses before their turn
//! (nine files, a file over 100 MB, no text, a bench's chat).
//!
//! Needs `MCBENCH_DIFF_DATA`, `MCBENCH_NODE_DIFF`, `MCBENCH_DIFF_SCRATCH`
//! (`mcbench-agent`'s `tests/common`), with `HOME`, `CLAUDE_CONFIG_DIR` and
//! `CODEX_HOME` inside `MCBENCH_DIFF_SCRATCH` (as `core_reads.rs`).

#[path = "../../agent/tests/common/mod.rs"]
mod common;

use std::path::Path;
use std::process::Command as Process;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde_json::{Value, json};

use mcbench_lite::core::backend::Backend;
use mcbench_lite::core::views::Transcript;
use mcbench_lite::core::{Core, Parts};
use mcbench_store::{Message, MessageId};
use mcbench_ui::{
    Command, Outgoing, OutgoingFile, Resolution, SearchScope, Subject, Target, Update, Window,
};

struct NoModel;

#[async_trait]
impl Backend for NoModel {
    fn provider(&self, _: &str) -> Result<Arc<dyn mcbench_providers::Provider>, String> {
        Err("no model in the commands".into())
    }
    fn tools(&self) -> mcbench_tools::Tools {
        mcbench_tools::Tools::of(Vec::new())
    }
    fn sync_attachments(&self, _: &str) -> Result<(), String> {
        Ok(())
    }
    async fn reset_sandbox(&self, _: &str) -> Result<(), String> {
        Ok(())
    }
    fn network_access(&self) -> bool {
        true
    }
    async fn shutdown(&self) {}
}

#[derive(Default)]
struct Recording(Mutex<Vec<Update>>);

impl Window for Recording {
    fn publish(&self, update: Update) {
        self.0.lock().unwrap().push(update);
    }
}

/// One operation: today's request, and the command that replaces it.
struct Op {
    method: &'static str,
    path: String,
    body: Option<Value>,
    command: Command,
    /// What the answer is compared by.
    kind: Kind,
}

/// `$error:<chat>`: the chat's last error message, made by an earlier
/// op's turn (`tests/node/commands.js` reads it the same way).
fn last_error(store: &mcbench_store::Store, chat: &str) -> String {
    let live = store.conversation(chat).unwrap().unwrap();
    let conversation = live.lock().unwrap();
    conversation
        .messages
        .iter()
        .rev()
        .find(|m| matches!(m.message, Message::Error(_)))
        .expect("an error message")
        .id
        .0
        .clone()
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    /// A change: only its error, if any.
    Change,
    /// A send: the route's refusal against `Update::SendRefused`.
    Send,
    /// The archive's JSON.
    Archive,
    /// The search's JSON.
    Search,
    /// The library as the interface holds it.
    Prompts,
    /// The starred models.
    Favorites,
    /// A read's refusal (`Models`, `ModelInfo`).
    Refused,
    /// A branch switch: its body (`messages`, `messageTree`).
    Branch,
    /// A send whose turn runs (and fails at its model): its events as the
    /// route's NDJSON lines, `turn_snapshot` left out.
    Turn,
    /// Opening a chat: its transcript as `GET /api/conversations/:id`'s
    /// body.
    Open,
}

/// Node's answer as compared: `{ error }`, or the body a read compares.
fn node_answer(kind: Kind, status: u64, body: &Value, lines: &[Value]) -> Value {
    if status != 200 {
        return json!({ "error": body["error"] });
    }
    match kind {
        Kind::Turn => {
            json!({ "lines": lines.iter().filter(|l| l["type"] != "turn_snapshot").collect::<Vec<_>>() })
        }
        Kind::Change | Kind::Send => json!({ "ok": null }),
        Kind::Prompts => json!({ "ok": body["prompts"] }),
        _ => json!({ "ok": body }),
    }
}

/// The core's answer to one command, from what it published.
fn rust_answer(kind: Kind, updates: &[Update]) -> Value {
    let error = updates.iter().find_map(|u| match u {
        Update::Error(e) => Some(e.clone()),
        Update::SendRefused { error, .. } | Update::TranscriptFailed { error, .. } => {
            Some(error.clone())
        }
        Update::Archive(Err(e))
        | Update::Prompts(Err(e))
        | Update::Found {
            results: Err(e), ..
        } => Some(e.clone()),
        Update::Models { models: Err(e), .. } => Some(e.clone()),
        Update::ModelInfo { facts: Err(e), .. } => Some(e.clone()),
        _ => None,
    });
    if let Some(error) = error {
        return json!({ "error": error });
    }
    let last = |pick: &dyn Fn(&Update) -> Option<Value>| {
        updates
            .iter()
            .rev()
            .find_map(pick)
            .unwrap_or_else(|| panic!("no answer in {updates:?}"))
    };
    match kind {
        Kind::Change | Kind::Send => json!({ "ok": null }),
        Kind::Turn => json!({ "lines": updates.iter().filter_map(|u| match u {
            Update::Turn { event, .. } => event.to_json(),
            _ => None,
        }).collect::<Vec<_>>() }),
        Kind::Archive => last(&|u| match u {
            Update::Archive(Ok(archive)) => Some(json!({ "ok": {
                "conversations": archive.conversations.iter().map(|row| json!({
                    "id": row.id, "title": row.title, "provider": row.provider, "model": row.model,
                    "archivedAt": row.archived_at, "expiresAt": row.expires_at,
                })).collect::<Vec<_>>(),
                "benches": archive.benches.iter().map(|row| json!({
                    "id": row.id, "title": row.title, "archivedAt": row.archived_at,
                    "expiresAt": row.expires_at, "models": row.models,
                })).collect::<Vec<_>>(),
            } })),
            _ => None,
        }),
        Kind::Search => last(&|u| match u {
            Update::Found {
                results: Ok(results),
                ..
            } => Some(json!({ "ok": { "results": results.iter()
                .map(|hit| json!({ "id": hit.id, "snippet": hit.snippet })).collect::<Vec<_>>() } })),
            _ => None,
        }),
        Kind::Prompts => last(&|u| match u {
            Update::Prompts(Ok(library)) => Some(json!({ "ok": library.iter()
                .map(mcbench_store::LibraryPrompt::to_json).collect::<Vec<_>>() })),
            _ => None,
        }),
        Kind::Favorites => last(&|u| match u {
            Update::Favorites(list) => Some(json!({ "ok": { "favorites": list.iter()
                .map(|(p, m)| format!("{p}::{m}")).collect::<Vec<_>>() } })),
            _ => None,
        }),
        Kind::Refused => panic!("a refused read published no error: {updates:?}"),
        Kind::Branch => last(&|u| match u {
            Update::Transcript {
                chat,
                header,
                messages,
                tree,
            } => {
                let transcript = Transcript {
                    chat: chat.clone(),
                    header: header.clone(),
                    messages: messages.clone(),
                    tree: tree.clone(),
                };
                Some(json!({ "ok": transcript.branch_json().unwrap() }))
            }
            _ => None,
        }),
        Kind::Open => last(&|u| match u {
            Update::Transcript {
                chat,
                header,
                messages,
                tree,
            } => {
                let transcript = Transcript {
                    chat: chat.clone(),
                    header: header.clone(),
                    messages: messages.clone(),
                    tree: tree.clone(),
                };
                Some(json!({ "ok": transcript.to_json().unwrap() }))
            }
            _ => None,
        }),
    }
}

fn node(env: &common::Env, data: &Path, script: &Path, args: &[&Path]) {
    let agent = Path::new(env!("CARGO_MANIFEST_DIR")).join("../agent/tests/node");
    let ours = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/node");
    let output = Process::new(std::env::var("MCBENCH_NODE").unwrap_or_else(|_| "node".into()))
        .arg("-r")
        .arg(agent.join("det.js"))
        .arg("-r")
        .arg(ours.join("startup.js"))
        .arg(script)
        .args(args)
        .env("MCBENCH_NODE_DIFF", &env.checkout)
        .env("MCBENCH_DATA_DIR", data)
        .env("MCBENCH_SETTINGS_FILE", data.join("settings.json"))
        .env("MCBENCH_TOOLS_DIR", env.scratch.join("tools-empty"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "node {}: {}",
        script.display(),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn first_difference(path: &str, a: &Value, b: &Value) -> String {
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => {
            for (key, value) in x {
                match y.get(key) {
                    Some(other) if other == value => {}
                    Some(other) => return first_difference(&format!("{path}.{key}"), value, other),
                    None => return format!("{path}.{key}: only node has it"),
                }
            }
            format!("{path}: rust has more keys")
        }
        (Value::Array(x), Value::Array(y)) => {
            for (i, (p, q)) in x.iter().zip(y).enumerate() {
                if p != q {
                    return first_difference(&format!("{path}[{i}]"), p, q);
                }
            }
            format!("{path}: node {} items, rust {}", x.len(), y.len())
        }
        _ => {
            let (a, b) = (a.to_string(), b.to_string());
            let at = a.chars().zip(b.chars()).take_while(|(x, y)| x == y).count();
            let from = at.saturating_sub(120);
            let cut = |s: &str| s.chars().skip(from).take(300).collect::<String>();
            format!("{path}:\n  node {}\n  rust {}", cut(&a), cut(&b))
        }
    }
}

/// The starred models of a folder's settings (and nothing else of them).
fn favorites_of(folder: &Path) -> Value {
    let text = std::fs::read_to_string(folder.join("settings.json")).unwrap_or_default();
    if text.is_empty() {
        return Value::Null;
    }
    let settings: Value = serde_json::from_str(&text).unwrap();
    settings["modelFavorites"].clone()
}

#[test]
#[ignore = "needs MCBENCH_DIFF_DATA, MCBENCH_NODE_DIFF and MCBENCH_DIFF_SCRATCH (mcbench-agent's tests/common)"]
fn the_commands_change_and_answer_as_todays_routes() {
    let env = common::env();
    for name in ["HOME", "CLAUDE_CONFIG_DIR", "CODEX_HOME"] {
        let value = std::env::var(name).unwrap_or_default();
        assert!(
            Path::new(&value).starts_with(&env.scratch),
            "{name} must be inside MCBENCH_DIFF_SCRATCH, so no sign-in is read"
        );
    }
    let node_data = common::clone(&env, &env.fixture, "core-commands-node");
    let rust_data = common::clone(&env, &env.fixture, "core-commands-rust");

    let core = Core::start(Parts {
        data: rust_data.clone(),
        program: std::env::current_exe().unwrap(),
        host: common::DetHost::new(common::DET_NOW),
        shipped: Box::new(|| Err("unused".into())),
        client: mcbench_setup::fetch::client(),
        backend: Some(Arc::new(NoModel)),
        installer: None,
        share: None,
        live: Default::default(),
    })
    .unwrap();
    let window = Arc::new(Recording::default());
    mcbench_ui::Core::attach(&core, window.clone());
    let handle = core.handle().clone();
    handle.block_on(core.started());

    // The subjects, as the fixture holds them.
    let store = Arc::clone(core.store());
    let mut chats = Vec::new();
    let mut bench_chat = None;
    let mut error_chat = None;
    let mut branch_chat = None;
    for (id, archived) in store.list_conversation_ids().unwrap() {
        if archived {
            continue;
        }
        let live = store.conversation(&id).unwrap().unwrap();
        let conversation = live.lock().unwrap();
        if conversation.bench.is_some() {
            bench_chat.get_or_insert(id.clone());
            continue;
        }
        if conversation.messages.is_empty() {
            continue;
        }
        let error = conversation.messages.iter().find_map(|m| match &m.message {
            Message::Error(e) if !e.dismissed => Some(m.id.0.clone()),
            _ => None,
        });
        if let (None, Some(error)) = (&error_chat, error) {
            error_chat = Some((id.clone(), error, conversation.messages[0].id.0.clone()));
            continue;
        }
        let tree = store.message_tree(&conversation).unwrap();
        let shown: Vec<&str> = conversation
            .messages
            .iter()
            .map(|m| m.id.0.as_str())
            .collect();
        let other = tree.messages.iter().find(|m| {
            m.branchable && m.role.as_deref() == Some("user") && !shown.contains(&m.id.as_str())
        });
        if let (None, Some(other)) = (&branch_chat, other) {
            branch_chat = Some((id.clone(), other.id.clone()));
            continue;
        }
        chats.push((
            id.clone(),
            conversation.provider.clone(),
            conversation.model.clone(),
        ));
    }
    let benches: Vec<String> = store
        .list_bench_ids()
        .unwrap()
        .into_iter()
        .filter(|(_, archived)| !archived)
        .map(|(id, _)| id)
        .collect();
    assert!(chats.len() >= 2, "the fixture has two chats of its own");
    let (a, a_provider, a_model) = chats[0].clone();
    let (b, _, _) = chats[1].clone();
    println!(
        "subjects: chats {a} {b}, bench {:?}, bench chat {:?}, error {:?}, branch {:?}",
        benches.first(),
        bench_chat,
        error_chat.as_ref().map(|e| &e.0),
        branch_chat.as_ref().map(|b| &b.0)
    );

    let chat_body = |text: &str, files: Vec<Value>| {
        json!({ "provider": a_provider, "model": a_model, "effort": "", "resolution": 1,
            "text": text, "files": files, "selections": [], "prompts": [] })
    };
    let send = |chat: &str, text: &str, files: Vec<OutgoingFile>| {
        Command::Send(Outgoing {
            target: Target::Chat {
                id: chat.into(),
                provider: a_provider.clone(),
                model: a_model.clone(),
            },
            text: text.into(),
            attachments: files,
            selections: Vec::new(),
            prompts: Vec::new(),
            effort: None,
            resolution: Resolution::Standard,
            edit_of: None,
        })
    };
    let file = |name: &str, bytes: usize| OutgoingFile {
        name: name.into(),
        content_type: "text/plain".into(),
        size: bytes as u64,
        data: vec![b'x'; bytes].into(),
    };
    let file_json = |file: &OutgoingFile| {
        use base64::Engine as _;
        json!({ "name": file.name, "type": file.content_type, "size": file.size,
            "data": format!("data:{};base64,{}", file.content_type,
                base64::engine::general_purpose::STANDARD.encode(&file.data)) })
    };
    let op = |method: &'static str, path: String, body: Option<Value>, command, kind| Op {
        method,
        path,
        body,
        command,
        kind,
    };
    let chat_op = |id: &str, what: &str| format!("/api/conversations/{id}/{what}");
    let mut ops = vec![
        // Opening a chat reads it and marks nothing seen (the rows after
        // show it); the mark is the next op's.
        op(
            "GET",
            format!("/api/conversations/{a}"),
            None,
            Command::Open { chat: a.clone() },
            Kind::Open,
        ),
        op(
            "GET",
            "/api/conversations/nobody".into(),
            None,
            Command::Open {
                chat: "nobody".into(),
            },
            Kind::Open,
        ),
        op(
            "POST",
            chat_op(&a, "seen"),
            None,
            Command::MarkSeen { chat: a.clone() },
            Kind::Change,
        ),
        op(
            "POST",
            chat_op(&b, "archive"),
            None,
            Command::Archive(Subject::Chat(b.clone())),
            Kind::Change,
        ),
        op(
            "GET",
            "/api/archive".into(),
            None,
            Command::ReadArchive,
            Kind::Archive,
        ),
        op(
            "POST",
            chat_op(&b, "restore"),
            None,
            Command::Restore(Subject::Chat(b.clone())),
            Kind::Change,
        ),
        op(
            "POST",
            chat_op(&b, "restore"),
            None,
            Command::Restore(Subject::Chat(b.clone())),
            Kind::Change,
        ),
        op(
            "DELETE",
            format!("/api/conversations/{a}"),
            None,
            Command::Delete(Subject::Chat(a.clone())),
            Kind::Change,
        ),
        op(
            "POST",
            chat_op("nobody", "archive"),
            None,
            Command::Archive(Subject::Chat("nobody".into())),
            Kind::Change,
        ),
        op(
            "POST",
            chat_op(&b, "archive"),
            None,
            Command::Archive(Subject::Chat(b.clone())),
            Kind::Change,
        ),
        op(
            "DELETE",
            format!("/api/conversations/{b}"),
            None,
            Command::Delete(Subject::Chat(b.clone())),
            Kind::Change,
        ),
    ];
    // A turn of chat A that fails at its model stores an error: dismissed
    // (a message that is not one first); then an edit of A's first prompt
    // makes a branch, and the switch goes back to the first one.
    let first_prompt = {
        let live = store.conversation(&a).unwrap().unwrap();
        let conversation = live.lock().unwrap();
        conversation.prompts()[0].id.0.clone()
    };
    let turn_body = |text: &str, edit_of: Option<&str>| {
        let mut body = chat_body(text, Vec::new());
        if let Some(edit) = edit_of {
            body["editOf"] = json!(edit);
        }
        body
    };
    ops.extend([
        op(
            "POST",
            chat_op(&a, "chat"),
            Some(turn_body("Build a tower", None)),
            send(&a, "Build a tower", Vec::new()),
            Kind::Turn,
        ),
        op(
            "POST",
            chat_op(&a, "dismiss-error"),
            Some(json!({ "messageId": first_prompt })),
            Command::DismissError {
                chat: a.clone(),
                message: MessageId(first_prompt.clone()),
            },
            Kind::Change,
        ),
        op(
            "POST",
            chat_op(&a, "dismiss-error"),
            Some(json!({ "messageId": format!("$error:{a}") })),
            Command::DismissError {
                chat: a.clone(),
                message: MessageId(format!("$error:{a}")),
            },
            Kind::Change,
        ),
        op(
            "POST",
            chat_op(&a, "chat"),
            Some(turn_body("Build a barn", Some(&first_prompt))),
            Command::Send(Outgoing {
                edit_of: Some(MessageId(first_prompt.clone())),
                ..match send(&a, "Build a barn", Vec::new()) {
                    Command::Send(outgoing) => outgoing,
                    _ => unreachable!(),
                }
            }),
            Kind::Turn,
        ),
        op(
            "POST",
            chat_op(&a, "branch"),
            Some(json!({ "messageId": "nothing" })),
            Command::Branch {
                chat: a.clone(),
                message: MessageId("nothing".into()),
            },
            Kind::Branch,
        ),
        op(
            "POST",
            chat_op(&a, "branch"),
            Some(json!({ "messageId": first_prompt })),
            Command::Branch {
                chat: a.clone(),
                message: MessageId(first_prompt.clone()),
            },
            Kind::Branch,
        ),
    ]);
    if let Some(bench) = benches.first() {
        ops.extend([
            op(
                "POST",
                format!("/api/benches/{bench}/archive"),
                None,
                Command::Archive(Subject::Bench(bench.clone())),
                Kind::Change,
            ),
            op(
                "GET",
                "/api/archive".into(),
                None,
                Command::ReadArchive,
                Kind::Archive,
            ),
            op(
                "POST",
                format!("/api/benches/{bench}/restore"),
                None,
                Command::Restore(Subject::Bench(bench.clone())),
                Kind::Change,
            ),
        ]);
    }
    if let Some((chat, error, prompt)) = &error_chat {
        ops.extend([
            op(
                "POST",
                chat_op(chat, "dismiss-error"),
                Some(json!({ "messageId": prompt })),
                Command::DismissError {
                    chat: chat.clone(),
                    message: MessageId(prompt.clone()),
                },
                Kind::Change,
            ),
            op(
                "POST",
                chat_op(chat, "dismiss-error"),
                Some(json!({ "messageId": error })),
                Command::DismissError {
                    chat: chat.clone(),
                    message: MessageId(error.clone()),
                },
                Kind::Change,
            ),
        ]);
    }
    if let Some((chat, message)) = &branch_chat {
        ops.extend([
            op(
                "POST",
                chat_op(chat, "branch"),
                Some(json!({ "messageId": "nothing" })),
                Command::Branch {
                    chat: chat.clone(),
                    message: MessageId("nothing".into()),
                },
                Kind::Branch,
            ),
            op(
                "POST",
                chat_op(chat, "branch"),
                Some(json!({ "messageId": message })),
                Command::Branch {
                    chat: chat.clone(),
                    message: MessageId(message.clone()),
                },
                Kind::Branch,
            ),
        ]);
    }
    let stars = [
        ("openrouter", "a/b"),
        ("nobody", "x"),
        ("anthropic", " c "),
        ("openrouter", "a/b"),
    ];
    ops.extend([
        op(
            "PUT",
            "/api/model-favorites".into(),
            Some(json!({ "favorites": stars.iter().map(|(p, m)| format!("{p}::{m}")).collect::<Vec<_>>() })),
            Command::SetFavorites(
                stars
                    .iter()
                    .map(|(p, m)| (p.to_string(), m.to_string()))
                    .collect(),
            ),
            Kind::Favorites,
        ),
        op(
            "GET",
            "/api/search?scope=build&q=house".into(),
            None,
            Command::Search {
                scope: SearchScope::Build,
                query: "house".into(),
                request: 1,
            },
            Kind::Search,
        ),
        op(
            "GET",
            "/api/prompts".into(),
            None,
            Command::ReadPrompts,
            Kind::Prompts,
        ),
        op(
            "GET",
            "/api/models?provider=nobody".into(),
            None,
            Command::Models {
                provider: "nobody".into(),
            },
            Kind::Refused,
        ),
        op(
            "GET",
            "/api/model-info?provider=openrouter&model=".into(),
            None,
            Command::ModelInfo {
                provider: "openrouter".into(),
                model: String::new(),
            },
            Kind::Refused,
        ),
    ]);
    let nine: Vec<OutgoingFile> = (0..9).map(|i| file(&format!("f{i}.txt"), 4)).collect();
    let big = vec![file(
        "big.txt",
        mcbench_agent::MAX_ATTACHMENT_BYTES as usize + 1,
    )];
    for (text, files) in [("x", nine), ("x", big), ("   ", Vec::new())] {
        ops.push(op(
            "POST",
            chat_op(&a, "chat"),
            Some(chat_body(text, files.iter().map(file_json).collect())),
            send(&a, text, files),
            Kind::Send,
        ));
    }
    if let Some(member) = &bench_chat {
        ops.push(op(
            "POST",
            chat_op(member, "chat"),
            Some(chat_body("x", Vec::new())),
            send(member, "x", Vec::new()),
            Kind::Send,
        ));
    }

    // Node's side.
    let ops_file = env.scratch.join("core-commands-ops.json");
    let node_out = env.scratch.join("core-commands-node.json");
    std::fs::write(
        &ops_file,
        serde_json::to_string(
            &ops.iter()
                .map(|o| json!({ "method": o.method, "path": o.path, "body": o.body }))
                .collect::<Vec<_>>(),
        )
        .unwrap(),
    )
    .unwrap();
    node(
        &env,
        &node_data,
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/node/commands.js"),
        &[&ops_file, &node_out],
    );
    let theirs: Vec<Value> =
        serde_json::from_str(&std::fs::read_to_string(&node_out).unwrap()).unwrap();

    // The core's side.
    let mut differences = Vec::new();
    for (index, (op, node)) in ops.into_iter().zip(&theirs).enumerate() {
        let from = window.0.lock().unwrap().len();
        let command = match op.command {
            Command::DismissError { chat, message } if message.0.starts_with("$error:") => {
                Command::DismissError {
                    message: MessageId(last_error(&store, &message.0["$error:".len()..])),
                    chat,
                }
            }
            other => other,
        };
        let turn_of = match (&command, op.kind) {
            (Command::Send(outgoing), Kind::Turn) => match &outgoing.target {
                Target::Chat { id, .. } => Some(id.clone()),
                Target::NewChat { .. } | Target::NewBench { .. } | Target::Bench(_) => {
                    unreachable!("the ops send to chats")
                }
            },
            _ => None,
        };
        handle
            .block_on(handle.spawn({
                let core = core.clone();
                async move {
                    core.command(command).await;
                    // As today's route answers once its turn has ended.
                    if let Some(chat) = turn_of {
                        core.turns().free(&chat).await;
                        core.turns().titles_settled().await;
                    }
                }
            }))
            .unwrap();
        let updates = window.0.lock().unwrap()[from..].to_vec();
        let ours = rust_answer(op.kind, &updates);
        println!(
            "op {index}: {} {} -> {} {}",
            op.method,
            op.path,
            node["status"],
            ours.to_string().chars().take(100).collect::<String>()
        );
        let lines = node["lines"].as_array().cloned().unwrap_or_default();
        let expected = node_answer(
            op.kind,
            node["status"].as_u64().unwrap(),
            &node["body"],
            &lines,
        );
        if ours != expected {
            differences.push(first_difference(
                &format!("op {index} {} {}", op.method, op.path),
                &expected,
                &ours,
            ));
        }
    }
    let answered = theirs.len();
    core.shutdown();
    drop(core);

    // The folders afterwards.
    let dump = Path::new(env!("CARGO_MANIFEST_DIR")).join("../agent/tests/node/dump.js");
    let node_dump = env.scratch.join("core-commands-node-dump.json");
    let rust_dump = env.scratch.join("core-commands-rust-dump.json");
    node(&env, &node_data, &dump, &[&node_data, &node_dump]);
    node(&env, &rust_data, &dump, &[&rust_data, &rust_dump]);
    let node_doc: Value =
        serde_json::from_str(&std::fs::read_to_string(&node_dump).unwrap()).unwrap();
    let rust_doc: Value =
        serde_json::from_str(&std::fs::read_to_string(&rust_dump).unwrap()).unwrap();
    let mut rows = 0;
    for (table, node_rows) in node_doc["tables"].as_object().unwrap() {
        let node_rows = node_rows.as_array().unwrap();
        let rust_rows = rust_doc["tables"][table]
            .as_array()
            .cloned()
            .unwrap_or_default();
        rows += node_rows.len();
        if node_rows.len() != rust_rows.len() {
            differences.push(format!(
                "{table}: node {} rows, rust {}",
                node_rows.len(),
                rust_rows.len()
            ));
        }
        for (i, (x, y)) in node_rows.iter().zip(&rust_rows).enumerate() {
            if x != y {
                differences.push(first_difference(&format!("{table} row {i}"), x, y));
            }
        }
    }
    // The files, settings.json's text left out: only its starred models are
    // compared (the clone has none of the fixture's settings).
    let files = |doc: &Value| -> Vec<Value> {
        doc["files"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|entry| entry[0] != "settings.json")
            .cloned()
            .collect()
    };
    if files(&node_doc) != files(&rust_doc) {
        differences.push("files differ".into());
    }
    if favorites_of(&node_data) != favorites_of(&rust_data) {
        differences.push("the starred models differ".into());
    }
    for key in ["searchVocabulary", "sqlite_master", "user_version"] {
        if node_doc[key] != rust_doc[key] {
            differences.push(format!("{key} differs"));
        }
    }
    println!(
        "core commands: {answered} operations, {rows} rows, {} differences",
        differences.len()
    );
    for line in &differences {
        eprintln!("{line}\n");
    }
    assert!(differences.is_empty(), "{} differences", differences.len());
    for path in [&node_data, &rust_data] {
        std::fs::remove_dir_all(path).unwrap();
    }
    for file in [&ops_file, &node_out, &node_dump, &rust_dump] {
        std::fs::remove_file(file).unwrap();
    }
}
