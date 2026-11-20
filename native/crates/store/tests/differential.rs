//! The differential proof against today's Node store (`server/store.js`).
//!
//! `MCBENCH_DIFF_DATA=<schema-14 data folder>` names the fixture (it is
//! cloned, never opened; it must have no `settings.json`, `auth-token` or
//! `backups`, which hold real keys: `fixture::guard` refuses it otherwise),
//! `MCBENCH_NODE_DIFF=<checkout>` the checkout whose
//! `server/` and `node_modules/` (better-sqlite3) run, `MCBENCH_DIFF_SCRATCH`
//! where the clones go (they are removed at the end unless the test fails).
//! With the variables unset the test is ignored; with them set, anything
//! missing fails it.
//!
//! Each clone of the fixture is made without those three entries and gets
//! `fixture::MADE_UP_SETTINGS` as its `settings.json`. The settings are
//! compared only by `settings_summary` (the sha256 of their canonical JSON
//! and of each field, by field path) and the file by its sha256: nothing
//! this test writes or prints holds a settings value.
//!
//! 1. Reads: the read document (`tests/node/read.js`) of one clone through
//!    Node and of another through this crate are byte-identical.
//! 2. Writes: `tests/node/gen.js`'s script runs through Node on one clone and
//!    through this crate on another; every op's result is the same, every
//!    table row by row and the files under images/, attachments/ and worlds/
//!    and the settings (by sha256) and prompts files are the same
//!    (`tests/node/dump.js`),
//!    Node reads the Rust-written folder to the document it reads from the
//!    Node-written one, and this crate reads the Node-written folder to it.
//! 3. A fresh folder this crate creates: Node opens it without migrating
//!    and both dumps and read documents equal those of a folder Node created.

mod common;
#[path = "../../agent/tests/common/fixture.rs"]
mod fixture;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use common::DetHost;
use mcbench_store::json::{self, Json, Truthy};
use mcbench_store::{
    Attachment, Bench, Content, Error, ImageData, LibraryPrompt, NewBench, NewConversation,
    NewMessage, PromptLibrary, PromptLink, Reply, SearchScope, SettingsFile, Store, Telemetry,
    ToolCall, ToolTelemetry, World, object, to_json,
};
use mcbench_world::BlockState;

const T0: i64 = 1794873600000;
const CHAT_MAX_BYTES: u64 = 256 * 1024 * 1024;

struct Env {
    fixture: PathBuf,
    node_checkout: PathBuf,
    scratch: PathBuf,
    scripts: PathBuf,
}

fn env() -> Env {
    let var = |name: &str| std::env::var(name).unwrap_or_else(|_| panic!("{name} must be set"));
    let env = Env {
        fixture: PathBuf::from(var("MCBENCH_DIFF_DATA")),
        node_checkout: PathBuf::from(var("MCBENCH_NODE_DIFF")),
        scratch: PathBuf::from(var("MCBENCH_DIFF_SCRATCH")),
        scripts: Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("node"),
    };
    fixture::guard(&env.fixture);
    assert!(
        env.fixture.join("mcbench.sqlite3").exists(),
        "no database in {}",
        env.fixture.display()
    );
    assert!(
        env.node_checkout.join("server").join("store.js").exists(),
        "no server/store.js in {}",
        env.node_checkout.display()
    );
    std::fs::create_dir_all(&env.scratch).unwrap();
    env
}

/// A clone of `from` at `scratch/name` without `fixture::LEFT_OUT`. A
/// clone of the fixture gets the made-up settings; a clone of a folder this
/// test wrote carries that folder's own settings file, when it has one.
fn clone(env: &Env, from: &Path, name: &str) -> PathBuf {
    let to = env.scratch.join(name);
    fixture::clone(from, &to);
    if from == env.fixture {
        fixture::write_made_up_settings(&to);
    } else {
        assert!(
            from.starts_with(&env.scratch),
            "{} is not this test's",
            from.display()
        );
        if from.join("settings.json").exists() {
            std::fs::copy(from.join("settings.json"), to.join("settings.json")).unwrap();
        }
    }
    to
}

fn node(env: &Env, data: Option<&Path>, script: &str, args: &[&Path]) {
    let mut command =
        Command::new(std::env::var("MCBENCH_NODE").unwrap_or_else(|_| "node".to_string()));
    command
        .arg("-r")
        .arg(env.scripts.join("det.js"))
        .arg(env.scripts.join(script))
        .args(args)
        .env("MCBENCH_NODE_DIFF", &env.node_checkout)
        .env("MCBENCH_CHAT_MAX_BYTES", CHAT_MAX_BYTES.to_string())
        .env("MCBENCH_TOOLS_DIR", env.scratch.join("tools-empty"))
        .env_remove("MCBENCH_SETTINGS_FILE")
        .env_remove("MCBENCH_DISK_RESERVE_BYTES")
        .env_remove("MCBENCH_REPORTS_KEPT");
    match data {
        Some(data) => command.env("MCBENCH_DATA_DIR", data),
        None => command.env_remove("MCBENCH_DATA_DIR"),
    };
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "node {script}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn open(root: &Path, host: Arc<DetHost>) -> (Store, SettingsFile, PromptLibrary) {
    let store = common::open(root, host.clone(), common::limits(CHAT_MAX_BYTES)).unwrap();
    let settings = SettingsFile::open(root.join("settings.json")).unwrap();
    let prompts = PromptLibrary::new(root.join("prompts.json"), host);
    (store, settings, prompts)
}

fn attempt(result: Result<Json, Error>) -> Json {
    match result {
        Ok(value) => object! { "ok" => value },
        Err(error) => object! { "error" => error.message, "status" => error.status.map(u32::from) },
    }
}

fn bench_json(bench: &Bench) -> Json {
    let mut votes = json::Object::new();
    for (round, vote) in &bench.votes {
        votes.insert(
            (round).to_string(),
            object! { "pick" => vote.pick.clone(), "blind" => vote.blind },
        );
    }
    object! {
        "id" => bench.id.as_str(),
        "title" => bench.title.as_str(),
        "createdAt" => bench.created_at.as_str(),
        "head" => bench.head.clone(),
        "nodes" => Json::Array(bench.nodes.clone()),
        "rounds" => Json::Array(bench.rounds.clone()),
        "blind" => bench.blind,
        "revealed" => bench.revealed,
        "order" => bench.order.clone(),
        "votes" => votes,
    }
}

fn attachment_json(file: &Attachment) -> Json {
    to_json(file)
}

