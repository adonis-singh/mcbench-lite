//! A stand-in for the core, for the dev binary and the in-process tests:
//! it implements every command of the seam (`src/lib.rs`) against recorded
//! data, so the interface can be exercised without the real core. Never the
//! app's core.
//!
//! Its data is an authored fixture ([`super::fixtures`]) or a folder that
//! `dev/export-data.py` wrote from a copy of a data folder
//! ([`StandIn::from_folder`]). A sent message is answered by streaming a
//! canned answer a word at a time, [`Rate`] apart (zero under test); a chat
//! named [`FAILING_CHAT`] fails every turn. Preferences are kept in a file
//! when one is given, so the dev binary reopens where it was.
//!
//! A new answer (a command the interface starts sending) is a match arm in
//! [`StandIn::send`] and, when it needs data, a field of `Data` that both
//! sources fill.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};

use super::fixtures::Fixture;
use super::turns::{self, Script, Step};
use crate::{
    Archive, ArchivedConversation, Auth, Command, ConversationHeader, ConversationList,
    ConversationSummary, Core, ListedBench, ListedChat, MessageTree, ModelFacts, ModelInfo,
    Preferences, Provider, Resolution, SearchHit, SearchScope, Subject, Target, TreeMessage,
    TurnEvent, Update, Window,
};
use mcbench_store::{ConversationId, StoredMessage};

// settings agent: Settings, sign-ins, installs, setup plays, votes, shares.
mod settings;
// bench agent: benches, votes, the leaderboard, the library's saves.
mod bench;
pub use bench::{
    ADDED_TITLE, BRANCH_REFUSED, BenchPlay, CREATE_REFUSED, DETAILS_FAILED, FORK_FAILED, FORK_WAIT,
    GENERATE_FAILED, LEADERBOARD_FAILED, MEMBER_STEPS, PROMPTS_FAILED, RETRY_REFUSED,
    REVEAL_FAILED, ROUND_REFUSED, SAVE_FAILED, VOTE_REFUSED,
};
pub use settings::{
    CLEAR_FAILED, INSTALL_FAILED, INSTALL_REFUSED, PASTE_PROMPT, Play, READ_FAILED, REMOVE_FAILED,
    SAVE_REFUSED, SETUP_FAILURE, SHARING_NOT_JOINED, SIGN_IN_TIMED_OUT, SIGN_IN_URL, USER_CODE,
    WSL_INSTALLING, WSL_NEEDED, WSL_REFUSED, WSL_RESTART,
};
// pane agent: the live world over the fake pane.
mod live_world;
pub use live_world::{
    CLICK_FAILED, DATA_NOT_READY, EXPORT_FAILED, FELL_OUT, LiveFailures, NO_ADAPTER, NOT_JOINED,
    NOTHING_SHOWN, PLACE_FAILED, SELECTION_REFUSED, SPAWN_FAILED, STOPPED, STOPPED_REFUSED,
    UNKNOWN_BLOCK, Y_OFFSET,
};
// share agent: the Share panel's answers (`ui-seam-share.md`).
mod share;
pub use share::{
    BENCH_EMPTY, BENCH_RUNNING, CHAT_EMPTY, CHAT_RUNNING, NOT_FOUND, NOT_SHARED, PAGES_LATE,
    PUBLISHING, PUSH_FAILED, REPO_NEEDED, SHARE_EXPORT_FAILED, SITE_FAILED, SharePlay,
};
// end of the share agent's block

/// The model every fixture chat uses.
pub const MODEL: &str = "claude-opus-5-5";

/// The chat whose every turn fails (the `gate` fixture's third chat).
pub const FAILING_CHAT: &str = "chat-farm";

/// The message of its failure.
pub const FAILURE: &str = "The provider refused the request: rate limited (a fixture error)";

/// How long between two streamed words; zero streams a turn at once.
pub type Rate = Duration;

/// The rate of the dev binary's turns (`--rate-ms`, 60 by default).
pub const DEV_RATE: Rate = Duration::from_millis(60);

/// The stand-in's own list row; [`ChatRow::listed`] makes the core's.
#[derive(Clone, Debug, PartialEq)]
struct ChatRow {
    id: String,
    title: String,
    provider: String,
    model: String,
    effort: Option<String>,
    resolution: Resolution,
    created_at: String,
    bench: Option<BenchSeat>,
    unread: bool,
    message_count: u32,
    prompts: Vec<String>,
    running_since: Option<i64>,
}

/// A bench model's seat: its bench, slot, rounds sent and halt.
#[derive(Clone, Debug, PartialEq)]
struct BenchSeat {
    bench: String,
    slot: u32,
    round: u32,
    halted: Option<Halted>,
}

/// How a bench model stopped short.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Halted {
    Stopped,
    Failed,
}

impl ChatRow {
    /// The core's row (`ListedChat`).
    fn listed(&self) -> ListedChat {
        ListedChat {
            summary: ConversationSummary {
                id: ConversationId(self.id.clone()),
                title: self.title.clone(),
                provider: self.provider.clone(),
                model: self.model.clone(),
                effort: self.effort.clone(),
                resolution: i64::from(self.resolution.value()),
                created_at: self.created_at.clone(),
                bench_id: self.bench.as_ref().map(|b| b.bench.clone()),
                bench_slot: self.bench.as_ref().map(|b| i64::from(b.slot)),
                unread: self.unread,
                message_count: i64::from(self.message_count),
                prompts: self.prompts.clone(),
            },
            progress: self.bench.as_ref().map(|b| {
                (
                    i64::from(b.round),
                    b.halted.map(|h| match h {
                        Halted::Stopped => "stopped",
                        Halted::Failed => "failed",
                    }),
                )
            }),
            running_since: self.running_since,
        }
    }
}

/// One stored row.
#[derive(Clone, Debug)]
struct Row {
    id: String,
    parent: Option<String>,
    position: u32,
    payload: Value,
}

/// A chat with every branch's rows.
#[derive(Clone, Debug)]
struct ChatData {
    row: ChatRow,
    rows: Vec<Row>,
    head: Option<String>,
    archived_at: Option<i64>,
}

impl ChatData {
    /// The active branch: the head's line, oldest first.
    fn branch(&self) -> Vec<&Row> {
        let mut line = Vec::new();
        let mut at = self.head.clone();
        while let Some(id) = at {
            let row = self
                .rows
                .iter()
                .find(|r| r.id == id)
                .unwrap_or_else(|| panic!("chat {}: no row {id}", self.row.id));
            at = row.parent.clone();
            line.push(row);
        }
        line.reverse();
        line
    }

    fn push(&mut self, prefix: &str, mut payload: Value) -> String {
        payload["createdAt"] = Value::String(super::fixtures::CREATED_AT.to_string());
        let id = format!("{prefix}{}", self.rows.len() + 1);
        let position = self.rows.iter().map(|r| r.position + 1).max().unwrap_or(0);
        self.rows.push(Row {
            id: id.clone(),
            parent: self.head.clone(),
            position,
            payload,
        });
        self.head = Some(id.clone());
        self.row.message_count += 1;
        id
    }
}

/// Everything the stand-in serves.
struct Data {
    chats: Vec<ChatData>,
    benches: Vec<ListedBench>,
    prompt_titles: BTreeMap<String, String>,
    providers: Vec<Provider>,
    favorites: Vec<(String, String)>,
    /// A provider's models, when the data holds them.
    models: HashMap<String, Vec<ModelInfo>>,
    preferences: Preferences,
    /// Why the kept preferences could not be read, shown at attach.
    preferences_error: Option<String>,
    preferences_file: Option<PathBuf>,
    // pickers: the catalogues' failures and the slow ones.
    /// The providers whose catalogue fails, with the failure's text.
    catalogue_errors: HashMap<String, String>,
    /// The providers whose catalogue answers after [`SLOW`].
    slow: Vec<String>,
    /// The folder of stored pictures (`images/<chat>/<name>`) and
    /// attachments (`attachments/<owner>/<name>`): the exported folder's;
    /// none for a fixture, whose addresses name no file.
    files: Option<PathBuf>,
    created: usize,
    // sidebar agent: the prompt library, archived benches, and a switch
    // that makes the archive's reads fail.
    library: Vec<crate::LibraryPrompt>,
    archived_benches: Vec<ArchivedSeat>,
    archive_fails: bool,
}

/// An archived bench: its row (to list it again on restore), when it was
/// archived, and how many models it ran.
struct ArchivedSeat {
    bench: ListedBench,
    archived_at: i64,
    models: i64,
}

/// The stand-in.
pub struct StandIn {
    data: Arc<Mutex<Data>>,
    window: Mutex<Option<Arc<dyn Window>>>,
    streamed: String,
    rate: Rate,
    stop: Arc<AtomicBool>,
    // chat (stage four's chat agent): the turns it streams, the picture a
    // shot returns, sends it refuses, turns it runs for a join, a stop it
    // makes itself.
    script: Mutex<Script>,
    picture: Mutex<Option<PathBuf>>,
    refusing: Mutex<HashMap<String, String>>,
    joinable: Mutex<Vec<String>>,
    stop_at: Arc<Mutex<Option<usize>>>,
    branch_failure: Mutex<Option<BranchFailure>>,
    dismiss_failure: Mutex<Option<String>>,
    // pickers: how a slow answer waits (a thread unless set).
    later: Mutex<Option<Later>>,
    // chat-fix: the setup steps it publishes (`--setup-pending`), none
    // unless set.
    setup: Mutex<Option<Vec<mcbench_setup::Step>>>,
    // chat-fix: whether the archive was read (the seam's second rule: an
    // archive, restore or delete answers with the archive again only then).
    archive_read: AtomicBool,
    // chat-fix: every search fails with this sentence while set.
    search_failure: Mutex<Option<String>>,
    // chat-fix: the turns running now, by chat, and whether they wait to be
    // stepped.
    live: Arc<Mutex<HashMap<String, Live>>>,
    by_hand: AtomicBool,
    // seen: the next mark fails with this sentence.
    seen_failure: Mutex<Option<String>>,
    // settings agent: Settings and what it plays.
    settings: Mutex<settings::State>,
    // chat-fix-2: chats that cannot be read, and how long an archive read
    // takes.
    unreadable: Mutex<Vec<String>>,
    archive_delay: Mutex<Option<Duration>>,
    // start-place agent: the first lists late, as the real core's cold
    // start sends them (after Setup and the pane): `Some(None)` held until
    // `release_lists`, `Some(Some(d))` sent `d` after the window attaches.
    lists_late: Mutex<Option<Option<Duration>>>,
    // pane agent: the live world.
    live_world: Mutex<live_world::LiveWorld>,
    // bench agent: the benches' rounds and votes, the plays, the asked set.
    bench: Arc<Mutex<bench::State>>,
    // par-chat: the pair check of a chat's send and the sends recorded.
    pairs: Mutex<Pairs>,
    // share agent: the publishes under way and the share plays.
    share: Mutex<share::State>,
}

// par-chat (stage four's parity pass)
/// What a chat's send checks of its provider and model, as the route does
/// (`server/index.js` `handleChat`), and what it was sent.
#[derive(Default)]
struct Pairs {
    /// Providers the core no longer knows (`forget_provider`).
    forgotten: Vec<String>,
    /// Every model is read as empty (`blank_models`).
    blank: bool,
    /// The targets of every `Send`, in order, once `record_sends` asked.
    sent: Option<Arc<Mutex<Vec<Target>>>>,
    /// Facts that replace a model's own (`describe_model`).
    facts: HashMap<(String, String), ModelFacts>,
    /// Every `CreateChat` fails with this text while set (`fail_creates`).
    create_failure: Option<String>,
    /// `CreateChat` answers wait for `release_creates` (`hold_creates`).
    hold_creates: bool,
    /// The held requests, in order.
    held_creates: Vec<(String, String)>,
    /// How many `CreateChat` commands came.
    creates: usize,
    /// The next providers' read fails with this text (`fail_providers_read`).
    providers_failure: Option<String>,
}
// end of par-chat's block

/// How the stand-in fails the next branch switch
/// ([`StandIn::fail_next_branch`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BranchFailure {
    /// The route refuses it: `Update::Error` with this text, then the
    /// unchanged transcript.
    Refused(String),
    /// The chat can no longer be read: `Update::TranscriptFailed`.
    Unreadable(String),
}

