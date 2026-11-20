//! Bench and the prompt library against today's routes
//! (`tests/node/bench.js`): the same operations on two clones of a
//! fixture's database, prompt library and chats' files, through today's
//! `handleApi` on one and through `Core::command` with a recording
//! `Window` on the other, with the same clock and ids and the same model
//! (`Done.` with fixed telemetry; a model named `fail/...` fails; a title
//! is `A title`), each side waiting after every operation until nothing is
//! busy. Compared: each operation's answer (a refusal's sentence; a bench's
//! details as the route's JSON text, byte for byte; a round count, a
//! branch's round number, the votes, the leaderboard, a fork's chat, the
//! library as saved) against what the command published, and afterwards
//! every row of both databases and the files beside them
//! (`mcbench-agent`'s `tests/node/dump.js`).
//!
//! The operations: a blind bench made (a file and a library prompt on its
//! first round) and read; a round sent, the first round edited, the branch
//! switched back, each read; votes cast, as a tie, taken back and cast
//! again, on a round that is not one (refused), the leaderboard read; the
//! names revealed and a vote then (refused); a bench whose second model
//! fails, that model retried and the other one (refused); a model's chat
//! forked, and at a round that is not one (refused); the library saved;
//! and sends refused (one model; selections on a round).
//!
//! Needs `MCBENCH_DIFF_DATA`, `MCBENCH_NODE_DIFF`, `MCBENCH_DIFF_SCRATCH`
//! (`mcbench-agent`'s `tests/common`), with `HOME`, `CLAUDE_CONFIG_DIR` and
//! `CODEX_HOME` inside `MCBENCH_DIFF_SCRATCH` (as `core_reads.rs`).

#[path = "../../agent/tests/common/mod.rs"]
mod common;

use std::path::{Path, PathBuf};
use std::process::Command as Process;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use mcbench_lite::core::backend::Backend;
use mcbench_lite::core::{Core, Parts};
use mcbench_providers::{
    AssistantMessage, CompletionRequest, Delta, ModelFacts, ModelInfo, Outcome, Provider,
    ProviderError, ProviderId, Telemetry, Usage,
};
use mcbench_ui::{
    BenchChoice, Command, LibraryPrompt, MessageId, Outgoing, OutgoingFile, PromptLink, Resolution,
    Selection, Target, Update, VotePick, Window,
};

/// `bench.js`'s model.
struct Model;

#[async_trait]
impl Provider for Model {
    fn id(&self) -> ProviderId {
        ProviderId::OpenRouter
    }
    async fn complete(
        &self,
        request: &CompletionRequest,
        on_delta: &mut (dyn FnMut(Delta) + Send),
        _: &CancellationToken,
    ) -> Result<AssistantMessage, ProviderError> {
        let mut message = AssistantMessage {
            content: "A title".into(),
            reasoning: None,
            reasoning_details: None,
            finish_reason: None,
            telemetry: Telemetry {
                provider: ProviderId::OpenRouter,
                model: String::new(),
                effort: None,
                started_at: String::new(),
                wall_ms: 0,
                first_delta_ms: None,
                context_bytes: 0,
                message_count: 0,
                usage: None,
                outcome: Outcome::Completed {
                    context_tokens: None,
                },
            },
            notices: None,
            plan: None,
            rate_limit: None,
            tool_calls: Vec::new(),
        };
        if request.tools.is_empty() {
            return Ok(message);
        }
        if request.model.starts_with("fail/") {
            return Err(ProviderError::new("The model failed"));
        }
        on_delta(Delta::Content("Done.".into()));
        message.content = "Done.".into();
        message.finish_reason = Some("stop".into());
        message.telemetry = Telemetry {
            provider: ProviderId::OpenRouter,
            model: "diff/model".into(),
            effort: None,
            started_at: "2026-11-17T00:00:00.000Z".into(),
            wall_ms: 10,
            first_delta_ms: Some(5),
            context_bytes: 100,
            message_count: 2,
            usage: Some(Usage {
                input_tokens: Some(10.0),
                output_tokens: Some(5.0),
                cached_input_tokens: Some(0.0),
                cache_write_tokens: Some(0.0),
                reasoning_tokens: None,
            }),
            outcome: Outcome::Completed {
                context_tokens: Some(10.0),
            },
        };
        Ok(message)
    }
    async fn models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        Ok(Vec::new())
    }
    async fn model_info(&self, _: &str) -> Result<ModelFacts, ProviderError> {
        Ok(ModelFacts {
            image_input: Some(true),
            ..ModelFacts::unknown()
        })
    }
    async fn close_session(&self, _: &str) {}
}