fn relative(root: &Path, file: &Path) -> String {
    file.strip_prefix(root)
        .unwrap()
        .to_string_lossy()
        .into_owned()
}

fn sha(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let digest = <sha2::Sha256 as sha2::Digest>::digest(bytes);
    digest.iter().fold(String::new(), |mut out, b| {
        write!(out, "{b:02x}").unwrap();
        out
    })
}

/// The canonical JSON of `value`: object keys sorted, no spaces
/// (`read.js`'s `canonical`).
fn canonical(value: &Json) -> String {
    match value {
        Json::Object(object) => {
            let mut keys: Vec<&String> = object.keys().collect();
            keys.sort();
            let fields: Vec<String> = keys
                .iter()
                .map(|key| {
                    format!(
                        "{}:{}",
                        json::stringify(&Json::String((*key).clone())),
                        canonical(&object[key.as_str()])
                    )
                })
                .collect();
            format!("{{{}}}", fields.join(","))
        }
        Json::Array(items) => format!(
            "[{}]",
            items.iter().map(canonical).collect::<Vec<_>>().join(",")
        ),
        other => json::stringify(other),
    }
}

/// What the test keeps of a settings value (`read.js`'s
/// `settingsSummary`): the sha256 of its canonical JSON, and per field path
/// (`providers.openai.apiKey`, `[0]`), sorted, the sha256 of that field's
/// canonical JSON. Never a value.
fn settings_summary(value: &Json) -> Json {
    fn leaves(value: &Json, at: &str, out: &mut Vec<(String, String)>) {
        match value {
            Json::Object(object) if !object.is_empty() => {
                for (key, field) in object {
                    let path = if at.is_empty() {
                        key.clone()
                    } else {
                        format!("{at}.{key}")
                    };
                    leaves(field, &path, out);
                }
            }
            Json::Array(items) if !items.is_empty() => {
                for (i, item) in items.iter().enumerate() {
                    leaves(item, &format!("{at}[{i}]"), out);
                }
            }
            leaf => out.push((at.to_string(), sha(canonical(leaf).as_bytes()))),
        }
    }
    let mut fields = Vec::new();
    leaves(value, "", &mut fields);
    fields.sort();
    let mut object = json::Object::new();
    for (path, digest) in fields {
        object.insert(path, Json::String(digest));
    }
    object! { "sha256" => sha(canonical(value).as_bytes()), "fields" => Json::Object(object) }
}