/// The text of a refused send ([`StandIn::refuse_sends_to`]): the route's
/// 409.
pub const REFUSAL: &str = "A turn is already running for this conversation";
// par-chat: the route's refusals of a chat message's pair (`handleChat`).
/// An unknown provider.
pub const UNKNOWN_PROVIDER: &str = "Choose a configured provider";
/// An empty model.
pub const NO_MODEL: &str = "Choose a model";
/// `POST /api/conversations`'s refusal of its pair.
pub const NO_PAIR: &str = "Provider and model are required";

fn user_payload(text: &str) -> Value {
    json!({ "role": "user", "content": [{ "type": "text", "text": text }] })
}

/// The providers of `server/settings.js`, without Cursor: label, auth and
/// whether a harness. The stand-in's own table (dev only).
const PROVIDERS: [(&str, &str, Auth, bool); 9] = [
    ("openrouter", "OpenRouter", Auth::ApiKey, false),
    ("openai", "OpenAI", Auth::ApiKey, false),
    ("codex", "ChatGPT", Auth::CliLogin, false),
    ("anthropic", "Anthropic", Auth::ApiKey, false),
    ("groq", "Groq", Auth::ApiKey, false),
    ("google", "Google", Auth::ApiKey, false),
    ("qwencloud", "QwenCloud", Auth::ApiKey, false),
    ("claude-code", "Claude Code", Auth::CliLogin, true),
    ("codex-cli", "Codex CLI", Auth::CliLogin, true),
];

fn provider(
    id: &str,
    connected: bool,
    enabled: bool,
    hidden: Vec<String>,
    order: Vec<String>,
) -> Provider {
    let (_, label, auth, harness) = PROVIDERS
        .iter()
        .find(|(p, ..)| *p == id)
        .unwrap_or_else(|| panic!("no provider {id}"));
    Provider {
        id: id.into(),
        label: (*label).into(),
        connected,
        auth: *auth,
        harness: *harness,
        enabled,
        hidden_models: hidden,
        model_order: order,
    }
}

/// The fixtures' models: two of Anthropic's, served by Anthropic and by
/// Claude Code.
fn fixture_models() -> HashMap<String, Vec<ModelInfo>> {
    let facts = |context: f64| ModelFacts {
        known: true,
        image_input: Some(true),
        image_output: false,
        tools: Some(true),
        reasoning: true,
        efforts: vec!["low".into(), "medium".into(), "high".into()],
        default_effort: Some("medium".into()),
        mandatory_reasoning: false,
        context_window: Some(Some(context)),
        fast: None,
    };
    let models = vec![
        ModelInfo {
            id: MODEL.into(),
            name: "Claude Opus 5.5".into(),
            created: 1_760_000_000.,
            facts: facts(1_000_000.),
        },
        ModelInfo {
            id: "claude-haiku-5-5".into(),
            name: "Claude Haiku 5.5".into(),
            created: 1_759_000_000.,
            facts: facts(200_000.),
        },
    ];
    [
        ("anthropic".to_string(), models.clone()),
        ("claude-code".to_string(), models),
    ]
    .into()
}

impl StandIn {
    /// A stand-in over an authored fixture, streaming `streamed` at `rate`;
    /// preferences go to `preferences_file` when given.
    pub fn new(
        fixture: Fixture,
        streamed: String,
        rate: Rate,
        preferences_file: Option<PathBuf>,
    ) -> Self {
        let chats = fixture
            .chats
            .into_iter()
            .map(|chat| {
                // bench agent: a bench model's own model.
                let (provider, model, effort) = chat
                    .model
                    .unwrap_or_else(|| ("anthropic".into(), MODEL.into(), None));
                let mut data = ChatData {
                    row: ChatRow {
                        id: chat.id.clone(),
                        title: chat.title,
                        provider,
                        model,
                        effort,
                        resolution: Resolution::of(chat.resolution),
                        created_at: "2026-11-17T10:00:00Z".into(),
                        bench: chat.bench.map(|(bench, slot)| BenchSeat {
                            bench,
                            slot,
                            round: 0,
                            halted: None,
                        }),
                        unread: chat.unread,
                        message_count: 0,
                        prompts: chat.prompts,
                        running_since: chat.running_since,
                    },
                    rows: Vec::new(),
                    head: None,
                    archived_at: chat.archived_at,
                };
                for message in chat.messages {
                    let id = message.id;
                    let position = data.rows.len() as u32;
                    data.rows.push(Row {
                        id: id.clone(),
                        parent: data.head.clone(),
                        position,
                        payload: message.payload,
                    });
                    data.head = Some(id);
                    data.row.message_count += 1;
                }
                seat_state(&mut data);
                data
            })
            .collect();
        let benches = fixture
            .benches
            .into_iter()
            .map(|b| ListedBench {
                id: b.id,
                title: b.title,
                created_at: "2026-11-17T09:00:00Z".into(),
                head: Some("round-1".into()),
                resolution: Some(i64::from(Resolution::Standard.value())),
                rounds: 1,
                prompts: Vec::new(),
                blind: b.blind,
                revealed: b.revealed,
                order: b.order,
            })
            .collect();
        let providers = pickers_providers();
        let kept = read_preferences(preferences_file.as_deref());
        // sidebar agent: the fixture's library, deleted prompts' titles and
        // archived benches.
        let library = fixture
            .library
            .into_iter()
            .map(|(id, title, text, tags)| crate::LibraryPrompt {
                id,
                title,
                text,
                tags,
                favorite: false,
                attachments: Vec::new(),
                used_at: None,
                added_by: None,
            })
            .collect();
        let prompt_titles = fixture.prompt_titles.into_iter().collect();
        let archived_benches = fixture
            .archived_benches
            .into_iter()
            .map(|(id, title, archived_at, models)| ArchivedSeat {
                bench: ListedBench {
                    id,
                    title,
                    created_at: "2026-11-15T09:00:00Z".into(),
                    head: None,
                    resolution: Some(1),
                    rounds: 0,
                    prompts: Vec::new(),
                    blind: false,
                    revealed: false,
                    order: Vec::new(),
                },
                archived_at,
                models,
            })
            .collect();
        Self::over(
            Data {
                chats,
                benches,
                library,
                archived_benches,
                archive_fails: false,
                prompt_titles,
                providers,
                favorites: vec![
                    ("anthropic".into(), MODEL.into()),
                    ("openrouter".into(), "anthropic/claude-opus-4.5".into()),
                ],
                models: pickers_models(),
                preferences: kept.0,
                preferences_error: kept.1,
                preferences_file,
                catalogue_errors: [(FAILING_PROVIDER.to_string(), CATALOGUE_FAILURE.to_string())]
                    .into(),
                slow: vec![SLOW_PROVIDER.to_string()],
                files: None,
                created: 0,
            },
            streamed,
            rate,
        )
    }

    /// A stand-in over a folder `dev/export-data.py` wrote; preferences are
    /// kept in its `preferences.json`.
    pub fn from_folder(folder: &Path, streamed: String, rate: Rate) -> Self {
        let read = |name: &str| -> Value {
            let path = folder.join(name);
            let text = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
            serde_json::from_str(&text)
                .unwrap_or_else(|e| panic!("{} is not JSON: {e}", path.display()))
        };
        let lists = read("lists.json");
        let archive = read("archive.json");
        let mut chats = Vec::new();
        let listed: Vec<&Value> = lists["chats"]
            .as_array()
            .expect("lists.json chats")
            .iter()
            .collect();
        let archived: Vec<&Value> = archive["conversations"]
            .as_array()
            .expect("archive.json conversations")
            .iter()
            .collect();
        for row in listed.iter().chain(archived.iter()) {
            let id = row["id"].as_str().expect("a chat id");
            let chat = read(&format!("chats/{id}.json"));
            let rows: Vec<Row> = chat["messages"]
                .as_array()
                .expect("a chat's messages")
                .iter()
                .map(|m| Row {
                    id: m["id"].as_str().expect("a message id").into(),
                    parent: m["parentId"].as_str().map(str::to_string),
                    position: m["position"].as_u64().expect("a position") as u32,
                    payload: serde_json::from_str(m["payload"].as_str().expect("a payload"))
                        .expect("a payload is JSON"),
                })
                .collect();
            let header = &chat["header"];
            let archived_at = row["archivedAt"].as_i64();
            let chat_row = if archived_at.is_some() {
                ChatRow {
                    id: id.into(),
                    title: header["title"].as_str().unwrap_or_default().into(),
                    provider: header["provider"].as_str().unwrap_or_default().into(),
                    model: header["model"].as_str().unwrap_or_default().into(),
                    effort: header["effort"].as_str().map(str::to_string),
                    resolution: Resolution::Standard,
                    created_at: String::new(),
                    bench: None,
                    unread: false,
                    message_count: rows.len() as u32,
                    prompts: Vec::new(),
                    running_since: None,
                }
            } else {
                chat_row_of(row)
            };
            chats.push(ChatData {
                row: chat_row,
                head: chat["tree"]["headId"].as_str().map(str::to_string),
                rows,
                archived_at,
            });
        }
        let benches = lists["benches"]
            .as_array()
            .expect("lists.json benches")
            .iter()
            .map(|b| ListedBench {
                id: b["id"].as_str().expect("a bench id").into(),
                title: b["title"].as_str().unwrap_or_default().into(),
                created_at: b["createdAt"].as_str().unwrap_or_default().into(),
                head: b["head"].as_str().map(str::to_string),
                resolution: b["resolution"].as_i64(),
                rounds: b["rounds"].as_u64().unwrap_or(0) as usize,
                prompts: strings(&b["prompts"]),
                blind: b["blind"].as_bool().unwrap_or(false),
                revealed: b["revealed"].as_bool().unwrap_or(false),
                order: strings(&b["order"]),
            })
            .collect();
        let prompt_titles = lists["promptTitles"]
            .as_object()
            .map(|o| {
                o.iter()
                    .map(|(k, v)| (k.clone(), v.as_str().unwrap_or_default().to_string()))
                    .collect()
            })
            .unwrap_or_default();
        let settings = read("settings.json");
        let providers = PROVIDERS
            .iter()
            .filter_map(|(id, ..)| {
                let p = settings["providers"].get(*id)?;
                Some(provider(
                    id,
                    p["keySaved"].as_bool().unwrap_or(false)
                        || PROVIDERS
                            .iter()
                            .any(|(q, _, auth, _)| q == id && *auth == Auth::CliLogin),
                    p["enabled"].as_bool().unwrap_or(true),
                    strings(&p["hiddenModels"]),
                    strings(&p["modelOrder"]),
                ))
            })
            .collect();
        let favorites = settings["modelFavorites"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|f| {
                f.as_str()?
                    .split_once("::")
                    .map(|(p, m)| (p.to_string(), m.to_string()))
            })
            .filter(|(p, _)| p != "cursor")
            .collect();
        let preferences_file = folder.join("preferences.json");
        let kept = read_preferences(Some(&preferences_file));
        Self::over(
            Data {
                chats,
                benches,
                prompt_titles,
                providers,
                favorites,
                models: folder_models(folder),
                preferences: kept.0,
                preferences_error: kept.1,
                preferences_file: Some(preferences_file),
                catalogue_errors: HashMap::new(),
                slow: Vec::new(),
                files: Some(folder.to_path_buf()),
                created: 0,
                library: folder_library(folder),
                archived_benches: folder_archived_benches(&archive),
                archive_fails: false,
            },
            streamed,
            rate,
        )
    }

    fn over(data: Data, streamed: String, rate: Rate) -> Self {
        let settings = Mutex::new(settings::State::new(&data.providers));
        StandIn {
            settings,
            data: Arc::new(Mutex::new(data)),
            window: Mutex::new(None),
            streamed,
            rate,
            stop: Arc::new(AtomicBool::new(false)),
            script: Mutex::new(Script::Plain),
            picture: Mutex::new(None),
            refusing: Mutex::new(HashMap::new()),
            joinable: Mutex::new(Vec::new()),
            stop_at: Arc::new(Mutex::new(None)),
            branch_failure: Mutex::new(None),
            dismiss_failure: Mutex::new(None),
            later: Mutex::new(None),
            setup: Mutex::new(None),
            archive_read: AtomicBool::new(false),
            search_failure: Mutex::new(None),
            live: Arc::new(Mutex::new(HashMap::new())),
            by_hand: AtomicBool::new(false),
            seen_failure: Mutex::new(None),
            unreadable: Mutex::new(Vec::new()),
            archive_delay: Mutex::new(None),
            lists_late: Mutex::new(None),
            live_world: Mutex::new(live_world::LiveWorld::default()),
            bench: Arc::new(Mutex::new(bench::State::default())),
            pairs: Mutex::new(Pairs::default()),
            // share agent
            share: Mutex::new(share::State::default()),
        }
    }

