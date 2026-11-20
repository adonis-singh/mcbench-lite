//! The window's commands through the core (`Core::command`, what
//! `mcbench_ui::Core::send` spawns), each with a recording `Window`: the
//! updates a command publishes, on success and on a failure with a real
//! cause (a chat or message that is not there, a read-only folder, a file
//! over the limit, a database that refuses), checked against the seam's
//! rules (`src/core/commands.rs`). Each test has its own data folder.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

use mcbench_agent::lifecycle::{BenchRequest, Outgoing as TurnOutgoing};
use mcbench_lite::core::backend::Backend;
use mcbench_lite::core::{Core, Parts};
use mcbench_providers::{
    AssistantMessage, CompletionRequest, Delta, ModelFacts, ModelInfo, Outcome, Provider,
    ProviderError, ProviderId, Telemetry,
};
use mcbench_store::{Message, MessageId};
use mcbench_ui::{
    Command, Outgoing, OutgoingFile, Resolution, SearchScope, Subject, Target, TurnEvent, Update,
    Window,
};
// Only the tests of preferences.json name it, and they need a read-only folder (Unix).
#[cfg(unix)]
use mcbench_ui::Preferences;

fn telemetry() -> Telemetry {
    Telemetry {
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
    }
}

fn said(content: &str) -> AssistantMessage {
    AssistantMessage {
        content: content.into(),
        reasoning: None,
        reasoning_details: None,
        finish_reason: None,
        telemetry: telemetry(),
        notices: None,
        plan: None,
        rate_limit: None,
        tool_calls: Vec::new(),
    }
}

/// The models: one behind its gate, one (`fail/...`) that fails, or one
/// that answers `Done.` at once; a title is `A title`.
#[derive(Default)]
struct Models {
    gates: Mutex<HashMap<String, watch::Receiver<bool>>>,
    /// The model of each turn's request, in order.
    asked: Mutex<Vec<String>>,
}

#[async_trait]
impl Provider for Models {
    fn id(&self) -> ProviderId {
        ProviderId::OpenRouter
    }
    async fn complete(
        &self,
        request: &CompletionRequest,
        on_delta: &mut (dyn FnMut(Delta) + Send),
        cancel: &CancellationToken,
    ) -> Result<AssistantMessage, ProviderError> {
        if request.tools.is_empty() {
            return Ok(said("A title"));
        }
        self.asked.lock().unwrap().push(request.model.clone());
        if request.model.starts_with("fail/") {
            return Err(ProviderError::new("The model failed"));
        }
        let gate = self.gates.lock().unwrap().get(&request.model).cloned();
        if let Some(mut gate) = gate {
            tokio::select! {
                _ = gate.wait_for(|open| *open) => {}
                _ = cancel.cancelled() => return Err(ProviderError::new("This operation was aborted")),
            }
        }
        on_delta(Delta::Content("Done.".into()));
        Ok(said("Done."))
    }
    async fn models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        Ok(Vec::new())
    }
    async fn model_info(&self, _: &str) -> Result<ModelFacts, ProviderError> {
        Ok(ModelFacts::unknown())
    }
    async fn close_session(&self, _: &str) {}
}

struct Fake(Arc<Models>);

