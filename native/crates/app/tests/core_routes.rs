//! The route cases of today's tests whose subject is the server rather
//! than the lifecycle or the store (`ROUTES.md`, Tests), against the
//! core's methods and commands with a fake model, as today's tests drive
//! `handleApi` with one. Each test has its own data folder.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use serde_json::{Value, json};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

use mcbench_agent::lifecycle::{BenchRequest, ChatMessage, Outgoing};
use mcbench_lite::core::backend::Backend;
use mcbench_lite::core::{Core, Parts};
use mcbench_providers::{
    AssistantMessage, CompletionRequest, Delta, ModelFacts, ModelInfo, Outcome, Provider,
    ProviderError, ProviderId, Telemetry,
};
use mcbench_ui::{Command, Target, Update, Window};

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

/// The models: one that waits behind its gate, or answers `Done.` at
/// once; a title is `A title`.
#[derive(Default)]
struct Models {
    gates: Mutex<HashMap<String, watch::Receiver<bool>>>,
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
        Ok(ModelFacts {
            image_input: Some(true),
            ..ModelFacts::unknown()
        })
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

impl App {
    fn new(name: &str) -> App {
        let data =
            std::env::temp_dir().join(format!("mcbench-core-routes-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&data);
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
        let window = Arc::new(Recording::default());
        mcbench_ui::Core::attach(&core, window.clone());
        let app = App {
            core,
            models,
            window,
            data,
        };
        app.on(|core| async move { core.started().await });
        app
    }

    /// Runs `work` on the core's runtime and waits for it.
    fn on<T: Send + 'static, F: std::future::Future<Output = T> + Send + 'static>(
        &self,
        work: impl FnOnce(Core) -> F,
    ) -> T {
        let handle = self.core.handle().clone();
        handle
            .block_on(handle.spawn(work(self.core.clone())))
            .unwrap()
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

    fn bench(&self, blind: bool, models: [&str; 2]) -> (String, Vec<String>) {
        let models: Vec<String> = models.iter().map(|m| m.to_string()).collect();
        let id = self.on(move |core| async move {
            core.create_bench(BenchRequest {
                members: models
                    .into_iter()
                    .map(|m| ("openrouter".to_string(), m, None))
                    .collect(),
                blind,
                resolution: 1,
                message: Outgoing {
                    text: "A hut".into(),
                    ..Outgoing::default()
                },
            })
            .await
            .unwrap()
        });
        let members = self
            .core
            .store()
            .bench_members(&id)
            .unwrap()
            .iter()
            .map(|m| m.lock().unwrap().id.0.clone())
            .collect();
        (id, members)
    }

    fn updates(&self) -> Vec<Update> {
        self.window.0.lock().unwrap().clone()
    }
}

impl Drop for App {
    fn drop(&mut self) {
        self.core.shutdown();
        let _ = std::fs::remove_dir_all(&self.data);
    }
}

fn message(text: &str, edit_of: Option<String>) -> ChatMessage {
    ChatMessage {
        provider: "openrouter".into(),
        model: "own/a".into(),
        effort: None,
        resolution: 1,
        message: Outgoing {
            text: text.into(),
            ..Outgoing::default()
        },
        edit_of,
    }
}

fn until(what: &str, done: impl Fn() -> bool) {
    let started = Instant::now();
    while !done() {
        assert!(started.elapsed() < Duration::from_secs(60), "{what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// `server-turns.test.js`: a bench chat is changed only through its bench:
/// no chat-level prompt, edit, branch or archive.
#[test]
fn a_bench_chat_is_changed_only_through_its_bench() {
    let app = App::new("bench-chat");
    let (_, members) = app.bench(false, ["own/a", "own/b"]);
    for member in &members {
        app.free(member);
    }
    let chat = members[0].clone();
    let prompt = app
        .core
        .store()
        .conversation(&chat)
        .unwrap()
        .unwrap()
        .lock()
        .unwrap()
        .prompts()[0]
        .id
        .0
        .clone();
    let refused = app.on(move |core| async move {
        vec![
            core.chat(&chat, message("Add a door", None)).await.err(),
            core.chat(&chat, message("Add a door", Some(prompt.clone())))
                .await
                .err(),
            core.branch(&chat, &prompt).await.err(),
            core.archive_conversation(&chat).await.err(),
        ]
    });
    for refusal in refused {
        let refusal = refusal.expect("refused");
        assert_eq!(
            (refusal.status, refusal.message.as_str()),
            (409, "A bench chat is changed only through its bench")
        );
    }
    let chat = app.core.store().conversation(&members[0]).unwrap().unwrap();
    assert_eq!(
        chat.lock().unwrap().prompts().len(),
        1,
        "the model keeps its round"
    );
}

/// `server-turns.test.js`: a refused fork does not reveal a blind bench;
/// the fork made does.
#[test]
fn a_refused_fork_does_not_reveal_a_blind_bench_and_the_fork_made_does() {
    let app = App::new("fork");
    let gate = app.gate("blind/a");
    let (bench, members) = app.bench(true, ["blind/a", "blind/b"]);
    app.free(&members[1]);
    let revealed = |app: &App| app.core.store().bench(&bench).unwrap().unwrap().revealed;
    let a = members[0].clone();
    let (unknown, running) = app.on(move |core| async move {
        (
            core.fork(&a, &json!(9)).await.unwrap_err(),
            core.fork(&a, &json!(1)).await.unwrap_err(),
        )
    });
    assert_eq!(
        (unknown.status, unknown.message.as_str()),
        (400, "Unknown round")
    );
    assert_eq!(
        (running.status, running.message.as_str()),
        (409, "Wait for this round to finish before forking it")
    );
    assert!(!revealed(&app));
    gate.send_replace(true);
    app.free(&members[0]);
    let a = members[0].clone();
    let fork = app.on(move |core| async move { core.fork(&a, &json!(1)).await });
    let fork = fork.expect("the fork");
    assert!(revealed(&app));
    assert_ne!(fork.summary.id.0, members[0]);
    assert!(
        fork.summary.bench_id.is_none(),
        "the fork is a chat of its own"
    );
}

/// `server-turns.test.js`: a bench takes votes only while it hides its
/// models' names.
#[test]
fn a_bench_takes_votes_only_while_it_hides_its_models_names() {
    let app = App::new("votes");
    let (open, open_members) = app.bench(false, ["open/a", "open/b"]);
    for member in &open_members {
        app.free(member);
    }
    let round = |app: &App, bench: &str| {
        app.core.store().bench(bench).unwrap().unwrap().rounds[0]["id"].clone()
    };
    let (open_round, pick) = (round(&app, &open), json!(open_members[0]));
    let shown = app.on(move |core| async move { core.vote(&open, &open_round, &pick).await });
    let shown = shown.unwrap_err();
    assert_eq!(
        (shown.status, shown.message.as_str()),
        (409, "Votes are cast while the models' names are hidden")
    );
    let (blind, members) = app.bench(true, ["hid/a", "hid/b"]);
    for member in &members {
        app.free(member);
    }
    let blind_round = round(&app, &blind);
    let (b, r, a, other) = (
        blind.clone(),
        blind_round.clone(),
        json!(members[0]),
        json!(members[1]),
    );
    let after = app.on(move |core| async move {
        let cast = core.vote(&b, &r, &a).await;
        core.reveal(&b).await.unwrap();
        let mut after = Vec::new();
        for pick in [other, json!("tie"), Value::Null] {
            after.push(core.vote(&b, &r, &pick).await.unwrap_err().status);
        }
        (cast, after)
    });
    assert_eq!(
        after.0.unwrap()[blind_round.as_str().unwrap()],
        json!({ "pick": members[0], "blind": true })
    );
    assert_eq!(after.1, vec![409, 409, 409]);
    let stored = app.core.store().bench(&blind).unwrap().unwrap();
    assert_eq!(stored.votes[0].1.pick.as_deref(), Some(members[0].as_str()));
    assert!(stored.votes[0].1.blind);
}

/// `server-archive.test.js`: the chat list says how many chats and
/// benches are archived, after each archive, restore and delete, and the
/// list is published after each.
#[test]
fn the_chat_list_says_how_many_chats_and_benches_are_archived() {
    let app = App::new("archived");
    let counts =
        |app: &App| app.on(|core| async move { core.conversations().await.unwrap().archived });
    assert_eq!(counts(&app), (0, 0));
    let chat = app.on(|core| async move {
        let id = core
            .create_conversation("openrouter", "chat/model")
            .await
            .unwrap()
            .id;
        let stream = core.chat(&id, message("Build a shed", None)).await.unwrap();
        stream.done().await;
        id
    });
    app.free(&chat);
    let (bench, members) = app.bench(false, ["gate/a", "gate/b"]);
    for member in &members {
        app.free(member);
    }
    // The lists published from update `since` on: a list the turns above
    // publish as they close can still arrive after `free`, so a step's own
    // list is the one with its counts.
    let lists = |app: &App, since: usize| -> Vec<(usize, usize)> {
        app.updates()[since..]
            .iter()
            .filter_map(|u| match u {
                Update::Conversations(list) => Some(list.archived),
                _ => None,
            })
            .collect()
    };
    let steps: Vec<(&str, (usize, usize))> = vec![
        ("archive chat", (1, 0)),
        ("archive bench", (1, 1)),
        ("restore chat", (0, 1)),
        ("restore bench", (0, 0)),
        ("archive chat", (1, 0)),
        ("archive bench", (1, 1)),
        ("delete chat", (0, 1)),
        ("delete bench", (0, 0)),
    ];
    for (step, expected) in steps {
        let before = app.updates().len();
        let (chat, bench) = (chat.clone(), bench.clone());
        let step_name = step.to_string();
        app.on(move |core| async move {
            match step_name.as_str() {
                "archive chat" => core.archive_conversation(&chat).await,
                "archive bench" => core.archive_bench(&bench).await,
                "restore chat" => core.restore_conversation(&chat).await,
                "restore bench" => core.restore_bench(&bench).await,
                "delete chat" => core.delete_conversation(&chat).await,
                _ => core.delete_bench(&bench).await,
            }
            .unwrap()
        });
        assert_eq!(counts(&app), expected, "{step}");
        until(&format!("{step}: the list is published"), || {
            lists(&app, before).contains(&expected)
        });
    }
}

/// `provider-login.test.js`: `/api/activity` is busy while work is held
/// or a bench round is queued, idle otherwise.
#[test]
fn activity_is_busy_while_work_is_held_or_a_bench_round_is_queued() {
    let app = App::new("activity");
    let busy = |app: &App| app.on(|core| async move { core.busy().await.unwrap() });
    assert!(!busy(&app));
    // The message carries its own model (`message()` sends `own/a`), and a
    // turn asks for the model its message stored on the chat: that is the
    // model to hold. (The gate was on the chat's first model, so nothing
    // held the turn and it could end before `busy` was read.)
    let gate = app.gate("own/a");
    let chat = app.on(|core| async move {
        let id = core
            .create_conversation("openrouter", "chat/model")
            .await
            .unwrap()
            .id;
        core.chat(&id, message("Build", None)).await.unwrap();
        id
    });
    // A failure says what the chat holds.
    if !busy(&app) {
        let id = chat.clone();
        let messages = app.on(|core| async move {
            core.conversation(&id)
                .await
                .map(|transcript| transcript.messages)
        });
        panic!("a turn holds its chat; the chat's messages are {messages:?}");
    }
    gate.send_replace(true);
    app.free(&chat);
    assert!(!busy(&app));
    // A bench whose first round no model has started yet.
    let id = app.core.store().host().uuid();
    app.core
        .store()
        .create_bench(
            &id,
            mcbench_store::NewBench {
                title: "Queued".into(),
                blind: false,
                resolution: Some(1),
                content: mcbench_store::Content::Text("A well".into()),
                prompts: Vec::new(),
                members: ["queued/a", "queued/b"]
                    .map(|model| mcbench_store::BenchMember {
                        provider: "openrouter".into(),
                        model: model.into(),
                        effort: None,
                    })
                    .to_vec(),
            },
        )
        .unwrap();
    assert!(busy(&app));
    app.core.store().archive_bench(&id).unwrap();
    assert!(!busy(&app));
}

/// `resolution.test.js`: a message with an unknown resolution is refused
/// with 400 and changes nothing.
#[test]
fn a_message_with_an_unknown_resolution_is_refused() {
    let app = App::new("resolution");
    let (refused, prompts) = app.on(|core| async move {
        let id = core
            .create_conversation("openrouter", "chat/model")
            .await
            .unwrap()
            .id;
        let refused = core
            .chat(
                &id,
                ChatMessage {
                    resolution: 3,
                    ..message("Build", None)
                },
            )
            .await
            .unwrap_err();
        let prompts = core.conversation(&id).await.unwrap().messages.len();
        (refused, prompts)
    });
    assert_eq!(refused.status, 400);
    assert!(
        refused.message.starts_with("Resolution must be one of "),
        "{}",
        refused.message
    );
    assert_eq!(prompts, 0);
}

/// The commands: a message to a new chat publishes its transcript, its
/// turn's events in order and the transcript once it ended; opening a chat
/// publishes its transcript; a refusal is `Update::Error`.
#[test]
fn a_message_to_a_new_chat_publishes_its_transcript_and_its_turn() {
    let app = App::new("commands");
    mcbench_ui::Core::send(
        &app.core,
        Command::Send(mcbench_ui::Outgoing {
            target: Target::NewChat {
                provider: "openrouter".into(),
                model: "chat/model".into(),
            },
            text: "Build a hut".into(),
            attachments: Vec::new(),
            selections: Vec::new(),
            prompts: Vec::new(),
            effort: None,
            resolution: mcbench_ui::Resolution::Standard,
            edit_of: None,
        }),
    );
    until("the turn ends", || {
        let updates = app.updates();
        updates
            .iter()
            .filter(|u| matches!(u, Update::Transcript { .. }))
            .count()
            >= 2
    });
    let updates = app.updates();
    let turn: Vec<String> = updates
        .iter()
        .filter_map(|u| match u {
            Update::Turn { event, .. } => event
                .to_json()
                .map(|v| v["type"].as_str().unwrap().to_string())
                .or(Some("turn_snapshot".into())),
            Update::Transcript { messages, .. } => Some(format!("transcript {}", messages.len())),
            Update::TurnJoined { .. } => Some("joined".into()),
            _ => None,
        })
        .collect();
    assert_eq!(
        turn,
        [
            "joined",
            "transcript 1",
            "turn_snapshot",
            "assistant_delta",
            "assistant",
            "done",
            "transcript 2"
        ]
    );
    let chat = match updates.iter().find(|u| matches!(u, Update::Turn { .. })) {
        Some(Update::Turn { chat, .. }) => chat.clone(),
        _ => unreachable!(),
    };
    let before = app.updates().len();
    mcbench_ui::Core::send(&app.core, Command::Open { chat: chat.clone() });
    mcbench_ui::Core::send(
        &app.core,
        Command::Open {
            chat: "nobody".into(),
        },
    );
    // Other updates (the list, a title) can arrive among these two, so the
    // wait is for the two answers themselves, not for a count.
    let answered = |updates: &[Update]| {
        updates
            .iter()
            .any(|u| matches!(u, Update::Transcript { chat: c, .. } if *c == chat))
            && updates
                .iter()
                .any(|u| matches!(u, Update::TranscriptFailed { chat, .. } if chat == "nobody"))
    };
    until("both answered", || answered(&app.updates()[before..]));
    let after = &app.updates()[before..];
    assert!(after.iter().any(|u| matches!(u, Update::Transcript { chat: c, messages, .. } if *c == chat && messages.len() == 2)));
    assert!(
        after
            .iter()
            .any(|u| matches!(u, Update::TranscriptFailed { chat, error }
        if chat == "nobody" && error == "Conversation not found"))
    );
}
