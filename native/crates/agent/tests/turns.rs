//! `test/server-turns.test.js`, the cases whose subject is the turn
//! lifecycle, against [`Turns`] with a fake model, a fake `bash` and a fake
//! sandbox reset, as today's test drives `handleApi` with them. Each test
//! has its own data folder under the system's temporary folder.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde_json::{Map, Value, json};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

use mcbench_agent::context;
use mcbench_agent::contract::Placement;
use mcbench_agent::lifecycle::{BenchRequest, ChatMessage, Outgoing, Services, TurnConfig, Turns};
use mcbench_agent::{TurnEvent, TurnSink};
use mcbench_providers::{
    AssistantMessage, CompletionRequest, Delta, ModelFacts, ModelInfo, Outcome, Provider,
    ProviderError, ProviderId, Telemetry, ToolCall,
};
use mcbench_store::{
    BenchMember, Content, DEFAULT_TITLE, DataFolder, Limits, Message, NewBench, NewConversation,
    NewMessage, Options, PromptLink, Reply, Store, SystemHost,
};
use mcbench_tools::WorkFolders as SandboxWork;
use mcbench_tools::verify::Budgets;
use mcbench_tools::{Chat, Progress, Tool, ToolOutcome};

type Answer = Arc<
    dyn Fn(
            CompletionRequest,
            CancellationToken,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<AssistantMessage, ProviderError>> + Send>,
        > + Send
        + Sync,
>;

fn answer<F, Fut>(f: F) -> Answer
where
    F: Fn(CompletionRequest, CancellationToken) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = Result<AssistantMessage, ProviderError>> + Send + 'static,
{
    Arc::new(move |request, cancel| Box::pin(f(request, cancel)))
}

/// A gate a fake waits behind (`deferred()`).
#[derive(Clone)]
struct Gate(watch::Sender<bool>);

impl Gate {
    fn new() -> Gate {
        Gate(watch::channel(false).0)
    }
    fn open(&self) {
        self.0.send_replace(true);
    }
    async fn wait(&self) {
        let _ = self.0.subscribe().wait_for(|open| *open).await;
    }
}

fn telemetry(failed: Option<bool>) -> Telemetry {
    Telemetry {
        provider: ProviderId::OpenRouter,
        model: "m".into(),
        effort: None,
        started_at: "2026-11-17T00:00:00.000Z".into(),
        wall_ms: 1,
        first_delta_ms: None,
        context_bytes: 0,
        message_count: 0,
        usage: None,
        outcome: match failed {
            None => Outcome::Completed {
                context_tokens: None,
            },
            Some(cancelled) => Outcome::Failed { cancelled },
        },
    }
}

fn said(content: &str) -> AssistantMessage {
    AssistantMessage {
        content: content.into(),
        reasoning: None,
        reasoning_details: None,
        finish_reason: None,
        telemetry: telemetry(None),
        notices: None,
        plan: None,
        rate_limit: None,
        tool_calls: Vec::new(),
    }
}

fn calling(id: &str, name: &str, arguments: &str) -> AssistantMessage {
    AssistantMessage {
        tool_calls: vec![ToolCall {
            id: id.into(),
            kind: "function".into(),
            name: name.into(),
            arguments: arguments.into(),
            extra_content: None,
        }],
        ..said("")
    }
}

/// `waits(gate)`: answers once `gate` opens, or fails as a stopped request.
fn waits(gate: Gate) -> Answer {
    answer(move |_, cancel| {
        let gate = gate.clone();
        async move {
            tokio::select! {
                _ = gate.wait() => Ok(said("Done.")),
                _ = cancel.cancelled() => Err(stopped(String::new())),
            }
        }
    })
}

fn stopped(reply: String) -> ProviderError {
    ProviderError {
        message: "This operation was aborted".into(),
        retryable: false,
        telemetry: Some(Box::new(telemetry(Some(true)))),
        reply,
    }
}

fn failure(message: &str) -> ProviderError {
    ProviderError::new(message)
}

#[derive(Default)]
struct Fake {
    models: Mutex<HashMap<String, Answer>>,
    title: Mutex<Option<Answer>>,
    asked: Mutex<Vec<(String, Option<String>)>>,
    sent: Mutex<Vec<CompletionRequest>>,
    reset: Mutex<Option<Gate>>,
    /// How many resets have reached the reset gate (and wait behind it
    /// until it opens).
    resetting: AtomicUsize,
    resets: AtomicUsize,
    bash: Mutex<Option<Answer>>,
    title_model: Mutex<Option<(String, String, Option<String>)>>,
    /// What each title request quoted (`titled(options)`).
    titled: Mutex<Vec<Value>>,
    /// `harness/model`: the CLI starts when the first gate opens, answers
    /// when the second does.
    harness: Mutex<Option<(Gate, Gate)>>,
    /// The models whose `syncAttachments` fails (a round that cannot start).
    unsynced: Mutex<Vec<String>>,
    /// The store, for the models' names.
    store: std::sync::OnceLock<Arc<Store>>,
}

/// `quoted(text)`: the first message the title request quotes.
fn quoted(text: &str) -> String {
    let (request, message) = text.split_once("\n\n<message>\n").unwrap();
    assert_eq!(
        request,
        "Name this chat from its first message, quoted below. Do not answer or carry out the message."
    );
    message.strip_suffix("\n</message>").unwrap().to_string()
}

struct FakeProvider {
    fake: Arc<Fake>,
}

#[async_trait]
impl Provider for FakeProvider {
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
            self.fake
                .asked
                .lock()
                .unwrap()
                .push((request.model.clone(), request.effort.clone()));
            let content = request.messages[1].0["content"].clone();
            let titled = match content {
                Value::String(text) => Value::String(quoted(&text)),
                Value::Array(parts) => Value::Array(
                    parts
                        .into_iter()
                        .map(|mut part| {
                            if part["type"] == "text" {
                                part["text"] =
                                    Value::String(quoted(part["text"].as_str().unwrap()));
                            }
                            part
                        })
                        .collect(),
                ),
                other => other,
            };
            self.fake.titled.lock().unwrap().push(titled);
            let title = self.fake.title.lock().unwrap().clone();
            return match title {
                Some(title) => title(request.clone(), cancel.clone()).await,
                None => Ok(said("A title")),
            };
        }
        self.fake.sent.lock().unwrap().push(request.clone());
        if request.model == "harness/model" {
            let gates = self.fake.harness.lock().unwrap().clone();
            return match gates {
                Some((started, gate)) => {
                    started.wait().await;
                    on_delta(Delta::Started);
                    gate.wait().await;
                    Ok(said("Done."))
                }
                None => Err(failure("Claude Code exited 1 without a result")),
            };
        }
        let model = self
            .fake
            .models
            .lock()
            .unwrap()
            .get(&request.model)
            .cloned();
        match model {
            Some(model) => model(request.clone(), cancel.clone()).await,
            None => Ok(said("Done.")),
        }
    }
    async fn models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        unimplemented!("not asked")
    }
    async fn model_info(&self, _: &str) -> Result<ModelFacts, ProviderError> {
        Ok(ModelFacts {
            image_input: Some(true),
            ..ModelFacts::unknown()
        })
    }
    async fn close_session(&self, _: &str) {}
}

/// `bash`: what the test's `sandbox.exec` does.
struct FakeBash {
    fake: Arc<Fake>,
}