/// `read.js`'s document, through this crate.
fn read_document(root: &Path, inputs: &Path) -> String {
    let host = DetHost::new(T0);
    let (store, settings, prompts) = open(root, host.clone());
    let queries: Vec<String> = match json::parse(
        &std::fs::read_to_string(inputs.join("queries.json")).unwrap(),
    )
    .unwrap()
    {
        Json::Array(items) => items
            .iter()
            .map(|q| q.as_str().unwrap().to_string())
            .collect(),
        _ => panic!("queries"),
    };
    let urls: Vec<String> =
        match json::parse(&std::fs::read_to_string(inputs.join("urls.json")).unwrap()).unwrap() {
            Json::Array(items) => items
                .iter()
                .map(|q| q.as_str().unwrap().to_string())
                .collect(),
            _ => panic!("urls"),
        };
    let mut doc = json::Object::new();
    let conversations: Vec<Json> = store
        .list_conversations()
        .unwrap()
        .into_iter()
        .map(|row| {
            object! {
                "id" => row.id.0, "title" => row.title, "provider" => row.provider, "model" => row.model,
                "effort" => row.effort, "resolution" => row.resolution, "createdAt" => row.created_at,
                "benchId" => row.bench_id, "benchSlot" => row.bench_slot, "unread" => row.unread,
                "messageCount" => row.message_count, "prompts" => row.prompts,
            }
        })
        .collect();
    doc.insert("conversations".into(), Json::Array(conversations));
    doc.insert(
        "promptTitles".into(),
        Json::Object(store.prompt_titles().unwrap()),
    );
    let ids = store.list_conversation_ids().unwrap();
    let bench_ids = store.list_bench_ids().unwrap();
    let id_rows = |ids: &[(String, bool)]| {
        Json::Array(
            ids.iter()
                .map(|(id, archived)| object! { "id" => id.as_str(), "archived" => *archived })
                .collect(),
        )
    };
    doc.insert("conversationIds".into(), id_rows(&ids));
    doc.insert("benchIds".into(), id_rows(&bench_ids));
    let archive = store.list_archive().unwrap();
    doc.insert("archive".into(), object! {
            "conversations" => Json::Array(archive.conversations.iter().map(|row| object! {
                "id" => row.id.as_str(), "title" => row.title.as_str(), "provider" => row.provider.as_str(), "model" => row.model.as_str(),
                "archivedAt" => row.archived_at, "expiresAt" => row.expires_at,
            }).collect()),
            "benches" => Json::Array(archive.benches.iter().map(|row| object! {
                "id" => row.id.as_str(), "title" => row.title.as_str(), "archivedAt" => row.archived_at, "expiresAt" => row.expires_at, "models" => row.models,
            }).collect()),
        });
    let mut times = vec![0, T0];
    for at in archive
        .conversations
        .iter()
        .map(|row| row.expires_at)
        .chain(archive.benches.iter().map(|row| row.expires_at))
    {
        times.extend([at - 1, at, at + 1]);
    }
    doc.insert(
        "nextArchiveExpiry".into(),
        Json::Array(
            times
                .iter()
                .map(|now| {
                    Json::Array(vec![
                        (*now).into(),
                        store.next_archive_expiry(*now).unwrap().into(),
                    ])
                })
                .collect(),
        ),
    );
    doc.insert(
        "benches".into(),
        Json::Array(
            store
                .list_benches()
                .unwrap()
                .iter()
                .map(mcbench_store::ListedBench::to_json)
                .collect(),
        ),
    );
    let mut benches = json::Object::new();
    let mut members = json::Object::new();
    for (id, _) in &bench_ids {
        benches.insert(
            (id).to_string(),
            store
                .bench(id)
                .unwrap()
                .as_ref()
                .map_or(Json::Null, bench_json),
        );
        let ids: Vec<String> = store
            .bench_members(id)
            .unwrap()
            .iter()
            .map(|m| m.lock().unwrap().id.0.clone())
            .collect();
        members.insert((id).to_string(), ids.into());
    }
    doc.insert("bench".into(), Json::Object(benches));
    doc.insert("benchMembers".into(), Json::Object(members));
    doc.insert(
        "leaderboard".into(),
        Json::Array(
            store
                .leaderboard()
                .unwrap()
                .iter()
                .map(leaderboard_row)
                .collect(),
        ),
    );
    doc.insert("voteCount".into(), store.vote_count().unwrap().into());
    let mut chats = json::Object::new();
    for (id, _) in &ids {
        let Some(live) = store.conversation(id).unwrap() else {
            chats.insert((id).to_string(), Json::Null);
            continue;
        };
        let chat = live.lock().unwrap();
        let roots: Vec<_> = chat
            .messages
            .iter()
            .filter(|m| m.message.is_prompt() && m.parent.is_none())
            .collect();
        let tree = store.message_tree(&chat).unwrap();
        let mut checkpoints: Vec<(String, Json)> = chat
            .checkpoints
            .iter()
            .map(|(key, world)| (key.0.clone(), world_json(world)))
            .collect();
        checkpoints.sort_by(|a, b| a.0.cmp(&b.0));
        let header = chat.header();
        let entry = object! {
            "header" => object! {
                "id" => header.id, "title" => header.title, "provider" => header.provider, "model" => header.model,
                "effort" => header.effort, "resolution" => header.resolution,
                "benchId" => header.bench.as_ref().map(|b| b.0.clone()), "benchSlot" => header.bench.as_ref().map(|b| b.1),
                "createdAt" => chat.created_at.as_str(),
            },
            "messages" => Json::Array(chat.messages.iter().map(|m| Json::String(m.in_memory_text())).collect()),
            "publicMessages" => format!("[{}]", chat.messages.iter().map(|m| m.public_text()).collect::<Vec<_>>().join(",")),
            "messageTree" => tree_json(&tree),
            "currentMessageId" => chat.current_message_id.as_ref().map(|m| m.0.clone()),
            "nextMessagePosition" => chat.next_message_position,
            "world" => world_json(&chat.world),
            "checkpoints" => Json::Array(checkpoints.into_iter().map(|(k, v)| Json::Array(vec![k.into(), v])).collect()),
            "benchProgress" => attempt(store.bench_progress(id).map(|p| object! { "round" => p.round, "halted" => p.halted })),
            "reportReferences" => store.report_references(id).unwrap(),
            "repeatsRootPrompt" => match roots.last() {
                Some(root) => match &root.message {
                    mcbench_store::Message::User(user) => attempt(store.repeats_latest_root_prompt(&chat, &user.content).map(Json::Bool)),
                    _ => unreachable!(),
                },
                None => Json::Null,
            },
        };
        drop(chat);
        chats.insert((id).to_string(), entry);
    }
    doc.insert("chats".into(), Json::Object(chats));
    let mut used = json::Object::new();
    let mut refusal = json::Object::new();
    for (id, _) in &ids {
        used.insert(
            (id).to_string(),
            attempt(store.storage().used(id).map(Json::from)),
        );
        refusal.insert(
            (id).to_string(),
            attempt(store.storage().refusal(id).map(Json::from)),
        );
    }
    doc.insert("used".into(), Json::Object(used));
    doc.insert("refusal".into(), Json::Object(refusal));
    let hits = |scope, query: &str| {
        attempt(store.search(scope, query).map(|hits| {
            Json::Array(
                hits.into_iter()
                    .map(|hit| object! { "id" => hit.id, "snippet" => hit.snippet })
                    .collect(),
            )
        }))
    };
    doc.insert(
        "search".into(),
        Json::Array(
            queries
                .iter()
                .map(|q| {
                    Json::Array(vec![
                        q.as_str().into(),
                        hits(SearchScope::Build, q),
                        hits(SearchScope::Bench, q),
                    ])
                })
                .collect(),
        ),
    );
    doc.insert(
        "files".into(),
        Json::Array(
            urls.iter()
                .map(|url| {
                    let images = url.split('/').nth(2) == Some("images");
                    let file = if images {
                        store.image_file(url)
                    } else {
                        store.attachment_file(url)
                    };
                    let bytes = if images {
                        store.read_image(url)
                    } else {
                        store.read_attachment(url)
                    };
                    Json::Array(vec![
                        url.as_str().into(),
                        attempt(file.map(|file| relative(root, &file).into())),
                        // A file that cannot be read: today's message is
                        // Node's fs error, this crate's the OS error.
                        match bytes {
                            Ok(bytes) => object! { "ok" => sha(&bytes) },
                            Err(_) => object! { "error" => "unreadable" },
                        },
                    ])
                })
                .collect(),
        ),
    );
    doc.insert(
        "prompts".into(),
        attempt(
            prompts
                .list()
                .map(|list| Json::Array(list.iter().map(LibraryPrompt::to_json).collect())),
        ),
    );
    doc.insert("settings".into(), settings_summary(&settings.get().0));
    let limits = store.storage().limits();
    doc.insert("limits".into(), object! { "chatMaxBytes" => limits.chat_max_bytes as i64, "diskReserveBytes" => limits.disk_reserve_bytes as i64, "reportsKept" => limits.reports_kept as i64 });
    json::stringify(&Json::Object(doc))
}

/// A world as the read document shows it: its manifest, blocks and bounds.
fn world_json(world: &World) -> Json {
    object! {
        "manifest" => manifest(world),
        "size" => world.size(),
        "bounds" => world.bounds().map(|b| object! { "min" => b.min.to_vec(), "max" => b.max.to_vec() }),
    }
}

fn manifest(world: &World) -> String {
    world
        .persist(&mut |_, _| panic!("a loaded world has every chunk stored"))
        .unwrap()
        .0
}

fn tree_json(tree: &mcbench_store::MessageTree) -> Json {
    object! {
        "headId" => tree.head_id.clone(),
        "messages" => Json::Array(tree.messages.iter().map(|m| object! {
            "id" => m.id.as_str(), "parentId" => m.parent_id.clone(), "position" => m.position, "role" => m.role.clone(), "branchable" => m.branchable,
        }).collect()),
    }
}

fn leaderboard_row(entry: &mcbench_store::LeaderboardEntry) -> Json {
    object! {
        "provider" => entry.provider.as_str(), "model" => entry.model.as_str(), "effort" => entry.effort.clone(),
        "wins" => entry.wins, "votes" => entry.votes, "rating" => entry.rating, "benches" => entry.benches,
    }
}

/// Node reads its `read.js` document of a fresh clone of `folder`.
fn node_read(env: &Env, folder: &Path, inputs: &Path, name: &str) -> String {
    let data = clone(env, folder, name);
    let out = env.scratch.join(format!("{name}.json"));
    node(env, Some(&data), "read.js", &[inputs, &out]);
    std::fs::read_to_string(out).unwrap()
}