    // par-chat (stage four's parity pass)
    /// From now on the provider `id` is unknown to the core: a chat's send
    /// naming it is refused with `Choose a configured provider`, and
    /// nothing changes (the window's list of providers is not told).
    pub fn forget_provider(&self, id: &str) {
        self.pairs
            .lock()
            .expect("lock")
            .forgotten
            .push(id.to_string());
    }

    /// From now on every chat send's model is read as empty and refused
    /// with `Choose a model` (the window's own guard never sends one).
    pub fn blank_models(&self) {
        self.pairs.lock().expect("lock").blank = true;
    }

    /// Opening `chat` no longer fails (after [`StandIn::fail_reads_of`]).
    pub fn heal_reads_of(&self, chat: &str) {
        self.unreadable.lock().expect("lock").retain(|c| c != chat);
    }

    /// `ModelInfo` of `provider`'s `model` answers `facts` (a model no
    /// source describes: `ModelFacts::unknown()`; one with no tools).
    pub fn describe_model(&self, provider: &str, model: &str, facts: ModelFacts) {
        self.pairs
            .lock()
            .expect("lock")
            .facts
            .insert((provider.to_string(), model.to_string()), facts);
    }

    /// Every `CreateChat` fails with `text` while it is set (none: they
    /// succeed again).
    pub fn fail_creates(&self, text: Option<&str>) {
        self.pairs.lock().expect("lock").create_failure = text.map(str::to_string);
    }

    /// `CreateChat` answers wait until [`StandIn::release_creates`].
    pub fn hold_creates(&self) {
        self.pairs.lock().expect("lock").hold_creates = true;
    }

    /// The held `CreateChat` commands are answered, in order, and the
    /// next ones answer at once.
    pub fn release_creates(&self) {
        let held = {
            let mut pairs = self.pairs.lock().expect("lock");
            pairs.hold_creates = false;
            std::mem::take(&mut pairs.held_creates)
        };
        for (provider, model) in held {
            self.create_chat(provider, model);
        }
    }

    /// The next read of the providers (at attach, or `ReadProviders`)
    /// fails with `text`; the one after succeeds.
    pub fn fail_providers_read(&self, text: &str) {
        self.pairs.lock().expect("lock").providers_failure = Some(text.to_string());
    }

    /// A read of the providers: `Err` once after `fail_providers_read`.
    fn providers_read(&self, providers: Vec<Provider>) -> Update {
        match self.pairs.lock().expect("lock").providers_failure.take() {
            Some(text) => Update::Providers(Err(text)),
            None => Update::Providers(Ok(providers)),
        }
    }

    /// How many `CreateChat` commands came.
    pub fn creates(&self) -> usize {
        self.pairs.lock().expect("lock").creates
    }

    /// `POST /api/conversations`: a row with no message (so it is not
    /// listed), or the route's text; `ChatCreated` either way.
    fn create_chat(&self, provider: String, model: String) {
        let failure = self.pairs.lock().expect("lock").create_failure.clone();
        let known = self
            .data
            .lock()
            .expect("lock")
            .providers
            .iter()
            .any(|p| p.id == provider.trim());
        let result = match failure {
            Some(text) => Err(text),
            None if !known || model.trim().is_empty() => Err(NO_PAIR.to_string()),
            None => {
                let mut data = self.data.lock().expect("lock");
                data.created += 1;
                let id = format!("chat-new-{}", data.created);
                data.chats.insert(
                    0,
                    ChatData {
                        row: ChatRow {
                            id: id.clone(),
                            title: String::new(),
                            provider: provider.trim().to_string(),
                            model: model.trim().to_string(),
                            effort: None,
                            resolution: Resolution::Standard,
                            created_at: "2026-11-17T12:00:00Z".into(),
                            bench: None,
                            unread: false,
                            message_count: 0,
                            prompts: Vec::new(),
                            running_since: None,
                        },
                        rows: Vec::new(),
                        head: None,
                        archived_at: None,
                    },
                );
                Ok(id)
            }
        };
        self.window().publish(Update::ChatCreated(result));
    }

    /// The targets of every `Send` from now on, in order.
    pub fn record_sends(&self) -> Arc<Mutex<Vec<Target>>> {
        let sent = Arc::new(Mutex::new(Vec::new()));
        self.pairs.lock().expect("lock").sent = Some(sent.clone());
        sent
    }

    /// `outgoing`, its target recorded when `record_sends` asked.
    fn recorded(&self, outgoing: crate::Outgoing) -> crate::Outgoing {
        if let Some(sent) = &self.pairs.lock().expect("lock").sent {
            sent.lock().expect("lock").push(outgoing.target.clone());
        }
        outgoing
    }