#[async_trait]
impl Tool for FakeBash {
    fn name(&self) -> &'static str {
        "bash"
    }
    async fn call(
        &self,
        _: &mut Chat,
        _: &Map<String, Value>,
        _: &mut (dyn FnMut(Progress) + Send),
        cancel: &CancellationToken,
    ) -> Result<ToolOutcome, mcbench_tools::ToolError> {
        let bash = self.fake.bash.lock().unwrap().clone();
        let result = match bash {
            None => json!({ "ok": true, "text": "$ ls\n" }),
            Some(bash) => match bash(
                CompletionRequest {
                    conversation_id: None,
                    model: String::new(),
                    effort: None,
                    messages: Vec::new(),
                    tools: Vec::new(),
                    chat: false,
                    transcript: None,
                },
                cancel.clone(),
            )
            .await
            {
                Ok(_) => json!({ "ok": true, "text": "done" }),
                Err(error) if cancel.is_cancelled() => {
                    json!({ "ok": false, "cancelled": true, "error": error.message })
                }
                Err(error) => json!({ "ok": false, "error": error.message }),
            },
        };
        Ok(ToolOutcome {
            result,
            screenshots: None,
            images: Vec::new(),
        })
    }
}

struct FakeServices {
    fake: Arc<Fake>,
}

#[async_trait]
impl Services for FakeServices {
    fn provider(&self, _: &str) -> Result<Arc<dyn Provider>, String> {
        Ok(Arc::new(FakeProvider {
            fake: Arc::clone(&self.fake),
        }))
    }
    fn tools(&self) -> mcbench_tools::Tools {
        mcbench_tools::Tools::of(vec![Arc::new(FakeBash {
            fake: Arc::clone(&self.fake),
        })])
    }
    fn title_model(&self) -> Option<(String, String, Option<String>)> {
        self.fake.title_model.lock().unwrap().clone()
    }
    fn sync_attachments(&self, chat: &str) -> Result<(), String> {
        // A chat's own prompt syncs with the chat locked: only a bench
        // round (which syncs before it locks) is looked up.
        if self.fake.unsynced.lock().unwrap().is_empty() {
            return Ok(());
        }
        let store = self.fake.store.get().expect("the store");
        let model = store
            .conversation(chat)
            .unwrap()
            .unwrap()
            .lock()
            .unwrap()
            .model
            .clone();
        if self.fake.unsynced.lock().unwrap().contains(&model) {
            return Err("This chat is out of storage".into());
        }
        Ok(())
    }
    async fn reset_sandbox(&self, _: &str) -> Result<(), String> {
        let gate = self.fake.reset.lock().unwrap().clone();
        if let Some(gate) = gate {
            self.fake.resetting.fetch_add(1, Ordering::SeqCst);
            gate.wait().await;
        }
        self.fake.resets.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    fn conversations_changed(&self) {}
}

#[derive(Clone, Debug, Default)]
struct Lines(Arc<Mutex<Vec<Value>>>);

impl TurnSink for Lines {
    fn emit(&mut self, event: TurnEvent) {
        if let Some(value) = event.to_json() {
            self.0.lock().unwrap().push(value);
        }
    }
}

impl Lines {
    fn last_type(&self) -> String {
        self.0.lock().unwrap().last().unwrap()["type"]
            .as_str()
            .unwrap()
            .to_string()
    }
    fn last(&self) -> Value {
        self.0.lock().unwrap().last().unwrap().clone()
    }
}

struct App {
    store: Arc<Store>,
    turns: Turns,
    fake: Arc<Fake>,
    root: PathBuf,
}

impl Drop for App {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn app(name: &str) -> App {
    let root = std::env::temp_dir().join(format!("mcbench-turns-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let store = Arc::new(
        Store::open(
            &DataFolder { root: root.clone() },
            Options {
                host: Arc::new(SystemHost),
                limits: Limits::from_env(|_| None).unwrap(),
                chat_removed: Box::new(|_| {}),
            },
        )
        .unwrap(),
    );
    let fake = Arc::new(Fake::default());
    assert!(fake.store.set(Arc::clone(&store)).is_ok());
    let turns = Turns::new(
        Arc::clone(&store),
        Arc::new(FakeServices {
            fake: Arc::clone(&fake),
        }),
        TurnConfig {
            placement: Placement::Updates,
            network_access: true,
            budgets: Budgets::DEFAULT,
            max_context_bytes: context::DEFAULT_MAX_BYTES,
            keep_exchanges: context::KEEP_EXCHANGES,
            work: Arc::new(SandboxWork {
                root: root.join("sandbox"),
            }),
        },
    );
    App {
        store,
        turns,
        fake,
        root,
    }
}

fn message(text: &str) -> ChatMessage {
    ChatMessage {
        provider: "openrouter".into(),
        model: "chat/model".into(),
        effort: None,
        resolution: 1,
        message: Outgoing {
            text: text.into(),
            ..Outgoing::default()
        },
        edit_of: None,
    }
}

impl App {
    fn new_chat(&self) -> String {
        let live = self
            .store
            .create_conversation("openrouter", "chat/model", NewConversation::default())
            .unwrap();
        live.lock().unwrap().id.0.clone()
    }

    fn model(&self, model: &str, answer: Answer) {
        self.fake
            .models
            .lock()
            .unwrap()
            .insert(model.into(), answer);
    }

    /// Sends `body` to `id` and waits for its turn to end.
    async fn chat(
        &self,
        id: &str,
        body: ChatMessage,
    ) -> Result<Lines, mcbench_agent::lifecycle::Refusal> {
        let lines = Lines::default();
        let stream = self.turns.send(id, body, Box::new(lines.clone())).await?;
        stream.done().await;
        Ok(lines)
    }

    /// Starts a turn without waiting for it.
    async fn start(&self, id: &str, body: ChatMessage) -> Lines {
        let lines = Lines::default();
        self.turns
            .send(id, body, Box::new(lines.clone()))
            .await
            .unwrap();
        lines
    }

    fn messages(&self, id: &str) -> Vec<Message> {
        let live = self.store.conversation(id).unwrap().unwrap();
        let conversation = live.lock().unwrap();
        conversation
            .messages
            .iter()
            .map(|m| m.message.clone())
            .collect()
    }

    fn roles(&self, id: &str) -> Vec<&'static str> {
        self.messages(id)
            .iter()
            .map(|m| match m {
                Message::User(_) => "user",
                Message::Assistant(_) => "assistant",
                Message::Tool(_) => "tool",
                Message::Error(_) => "error",
            })
            .collect()
    }

    fn prompts(&self, id: &str) -> Vec<(String, String)> {
        let live = self.store.conversation(id).unwrap().unwrap();
        let conversation = live.lock().unwrap();
        conversation
            .prompts()
            .iter()
            .map(|m| {
                (
                    m.id.0.clone(),
                    match &m.message {
                        Message::User(user) => user.content.text(),
                        _ => unreachable!(),
                    },
                )
            })
            .collect()
    }

    async fn new_bench(&self, text: &str, models: &[&str]) -> (String, Vec<String>) {
        let id = self
            .turns
            .create_bench(BenchRequest {
                members: models
                    .iter()
                    .map(|m| ("openrouter".to_string(), m.to_string(), None))
                    .collect(),
                blind: false,
                resolution: 1,
                message: Outgoing {
                    text: text.into(),
                    ..Outgoing::default()
                },
            })
            .await
            .unwrap();
        let mut ids = Vec::new();
        for model in models {
            let member = self
                .store
                .bench_members(&id)
                .unwrap()
                .into_iter()
                .find(|m| m.lock().unwrap().model == *model)
                .unwrap();
            ids.push(member.lock().unwrap().id.0.clone());
        }
        (id, ids)
    }

    async fn free(&self, ids: &[String]) {
        for id in ids {
            self.turns.free(id).await;
        }
    }