struct Fake;

#[async_trait]
impl Backend for Fake {
    fn provider(&self, _: &str) -> Result<Arc<dyn Provider>, String> {
        Ok(Arc::new(Model))
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

/// What an operation's answer is compared by.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind {
    /// A bench made: its id.
    Made,
    /// A bench's details: the route's JSON text.
    Details,
    /// A round sent: `{ rounds }`.
    Rounds,
    /// A branch switch: `{ round }`.
    Branch,
    /// A vote: `{ votes }`.
    Votes,
    /// `{ ok: true }`.
    Ok,
    /// The leaderboard's body.
    Leaderboard,
    /// A fork: the new chat's id.
    Fork,
    /// The library as saved.
    Prompts,
}

/// The command of an operation, made from the ids as this side resolves
/// its placeholders.
type MakeCommand = Box<dyn Fn(&dyn Fn(&str) -> String) -> Command>;

/// One operation: today's request, the command that replaces it, and what
/// is compared.
struct Op {
    method: &'static str,
    path: String,
    body: Option<Value>,
    command: MakeCommand,
    kind: Kind,
}

fn choice(model: &str) -> BenchChoice {
    BenchChoice {
        provider: "openrouter".into(),
        model: model.into(),
        effort: None,
    }
}

fn file() -> OutgoingFile {
    OutgoingFile {
        name: "notes.txt".into(),
        content_type: "text/plain".into(),
        size: 5,
        data: b"hello".to_vec().into(),
    }
}

fn file_json() -> Value {
    json!({ "name": "notes.txt", "type": "text/plain", "size": 5, "data": "data:text/plain;base64,aGVsbG8=" })
}

fn message(target: Target, text: &str) -> Outgoing {
    Outgoing {
        target,
        text: text.into(),
        attachments: Vec::new(),
        selections: Vec::new(),
        prompts: Vec::new(),
        effort: None,
        resolution: Resolution::Standard,
        edit_of: None,
    }
}

fn library() -> Vec<LibraryPrompt> {
    vec![
        LibraryPrompt {
            id: "diff-1".into(),
            title: "A hut".into(),
            text: "Build a hut".into(),
            tags: vec!["houses".into(), "small".into()],
            favorite: true,
            attachments: Vec::new(),
            used_at: Some(1_794_873_600_000),
            added_by: None,
        },
        LibraryPrompt {
            id: "diff-2".into(),
            title: "A tower".into(),
            text: "Build a tower".into(),
            tags: Vec::new(),
            favorite: false,
            attachments: Vec::new(),
            used_at: None,
            added_by: None,
        },
    ]
}

/// The ops of the module's doc.
fn ops() -> Vec<Op> {
    let op = |method: &'static str,
              path: &str,
              body: Option<Value>,
              command: MakeCommand,
              kind: Kind| Op {
        method,
        path: path.into(),
        body,
        command,
        kind,
    };
    let get = |n: usize| {
        op(
            "GET",
            &format!("/api/benches/$bench:{n}"),
            None,
            Box::new(move |r| Command::ReadBench {
                bench: r(&format!("$bench:{n}")),
            }),
            Kind::Details,
        )
    };
    let vote = |round: &'static str, pick: Value| {
        let picked = pick.clone();
        op(
            "POST",
            "/api/benches/$bench:0/vote",
            Some(json!({ "roundId": round, "pick": pick })),
            Box::new(move |r| Command::Vote {
                bench: r("$bench:0"),
                round: r(round),
                pick: match &picked {
                    Value::Null => VotePick::TakeBack,
                    Value::String(tie) if tie == "tie" => VotePick::Tie,
                    Value::String(member) => VotePick::Member(r(member)),
                    other => panic!("a pick {other}"),
                },
            }),
            Kind::Votes,
        )
    };
    let leaderboard = || {
        op(
            "GET",
            "/api/leaderboard",
            None,
            Box::new(|_| Command::ReadLeaderboard),
            Kind::Leaderboard,
        )
    };
    let new_bench = |models: [&'static str; 2], blind: bool, extras: bool| {
        let members: Vec<Value> = models
            .iter()
            .map(|m| json!({ "provider": "openrouter", "model": m, "effort": "" }))
            .collect();
        let (files, prompts) = if extras {
            (
                vec![file_json()],
                json!([{ "id": "diff-1", "title": "A hut" }]),
            )
        } else {
            (Vec::new(), json!([]))
        };
        op(
            "POST",
            "/api/benches",
            Some(json!({ "members": members, "blind": blind, "resolution": 1,
                "text": "Build a hut", "files": files, "selections": [], "prompts": prompts })),
            Box::new(move |_| {
                let mut m = message(
                    Target::NewBench {
                        members: models.iter().map(|m| choice(m)).collect(),
                        blind,
                    },
                    "Build a hut",
                );
                if extras {
                    m.attachments = vec![file()];
                    m.prompts = vec![PromptLink {
                        id: "diff-1".into(),
                        title: "A hut".into(),
                    }];
                }
                Command::Send(m)
            }),
            Kind::Made,
        )
    };
    let round = |text: &'static str, edit_of: Option<&'static str>| {
        let mut body =
            json!({ "text": text, "files": [], "selections": [], "prompts": [], "resolution": 1 });
        if let Some(edit) = edit_of {
            body["editOf"] = json!(edit);
        }
        op(
            "POST",
            "/api/benches/$bench:0/rounds",
            Some(body),
            Box::new(move |r| {
                let mut m = message(Target::Bench(r("$bench:0")), text);
                m.edit_of = edit_of.map(|e| MessageId(r(e)));
                Command::Send(m)
            }),
            Kind::Rounds,
        )
    };
    vec![
        new_bench(["diff/a", "diff/b"], true, true),
        get(0),
        round("Add a door", None),
        get(0),
        round("A barn instead", Some("$round:0:0")),
        get(0),
        op(
            "POST",
            "/api/benches/$bench:0/branch",
            Some(json!({ "roundId": "$node:0:0" })),
            Box::new(|r| Command::BenchBranch {
                bench: r("$bench:0"),
                round: r("$node:0:0"),
            }),
            Kind::Branch,
        ),
        get(0),
        vote("$round:0:0", json!("$member:0:0")),
        vote("$round:0:1", json!("tie")),
        vote("nothing", json!("tie")),
        leaderboard(),
        vote("$round:0:1", Value::Null),
        vote("$round:0:0", json!("$member:0:1")),
        op(
            "POST",
            "/api/benches/$bench:0/reveal",
            None,
            Box::new(|r| Command::Reveal {
                bench: r("$bench:0"),
            }),
            Kind::Ok,
        ),
        vote("$round:0:0", json!("tie")),
        get(0),
        new_bench(["diff/c", "fail/d"], false, false),
        get(1),
        op(
            "POST",
            "/api/benches/$bench:1/retry",
            Some(json!({ "memberId": "$member:1:1" })),
            Box::new(|r| Command::RetryMember {
                bench: r("$bench:1"),
                member: r("$member:1:1"),
            }),
            Kind::Ok,
        ),
        op(
            "POST",
            "/api/benches/$bench:1/retry",
            Some(json!({ "memberId": "$member:1:0" })),
            Box::new(|r| Command::RetryMember {
                bench: r("$bench:1"),
                member: r("$member:1:0"),
            }),
            Kind::Ok,
        ),
        get(1),
        op(
            "POST",
            "/api/conversations/$member:0:0/fork",
            Some(json!({ "round": 1 })),
            Box::new(|r| Command::Fork {
                member: r("$member:0:0"),
                round: 1,
            }),
            Kind::Fork,
        ),
        op(
            "POST",
            "/api/conversations/$member:0:1/fork",
            Some(json!({ "round": 9 })),
            Box::new(|r| Command::Fork {
                member: r("$member:0:1"),
                round: 9,
            }),
            Kind::Fork,
        ),
        op(
            "PUT",
            "/api/prompts",
            Some(
                json!({ "prompts": library().iter().map(LibraryPrompt::to_json).collect::<Vec<_>>() }),
            ),
            Box::new(|_| Command::SavePrompts(library())),
            Kind::Prompts,
        ),
        op(
            "POST",
            "/api/benches",
            Some(
                json!({ "members": [{ "provider": "openrouter", "model": "diff/a", "effort": "" }],
                "blind": false, "resolution": 1, "text": "x", "files": [], "selections": [], "prompts": [] }),
            ),
            Box::new(|_| {
                Command::Send(message(
                    Target::NewBench {
                        members: vec![choice("diff/a")],
                        blind: false,
                    },
                    "x",
                ))
            }),
            Kind::Made,
        ),
        op(
            "POST",
            "/api/benches/$bench:0/rounds",
            Some(
                json!({ "text": "x", "files": [], "prompts": [], "resolution": 1,
                "selections": [{ "letter": "A", "label": "A", "min": [0, 0, 0], "max": [1, 1, 1], "blocks": 1, "block": "minecraft:stone" }] }),
            ),
            Box::new(|r| {
                let mut m = message(Target::Bench(r("$bench:0")), "x");
                m.selections = vec![Selection {
                    letter: "A".into(),
                    label: "A".into(),
                    min: [0, 0, 0],
                    max: [1, 1, 1],
                    blocks: 1,
                    block: Some("minecraft:stone".into()),
                }];
                Command::Send(m)
            }),
            Kind::Rounds,
        ),
        leaderboard(),
    ]
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

/// What these operations read of the fixture: the database, the prompt
/// library, and the worlds, images and files of its chats (the start reads
/// its benches' chats whole), each copied as `common::clone` copies (a
/// reflink). Its `settings.json` is not copied: no title model, no key.
fn light_clone(env: &common::Env, name: &str) -> PathBuf {
    let to = env.scratch.join(name);
    if to.exists() {
        std::fs::remove_dir_all(&to).unwrap();
    }
    std::fs::create_dir_all(&to).unwrap();
    for entry in [
        "prompts.json",
        "mcbench.sqlite3",
        "mcbench.sqlite3-wal",
        "mcbench.sqlite3-shm",
        "worlds",
        "images",
        "attachments",
    ] {
        let from = env.fixture.join(entry);
        if !from.exists() {
            continue;
        }
        let mut command = Process::new("cp");
        if cfg!(target_os = "macos") {
            command.arg("-c").arg("-R");
        } else {
            command.arg("-r").arg("--reflink=always");
        }
        let status = command.arg(&from).arg(to.join(entry)).status().unwrap();
        assert!(status.success(), "cp {}", from.display());
    }
    to
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

/// Node's answer as compared: `{ error }` or what `kind` compares.
fn node_answer(kind: Kind, answer: &Value) -> Value {
    let body = &answer["body"];
    if answer["status"] != 200 {
        return json!({ "error": body["error"] });
    }
    match kind {
        Kind::Made => json!({ "ok": body["id"] }),
        Kind::Details => json!({ "ok": answer["text"] }),
        Kind::Rounds => json!({ "ok": body["rounds"] }),
        Kind::Branch => json!({ "ok": body["round"] }),
        Kind::Votes => json!({ "ok": body["votes"] }),
        Kind::Ok => json!({ "ok": body["ok"] }),
        Kind::Leaderboard => json!({ "ok": body }),
        Kind::Fork => json!({ "ok": body["id"] }),
        Kind::Prompts => json!({ "ok": body["prompts"] }),
    }
}

/// The core's answer to one command, from what it published.
fn rust_answer(kind: Kind, updates: &[Update], branched_to: Option<&str>) -> Value {
    let refused = updates.iter().find_map(|u| match u {
        Update::Error(e)
        | Update::SendRefused { error: e, .. }
        | Update::Forked { chat: Err(e), .. }
        | Update::PromptsSaved(Err(e)) => Some(e.clone()),
        _ => None,
    });
    if let Some(error) = refused {
        return json!({ "error": error });
    }
    let last = |pick: &dyn Fn(&Update) -> Option<Value>| {
        updates
            .iter()
            .rev()
            .find_map(pick)
            .unwrap_or_else(|| panic!("no answer for {kind:?} in {updates:?}"))
    };
    let details = |u: &Update| match u {
        Update::Bench {
            details: Ok(details),
            ..
        } => Some(details.clone()),
        _ => None,
    };
    match kind {
        Kind::Made => last(&|u| match u {
            Update::BenchSent { bench, .. } => Some(json!({ "ok": bench })),
            _ => None,
        }),
        Kind::Rounds => last(&|u| match u {
            Update::BenchSent { round, .. } => Some(json!({ "ok": round })),
            _ => None,
        }),
        Kind::Details => last(&|u| {
            details(u).map(|d| json!({ "ok": mcbench_store::json::stringify(&d.to_json()) }))
        }),
        Kind::Branch => last(&|u| {
            details(u).map(|d| {
                let at = d
                    .rounds
                    .iter()
                    .position(|r| Some(r.id.as_str()) == branched_to)
                    .map_or(0, |i| i + 1);
                json!({ "ok": at })
            })
        }),
        Kind::Votes => last(&|u| {
            details(u)
                .map(|d| json!({ "ok": mcbench_lite::core::routes::benches::votes_json(&d.votes) }))
        }),
        Kind::Ok => json!({ "ok": true }),
        Kind::Leaderboard => last(&|u| match u {
            Update::Leaderboard(Ok(board)) => Some(json!({ "ok": {
                "entries": board.entries.iter().map(|e| json!({
                    "provider": e.provider, "model": e.model, "effort": e.effort, "wins": e.wins,
                    "votes": e.votes, "rating": mcbench_store::json::number(e.rating), "benches": e.benches,
                })).collect::<Vec<_>>(),
                "votes": board.votes,
            } })),
            _ => None,
        }),
        Kind::Fork => last(&|u| match u {
            Update::Forked { chat: Ok(chat), .. } => Some(json!({ "ok": chat })),
            _ => None,
        }),
        Kind::Prompts => last(&|u| match u {
            Update::PromptsSaved(Ok(library)) => Some(
                json!({ "ok": library.iter().map(LibraryPrompt::to_json).collect::<Vec<_>>() }),
            ),
            _ => None,
        }),
    }
}

/// A dump with the one thing the two sides may do in another order made
/// comparable: the models of a bench run at once, and each side's store
/// takes their messages in the order their turns reach it (Node's turns
/// all store their prompts before any answers; the core's first model may
/// answer before the next one's prompt is stored). So a message made in
/// this run (an id of `det.js`'s pattern) is named by its chat and its
/// place (`UNIQUE (conversation_id, position)`) wherever it appears; the
/// tables those messages are rows of (messages, the prompts they were sent
/// from, the search documents) are compared as sets of rows, a search
/// document without its rowid; the index's vocabulary by document source
/// instead of rowid; and the FTS5 table and its shadow tables (`search`,
/// `search_data`, `search_idx`, `search_docsize`, `search_config`), whose
/// rows and bytes follow the insertion order, are left out (the documents
/// and the vocabulary hold what they index). A bench model's chat's own
/// title document is compared by its source only (see the comment at it).
/// Every other table is compared row by row in order.
fn canonical(text: &str) -> Value {
    /// `conversations.bench_id`'s column.
    const BENCH_ID: usize = 11;
    let doc: Value = serde_json::from_str(text).unwrap();
    let names: std::collections::HashMap<String, String> = doc["tables"]["messages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| {
            let place = format!("msg@{}@{}", row[1].as_str().unwrap(), row[3]);
            (row[0].as_str().unwrap().to_string(), place.replace('"', ""))
        })
        .collect();
    let made = regex::Regex::new("[0-9a-f]{8}-0000-4000-8000-[0-9a-f]{12}").unwrap();
    let text = made.replace_all(text, |id: &regex::Captures| {
        names
            .get(&id[0])
            .cloned()
            .unwrap_or_else(|| id[0].to_string())
    });
    let mut doc: Value = serde_json::from_str(&text).unwrap();
    let sources: std::collections::HashMap<String, Value> = doc["tables"]["search_docs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| (row[0].to_string(), row[2].clone()))
        .collect();
    // A bench model's chat's title document is written by its chat's
    // saves, so it holds the bench's title only once the chat saved after
    // the title came (`setBenchTitle` writes the bench's own document only):
    // when that is depends on the model's speed, on both sides. Those
    // documents are compared by their source, not their text.
    let members: Vec<String> = doc["tables"]["conversations"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| made.is_match(row[0].as_str().unwrap()) && !row[BENCH_ID].is_null())
        .map(|row| format!("title:{}", row[0].as_str().unwrap()))
        .collect();
    let tables = doc["tables"].as_object_mut().unwrap();
    for name in [
        "search",
        "search_data",
        "search_idx",
        "search_docsize",
        "search_config",
    ] {
        tables.shift_remove(name);
    }
    for row in tables["search_docs"].as_array_mut().unwrap() {
        if members.iter().any(|m| row[2] == m.as_str()) {
            row[3] = Value::Null;
        }
    }
    let sorted = |rows: &mut Value, drop_first: bool| {
        let rows = rows.as_array_mut().unwrap();
        if drop_first {
            for row in rows.iter_mut() {
                row.as_array_mut().unwrap().remove(0);
            }
        }
        rows.sort_by_key(|row| row.to_string());
    };
    sorted(&mut tables["search_docs"], true);
    for name in ["messages", "message_prompts"] {
        sorted(&mut tables[name], false);
    }
    let vocabulary = doc["searchVocabulary"].as_array_mut().unwrap();
    for row in vocabulary.iter_mut() {
        let doc = row[1].to_string();
        row[1] = sources[&doc].clone();
    }
    vocabulary.retain(|row| !members.iter().any(|m| row[1] == m.as_str()));
    vocabulary.sort_by_key(|row| row.to_string());
    doc
}

#[test]
#[ignore = "needs MCBENCH_DIFF_DATA, MCBENCH_NODE_DIFF and MCBENCH_DIFF_SCRATCH (mcbench-agent's tests/common)"]
fn the_bench_and_library_commands_change_and_answer_as_todays_routes() {
    let env = common::env();
    for name in ["HOME", "CLAUDE_CONFIG_DIR", "CODEX_HOME"] {
        let value = std::env::var(name).unwrap_or_default();
        assert!(
            Path::new(&value).starts_with(&env.scratch),
            "{name} must be inside MCBENCH_DIFF_SCRATCH, so no sign-in is read"
        );
    }
    let node_data = light_clone(&env, "core-bench-node");
    let rust_data = light_clone(&env, "core-bench-rust");
    let ops = ops();

    // Node's side.
    let ops_file = env.scratch.join("core-bench-ops.json");
    let node_out = env.scratch.join("core-bench-node.json");
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
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/node/bench.js"),
        &[&ops_file, &node_out],
    );
    let theirs: Vec<Value> =
        serde_json::from_str(&std::fs::read_to_string(&node_out).unwrap()).unwrap();
    assert_eq!(theirs.len(), ops.len());

    // The core's side.
    let core = Core::start(Parts {
        data: rust_data.clone(),
        program: std::env::current_exe().unwrap(),
        host: common::DetHost::new(common::DET_NOW),
        shipped: Box::new(|| Err("unused".into())),
        client: mcbench_setup::fetch::client(),
        backend: Some(Arc::new(Fake)),
        installer: None,
        share: None,
        live: Default::default(),
    })
    .unwrap();
    let window = Arc::new(Recording::default());
    mcbench_ui::Core::attach(&core, window.clone());
    let handle = core.handle().clone();
    handle.block_on(core.started());
    let store = Arc::clone(core.store());
    let benches: Arc<Mutex<Vec<String>>> = Arc::default();

    let mut differences = Vec::new();
    for (index, (op, node)) in ops.iter().zip(&theirs).enumerate() {
        let resolve = |text: &str| -> String {
            let benches = benches.lock().unwrap();
            let parts: Vec<&str> = text.split(':').collect();
            let n = |i: usize| parts[i].parse::<usize>().unwrap();
            match parts[0] {
                "$bench" => benches[n(1)].clone(),
                "$round" => store.bench(&benches[n(1)]).unwrap().unwrap().rounds[n(2)]["id"]
                    .as_str()
                    .unwrap()
                    .to_string(),
                "$node" => store.bench(&benches[n(1)]).unwrap().unwrap().nodes[n(2)]["id"]
                    .as_str()
                    .unwrap()
                    .to_string(),
                "$member" => store.bench_members(&benches[n(1)]).unwrap()[n(2)]
                    .lock()
                    .unwrap()
                    .id
                    .0
                    .clone(),
                _ => text.to_string(),
            }
        };
        let command = (op.command)(&resolve);
        let branched_to = match &command {
            Command::BenchBranch { round, .. } => Some(round.clone()),
            _ => None,
        };
        let from = window.0.lock().unwrap().len();
        let idle_benches = Arc::clone(&benches);
        handle
            .block_on(handle.spawn({
                let core = core.clone();
                async move {
                    core.command(command).await;
                    // As `bench.js` waits: until nothing is busy.
                    loop {
                        let ids = idle_benches.lock().unwrap().clone();
                        for id in ids {
                            for member in core.store().bench_members(&id).unwrap() {
                                let member = member.lock().unwrap().id.0.clone();
                                core.turns().free(&member).await;
                            }
                        }
                        core.turns().titles_settled().await;
                        if !core.busy().await.unwrap() {
                            break;
                        }
                    }
                }
            }))
            .unwrap();
        let updates = window.0.lock().unwrap()[from..].to_vec();
        if op.kind == Kind::Made {
            for u in &updates {
                if let Update::BenchSent { bench, .. } = u {
                    benches.lock().unwrap().push(bench.clone());
                }
            }
        }
        let ours = rust_answer(op.kind, &updates, branched_to.as_deref());
        let expected = node_answer(op.kind, node);
        println!(
            "op {index}: {} {} -> {} {}",
            op.method,
            op.path,
            node["status"],
            ours.to_string().chars().take(100).collect::<String>()
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
    let node_dump = env.scratch.join("core-bench-node-dump.json");
    let rust_dump = env.scratch.join("core-bench-rust-dump.json");
    node(&env, &node_data, &dump, &[&node_data, &node_dump]);
    node(&env, &rust_data, &dump, &[&rust_data, &rust_dump]);
    let node_doc = canonical(&std::fs::read_to_string(&node_dump).unwrap());
    let rust_doc = canonical(&std::fs::read_to_string(&rust_dump).unwrap());
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
    for key in ["files", "searchVocabulary", "sqlite_master", "user_version"] {
        if node_doc[key] != rust_doc[key] {
            differences.push(first_difference(key, &node_doc[key], &rust_doc[key]));
        }
    }
    println!(
        "core bench: {answered} operations, {rows} rows, {} differences",
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