    /// The route's refusal of a chat message's pair, checked before
    /// anything changes: an unknown provider, then an empty model.
    fn refuse_pair(&self, provider: &str, model: &str) -> Option<&'static str> {
        let pairs = self.pairs.lock().expect("lock");
        let known = self
            .data
            .lock()
            .expect("lock")
            .providers
            .iter()
            .any(|p| p.id == provider);
        if !known || pairs.forgotten.iter().any(|p| p == provider) {
            return Some(UNKNOWN_PROVIDER);
        }
        (pairs.blank || model.trim().is_empty()).then_some(NO_MODEL)
    }
    // end of par-chat's block

    // seen: a failed mark on demand.
    /// The next `MarkSeen` fails with `error`: `Update::Error`, then the
    /// unchanged list (the seam's second rule).
    pub fn fail_next_seen(&self, error: &str) {
        *self.seen_failure.lock().expect("lock") = Some(error.to_string());
    }

    /// The next error dismissal fails with `error`: `Update::Error`, then
    /// the unchanged transcript (the seam's second rule).
    pub fn fail_next_dismiss(&self, error: &str) {
        *self.dismiss_failure.lock().expect("lock") = Some(error.to_string());
    }

    /// The next branch switch fails as `failure` says.
    pub fn fail_next_branch(&self, failure: BranchFailure) {
        *self.branch_failure.lock().expect("lock") = Some(failure);
    }

    // chat (stage four's chat agent)
    /// Streams `script`'s turns from now on.
    pub fn set_script(&self, script: Script) {
        *self.script.lock().expect("lock") = script;
    }

    /// Stores sent files, shots and the picture a shot returns under
    /// `folder` (`attachments/<chat>/`, `images/<chat>/`), so the core's
    /// addresses name files there; `picture` is what `see_build` returns.
    pub fn keep_files(&self, folder: &Path, picture: &Path) {
        std::fs::create_dir_all(folder)
            .unwrap_or_else(|e| panic!("cannot create {}: {e}", folder.display()));
        self.data.lock().expect("lock").files = Some(folder.to_path_buf());
        *self.picture.lock().expect("lock") = Some(picture.to_path_buf());
    }

    /// Refuses every send to `chat` (`new` for a new chat) with
    /// [`REFUSAL`], as the route refuses before the turn begins.
    pub fn refuse_sends_to(&self, chat: &str) {
        self.refuse_sends_with(chat, REFUSAL);
    }

    /// Refuses every send to `chat` with `error` (an attachment the core's
    /// limits refuse, say).
    pub fn refuse_sends_with(&self, chat: &str, error: &str) {
        self.refusing
            .lock()
            .expect("lock")
            .insert(chat.to_string(), error.to_string());
    }

    /// A join of `chat`'s running turn (its list row's `running_since`)
    /// streams a turn, as a turn begun elsewhere would replay.
    pub fn run_turn_on_join(&self, chat: &str) {
        self.joinable.lock().expect("lock").push(chat.to_string());
    }

    /// The next turn stops itself after `steps` of its steps, as if Stop
    /// were pressed then.
    pub fn stop_after(&self, steps: usize) {
        *self.stop_at.lock().expect("lock") = Some(steps);
    }

    fn steps(&self) -> Vec<Step> {
        match *self.script.lock().expect("lock") {
            Script::Plain => turns::plain(&self.streamed),
            Script::Full => {
                let picture = self
                    .picture
                    .lock()
                    .expect("lock")
                    .clone()
                    .expect("a full turn needs keep_files (its shot's picture)");
                turns::full(&self.streamed, &picture)
            }
        }
    }

    /// The chat's row shows its turn running since `since`; the lists go
    /// out.
    fn set_running(&self, chat: &str, since: Option<i64>) {
        self.with_chat(chat, |c| c.row.running_since = since);
        self.publish_lists();
    }

    /// A user message's payload: its text, files (stored under the files
    /// folder) and selections, as the route stores them.
    fn user_message(&self, chat: &str, outgoing: &crate::Outgoing) -> Value {
        if outgoing.attachments.is_empty() && outgoing.selections.is_empty() {
            return user_payload(&outgoing.text);
        }
        let mut parts = vec![json!({ "type": "text", "text": outgoing.text })];
        for (n, file) in outgoing.attachments.iter().enumerate() {
            let folder = self
                .data
                .lock()
                .expect("lock")
                .files
                .clone()
                .expect("the stand-in stores files only with keep_files");
            let stored = format!("{n}-{}", file.name);
            let path = folder.join("attachments").join(chat).join(&stored);
            std::fs::create_dir_all(path.parent().expect("a folder"))
                .unwrap_or_else(|e| panic!("cannot create {}: {e}", path.display()));
            std::fs::write(&path, &file.data)
                .unwrap_or_else(|e| panic!("cannot write {}: {e}", path.display()));
            parts.push(json!({ "type": "attachment", "attachment": {
                "name": file.name, "type": file.content_type, "size": file.size,
                "url": format!("{}{chat}/{stored}", crate::state::files::ATTACHMENTS),
                "path": format!("/work/attachments/{stored}"),
            } }));
        }
        for selection in &outgoing.selections {
            parts.push(json!({ "type": "selection", "selection": {
                "letter": selection.letter, "label": selection.label,
                "min": selection.min, "max": selection.max,
                "blocks": selection.blocks, "block": selection.block,
            } }));
        }
        json!({ "role": "user", "content": parts })
    }

    /// Starts `chat`'s turn begun at `since` with `steps`: its snapshot is
    /// the chat as stored now; with `announce`, `Update::TurnJoined` and the
    /// snapshot go to the window first (a send the core took, a join). The
    /// turn then runs (or waits for [`StandIn::advance`] when turns go by
    /// hand).
    fn begin(&self, chat: String, since: i64, steps: Vec<Step>, announce: bool) {
        self.stop.store(false, Ordering::SeqCst);
        let runner = self.runner();
        {
            let snapshot = transcript_of(&self.data.lock().expect("lock"), &chat, Some(since));
            let mut live = runner.live.lock().expect("lock");
            if announce {
                runner.window.publish(Update::TurnJoined {
                    chat: chat.clone(),
                    since,
                });
                runner.window.publish(snapshot.clone());
            }
            live.insert(
                chat.clone(),
                Live {
                    since,
                    snapshot,
                    events: Vec::new(),
                    steps: steps.into(),
                    taken: 0,
                    stop_at: self.stop_at.lock().expect("lock").take(),
                    tool: None,
                    streamed: String::new(),
                },
            );
        }
        if self.by_hand.load(Ordering::SeqCst) {
            return;
        }
        let rate = self.rate;
        let drive = move || {
            while let Some(delta) = runner.step(&chat) {
                if delta && !rate.is_zero() {
                    std::thread::sleep(rate);
                }
            }
        };
        if rate.is_zero() {
            drive();
        } else {
            std::thread::spawn(drive);
        }
    }

    fn runner(&self) -> Runner {
        Runner {
            data: self.data.clone(),
            window: self.window(),
            live: self.live.clone(),
            stop: self.stop.clone(),
            picture: self.picture.lock().expect("lock").clone(),
        }
    }

    // chat-fix: turns by hand, for the join's order.
    /// Turns wait to be stepped ([`StandIn::advance`], [`StandIn::finish`])
    /// instead of running at once.
    pub fn turns_by_hand(&self) {
        self.by_hand.store(true, Ordering::SeqCst);
    }

    /// Starts a turn in `chat` as the core runs one the window did not
    /// send (a bench round, a chat running in the background): the list
    /// shows it running and its events are published as it goes, though
    /// no window joined it.
    pub fn start_turn(&self, chat: &str) {
        let since = now_ms();
        self.set_running(chat, Some(since));
        let steps = self.steps();
        self.begin(chat.to_string(), since, steps, false);
    }

    /// Runs `chat`'s turn on by `events` published events; whether it
    /// still runs.
    pub fn advance(&self, chat: &str, events: usize) -> bool {
        let runner = self.runner();
        for _ in 0..events {
            if runner.step(chat).is_none() {
                return false;
            }
        }
        true
    }

    /// Runs `chat`'s turn to its end.
    pub fn finish(&self, chat: &str) {
        let runner = self.runner();
        while runner.step(chat).is_some() {}
    }

    /// `chat`'s `Update::Transcript` as stored now (for tests).
    pub fn transcript_for_test(&self, chat: &str) -> Update {
        transcript_of(&self.data.lock().expect("lock"), chat, None)
    }

    /// The events `chat`'s running turn published so far, and the
    /// snapshot of its start (for tests).
    pub fn live_turn(&self, chat: &str) -> Option<(Update, Vec<TurnEvent>)> {
        self.live
            .lock()
            .expect("lock")
            .get(chat)
            .map(|l| (l.snapshot.clone(), l.events.clone()))
    }

    fn window(&self) -> Arc<dyn Window> {
        self.window
            .lock()
            .expect("lock")
            .clone()
            .expect("attach ran first")
    }

    fn publish_lists(&self) {
        let data = self.data.lock().expect("lock");
        let lists = lists_of(&data);
        drop(data);
        self.window().publish(Update::Conversations(lists));
        // bench agent: the watched benches whose details changed.
        self.bench_lists_published();
    }

    /// The end of an archive, restore or delete (the seam's second rule):
    /// the archive again when it was read, then the list.
    /// The list, then the archive when it was read (the core's
    /// `end_in_lists`).
    fn archive_changed(&self) {
        self.publish_lists();
        if self.archive_read.load(Ordering::SeqCst) {
            self.publish_archive();
        }
    }

    fn publish_archive(&self) {
        let archive = self.archive_now();
        self.window().publish(Update::Archive(Ok(archive)));
    }

    /// The archive as it is now (chat-fix-2: split from `publish_archive`
    /// for the delayed read).
    fn archive_now(&self) -> Archive {
        let data = self.data.lock().expect("lock");
        let conversations = data
            .chats
            .iter()
            .filter(|c| c.row.bench.is_none())
            .filter_map(|c| {
                Some(ArchivedConversation {
                    id: c.row.id.clone(),
                    title: c.row.title.clone(),
                    provider: c.row.provider.clone(),
                    model: c.row.model.clone(),
                    archived_at: c.archived_at?,
                    expires_at: c.archived_at? + 7 * 24 * 60 * 60 * 1000,
                })
            })
            .collect();
        // sidebar agent: the archived benches, newest first.
        let benches = data
            .archived_benches
            .iter()
            .map(|a| crate::ArchivedBench {
                id: a.bench.id.clone(),
                title: a.bench.title.clone(),
                archived_at: a.archived_at,
                expires_at: a.archived_at + 7 * 24 * 60 * 60 * 1000,
                models: a.models,
            })
            .collect();
        drop(data);
        Archive {
            conversations,
            benches,
        }
    }

    fn publish_transcript(&self, chat: &str) {
        let update = transcript_of(&self.data.lock().expect("lock"), chat, None);
        self.window().publish(update);
    }

    fn with_chat<R>(&self, chat: &str, f: impl FnOnce(&mut ChatData) -> R) -> R {
        let mut data = self.data.lock().expect("lock");
        let found = data
            .chats
            .iter_mut()
            .find(|c| c.row.id == chat)
            .unwrap_or_else(|| panic!("the stand-in has no chat {chat}"));
        f(found)
    }

    /// The user's message is stored (in place of the one it edits); the
    /// turn is taken, then streams (or, in [`FAILING_CHAT`], fails).
    fn answer(&self, chat: String, provider: String, model: String, outgoing: crate::Outgoing) {
        if let Some(error) = self.refusing.lock().expect("lock").get(&chat).cloned() {
            return self.window().publish(Update::SendRefused {
                target: outgoing.target,
                error,
            });
        }
        // par-chat: the route's pair check, then the chat takes the
        // message's provider, model, effort and resolution (`handleChat`).
        if let Some(error) = self.refuse_pair(&provider, &model) {
            return self.window().publish(Update::SendRefused {
                target: outgoing.target,
                error: error.to_string(),
            });
        }
        let payload = self.user_message(&chat, &outgoing);
        let title: String = outgoing.text.chars().take(40).collect();
        self.with_chat(&chat, |c| {
            // A row made by `CreateChat` is titled from its first message.
            if c.rows.is_empty() && c.row.title.is_empty() {
                c.row.title = title;
            }
            c.row.provider = provider;
            c.row.model = model;
            c.row.effort = outgoing.effort.clone();
            c.row.resolution = outgoing.resolution;
        });
        self.with_chat(&chat, |c| {
            if let Some(of) = &outgoing.edit_of {
                // The edit branches from the edited message's parent.
                let parent = c
                    .rows
                    .iter()
                    .find(|r| r.id == of.0)
                    .unwrap_or_else(|| panic!("messageId must point at a user message: {}", of.0))
                    .parent
                    .clone();
                c.head = parent;
            }
            c.push("u", payload)
        });
        self.take_turn(chat);
    }

    /// The core took a send: the list shows the turn running, then
    /// `TurnJoined`, the snapshot and the turn's events.
    fn take_turn(&self, chat: String) {
        let since = now_ms();
        self.set_running(&chat, Some(since));
        if chat == FAILING_CHAT {
            return self.fail(chat, since);
        }
        let steps = self.steps();
        self.begin(chat, since, steps, true);
    }

    /// A turn that fails at once: `TurnJoined` and its snapshot, then the
    /// error is stored and the turn's event is the error and no `done` (a
    /// failed turn sends none), then the transcript and the list.
    fn fail(&self, chat: String, since: i64) {
        let runner = self.runner();
        let snapshot = transcript_of(&self.data.lock().expect("lock"), &chat, Some(since));
        let _live = runner.live.lock().expect("lock");
        runner.window.publish(Update::TurnJoined {
            chat: chat.clone(),
            since,
        });
        runner.window.publish(snapshot);
        let id = self.with_chat(&chat, |c| {
            c.push("e", json!({ "role": "error", "content": FAILURE }))
        });
        runner.window.publish(Update::Turn {
            chat: chat.clone(),
            event: TurnEvent::Error {
                message: FAILURE.to_string(),
                message_id: Some(id),
            },
        });
        runner.after_end(&chat);
    }

    fn save_preferences(&self, preferences: Preferences) {
        let mut data = self.data.lock().expect("lock");
        if let Some(path) = &data.preferences_file {
            write_preferences(path, &preferences);
        }
        data.preferences = preferences;
    }

    fn search(&self, scope: SearchScope, query: &str) -> Vec<SearchHit> {
        let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
        let data = self.data.lock().expect("lock");
        let mut results = Vec::new();
        match scope {
            SearchScope::Build => {
                for chat in data
                    .chats
                    .iter()
                    .filter(|c| c.row.bench.is_none() && c.archived_at.is_none())
                {
                    // sidebar agent: as the store, a match in the title alone
                    // gives no snippet.
                    let texts: Vec<String> = chat
                        .branch()
                        .iter()
                        .map(|r| message_text(&r.payload))
                        .collect();
                    if let Some(found) = texts.iter().find_map(|t| snippet(t, &words)) {
                        results.push(SearchHit {
                            id: chat.row.id.clone(),
                            snippet: Some(found),
                        });
                    } else if snippet(&chat.row.title, &words).is_some() {
                        results.push(SearchHit {
                            id: chat.row.id.clone(),
                            snippet: None,
                        });
                    }
                }
            }
            SearchScope::Bench => {
                for bench in &data.benches {
                    if let Some(snippet) = snippet(&bench.title, &words) {
                        results.push(SearchHit {
                            id: bench.id.clone(),
                            snippet: Some(snippet),
                        });
                    }
                }
            }
        }
        results
    }
}

/// A bench model's round and halt from its stored messages.
fn seat_state(data: &mut ChatData) {
    let rounds = data
        .rows
        .iter()
        .filter(|r| r.payload["role"] == "user" && r.payload["seeBuild"] != true)
        .count() as u32;
    let last = data.rows.last().map(|r| r.payload.clone());
    if let Some(seat) = data.row.bench.as_mut() {
        seat.round = rounds;
        seat.halted = match last {
            Some(p) if p["role"] == "error" => Some(Halted::Failed),
            Some(p) if p["stopped"] == true => Some(Halted::Stopped),
            _ => None,
        };
    }
}

fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|v| v.as_str().map(str::to_string))
        .collect()
}

fn chat_row_of(row: &Value) -> ChatRow {
    ChatRow {
        id: row["id"].as_str().expect("a chat id").into(),
        title: row["title"].as_str().unwrap_or_default().into(),
        provider: row["provider"].as_str().unwrap_or_default().into(),
        model: row["model"].as_str().unwrap_or_default().into(),
        effort: row["effort"].as_str().map(str::to_string),
        resolution: Resolution::of(row["resolution"].as_u64().unwrap_or(1) as u32),
        created_at: row["createdAt"].as_str().unwrap_or_default().into(),
        bench: row["bench"].as_object().map(|b| BenchSeat {
            bench: b["bench"].as_str().unwrap_or_default().into(),
            slot: b["slot"].as_u64().unwrap_or(0) as u32,
            round: b["round"].as_u64().unwrap_or(0) as u32,
            halted: match b["halted"].as_str() {
                Some("failed") => Some(Halted::Failed),
                Some("stopped") => Some(Halted::Stopped),
                _ => None,
            },
        }),
        unread: row["unread"].as_bool().unwrap_or(false),
        message_count: row["messageCount"].as_u64().unwrap_or(0) as u32,
        prompts: strings(&row["prompts"]),
        running_since: row["runningSince"].as_i64(),
    }
}

/// A chat's `Update::Transcript` as stored now (its header's running turn
/// `running_since`), or `TranscriptFailed` when it cannot be read.
fn transcript_of(data: &Data, chat: &str, running_since: Option<i64>) -> Update {
    let Some(found) = data
        .chats
        .iter()
        .find(|c| c.row.id == chat && c.archived_at.is_none())
    else {
        return Update::TranscriptFailed {
            chat: chat.into(),
            error: "Conversation not found".into(),
        };
    };
    let messages: Vec<StoredMessage> = found
        .branch()
        .into_iter()
        .map(|r| {
            // Decoded as the store decodes a row it loads.
            StoredMessage::from_payload(
                &r.id,
                r.parent.as_deref(),
                r.position,
                r.payload.to_string(),
            )
            .unwrap_or_else(|e| panic!("a stored message the store refuses: {e}"))
        })
        .collect();
    let tree = MessageTree {
        head_id: found.head.clone(),
        messages: found
            .rows
            .iter()
            .map(|r| TreeMessage {
                id: r.id.clone(),
                parent_id: r.parent.clone(),
                position: i64::from(r.position),
                role: r.payload["role"].as_str().map(str::to_string),
                branchable: r.payload["role"] == "user" && r.payload["seeBuild"] != true,
            })
            .collect(),
    };
    let header = ConversationHeader {
        id: found.row.id.clone(),
        title: found.row.title.clone(),
        provider: found.row.provider.clone(),
        model: found.row.model.clone(),
        effort: found.row.effort.clone(),
        resolution: found.row.resolution.value(),
        bench: found.row.bench.as_ref().map(|b| (b.bench.clone(), b.slot)),
        running_since,
        share: None,
    };
    Update::Transcript {
        chat: chat.to_string(),
        header,
        messages,
        tree,
    }
}

// chat-fix: a running turn, kept so a join can replay it.
/// A turn the stand-in runs: its start, the chat as stored then, the
/// events published so far, and the steps left.
struct Live {
    since: i64,
    snapshot: Update,
    events: Vec<TurnEvent>,
    steps: std::collections::VecDeque<Step>,
    taken: usize,
    stop_at: Option<usize>,
    /// The tool call running, which a stop cancels.
    tool: Option<(String, String)>,
    /// The answer streamed since the last full message.
    streamed: String,
}

