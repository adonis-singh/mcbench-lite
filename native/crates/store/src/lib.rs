//! # mcbench-store
//!
//! The SQLite store with today's schema, the storage budgets, settings,
//! attachments and images, and the prompt library's storage. It opens
//! today's data folders as they are: `mcbench.sqlite3` at schema version 14
//! (`server/store.js` `SCHEMA_VERSION`), `worlds/<chat>/chunks/<hash>.json`
//! chunk files, `images/`, `attachments/`, `settings.json`, `prompts.json`
//! and `.env`. It replaces `server/store.js`, `storage.js`, `settings.js`
//! (the file and its validation), `dotenv.js` and the storage half of
//! `prompts.js`.
//!
//! ## Owner
//!
//! The stage-three session.
//!
//! ## Seams
//!
//! | Seam | What crosses | Who calls whom | Thread or process |
//! |---|---|---|---|
//! | data folder | the files of [`DataFolder`]: the database, chunk files, images, attachments | this crate reads and writes; nothing else opens the database | the caller's thread, one at a time behind the store's lock |
//! | world | [`World`]s, their manifests and chunk files under `worlds/<chat>/chunks/` | this crate calls `World::from_manifest_cached`, `World::persist`, `World::persist_chunks` and `Chunk::set_persisted` (module `world`, `chats.rs`) | the caller's thread |
//! | agent | [`LiveConversation`], [`StoredMessage`] (the typed [`Message`] and its row's text), [`NewMessage`] for each writer, turn ends and branches | `mcbench-agent` calls this crate | the async runtime, through `spawn_blocking` |
//! | tools | attachment and image files by URL ([`Store::file_path`]), the chat's budget ([`Storage::reserve`]) | `mcbench-tools` calls this crate | the async runtime, through `spawn_blocking` |
//! | providers | [`Settings`] (keys, switches, favourites) and the `.env` variables ([`dot_env`]); a removed chat's harness ([`Options::chat_removed`]) | the core passes settings to `mcbench-providers`; the store calls the hook | the caller's thread |
//! | ui | [`ConversationSummary`] lists, [`ConversationHeader`], [`MessageTree`], [`StoredMessage`]s, search hits, the archive, benches, the leaderboard, settings; files as paths ([`Store::file_path`]) | the core reads them for `mcbench-ui`'s events; the interface never opens the store | the async runtime to the interface thread |
//!
//! ## Behaviour defined by
//!
//! Today's tests (docs/native-app.md, Appendix A), ported in `tests/` with
//! their names, cases, values and messages: `store.test.js` (33 of 37; the
//! four migration tests are not, this store opening version 14 only),
//! `storage.test.js` 9, `storage-limits.test.js` (the store's 5 of 8),
//! `store-missing-database.test.js` 2, `dotenv.test.js` 3,
//! `settings-data-dir.test.js` 2, `settings.test.js` (the file's 7 of 11),
//! `server-archive.test.js` (the archive's 4 of 5),
//! `attachment-serving.test.js` (the files' 2 of 3) and `prompts.test.js`
//! (the library's 5 of 7). And the differential proof against
//! `server/store.js` on clones of a real data folder (`tests/differential.rs`,
//! with `MCBENCH_DIFF_DATA`, `MCBENCH_NODE_DIFF` and `MCBENCH_DIFF_SCRATCH`):
//! the read document of every export, the results of a 600-op write script,
//! every table row by row and the files after it, both reads of both written
//! folders, and a fresh folder, all byte-identical.
//!
//! ## Decided
//!
//! The SQLite binding is `rusqlite` 0.43 with the bundled SQLite (FTS5 and the
//! JSON functions today's SQL uses), behind a blocking API: async callers
//! reach the store through `spawn_blocking` (`native/README.md`, Decisions of
//! stage three). The schema, the SQL and the pragmas of `server/store.js`
//! are today's, `recursive_triggers` included (better-sqlite3 is built with
//! it on).
//!
//! Folders at another schema version than 14 are refused with today's
//! message, which names the version found and the one opened: no migrations
//! in Rust (a folder older than 14 is opened once by today's app first).
//!
//! One writer connection behind one lock, re-entrant on its thread so a
//! change made of several (an edited prompt, a bench's round edit) is one
//! transaction, as `database.transaction` nests today. One app on a data
//! folder is the app's rule (`mcbench_setup::InstanceLock`, today's
//! `server/pid-lock.js`), not the store's: the store takes no lock of its own,
//! and the app opens one store per folder.
//!
//! Worlds are `mcbench_world::World`, read with their chunk files when a chat
//! loads (one chunk cache per branch load, as `loadBranch`), written by
//! `persist` with the store's chunk writer (a file is written when missing,
//! counted against the chat's budget, the disk's reserve checked at the
//! first), and after a failed save the chunks whose files it removed are
//! marked not stored (`discardChunks`). Their palette is shared across
//! `clone_world` and `reference` as today's.
//!
//! Time, ids, randomness and the disk's free space come from a [`Host`]
//! ([`SystemHost`] in the app), so the tests and the differential proof run
//! the same operations with the same ids and clock on both sides.
//!
//! The JSON the store writes or re-reads goes through `mcbench_json`'s
//! `stringify` and `parse` (module [`json`]).
//!
//! The typed message (with the stage-four session, `transcript-seam.md`):
//! a row's JSON text stays the source of truth and is never rewritten from a
//! typed value; [`Message`] is decoded from it on load, read-only, unknown
//! fields refused naming the chat and message; new rows come from
//! [`NewMessage`]'s constructors, one per writer of today's server in its key
//! order; a dismissal and a fork's copy change the text as `store.js` does.