fn rust_read(env: &Env, folder: &Path, inputs: &Path, name: &str) -> String {
    let data = clone(env, folder, name);
    let doc = read_document(&data, inputs);
    std::fs::write(env.scratch.join(format!("{name}.json")), &doc).unwrap();
    doc
}

fn dump(env: &Env, folder: &Path, name: &str) -> String {
    let out = env.scratch.join(format!("{name}.json"));
    let mut command =
        Command::new(std::env::var("MCBENCH_NODE").unwrap_or_else(|_| "node".to_string()));
    let output = command
        .arg(env.scripts.join("dump.js"))
        .arg(folder)
        .arg(&out)
        .env("MCBENCH_NODE_DIFF", &env.node_checkout)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "dump.js: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    std::fs::read_to_string(out).unwrap()
}

/// The first place two JSON documents differ, by path.
fn first_difference(a: &Json, b: &Json, at: &str) -> Option<String> {
    match (a, b) {
        (Json::Object(x), Json::Object(y)) => {
            let keys_x: Vec<&str> = x.keys().map(String::as_str).collect();
            let keys_y: Vec<&str> = y.keys().map(String::as_str).collect();
            for (key, value) in x.iter() {
                match y.get(key) {
                    Some(other) => {
                        if let Some(found) = first_difference(value, other, &format!("{at}.{key}"))
                        {
                            return Some(found);
                        }
                    }
                    None => return Some(format!("{at}.{key}: only on the left")),
                }
            }
            if keys_x != keys_y {
                return Some(format!("{at}: keys {keys_x:?} against {keys_y:?}"));
            }
            None
        }
        (Json::Array(x), Json::Array(y)) => {
            for (i, (p, q)) in x.iter().zip(y).enumerate() {
                if let Some(found) = first_difference(p, q, &format!("{at}[{i}]")) {
                    return Some(found);
                }
            }
            (x.len() != y.len()).then(|| format!("{at}: {} items against {}", x.len(), y.len()))
        }
        (Json::String(x), Json::String(y))
            if x != y && (x.starts_with('{') || x.starts_with('[')) =>
        {
            match (json::parse(x), json::parse(y)) {
                (Ok(p), Ok(q)) if p != q => first_difference(&p, &q, &format!("{at}(text)"))
                    .or_else(|| Some(format!("{at}: text {x:.300} against {y:.300}"))),
                _ => Some(format!("{at}: {x:.300} against {y:.300}")),
            }
        }
        _ if a != b => Some(format!(
            "{at}: {:.300} against {:.300}",
            json::stringify(a),
            json::stringify(b)
        )),
        _ => None,
    }
}

fn same(left: &str, right: &str, what: &str) {
    if left == right {
        eprintln!("{what}: identical ({} bytes)", left.len());
        return;
    }
    let found = first_difference(
        &json::parse(left).unwrap(),
        &json::parse(right).unwrap(),
        "$",
    );
    panic!(
        "{what} differ: {}",
        found.unwrap_or_else(|| "in formatting only".to_string())
    );
}

// ---- the write script through this crate -----------------------------------

struct Runner<'a> {
    store: &'a Store,
    settings: &'a SettingsFile,
    prompts: &'a PromptLibrary,
    host: Arc<DetHost>,
    vars: HashMap<String, Json>,
    benches: HashMap<String, Bench>,
}

fn decode<T: serde::de::DeserializeOwned>(value: &Json) -> T {
    serde_json::from_str(&json::stringify(value))
        .unwrap_or_else(|e| panic!("{e}: {}", json::stringify(value)))
}