    /// While `work` runs, writes of `statement` fail as a full disk does.
    fn refuse(&self, statement: &str) -> rusqlite::Connection {
        let db = rusqlite::Connection::open(self.root.join("mcbench.sqlite3")).unwrap();
        db.execute_batch(&format!(
            "CREATE TRIGGER refuse_write BEFORE {statement} BEGIN SELECT RAISE(ABORT, 'the disk is full'); END"
        ))
        .unwrap();
        db
    }
}

fn allow(db: rusqlite::Connection) {
    db.execute_batch("DROP TRIGGER refuse_write").unwrap();
}

/// A while for what must NOT happen to have its chance (a check of an
/// absence); what must happen is waited for with [`until`].
async fn a_while() {
    for _ in 0..20 {
        tokio::task::yield_now().await;
    }
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
}

/// Waits until `done`, failing after a minute (a hang, not a slow machine).
async fn until(what: &str, done: impl Fn() -> bool) {
    let started = std::time::Instant::now();
    while !done() {
        assert!(
            started.elapsed() < std::time::Duration::from_secs(60),
            "{what}"
        );
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
}

impl App {
    /// Waits until `count` resets wait behind the reset gate.
    async fn resetting(&self, count: usize) {
        until(&format!("{count} resets reach the gate"), || {
            self.fake.resetting.load(Ordering::SeqCst) >= count
        })
        .await;
    }
}

fn last_assistant(app: &App, id: &str) -> mcbench_store::Assistant {
    match app.messages(id).pop().unwrap() {
        Message::Assistant(a) => a,
        other => panic!("not an assistant message: {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_stop_during_a_tool_is_recorded_as_a_stop_which_holds_the_models_bench_queue() {
    let app = app("stop-tool");
    let started = Gate::new();
    app.model(
        "stop/a",
        answer(|_, _| async { Ok(calling("wait", "bash", r#"{"command":"sleep 100"}"#)) }),
    );
    let gate = started.clone();
    *app.fake.bash.lock().unwrap() = Some(answer(move |_, cancel| {
        let gate = gate.clone();
        async move {
            gate.open();
            cancel.cancelled().await;
            Err(failure("killed"))
        }
    }));
    let (bench, ids) = app.new_bench("A hut", &["stop/a", "stop/b"]).await;
    started.wait().await;
    app.turns.stop(&ids[0]);
    app.free(&ids).await;
    let last = last_assistant(&app, &ids[0]);
    assert_eq!(
        (last.content.as_str(), last.stopped),
        ("(stopped by user)", true)
    );
    let progress = app.store.bench_progress(&ids[0]).unwrap();
    assert_eq!((progress.round, progress.halted), (1, Some("stopped")));
    app.turns
        .bench_round(
            &bench,
            Outgoing {
                text: "Add a door".into(),
                ..Outgoing::default()
            },
            None,
        )
        .await
        .unwrap();
    app.free(&ids).await;
    assert_eq!(
        app.prompts(&ids[0]).len(),
        1,
        "the stopped model does not take the next round"
    );
    assert_eq!(app.prompts(&ids[1]).len(), 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_chat_message_is_checked_whole_before_anything_changes_and_a_failed_save_leaves_nothing_in_memory()
 {
    let app = app("checked");
    let id = app.new_chat();
    let png = format!("data:image/png;base64,{}", "cG5n");
    let mut body = message("Build");
    body.provider = "nope".into();
    body.effort = Some("high".into());
    body.message.files = vec![
        mcbench_store::json::parse(&json!({ "name": "ref.png", "data": png }).to_string()).unwrap(),
    ];
    let refused = app.chat(&id, body).await.unwrap_err();
    assert_eq!(
        (refused.status, refused.message.as_str()),
        (400, "Choose a configured provider")
    );
    assert_eq!(
        app.store
            .conversation(&id)
            .unwrap()
            .unwrap()
            .lock()
            .unwrap()
            .effort,
        None
    );
    assert!(
        !app.root.join("attachments").join(&id).exists(),
        "no attachment is written for a refused message"
    );
    assert!(!app.turns.held(&id));

    let db = app.refuse(&format!(
        "INSERT ON messages WHEN NEW.conversation_id = '{id}'"
    ));
    let mut body = message("Build");
    body.effort = Some("high".into());
    let failed = app.chat(&id, body).await.unwrap_err();
    allow(db);
    assert_eq!(
        (failed.status, failed.message.as_str()),
        (500, "the disk is full")
    );
    assert!(
        app.messages(&id).is_empty(),
        "the unsaved message is not served"
    );
    assert_eq!(
        app.store
            .conversation(&id)
            .unwrap()
            .unwrap()
            .lock()
            .unwrap()
            .effort,
        None
    );
    let sent = app.chat(&id, message("Build")).await.unwrap();
    assert_eq!(sent.last_type(), "done");
    assert_eq!(app.roles(&id), ["user", "assistant"]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_edit_or_branch_switch_holds_the_chat_until_its_sandbox_is_reset_and_none_runs_under_a_turn()
 {
    let app = app("edit-holds");
    let id = app.new_chat();
    app.chat(&id, message("Build a hut")).await.unwrap();
    let first = app.prompts(&id)[0].0.clone();

    let gate = Gate::new();
    app.model("chat/model", waits(gate.clone()));
    let running = app.start(&id, message("Add a door")).await;
    let edit = ChatMessage {
        edit_of: Some(first.clone()),
        ..message("Build a tower")
    };
    assert_eq!(
        app.turns
            .send(&id, edit.clone(), Box::new(Lines::default()))
            .await
            .unwrap_err()
            .status,
        409
    );
    assert_eq!(app.turns.branch(&id, &first).await.unwrap_err().status, 409);
    gate.open();
    app.turns.free(&id).await;
    assert_eq!(running.last_type(), "done");

    let resetting = Gate::new();
    *app.fake.reset.lock().unwrap() = Some(resetting.clone());
    let asked = Arc::new(AtomicUsize::new(0));
    let count = Arc::clone(&asked);
    app.model(
        "chat/model",
        answer(move |_, _| {
            count.fetch_add(1, Ordering::SeqCst);
            async { Ok(said("Done.")) }
        }),
    );
    let turns = app.turns.clone();
    let lines = Lines::default();
    let sink = lines.clone();
    let chat = id.clone();
    let editing =
        tokio::spawn(async move { turns.send(&chat, edit, Box::new(sink)).await.map(drop) });
    app.resetting(1).await;
    assert_eq!(
        app.prompts(&id)
            .iter()
            .map(|p| p.1.as_str())
            .collect::<Vec<_>>(),
        ["Build a tower"],
        "the edit is stored before the reset"
    );
    assert_eq!(
        asked.load(Ordering::SeqCst),
        0,
        "its turn does not start on the shell being reset"
    );
    assert_eq!(
        app.turns
            .send(&id, message("Again"), Box::new(Lines::default()))
            .await
            .unwrap_err()
            .status,
        409,
        "no other turn starts on it either"
    );
    resetting.open();
    editing.await.unwrap().unwrap();
    app.turns.free(&id).await;
    assert_eq!(lines.last_type(), "done");
    assert_eq!(asked.load(Ordering::SeqCst), 1);
    let live = app.store.conversation(&id).unwrap().unwrap();
    let tree = app.store.message_tree(&live.lock().unwrap()).unwrap();
    let roots: Vec<_> = tree
        .messages
        .iter()
        .filter(|m| m.parent_id.is_none())
        .collect();
    assert!(roots.iter().any(|m| m.id == first));
    assert_eq!(roots.len(), 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_bench_retry_holds_the_model_until_its_sandbox_is_reset_even_when_another_model_finishes_meanwhile()
 {
    let app = app("bench-retry");
    app.model(
        "retry/a",
        answer(|_, _| async { Err(failure("HTTP 400: bad request")) }),
    );
    let gate = Gate::new();
    app.model("retry/b", waits(gate.clone()));
    let (bench, ids) = app.new_bench("A tower", &["retry/a", "retry/b"]).await;
    app.free(&ids[..1]).await;
    let progress = app.store.bench_progress(&ids[0]).unwrap();
    assert_eq!((progress.round, progress.halted), (1, Some("failed")));
    app.fake.models.lock().unwrap().remove("retry/a");

    let resetting = Gate::new();
    *app.fake.reset.lock().unwrap() = Some(resetting.clone());
    let turns = app.turns.clone();
    let (b, member) = (bench.clone(), ids[0].clone());
    let retry = tokio::spawn(async move { turns.retry_member(&b, &member).await });
    app.resetting(1).await;
    assert_eq!(
        app.turns
            .retry_member(&bench, &ids[0])
            .await
            .unwrap_err()
            .status,
        409
    );
    gate.open();
    app.free(&ids[1..]).await;
    assert!(
        app.turns.turn(&ids[0]).is_none(),
        "the reverted round does not start under the reset"
    );
    assert!(app.prompts(&ids[0]).is_empty());
    resetting.open();
    retry.await.unwrap().unwrap();
    *app.fake.reset.lock().unwrap() = None;
    app.free(&ids[..1]).await;
    assert_eq!(app.roles(&ids[0]), ["user", "assistant"]);
    let progress = app.store.bench_progress(&ids[0]).unwrap();
    assert_eq!((progress.round, progress.halted), (1, None));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_bench_round_edit_holds_the_bench_the_new_version_is_on_the_line_first_and_no_round_is_added_or_started_until_the_resets_end()
 {
    let app = app("round-edit");
    let (bench, ids) = app.new_bench("A tower", &["edit/a", "edit/b"]).await;
    app.free(&ids).await;
    let text = |t: &str| Outgoing {
        text: t.into(),
        ..Outgoing::default()
    };
    app.turns
        .bench_round(&bench, text("Add a roof"), None)
        .await
        .unwrap();
    app.free(&ids).await;
    let roof = app
        .store
        .bench(&bench)
        .unwrap()
        .unwrap()
        .round_id(1)
        .unwrap()
        .to_string();
    let unknown = app
        .turns
        .bench_round(&bench, text("Add a spire"), Some("nope".into()))
        .await
        .unwrap_err();
    assert_eq!(
        (unknown.status, unknown.message.as_str()),
        (400, "Unknown round")
    );
    let resetting = Gate::new();
    *app.fake.reset.lock().unwrap() = Some(resetting.clone());
    let turns = app.turns.clone();
    let b = bench.clone();
    let edit =
        tokio::spawn(async move { turns.bench_round(&b, text("Add a spire"), Some(roof)).await });
    app.resetting(1).await;
    let stored = app.store.bench(&bench).unwrap().unwrap();
    let contents: Vec<String> = (0..stored.rounds.len())
        .map(|i| stored.round_content(i).unwrap().text())
        .collect();
    assert_eq!(contents, ["A tower", "Add a spire"]);
    assert_eq!(
        app.turns
            .bench_round(&bench, text("Add a window"), None)
            .await
            .unwrap_err()
            .status,
        409
    );
    assert_eq!(app.store.bench(&bench).unwrap().unwrap().nodes.len(), 3);
    assert!(
        ids.iter().all(|id| app.turns.turn(id).is_none()),
        "no model starts the new version on the old shell"
    );
    resetting.open();
    assert_eq!(edit.await.unwrap().unwrap(), 2);
    *app.fake.reset.lock().unwrap() = None;
    app.free(&ids).await;
    for id in &ids {
        assert_eq!(
            app.prompts(id)
                .iter()
                .map(|p| p.1.as_str())
                .collect::<Vec<_>>(),
            ["A tower", "Add a spire"]
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_message_a_bench_and_a_round_store_the_library_prompts_they_were_sent_from() {
    let app = app("linked");
    let castle = PromptLink {
        id: "castle".into(),
        title: "Castle".into(),
    };
    let id = app.new_chat();
    let mut body = message("Build a castle");
    body.message.prompts = vec![castle.clone()];
    app.chat(&id, body).await.unwrap();
    app.chat(&id, message("Add a moat")).await.unwrap();
    let links: Vec<Vec<PromptLink>> = app
        .messages(&id)
        .iter()
        .filter(|m| m.is_prompt())
        .map(|m| m.prompts().to_vec())
        .collect();
    assert_eq!(
        links,
        [vec![castle.clone()], vec![]],
        "a message typed by hand records none"
    );

    let tower = PromptLink {
        id: "tower".into(),
        title: "Tower".into(),
    };
    let bench = app
        .turns
        .create_bench(BenchRequest {
            members: vec![
                ("openrouter".into(), "linked/a".into(), None),
                ("openrouter".into(), "linked/b".into(), None),
            ],
            blind: false,
            resolution: 1,
            message: Outgoing {
                text: "Build a castle".into(),
                prompts: vec![castle.clone()],
                ..Outgoing::default()
            },
        })
        .await
        .unwrap();
    let members: Vec<String> = app
        .store
        .bench_members(&bench)
        .unwrap()
        .iter()
        .map(|m| m.lock().unwrap().id.0.clone())
        .collect();
    app.free(&members).await;
    app.turns
        .bench_round(
            &bench,
            Outgoing {
                text: "Build a tall tower".into(),
                prompts: vec![tower.clone()],
                ..Outgoing::default()
            },
            None,
        )
        .await
        .unwrap();
    app.free(&members).await;
    let stored = app.store.bench(&bench).unwrap().unwrap();
    assert_eq!(
        (0..2)
            .map(|i| stored.round_prompts(i).unwrap().unwrap())
            .collect::<Vec<_>>(),
        [vec![castle.clone()], vec![tower.clone()]]
    );
    for member in &members {
        let links: Vec<Vec<PromptLink>> = app
            .messages(member)
            .iter()
            .filter(|m| m.is_prompt())
            .map(|m| m.prompts().to_vec())
            .collect();
        assert_eq!(links, [vec![castle.clone()], vec![tower.clone()]]);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_bench_whose_message_is_refused_leaves_nothing_saved() {
    let app = app("bench-refused");
    let before = app.store.list_benches().unwrap().len();
    let refused = app
        .turns
        .create_bench(BenchRequest {
            members: vec![
                ("openrouter".into(), "x/a".into(), None),
                ("openrouter".into(), "x/b".into(), None),
            ],
            blind: false,
            resolution: 1,
            message: Outgoing {
                text: "  ".into(),
                ..Outgoing::default()
            },
        })
        .await
        .unwrap_err();
    assert_eq!(
        (refused.status, refused.message.as_str()),
        (400, "Message text or an attachment is required")
    );
    assert_eq!(app.store.list_benches().unwrap().len(), before);
}

fn crashed_bench(app: &App, title: &str) -> (String, String, String) {
    let id = mcbench_store::Host::uuid(&SystemHost);
    let (bench, members) = app
        .store
        .create_bench(
            &id,
            NewBench {
                title: title.into(),
                blind: false,
                resolution: None,
                content: Content::Text("A well".into()),
                prompts: Vec::new(),
                members: ["a", "b"]
                    .iter()
                    .map(|m| BenchMember {
                        provider: "openrouter".into(),
                        model: format!("{title}/{m}"),
                        effort: None,
                    })
                    .collect(),
            },
        )
        .unwrap();
    let crashed = &members[0];
    let mut conversation = crashed.lock().unwrap();
    let world = conversation.world.reference();
    app.store
        .append_message(
            &mut conversation,
            NewMessage::bench_prompt(
                bench.round_id(0).unwrap(),
                None,
                &Content::Text("A well".into()),
            ),
            Some(world),
        )
        .unwrap();
    app.store
        .append_message(
            &mut conversation,
            NewMessage::reply(&Reply {
                tool_calls: vec![mcbench_store::ToolCall {
                    id: "lost".into(),
                    kind: "function".into(),
                    function: mcbench_store::FunctionCall {
                        name: "bash".into(),
                        arguments: "{}".into(),
                    },
                    extra_content: None,
                }],
                ..Reply::default()
            }),
            None,
        )
        .unwrap();
    app.store.save(&mut conversation).unwrap();
    let queued = members[1].lock().unwrap().id.0.clone();
    (bench.id, conversation.id.0.clone(), queued)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn after_a_restart_a_crashed_model_reads_as_failed_and_can_be_retried_and_queued_rounds_start()
 {
    let app = app("restart");
    let (bench, crashed, queued) = crashed_bench(&app, "crash");
    let ended = app.turns.start().await.unwrap();
    assert!(ended.contains(&crashed));
    assert!(
        !app.turns.held(&crashed),
        "the crashed model is held for a retry"
    );
    app.turns.free(&queued).await;
    assert_eq!(
        app.roles(&queued),
        ["user", "assistant"],
        "the queued round started"
    );
    let progress = app.store.bench_progress(&crashed).unwrap();
    assert_eq!((progress.round, progress.halted), (1, Some("failed")));
    app.turns.retry_member(&bench, &crashed).await.unwrap();
    app.turns.free(&crashed).await;
    assert_eq!(app.roles(&crashed), ["user", "assistant"]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_bench_model_whose_turn_end_was_not_saved_holds_its_queue_as_failed_and_can_be_retried() {
    let app = app("unsaved");
    let (bench, unsaved, queued) = crashed_bench(&app, "unsaved");
    app.turns.advance_bench(&bench).await;
    app.turns.free(&queued).await;
    assert!(!app.turns.held(&unsaved));
    assert_eq!(app.roles(&unsaved), ["user", "assistant", "tool", "error"]);
    let progress = app.store.bench_progress(&unsaved).unwrap();
    assert_eq!((progress.round, progress.halted), (1, Some("failed")));
    app.turns.retry_member(&bench, &unsaved).await.unwrap();
    app.turns.free(&unsaved).await;
    assert_eq!(app.roles(&unsaved), ["user", "assistant"]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_turn_stopped_while_its_reply_streams_keeps_the_text_the_user_saw_which_no_model_request_carries()
 {
    let app = app("stop-reply");
    let id = app.new_chat();
    let entered = Gate::new();
    let e = entered.clone();
    app.model(
        "chat/model",
        answer(move |_, cancel| {
            let e = e.clone();
            async move {
                e.open();
                cancel.cancelled().await;
                Err(stopped("First the floor, then".into()))
            }
        }),
    );
    let turn = app.start(&id, message("Plan a hut")).await;
    entered.wait().await;
    app.turns.stop(&id);
    app.turns.free(&id).await;
    assert_eq!(turn.last_type(), "done");
    let last = last_assistant(&app, &id);
    assert_eq!(
        (last.content.as_str(), last.stopped, last.reply.as_deref()),
        ("(stopped by user)", true, Some("First the floor, then"))
    );
    app.fake.models.lock().unwrap().remove("chat/model");
    app.fake.sent.lock().unwrap().clear();
    app.chat(&id, message("continue")).await.unwrap();
    let sent = app.fake.sent.lock().unwrap();
    assert!(sent[0].messages.iter().all(|m| !m.0.contains_key("reply")));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stopping_a_turn_does_not_stop_its_title_and_a_chat_still_untitled_gets_one_on_its_next_turn()
 {
    let app = app("title-stop");
    let id = app.new_chat();
    let named = Gate::new();
    let gate = named.clone();
    *app.fake.title.lock().unwrap() = Some(answer(move |_, _| {
        let gate = gate.clone();
        async move {
            gate.wait().await;
            Ok(said("Stone hut"))
        }
    }));
    app.model("chat/model", waits(Gate::new()));
    let turn = app.start(&id, message("Build a hut")).await;
    until("the title is asked", || {
        !app.fake.titled.lock().unwrap().is_empty()
    })
    .await;
    app.turns.stop(&id);
    app.turns.free(&id).await;
    assert_eq!(turn.last_type(), "done");
    assert!(app.turns.busy(), "a title under way is activity");
    named.open();
    app.turns.titles_settled().await;
    assert!(!app.turns.busy(), "nothing is under way");
    assert_eq!(
        app.store
            .conversation(&id)
            .unwrap()
            .unwrap()
            .lock()
            .unwrap()
            .title,
        "Stone hut"
    );

    let other = app.new_chat();
    *app.fake.title.lock().unwrap() =
        Some(answer(|_, _| async { Err(failure("HTTP 400: no title")) }));
    app.fake.models.lock().unwrap().remove("chat/model");
    app.chat(&other, message("Build a tower")).await.unwrap();
    app.turns.titles_settled().await;
    assert_eq!(
        app.store
            .conversation(&other)
            .unwrap()
            .unwrap()
            .lock()
            .unwrap()
            .title,
        DEFAULT_TITLE
    );
    match app.messages(&other).pop().unwrap() {
        Message::Error(error) => {
            assert_eq!(error.content, "Title generation failed: HTTP 400: no title")
        }
        other => panic!("{other:?}"),
    }
    *app.fake.title.lock().unwrap() = Some(answer(|_, _| async { Ok(said("Tall tower")) }));
    app.chat(&other, message("Make it taller")).await.unwrap();
    app.turns.titles_settled().await;
    assert_eq!(
        app.store
            .conversation(&other)
            .unwrap()
            .unwrap()
            .lock()
            .unwrap()
            .title,
        "Tall tower"
    );
}

// `executeTurn`'s `finally` releases the turn however it ends: a turn
// whose task panics (fatal in the core) gives its hold back, so the
// shutdown after it waits for nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_turn_that_panics_gives_its_hold_back_and_the_shutdown_does_not_wait_for_it() {
    let app = app("turn-panic");
    let id = app.new_chat();
    app.model(
        "chat/model",
        answer(|_, _| async { panic!("a model that panics, on purpose") }),
    );
    let _turn = app.start(&id, message("Build a hut")).await;
    tokio::time::timeout(std::time::Duration::from_secs(60), app.turns.free(&id))
        .await
        .expect("the panicked turn's hold is given back");
    assert!(!app.turns.held(&id));
    tokio::time::timeout(std::time::Duration::from_secs(60), app.turns.shutdown())
        .await
        .expect("the shutdown waits for no panicked turn");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_bench_chat_is_changed_only_through_its_bench() {
    let app = app("own");
    let (bench, ids) = app.new_bench("A hut", &["own/a", "own/b"]).await;
    app.free(&ids).await;
    let prompt = app.prompts(&ids[0])[0].0.clone();
    let mut body = message("Add a door");
    body.model = "own/a".into();
    let refused = [
        app.turns
            .send(&ids[0], body.clone(), Box::new(Lines::default()))
            .await
            .unwrap_err(),
        app.turns
            .send(
                &ids[0],
                ChatMessage {
                    edit_of: Some(prompt.clone()),
                    ..body
                },
                Box::new(Lines::default()),
            )
            .await
            .unwrap_err(),
        app.turns.branch(&ids[0], &prompt).await.unwrap_err(),
    ];
    for refusal in refused {
        assert_eq!(
            (refusal.status, refusal.message.as_str()),
            (409, "A bench chat is changed only through its bench")
        );
    }
    assert_eq!(app.prompts(&ids[0]).len(), 1, "the model keeps its round");
    assert_eq!(
        app.store.bench_members(&bench).unwrap().len(),
        2,
        "and its place in the bench"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_turn_whose_end_could_not_be_saved_is_ended_before_its_chat_takes_the_next_prompt() {
    let app = app("end-unsaved");
    let id = app.new_chat();
    let running = Gate::new();
    let lost = Gate::new();
    app.model(
        "chat/model",
        answer(|_, _| async { Ok(calling("lost", "bash", r#"{"command":"make"}"#)) }),
    );
    let (r, l) = (running.clone(), lost.clone());
    *app.fake.bash.lock().unwrap() = Some(answer(move |_, _| {
        let (r, l) = (r.clone(), l.clone());
        async move {
            r.open();
            l.wait().await;
            Err(failure("the sandbox is gone"))
        }
    }));
    let turn = app.start(&id, message("Build a hut")).await;
    running.wait().await;
    let db = app.refuse(&format!(
        "INSERT ON messages WHEN NEW.conversation_id = '{id}'"
    ));
    lost.open();
    app.turns.free(&id).await;
    allow(db);
    let last = turn.last();
    assert!(
        last["message"]
            .as_str()
            .unwrap()
            .contains("saving this failed: the disk is full"),
        "{last}"
    );
    assert!(!app.turns.held(&id));
    assert_eq!(
        app.roles(&id),
        ["user", "assistant"],
        "the stored turn is under way"
    );
    let model = Arc::new(Mutex::new(None));
    let seen = Arc::clone(&model);
    app.model(
        "chat/model",
        answer(move |request, _| {
            *seen.lock().unwrap() = Some(request);
            async { Ok(said("Done.")) }
        }),
    );
    let next = app.chat(&id, message("Add a door")).await.unwrap();
    assert_eq!(next.last_type(), "done");
    assert_eq!(
        app.roles(&id),
        ["user", "assistant", "tool", "error", "user", "assistant"]
    );
    let sent = model.lock().unwrap().clone().unwrap();
    let calls: Vec<Value> = sent
        .messages
        .iter()
        .flat_map(|m| {
            m.0.get("tool_calls")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default()
        })
        .map(|c| c["id"].clone())
        .collect();
    let results: Vec<Value> = sent
        .messages
        .iter()
        .filter(|m| m.0["role"] == "tool")
        .map(|m| m.0["tool_call_id"].clone())
        .collect();
    assert_eq!(results, calls, "the model is sent a result for every call");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_title_that_arrives_for_a_reverted_first_prompt_is_dropped_and_the_new_first_prompt_is_titled()
 {
    let app = app("title-revert");
    let id = app.new_chat();
    let slow = Gate::new();
    let gate = slow.clone();
    *app.fake.title.lock().unwrap() = Some(answer(move |request, _| {
        let gate = gate.clone();
        async move {
            let asked = request.messages[1].0["content"]
                .as_str()
                .unwrap()
                .to_string();
            if asked.contains("\n<message>\nBuild a hut\n</message>") {
                gate.wait().await;
                Ok(said("Stone hut"))
            } else {
                Ok(said("Tall tower"))
            }
        }
    }));
    app.chat(&id, message("Build a hut")).await.unwrap();
    let hut = app.prompts(&id)[0].0.clone();
    app.chat(
        &id,
        ChatMessage {
            edit_of: Some(hut),
            ..message("Build a tower")
        },
    )
    .await
    .unwrap();
    until("the new first prompt is titled", || {
        chat_title(&app, &id) == "Tall tower"
    })
    .await;
    let before = app
        .store
        .conversation(&id)
        .unwrap()
        .unwrap()
        .lock()
        .unwrap()
        .title
        .clone();
    slow.open();
    app.turns.titles_settled().await;
    assert_eq!(
        before, "Tall tower",
        "the new first prompt is titled while the old title is under way"
    );
    assert_eq!(
        app.store
            .conversation(&id)
            .unwrap()
            .unwrap()
            .lock()
            .unwrap()
            .title,
        "Tall tower"
    );
    *app.fake.title.lock().unwrap() = Some(answer(|_, _| async { Ok(said("A door")) }));
    app.chat(&id, message("Add a door")).await.unwrap();
    app.turns.titles_settled().await;
    assert_eq!(
        app.store
            .conversation(&id)
            .unwrap()
            .unwrap()
            .lock()
            .unwrap()
            .title,
        "Tall tower"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_error_is_not_dismissed_under_a_running_turn() {
    let app = app("dismiss");
    let id = app.new_chat();
    app.model(
        "chat/model",
        answer(|_, _| async { Err(failure("HTTP 400: bad request")) }),
    );
    app.chat(&id, message("Build a hut")).await.unwrap();
    let error = {
        let live = app.store.conversation(&id).unwrap().unwrap();
        let conversation = live.lock().unwrap();
        let last = conversation.messages.last().unwrap();
        assert!(matches!(last.message, Message::Error(_)));
        last.id.0.clone()
    };
    let gate = Gate::new();
    app.model("chat/model", waits(gate.clone()));
    let turn = app.start(&id, message("Again")).await;
    let db = app.refuse(&format!("UPDATE ON conversations WHEN NEW.id = '{id}'"));
    let under = app.turns.dismiss_error(&id, &error).await.unwrap_err();
    allow(db);
    gate.open();
    app.turns.free(&id).await;
    assert_eq!(turn.last_type(), "done", "the turn is saved");
    assert_eq!(
        (under.status, under.message.as_str()),
        (409, "Wait for the running turn before dismissing an error")
    );
    app.turns.dismiss_error(&id, &error).await.unwrap();
    let dismissed = app.messages(&id).into_iter().find_map(|m| match m {
        Message::Error(e) => Some(e.dismissed),
        _ => None,
    });
    assert_eq!(dismissed, Some(true));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn with_no_title_model_chosen_a_chat_is_titled_by_its_own_model_and_a_bench_by_its_first_model_at_low_effort()
 {
    let app = app("title-model");
    let id = app.new_chat();
    app.chat(
        &id,
        ChatMessage {
            effort: Some("high".into()),
            ..message("Build a hut")
        },
    )
    .await
    .unwrap();
    app.turns.titles_settled().await;
    let (_, ids) = app
        .new_bench("A tower", &["first/model", "second/model"])
        .await;
    app.free(&ids).await;
    app.turns.titles_settled().await;
    *app.fake.title_model.lock().unwrap() = Some((
        "openrouter".into(),
        "title/model".into(),
        Some("low".into()),
    ));
    let other = app.new_chat();
    app.chat(&other, message("Build a well")).await.unwrap();
    app.turns.titles_settled().await;
    let asked = app.fake.asked.lock().unwrap().clone();
    let low = Some("low".to_string());
    assert_eq!(
        asked,
        [
            ("chat/model".to_string(), low.clone()),
            ("first/model".into(), low.clone()),
            ("title/model".into(), low)
        ]
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn shutdown_refuses_new_turns_and_rounds_and_waits_for_every_turn_under_way() {
    let app = app("shutdown");
    let gate = Gate::new();
    let entered = Gate::new();
    let (g, e) = (gate.clone(), entered.clone());
    app.model(
        "down/a",
        answer(move |_, _| {
            let (g, e) = (g.clone(), e.clone());
            async move {
                e.open();
                g.wait().await;
                Err(stopped(String::new()))
            }
        }),
    );
    let (bench, ids) = app.new_bench("A hut", &["down/a", "down/b"]).await;
    app.free(&ids[1..]).await;
    entered.wait().await;
    let id = app.new_chat();
    let turns = app.turns.clone();
    let down = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = Arc::clone(&down);
    let stopping = tokio::spawn(async move {
        turns.shutdown().await;
        flag.store(true, Ordering::SeqCst);
    });
    until("the shutdown has begun", || app.turns.closed()).await;
    let refused = app
        .turns
        .send(&id, message("Build"), Box::new(Lines::default()))
        .await
        .unwrap_err();
    assert_eq!(
        (refused.status, refused.message.as_str()),
        (503, "The server is shutting down")
    );
    a_while().await;
    assert!(
        !down.load(Ordering::SeqCst),
        "nothing ends before the turn under way is saved"
    );
    assert!(app.turns.held(&ids[0]));
    gate.open();
    stopping.await.unwrap();
    assert!(
        !app.turns.held(&ids[0]),
        "the running turn ended before shutdown did"
    );
    assert!(
        last_assistant(&app, &ids[0]).stopped,
        "the stopped turn was recorded"
    );
    app.turns.advance_bench(&bench).await;
    assert!(
        ids.iter().all(|id| !app.turns.held(id)),
        "no round starts after"
    );
}

// ----- `agent-retry.test.js` (2) and `turn-lifecycle.test.js` (2 of 3) -----

/// A turn of chat `id` run directly (`runTurn`), with the events it emits.
async fn run(
    app: &App,
    id: &str,
    cancel: &CancellationToken,
) -> (Result<(), mcbench_agent::TurnError>, Vec<Value>) {
    let lines = Lines::default();
    let sink: mcbench_agent::turn::SharedSink = Arc::new(Mutex::new(lines.clone()));
    let setup = mcbench_agent::TurnSetup {
        store: Arc::clone(&app.store),
        conversation: app.store.conversation(id).unwrap().unwrap(),
        provider: Arc::new(FakeProvider {
            fake: Arc::clone(&app.fake),
        }),
        tools: mcbench_tools::Tools::of(vec![Arc::new(FakeBash {
            fake: Arc::clone(&app.fake),
        })]),
        chat: true,
        placement: Placement::Updates,
        network_access: true,
        budgets: Budgets::DEFAULT,
        max_context_bytes: context::DEFAULT_MAX_BYTES,
        keep_exchanges: context::KEEP_EXCHANGES,
        archives: Arc::new(context::Archives::default()),
        work: Arc::new(SandboxWork {
            root: app.root.join("sandbox"),
        }),
    };
    let result = mcbench_agent::run_turn(&setup, &sink, cancel, &mut || {}).await;
    let events = lines.0.lock().unwrap().clone();
    (result, events)
}

fn prompted(app: &App, provider: &str) -> String {
    let live = app
        .store
        .create_conversation(provider, "fixture", NewConversation::default())
        .unwrap();
    let mut conversation = live.lock().unwrap();
    app.store
        .append_message(
            &mut conversation,
            NewMessage::prompt(&Content::Text("go".into()), &[]),
            None,
        )
        .unwrap();
    app.store.save(&mut conversation).unwrap();
    conversation.id.0.clone()
}

/// A model that fails as an overloaded API does once, then answers.
fn overloaded_once(app: &App) -> Arc<AtomicUsize> {
    let calls = Arc::new(AtomicUsize::new(0));
    let count = Arc::clone(&calls);
    app.model("fixture", answer(move |_, _| {
        let n = count.fetch_add(1, Ordering::SeqCst);
        async move {
            if n == 0 {
                Err(failure(r#"Claude Code success: API Error: 529 {"type":"error","error":{"type":"overloaded_error"}}"#))
            } else {
                Ok(said("Done."))
            }
        }
    }));
    calls
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_harness_providers_completion_runs_once_its_cli_already_retried() {
    let app = app("harness-once");
    for provider in ["claude-code", "codex-cli", "cursor"] {
        let id = prompted(&app, provider);
        let calls = overloaded_once(&app);
        let (result, events) = run(&app, &id, &CancellationToken::new()).await;
        assert!(result.unwrap_err().message.contains("overloaded_error"));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(!events.iter().any(|e| e["type"] == "completion_retry"));
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_model_api_providers_completion_is_retried_after_a_transient_error() {
    let app = app("api-retry");
    let id = prompted(&app, "anthropic");
    let calls = overloaded_once(&app);
    let (result, events) = run(&app, &id, &CancellationToken::new()).await;
    result.unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(
        events
            .iter()
            .filter(|e| e["type"] == "completion_retry")
            .count(),
        1
    );
    match app
        .store
        .conversation(&id)
        .unwrap()
        .unwrap()
        .lock()
        .unwrap()
        .messages
        .last()
        .unwrap()
        .message
        .clone()
    {
        Message::Assistant(a) => assert_eq!(a.content, "Done."),
        other => panic!("{other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_stop_during_a_tool_throws_the_abort_and_the_calls_after_it_do_nothing() {
    let app = app("stop-calls");
    let id = prompted(&app, "openrouter");
    app.model(
        "fixture",
        answer(|_, _| async {
            Ok(AssistantMessage {
                tool_calls: vec![
                    ToolCall {
                        id: "wait".into(),
                        kind: "function".into(),
                        name: "bash".into(),
                        arguments: r#"{"command":"sleep 100"}"#.into(),
                        extra_content: None,
                    },
                    ToolCall {
                        id: "after".into(),
                        kind: "function".into(),
                        name: "prompt_library".into(),
                        arguments: r#"{"save":[{"title":"Added after the stop","text":"x"}]}"#
                            .into(),
                        extra_content: None,
                    },
                ],
                ..said("")
            })
        }),
    );
    let cancel = CancellationToken::new();
    let stop = cancel.clone();
    *app.fake.bash.lock().unwrap() = Some(answer(move |_, cancel| {
        let stop = stop.clone();
        async move {
            stop.cancel();
            cancel.cancelled().await;
            Err(failure("killed"))
        }
    }));
    let (result, _) = run(&app, &id, &cancel).await;
    assert_eq!(result.unwrap_err().message, "This operation was aborted");
    let results: Vec<(String, Value)> = app
        .messages(&id)
        .into_iter()
        .filter_map(|m| match m {
            Message::Tool(t) => Some((
                t.tool_call_id,
                serde_json::from_str::<Value>(&t.content).unwrap()["cancelled"].clone(),
            )),
            _ => None,
        })
        .collect();
    assert_eq!(
        results,
        [
            ("wait".to_string(), Value::Bool(true)),
            ("after".to_string(), Value::Bool(true))
        ]
    );
    assert!(
        !app.messages(&id)
            .iter()
            .any(|m| matches!(m, Message::Assistant(a) if a.content == "(stopped by user)"))
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_stopped_turn_answers_the_calls_it_left_as_cancelled() {
    let app = app("stopped-calls");
    let id = prompted(&app, "openrouter");
    let live = app.store.conversation(&id).unwrap().unwrap();
    let mut conversation = live.lock().unwrap();
    let call = |id: &str| mcbench_store::ToolCall {
        id: id.into(),
        kind: "function".into(),
        function: mcbench_store::FunctionCall {
            name: "bash".into(),
            arguments: "{}".into(),
        },
        extra_content: None,
    };
    app.store
        .append_message(
            &mut conversation,
            NewMessage::reply(&Reply {
                tool_calls: vec![call("done"), call("left")],
                ..Reply::default()
            }),
            None,
        )
        .unwrap();
    app.store
        .append_message(
            &mut conversation,
            NewMessage::tool_result(
                "done",
                &mcbench_store::ToolTelemetry {
                    name: "bash".into(),
                    started_at: String::new(),
                    wall_ms: 0,
                    ok: true,
                    cancelled: false,
                },
                "ok",
            ),
            None,
        )
        .unwrap();
    app.store
        .end_turn(
            &mut conversation,
            NewMessage::stopped("(stopped by user)", None, None),
        )
        .unwrap();
    let tail: Vec<(String, Option<Value>)> = conversation.messages[2..]
        .iter()
        .map(|m| match &m.message {
            Message::Tool(t) if t.content != "ok" => (
                t.tool_call_id.clone(),
                Some(serde_json::from_str::<Value>(&t.content).unwrap()["cancelled"].clone()),
            ),
            Message::Tool(t) => (t.tool_call_id.clone(), None),
            _ => ("assistant".into(), None),
        })
        .collect();
    assert_eq!(
        tail,
        [
            ("done".to_string(), None),
            ("left".to_string(), Some(Value::Bool(true))),
            ("assistant".to_string(), None)
        ]
    );
}

// ----- the cases of `server-turns.test.js` added since -----

const PNG: &str = "data:image/png;base64,cG5n";

fn png_file(name: &str) -> mcbench_store::json::Json {
    mcbench_store::json::parse(&json!({ "name": name, "data": PNG }).to_string()).unwrap()
}

fn titles(app: &App) -> Vec<Value> {
    app.fake.titled.lock().unwrap().clone()
}

fn bench_title(app: &App, bench: &str) -> String {
    app.store.bench(bench).unwrap().unwrap().title
}

fn chat_title(app: &App, id: &str) -> String {
    app.store
        .conversation(id)
        .unwrap()
        .unwrap()
        .lock()
        .unwrap()
        .title
        .clone()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_harness_chats_title_starts_once_its_cli_has_started_and_a_turn_that_ends_first_is_titled_too()
 {
    let app = app("harness-title");
    let id = app.new_chat();
    *app.fake.title.lock().unwrap() = Some(answer(|_, _| async { Ok(said("Stone hut")) }));
    let (started, gate) = (Gate::new(), Gate::new());
    *app.fake.harness.lock().unwrap() = Some((started.clone(), gate.clone()));
    let body = ChatMessage {
        provider: "claude-code".into(),
        model: "harness/model".into(),
        ..message("Build a hut")
    };
    let _turn = app.start(&id, body).await;
    a_while().await;
    assert!(titles(&app).is_empty(), "no title while the CLI starts");
    started.open();
    until("the title is asked once the CLI started", || {
        !titles(&app).is_empty()
    })
    .await;
    assert_eq!(titles(&app), [json!("Build a hut")]);
    gate.open();
    app.turns.free(&id).await;
    app.turns.titles_settled().await;
    assert_eq!(chat_title(&app, &id), "Stone hut");

    // A CLI that never starts: the title comes with the turn's end.
    let other = app.new_chat();
    *app.fake.harness.lock().unwrap() = None;
    let body = ChatMessage {
        provider: "claude-code".into(),
        model: "harness/model".into(),
        ..message("Build a tower")
    };
    app.chat(&other, body).await.unwrap();
    app.turns.free(&other).await;
    app.turns.titles_settled().await;
    assert_eq!(titles(&app), [json!("Build a hut"), json!("Build a tower")]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_bench_round_that_cannot_start_is_stored_as_that_models_failed_round_which_a_retry_runs_again()
 {
    // Today's test fails `shareAttachment` for one model; here the same
    // step of `startRound`, its `syncAttachments`, fails for that model.
    let app = app("round-cannot-start");
    app.fake.unsynced.lock().unwrap().push("start/a".into());
    let bench = app
        .turns
        .create_bench(BenchRequest {
            members: ["start/a", "start/b", "start/c"]
                .iter()
                .map(|m| ("openrouter".to_string(), m.to_string(), None))
                .collect(),
            blind: true,
            resolution: 1,
            message: Outgoing {
                text: "A hut".into(),
                files: vec![png_file("notes.png")],
                ..Outgoing::default()
            },
        })
        .await
        .unwrap();
    let ids: Vec<String> = ["start/a", "start/b", "start/c"]
        .iter()
        .map(|model| {
            app.store
                .bench_members(&bench)
                .unwrap()
                .into_iter()
                .find(|m| m.lock().unwrap().model == *model)
                .unwrap()
                .lock()
                .unwrap()
                .id
                .0
                .clone()
        })
        .collect();
    app.free(&ids).await;
    assert_eq!(app.roles(&ids[0]), ["user", "error"]);
    let round = app
        .store
        .bench(&bench)
        .unwrap()
        .unwrap()
        .round_id(0)
        .unwrap()
        .to_string();
    match &app.messages(&ids[0])[..] {
        [Message::User(user), Message::Error(error)] => {
            assert_eq!(
                (user.round.as_deref(), error.content.as_str()),
                (Some(round.as_str()), "This chat is out of storage")
            );
        }
        other => panic!("{other:?}"),
    }
    let progress = app.store.bench_progress(&ids[0]).unwrap();
    assert_eq!((progress.round, progress.halted), (1, Some("failed")));
    app.fake.unsynced.lock().unwrap().clear();
    app.turns.retry_member(&bench, &ids[0]).await.unwrap();
    app.free(&ids[..1]).await;
    assert_eq!(app.roles(&ids[0]), ["user", "assistant"]);
    let progress = app.store.bench_progress(&ids[0]).unwrap();
    assert_eq!((progress.round, progress.halted), (1, None));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_bench_whose_title_failed_is_titled_from_its_first_round_when_it_is_sent_its_next_round()
{
    let app = app("bench-title-failed");
    *app.fake.title.lock().unwrap() =
        Some(answer(|_, _| async { Err(failure("HTTP 400: no title")) }));
    let bench = app
        .turns
        .create_bench(BenchRequest {
            members: vec![
                ("openrouter".into(), "title/a".into(), None),
                ("openrouter".into(), "title/b".into(), None),
            ],
            blind: false,
            resolution: 1,
            message: Outgoing {
                text: "A hut".into(),
                files: vec![png_file("ref.png")],
                ..Outgoing::default()
            },
        })
        .await
        .unwrap();
    let ids: Vec<String> = app
        .store
        .bench_members(&bench)
        .unwrap()
        .iter()
        .map(|m| m.lock().unwrap().id.0.clone())
        .collect();
    app.free(&ids).await;
    app.turns.titles_settled().await;
    assert_eq!(bench_title(&app, &bench), "New bench");
    *app.fake.title.lock().unwrap() = Some(answer(|_, _| async { Ok(said("Two huts")) }));
    app.fake.titled.lock().unwrap().clear();
    app.turns
        .bench_round(
            &bench,
            Outgoing {
                text: "Add a door".into(),
                ..Outgoing::default()
            },
            None,
        )
        .await
        .unwrap();
    app.free(&ids).await;
    app.turns.titles_settled().await;
    assert_eq!(bench_title(&app, &bench), "Two huts");
    assert_eq!(
        titles(&app),
        [
            json!([{ "type": "text", "text": "A hut" }, { "type": "image_url", "image_url": { "url": PNG } }])
        ],
        "the first round names the bench, with its image"
    );
    assert_eq!(
        ids.iter()
            .map(|id| chat_title(&app, id))
            .collect::<Vec<_>>(),
        ["Two huts", "Two huts"]
    );
    *app.fake.title.lock().unwrap() = Some(answer(|_, _| async { Ok(said("Doors")) }));
    app.turns
        .bench_round(
            &bench,
            Outgoing {
                text: "Add a window".into(),
                ..Outgoing::default()
            },
            None,
        )
        .await
        .unwrap();
    app.free(&ids).await;
    app.turns.titles_settled().await;
    assert_eq!(bench_title(&app, &bench), "Two huts");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_bench_title_that_arrives_for_a_first_round_since_edited_is_dropped_and_the_new_first_round_is_titled()
 {
    let app = app("bench-retitle");
    let slow = Gate::new();
    let gate = slow.clone();
    *app.fake.title.lock().unwrap() = Some(answer(move |request, _| {
        let gate = gate.clone();
        async move {
            let asked = quoted(request.messages[1].0["content"].as_str().unwrap());
            if asked == "A hut" {
                gate.wait().await;
                Ok(said("Stone hut"))
            } else {
                Ok(said("Tall tower"))
            }
        }
    }));
    let (bench, ids) = app.new_bench("A hut", &["retitle/a", "retitle/b"]).await;
    app.free(&ids).await;
    let first = app
        .store
        .bench(&bench)
        .unwrap()
        .unwrap()
        .round_id(0)
        .unwrap()
        .to_string();
    app.turns
        .bench_round(
            &bench,
            Outgoing {
                text: "A tower".into(),
                ..Outgoing::default()
            },
            Some(first),
        )
        .await
        .unwrap();
    app.free(&ids).await;
    until("the new first round is titled", || {
        bench_title(&app, &bench) == "Tall tower"
    })
    .await;
    let before = bench_title(&app, &bench);
    slow.open();
    app.turns.titles_settled().await;
    assert_eq!(before, "Tall tower");
    assert_eq!(bench_title(&app, &bench), "Tall tower");
}