pub mod json;

/// A pattern compiled once (`regex::Regex`), as a JavaScript literal is.
#[macro_export]
#[doc(hidden)]
macro_rules! regex {
    ($pattern:expr) => {{
        static PATTERN: std::sync::LazyLock<regex::Regex> =
            std::sync::LazyLock::new(|| regex::Regex::new($pattern).expect("a pattern"));
        &*PATTERN
    }};
}

mod archive;
mod benches;
mod chats;
mod dotenv;
mod files;
mod host;
mod payload;
mod prompts;
mod seed;
mod settings;
mod sql;
mod storage;
mod world;

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use parking_lot::ReentrantMutex;
use rusqlite::Connection;

pub use archive::{
    Archive, ArchivedBench, ArchivedConversation, MAX_SEARCH_TERMS, SearchHit, SearchScope,
};
pub use benches::{
    Bench, BenchMember, BenchProgress, LeaderboardEntry, ListedBench, NewBench, Vote,
};
pub use chats::{ConversationHeader, MessageTree, NewConversation, TreeMessage, unanswered_result};
pub use dotenv::dot_env;
pub use files::{AttachmentFile, ImageData};
pub use host::{Host, SystemHost, iso};
pub use mcbench_world::World;
pub use mcbench_world::WorldManifest;
pub use payload::Attachment;
pub use payload::{
    Assistant, Content, ErrorMessage, FunctionCall, ImageUrl, Message, NewMessage, Notice, Part,
    PlanItem, PromptLink, Reply, Selection, Telemetry, Tool, ToolCall, ToolTelemetry, Usage, User,
    to_json,
};
pub use prompts::{LibraryPrompt, PromptAddedBy, PromptAttachment, PromptLibrary};
pub use settings::{PROVIDERS, ProviderInfo, Settings, SettingsFile, normalize_model_favorites};
pub use storage::{Limits, Measurement, Storage, expendable, measure_now};

/// The schema version the store opens (`server/store.js` `SCHEMA_VERSION`).
pub const SCHEMA_VERSION: u32 = 14;
/// How long an archived chat or bench is kept (`ARCHIVE_TTL_MS`, 7 days).
pub const ARCHIVE_TTL_MS: i64 = 7 * 24 * 60 * 60 * 1000;
/// The largest attachment (`MAX_ATTACHMENT_BYTES`, 100 MiB).
pub const MAX_ATTACHMENT_BYTES: u64 = 100 * 1024 * 1024;
/// The most a message's attachments may hold together
/// (`MAX_MESSAGE_ATTACHMENT_BYTES`, 200 MiB).
pub const MAX_MESSAGE_ATTACHMENT_BYTES: u64 = 200 * 1024 * 1024;
/// A new chat's title (`DEFAULT_TITLE`).
pub const DEFAULT_TITLE: &str = "New build";
/// Why a turn the server stopped in the middle of ended (`INTERRUPTED`).
pub const INTERRUPTED: &str = "The server stopped during this turn";
/// Why a turn of this run whose end was not saved ended (`UNSAVED`).
pub const UNSAVED: &str = "How this turn ended was not saved";

/// A data folder: today's layout, opened unchanged.
#[derive(Clone, Debug)]
pub struct DataFolder {
    /// The folder (`MCBENCH_DATA_DIR`, or the platform's data folder:
    /// docs/native-app.md, Data folders).
    pub root: PathBuf,
}