/// What runs the turns: the data, the window, the running turns.
#[derive(Clone)]
struct Runner {
    data: Arc<Mutex<Data>>,
    window: Arc<dyn Window>,
    live: Arc<Mutex<HashMap<String, Live>>>,
    stop: Arc<AtomicBool>,
    picture: Option<PathBuf>,
}

impl Runner {
    /// Publishes `event` of `chat`'s turn and keeps it for joins. Every
    /// running turn's events go to the window, joined or not.
    fn emit(&self, turn: &mut Live, chat: &str, event: TurnEvent) {
        turn.events.push(event.clone());
        self.window.publish(Update::Turn {
            chat: chat.to_string(),
            event,
        });
    }

    fn store(&self, chat: &str, prefix: &str, payload: Value) {
        self.data
            .lock()
            .expect("lock")
            .chats
            .iter_mut()
            .find(|c| c.row.id == chat)
            .expect("the chat exists")
            .push(prefix, payload);
    }

    /// Takes `chat`'s turn on to its next published event (rows are stored
    /// on the way): whether that event was a streamed piece, or `None`
    /// once the turn has ended (a stop ends it as the route ends a stopped
    /// turn: a running tool cancelled, `(stopped by user)`).
    fn step(&self, chat: &str) -> Option<bool> {
        let mut live = self.live.lock().expect("lock");
        let turn = live.get_mut(chat)?;
        loop {
            if self.stop.load(Ordering::SeqCst) || turn.stop_at.is_some_and(|at| turn.taken >= at) {
                self.stop_turn(turn, chat);
                live.remove(chat);
                drop(live);
                return None;
            }
            let Some(step) = turn.steps.pop_front() else {
                self.emit(turn, chat, TurnEvent::Done);
                live.remove(chat);
                self.after_end(chat);
                return None;
            };
            turn.taken += 1;
            match step {
                Step::Delta(event) => {
                    if let TurnEvent::AssistantDelta(text) = &event {
                        turn.streamed.push_str(text);
                    }
                    self.emit(turn, chat, event);
                    return Some(true);
                }
                Step::Event(event) => {
                    match &event {
                        TurnEvent::ToolStart { call_id, name, .. } => {
                            turn.tool = Some((call_id.clone(), name.clone()))
                        }
                        TurnEvent::ToolResult { .. } => turn.tool = None,
                        TurnEvent::Assistant { .. } | TurnEvent::CompletionRetry { .. } => {
                            turn.streamed.clear()
                        }
                        _ => {}
                    }
                    self.emit(turn, chat, event);
                    return Some(false);
                }
                Step::Store(prefix, payload) => self.store(chat, prefix, payload),
                Step::Shot(name) => self.shot(chat, name),
            }
        }
    }

    fn shot(&self, chat: &str, name: &str) {
        let folder = self
            .data
            .lock()
            .expect("lock")
            .files
            .clone()
            .expect("a shot needs keep_files");
        let path = folder.join("images").join(chat).join(name);
        std::fs::create_dir_all(path.parent().expect("a folder"))
            .unwrap_or_else(|e| panic!("cannot create {}: {e}", path.display()));
        std::fs::copy(self.picture.as_ref().expect("a picture"), &path)
            .unwrap_or_else(|e| panic!("cannot write {}: {e}", path.display()));
        self.store(
            chat,
            "u",
            json!({ "role": "user", "seeBuild": true, "content": [
                { "type": "image_url", "image_url": {
                    "url": format!("{}{chat}/{name}", crate::state::files::IMAGES) } }
            ] }),
        );
    }

    /// A stop: the running tool cancelled, `(stopped by user)`, `Done`, then
    /// the transcript and the list.
    fn stop_turn(&self, turn: &mut Live, chat: &str) {
        if let Some((call_id, name)) = turn.tool.take() {
            let content = json!({ "ok": false, "cancelled": true }).to_string();
            self.emit(
                turn,
                chat,
                TurnEvent::ToolResult {
                    call_id: call_id.clone(),
                    name: name.clone(),
                    shown: Box::new(mcbench_agent::tool_shown(&name, &content)),
                    result: json!({ "ok": false, "cancelled": true }),
                    image_urls: None,
                },
            );
            self.store(
                chat,
                "t",
                json!({ "role": "tool", "tool_call_id": call_id, "content": content }),
            );
        }
        self.emit(
            turn,
            chat,
            TurnEvent::Assistant {
                text: "(stopped by user)".into(),
                last: true,
            },
        );
        self.store(
            chat,
            "a",
            json!({ "role": "assistant", "content": "(stopped by user)",
                "stopped": true, "reply": turn.streamed }),
        );
        self.emit(turn, chat, TurnEvent::Done);
        self.after_end(chat);
    }

    /// After a turn's `Done` or `Error`: the chat's transcript as stored,
    /// then the list with the turn no longer running and the chat unread
    /// (finished after it was last seen, as the store has it) until the
    /// window marks it seen.
    fn after_end(&self, chat: &str) {
        let mut data = self.data.lock().expect("lock");
        let row = &mut data
            .chats
            .iter_mut()
            .find(|c| c.row.id == chat)
            .expect("the chat exists")
            .row;
        row.running_since = None;
        row.unread = true;
        let transcript = transcript_of(&data, chat, None);
        let lists = lists_of(&data);
        drop(data);
        self.window.publish(transcript);
        self.window.publish(Update::Conversations(lists));
    }
}

fn lists_of(data: &Data) -> ConversationList {
    // sidebar agent: an archived bench's models are not listed.
    let listed = |c: &&ChatData| {
        c.archived_at.is_none()
            && !c.rows.is_empty()
            && c.row
                .bench
                .as_ref()
                .is_none_or(|seat| data.benches.iter().any(|b| b.id == seat.bench))
    };
    let archived_chats = data
        .chats
        .iter()
        .filter(|c| c.archived_at.is_some() && c.row.bench.is_none())
        .count();
    ConversationList {
        conversations: data
            .chats
            .iter()
            .filter(listed)
            .map(|c| c.row.listed())
            .collect(),
        benches: data.benches.clone(),
        prompt_titles: data
            .prompt_titles
            .iter()
            .map(|(k, v)| (k.clone(), Value::String(v.clone())))
            .collect(),
        archived: (archived_chats, data.archived_benches.len()),
    }
}

/// A message's plain text for search: its text content.
fn message_text(payload: &Value) -> String {
    match &payload["content"] {
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|p| p["text"].as_str())
            .collect::<Vec<_>>()
            .join(" "),
        _ => String::new(),
    }
}

/// The text around the first word of `words` found in `text`, split into
/// plain and matched parts; none when a word is missing.
fn snippet(text: &str, words: &[String]) -> Option<String> {
    let lower = text.to_lowercase();
    if words.is_empty() || !words.iter().all(|w| lower.contains(w.as_str())) {
        return None;
    }
    let at = lower.find(words[0].as_str())?;
    // Lowercasing can change byte lengths; cut on char boundaries of `text`.
    let start = text
        .char_indices()
        .map(|(i, _)| i)
        .rfind(|i| *i + 40 <= at)
        .unwrap_or(0);
    let end = (at + words[0].len()).min(text.len());
    let end = (end..=text.len())
        .find(|i| text.is_char_boundary(*i))
        .unwrap_or(text.len());
    let at = (0..=at)
        .rev()
        .find(|i| text.is_char_boundary(*i))
        .unwrap_or(0);
    let mut parts = Vec::new();
    if start > 0 {
        parts.push(("\u{2026}".to_string(), false));
    }
    parts.push((text[start..at].to_string(), false));
    parts.push((text[at..end].to_string(), true));
    let tail_end = (end + 60).min(text.len());
    let tail_end = (tail_end..=text.len())
        .find(|i| text.is_char_boundary(*i))
        .unwrap_or(text.len());
    parts.push((text[end..tail_end].to_string(), false));
    if tail_end < text.len() {
        parts.push(("\u{2026}".to_string(), false));
    }
    // The store's marks: \u{1} opens a match, \u{2} closes it.
    Some(
        parts
            .into_iter()
            .map(|(t, matched)| if matched { format!("\u{1}{t}\u{2}") } else { t })
            .collect(),
    )
}

/// The preferences kept at `path`, as the core keeps them: no file is the
/// first run (the defaults); a file that does not parse is an error the
/// window shows, then the defaults.
fn read_preferences(path: Option<&Path>) -> (Preferences, Option<String>) {
    let Some(path) = path.filter(|p| p.exists()) else {
        // chat: the first run's last model is the stand-in's, so a new chat
        // sends without the model picker (the pickers' milestone).
        return (
            Preferences {
                last_model: Some(("anthropic".into(), MODEL.into())),
                ..Preferences::default()
            },
            None,
        );
    };
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    match Preferences::from_text(&text) {
        Ok(preferences) => (preferences, None),
        Err(e) => (
            Preferences::default(),
            Some(format!("{} could not be read: {e}", path.display())),
        ),
    }
}

fn write_preferences(path: &Path, p: &Preferences) {
    std::fs::write(path, p.to_text())
        .unwrap_or_else(|e| panic!("cannot write {}: {e}", path.display()));
}

/// Now, epoch ms.
fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("after 1970")
        .as_millis() as i64
}

/// A stored file's type and whether it shows inline, by its extension
/// (`server/index.js`: these are sent inline, anything else as a download).
fn file_type(name: &str) -> (&'static str, bool) {
    let extension = name.rsplit('.').next().unwrap_or("").to_lowercase();
    match extension.as_str() {
        "png" => ("image/png", true),
        "jpg" | "jpeg" => ("image/jpeg", true),
        "webp" => ("image/webp", true),
        "gif" => ("image/gif", true),
        "svg" => ("image/svg+xml", true),
        "pdf" => ("application/pdf", true),
        "txt" => ("text/plain", true),
        "md" => ("text/markdown", true),
        "csv" => ("text/csv", true),
        _ => ("application/octet-stream", false),
    }
}

impl Core for StandIn {
    fn image_file(&self, address: &str) -> Result<PathBuf, String> {
        let rest = address
            .strip_prefix(crate::state::files::IMAGES)
            .unwrap_or_else(|| panic!("not a picture's address: {address}"));
        let data = self.data.lock().expect("lock");
        data.files
            .as_ref()
            .map(|folder| folder.join("images").join(rest))
            .filter(|path| path.is_file())
            .ok_or_else(|| "Image not found".to_string())
    }

    fn attachment_file(&self, address: &str) -> Result<crate::AttachmentFile, String> {
        let rest = address
            .strip_prefix(crate::state::files::ATTACHMENTS)
            .unwrap_or_else(|| panic!("not an attachment's address: {address}"));
        let data = self.data.lock().expect("lock");
        let path = data
            .files
            .as_ref()
            .map(|folder| folder.join("attachments").join(rest))
            .filter(|path| path.is_file())
            .ok_or_else(|| "Attachment not found".to_string())?;
        let file_name = rest.rsplit('/').next().unwrap_or(rest).to_string();
        let (content_type, inline) = file_type(&file_name);
        Ok(crate::AttachmentFile {
            path,
            content_type,
            inline,
            file_name,
        })
    }

    fn preferences(&self) -> Preferences {
        self.data.lock().expect("lock").preferences.clone()
    }

    // pane agent: made-up block icons.
    fn block_icon(&self, id: &str) -> Result<Vec<u8>, String> {
        self.icon_of(id)
    }

    // pane agent: stopped at once: the clock and the fake pane end.
    fn shutdown(&self) {
        self.stop_live();
    }

    fn attach(&self, window: Arc<dyn Window>) {
        *self.window.lock().expect("lock") = Some(window.clone());
        // start-place agent: the first lists may come late.
        match self.lists_late.lock().expect("lock").take() {
            None => self.publish_lists(),
            Some(None) => {}
            Some(Some(delay)) => {
                let (data, window) = (lists_of(&self.data.lock().expect("lock")), window.clone());
                std::thread::spawn(move || {
                    std::thread::sleep(delay);
                    window.publish(Update::Conversations(data));
                });
            }
        }
        let data = self.data.lock().expect("lock");
        let (providers, favorites) = (data.providers.clone(), data.favorites.clone());
        drop(data);
        // par-chat: the first read fails when asked (`fail_providers_read`).
        window.publish(self.providers_read(providers));
        if let Some(error) = self.data.lock().expect("lock").preferences_error.clone() {
            window.publish(Update::Error(error));
        }
        window.publish(Update::Favorites(favorites));
        if let Some(steps) = self.setup.lock().expect("lock").clone() {
            window.publish(Update::Setup(steps));
        }
        // pane agent: the pane's handle once the renderer has started.
        self.publish_pane();
        // bench agent: a model's library edit, when played.
        self.bench_attached();
    }