#[async_trait]
impl Backend for Fake {
    fn provider(&self, _: &str) -> Result<Arc<dyn Provider>, String> {
        Ok(self.0.clone())
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
        false
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

struct App {
    core: Core,
    models: Arc<Models>,
    window: Arc<Recording>,
    data: PathBuf,
}

fn folder(name: &str) -> PathBuf {
    let data = std::env::temp_dir().join(format!(
        "mcbench-core-commands-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&data);
    data
}

impl App {
    fn new(name: &str) -> App {
        App::on_folder(folder(name), |_| {})
    }

    /// A core on `data`, `before` run on the folder first, and the window
    /// attached once the binary has read the preferences.
    fn on_folder(data: PathBuf, before: impl FnOnce(&Path)) -> App {
        std::fs::create_dir_all(&data).unwrap();
        before(&data);
        let models = Arc::new(Models::default());
        let core = Core::start(Parts {
            data: data.clone(),
            program: std::env::current_exe().unwrap(),
            host: Arc::new(mcbench_store::SystemHost),
            shipped: Box::new(|| Err("unused".into())),
            client: mcbench_setup::fetch::client(),
            backend: Some(Arc::new(Fake(Arc::clone(&models)))),
            installer: None,
            share: None,
            live: Default::default(),
        })
        .unwrap();
        let app = App {
            core,
            models,
            window: Arc::new(Recording::default()),
            data,
        };
        app.on(|core| async move { core.started().await });
        app
    }

    fn attach(&self) {
        mcbench_ui::Core::attach(&self.core, self.window.clone());
        until("the attach's favourites", || {
            self.updates()
                .iter()
                .any(|u| matches!(u, Update::Favorites(_)))
        });
    }

    fn on<T: Send + 'static, F: std::future::Future<Output = T> + Send + 'static>(
        &self,
        work: impl FnOnce(Core) -> F,
    ) -> T {
        let handle = self.core.handle().clone();
        handle
            .block_on(handle.spawn(work(self.core.clone())))
            .unwrap()
    }

    /// Runs `command` to its end; the updates published meanwhile, setup's
    /// left out.
    fn command(&self, command: Command) -> Vec<Update> {
        let from = self.updates().len();
        self.on(|core| async move { core.command(command).await });
        self.updates()[from..]
            .iter()
            .filter(|u| !matches!(u, Update::Setup(_)))
            .cloned()
            .collect()
    }

    fn gate(&self, model: &str) -> watch::Sender<bool> {
        let (open, gate) = watch::channel(false);
        self.models.gates.lock().unwrap().insert(model.into(), gate);
        open
    }

    fn free(&self, id: &str) {
        let id = id.to_string();
        self.on(|core| async move {
            core.turns().free(&id).await;
            core.turns().titles_settled().await;
        });
    }

    /// Sends `message` and waits for its turn to be joined (the turn's first
    /// event): the chat and the turn's start.
    fn sent(&self, message: Outgoing) -> (String, i64) {
        let from = self.updates().len();
        let refused = self.command(Command::Send(message));
        assert!(
            !refused
                .iter()
                .any(|u| matches!(u, Update::SendRefused { .. })),
            "{:?}",
            kinds(&refused)
        );
        let joined = || {
            self.updates()[from..].iter().find_map(|u| match u {
                Update::TurnJoined { chat, since } => Some((chat.clone(), *since)),
                _ => None,
            })
        };
        until("the send's turn is joined", || joined().is_some());
        joined().unwrap()
    }

    /// A chat of `model` with the prompt `text` answered.
    fn chat(&self, model: &str, text: &str) -> String {
        let (chat, _) = self.sent(new_chat(model, text));
        self.free(&chat);
        chat
    }

    fn updates(&self) -> Vec<Update> {
        self.window.0.lock().unwrap().clone()
    }

    fn prompts_of(&self, chat: &str) -> Vec<String> {
        self.core
            .store()
            .conversation(chat)
            .unwrap()
            .unwrap()
            .lock()
            .unwrap()
            .prompts()
            .iter()
            .map(|m| m.id.0.clone())
            .collect()
    }

    fn sqlite(&self) -> rusqlite::Connection {
        rusqlite::Connection::open(self.data.join("mcbench.sqlite3")).unwrap()
    }
}

impl Drop for App {
    fn drop(&mut self) {
        self.core.shutdown();
        let _ = std::fs::remove_dir_all(&self.data);
    }
}

fn outgoing(target: Target, text: &str) -> Outgoing {
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

fn new_chat(model: &str, text: &str) -> Outgoing {
    outgoing(
        Target::NewChat {
            provider: "openrouter".into(),
            model: model.into(),
        },
        text,
    )
}

/// A message to the open chat `chat`, the picker on `model`.
fn to_chat(chat: &str, model: &str) -> Target {
    Target::Chat {
        id: chat.into(),
        provider: "openrouter".into(),
        model: model.into(),
    }
}

/// The updates without the lists: the list's own task publishes one
/// whenever a hold ends or a chat changes, among a command's own updates.
fn without_lists(updates: &[Update]) -> Vec<Update> {
    updates
        .iter()
        .filter(|u| !matches!(u, Update::Conversations(_)))
        .cloned()
        .collect()
}

fn until(what: &str, done: impl Fn() -> bool) {
    let started = Instant::now();
    while !done() {
        assert!(started.elapsed() < Duration::from_secs(60), "{what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// A short name for each update, with the text of an error.
fn kinds(updates: &[Update]) -> Vec<String> {
    updates
        .iter()
        .map(|u| match u {
            Update::Conversations(_) => "conversations".into(),
            Update::Transcript { .. } => "transcript".into(),
            Update::TranscriptFailed { error, .. } => format!("transcript failed: {error}"),
            Update::Turn { .. } => "turn".into(),
            Update::TurnJoined { .. } => "joined".into(),
            Update::TurnNotRunning { .. } => "not running".into(),
            Update::SendRefused { error, .. } => format!("refused: {error}"),
            Update::Archive(Ok(_)) => "archive".into(),
            Update::Archive(Err(error)) => format!("archive failed: {error}"),
            Update::Found { results: Ok(_), .. } => "found".into(),
            Update::Found {
                results: Err(error),
                ..
            } => format!("found failed: {error}"),
            Update::Providers(_) => "providers".into(),
            Update::Models { .. } => "models".into(),
            Update::ModelInfo { .. } => "model info".into(),
            Update::Favorites(_) => "favorites".into(),
            Update::Error(error) => format!("error: {error}"),
            Update::Prompts(Ok(_)) => "prompts".into(),
            Update::Prompts(Err(error)) => format!("prompts failed: {error}"),
            other => format!("{other:?}"),
        })
        .collect()
}

/// The command's own updates: the coalesced list a change also triggers
/// (`Update::Conversations` published by the list's own task) can land
/// anywhere among them, so a run of lists counts once; the sequence must
/// then be exactly `expected`.
fn assert_sequence(updates: &[Update], expected: &[&str]) {
    let got = kinds(updates);
    // Each update is the next expected one, or a list of the list's own.
    let mut next = 0;
    for kind in &got {
        if expected.get(next).copied() == Some(kind.as_str()) {
            next += 1;
        } else {
            assert_eq!(kind, "conversations", "all: {got:?}, expected {expected:?}");
        }
    }
    assert_eq!(next, expected.len(), "all: {got:?}, expected {expected:?}");
}

fn last_list(updates: &[Update]) -> mcbench_agent::ConversationList {
    updates
        .iter()
        .rev()
        .find_map(|u| match u {
            Update::Conversations(list) => Some(list.clone()),
            _ => None,
        })
        .expect("a list")
}

fn listed(list: &mcbench_agent::ConversationList, chat: &str) -> bool {
    list.conversations
        .iter()
        .any(|row| row.summary.id.0 == chat)
}

#[cfg(unix)]
fn read_only(folder: &Path, on: bool) {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::set_permissions(
        folder,
        std::fs::Permissions::from_mode(if on { 0o555 } else { 0o755 }),
    )
    .unwrap();
}

/// After attach: the lists, the providers in `PROVIDERS`' order without
/// Cursor, and the favourites.
#[test]
fn attach_publishes_the_lists_the_providers_and_the_favourites() {
    let app = App::new("attach");
    app.attach();
    let updates = app.updates();
    // The attach's own three in order; the list's own task publishes the
    // list the core's start asked for whenever it gets to it, before, among
    // or after them.
    let order = kinds(&updates);
    let at = |kind: &str| {
        let found: Vec<usize> = (0..order.len()).filter(|&i| order[i] == kind).collect();
        assert_eq!(found.len(), 1, "{kind} once: {order:?}");
        found[0]
    };
    let (providers, favorites) = (at("providers"), at("favorites"));
    assert!(
        order[..providers].iter().any(|k| k == "conversations") && providers < favorites,
        "{order:?}"
    );
    let providers = updates
        .iter()
        .find_map(|u| match u {
            Update::Providers(Ok(list)) => Some(list.clone()),
            _ => None,
        })
        .unwrap();
    let ids: Vec<&str> = providers.iter().map(|p| p.id.as_str()).collect();
    let expected: Vec<&str> = mcbench_store::PROVIDERS
        .iter()
        .map(|p| p.id)
        .filter(|id| *id != "cursor")
        .collect();
    assert_eq!(ids, expected);
    let codex = providers.iter().find(|p| p.id == "codex").unwrap();
    assert_eq!(codex.auth, mcbench_ui::Auth::CliLogin);
    assert!(providers.iter().all(|p| p.enabled));
    assert!(
        updates
            .iter()
            .any(|u| matches!(u, Update::Favorites(list) if list.is_empty()))
    );
}

/// Send: a refusal before the turn starts is `Update::SendRefused` with
/// the route's sentence (too many files, a file over 100 MB, more than
/// 200 MB together, no text, a chat that is not there, a turn already
/// running), and nothing is stored; a send taken is `Update::TurnJoined`
/// (its `since` the turn's start), the transcript, then the turn.
#[test]
fn a_send_refused_before_its_turn_is_send_refused_with_the_routes_sentence() {
    let app = App::new("send");
    app.attach();
    let chat = app.chat("own/a", "Build a hut");
    let file = |name: &str, bytes: usize| OutgoingFile {
        name: name.into(),
        content_type: "application/octet-stream".into(),
        size: bytes as u64,
        data: vec![7u8; bytes].into(),
    };
    let with = |files: Vec<OutgoingFile>, text: &str| Outgoing {
        attachments: files,
        ..outgoing(to_chat(&chat, "own/a"), text)
    };
    let cases: Vec<(Outgoing, String)> = vec![
        (
            with(
                (0..mcbench_agent::MAX_ATTACHMENTS + 1)
                    .map(|i| file(&format!("f{i}.txt"), 3))
                    .collect(),
                "x",
            ),
            "A message supports at most 8 attachments".into(),
        ),
        (
            with(
                vec![file(
                    "big.bin",
                    mcbench_agent::MAX_ATTACHMENT_BYTES as usize + 1,
                )],
                "x",
            ),
            "Attachment \"big.bin\" exceeds 100 MB".into(),
        ),
        (
            with(
                (0..3)
                    .map(|i| {
                        file(
                            &format!("part{i}.bin"),
                            mcbench_agent::MAX_ATTACHMENTS_BYTES as usize / 3 + 1024,
                        )
                    })
                    .collect(),
                "x",
            ),
            "Attachments exceed 200 MB total".into(),
        ),
        (
            with(Vec::new(), "   "),
            "Message text or an attachment is required".into(),
        ),
        (
            outgoing(to_chat("nobody", "own/a"), "x"),
            "Conversation not found".into(),
        ),
    ];
    for (message, sentence) in cases {
        let target = message.target.clone();
        let updates = without_lists(&app.command(Command::Send(message)));
        assert!(
            matches!(updates.as_slice(), [Update::SendRefused { target: t, error }] if *t == target && *error == sentence),
            "{sentence}: {:?}",
            kinds(&updates)
        );
    }
    assert_eq!(app.prompts_of(&chat).len(), 1, "nothing was stored");
    // The folder is made by any message (`saveAttachments` makes it, as
    // today's does); no file was left in it.
    assert_eq!(
        std::fs::read_dir(app.data.join("attachments").join(&chat))
            .unwrap()
            .count(),
        0,
        "no file was left"
    );

    // A turn running: the second send is refused.
    let gate = app.gate("own/gated");
    let (gated, since) = app.sent(new_chat("own/gated", "Build a tower"));
    let header = app.on({
        let gated = gated.clone();
        move |core| async move { core.conversation(&gated).await.unwrap().header }
    });
    assert_eq!(header.running_since, Some(since));
    let again = without_lists(&app.command(Command::Send(outgoing(
        to_chat(&gated, "own/gated"),
        "More",
    ))));
    // The running turn publishes its own updates (its transcript, its
    // events) whenever its task gets to them, so some may fall among this
    // command's: what the refused send itself published is the refusal,
    // once.
    let refused: Vec<String> = kinds(&again)
        .into_iter()
        .filter(|kind| kind.starts_with("refused"))
        .collect();
    assert_eq!(
        refused,
        ["refused: A turn is already running for this conversation"],
        "{:?}",
        kinds(&again)
    );
    gate.send_replace(true);
    app.free(&gated);
    let joined_at = app
        .updates()
        .iter()
        .position(|u| matches!(u, Update::TurnJoined { chat, .. } if *chat == gated))
        .unwrap();
    let first_turn = app
        .updates()
        .iter()
        .position(|u| matches!(u, Update::Turn { chat, .. } if *chat == gated))
        .unwrap();
    assert!(joined_at < first_turn, "joined before the turn's events");
}

/// A message to an open chat carries the picker's provider and model and
/// the chat takes them (`handleChat`: `conversation.provider = provider`,
/// `conversation.model = model`), so a model picked in an open chat runs
/// the turn. The route's checks come first, in its order: the chat, then
/// an unknown provider (`Choose a configured provider`), then an empty
/// model (`Choose a model`); a refused message reaches no model and
/// changes nothing.
#[test]
fn a_send_to_an_open_chat_runs_on_the_messages_model_and_a_refused_pair_changes_nothing() {
    let app = App::new("chat-model");
    app.attach();
    let chat = app.chat("own/a", "Build a hut");
    let pair = || {
        let header = app.on({
            let chat = chat.clone();
            move |core| async move { core.conversation(&chat).await.unwrap().header }
        });
        (header.provider, header.model)
    };
    let asked = || app.models.asked.lock().unwrap().clone();
    let to = |id: &str, provider: &str, model: &str| Target::Chat {
        id: id.into(),
        provider: provider.into(),
        model: model.into(),
    };
    let before = asked();
    assert_eq!(before, ["own/a"]);

    for (target, sentence) in [
        (to(&chat, "nobody", "own/b"), "Choose a configured provider"),
        (to(&chat, "", ""), "Choose a configured provider"),
        (to(&chat, "openrouter", "  "), "Choose a model"),
        (to("nobody", "nobody", ""), "Conversation not found"),
    ] {
        let updates =
            without_lists(&app.command(Command::Send(outgoing(target.clone(), "Add a door"))));
        assert!(
            matches!(updates.as_slice(), [Update::SendRefused { target: t, error }] if *t == target && error == sentence),
            "{sentence}: {:?}",
            kinds(&updates)
        );
        assert_eq!(
            pair(),
            (String::from("openrouter"), String::from("own/a")),
            "{sentence}"
        );
        assert_eq!(app.prompts_of(&chat).len(), 1, "{sentence}");
    }
    assert_eq!(asked(), before, "no refused message reached a model");

    // The pair is trimmed as the route trims it.
    let (joined, _) = app.sent(outgoing(to(&chat, " openrouter ", " own/b "), "Add a door"));
    assert_eq!(joined, chat);
    app.free(&chat);
    assert_eq!(asked(), ["own/a", "own/b"]);
    assert_eq!(pair(), (String::from("openrouter"), String::from("own/b")));
    assert_eq!(app.prompts_of(&chat).len(), 2);
}

/// `Command::CreateChat` makes a new chat's row as the page does at New
/// chat (`POST /api/conversations`): it ends in `Update::ChatCreated` with
/// the id, or with the route's sentence and no row. The row has no message,
/// so the list leaves it out; it opens as an empty transcript, and its
/// first message, sent as to any open chat, lists it and runs on the
/// message's model.
#[test]
fn create_chat_makes_an_unlisted_row_that_its_first_message_lists() {
    let app = App::new("create-chat");
    app.attach();
    let rows = || -> i64 {
        app.sqlite()
            .query_row("SELECT count(*) FROM conversations", [], |row| row.get(0))
            .unwrap()
    };
    let list = || app.on(|core| async move { core.conversations().await.unwrap() });
    let create = |provider: &str, model: &str| {
        without_lists(&app.command(Command::CreateChat {
            provider: provider.into(),
            model: model.into(),
        }))
    };

    for (provider, model) in [("nobody", "own/a"), ("openrouter", "  "), ("", "")] {
        let refused = create(provider, model);
        assert!(
            matches!(refused.as_slice(), [Update::ChatCreated(Err(e))] if e == "Provider and model are required"),
            "{provider:?} {model:?}: {:?}",
            kinds(&refused)
        );
    }
    assert_eq!(rows(), 0, "a refused command makes no row");

    let created = create(" openrouter ", " own/a ");
    let [Update::ChatCreated(Ok(chat))] = created.as_slice() else {
        panic!("{:?}", kinds(&created));
    };
    let chat = chat.clone();
    assert_eq!(rows(), 1);
    assert!(
        !listed(&list(), &chat),
        "a chat with no message is not listed"
    );
    // The core remembers nothing for the window: a second command is a
    // second row.
    let again = create("openrouter", "own/a");
    assert!(
        matches!(again.as_slice(), [Update::ChatCreated(Ok(other))] if *other != chat),
        "{:?}",
        kinds(&again)
    );
    assert_eq!(rows(), 2);

    // It opens as a chat with no message, of the trimmed pair.
    let opened = without_lists(&app.command(Command::Open { chat: chat.clone() }));
    assert_eq!(kinds(&opened), ["transcript"]);
    let header = app.on({
        let chat = chat.clone();
        move |core| async move { core.conversation(&chat).await.unwrap().header }
    });
    assert_eq!(
        (header.provider, header.model),
        (String::from("openrouter"), String::from("own/a"))
    );
    assert!(app.prompts_of(&chat).is_empty());

    // Its first message goes as to any open chat, here with another model
    // picked since: the row is listed and the turn runs on that model.
    let (joined, _) = app.sent(outgoing(to_chat(&chat, "own/b"), "Build a hut"));
    assert_eq!(joined, chat);
    app.free(&chat);
    assert!(listed(&list(), &chat));
    assert_eq!(*app.models.asked.lock().unwrap(), ["own/b"]);
    assert_eq!(app.prompts_of(&chat).len(), 1);
}

/// A send whose files do not fit the chat's storage budget is refused with
/// the store's 507 sentence: run in a child process of this test binary
/// with `MCBENCH_CHAT_MAX_BYTES` set, which the core reads when it starts.
#[test]
fn a_send_over_the_chats_storage_budget_is_send_refused() {
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "a_send_over_a_small_budget",
            "--ignored",
            "--nocapture",
        ])
        .env("MCBENCH_CHAT_MAX_BYTES", "4096")
        .output()
        .unwrap();
    assert!(
        output.status.success() && String::from_utf8_lossy(&output.stdout).contains("1 passed"),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
#[ignore = "run by a_send_over_the_chats_storage_budget_is_send_refused, with MCBENCH_CHAT_MAX_BYTES set"]
fn a_send_over_a_small_budget() {
    assert_eq!(
        std::env::var("MCBENCH_CHAT_MAX_BYTES").as_deref(),
        Ok("4096")
    );
    let app = App::new("budget");
    app.attach();
    let chat = app.chat("own/a", "Build a hut");
    let updates = without_lists(&app.command(Command::Send(Outgoing {
        attachments: vec![OutgoingFile {
            name: "big.txt".into(),
            content_type: "text/plain".into(),
            size: 8192,
            data: vec![b'x'; 8192].into(),
        }],
        ..outgoing(to_chat(&chat, "own/a"), "Read this")
    })));
    let kinds = kinds(&updates);
    assert_eq!(kinds.len(), 1, "{kinds:?}");
    assert!(
        kinds[0].starts_with("refused: This chat's storage is full: ")
            && kinds[0].ends_with(
                "of its 4 KiB budget (MCBENCH_CHAT_MAX_BYTES) and this write needs 8 KiB."
            ),
        "{kinds:?}"
    );
    assert_eq!(app.prompts_of(&chat).len(), 1, "nothing was stored");
}

/// A message's files reach the store as today's page sends them: the
/// attachment is stored under its name with the bytes given.
#[test]
fn a_sends_files_are_stored_with_their_bytes() {
    let app = App::new("send-files");
    app.attach();
    let (chat, _) = app.sent(Outgoing {
        attachments: vec![OutgoingFile {
            name: "notes.txt".into(),
            content_type: "text/plain".into(),
            size: 5,
            data: b"hello".to_vec().into(),
        }],
        ..new_chat("own/a", "Read this")
    });
    app.free(&chat);
    let stored = app.core.store().conversation(&chat).unwrap().unwrap();
    let stored = stored.lock().unwrap();
    let Message::User(user) = &stored.messages[0].message else {
        panic!("a prompt")
    };
    let mcbench_store::Content::Parts(parts) = &user.content else {
        panic!("parts")
    };
    let attachment = parts
        .iter()
        .find_map(|p| match p {
            mcbench_store::Part::Attachment { attachment } => Some(attachment.clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        (attachment.name.as_str(), attachment.mime.as_str()),
        ("notes.txt", "text/plain")
    );
    assert_eq!(
        app.core.store().read_attachment(&attachment.url).unwrap(),
        b"hello"
    );
    let file = mcbench_ui::Core::attachment_file(&app.core, &attachment.url).unwrap();
    assert_eq!(std::fs::read(&file.path).unwrap(), b"hello");
    assert_eq!(
        mcbench_ui::Core::attachment_file(&app.core, &format!("/api/attachments/{chat}/none.txt"))
            .unwrap_err(),
        "Attachment not found"
    );
    assert_eq!(
        mcbench_ui::Core::image_file(&app.core, &format!("/api/images/{chat}/none.png"))
            .unwrap_err(),
        "Image not found"
    );
    // A block's icon before setup has the data (this core has no backend).
    assert_eq!(
        mcbench_ui::Core::block_icon(&app.core, "stone").unwrap_err(),
        "Minecraft's data is not ready yet"
    );
}

/// The data folder's lock is held while the core runs and is gone once
/// `shutdown` has returned, with the core's value still alive: the app's
/// process exits after `shutdown` without dropping it.
#[test]
fn shutdown_lets_go_of_the_data_folders_lock() {
    let app = App::new("lock");
    let lock = app.data.join("server.pid");
    assert_eq!(
        std::fs::read_to_string(&lock).unwrap().trim(),
        std::process::id().to_string()
    );
    app.core.shutdown();
    assert!(!lock.exists(), "server.pid stayed after shutdown");
    // A second shutdown (the harness's own, at drop) does nothing.
    app.core.shutdown();
    assert!(!lock.exists());
}

/// JoinTurn: `TurnJoined` with the turn's start, then the transcript and
/// the snapshot and every event; `TurnNotRunning` when none runs.
#[test]
fn join_turn_joins_a_running_turn_or_says_none_runs() {
    let app = App::new("join");
    app.attach();
    let gate = app.gate("own/gated");
    let (chat, since) = app.sent(new_chat("own/gated", "Build"));
    until("the snapshot is published", || {
        app.updates().iter().any(
            |u| matches!(u, Update::Turn { chat: c, event: TurnEvent::Snapshot } if *c == chat),
        )
    });
    let joined = without_lists(&app.command(Command::JoinTurn { chat: chat.clone() }));
    assert!(
        matches!(&joined[..], [Update::TurnJoined { chat: c, since: s }, Update::Transcript { .. }, Update::Turn { event: TurnEvent::Snapshot, .. }, ..] if *c == chat && *s == since),
        "{:?}",
        kinds(&joined)
    );
    let from = app.updates().len();
    gate.send_replace(true);
    app.free(&chat);
    let dones = app.updates()[from..]
        .iter()
        .filter(|u| {
            matches!(
                u,
                Update::Turn {
                    event: TurnEvent::Done,
                    ..
                }
            )
        })
        .count();
    assert_eq!(dones, 1, "the turn's end is published once");
    assert_eq!(
        kinds(&without_lists(
            &app.command(Command::JoinTurn { chat: chat.clone() })
        )),
        ["not running"]
    );
}

/// Archive, restore and delete of a chat and of a bench end in the list
/// (and the archive, once it was read), after the route's sentence when
/// one failed.
#[test]
fn archive_restore_and_delete_end_in_the_lists() {
    let app = App::new("archive");
    app.attach();
    let chat = app.chat("own/a", "Build a hut");
    let other = app.chat("own/a", "Build a shed");

    let archived = app.command(Command::Archive(Subject::Chat(chat.clone())));
    assert_sequence(&archived, &["conversations"]);
    let list = last_list(&archived);
    assert_eq!(list.archived, (1, 0));
    assert!(!listed(&list, &chat));

    let read = without_lists(&app.command(Command::ReadArchive));
    assert!(
        matches!(&read[..], [Update::Archive(Ok(archive))] if archive.conversations.len() == 1 && archive.conversations[0].id == chat),
        "{:?}",
        kinds(&read)
    );

    // The archive was read: each change publishes it again.
    let restored = app.command(Command::Restore(Subject::Chat(chat.clone())));
    assert_sequence(&restored, &["conversations", "archive"]);
    assert!(listed(&last_list(&restored), &chat));
    assert!(
        matches!(without_lists(&restored).last(), Some(Update::Archive(Ok(a))) if a.conversations.is_empty())
    );

    let again = app.command(Command::Restore(Subject::Chat(chat.clone())));
    assert_sequence(
        &again,
        &[
            "error: This chat is not archived",
            "conversations",
            "archive",
        ],
    );
    let refused = app.command(Command::Delete(Subject::Chat(other.clone())));
    assert_sequence(
        &refused,
        &["error: Archive it first", "conversations", "archive"],
    );
    assert!(listed(&last_list(&refused), &other));
    let missing = app.command(Command::Archive(Subject::Chat("nobody".into())));
    assert_sequence(
        &missing,
        &["error: Conversation not found", "conversations", "archive"],
    );

    app.command(Command::Archive(Subject::Chat(other.clone())));
    let deleted = app.command(Command::Delete(Subject::Chat(other.clone())));
    assert_sequence(&deleted, &["conversations", "archive"]);
    assert_eq!(last_list(&deleted).archived, (0, 0));
    assert!(app.core.store().conversation(&other).unwrap().is_none());

    // A bench.
    let bench = app.on(|core| async move {
        core.create_bench(BenchRequest {
            members: ["own/a", "own/b"]
                .iter()
                .map(|m| ("openrouter".to_string(), m.to_string(), None))
                .collect(),
            blind: false,
            resolution: 1,
            message: TurnOutgoing {
                text: "A well".into(),
                ..TurnOutgoing::default()
            },
        })
        .await
        .unwrap()
    });
    for member in app.core.store().bench_members(&bench).unwrap() {
        let id = member.lock().unwrap().id.0.clone();
        app.free(&id);
    }
    let archived = app.command(Command::Archive(Subject::Bench(bench.clone())));
    assert_sequence(&archived, &["conversations", "archive"]);
    assert_eq!(last_list(&archived).archived, (0, 1));
    let restored = app.command(Command::Restore(Subject::Bench(bench.clone())));
    assert_sequence(&restored, &["conversations", "archive"]);
    assert_eq!(last_list(&restored).archived, (0, 0));
    let refused = app.command(Command::Delete(Subject::Bench(bench.clone())));
    assert_sequence(
        &refused,
        &["error: Archive it first", "conversations", "archive"],
    );
    app.command(Command::Archive(Subject::Bench(bench.clone())));
    let deleted = app.command(Command::Delete(Subject::Bench(bench.clone())));
    assert_sequence(&deleted, &["conversations", "archive"]);
    assert!(app.core.store().bench(&bench).unwrap().is_none());
}

/// MarkSeen ends in the list with the chat seen; a database that refuses
/// the write is its error, then the list as it is.
#[test]
fn mark_seen_ends_in_the_list() {
    let app = App::new("seen");
    app.attach();
    let chat = app.chat("own/a", "Build a hut");
    let unread = |list: &mcbench_agent::ConversationList| {
        list.conversations
            .iter()
            .find(|row| row.summary.id.0 == chat)
            .unwrap()
            .summary
            .unread
    };
    let before = app.on(|core| async move { core.conversations().await.unwrap() });
    assert!(unread(&before), "a turn's answer is unread");
    // Open publishes the transcript and nothing else: it marks nothing seen.
    // (A list the list's own task publishes for an earlier change may land
    // among its updates; the row's unread mark shows nothing was marked.)
    let opened = without_lists(&app.command(Command::Open { chat: chat.clone() }));
    assert_eq!(kinds(&opened), ["transcript"]);
    let after_open = app.on(|core| async move { core.conversations().await.unwrap() });
    assert!(unread(&after_open), "opening does not mark the chat seen");
    let missing = without_lists(&app.command(Command::Open {
        chat: "nobody".into(),
    }));
    assert_eq!(
        kinds(&missing),
        ["transcript failed: Conversation not found"]
    );
    let seen = app.command(Command::MarkSeen { chat: chat.clone() });
    assert_sequence(&seen, &["conversations"]);
    assert!(!unread(&last_list(&seen)));

    let other = app.chat("own/a", "Build a shed");
    app.sqlite()
        .execute_batch(
            "CREATE TRIGGER refuse_seen BEFORE UPDATE OF seen_at ON conversations \
             BEGIN SELECT RAISE(ABORT, 'the disk refused the write'); END;",
        )
        .unwrap();
    let refused = app.command(Command::MarkSeen {
        chat: other.clone(),
    });
    // The list's own task may publish a list for the chat just answered
    // before the command's error: the command's own updates are the error,
    // then the list.
    let own = kinds(&without_lists(&refused));
    assert!(
        matches!(&own[..], [only] if only.starts_with("error: ") && only.contains("the disk refused the write")),
        "{own:?}"
    );
    assert_eq!(
        kinds(&refused).last().map(String::as_str),
        Some("conversations")
    );
    let row = last_list(&refused);
    assert!(
        row.conversations
            .iter()
            .find(|r| r.summary.id.0 == other)
            .unwrap()
            .summary
            .unread
    );
}

/// DismissError ends in the chat's transcript: the error dismissed; on a
/// refusal the transcript unchanged after the route's sentence; for a
/// chat that is not there, `TranscriptFailed`.
#[test]
fn dismiss_error_ends_in_the_chats_transcript() {
    let app = App::new("dismiss");
    app.attach();
    let chat = app.chat("fail/a", "Build a hut");
    let (error, prompt) = {
        let live = app.core.store().conversation(&chat).unwrap().unwrap();
        let live = live.lock().unwrap();
        let error = live
            .messages
            .iter()
            .find(|m| matches!(m.message, Message::Error(_)))
            .expect("a stored error")
            .id
            .clone();
        (error, live.messages[0].id.clone())
    };
    let dismissed = |updates: &[Update]| match updates.last() {
        Some(Update::Transcript { messages, .. }) => messages
            .iter()
            .find(|m| m.id == error)
            .map(|m| matches!(&m.message, Message::Error(e) if e.dismissed))
            .unwrap(),
        other => panic!("{other:?}"),
    };
    let refused = without_lists(&app.command(Command::DismissError {
        chat: chat.clone(),
        message: prompt,
    }));
    assert_sequence(
        &refused,
        &[
            "error: messageId must point at an error message",
            "transcript",
        ],
    );
    assert!(!dismissed(&refused));
    let done = without_lists(&app.command(Command::DismissError {
        chat: chat.clone(),
        message: error.clone(),
    }));
    assert_sequence(&done, &["transcript"]);
    assert!(dismissed(&done));
    let missing = without_lists(&app.command(Command::DismissError {
        chat: "nobody".into(),
        message: error,
    }));
    assert_sequence(
        &missing,
        &[
            "error: Conversation not found",
            "transcript failed: Conversation not found",
        ],
    );
}

/// Branch ends in the chat's transcript on the branch through the message;
/// on a refusal the transcript unchanged after the route's sentence; for a
/// chat that is not there, `TranscriptFailed`. An edit (`edit_of`) is the
/// one way a chat branches.
#[test]
fn branch_ends_in_the_chats_transcript() {
    let app = App::new("branch");
    app.attach();
    let chat = app.chat("own/a", "Build a hut");
    let first = app.prompts_of(&chat)[0].clone();
    let (edited, _) = app.sent(Outgoing {
        edit_of: Some(MessageId(first.clone())),
        ..outgoing(to_chat(&chat, "own/a"), "Build a barn")
    });
    assert_eq!(edited, chat);
    app.free(&chat);
    let second = app.prompts_of(&chat)[0].clone();
    assert_ne!(first, second, "the edit is a new branch");
    let shown = |updates: &[Update]| match updates.last() {
        Some(Update::Transcript { messages, .. }) => messages[0].id.0.clone(),
        other => panic!("{other:?}"),
    };
    let switched = without_lists(&app.command(Command::Branch {
        chat: chat.clone(),
        message: MessageId(first.clone()),
    }));
    assert_sequence(&switched, &["transcript"]);
    assert_eq!(shown(&switched), first);
    let refused = without_lists(&app.command(Command::Branch {
        chat: chat.clone(),
        message: MessageId("nothing".into()),
    }));
    let kinds_refused = kinds(&refused);
    assert_eq!(kinds_refused.len(), 2, "{kinds_refused:?}");
    assert!(kinds_refused[0].starts_with("error: "), "{kinds_refused:?}");
    assert_eq!(shown(&refused), first, "unchanged");
    let missing = without_lists(&app.command(Command::Branch {
        chat: "nobody".into(),
        message: MessageId(first),
    }));
    assert_sequence(
        &missing,
        &[
            "error: Conversation not found",
            "transcript failed: Conversation not found",
        ],
    );
}

/// SetFavorites ends in the favourites as kept (normalised as today); a
/// save into a read-only folder is the page's notice, then the list
/// unchanged.
#[cfg(unix)]
#[test]
fn set_favorites_ends_in_the_favourites_as_kept() {
    let app = App::new("favorites");
    app.attach();
    let pair = |p: &str, m: &str| (p.to_string(), m.to_string());
    let saved = without_lists(&app.command(Command::SetFavorites(vec![
        pair("openrouter", "a/b"),
        pair("openrouter", "a/b"),
        pair("nobody", "x"),
        pair("anthropic", " c "),
    ])));
    let kept = vec![pair("openrouter", "a/b"), pair("anthropic", "c")];
    assert!(
        matches!(&saved[..], [Update::Favorites(list)] if *list == kept),
        "{saved:?}"
    );
    read_only(&app.data, true);
    let refused =
        without_lists(&app.command(Command::SetFavorites(vec![pair("openrouter", "z/z")])));
    read_only(&app.data, false);
    let kinds = kinds(&refused);
    assert_eq!(kinds.len(), 2, "{kinds:?}");
    assert!(
        kinds[0].starts_with("error: The star was not saved, and went back: "),
        "{kinds:?}"
    );
    assert!(matches!(&refused[1], Update::Favorites(list) if *list == kept));
}

/// The preferences: no file is the defaults; a saved file reads back; a
/// file that does not parse is an error naming it (published once the
/// window is attached), the defaults, and the file left as it was; a save
/// into a read-only folder is an error naming the file, and a save has no
/// update on success.
#[cfg(unix)]
#[test]
fn the_preferences_are_read_once_and_saved_whole() {
    let data = folder("preferences");
    let app = App::on_folder(data.clone(), |_| {});
    assert_eq!(
        mcbench_ui::Core::preferences(&app.core),
        Preferences::default()
    );
    app.attach();
    let preferences = Preferences {
        light: true,
        sidebar_width: 280.0,
        place: Some("/chat/x".into()),
        last_model: Some(("openrouter".into(), "own/a".into())),
        ..Preferences::default()
    };
    let saved = without_lists(&app.command(Command::SavePreferences(preferences.clone())));
    assert!(saved.is_empty(), "{:?}", kinds(&saved));
    let file = data.join("preferences.json");
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        preferences.to_text()
    );
    assert_eq!(mcbench_ui::Core::preferences(&app.core), preferences);
    read_only(&data, true);
    let refused = without_lists(&app.command(Command::SavePreferences(Preferences::default())));
    read_only(&data, false);
    let message = format!(
        "error: The window's preferences could not be saved to {}: ",
        file.display()
    );
    assert!(
        matches!(&kinds(&refused)[..], [only] if only.starts_with(&message)),
        "{:?}",
        kinds(&refused)
    );
    assert_eq!(mcbench_ui::Core::preferences(&app.core), preferences);
    drop(app);

    // A file that is not JSON: refused whole. (A field that is lacking or
    // unreadable is the interface's to default, `Preferences::from_text`.)
    let data = folder("preferences-broken");
    let app = App::on_folder(data.clone(), |data| {
        std::fs::write(data.join("preferences.json"), "{ not json").unwrap();
    });
    assert_eq!(
        mcbench_ui::Core::preferences(&app.core),
        Preferences::default()
    );
    app.attach();
    let file = data.join("preferences.json");
    let first = app
        .updates()
        .into_iter()
        .find(|u| matches!(u, Update::Error(_)))
        .expect("the error");
    assert!(
        matches!(&first, Update::Error(e) if e.starts_with(&format!("The window's preferences in {} could not be read, so the defaults are used until they are saved again: ", file.display()))),
        "{first:?}"
    );
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "{ not json");
}

/// The reads: the archive, a search (its `request` kept, the snippet's
/// marks), the prompt library; each with its failure's sentence when the
/// database or the file refuses; the models' and a model's facts' refusals.
#[test]
fn the_reads_answer_with_their_result_or_the_routes_sentence() {
    let app = App::new("reads");
    app.attach();
    app.chat("own/a", "Build a red stone house");
    let read = |command: Command| without_lists(&app.command(command));
    let found = read(Command::Search {
        scope: SearchScope::Build,
        query: "house".into(),
        request: 41,
    });
    assert!(
        matches!(&found[..], [Update::Found { request: 41, results: Ok(results) }] if results.len() == 1 && results[0].snippet.as_deref().is_some_and(|s| s.contains("\u{1}house\u{2}"))),
        "{found:?}"
    );
    let prompts = read(Command::ReadPrompts);
    assert!(
        matches!(&prompts[..], [Update::Prompts(Ok(library))] if library.len() == 5 && library[0].id == "medieval-house"),
        "{prompts:?}"
    );
    // The library as the store types it: an attachment's type and size.
    std::fs::write(
        app.data.join("prompts.json"),
        r#"[{"id":"ref","title":"Ref","text":"x","usedAt":17,"attachments":[{"name":"a.png","type":"text/html","size":3,"data":"data:image/png;base64,cG5n"}]}]"#,
    )
    .unwrap();
    let typed = read(Command::ReadPrompts);
    assert!(
        matches!(&typed[..], [Update::Prompts(Ok(library))] if library.len() == 1
            && library[0].used_at == Some(17)
            && library[0].attachments[0].content_type == "image/png"
            && library[0].attachments[0].size == 3),
        "{typed:?}"
    );
    let models = read(Command::Models {
        provider: "nobody".into(),
    });
    assert!(
        matches!(&models[..], [Update::Models { provider, models: Err(e) }] if provider == "nobody" && e == "Unknown provider"),
        "{models:?}"
    );
    let facts = read(Command::ModelInfo {
        provider: "openrouter".into(),
        model: " ".into(),
    });
    assert!(
        matches!(&facts[..], [Update::ModelInfo { facts: Err(e), .. }] if e == "model query parameter is required"),
        "{facts:?}"
    );
    let archive = read(Command::ReadArchive);
    assert!(matches!(&archive[..], [Update::Archive(Ok(a))] if a.conversations.is_empty()));

    // Failures with real causes.
    std::fs::write(app.data.join("prompts.json"), "[not json").unwrap();
    let prompts = read(Command::ReadPrompts);
    assert!(
        matches!(&prompts[..], [Update::Prompts(Err(e))] if !e.is_empty()),
        "{prompts:?}"
    );
    let db = app.sqlite();
    db.execute_batch("DROP TABLE search;").unwrap();
    let found = read(Command::Search {
        scope: SearchScope::Build,
        query: "house".into(),
        request: 42,
    });
    assert!(
        matches!(&found[..], [Update::Found { request: 42, results: Err(e) }] if e.contains("no such table: search")),
        "{found:?}"
    );
    db.execute_batch("DROP TABLE benches;").unwrap();
    let archive = read(Command::ReadArchive);
    assert!(
        matches!(&archive[..], [Update::Archive(Err(e))] if e.contains("benches")),
        "{archive:?}"
    );
}

// The settings commands are in `tests/core_settings.rs`.