impl DataFolder {
    /// `mcbench.sqlite3`.
    pub fn database(&self) -> PathBuf {
        self.root.join("mcbench.sqlite3")
    }
    /// `worlds/`: a folder per chat of chunk files named by hash.
    pub fn worlds(&self) -> PathBuf {
        self.root.join("worlds")
    }
    /// `images/`.
    pub fn images(&self) -> PathBuf {
        self.root.join("images")
    }
    /// `attachments/`.
    pub fn attachments(&self) -> PathBuf {
        self.root.join("attachments")
    }
    /// `settings.json` (unless `MCBENCH_SETTINGS_FILE` names another).
    pub fn settings(&self) -> PathBuf {
        self.root.join("settings.json")
    }
    /// `prompts.json`, the prompt library.
    pub fn prompts(&self) -> PathBuf {
        self.root.join("prompts.json")
    }
}

/// A chat's id (a UUID, as today).
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ConversationId(pub String);

/// A bench's id.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BenchId(pub String);

/// A message's id.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MessageId(pub String);

/// A payload's text. Nothing in this crate takes or gives it any more
/// ([`StoredMessage`] holds the text, [`NewMessage`] makes new rows); it stays
/// only because `mcbench-ui` names it (`Update`'s transcript) until that
/// crate takes [`StoredMessage`], and goes then.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MessagePayload(pub String);

/// A message of a chat's active branch: the typed [`Message`] decoded from
/// its row, whose text (`payload_json`) stays the source of truth.
#[derive(Clone, Debug, PartialEq)]
pub struct StoredMessage {
    /// `id`.
    pub id: MessageId,
    /// `parent_id`: the message before it on its branch.
    pub parent: Option<MessageId>,
    /// `position` among all the chat's messages.
    pub position: u32,
    /// The message.
    pub message: Message,
    /// `payload_json`, byte for byte.
    text: String,
}

impl StoredMessage {
    /// A message from a row's parts, decoded from its `payload_json` text
    /// as the store decodes a row it loads (unknown fields refused): for a
    /// stand-in outside the store, such as the interface's fixtures. The
    /// error says why the text is not a stored message.
    pub fn from_payload(
        id: &str,
        parent: Option<&str>,
        position: u32,
        text: String,
    ) -> Result<StoredMessage, String> {
        let message = payload::decode(&text, &format!("Message {id}"))?;
        Ok(StoredMessage {
            id: MessageId(id.to_string()),
            parent: parent.map(|parent| MessageId(parent.to_string())),
            position,
            message,
            text,
        })
    }

    /// `payload_json`: the row's text.
    pub fn payload_text(&self) -> &str {
        &self.text
    }

    /// The message as `server/store.js` holds it in memory, as
    /// `JSON.stringify` writes it: the payload's keys in stored order, then
    /// `id`, `parentId` and `position` (what `agent-context.js` measures and
    /// archives).
    pub fn in_memory_text(&self) -> String {
        payload::splice(
            &self.text,
            &[
                ("id", &json::str(&self.id.0)),
                (
                    "parentId",
                    &self.parent.as_ref().map(|p| p.0.clone()).into(),
                ),
                ("position", &self.position.into()),
            ],
        )
    }

    /// `publicMessages`' form, as `JSON.stringify` writes it: the payload's
    /// keys, then `id`.
    pub fn public_text(&self) -> String {
        payload::splice(&self.text, &[("id", &json::str(&self.id.0))])
    }
}

/// A chat as `server/store.js` holds it while it is loaded: the messages of
/// its active branch, its world and the worlds checkpointed before each
/// prompt, and the messages appended since it was last saved.
#[derive(Clone, Debug)]
pub struct Conversation {
    /// `id`.
    pub id: ConversationId,
    /// `title`.
    pub title: String,
    /// `provider`, a key of `mcbench-providers`' providers.
    pub provider: String,
    /// `model`.
    pub model: String,
    /// `effort`.
    pub effort: Option<String>,
    /// `resolution`.
    pub resolution: u32,
    /// `bench_id` and `bench_slot` when the chat is a bench member.
    pub bench: Option<(BenchId, u32)>,
    /// `created_at`, as `toISOString()` wrote it.
    pub created_at: String,
    /// The active branch, oldest first.
    pub messages: Vec<StoredMessage>,
    /// `current_message_id`: the branch's head.
    pub current_message_id: Option<MessageId>,
    /// The position the next message takes.
    pub next_message_position: u32,
    /// The world at the head.
    pub world: World,
    /// The world before each prompt of the branch.
    pub checkpoints: HashMap<MessageId, World>,
    /// Messages appended or changed since the last save.
    pub unsaved: HashSet<MessageId>,
    /// Which load of the chat this is (`live.get(id) === conversation`).
    token: u64,
}