    fn send(&self, command: Command) {
        match command {
            // The transcript, or its failure, and nothing else: only
            // `MarkSeen` marks a chat seen.
            Command::Open { chat } => {
                // chat-fix-2: a chat that cannot be read (`--unreadable`).
                if self.unreadable.lock().expect("lock").contains(&chat) {
                    return self.window().publish(Update::TranscriptFailed {
                        chat,
                        error: UNREADABLE.to_string(),
                    });
                }
                self.publish_transcript(&chat)
            }
            // par-chat: every send is recorded when asked; a chat's pair is
            // checked, then becomes the chat's.
            Command::Send(outgoing) => {
                let outgoing = self.recorded(outgoing);
                match outgoing.target.clone() {
                    Target::Chat {
                        id,
                        provider,
                        model,
                    } => self.answer(id, provider, model, outgoing),
                    Target::NewChat { provider, model } => {
                        if let Some(error) = self.refusing.lock().expect("lock").get("new").cloned()
                        {
                            return self.window().publish(Update::SendRefused {
                                target: outgoing.target,
                                error,
                            });
                        }
                        let id = {
                            let mut data = self.data.lock().expect("lock");
                            data.created += 1;
                            let id = format!("chat-new-{}", data.created);
                            let title: String = outgoing.text.chars().take(40).collect();
                            data.chats.insert(
                                0,
                                ChatData {
                                    row: ChatRow {
                                        id: id.clone(),
                                        title,
                                        provider,
                                        model,
                                        effort: outgoing.effort.clone(),
                                        resolution: outgoing.resolution,
                                        created_at: "2026-11-17T12:00:00Z".into(),
                                        bench: None,
                                        unread: false,
                                        message_count: 0,
                                        prompts: Vec::new(),
                                        running_since: None,
                                    },
                                    rows: Vec::new(),
                                    head: None,
                                    archived_at: None,
                                },
                            );
                            id
                        };
                        // The row is listed once it has a message (`emptyChat`).
                        let payload = self.user_message(&id, &outgoing);
                        self.with_chat(&id, |c| c.push("u", payload));
                        self.publish_lists();
                        self.take_turn(id);
                    }
                    // bench agent: a bench's rounds (`dev/standin/bench.rs`).
                    Target::NewBench { .. } | Target::Bench(_) => self.bench_send(outgoing),
                }
            }
            // par-chat: the providers read again (`ui-seam-parity.md` item 2).
            Command::ReadProviders => {
                let providers = self.data.lock().expect("lock").providers.clone();
                self.window().publish(self.providers_read(providers));
            }
            // par-chat: a new chat's row (`ui-seam-parity.md` item 1).
            Command::CreateChat { provider, model } => {
                let hold = {
                    let mut pairs = self.pairs.lock().expect("lock");
                    pairs.creates += 1;
                    if pairs.hold_creates {
                        pairs.held_creates.push((provider.clone(), model.clone()));
                    }
                    pairs.hold_creates
                };
                if !hold {
                    self.create_chat(provider, model);
                }
            }
            Command::Stop { chat } => {
                // bench agent: a bench model stops on its own.
                if !self.bench_stop(&chat) {
                    self.stop.store(true, Ordering::SeqCst)
                }
            }
            Command::Branch { chat, message } => {
                // A switch ends in the chat's transcript, or in its failed
                // read (the agreed rule, `Command::Branch`).
                match self.branch_failure.lock().expect("lock").take() {
                    Some(BranchFailure::Refused(error)) => {
                        self.window().publish(Update::Error(error));
                        return self.publish_transcript(&chat);
                    }
                    Some(BranchFailure::Unreadable(error)) => {
                        return self
                            .window()
                            .publish(Update::TranscriptFailed { chat, error });
                    }
                    None => {}
                }
                self.with_chat(&chat, |c| {
                    // The branch holding the message, to its newest end.
                    let mut head = message.0.clone();
                    while let Some(child) = c
                        .rows
                        .iter()
                        .filter(|r| r.parent.as_deref() == Some(head.as_str()))
                        .max_by_key(|r| r.position)
                    {
                        head = child.id.clone();
                    }
                    c.head = Some(head);
                });
                self.publish_transcript(&chat);
            }
            // A running turn's join: `TurnJoined`, its snapshot, every
            // event so far, then the live ones (the seam's order; the core
            // de-duplicates, the window does not). A turn set to run on a
            // join (`--join`) starts now; any other list row's
            // `running_since` is found ended.
            Command::JoinTurn { chat } => {
                {
                    let live = self.live.lock().expect("lock");
                    if let Some(turn) = live.get(&chat) {
                        let window = self.window();
                        window.publish(Update::TurnJoined {
                            chat: chat.clone(),
                            since: turn.since,
                        });
                        window.publish(turn.snapshot.clone());
                        for event in &turn.events {
                            window.publish(Update::Turn {
                                chat: chat.clone(),
                                event: event.clone(),
                            });
                        }
                        return;
                    }
                }
                let joinable = {
                    let mut joinable = self.joinable.lock().expect("lock");
                    let at = joinable.iter().position(|c| *c == chat);
                    at.map(|at| joinable.remove(at))
                };
                match joinable {
                    Some(chat) => {
                        let since = self
                            .with_chat(&chat, |c| c.row.running_since)
                            .unwrap_or_else(now_ms);
                        let steps = self.steps();
                        self.begin(chat, since, steps, true);
                    }
                    None => self.window().publish(Update::TurnNotRunning { chat }),
                }
            }
            // It ends in the list (and the archive when read), after the
            // error on a failure.
            Command::MarkSeen { chat } => {
                match self.seen_failure.lock().expect("lock").take() {
                    Some(error) => self.window().publish(Update::Error(error)),
                    None => {
                        self.with_chat(&chat, |c| c.row.unread = false);
                    }
                }
                self.archive_changed();
            }
            // It ends in the chat's transcript (the seam's second rule).
            Command::DismissError { chat, message } => {
                if let Some(error) = self.dismiss_failure.lock().expect("lock").take() {
                    self.window().publish(Update::Error(error));
                    return self.publish_transcript(&chat);
                }
                self.with_chat(&chat, |c| {
                    let row = c
                        .rows
                        .iter_mut()
                        .find(|r| r.id == message.0)
                        .unwrap_or_else(|| panic!("no message {} in {chat}", message.0));
                    assert_eq!(
                        row.payload["role"], "error",
                        "messageId must point at an error message"
                    );
                    row.payload["dismissed"] = json!(true);
                });
                self.publish_transcript(&chat);
            }
            Command::Archive(Subject::Chat(chat)) => {
                self.with_chat(&chat, |c| c.archived_at = Some(now_ms()));
                self.archive_changed();
            }
            Command::Restore(Subject::Chat(chat)) => {
                self.with_chat(&chat, |c| c.archived_at = None);
                self.archive_changed();
            }
            Command::Delete(Subject::Chat(chat)) => {
                self.data
                    .lock()
                    .expect("lock")
                    .chats
                    .retain(|c| !(c.row.id == chat && c.archived_at.is_some()));
                // sidebar agent: the list follows (its archived counts).
                self.archive_changed();
            }
            // sidebar agent: a bench's archive, restore and delete.
            Command::Archive(Subject::Bench(bench)) => {
                {
                    let mut data = self.data.lock().expect("lock");
                    let at = data
                        .benches
                        .iter()
                        .position(|b| b.id == bench)
                        .unwrap_or_else(|| panic!("the stand-in has no bench {bench}"));
                    let row = data.benches.remove(at);
                    let models = data
                        .chats
                        .iter()
                        .filter(|c| c.row.bench.as_ref().is_some_and(|s| s.bench == bench))
                        .count() as i64;
                    data.archived_benches.insert(
                        0,
                        ArchivedSeat {
                            bench: row,
                            archived_at: now_ms(),
                            models,
                        },
                    );
                }
                self.archive_changed();
            }
            Command::Restore(Subject::Bench(bench)) => {
                {
                    let mut data = self.data.lock().expect("lock");
                    let at = data
                        .archived_benches
                        .iter()
                        .position(|a| a.bench.id == bench)
                        .unwrap_or_else(|| panic!("the stand-in has no archived bench {bench}"));
                    let seat = data.archived_benches.remove(at);
                    data.benches.insert(0, seat.bench);
                }
                self.archive_changed();
            }
            Command::Delete(Subject::Bench(bench)) => {
                // bench agent: it leaves the watched set.
                self.bench_deleted(&bench);
                {
                    let mut data = self.data.lock().expect("lock");
                    data.archived_benches.retain(|a| a.bench.id != bench);
                    data.chats
                        .retain(|c| c.row.bench.as_ref().is_none_or(|s| s.bench != bench));
                }
                self.archive_changed();
            }
            Command::ReadArchive => {
                // chat-fix-2: a slow read (`--archive-ms`) answers later, on
                // a thread of its own, so `Loading…` shows meanwhile.
                if let Some(delay) = *self.archive_delay.lock().expect("lock") {
                    let window = self.window();
                    let fails = self.data.lock().expect("lock").archive_fails;
                    let answer = if fails {
                        Update::Archive(Err(ARCHIVE_FAILURE.to_string()))
                    } else {
                        Update::Archive(Ok(self.archive_now()))
                    };
                    self.archive_read.store(true, Ordering::SeqCst);
                    std::thread::spawn(move || {
                        std::thread::sleep(delay);
                        window.publish(answer);
                    });
                    return;
                }
                self.archive_read.store(true, Ordering::SeqCst);
                if self.data.lock().expect("lock").archive_fails {
                    self.window()
                        .publish(Update::Archive(Err(ARCHIVE_FAILURE.to_string())));
                } else {
                    self.publish_archive()
                }
            }
            Command::Search {
                scope,
                query,
                request,
            } => {
                let results = match self.search_failure.lock().expect("lock").clone() {
                    Some(error) => Err(error),
                    None => Ok(self.search(scope, &query)),
                };
                self.window().publish(Update::Found { request, results });
            }
            Command::Models { provider } => self.answer_models(provider),
            Command::ModelInfo { provider, model } => {
                // par-chat: facts set by `describe_model` come first.
                let described = self
                    .pairs
                    .lock()
                    .expect("lock")
                    .facts
                    .get(&(provider.clone(), model.clone()))
                    .cloned();
                let facts = described.map(Ok).unwrap_or_else(|| {
                    self.data
                        .lock()
                        .expect("lock")
                        .models
                        .get(&provider)
                        .and_then(|list| list.iter().find(|m| m.id == model))
                        .map(|m| m.facts.clone())
                        .ok_or_else(|| {
                            format!("The stand-in's data holds no model {model} of {provider}")
                        })
                });
                self.window().publish(Update::ModelInfo {
                    provider,
                    model,
                    facts,
                });
            }
            Command::SetFavorites(favorites) => self.save_favorites(favorites),
            Command::SavePreferences(preferences) => self.save_preferences(preferences),
            Command::ReadPrompts => {
                // bench agent: `prompts-fail` fails the first read.
                if self.prompts_read_fails() {
                    return self
                        .window()
                        .publish(Update::Prompts(Err(PROMPTS_FAILED.into())));
                }
                let library = self.data.lock().expect("lock").library.clone();
                self.window().publish(Update::Prompts(Ok(library)));
            }
            // settings agent: the settings seam.
            command @ (Command::ReadSettings { .. }
            | Command::SaveSettings(_)
            | Command::SignIn { .. }
            | Command::SignInInput { .. }
            | Command::CancelSignIn { .. }
            | Command::SignOut { .. }
            | Command::InstallCli { .. }
            | Command::RetrySetup
            | Command::ReadLeaderboard
            | Command::ClearVotes
            | Command::ReadShares
            | Command::RemoveShare { .. }
            | Command::InstallWsl
            | Command::RestartComputer) => self.settings_command(command),
            // share agent: the Share panel (`ui-seam-share.md`).
            command @ (Command::ReadShare { .. }
            | Command::Publish { .. }
            | Command::Unshare { .. }) => self.share_command(command),
            // pane agent: the live view.
            Command::Live(command) => self.live_command(command),
            Command::Viewer(input) => self.viewer_input(input),
            Command::DescribeSelection {
                chat,
                request,
                min,
                max,
            } => self.describe_selection(chat, request, min, max),
            Command::ExportSchematic { chat, to } => {
                let known = self.chat_known(&chat);
                self.export("schematic", &to, known)
            }
            Command::ExportWorld { view, to, .. } => {
                let known = self.view_shown(&view);
                self.export("world", &to, known)
            }
            // bench agent: Bench and the prompt library.
            command @ (Command::ReadBench { .. }
            | Command::Vote { .. }
            | Command::Reveal { .. }
            | Command::BenchBranch { .. }
            | Command::RetryMember { .. }
            | Command::Fork { .. }
            | Command::SavePrompts(_)
            | Command::GeneratePrompt { .. }
            | Command::CancelGenerate { .. }) => self.bench_command(command),
        }
    }
}

