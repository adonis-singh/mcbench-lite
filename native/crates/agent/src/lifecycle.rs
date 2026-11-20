//! The turn lifecycle the HTTP layer holds today (`server/index.js`
//! `handleChat`, `executeTurn`, the bench routes, `advanceBench`,
//! `startRound`, the titles, `startHttpServer`'s recovery and `shutdown`;
//! `server/turn-stream.js`), as [`Turns`], which the app's core owns and
//! wraps. It publishes nothing to the interface: each turn's events go to
//! its [`TurnStream`], which replays them to every sink attached to it, and
//! [`Services::conversations_changed`] says when the chat list changed.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde_json::Value;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use mcbench_providers::api::js;
use mcbench_providers::{CompletionRequest, Provider, ProviderId};
use mcbench_store::json::Json;
use mcbench_store::{
    Attachment, BenchMember, Content, DEFAULT_TITLE, Error, LiveConversation, NewBench, NewMessage,
    Part, PromptLink, Selection, Store, UNSAVED,
};
use mcbench_tools::Tools;
use mcbench_tools::verify::Budgets;

use crate::api::{supports_image_data, supports_image_type};
use crate::context::{Archives, WorkFiles};
use crate::contract::{self, Placement};
use crate::events::{TurnEvent, TurnSink};
use crate::texts;
use crate::turn::{self, SharedSink, TurnSetup};

/// A new bench's title (`BENCH_TITLE`).
pub const BENCH_TITLE: &str = "New bench";

/// A request refused, with the HTTP status today's route answers it with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refusal {
    /// The status (400, 404, 409, 503; 500 for a failure).
    pub status: u16,
    /// The message.
    pub message: String,
}

impl Refusal {
    fn new(status: u16, message: impl Into<String>) -> Refusal {
        Refusal {
            status,
            message: message.into(),
        }
    }
}

impl From<Error> for Refusal {
    fn from(error: Error) -> Refusal {
        Refusal::new(error.status.unwrap_or(500), error.message)
    }
}

impl From<String> for Refusal {
    fn from(message: String) -> Refusal {
        Refusal::new(500, message)
    }
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

/// What the lifecycle needs of the app around it.
#[async_trait]
pub trait Services: Send + Sync {
    /// The provider for a key `settings.PROVIDERS` holds.
    fn provider(&self, key: &str) -> Result<Arc<dyn Provider>, String>;
    /// The tools a turn dispatches to.
    fn tools(&self) -> Tools;
    /// `settings.get().titleModel`: provider, model and effort, when chosen.
    fn title_model(&self) -> Option<(String, String, Option<String>)>;
    /// `sandbox.syncAttachments(chat)`.
    fn sync_attachments(&self, chat: &str) -> Result<(), String>;
    /// `sandbox.reset(chat)`.
    async fn reset_sandbox(&self, chat: &str) -> Result<(), String>;
    /// The chat list changed (`publishConversations`).
    fn conversations_changed(&self);
    /// The sink every turn of `chat` gets before its first event when no
    /// other is attached by then (a bench round's turn, which no request
    /// started; a turn's own request attaches its sink first): the app's
    /// window. None by default.
    fn turn_sink(&self, _chat: &str) -> Option<Box<dyn TurnSink>> {
        None
    }
}

/// The process's settings a turn runs with.
#[derive(Clone)]
pub struct TurnConfig {
    /// `test_build`'s placement mode.
    pub placement: Placement,
    /// `sandbox.networkAccess()`.
    pub network_access: bool,
    /// The verifier's budgets.
    pub budgets: Budgets,
    /// `MCBENCH_CONTEXT_MAX_BYTES`.
    pub max_context_bytes: usize,
    /// `keepExchanges`.
    pub keep_exchanges: usize,
    /// Where context archives go.
    pub work: Arc<dyn WorkFiles>,
}

struct StreamState {
    open: bool,
    events: Vec<TurnEvent>,
    listeners: Vec<Box<dyn TurnSink>>,
}

/// One running turn's events (`TurnStream`): every sink attached gets the
/// events so far, then each as it comes, until the turn ends.
pub struct TurnStream {
    cancel: CancellationToken,
    started_at: i64,
    state: Mutex<StreamState>,
    done: watch::Sender<bool>,
}

impl std::fmt::Debug for TurnStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TurnStream")
            .field("started_at", &self.started_at)
            .field("stopped", &self.stopped())
            .finish_non_exhaustive()
    }
}

impl TurnStream {
    fn new(started_at: i64) -> TurnStream {
        TurnStream {
            cancel: CancellationToken::new(),
            started_at,
            state: Mutex::new(StreamState {
                open: true,
                events: Vec::new(),
                listeners: Vec::new(),
            }),
            done: watch::channel(false).0,
        }
    }

    /// `startedAt`: when the turn was taken (`runningSince`).
    pub fn started_at(&self) -> i64 {
        self.started_at
    }

    /// Stops the turn (`controller.abort()`).
    pub fn stop(&self) {
        self.cancel.cancel();
    }

    /// Whether it was stopped.
    pub fn stopped(&self) -> bool {
        self.cancel.is_cancelled()
    }

    /// Gives `sink` the events so far and every later one; refused once the
    /// turn has ended.
    pub fn attach(&self, mut sink: Box<dyn TurnSink>) -> Result<(), String> {
        let mut state = self.state.lock().expect("a stream's lock");
        if !state.open {
            return Err("This turn has ended".into());
        }
        for event in &state.events {
            sink.emit(event.clone());
        }
        state.listeners.push(sink);
        Ok(())
    }

    /// Runs `look` with the events so far while no event can be pushed (the
    /// stream's lock is held), so what it does is placed between two of the
    /// turn's events; refused once the turn has ended.
    pub fn with_events<T>(&self, look: impl FnOnce(&[TurnEvent]) -> T) -> Result<T, String> {
        let state = self.state.lock().expect("a stream's lock");
        if !state.open {
            return Err("This turn has ended".into());
        }
        Ok(look(&state.events))
    }

    /// Attaches `sink` when no sink is attached yet (before the first event).
    fn attach_when_unheard(&self, sink: Box<dyn TurnSink>) {
        let mut state = self.state.lock().expect("a stream's lock");
        assert!(state.open, "a turn's stream is open before its first event");
        if state.listeners.is_empty() {
            assert!(state.events.is_empty(), "attached before the first event");
            state.listeners.push(sink);
        }
    }