/// A loaded chat, shared as `server/store.js` shares the object in `live`:
/// everyone who asks for the chat gets this one. A chat is locked before the
/// store is entered, never the other way round.
pub type LiveConversation = Arc<Mutex<Conversation>>;

/// A chat in the sidebar's list (`listConversations`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConversationSummary {
    /// `id`.
    pub id: ConversationId,
    /// `title`.
    pub title: String,
    /// `provider`.
    pub provider: String,
    /// `model`.
    pub model: String,
    /// `effort`.
    pub effort: Option<String>,
    /// `resolution`.
    pub resolution: i64,
    /// `createdAt`.
    pub created_at: String,
    /// `benchId`.
    pub bench_id: Option<String>,
    /// `benchSlot`.
    pub bench_slot: Option<i64>,
    /// A turn finished after the user last looked at it.
    pub unread: bool,
    /// How many messages it has on all branches.
    pub message_count: i64,
    /// The ids of the library prompts any of its messages was sent from.
    pub prompts: Vec<String>,
}

/// An error of the store: its message, byte for byte today's, and the HTTP
/// status `server/index.js` answers it with when it is input the server
/// refuses (`statusCode`; 507 is a storage refusal, `STORAGE_FULL`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    /// The message.
    pub message: String,
    /// `statusCode`, when today's error carries one.
    pub status: Option<u16>,
}

impl Error {
    /// An error with no status (the server answers 500).
    pub fn plain(message: impl Into<String>) -> Error {
        Error {
            message: message.into(),
            status: None,
        }
    }