// pickers: the authored catalogues (every provider but Cursor, a few dozen
// models with varied facts), the provider whose catalogue fails, the slow
// one, the one not connected, the one turned off, favourites kept here, a
// star that cannot be saved, and the exported folder's catalogues.

/// The sentence of a search the stand-in fails ([`StandIn::fail_searches`];
/// the core's words for a search it could not run).
pub const SEARCH_FAILURE: &str = "Search failed: the index could not be read (a fixture error)";

/// The provider whose catalogue fails in the fixture.
pub const FAILING_PROVIDER: &str = "groq";
/// Its failure, as the core words a provider's (`server/llm.js`).
pub const CATALOGUE_FAILURE: &str =
    "LLM request failed (HTTP 401): Invalid API Key (a fixture error)";
/// The provider whose catalogue is slow in the fixture.
pub const SLOW_PROVIDER: &str = "google";
/// How long the slow catalogue takes.
pub const SLOW: Duration = Duration::from_millis(1200);
/// The model whose star the stand-in cannot save.
pub const STAR_FAILS: (&str, &str) = ("openrouter", "x-ai/grok-4-fast");
/// The text of that failure.
pub const STAR_FAILURE: &str = "The star was not saved, and went back: the stand-in refuses to star x-ai/grok-4-fast (a fixture error)";

/// The fixture's providers, in the core's order: OpenAI not configured,
/// QwenCloud turned off in Settings, Codex CLI not signed in; a model
/// hidden on OpenRouter and an order saved on ChatGPT.
fn pickers_providers() -> Vec<Provider> {
    vec![
        provider(
            "openrouter",
            true,
            true,
            vec!["meta-llama/llama-3.1-8b-instruct".into()],
            Vec::new(),
        ),
        provider("openai", false, true, Vec::new(), Vec::new()),
        provider(
            "codex",
            true,
            true,
            Vec::new(),
            vec!["gpt-5.5-codex".into()],
        ),
        provider("anthropic", true, true, Vec::new(), Vec::new()),
        provider("groq", true, true, Vec::new(), Vec::new()),
        provider("google", true, true, Vec::new(), Vec::new()),
        provider("qwencloud", true, false, Vec::new(), Vec::new()),
        provider("claude-code", true, true, Vec::new(), Vec::new()),
        provider("codex-cli", false, true, Vec::new(), Vec::new()),
    ]
}

/// A model of the fixture's catalogues.
#[allow(clippy::too_many_arguments)]
fn catalogued(
    id: &str,
    name: &str,
    created: f64,
    tools: Option<bool>,
    image_input: Option<bool>,
    efforts: &[&str],
    default_effort: Option<&str>,
    context: Option<f64>,
) -> ModelInfo {
    ModelInfo {
        id: id.into(),
        name: name.into(),
        created,
        facts: ModelFacts {
            known: true,
            image_input,
            image_output: false,
            tools,
            reasoning: !efforts.is_empty(),
            efforts: efforts.iter().map(|e| e.to_string()).collect(),
            default_effort: default_effort.map(str::to_string),
            mandatory_reasoning: false,
            context_window: Some(context),
            fast: None,
        },
    }
}

/// The fixture's catalogues: Anthropic's and Claude Code's two models
/// (`fixture_models`), and the others.
fn pickers_models() -> HashMap<String, Vec<ModelInfo>> {
    const LMH: &[&str] = &["low", "medium", "high"];
    const OPENAI: &[&str] = &["minimal", "low", "medium", "high", "xhigh"];
    let t = Some(true);
    let mut models = fixture_models();
    let or =
        |id: &str, name: &str, day: f64, tools: Option<bool>, efforts: &[&str], context: f64| {
            catalogued(
                id,
                name,
                1_760_000_000. - day * 86_400.,
                tools,
                Some(true),
                efforts,
                efforts.get(1).copied(),
                Some(context),
            )
        };
    models.insert(
        "openrouter".into(),
        vec![
            or("openai/gpt-5.5", "OpenAI: GPT-5.5", 1., t, OPENAI, 400_000.),
            or(
                "anthropic/claude-opus-4.5",
                "Anthropic: Claude Opus 4.5",
                2.,
                t,
                LMH,
                200_000.,
            ),
            or(
                "x-ai/grok-4-fast",
                "xAI: Grok 4 Fast",
                3.,
                t,
                &[],
                2_000_000.,
            ),
            or(
                "google/gemini-3-pro",
                "Google: Gemini 3 Pro",
                4.,
                t,
                LMH,
                1_048_576.,
            ),
            or(
                "moonshotai/kimi-k2-thinking",
                "MoonshotAI: Kimi K2 Thinking",
                5.,
                t,
                &["on"],
                262_144.,
            ),
            or(
                "deepseek/deepseek-v4",
                "DeepSeek: DeepSeek V4",
                6.,
                t,
                &["on"],
                163_840.,
            ),
            or("qwen/qwen3-max", "Qwen: Qwen3 Max", 7., t, &[], 262_144.),
            or(
                "mistralai/mistral-large-3",
                "Mistral: Mistral Large 3",
                8.,
                t,
                &[],
                131_072.,
            ),
            or(
                "meta-llama/llama-4-maverick",
                "Meta: Llama 4 Maverick",
                9.,
                t,
                &[],
                1_048_576.,
            ),
            or(
                "meta-llama/llama-3.1-8b-instruct",
                "Meta: Llama 3.1 8B Instruct",
                10.,
                t,
                &[],
                131_072.,
            ),
            or(
                "openai/gpt-5.5-mini",
                "OpenAI: GPT-5.5 Mini",
                11.,
                t,
                OPENAI,
                400_000.,
            ),
            or(
                "openai/gpt-oss-120b",
                "OpenAI: gpt-oss-120b",
                12.,
                t,
                LMH,
                131_072.,
            ),
            or(
                "anthropic/claude-sonnet-4.5",
                "Anthropic: Claude Sonnet 4.5",
                13.,
                t,
                LMH,
                1_000_000.,
            ),
            or(
                "anthropic/claude-haiku-4.5",
                "Anthropic: Claude Haiku 4.5",
                14.,
                t,
                LMH,
                200_000.,
            ),
            or("x-ai/grok-4", "xAI: Grok 4", 15., t, &["high"], 256_000.),
            or(
                "google/gemini-2.5-flash",
                "Google: Gemini 2.5 Flash",
                16.,
                t,
                LMH,
                1_048_576.,
            ),
            or(
                "google/gemma-3-27b-it",
                "Google: Gemma 3 27B",
                17.,
                Some(false),
                &[],
                131_072.,
            ),
            or(
                "mistralai/mixtral-8x22b",
                "Mistral: Mixtral 8x22B",
                18.,
                None,
                &[],
                65_536.,
            ),
            or(
                "openai/gpt-4o-audio",
                "OpenAI: GPT-4o Audio",
                19.,
                Some(false),
                &[],
                128_000.,
            ),
            or(
                "qwen/qwen3-coder",
                "Qwen: Qwen3 Coder",
                20.,
                t,
                &[],
                262_144.,
            ),
        ],
    );
    models.insert(
        "codex".into(),
        vec![
            catalogued(
                "gpt-5.5",
                "GPT-5.5",
                1_760_000_000.,
                t,
                Some(true),
                OPENAI,
                Some("medium"),
                Some(400_000.),
            ),
            catalogued(
                "gpt-5.5-codex",
                "GPT-5.5 Codex",
                1_759_900_000.,
                t,
                Some(true),
                &["low", "medium", "high", "xhigh"],
                Some("medium"),
                Some(400_000.),
            ),
            catalogued(
                "gpt-5.5-mini",
                "GPT-5.5 Mini",
                1_759_800_000.,
                t,
                None,
                OPENAI,
                None,
                None,
            ),
        ],
    );
    models.insert(
        "google".into(),
        vec![
            catalogued(
                "gemini-3-pro",
                "Gemini 3 Pro",
                1_760_000_000.,
                t,
                Some(true),
                &["low", "high"],
                Some("high"),
                Some(1_048_576.),
            ),
            catalogued(
                "gemini-2.5-flash",
                "Gemini 2.5 Flash",
                1_750_000_000.,
                t,
                Some(true),
                &["none", "low", "medium", "high"],
                Some("medium"),
                Some(1_048_576.),
            ),
            catalogued(
                "imagen-4",
                "Imagen 4",
                1_749_000_000.,
                Some(false),
                Some(true),
                &[],
                None,
                None,
            ),
        ],
    );
    models.insert(
        "qwencloud".into(),
        vec![catalogued(
            "qwen3-max",
            "Qwen3 Max",
            1_760_000_000.,
            t,
            Some(false),
            &[],
            None,
            Some(262_144.),
        )],
    );
    models
}

/// The exported folder's catalogues: `models/<provider>.json`, each a
/// `GET /api/models` answer (`{ provider, models }`), when the folder has
/// them; none otherwise.
fn folder_models(folder: &Path) -> HashMap<String, Vec<ModelInfo>> {
    let dir = folder.join("models");
    if !dir.is_dir() {
        return HashMap::new();
    }
    std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()))
        .map(|entry| {
            let path = entry.expect("a catalogue file").path();
            let text = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
            let answer: Value = serde_json::from_str(&text)
                .unwrap_or_else(|e| panic!("{} is not JSON: {e}", path.display()));
            let provider = answer["provider"]
                .as_str()
                .unwrap_or_else(|| panic!("{} has no provider", path.display()))
                .to_string();
            let models = answer["models"]
                .as_array()
                .unwrap_or_else(|| panic!("{} has no models", path.display()))
                .iter()
                .map(|m| model_of(m, &path))
                .collect();
            (provider, models)
        })
        .collect()
}

// sidebar agent: the archive's failure, the folder's library and archived
// benches, and the list states the dev binary and the tests step through.

/// What a failed archive read says (`--archive-fails`).
pub const ARCHIVE_FAILURE: &str = "The archive could not be read (a fixture error)";

/// Why a chat named by `--unreadable` cannot be read (chat-fix-2).
pub const UNREADABLE: &str = "the stand-in cannot read this chat (a fixture error)";

/// The exported folder's prompt library (`prompts.json`, `GET /api/prompts`'s
/// shape), when the folder has one; else none.
fn folder_library(folder: &Path) -> Vec<crate::LibraryPrompt> {
    let path = folder.join("prompts.json");
    if !path.exists() {
        return Vec::new();
    }
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    let value: Value = serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("{} is not JSON: {e}", path.display()));
    value["prompts"]
        .as_array()
        .unwrap_or_else(|| panic!("{} has no prompts", path.display()))
        .iter()
        .map(|p| crate::LibraryPrompt {
            id: p["id"].as_str().expect("a prompt id").into(),
            title: p["title"].as_str().unwrap_or_default().into(),
            text: p["text"].as_str().unwrap_or_default().into(),
            tags: strings(&p["tags"]),
            favorite: p["favorite"] == true,
            attachments: p["attachments"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|a| {
                    let data = a["data"].as_str().expect("an attachment's data URL");
                    // As the store reads it: the type from the data URL,
                    // the size as given or 0.
                    let content_type = data
                        .strip_prefix("data:")
                        .and_then(|rest| rest.split_once(';'))
                        .map(|(kind, _)| kind.to_string())
                        .unwrap_or_else(|| panic!("{data:.40} is not a data URL"));
                    crate::PromptAttachment {
                        name: a["name"].as_str().expect("an attachment's name").into(),
                        content_type,
                        size: a["size"].as_u64().unwrap_or(0),
                        data: data.into(),
                    }
                })
                .collect(),
            used_at: p["usedAt"].as_i64(),
            added_by: p["addedBy"].as_object().map(|b| crate::PromptAddedBy {
                chat: b["chat"].as_str().unwrap_or_default().into(),
                model: b["model"].as_str().unwrap_or_default().into(),
            }),
        })
        .collect()
}