impl Runner<'_> {
    fn resolve(&self, value: &Json) -> Json {
        match value {
            Json::String(text) if text.starts_with('$') => {
                if let Some(open) = text.rfind('[')
                    && text.ends_with(']')
                {
                    let index: usize = text[open + 1..text.len() - 1].parse().unwrap();
                    return self
                        .resolve(&Json::String(text[..open].to_string()))
                        .as_array()
                        .unwrap()[index]
                        .clone();
                }
                self.vars
                    .get(text)
                    .cloned()
                    .unwrap_or_else(|| panic!("unknown variable {text}"))
            }
            Json::Array(items) => {
                Json::Array(items.iter().map(|item| self.resolve(item)).collect())
            }
            Json::Object(object) => Json::Object(
                object
                    .iter()
                    .map(|(k, v)| (k.to_string(), self.resolve(v)))
                    .collect(),
            ),
            other => other.clone(),
        }
    }

    fn text(&self, op: &Json, key: &str) -> String {
        self.resolve(op.get(key).unwrap())
            .as_str()
            .unwrap()
            .to_string()
    }

    fn chat(&self, id: &str) -> Result<mcbench_store::LiveConversation, Error> {
        self.store
            .conversation(id)?
            .ok_or_else(|| Error::plain(format!("no chat {id}")))
    }

    fn remember(&mut self, op: &Json, value: Json) -> Json {
        if let Some(Json::String(name)) = op.get("as") {
            self.vars.insert(name.clone(), value.clone());
        }
        value
    }

    /// `world.clone()`, each block of `blocks` (`[x, y, z, id, states]`)
    /// set, `finalize()`: a build, through `World`'s own calls.
    fn built(base: &World, blocks: &Json) -> Result<World, Error> {
        let mut world = base.clone_world()?;
        for block in blocks.as_array().unwrap() {
            let block = block.as_array().unwrap();
            let at = |i: usize| block[i].as_f64().unwrap() as i32;
            let states: Vec<(String, String)> = block[4]
                .as_object()
                .unwrap()
                .iter()
                .map(|(k, v)| (k.clone(), v.as_str().unwrap().to_string()))
                .collect();
            let states: Vec<(&str, &str)> = states
                .iter()
                .map(|(k, v)| (k.as_str(), v.as_str()))
                .collect();
            world.set(
                [at(0), at(1), at(2)],
                BlockState::new(block[3].as_str().unwrap(), &states),
            )?;
        }
        world.finalize();
        Ok(world)
    }

    fn append(&mut self, chat: &str, message: NewMessage, checkpoint: bool) -> Result<Json, Error> {
        let live = self.chat(chat)?;
        let mut conversation = live.lock().unwrap();
        let world = checkpoint.then(|| conversation.world.reference());
        let message = self
            .store
            .append_message(&mut conversation, message, world)?;
        self.vars
            .insert("$last".to_string(), message.id.0.clone().into());
        Ok(message.in_memory_text().into())
    }

    fn reply(value: &Json) -> NewMessage {
        let field = |key: &str| value.get(key).filter(|v| !matches!(v, Json::Null));
        NewMessage::reply(&Reply {
            content: field("content")
                .and_then(Json::as_str)
                .unwrap_or("")
                .to_string(),
            reasoning: field("reasoning")
                .and_then(Json::as_str)
                .map(str::to_string),
            reasoning_details: field("reasoning_details").map(decode),
            finish_reason: field("finish_reason")
                .and_then(Json::as_str)
                .map(str::to_string),
            telemetry: field("telemetry").map(decode::<Telemetry>),
            notices: field("notices").map(decode),
            plan: field("plan").map(decode),
            rate_limit: field("rateLimit").map(decode),
            tool_calls: field("tool_calls")
                .map(decode::<Vec<ToolCall>>)
                .unwrap_or_default(),
        })
    }

    fn last(value: &Json) -> NewMessage {
        let content = value.get("content").unwrap().as_str().unwrap();
        let telemetry: Option<Telemetry> =
            value.get("telemetry").filter(|t| t.truthy()).map(decode);
        match value.get("kind").unwrap().as_str().unwrap() {
            "stopped" => NewMessage::stopped(
                content,
                value.get("reply").and_then(Json::as_str),
                telemetry.as_ref(),
            ),
            _ => NewMessage::error(content, telemetry.as_ref()),
        }
    }

    fn branch(conversation: &mcbench_store::Conversation) -> Json {
        object! { "head" => conversation.current_message_id.as_ref().map(|m| m.0.clone()), "messages" => conversation.messages.len() }
    }

    fn member(&self, op: &Json, m: i64) -> String {
        let bench = op.get("bench").unwrap().as_str().unwrap();
        self.vars
            .get(&format!("{bench}.member{m}"))
            .unwrap()
            .as_str()
            .unwrap()
            .to_string()
    }

    fn bench_key(op: &Json) -> String {
        op.get("bench").unwrap().as_str().unwrap().to_string()
    }

    fn run(&mut self, op: &Json) -> Result<Json, Error> {
        let kind = op.get("op").unwrap().as_str().unwrap();
        let number = |key: &str| op.get(key).and_then(Json::as_f64).unwrap() as i64;
        let store = self.store;
        Ok(match kind {
            "now" => {
                self.host.set_now(number("ms"));
                Json::Null
            }
            "get" => Self::branch(&self.chat(&self.text(op, "chat"))?.lock().unwrap()),
            "create" => {
                let live = store.create_conversation(
                    op.get("provider").unwrap().as_str().unwrap(),
                    op.get("model").unwrap().as_str().unwrap(),
                    NewConversation {
                        effort: op.get("effort").and_then(Json::as_str).map(str::to_string),
                        resolution: op.get("resolution").and_then(Json::as_f64).unwrap_or(1.0)
                            as u32,
                        ..NewConversation::default()
                    },
                )?;
                let id = live.lock().unwrap().id.0.clone();
                self.remember(op, id.into())
            }
            "prompt" => {
                let content: Content = decode(&self.resolve(op.get("content").unwrap()));
                let links: Vec<PromptLink> = decode(&self.resolve(op.get("prompts").unwrap()));
                let text = self.append(
                    &self.text(op, "chat"),
                    NewMessage::prompt(&content, &links),
                    true,
                )?;
                let last = self.vars["$last"].clone();
                self.remember(op, last);
                text
            }
            "reply" => self.append(
                &self.text(op, "chat"),
                Self::reply(op.get("reply").unwrap()),
                false,
            )?,
            "toolResult" => {
                let telemetry: ToolTelemetry = decode(op.get("telemetry").unwrap());
                let message = NewMessage::tool_result(
                    &self.text(op, "callId"),
                    &telemetry,
                    &self.text(op, "content"),
                );
                self.append(&self.text(op, "chat"), message, false)?
            }
            "unanswered" => {
                let stopped = op.get("stopped") == Some(&Json::Bool(true));
                let message = NewMessage::unanswered(
                    &self.text(op, "callId"),
                    stopped,
                    &self.text(op, "reason"),
                );
                self.append(&self.text(op, "chat"), message, false)?
            }
            "screenshots" => {
                let urls: Vec<String> = decode(&self.resolve(op.get("urls").unwrap()));
                self.append(
                    &self.text(op, "chat"),
                    NewMessage::screenshots(&self.text(op, "text"), &urls),
                    false,
                )?
            }
            "save" => {
                store.save(&mut self.chat(&self.text(op, "chat"))?.lock().unwrap())?;
                Json::Null
            }
            "build" | "buildMember" => {
                let id = if kind == "build" {
                    self.text(op, "chat")
                } else {
                    self.member(op, number("member"))
                };
                let live = self.chat(&id)?;
                let mut conversation = live.lock().unwrap();
                conversation.world = Self::built(&conversation.world, op.get("blocks").unwrap())?;
                Json::from(conversation.world.size())
            }
            "persistWorld" => {
                let id = self.text(op, "chat");
                let live = self.chat(&id)?;
                let conversation = live.lock().unwrap();
                let candidate = Self::built(&conversation.world, op.get("blocks").unwrap())?;
                store.persist_world(&id, &candidate)?;
                Json::Null
            }
            "revert" => {
                let live = self.chat(&self.text(op, "chat"))?;
                let message = store.revert(&mut live.lock().unwrap(), &self.text(op, "message"))?;
                message.in_memory_text().into()
            }
            "editPrompt" => {
                let live = self.chat(&self.text(op, "chat"))?;
                let mut conversation = live.lock().unwrap();
                let content = Content::Text(self.text(op, "content"));
                let mut appended = None;
                store.edit_prompt(
                    &mut conversation,
                    &self.text(op, "message"),
                    |conversation| {
                        let world = conversation.world.clone();
                        let message = store.append_message(
                            conversation,
                            NewMessage::prompt(&content, &[]),
                            Some(world),
                        )?;
                        appended = Some(message.id.0.clone());
                        store.save(conversation)
                    },
                )?;
                if let Some(id) = appended {
                    self.vars.insert("$last".to_string(), id.clone().into());
                    self.remember(op, id.into());
                }
                Self::branch(&conversation)
            }
            "activate" => {
                let live = self.chat(&self.text(op, "chat"))?;
                let mut conversation = live.lock().unwrap();
                store.activate_branch(&mut conversation, &self.text(op, "message"))?;
                Self::branch(&conversation)
            }
            "activateAll" => {
                let id = self.text(op, "chat");
                let tree = store.message_tree(&self.chat(&id)?.lock().unwrap())?;
                let mut out = Vec::new();
                for message in tree.messages.iter().filter(|m| m.branchable) {
                    store.activate_branch(&mut self.chat(&id)?.lock().unwrap(), &message.id)?;
                    let live = self.chat(&id)?;
                    let now = live.lock().unwrap();
                    out.push(Json::Array(vec![
                        message.id.as_str().into(),
                        now.current_message_id.as_ref().map(|m| m.0.clone()).into(),
                        now.messages.len().into(),
                        manifest(&now.world).into(),
                    ]));
                }
                Json::Array(out)
            }
            "setTitle" => {
                let live = self.chat(&self.text(op, "chat"))?;
                store.set_title(&mut live.lock().unwrap(), &self.text(op, "title"))?;
                Json::Null
            }
            "markFinished" => {
                store.mark_finished(&self.text(op, "chat"))?;
                Json::Null
            }
            "markSeen" => {
                store.mark_seen(&self.text(op, "chat"))?;
                Json::Null
            }
            "dismiss" => {
                let live = self.chat(&self.text(op, "chat"))?;
                store.dismiss_error(&mut live.lock().unwrap(), &self.text(op, "message"))?;
                Json::Null
            }
            "endTurn" => {
                let live = self.chat(&self.text(op, "chat"))?;
                let message = store.end_turn(
                    &mut live.lock().unwrap(),
                    Self::last(op.get("final").unwrap()),
                )?;
                self.vars
                    .insert("$last".to_string(), message.id.0.clone().into());
                message.in_memory_text().into()
            }
            "endUnfinished" => {
                let live = self.chat(&self.text(op, "chat"))?;
                Json::Bool(
                    store
                        .end_unfinished_turn(&mut live.lock().unwrap(), &self.text(op, "reason"))?,
                )
            }
            "interrupt" => store.interrupt_unfinished()?.into(),
            "archive" => {
                store.archive_conversation(&self.text(op, "chat"))?;
                Json::Null
            }
            "restore" => {
                store.restore_conversation(&self.text(op, "chat"))?;
                Json::Null
            }
            "delete" => {
                store.delete_conversation(&self.text(op, "chat"))?;
                Json::Null
            }
            "archiveBench" => store.archive_bench(&self.text(op, "bench"))?.into(),
            "restoreBench" => {
                store.restore_bench(&self.text(op, "bench"))?;
                Json::Null
            }
            "deleteBench" => {
                store.delete_bench(&self.text(op, "bench"))?;
                Json::Null
            }
            "purge" => {
                store.clear_expired_archive(number("now"))?;
                Json::Null
            }
            "nextExpiry" => store.next_archive_expiry(number("now"))?.into(),
            "collect" => {
                store.collect_chunks()?;
                Json::Null
            }
            "createBench" => {
                let members = op
                    .get("members")
                    .unwrap()
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|m| mcbench_store::BenchMember {
                        provider: m.get("provider").unwrap().as_str().unwrap().to_string(),
                        model: m.get("model").unwrap().as_str().unwrap().to_string(),
                        effort: m.get("effort").and_then(Json::as_str).map(str::to_string),
                    })
                    .collect();
                let (bench, chats) = store.create_bench(
                    &self.text(op, "id"),
                    NewBench {
                        title: self.text(op, "title"),
                        blind: op.get("blind") == Some(&Json::Bool(true)),
                        resolution: op
                            .get("resolution")
                            .and_then(Json::as_f64)
                            .map(|r| r as u32),
                        content: decode(&self.resolve(op.get("content").unwrap())),
                        prompts: decode(op.get("prompts").unwrap()),
                        members,
                    },
                )?;
                let ids: Vec<String> = chats
                    .iter()
                    .map(|c| c.lock().unwrap().id.0.clone())
                    .collect();
                if let Some(Json::String(name)) = op.get("as") {
                    self.vars.insert(name.clone(), bench.id.clone().into());
                    for (m, id) in ids.iter().enumerate() {
                        self.vars
                            .insert(format!("{name}.member{m}"), id.as_str().into());
                    }
                    self.benches.insert(name.clone(), bench.clone());
                }
                object! { "bench" => bench_json(&bench), "members" => ids }
            }
            "loadBench" => {
                let bench = store.bench(&self.text(op, "id"))?.unwrap();
                let name = self.text_raw(op, "as");
                self.vars.insert(name.clone(), bench.id.clone().into());
                for (m, member) in store.bench_members(&bench.id)?.iter().enumerate() {
                    self.vars.insert(
                        format!("{name}.member{m}"),
                        member.lock().unwrap().id.0.clone().into(),
                    );
                }
                self.benches.insert(name, bench.clone());
                bench_json(&bench)
            }
            "benchPrompt" => {
                let key = Self::bench_key(op);
                let bench = self.benches.get(&key).unwrap().clone();
                let member = self.member(op, number("member"));
                let live = self.chat(&member)?;
                let mut conversation = live.lock().unwrap();
                let round_index = number("index") as usize;
                let round = bench.round_id(round_index).unwrap().to_string();
                let links = bench.round_prompts(round_index)?;
                let mut content = bench.round_content(round_index)?;
                if op.get("share") == Some(&Json::Bool(true))
                    && let Content::Parts(parts) = &mut content
                {
                    for part in parts.iter_mut() {
                        if let mcbench_store::Part::Attachment { attachment } = part {
                            *attachment = store.share_attachment(attachment, &member)?;
                        }
                    }
                }
                let world = conversation.world.reference();
                let message = store.append_message(
                    &mut conversation,
                    NewMessage::bench_prompt(&round, links.as_deref(), &content),
                    Some(world),
                )?;
                self.vars
                    .insert("$last".to_string(), message.id.0.clone().into());
                store.save(&mut conversation)?;
                message.in_memory_text().into()
            }
            "memberReply" => {
                let member = self.member(op, number("member"));
                let live = self.chat(&member)?;
                let mut conversation = live.lock().unwrap();
                match op.get("reply").filter(|r| r.truthy()) {
                    None => store
                        .end_turn(
                            &mut conversation,
                            NewMessage::error("Provider failed", None),
                        )?
                        .in_memory_text()
                        .into(),
                    Some(reply) => {
                        let message =
                            store.append_message(&mut conversation, Self::reply(reply), None)?;
                        self.vars
                            .insert("$last".to_string(), message.id.0.clone().into());
                        store.save(&mut conversation)?;
                        message.in_memory_text().into()
                    }
                }
            }
            "benchProgress" => Json::Array(
                (0..3)
                    .map(|m| {
                        let p = store.bench_progress(&self.member(op, m))?;
                        Ok(object! { "round" => p.round, "halted" => p.halted })
                    })
                    .collect::<Result<_, Error>>()?,
            ),
            "appendRound" => {
                let key = Self::bench_key(op);
                let content = Content::Text(self.text(op, "content"));
                let links: Vec<PromptLink> = decode(op.get("prompts").unwrap());
                let bench = self.benches.get_mut(&key).unwrap();
                store.append_round(bench, &content, &links)?;
                bench_json(bench)
            }
            "vote" => {
                let key = Self::bench_key(op);
                let id = |m: &Json| self.member(op, m.as_f64().unwrap() as i64);
                let pick = op.get("pick").filter(|p| !matches!(p, Json::Null)).map(id);
                let judged: Vec<String> = op
                    .get("judged")
                    .unwrap()
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(id)
                    .collect();
                let bench = self.benches.get_mut(&key).unwrap();
                let round = bench
                    .round_id(number("round") as usize)
                    .unwrap()
                    .to_string();
                store.cast_vote(bench, &round, pick.as_deref(), &judged)?;
                bench_json(bench)
            }
            "clearVote" => {
                let bench = self.benches.get_mut(&Self::bench_key(op)).unwrap();
                let round = bench
                    .round_id(number("round") as usize)
                    .unwrap()
                    .to_string();
                store.clear_vote(bench, &round)?;
                bench_json(bench)
            }
            "reveal" => {
                let bench = self.benches.get_mut(&Self::bench_key(op)).unwrap();
                store.reveal_bench(bench)?;
                bench_json(bench)
            }
            "editRound" => {
                let content = Content::Text(self.text(op, "content"));
                let links: Vec<PromptLink> = decode(op.get("links").unwrap());
                let bench = self.benches.get_mut(&Self::bench_key(op)).unwrap();
                let reverted =
                    store.edit_bench_round(bench, number("index") as usize, &content, &links)?;
                let ids: Vec<String> = reverted
                    .iter()
                    .map(|c| c.lock().unwrap().id.0.clone())
                    .collect();
                object! { "bench" => bench_json(bench), "reverted" => ids }
            }
            "followBranch" => {
                let key = Self::bench_key(op);
                let bench = self.benches.get(&key).unwrap();
                let round = &bench.rounds[number("round") as usize];
                let id = match op.get("which").unwrap().as_str().unwrap() {
                    "self" => round.get("id").cloned(),
                    "previous" => round
                        .get("branch")
                        .and_then(|b| b.get("previousId"))
                        .cloned(),
                    _ => round.get("branch").and_then(|b| b.get("nextId")).cloned(),
                };
                let Some(Json::String(id)) = id else {
                    return Ok("none".into());
                };
                let bench = self.benches.get_mut(&key).unwrap();
                store.follow_bench_branch(bench, &id)?;
                let bench = bench_json(bench);
                let heads: Vec<Json> = (0..3)
                    .map(|m| match self.vars.get(&format!("{key}.member{m}")) {
                        Some(Json::String(id)) => self
                            .chat(id)
                            .map(|live| Self::branch(&live.lock().unwrap())),
                        _ => Ok(Json::Null),
                    })
                    .collect::<Result<_, _>>()?;
                object! { "bench" => bench, "heads" => heads }
            }
            "retryRound" => {
                let member = self.member(op, number("member"));
                let live = self.chat(&member)?;
                let mut conversation = live.lock().unwrap();
                let bench = self.benches.get_mut(&Self::bench_key(op)).unwrap();
                store.retry_bench_round(bench, &mut conversation)?;
                bench_json(bench)
            }
            "benchTitle" => {
                let title = self.text(op, "title");
                let bench = self.benches.get_mut(&Self::bench_key(op)).unwrap();
                store.set_bench_title(bench, &title)?;
                bench_json(bench)
            }
            "forkMember" => {
                let live = self.chat(&self.member(op, number("member")))?;
                let source = live.lock().unwrap().clone();
                let fork =
                    store.fork_conversation(&source, source.messages.len(), &source.world, None)?;
                let id = fork.lock().unwrap().id.0.clone();
                self.remember(op, id.into())
            }
            "leaderboard" => {
                Json::Array(store.leaderboard()?.iter().map(leaderboard_row).collect())
            }
            "voteCount" => store.vote_count()?.into(),
            "clearAllVotes" => {
                store.clear_all_votes()?;
                Json::Null
            }
            "fork" => {
                let live = self.chat(&self.text(op, "chat"))?;
                let source = live.lock().unwrap().clone();
                let which = op.get("world").unwrap().as_str().unwrap();
                let world = if which == "head" {
                    source.world.clone()
                } else {
                    let message =
                        self.resolve(&Json::String(which["checkpoint:".len()..].to_string()));
                    source
                        .checkpoints
                        .get(&mcbench_store::MessageId(
                            message.as_str().unwrap().to_string(),
                        ))
                        .cloned()
                        .ok_or_else(|| Error::plain("no such checkpoint"))?
                };
                let fork =
                    store.fork_conversation(&source, number("count") as usize, &world, None)?;
                let id = fork.lock().unwrap().id.0.clone();
                self.remember(op, id.into())
            }
            "images" => {
                let images: Vec<ImageData> = op
                    .get("images")
                    .unwrap()
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|image| match image.get("png") {
                        Some(png) => ImageData::Png(base64(png.as_str().unwrap())),
                        None => ImageData::DataUrl(
                            image.get("dataUrl").unwrap().as_str().unwrap().to_string(),
                        ),
                    })
                    .collect();
                let urls =
                    store.save_images(&self.text(op, "chat"), &images, &self.text(op, "prefix"))?;
                self.remember(op, urls.into())
            }
            "attachments" => {
                let files: Vec<Json> = op
                    .get("files")
                    .unwrap()
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(file)
                    .collect();
                let saved = store.save_attachments(&self.text(op, "chat"), &files)?;
                let saved = Json::Array(saved.iter().map(attachment_json).collect());
                self.remember(op, saved)
            }
            "repeats" => {
                let live = self.chat(&self.text(op, "chat"))?;
                let content: Content = decode(&self.resolve(op.get("content").unwrap()));
                Json::Bool(store.repeats_latest_root_prompt(&live.lock().unwrap(), &content)?)
            }
            "reserve" => {
                let id = self.text(op, "chat");
                store.storage().reserve(&id, number("bytes") as u64, true)?;
                store.storage().used(&id)?.into()
            }
            "refund" => {
                let id = self.text(op, "chat");
                store.storage().refund(&id, number("bytes") as u64);
                store.storage().used(&id)?.into()
            }
            "refusal" => store.storage().refusal(&self.text(op, "chat"))?.into(),
            "favorites" => settings_summary(
                &self
                    .settings
                    .update_model_favorites(op.get("value").unwrap())?
                    .into(),
            ),
            "settingsUpdate" => settings_summary(&self.settings.update(op.get("next").unwrap())?.0),
            "promptsApply" => Json::Array(
                self.prompts
                    .apply(op.get("args").unwrap(), op.get("by").unwrap())?,
            ),
            "promptsSave" => Json::Array(
                self.prompts
                    .save(op.get("prompts").unwrap().as_array().unwrap())?
                    .iter()
                    .map(LibraryPrompt::to_json)
                    .collect(),
            ),
            "promptsList" => Json::Array(
                self.prompts
                    .list()?
                    .iter()
                    .map(LibraryPrompt::to_json)
                    .collect(),
            ),
            other => panic!("unknown op {other}"),
        })
    }

    fn text_raw(&self, op: &Json, key: &str) -> String {
        op.get(key).unwrap().as_str().unwrap().to_string()
    }
}