    fn push(&self, event: TurnEvent) {
        let mut state = self.state.lock().expect("a stream's lock");
        if !state.open {
            drop(state);
            panic!("This turn has ended");
        }
        for listener in &mut state.listeners {
            listener.emit(event.clone());
        }
        state.events.push(event);
    }

    fn end(&self) {
        let mut state = self.state.lock().expect("a stream's lock");
        if !state.open {
            drop(state);
            panic!("This turn has ended");
        }
        state.open = false;
        state.listeners.clear();
        drop(state);
        self.done.send_replace(true);
    }

    /// Resolves once the turn has ended.
    pub async fn done(&self) {
        let mut done = self.done.subscribe();
        // An error is the sender gone, which also means ended.
        let _ = done.wait_for(|ended| *ended).await;
    }
}

struct StreamSink(Arc<TurnStream>);

impl TurnSink for StreamSink {
    fn emit(&mut self, event: TurnEvent) {
        self.0.push(event);
    }
}

/// A turn's hold while the turn runs: dropped before the turn's end gave
/// it back (a panic in the turn, or its task dropped), it gives it back.
struct Unfinished {
    turns: Turns,
    key: String,
    hold: Option<Hold>,
}

impl Drop for Unfinished {
    fn drop(&mut self) {
        let Some(hold) = self.hold.take() else { return };
        let mut holds = self.turns.0.holds.lock().unwrap_or_else(|e| e.into_inner());
        if holds
            .held
            .get(&self.key)
            .is_some_and(|held| held.same(&hold))
        {
            holds.held.remove(&self.key);
            drop(holds);
            hold.end();
            self.turns.0.services.conversations_changed();
        }
    }
}

/// What holds a chat or a bench (`bench:<id>`).
#[derive(Clone)]
enum Hold {
    Turn(Arc<TurnStream>),
    Edit(Arc<watch::Sender<bool>>),
}

impl Hold {
    fn end(&self) {
        match self {
            Hold::Turn(turn) => turn.end(),
            Hold::Edit(done) => {
                done.send_replace(true);
            }
        }
    }

    fn same(&self, other: &Hold) -> bool {
        match (self, other) {
            (Hold::Turn(a), Hold::Turn(b)) => Arc::ptr_eq(a, b),
            (Hold::Edit(a), Hold::Edit(b)) => Arc::ptr_eq(a, b),
            _ => false,
        }
    }

    async fn done(&self) {
        match self {
            Hold::Turn(turn) => turn.done().await,
            Hold::Edit(done) => {
                let _ = done.subscribe().wait_for(|ended| *ended).await;
            }
        }
    }
}

#[derive(Default)]
struct Holds {
    held: HashMap<String, Hold>,
    closed: bool,
}

/// What a title is made from: the text, the images as data URLs, and the
/// chat's provider and model (`generateTitle`'s `text`, `images`, `chatModel`).
type TitleSource = (String, Vec<String>, (String, String));

#[derive(Clone, Debug, PartialEq)]
enum TitleState {
    Titling(String),
    Failed,
}

struct Titles {
    cancel: CancellationToken,
    states: Mutex<HashMap<String, TitleState>>,
    tasks: Mutex<Vec<JoinHandle<()>>>,
}

struct Inner {
    store: Arc<Store>,
    services: Arc<dyn Services>,
    config: TurnConfig,
    archives: Arc<Archives>,
    holds: Mutex<Holds>,
    titles: Titles,
    turns: Mutex<Vec<JoinHandle<()>>>,
}

/// A user message as a route's body gives it (`readMessage`'s input).
#[derive(Clone, Debug, Default)]
pub struct Outgoing {
    /// `text`, trimmed here.
    pub text: String,
    /// `files`: `{ name, data }` with the bytes as a data URL.
    pub files: Vec<Json>,
    /// `selections`, already read (`readSelection`).
    pub selections: Vec<Selection>,
    /// `prompts`: the library prompts it came from.
    pub prompts: Vec<PromptLink>,
}

/// `POST /api/conversations/:id/chat`'s body.
#[derive(Clone, Debug, Default)]
pub struct ChatMessage {
    /// `provider`.
    pub provider: String,
    /// `model`.
    pub model: String,
    /// `effort`, when the body has a string for it.
    pub effort: Option<String>,
    /// `resolution`.
    pub resolution: u32,
    /// The message.
    pub message: Outgoing,
    /// `editOf`: the prompt this is a new version of.
    pub edit_of: Option<String>,
}

/// `POST /api/benches`' body.
#[derive(Clone, Debug, Default)]
pub struct BenchRequest {
    /// `members`: provider, model, effort each.
    pub members: Vec<(String, String, Option<String>)>,
    /// `blind`.
    pub blind: bool,
    /// `resolution`.
    pub resolution: u32,
    /// The first round.
    pub message: Outgoing,
}

struct Message0 {
    text: String,
    files: Vec<Json>,
    selections: Vec<Selection>,
    prompts: Vec<PromptLink>,
}

fn read_message(message: &Outgoing) -> Result<Message0, Refusal> {
    let text = js::trim(&message.text).to_string();
    if message.files.len() > crate::MAX_ATTACHMENTS {
        return Err(Refusal::new(
            400,
            "A message supports at most 8 attachments",
        ));
    }
    if text.is_empty() && message.files.is_empty() {
        return Err(Refusal::new(
            400,
            "Message text or an attachment is required",
        ));
    }
    Ok(Message0 {
        text,
        files: message.files.clone(),
        selections: message.selections.clone(),
        prompts: message.prompts.clone(),
    })
}

fn read_resolution(resolution: u32) -> Result<u32, Refusal> {
    let known = contract::resolutions();
    if !known.contains(&resolution) {
        let list: Vec<String> = known.iter().map(u32::to_string).collect();
        return Err(Refusal::new(
            400,
            format!("Resolution must be one of {}", list.join(", ")),
        ));
    }
    Ok(resolution)
}

fn known_provider(key: &str) -> bool {
    serde_json::from_value::<ProviderId>(Value::String(key.into())).is_ok()
}

fn bench_key(id: &str) -> String {
    format!("bench:{id}")
}

/// The image data URLs of a message's files (`titleImages`).
fn title_images(files: &[Json]) -> Vec<String> {
    files
        .iter()
        .filter_map(|file| file.get("data").and_then(Json::as_str))
        .filter(|data| supports_image_data(data))
        .map(str::to_string)
        .collect()
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    tokio::task::spawn_blocking(f)
        .await
        .expect("a store task does not panic")
}

/// The turn lifecycle of one data folder.
#[derive(Clone)]
pub struct Turns(Arc<Inner>);