/// One model of a `GET /api/models` answer.
fn model_of(m: &Value, path: &Path) -> ModelInfo {
    let wrong = |field: &str| -> ! {
        panic!(
            "{}: a model's {field} is missing or of the wrong type: {m}",
            path.display()
        )
    };
    let opt_bool = |field: &str| match &m[field] {
        Value::Null => None,
        Value::Bool(b) => Some(*b),
        _ => wrong(field),
    };
    ModelInfo {
        id: m["id"].as_str().unwrap_or_else(|| wrong("id")).into(),
        name: m["name"].as_str().unwrap_or_else(|| wrong("name")).into(),
        created: m["created"].as_f64().unwrap_or_else(|| wrong("created")),
        facts: ModelFacts {
            known: m["known"].as_bool().unwrap_or_else(|| wrong("known")),
            image_input: opt_bool("imageInput"),
            image_output: m["imageOutput"]
                .as_bool()
                .unwrap_or_else(|| wrong("imageOutput")),
            tools: opt_bool("tools"),
            reasoning: m["reasoning"]
                .as_bool()
                .unwrap_or_else(|| wrong("reasoning")),
            efforts: strings(&m["efforts"]),
            default_effort: m["defaultEffort"].as_str().map(str::to_string),
            mandatory_reasoning: m["mandatoryReasoning"]
                .as_bool()
                .unwrap_or_else(|| wrong("mandatoryReasoning")),
            context_window: match m.get("contextWindow") {
                None => None,
                Some(Value::Null) => Some(None),
                Some(v) => Some(Some(v.as_f64().unwrap_or_else(|| wrong("contextWindow")))),
            },
            fast: None,
        },
    }
}

/// Runs an answer after a delay (the gallery's: GPUI's timer, so its
/// in-process tests stay on the test scheduler's thread).
pub type Later = Box<dyn Fn(Duration, Box<dyn FnOnce() + Send>) + Send + Sync>;

impl StandIn {
    /// Makes a slow answer wait with `later` instead of a thread.
    pub fn set_later(&self, later: Later) {
        *self.later.lock().expect("lock") = Some(later);
    }

    /// Adds `count` more models to `provider`'s catalogue (the pickers'
    /// timing with 400 models): `<vendor>/synthetic-<n>`, half from the
    /// four vendors the OpenRouter tab shows by default.
    pub fn add_synthetic_models(&self, provider: &str, count: usize) {
        const VENDORS: [&str; 8] = [
            "openai/",
            "meta-llama/",
            "anthropic/",
            "mistralai/",
            "x-ai/",
            "deepseek/",
            "google/",
            "qwen/",
        ];
        let mut data = self.data.lock().expect("lock");
        let list = data.models.entry(provider.to_string()).or_default();
        for n in 0..count {
            let vendor = VENDORS[n % VENDORS.len()];
            list.push(catalogued(
                &format!("{vendor}synthetic-{n}"),
                &format!("Synthetic: Model {n}"),
                1_700_000_000. - n as f64,
                Some(true),
                Some(n % 3 != 0),
                if n % 2 == 0 { &["low", "high"] } else { &[] },
                None,
                Some(128_000.),
            ));
        }
    }

    /// `GET /api/models`: the catalogue, its failure, or (the slow one)
    /// the catalogue after [`SLOW`] from another thread.
    fn answer_models(&self, provider: String) {
        let (models, slow) = {
            let data = self.data.lock().expect("lock");
            let connected = data
                .providers
                .iter()
                .find(|p| p.id == provider)
                .map(|p| (p.connected, p.label.clone()));
            let models =
                match (data.catalogue_errors.get(&provider), connected) {
                    (Some(error), _) => Err(error.clone()),
                    (None, Some((false, label))) => Err(format!("{label} is not configured")),
                    _ => data.models.get(&provider).cloned().ok_or_else(|| {
                        format!("The stand-in's data holds no models for {provider}")
                    }),
                };
            (models, data.slow.contains(&provider))
        };
        let window = self.window();
        if slow {
            let answer: Box<dyn FnOnce() + Send> =
                Box::new(move || window.publish(Update::Models { provider, models }));
            match self.later.lock().expect("lock").as_ref() {
                Some(later) => later(SLOW, answer),
                None => {
                    std::thread::spawn(move || {
                        std::thread::sleep(SLOW);
                        answer()
                    });
                }
            }
        } else {
            window.publish(Update::Models { provider, models });
        }
    }

    /// `PUT /api/model-favorites`: kept here and answered; a list with
    /// [`STAR_FAILS`] is refused: the notice, then the unchanged list.
    fn save_favorites(&self, favorites: Vec<(String, String)>) {
        let fails = favorites
            .iter()
            .any(|(p, m)| (p.as_str(), m.as_str()) == STAR_FAILS);
        let kept = {
            let mut data = self.data.lock().expect("lock");
            if !fails {
                data.favorites = favorites;
            }
            data.favorites.clone()
        };
        if fails {
            self.window()
                .publish(Update::Error(STAR_FAILURE.to_string()));
        }
        self.window().publish(Update::Favorites(kept));
    }
}

/// The exported folder's archived benches (`archive.json` `benches`;
/// `models` is the count the archive gives).
fn folder_archived_benches(archive: &Value) -> Vec<ArchivedSeat> {
    archive["benches"]
        .as_array()
        .expect("archive.json benches")
        .iter()
        .map(|b| ArchivedSeat {
            bench: ListedBench {
                id: b["id"].as_str().expect("a bench id").into(),
                title: b["title"].as_str().unwrap_or_default().into(),
                created_at: String::new(),
                head: None,
                resolution: Some(1),
                rounds: 0,
                prompts: Vec::new(),
                blind: false,
                revealed: false,
                order: Vec::new(),
            },
            archived_at: b["archivedAt"].as_i64().expect("archivedAt"),
            models: b["models"]
                .as_i64()
                .expect("archive.json: a bench's models is a count"),
        })
        .collect()
}

impl StandIn {
    /// A chat's turn starts (`running`) or ends; an ended turn leaves the
    /// chat finished and unseen when `unread`, and a bench model halted
    /// when `halted` says how. The list is published.
    pub fn set_turn(&self, chat: &str, running: bool, unread: bool, halted: Option<&str>) {
        self.with_chat(chat, |c| {
            c.row.running_since = running.then(now_ms);
            if !running {
                c.row.unread = unread;
            }
            if let Some(seat) = c.row.bench.as_mut() {
                seat.halted = match halted {
                    Some("failed") => Some(Halted::Failed),
                    Some("stopped") => Some(Halted::Stopped),
                    None => None,
                    Some(other) => panic!("a bench model halts failed or stopped, not {other}"),
                };
            }
        });
        self.publish_lists();
    }

    /// Lists a chat's turn as running from now, before the window attaches
    /// (`--running <chat>`; the stand-in finds it ended when it is joined).
    pub fn mark_running(&self, chat: &str) {
        self.with_chat(chat, |c| c.row.running_since = Some(now_ms()));
    }

    /// Makes every search fail with `error` (`--search-fails`), or succeed
    /// again with `None`.
    pub fn fail_searches(&self, error: Option<String>) {
        *self.search_failure.lock().expect("lock") = error;
    }

    // chat-fix-2: rows nothing else shows.
    /// Opening `chat` fails with [`UNREADABLE`] (`--unreadable CHAT`): the
    /// transcript's `This chat could not be read: …` with `Read again`.
    pub fn fail_reads_of(&self, chat: &str) {
        self.unreadable.lock().expect("lock").push(chat.to_string());
    }

    /// The archive's reads answer after `delay` (`--archive-ms N`), so the
    /// archived section shows `Loading…` meanwhile.
    pub fn delay_archive_reads(&self, delay: Duration) {
        *self.archive_delay.lock().expect("lock") = Some(delay);
    }

    // start-place agent
    /// The first lists reach the window `delay` after it attaches
    /// (`--lists-ms N`), as the real core's cold start sends them after its
    /// setup and the pane.
    pub fn delay_first_lists(&self, delay: Duration) {
        *self.lists_late.lock().expect("lock") = Some(Some(delay));
    }

    /// The first lists are held until [`StandIn::release_lists`] (the
    /// in-process tests of a late start).
    pub fn hold_first_lists(&self) {
        *self.lists_late.lock().expect("lock") = Some(None);
    }

    /// Sends the lists held by [`StandIn::hold_first_lists`].
    pub fn release_lists(&self) {
        self.publish_lists();
    }

    /// Makes the archive's reads fail (`--archive-fails`), or succeed.
    pub fn fail_archive_reads(&self, fail: bool) {
        self.data.lock().expect("lock").archive_fails = fail;
    }

    // chat-fix: setup still running, so a send is held.
    /// Setup with a step still running (`--setup-pending`): the app's steps
    /// (`mcbench_setup::steps::native_steps`) with the data done, the textures
    /// running and the optional shell waiting, published at attach (and at
    /// once if attached), so a send is held with `Sends when the app is
    /// ready. 1 of 2 done.` until [`StandIn::finish_setup`].
    pub fn setup_pending(&self) {
        let steps: Vec<mcbench_setup::Step> = mcbench_setup::steps::native_steps()
            .into_iter()
            .map(|mut step| {
                step.state = match step.id.as_str() {
                    "data" => mcbench_setup::StepState::Done { seconds: 0.4 },
                    "textures" => mcbench_setup::StepState::Running {
                        detail: None,
                        progress: None,
                    },
                    _ => mcbench_setup::StepState::Waiting,
                };
                step
            })
            .collect();
        self.publish_setup(steps);
    }

    /// The running step of [`StandIn::setup_pending`] done: the app is
    /// ready and a held send goes.
    pub fn finish_setup(&self) {
        let mut steps = self
            .setup
            .lock()
            .expect("lock")
            .clone()
            .expect("finish_setup after setup_pending");
        for step in &mut steps {
            if matches!(step.state, mcbench_setup::StepState::Running { .. }) {
                step.state = mcbench_setup::StepState::Done { seconds: 2.0 };
            }
        }
        self.publish_setup(steps);
    }

    fn publish_setup(&self, steps: Vec<mcbench_setup::Step>) {
        *self.setup.lock().expect("lock") = Some(steps.clone());
        let window = self.window.lock().expect("lock").clone();
        if let Some(window) = window {
            window.publish(Update::Setup(steps));
        }
    }

    /// The dev binary's `--simulate-ms N`: every N ms the next step of a
    /// loop of list states, published as the core would: a Build chat runs,
    /// then finishes unseen; a bench model runs, then fails; the first
    /// running fixture chat ends. The first step comes `every` after the
    /// window attaches. Runs until the process ends.
    pub fn simulate(self: &std::sync::Arc<Self>, every: Duration) {
        let me = self.clone();
        std::thread::spawn(move || {
            let (chat, member) = {
                let data = me.data.lock().expect("lock");
                let chat = data
                    .chats
                    .iter()
                    .find(|c| {
                        c.row.bench.is_none()
                            && c.archived_at.is_none()
                            && !c.rows.is_empty()
                            && c.row.running_since.is_none()
                            && !c.row.unread
                    })
                    .map(|c| c.row.id.clone());
                let member = data
                    .chats
                    .iter()
                    .find(|c| c.row.bench.is_some() && c.archived_at.is_none())
                    .map(|c| c.row.id.clone());
                (chat, member)
            };
            // quit: the steps start once the window has attached (the
            // binary calls this before `run` opens it; on a slow start the
            // first step came first and `window` panicked).
            while me.window.lock().expect("lock").is_none() {
                std::thread::sleep(every);
            }
            loop {
                for step in 0..4 {
                    std::thread::sleep(every);
                    match (step, &chat, &member) {
                        (0, Some(chat), _) => me.set_turn(chat, true, false, None),
                        (1, Some(chat), _) => me.set_turn(chat, false, true, None),
                        (2, _, Some(member)) => me.set_turn(member, true, false, None),
                        (3, _, Some(member)) => me.set_turn(member, false, true, Some("failed")),
                        _ => {}
                    }
                }
            }
        });
    }
}