    /// Input the server refuses (`refused(message, statusCode)`).
    pub fn refused(message: impl Into<String>, status: u16) -> Error {
        Error {
            message: message.into(),
            status: Some(status),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Error {}

impl From<rusqlite::Error> for Error {
    fn from(error: rusqlite::Error) -> Error {
        match &error {
            // A trigger's RAISE(ABORT, 'text') is its text, as better-sqlite3 says it.
            rusqlite::Error::SqliteFailure(_, Some(message)) => Error::plain(message.clone()),
            _ => Error::plain(error.to_string()),
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Error {
        Error::plain(error.to_string())
    }
}

impl From<&str> for Error {
    fn from(message: &str) -> Error {
        Error::plain(message)
    }
}

impl From<String> for Error {
    fn from(message: String) -> Error {
        Error::plain(message)
    }
}

/// How a store is opened.
pub struct Options {
    /// The clock, ids and the disk.
    pub host: Arc<dyn Host>,
    /// The operator's limits.
    pub limits: Limits,
    /// Called with each chat removed for good, after its rows and folders
    /// are gone (`harness.destroy(id)`: its harness processes, sessions and
    /// `harness/<id>`).
    pub chat_removed: Box<dyn Fn(&str) + Send + Sync>,
}

pub(crate) struct Inner {
    pub(crate) db: Connection,
    /// `live`: the chats loaded, with the token of their load.
    pub(crate) live: HashMap<String, (u64, LiveConversation)>,
    next_token: u64,
    /// How deep the transactions under way are nested.
    depth: u32,
}

/// The store, open on one data folder.
pub struct Store {
    folder: DataFolder,
    host: Arc<dyn Host>,
    storage: Arc<Storage>,
    chat_removed: Box<dyn Fn(&str) + Send + Sync>,
    inner: ReentrantMutex<RefCell<Inner>>,
}

impl Store {
    /// Opens the folder's database at [`SCHEMA_VERSION`] with today's
    /// pragmas, or creates it in a folder that has none, then removes chunk
    /// files no stored world names (`collectChunks`). Refuses, as today, a
    /// missing database beside worlds, and a database at another version.
    pub fn open(folder: &DataFolder, options: Options) -> Result<Store, Error> {
        let root = &folder.root;
        std::fs::create_dir_all(root)?;
        restrict(root)?;
        let database = folder.database();
        let worlds = folder.worlds();
        if !database.exists() && worlds.exists() && std::fs::read_dir(&worlds)?.next().is_some() {
            return Err(Error::plain(format!(
                "The database {} is missing, but {} holds worlds. Restore the database first: starting without it would create an empty one and delete those worlds.",
                database.display(),
                worlds.display()
            )));
        }
        let db = Connection::open(&database)?;
        db.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA foreign_keys = ON;
             PRAGMA busy_timeout = 5000;
             PRAGMA cache_size = -65536;
             PRAGMA mmap_size = 268435456;
             PRAGMA temp_store = MEMORY;
             PRAGMA recursive_triggers = ON;",
        )?;
        let version: u32 = db.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if version == 0 {
            // The WAL header is already written, so the setting takes a rebuild.
            db.execute_batch("PRAGMA auto_vacuum = INCREMENTAL; VACUUM;")?;
            db.execute_batch(&format!(
                "BEGIN; {} PRAGMA user_version = {SCHEMA_VERSION}; COMMIT;",
                sql::fresh()
            ))?;
        } else if version != SCHEMA_VERSION {
            return Err(Error::plain(format!(
                "Database schema version {version} is unsupported (expected {SCHEMA_VERSION})"
            )));
        }
        // Stepped to its end, as better-sqlite3's `pragma()` reads every
        // row: the pages it frees are committed (and checkpointed) here, not
        // by the first save.
        {
            let mut vacuum = db.prepare("PRAGMA incremental_vacuum")?;
            let mut rows = vacuum.query([])?;
            while rows.next()?.is_some() {}
        }
        let storage = Arc::new(Storage::new(
            root.clone(),
            options.limits,
            Arc::clone(&options.host),
        ));
        let store = Store {
            folder: folder.clone(),
            host: options.host,
            storage,
            chat_removed: options.chat_removed,
            inner: ReentrantMutex::new(RefCell::new(Inner {
                db,
                live: HashMap::new(),
                next_token: 0,
                depth: 0,
            })),
        };
        store.collect_chunks()?;
        Ok(store)
    }

    /// The data folder.
    pub fn folder(&self) -> &DataFolder {
        &self.folder
    }

    /// The chats' budgets.
    pub fn storage(&self) -> &Arc<Storage> {
        &self.storage
    }

    /// The host the store reads time and ids from.
    pub fn host(&self) -> &Arc<dyn Host> {
        &self.host
    }

    /// Runs `f` with the database, the store's lock held. `f` must not call
    /// back into the store.
    pub(crate) fn with<T>(&self, f: impl FnOnce(&mut Inner) -> T) -> T {
        let guard = self.inner.lock();
        let mut inner = guard.borrow_mut();
        f(&mut inner)
    }

    /// `database.transaction(f)()`: `f` runs in one transaction, nested in
    /// a savepoint when one is under way; an error rolls back what it did.
    /// `f` may call the store again on this thread.
    pub fn transaction<T>(&self, f: impl FnOnce() -> Result<T, Error>) -> Result<T, Error> {
        let guard = self.inner.lock();
        let depth = {
            let mut inner = guard.borrow_mut();
            let depth = inner.depth;
            inner.db.execute_batch(&if depth == 0 {
                "BEGIN".to_string()
            } else {
                format!("SAVEPOINT nested{depth}")
            })?;
            inner.depth += 1;
            depth
        };
        let result = f();
        let mut inner = guard.borrow_mut();
        inner.depth -= 1;
        let end = match (&result, depth) {
            (Ok(_), 0) => "COMMIT".to_string(),
            (Ok(_), _) => format!("RELEASE nested{depth}"),
            (Err(_), 0) => "ROLLBACK".to_string(),
            (Err(_), _) => format!("ROLLBACK TO nested{depth}; RELEASE nested{depth}"),
        };
        inner.db.execute_batch(&end)?;
        result
    }

    /// Holds the store's lock while `f` runs, so nothing else changes the
    /// store meanwhile; `f` may call the store on this thread.
    pub(crate) fn locked<T>(&self, f: impl FnOnce() -> T) -> T {
        let _guard = self.inner.lock();
        f()
    }

    pub(crate) fn now(&self) -> i64 {
        self.host.now_ms()
    }

    pub(crate) fn new_token(&self) -> u64 {
        self.with(|inner| {
            inner.next_token += 1;
            inner.next_token
        })
    }

    pub(crate) fn chat_removed(&self, id: &str) {
        (self.chat_removed)(id);
    }
}

/// Chats, attachments and worlds are the user's alone: other accounts on
/// the machine cannot list or read them (`fs.chmodSync(dataDir, 0o700)`).
#[cfg(unix)]
fn restrict(root: &Path) -> Result<(), Error> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(root, std::fs::Permissions::from_mode(0o700))?;
    Ok(())
}

/// Windows has no mode bits: `fs.chmodSync` there changes only the
/// read-only flag, which a folder does not take.
#[cfg(windows)]
fn restrict(_: &Path) -> Result<(), Error> {
    Ok(())
}

#[cfg(test)]
mod seam {
    #[test]
    fn the_store_and_a_loaded_chat_are_send_and_sync() {
        fn shared<T: Send + Sync>() {}
        shared::<super::Store>();
        shared::<super::LiveConversation>();
        shared::<super::Storage>();
    }
}