fn base64(text: &str) -> Vec<u8> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .decode(text)
        .unwrap()
}

/// `run.js`'s `file(entry)`.
fn file(entry: &Json) -> Json {
    let Some(fill) = entry.get("fill") else {
        return entry.clone();
    };
    use base64::Engine;
    let count = fill.get("count").unwrap().as_f64().unwrap() as usize;
    let byte = fill.get("byte").unwrap().as_f64().unwrap() as u8;
    let mime = fill.get("mime").unwrap().as_str().unwrap();
    object! {
        "name" => entry.get("name").cloned().unwrap(),
        "data" => format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(vec![byte; count])),
    }
}

fn rust_run(root: &Path, inputs: &Path) -> String {
    let host = DetHost::new(T0);
    let (store, settings, prompts) = open(root, host.clone());
    let ops = json::parse(&std::fs::read_to_string(inputs.join("ops.json")).unwrap()).unwrap();
    let mut runner = Runner {
        store: &store,
        settings: &settings,
        prompts: &prompts,
        host,
        vars: HashMap::new(),
        benches: HashMap::new(),
    };
    let results: Vec<Json> = ops
        .as_array()
        .unwrap()
        .iter()
        .map(|op| {
            let kind = op.get("op").unwrap().clone();
            Json::Array(vec![kind, attempt(runner.run(op))])
        })
        .collect();
    json::stringify(&Json::Array(results))
}