impl Turns {
    /// The lifecycle over `store`.
    pub fn new(store: Arc<Store>, services: Arc<dyn Services>, config: TurnConfig) -> Turns {
        Turns(Arc::new(Inner {
            store,
            services,
            config,
            archives: Arc::new(Archives::default()),
            holds: Mutex::new(Holds::default()),
            titles: Titles {
                cancel: CancellationToken::new(),
                states: Mutex::new(HashMap::new()),
                tasks: Mutex::new(Vec::new()),
            },
            turns: Mutex::new(Vec::new()),
        }))
    }

    /// Runs `f` on tokio's blocking pool (the store, SQLite and the
    /// files are blocking), with this lifecycle.
    async fn on_store<T: Send + 'static>(&self, f: impl FnOnce(&Turns) -> T + Send + 'static) -> T {
        let turns = self.clone();
        blocking(move || f(&turns)).await
    }

    /// The store.
    pub fn store(&self) -> &Arc<Store> {
        &self.0.store
    }

    // ----- Holds (`Reservations`) -----

    fn take(&self, keys: &[String], turn: bool, busy: &str) -> Result<Vec<Hold>, Refusal> {
        let mut holds = self.0.holds.lock().expect("the holds' lock");
        if holds.closed {
            return Err(Refusal::new(503, "The server is shutting down"));
        }
        if keys.iter().any(|key| holds.held.contains_key(key)) {
            return Err(Refusal::new(409, busy));
        }
        let now = self.0.store.host().now_ms();
        Ok(keys
            .iter()
            .map(|key| {
                let hold = if turn {
                    Hold::Turn(Arc::new(TurnStream::new(now)))
                } else {
                    Hold::Edit(Arc::new(watch::channel(false).0))
                };
                holds.held.insert(key.clone(), hold.clone());
                hold
            })
            .collect())
    }

    fn release(&self, key: &str, hold: &Hold) {
        let mut holds = self.0.holds.lock().expect("the holds' lock");
        let ours = holds.held.get(key).is_some_and(|held| held.same(hold));
        if ours {
            holds.held.remove(key);
        }
        drop(holds);
        assert!(ours, "{key} is not held by this hold");
        hold.end();
    }

    fn release_turn(&self, key: &str, hold: &Hold) {
        self.release(key, hold);
        self.0.services.conversations_changed();
    }

    /// Whether anything holds `key` (a chat, or `bench:<id>`).
    pub fn held(&self, key: &str) -> bool {
        self.0
            .holds
            .lock()
            .expect("the holds' lock")
            .held
            .contains_key(key)
    }

    /// `busy()`'s first half (`reservations.holds.size ||
    /// titles.tasks.size`): something holds a chat or a bench (a turn, an
    /// edit, a round), or a title is under way. The queued bench rounds are
    /// the store's (the caller's other half).
    pub fn busy(&self) -> bool {
        if !self
            .0
            .holds
            .lock()
            .expect("the holds' lock")
            .held
            .is_empty()
        {
            return true;
        }
        let mut tasks = self.0.titles.tasks.lock().expect("titles");
        tasks.retain(|task| !task.is_finished());
        !tasks.is_empty()
    }

    fn hold(&self, key: &str) -> Option<Hold> {
        self.0
            .holds
            .lock()
            .expect("the holds' lock")
            .held
            .get(key)
            .cloned()
    }

    /// The running turn of a chat.
    pub fn turn(&self, chat: &str) -> Option<Arc<TurnStream>> {
        match self.hold(chat) {
            Some(Hold::Turn(turn)) => Some(turn),
            _ => None,
        }
    }

    /// Whether the lifecycle is closed (shutting down).
    pub fn closed(&self) -> bool {
        self.0.holds.lock().expect("the holds' lock").closed
    }

    /// Resolves once nothing holds `key`.
    pub async fn free(&self, key: &str) {
        while let Some(hold) = self.hold(key) {
            hold.done().await;
        }
    }

    async fn editing<T>(
        &self,
        keys: Vec<String>,
        busy: &str,
        work: impl std::future::Future<Output = Result<T, Refusal>>,
    ) -> Result<T, Refusal> {
        let holds = self.take(&keys, false, busy)?;
        let result = work.await;
        for (key, hold) in keys.iter().zip(&holds) {
            self.release(key, hold);
        }
        result
    }

    // ----- Titles -----

    fn title_state(
        &self,
        id: &str,
        title: &str,
        untitled: &str,
        prompts: &[String],
    ) -> &'static str {
        match self.0.titles.states.lock().expect("titles").get(id) {
            None => {
                if title == untitled {
                    "untitled"
                } else {
                    "titled"
                }
            }
            Some(TitleState::Titling(prompt)) if !prompts.contains(prompt) => "untitled",
            Some(TitleState::Titling(_)) => "titling",
            Some(TitleState::Failed) => "failed",
        }
    }

    fn titling(&self, id: &str, prompt: &str) -> bool {
        matches!(self.0.titles.states.lock().expect("titles").get(id), Some(TitleState::Titling(p)) if p == prompt)
    }

    /// `generateTitle(text, images, chatModel, signal)`.
    async fn generate_title(
        &self,
        text: String,
        images: Vec<String>,
        chat_model: (String, String),
    ) -> Result<String, String> {
        let (provider, model, effort) = self.0.services.title_model().unwrap_or((
            chat_model.0,
            chat_model.1,
            Some("low".into()),
        ));
        let provider = self.0.services.provider(&provider)?;
        let request = CompletionRequest {
            conversation_id: None,
            model,
            effort,
            messages: texts::title_messages(&text, &images),
            tools: Vec::new(),
            chat: false,
            transcript: None,
        };
        let message = provider
            .complete(&request, &mut |_| {}, &self.0.titles.cancel)
            .await
            .map_err(|error| error.message)?;
        texts::title_of(&message.content)
    }

    /// `makeTitle(id, prompt, ...)`: titles `id` for `prompt` in the
    /// background; `taken` or `lost` run only while `id` is still titling
    /// for it.
    fn make_title<Taken, Lost, TakenF, LostF>(
        &self,
        id: String,
        prompt: String,
        source: TitleSource,
        taken: Taken,
        lost: Lost,
    ) where
        Taken: FnOnce(Turns, String) -> TakenF + Send + 'static,
        TakenF: std::future::Future<Output = Result<(), String>> + Send,
        Lost: FnOnce(Turns, String) -> LostF + Send + 'static,
        LostF: std::future::Future<Output = Result<(), String>> + Send,
    {
        self.0
            .titles
            .states
            .lock()
            .expect("titles")
            .insert(id.clone(), TitleState::Titling(prompt.clone()));
        let turns = self.clone();
        let task = tokio::spawn(async move {
            let (text, images, chat_model) = source;
            let result = turns.generate_title(text, images, chat_model).await;
            let outcome = match result {
                Ok(title) => {
                    if !turns.titling(&id, &prompt) {
                        return;
                    }
                    turns.0.titles.states.lock().expect("titles").remove(&id);
                    taken(turns.clone(), title).await
                }
                Err(error) => {
                    if turns.0.titles.cancel.is_cancelled() || !turns.titling(&id, &prompt) {
                        return;
                    }
                    turns
                        .0
                        .titles
                        .states
                        .lock()
                        .expect("titles")
                        .insert(id.clone(), TitleState::Failed);
                    lost(turns.clone(), error).await
                }
            };
            if let Err(error) = outcome {
                eprintln!("[title] {error}");
            }
        });
        self.0.titles.tasks.lock().expect("titles").push(task);
    }

    /// `titleChat(id, prompt, text, images, chatModel)`.
    fn title_chat(
        &self,
        id: String,
        prompt: String,
        text: String,
        images: Vec<String>,
        model: (String, String),
    ) {
        let (taken_id, lost_id) = (id.clone(), id.clone());
        self.make_title(
            id,
            prompt.clone(),
            (text, images, model),
            move |turns: Turns, title: String| async move {
                let store = Arc::clone(&turns.0.store);
                blocking(move || -> Result<(), String> {
                    let Some(live) = store.conversation(&taken_id).map_err(|e| e.message)? else {
                        return Ok(());
                    };
                    let mut conversation = live.lock().expect("a chat's lock");
                    if !conversation.messages.iter().any(|m| m.id.0 == prompt) {
                        return Ok(());
                    }
                    store
                        .set_title(&mut conversation, &title)
                        .map_err(|e| e.message)
                })
                .await?;
                turns.0.services.conversations_changed();
                Ok(())
            },
            move |turns: Turns, error: String| async move {
                turns.free(&lost_id).await;
                let store = Arc::clone(&turns.0.store);
                blocking(move || -> Result<(), String> {
                    let Some(live) = store.conversation(&lost_id).map_err(|e| e.message)? else {
                        return Ok(());
                    };
                    let mut conversation = live.lock().expect("a chat's lock");
                    store
                        .append_message(
                            &mut conversation,
                            NewMessage::error(&format!("Title generation failed: {error}"), None),
                            None,
                        )
                        .map_err(|e| e.message)?;
                    store.save(&mut conversation).map_err(|e| e.message)
                })
                .await?;
                turns.0.services.conversations_changed();
                Ok(())
            },
        );
    }

    /// `titleBench(id)`: a bench still untitled is titled from its first
    /// round.
    fn title_bench(&self, id: &str) -> Result<(), Refusal> {
        let store = &self.0.store;
        let Some(bench) = store.bench(id)? else {
            return Ok(());
        };
        let round_ids: Vec<String> = (0..bench.rounds.len())
            .filter_map(|i| bench.round_id(i).map(str::to_string))
            .collect();
        let state = self.title_state(id, &bench.title, BENCH_TITLE, &round_ids);
        if bench.rounds.is_empty() || (state != "untitled" && state != "failed") {
            return Ok(());
        }
        let first = round_ids[0].clone();
        let parts = match bench.round_content(0)? {
            Content::Text(text) => vec![Part::Text { text }],
            Content::Parts(parts) => parts,
        };
        let files: Vec<Attachment> = parts
            .iter()
            .filter_map(|part| match part {
                Part::Attachment { attachment } => Some(attachment.clone()),
                _ => None,
            })
            .collect();
        let text: Vec<&str> = parts
            .iter()
            .filter_map(|part| match part {
                Part::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        let text = text.join("\n");
        let mut images = Vec::new();
        for file in files.iter().filter(|file| supports_image_type(&file.mime)) {
            use base64::Engine as _;
            let bytes = store.read_attachment(&file.url)?;
            images.push(format!(
                "data:{};base64,{}",
                file.mime,
                base64::engine::general_purpose::STANDARD.encode(bytes)
            ));
        }
        let members = store.bench_members(id)?;
        let model = {
            let first = members
                .first()
                .ok_or_else(|| Refusal::from("a bench has models".to_string()))?;
            let member = first.lock().expect("a chat's lock");
            (member.provider.clone(), member.model.clone())
        };
        let names: Vec<String> = files.iter().map(|f| f.name.clone()).collect();
        let bench_id = id.to_string();
        let round = first.clone();
        self.make_title(
            id.to_string(),
            first,
            (texts::title_source(&text, &names), images, model),
            move |turns: Turns, title: String| async move {
                let store = Arc::clone(&turns.0.store);
                blocking(move || -> Result<(), String> {
                    let Some(mut stored) = store.bench(&bench_id).map_err(|e| e.message)? else {
                        return Ok(());
                    };
                    if stored.round_id(0) != Some(round.as_str()) {
                        return Ok(());
                    }
                    store
                        .set_bench_title(&mut stored, &title)
                        .map_err(|e| e.message)
                })
                .await?;
                turns.0.services.conversations_changed();
                Ok(())
            },
            |_, error: String| async move {
                eprintln!("[title] {error}");
                Ok(())
            },
        );
        Ok(())
    }

    // ----- Chats -----

    fn own_chat(&self, id: &str) -> Result<LiveConversation, Refusal> {
        let live = self
            .0
            .store
            .conversation(id)?
            .ok_or_else(|| Refusal::new(404, "Conversation not found"))?;
        if live.lock().expect("a chat's lock").bench.is_some() {
            return Err(Refusal::new(
                409,
                "A bench chat is changed only through its bench",
            ));
        }
        Ok(live)
    }

    /// `storeMessage(message, owner, write)`: the files saved first, then
    /// `write` with the content; a write that fails leaves no file.
    fn store_message<T>(
        &self,
        message: &Message0,
        owner: &str,
        write: impl FnOnce(&[Attachment], Content, &[PromptLink]) -> Result<T, Refusal>,
    ) -> Result<T, Refusal> {
        let store = &self.0.store;
        let saved = store.save_attachments(owner, &message.files)?;
        let content = if !saved.is_empty() || !message.selections.is_empty() {
            let mut parts = vec![Part::Text {
                text: message.text.clone(),
            }];
            parts.extend(saved.iter().map(|attachment| Part::Attachment {
                attachment: attachment.clone(),
            }));
            parts.extend(message.selections.iter().map(|selection| Part::Selection {
                selection: selection.clone(),
            }));
            Content::Parts(parts)
        } else {
            Content::Text(message.text.clone())
        };
        match write(&saved, content, &message.prompts) {
            Ok(value) => Ok(value),
            Err(error) => {
                store.discard_attachments(owner, &saved)?;
                Err(error)
            }
        }
    }

    /// `handleChat`: a message to chat `id` (an edit of prompt `edit_of`
    /// when given), checked whole before anything changes, then its turn,
    /// running in the background. `listener` is attached before the turn
    /// starts (as the route attaches its response); others can attach to the
    /// stream returned.
    pub async fn send(
        &self,
        id: &str,
        body: ChatMessage,
        listener: Box<dyn TurnSink>,
    ) -> Result<Arc<TurnStream>, Refusal> {
        let chat = id.to_string();
        let live = self.on_store(move |t| t.own_chat(&chat)).await?;
        let keys = [id.to_string()];
        let hold = self
            .take(
                &keys,
                true,
                "A turn is already running for this conversation",
            )?
            .remove(0);
        let Hold::Turn(stream) = &hold else {
            unreachable!("a turn's hold")
        };
        let stream = Arc::clone(stream);
        let prepared = self.prepare_chat(id, &live, body).await;
        let title = match prepared {
            Ok(title) => title,
            Err(error) => {
                self.release_turn(id, &hold);
                return Err(error);
            }
        };
        self.0.services.conversations_changed();
        stream.attach(listener)?;
        let turns = self.clone();
        let chat = id.to_string();
        let task = tokio::spawn(async move {
            let mut started = false;
            let titles = turns.clone();
            let titled = chat.clone();
            let mut on_started = move || {
                if started {
                    return;
                }
                started = true;
                if let Some((prompt, text, images, model)) = title.clone() {
                    titles.title_chat(titled.clone(), prompt, text, images, model);
                }
            };
            turns.execute_turn(&chat, hold, &mut on_started).await;
        });
        self.0.turns.lock().expect("turns").push(task);
        Ok(stream)
    }

    /// The checks and the stored prompt of `handleChat`; the title to make,
    /// if any.
    async fn prepare_chat(
        &self,
        id: &str,
        live: &LiveConversation,
        body: ChatMessage,
    ) -> Result<Option<(String, String, Vec<String>, (String, String))>, Refusal> {
        if self.closed() {
            return Err(Refusal::new(503, "The server is shutting down"));
        }
        let provider = js::trim(&body.provider).to_string();
        let model = js::trim(&body.model).to_string();
        if !known_provider(&provider) {
            return Err(Refusal::new(400, "Choose a configured provider"));
        }
        if model.is_empty() {
            return Err(Refusal::new(400, "Choose a model"));
        }
        let resolution = read_resolution(body.resolution)?;
        let message = read_message(&body.message)?;
        let edit = body.edit_of.clone();
        let turns = self.clone();
        let live = Arc::clone(live);
        let chat = id.to_string();
        let effort = body.effort.clone();
        let title = blocking(move || {
            let store = Arc::clone(&turns.0.store);
            turns.store_message(&message, &chat, |files, content, prompts| {
                let mut conversation = live.lock().expect("a chat's lock");
                let mut title = None;
                let mut append =
                    |conversation: &mut mcbench_store::Conversation| -> Result<(), Error> {
                        turns
                            .0
                            .services
                            .sync_attachments(&chat)
                            .map_err(Error::plain)?;
                        if let Some(effort) = &effort {
                            let effort = js::trim(effort);
                            conversation.effort = (!effort.is_empty()).then(|| effort.to_string());
                        }
                        conversation.resolution = resolution;
                        conversation.provider = provider.clone();
                        conversation.model = model.clone();
                        let ids: Vec<String> = conversation
                            .messages
                            .iter()
                            .map(|m| m.id.0.clone())
                            .collect();
                        let state =
                            turns.title_state(&chat, &conversation.title, DEFAULT_TITLE, &ids);
                        let untitled = state == "untitled"
                            || state == "failed"
                            || (conversation.messages.is_empty()
                                && !store.repeats_latest_root_prompt(conversation, &content)?);
                        store.end_unfinished_turn(conversation, UNSAVED)?;
                        let checkpoint = conversation.world.reference();
                        let prompt = store.append_message(
                            conversation,
                            NewMessage::prompt(&content, prompts),
                            Some(checkpoint),
                        )?;
                        store.save(conversation)?;
                        if untitled {
                            let names: Vec<String> = files.iter().map(|f| f.name.clone()).collect();
                            title = Some((
                                prompt.id.0.clone(),
                                texts::title_source(&message.text, &names),
                                title_images(&message.files),
                                (provider.clone(), model.clone()),
                            ));
                        }
                        Ok(())
                    };
                match &edit {
                    None => append(&mut conversation)?,
                    Some(edit) => store.edit_prompt(&mut conversation, edit, append)?,
                }
                Ok(title)
            })
        })
        .await?;
        if body.edit_of.is_some() {
            self.0.services.reset_sandbox(id).await?;
        }
        Ok(title)
    }

    /// `executeTurn(id, turn, onStarted)`: runs the turn whose prompt is
    /// stored and records how it ended; the hold is released however it
    /// ends.
    async fn execute_turn(&self, id: &str, hold: Hold, on_started: &mut (dyn FnMut() + Send)) {
        let Hold::Turn(stream) = &hold else {
            unreachable!("a turn's hold")
        };
        // `executeTurn`'s `finally`: a turn that never reaches its end (its
        // task panicked, which the core reports as fatal, or was dropped)
        // still gives its hold back, so the shutdown that follows does not
        // wait for it.
        let mut unfinished = Unfinished {
            turns: self.clone(),
            key: id.to_string(),
            hold: Some(hold.clone()),
        };
        let stream = Arc::clone(stream);
        if let Some(app) = self.0.services.turn_sink(id) {
            stream.attach_when_unheard(app);
        }
        let sink: SharedSink = Arc::new(Mutex::new(StreamSink(Arc::clone(&stream))));
        let emit = |event: TurnEvent| stream.push(event);
        let store = Arc::clone(&self.0.store);
        let chat = id.to_string();
        let loaded = {
            let (store, chat) = (Arc::clone(&store), chat.clone());
            blocking(move || store.conversation(&chat)).await
        };
        let mut bench_id = None;
        let mut provider: Option<Arc<dyn Provider>> = None;
        let outcome: Result<(), turn::TurnError> = async {
            let live = loaded
                .map_err(|e| turn::TurnError::from(e.message))?
                .ok_or_else(|| turn::TurnError::from(format!("Conversation {chat} not found")))?;
            let (provider_key, bench) = {
                let conversation = live.lock().expect("a chat's lock");
                (
                    conversation.provider.clone(),
                    conversation.bench.as_ref().map(|(b, _)| b.0.clone()),
                )
            };
            bench_id = bench;
            emit(TurnEvent::Snapshot);
            let chosen = self.0.services.provider(&provider_key)?;
            provider = Some(Arc::clone(&chosen));
            let setup = TurnSetup {
                store: Arc::clone(&store),
                conversation: Arc::clone(&live),
                provider: chosen,
                tools: self.0.services.tools(),
                chat: bench_id.is_none(),
                placement: self.0.config.placement,
                network_access: self.0.config.network_access,
                budgets: self.0.config.budgets,
                max_context_bytes: self.0.config.max_context_bytes,
                keep_exchanges: self.0.config.keep_exchanges,
                archives: Arc::clone(&self.0.archives),
                work: Arc::clone(&self.0.config.work),
            };
            turn::run_turn(&setup, &sink, &stream.cancel, on_started).await?;
            let saved = Arc::clone(&store);
            blocking(move || saved.save(&mut live.lock().expect("a chat's lock")))
                .await
                .map_err(|e| turn::TurnError::from(e.message))?;
            emit(TurnEvent::Done);
            Ok(())
        }
        .await;
        if let Err(error) = outcome {
            let stopped = stream.stopped();
            if !stopped {
                eprintln!("[turn] {}", error.message);
            }
            let message = if stopped {
                "(stopped by user)".to_string()
            } else {
                error.message.clone()
            };
            let ended = {
                let (store, chat, message) = (Arc::clone(&store), chat.clone(), message.clone());
                blocking(move || -> Result<String, Error> {
                    let live = store
                        .conversation(&chat)?
                        .ok_or_else(|| Error::plain(format!("Conversation {chat} not found")))?;
                    let mut conversation = live.lock().expect("a chat's lock");
                    let last = if stopped {
                        NewMessage::stopped(
                            &message,
                            Some(&error.reply),
                            error.telemetry.as_deref(),
                        )
                    } else {
                        NewMessage::error(&message, error.telemetry.as_deref())
                    };
                    Ok(store.end_turn(&mut conversation, last)?.id.0)
                })
                .await
            };
            match ended {
                Ok(message_id) => {
                    if stopped {
                        emit(TurnEvent::Assistant {
                            text: message,
                            last: true,
                        });
                        emit(TurnEvent::Done);
                    } else {
                        emit(TurnEvent::Error {
                            message,
                            message_id: Some(message_id),
                        });
                    }
                }
                Err(failure) => {
                    eprintln!("[turn] saving how the turn ended failed: {failure}");
                    emit(TurnEvent::Error {
                        message: format!("{message} (saving this failed: {})", failure.message),
                        message_id: None,
                    });
                }
            }
        }
        on_started();
        {
            let (store, chat) = (Arc::clone(&store), chat.clone());
            if let Err(error) = blocking(move || store.mark_finished(&chat)).await {
                eprintln!("[turn] {error}");
            }
        }
        if let Some(provider) = provider {
            provider.close_session(id).await;
        }
        unfinished.hold = None;
        self.release_turn(id, &hold);
        if let Some(bench) = bench_id {
            self.advance_bench(&bench).await;
        }
    }

    /// `POST /api/conversations/:id/stop`.
    pub fn stop(&self, id: &str) {
        if let Some(turn) = self.turn(id) {
            turn.stop();
        }
    }

    /// `POST /api/conversations/:id/branch`: the latest branch through
    /// `message` becomes the chat's, its sandbox reset before anything
    /// starts on it.
    pub async fn branch(&self, id: &str, message: &str) -> Result<(), Refusal> {
        let chat = id.to_string();
        let live = self.on_store(move |t| t.own_chat(&chat)).await?;
        let store = Arc::clone(&self.0.store);
        let message = message.to_string();
        self.editing(
            vec![id.to_string()],
            "Stop the running turn before switching branches",
            async {
                blocking(move || {
                    store.activate_branch(&mut live.lock().expect("a chat's lock"), &message)
                })
                .await?;
                self.0.services.reset_sandbox(id).await?;
                Ok(())
            },
        )
        .await
    }

    /// `POST /api/conversations/:id/dismiss-error`: refused under a turn.
    pub async fn dismiss_error(&self, id: &str, message: &str) -> Result<(), Refusal> {
        let store = Arc::clone(&self.0.store);
        let chat = id.to_string();
        let live = self
            .on_store(move |t| t.0.store.conversation(&chat))
            .await?
            .ok_or_else(|| Refusal::new(404, "Conversation not found"))?;
        if self.held(id) {
            return Err(Refusal::new(
                409,
                "Wait for the running turn before dismissing an error",
            ));
        }
        let message = message.to_string();
        blocking(move || store.dismiss_error(&mut live.lock().expect("a chat's lock"), &message))
            .await?;
        Ok(())
    }

    // ----- Benches -----

    /// `memberProgress(id)`: rounds sent, and what holds its queue (nothing
    /// while it is held).
    pub fn member_progress(&self, id: &str) -> Result<(i64, Option<&'static str>), Refusal> {
        let progress = self.0.store.bench_progress(id)?;
        Ok((
            progress.round,
            if self.held(id) { None } else { progress.halted },
        ))
    }

    /// `POST /api/benches`: a bench of 2 to 8 models, written whole, its
    /// first round started and its title made.
    pub async fn create_bench(&self, body: BenchRequest) -> Result<String, Refusal> {
        let members: Vec<BenchMember> = body
            .members
            .iter()
            .map(|(provider, model, effort)| BenchMember {
                provider: js::trim(provider).to_string(),
                model: js::trim(model).to_string(),
                effort: effort
                    .as_deref()
                    .map(js::trim)
                    .filter(|e| !e.is_empty())
                    .map(str::to_string),
            })
            .collect();
        if members.len() < 2 || members.len() > 8 {
            return Err(Refusal::new(400, "A bench runs 2 to 8 models"));
        }
        if members
            .iter()
            .any(|m| !known_provider(&m.provider) || m.model.is_empty())
        {
            return Err(Refusal::new(400, "Every model needs a configured provider"));
        }
        if !body.message.selections.is_empty() {
            return Err(Refusal::new(
                400,
                "Selections belong to one build, not a bench",
            ));
        }
        let resolution = read_resolution(body.resolution)?;
        let id = self.0.store.host().uuid();
        let message = read_message(&body.message)?;
        let turns = self.clone();
        let bench_id = id.clone();
        let blind = body.blind;
        blocking(move || {
            turns.store_message(&message, &bench_id, |_, content, prompts| {
                turns
                    .0
                    .store
                    .create_bench(
                        &bench_id,
                        NewBench {
                            title: BENCH_TITLE.into(),
                            blind,
                            resolution: Some(resolution),
                            content,
                            prompts: prompts.to_vec(),
                            members,
                        },
                    )
                    .map(drop)
                    .map_err(Refusal::from)
            })
        })
        .await?;
        self.advance_bench(&id).await;
        let bench = id.clone();
        self.on_store(move |t| t.title_bench(&bench)).await?;
        Ok(id)
    }

    fn bench_of(&self, id: &str) -> Result<mcbench_store::Bench, Refusal> {
        self.0
            .store
            .bench(id)?
            .ok_or_else(|| Refusal::new(404, "Bench not found"))
    }

    fn member_ids(&self, bench: &str) -> Result<Vec<String>, Refusal> {
        Ok(self
            .0
            .store
            .bench_members(bench)?
            .iter()
            .map(|m| m.lock().expect("a chat's lock").id.0.clone())
            .collect())
    }

    /// `POST /api/benches/:id/rounds`: the next round, or with `edit_of` a
    /// new version of that round. Returns the bench's round count.
    pub async fn bench_round(
        &self,
        id: &str,
        message: Outgoing,
        edit_of: Option<String>,
    ) -> Result<usize, Refusal> {
        let bench_id = id.to_string();
        let mut bench = self.on_store(move |t| t.bench_of(&bench_id)).await?;
        if !message.selections.is_empty() {
            return Err(Refusal::new(
                400,
                "Selections belong to one build, not a bench",
            ));
        }
        let owner = id.to_string();
        match edit_of {
            None => {
                if self.held(&bench_key(id)) {
                    return Err(Refusal::new(409, "Wait for the round edit to finish"));
                }
                let message = read_message(&message)?;
                self.on_store(move |t| {
                    t.store_message(&message, &owner, |_, content, prompts| {
                        t.0.store
                            .append_round(&mut bench, &content, prompts)
                            .map_err(Refusal::from)
                    })
                })
                .await?;
            }
            Some(edit) => {
                let index = (0..bench.rounds.len())
                    .find(|&i| bench.round_id(i) == Some(edit.as_str()))
                    .ok_or_else(|| Refusal::new(400, "Unknown round"))?;
                let message = read_message(&message)?;
                let mut keys = vec![bench_key(id)];
                let bench_id = id.to_string();
                keys.extend(self.on_store(move |t| t.member_ids(&bench_id)).await?);
                self.editing(
                    keys,
                    "Stop the running models before editing a round",
                    async {
                        let reverted = self
                            .on_store(move |t| {
                                let reverted =
                                    t.store_message(&message, &owner, |_, content, prompts| {
                                        t.0.store
                                            .edit_bench_round(&mut bench, index, &content, prompts)
                                            .map_err(Refusal::from)
                                    })?;
                                Ok::<_, Refusal>(
                                    reverted
                                        .iter()
                                        .map(|m| m.lock().expect("a chat's lock").id.0.clone())
                                        .collect::<Vec<_>>(),
                                )
                            })
                            .await?;
                        for member in reverted {
                            self.0.services.reset_sandbox(&member).await?;
                        }
                        Ok(())
                    },
                )
                .await?;
            }
        }
        self.advance_bench(id).await;
        let bench_id = id.to_string();
        self.on_store(move |t| {
            t.title_bench(&bench_id)?;
            Ok::<_, Refusal>(t.bench_of(&bench_id)?.rounds.len())
        })
        .await
    }

    /// `POST /api/benches/:id/retry`: a model whose turn failed or was
    /// stopped runs that round again once its sandbox is reset.
    pub async fn retry_member(&self, id: &str, member: &str) -> Result<(), Refusal> {
        let (bench_id, member_id) = (id.to_string(), member.to_string());
        let (bench, live, progress) = self
            .on_store(move |t| {
                let bench = t.bench_of(&bench_id)?;
                let live =
                    t.0.store
                        .bench_members(&bench_id)?
                        .into_iter()
                        .find(|m| m.lock().expect("a chat's lock").id.0 == member_id)
                        .ok_or_else(|| Refusal::new(404, "This model is not in the bench"))?;
                let progress = t.member_progress(&member_id)?;
                Ok::<_, Refusal>((bench, live, progress))
            })
            .await?;
        if progress.1.is_none() {
            return Err(Refusal::new(
                409,
                "Only a failed or stopped model can be retried",
            ));
        }
        self.editing(
            vec![member.to_string()],
            "Only a failed or stopped model can be retried",
            async {
                let mut bench = bench;
                self.on_store(move |t| {
                    t.0.store
                        .retry_bench_round(&mut bench, &mut live.lock().expect("a chat's lock"))
                })
                .await?;
                self.0.services.reset_sandbox(member).await?;
                Ok(())
            },
        )
        .await?;
        self.advance_bench(id).await;
        let bench_id = id.to_string();
        self.on_store(move |t| t.title_bench(&bench_id)).await?;
        Ok(())
    }

    /// `POST /api/benches/:id/branch`: the bench and each model follow
    /// another version of a round. Returns the round's number.
    pub async fn bench_branch(&self, id: &str, round: &str) -> Result<usize, Refusal> {
        let bench_id = id.to_string();
        let (bench, members) = self
            .on_store(move |t| Ok::<_, Refusal>((t.bench_of(&bench_id)?, t.member_ids(&bench_id)?)))
            .await?;
        if !bench
            .nodes
            .iter()
            .any(|node| node.get("id").and_then(Json::as_str) == Some(round))
        {
            return Err(Refusal::new(400, "Unknown round"));
        }
        let mut keys = vec![bench_key(id)];
        keys.extend(members.iter().cloned());
        let number = self
            .editing(
                keys,
                "Stop the running models before switching branches",
                async {
                    let followed = round.to_string();
                    let bench = self
                        .on_store(move |t| {
                            let mut bench = bench;
                            t.0.store.follow_bench_branch(&mut bench, &followed)?;
                            Ok::<_, Refusal>(bench)
                        })
                        .await?;
                    for member in &members {
                        self.0.services.reset_sandbox(member).await?;
                    }
                    Ok((0..bench.rounds.len())
                        .position(|i| bench.round_id(i) == Some(round))
                        .map_or(0, |i| i + 1))
                },
            )
            .await?;
        self.advance_bench(id).await;
        Ok(number)
    }

    /// `advanceBench(benchId)`: starts the next round of every model free
    /// for it; nothing once closed.
    pub async fn advance_bench(&self, id: &str) {
        if self.closed() {
            return;
        }
        let bench = id.to_string();
        if let Err(error) = self.on_store(move |t| t.start_rounds(&bench)).await {
            eprintln!("[bench] {id} could not be advanced: {error}");
        }
        self.0.services.conversations_changed();
    }

    fn start_rounds(&self, id: &str) -> Result<(), Refusal> {
        let store = &self.0.store;
        let Some(bench) = store.bench(id)? else {
            return Ok(());
        };
        for member in store.bench_members(id)? {
            let member_id = member.lock().expect("a chat's lock").id.0.clone();
            if self.held(&member_id) {
                continue;
            }
            let result = (|| -> Result<(), Refusal> {
                store.end_unfinished_turn(&mut member.lock().expect("a chat's lock"), UNSAVED)?;
                let progress = store.bench_progress(&member_id)?;
                if progress.round as usize >= bench.rounds.len() || progress.halted.is_some() {
                    return Ok(());
                }
                self.start_round(&bench, &member, progress.round as usize)
            })();
            if let Err(error) = result {
                eprintln!("[bench] {member_id} could not be advanced: {error}");
            }
        }
        Ok(())
    }

    /// `startRound(bench, member, index)`: a round that cannot start is
    /// stored as that model's failed round.
    fn start_round(
        &self,
        bench: &mcbench_store::Bench,
        member: &LiveConversation,
        index: usize,
    ) -> Result<(), Refusal> {
        let store = &self.0.store;
        let member_id = member.lock().expect("a chat's lock").id.0.clone();
        let hold = self
            .take(
                std::slice::from_ref(&member_id),
                true,
                "This model is already running",
            )?
            .remove(0);
        let round = bench.round_id(index).unwrap_or_default().to_string();
        let prompts = bench.round_prompts(index)?;
        let mut content = bench.round_content(index)?;
        let started = (|| -> Result<(), Error> {
            if let Content::Parts(parts) = &content {
                let mut shared = Vec::with_capacity(parts.len());
                for part in parts {
                    shared.push(match part {
                        Part::Attachment { attachment } => Part::Attachment {
                            attachment: store.share_attachment(attachment, &member_id)?,
                        },
                        other => other.clone(),
                    });
                }
                content = Content::Parts(shared);
            }
            self.0
                .services
                .sync_attachments(&member_id)
                .map_err(Error::plain)?;
            let mut conversation = member.lock().expect("a chat's lock");
            let checkpoint = conversation.world.reference();
            store.append_message(
                &mut conversation,
                NewMessage::bench_prompt(&round, prompts.as_deref(), &content),
                Some(checkpoint),
            )?;
            store.save(&mut conversation)
        })();
        if let Err(error) = started {
            eprintln!("[bench] round {} could not start: {error}", index + 1);
            let stored = (|| -> Result<(), Error> {
                let live = store
                    .conversation(&member_id)?
                    .ok_or_else(|| Error::plain(format!("Conversation {member_id} not found")))?;
                let mut conversation = live.lock().expect("a chat's lock");
                let checkpoint = conversation.world.reference();
                store.append_message(
                    &mut conversation,
                    NewMessage::bench_prompt(&round, prompts.as_deref(), &content),
                    Some(checkpoint),
                )?;
                store.end_turn(&mut conversation, NewMessage::error(&error.message, None))?;
                store.mark_finished(&member_id)
            })();
            self.release_turn(&member_id, &hold);
            return stored.map_err(Refusal::from);
        }
        let turns = self.clone();
        let task = tokio::spawn(async move {
            turns.execute_turn(&member_id, hold, &mut || {}).await;
        });
        self.0.turns.lock().expect("turns").push(task);
        Ok(())
    }

    // ----- The process -----

    /// What `startHttpServer` does first: the turns the last run stopped in
    /// the middle of end as errors, and every bench carries on.
    pub async fn start(&self) -> Result<Vec<String>, Refusal> {
        let store = Arc::clone(&self.0.store);
        let ended = blocking(move || store.interrupt_unfinished()).await?;
        for bench in self.on_store(|t| t.0.store.list_benches()).await? {
            self.advance_bench(&bench.id).await;
        }
        Ok(ended)
    }

    /// `shutdown`'s part: no turn, edit or round starts from now (503);
    /// every turn under way is stopped and the titles cancelled, and this
    /// resolves once all of them have ended.
    pub async fn shutdown(&self) {
        let turns: Vec<Arc<TurnStream>> = {
            let mut holds = self.0.holds.lock().expect("the holds' lock");
            holds.closed = true;
            holds
                .held
                .values()
                .filter_map(|hold| match hold {
                    Hold::Turn(turn) => Some(Arc::clone(turn)),
                    Hold::Edit(_) => None,
                })
                .collect()
        };
        for turn in turns {
            turn.stop();
        }
        self.0.titles.cancel.cancel();
        loop {
            let pending: Vec<Hold> = self
                .0
                .holds
                .lock()
                .expect("the holds' lock")
                .held
                .values()
                .cloned()
                .collect();
            if pending.is_empty() {
                break;
            }
            for hold in pending {
                hold.done().await;
            }
        }
        let tasks: Vec<JoinHandle<()>> =
            std::mem::take(&mut *self.0.titles.tasks.lock().expect("titles"));
        for task in tasks {
            let _ = task.await;
        }
        let tasks: Vec<JoinHandle<()>> = std::mem::take(&mut *self.0.turns.lock().expect("turns"));
        for task in tasks {
            let _ = task.await;
        }
    }

    /// Resolves once every title under way has ended (for tests and the
    /// core's activity).
    pub async fn titles_settled(&self) {
        loop {
            let tasks: Vec<JoinHandle<()>> =
                std::mem::take(&mut *self.0.titles.tasks.lock().expect("titles"));
            if tasks.is_empty() {
                return;
            }
            for task in tasks {
                let _ = task.await;
            }
        }
    }
}

#[cfg(test)]
#[path = "lifecycle_tests.rs"]
mod tests;