#[test]
#[ignore = "needs MCBENCH_DIFF_DATA, MCBENCH_NODE_DIFF and MCBENCH_DIFF_SCRATCH (Node and a data folder)"]
fn node_and_rust_read_and_write_the_same() {
    let env = env();
    std::fs::create_dir_all(env.scratch.join("tools-empty")).unwrap();
    let inputs = env.scratch.join("inputs");
    let generated = clone(&env, &env.fixture, "gen");
    let status = Command::new(std::env::var("MCBENCH_NODE").unwrap_or_else(|_| "node".to_string()))
        .arg(env.scripts.join("gen.js"))
        .arg(&generated)
        .arg(&inputs)
        .env("MCBENCH_NODE_DIFF", &env.node_checkout)
        .status()
        .unwrap();
    assert!(status.success());

    // 1. Reads.
    let node_doc = node_read(&env, &env.fixture, &inputs, "read-node");
    let rust_doc = rust_read(&env, &env.fixture, &inputs, "read-rust");
    same(&node_doc, &rust_doc, "read document of the fixture");

    // 2. Writes.
    let node_folder = clone(&env, &env.fixture, "write-node");
    let node_out = env.scratch.join("write-node-out");
    node(&env, Some(&node_folder), "run.js", &[&inputs, &node_out]);
    let rust_folder = clone(&env, &env.fixture, "write-rust");
    let rust_results = rust_run(&rust_folder, &inputs);
    std::fs::write(env.scratch.join("write-rust-results.json"), &rust_results).unwrap();
    let node_results = std::fs::read_to_string(node_out.join("results.json")).unwrap();
    same(&node_results, &rust_results, "results of the write script");
    same(
        &dump(&env, &node_folder, "dump-node"),
        &dump(&env, &rust_folder, "dump-rust"),
        "tables and files after the writes",
    );
    let after_node = node_read(&env, &node_folder, &inputs, "after-node-node");
    same(
        &after_node,
        &node_read(&env, &rust_folder, &inputs, "after-rust-node"),
        "Node's read of the two written folders",
    );
    same(
        &after_node,
        &rust_read(&env, &node_folder, &inputs, "after-node-rust"),
        "Rust's read of the Node-written folder",
    );

    // 3. A fresh folder.
    let rust_fresh = env.scratch.join("fresh-rust");
    let node_fresh = env.scratch.join("fresh-node");
    for dir in [&rust_fresh, &node_fresh] {
        if dir.exists() {
            std::fs::remove_dir_all(dir).unwrap();
        }
    }
    drop(
        common::open(
            &rust_fresh,
            DetHost::new(T0),
            common::limits(CHAT_MAX_BYTES),
        )
        .unwrap(),
    );
    // Node makes its own by opening an empty folder (read.js opens the store).
    std::fs::create_dir_all(&node_fresh).unwrap();
    node(
        &env,
        Some(&node_fresh),
        "read.js",
        &[&inputs, &env.scratch.join("fresh-node-made.json")],
    );
    let rust_dump = dump(&env, &rust_fresh, "dump-fresh-rust");
    same(
        &dump(&env, &node_fresh, "dump-fresh-node"),
        &rust_dump,
        "a fresh folder's tables (sqlite_master included)",
    );
    same(
        &node_read(&env, &node_fresh, &inputs, "fresh-node-node"),
        &node_read(&env, &rust_fresh, &inputs, "fresh-rust-node"),
        "Node's read of the two fresh folders",
    );
    same(
        &node_read(&env, &node_fresh, &inputs, "fresh-node-node2"),
        &rust_read(&env, &node_fresh, &inputs, "fresh-node-rust"),
        "Rust's read of Node's fresh folder",
    );
}
