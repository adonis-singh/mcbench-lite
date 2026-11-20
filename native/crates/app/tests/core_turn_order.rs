//! The order the window relies on for a turn's updates (`src/core/turns.rs`,
//! agreed with the interface's owner 2026-11-17), through the core with a
//! scripted model whose turn emits a numbered sequence of events, one step
//! at a time: a `JoinTurn` at each position of the turn gives, from its
//! `TurnJoined` on, the transcript at the turn's snapshot, then every event
//! once and in order (the replay, then the live ones), then the turn's end:
//! `Done`, the chat's transcript with no running turn, then lists in which
//! it runs none; a join after the end is `TurnNotRunning`. The same for a
//! bench round's turn (no request started it) and for two chats' turns
//! interleaved.

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
use mcbench_ui::{Command, Outgoing, Resolution, Target, TurnEvent, Update, Window};

/// How many numbered events a scripted turn emits between its snapshot and
/// its answer.
const STEPS: usize = 4;

/// The step after the last numbered event: the turn answers.
const ALL: usize = STEPS + 1;

fn said(content: &str) -> AssistantMessage {
    AssistantMessage {
        content: content.into(),
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
    }
}

/// The scripted model: a turn of model `m` emits `e1` .. `eSTEPS` as text
/// deltas, each once `m`'s step counter allows it, then answers once it
/// reaches [`ALL`].
#[derive(Default)]
struct Script {
    steps: Mutex<std::collections::HashMap<String, watch::Receiver<usize>>>,
}

#[async_trait]
impl Provider for Script {
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
        let mut steps = self.steps.lock().unwrap()[&request.model].clone();
        for i in 1..=STEPS {
            tokio::select! {
                _ = steps.wait_for(|allowed| *allowed >= i) => {}
                _ = cancel.cancelled() => return Err(ProviderError::new("This operation was aborted")),
            }
            on_delta(Delta::Content(format!("e{i}")));
        }
        tokio::select! {
            _ = steps.wait_for(|allowed| *allowed >= ALL) => {}
            _ = cancel.cancelled() => return Err(ProviderError::new("This operation was aborted")),
        }
        Ok(said("All done."))
    }
    async fn models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        Ok(Vec::new())
    }
    async fn model_info(&self, _: &str) -> Result<ModelFacts, ProviderError> {
        Ok(ModelFacts::unknown())
    }
    async fn close_session(&self, _: &str) {}
}

struct Fake(Arc<Script>);

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
    script: Arc<Script>,
    window: Arc<Recording>,
    data: std::path::PathBuf,
}

impl App {
    fn new(name: &str) -> App {
        let data = std::env::temp_dir().join(format!(
            "mcbench-core-turn-order-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&data);
        let script = Arc::new(Script::default());
        let core = Core::start(Parts {
            data: data.clone(),
            program: std::env::current_exe().unwrap(),
            host: Arc::new(mcbench_store::SystemHost),
            shipped: Box::new(|| Err("unused".into())),
            client: mcbench_setup::fetch::client(),
            backend: Some(Arc::new(Fake(Arc::clone(&script)))),
            installer: None,
            share: None,
            live: Default::default(),
        })
        .unwrap();
        let window = Arc::new(Recording::default());
        mcbench_ui::Core::attach(&core, window.clone());
        let app = App {
            core,
            script,
            window,
            data,
        };
        app.on(|core| async move { core.started().await });
        app
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

    /// A step counter for model `model`.
    fn script(&self, model: &str) -> watch::Sender<usize> {
        let (allow, steps) = watch::channel(0);
        self.script
            .steps
            .lock()
            .unwrap()
            .insert(model.into(), steps);
        allow
    }

    fn updates(&self) -> Vec<Update> {
        self.window.0.lock().unwrap().clone()
    }

    /// Sends a new chat's first message to `model`; its chat once joined.
    fn send(&self, model: &str) -> String {
        let from = self.updates().len();
        self.on({
            let model = model.to_string();
            move |core| async move {
                core.command(Command::Send(Outgoing {
                    target: Target::NewChat {
                        provider: "openrouter".into(),
                        model,
                    },
                    text: "Build a hut".into(),
                    attachments: Vec::new(),
                    selections: Vec::new(),
                    prompts: Vec::new(),
                    effort: None,
                    resolution: Resolution::Standard,
                    edit_of: None,
                }))
                .await
            }
        });
        let joined = || {
            self.updates()[from..].iter().find_map(|u| match u {
                Update::TurnJoined { chat, .. } => Some(chat.clone()),
                _ => None,
            })
        };
        until("the send's turn is joined", || joined().is_some());
        joined().unwrap()
    }

    /// Waits until the window has `count` turn events of `chat`.
    fn events_at_least(&self, chat: &str, count: usize) {
        until(&format!("{count} events of {chat}"), || {
            self.updates()
                .iter()
                .filter(|u| matches!(u, Update::Turn { chat: c, .. } if c == chat))
                .count()
                >= count
        });
    }

    fn join(&self, chat: &str) -> usize {
        let from = self.updates().len();
        let chat = chat.to_string();
        self.on(|core| async move { core.command(Command::JoinTurn { chat }).await });
        from
    }

    /// Waits until `chat`'s turn has ended, and a list after its end's
    /// transcript says so.
    fn ended(&self, chat: &str) {
        let id = chat.to_string();
        self.on(move |core| async move { core.turns().free(&id).await });
        until("a list after the turn's end", || {
            let view = chat_view(&self.updates(), chat);
            view.iter()
                .position(|v| v == "transcript 2 idle")
                .is_some_and(|end| view[end..].iter().any(|v| v == "list idle"))
        });
    }
}

impl Drop for App {
    fn drop(&mut self) {
        self.core.shutdown();
        let _ = std::fs::remove_dir_all(&self.data);
    }
}

fn until(what: &str, done: impl Fn() -> bool) {
    let started = Instant::now();
    while !done() {
        assert!(started.elapsed() < Duration::from_secs(20), "{what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// The turn's events as names: `snapshot`, `e1` .. `eN`, `answer`, `done`.
fn event_name(event: &TurnEvent) -> String {
    match event {
        TurnEvent::Snapshot => "snapshot".into(),
        TurnEvent::AssistantDelta(text) => text.clone(),
        TurnEvent::Assistant { .. } => "answer".into(),
        TurnEvent::Done => "done".into(),
        other => format!("{other:?}"),
    }
}

/// The whole turn as its events are named.
fn whole_turn() -> Vec<String> {
    let mut turn = vec!["snapshot".to_string()];
    turn.extend((1..=STEPS).map(|i| format!("e{i}")));
    turn.extend(["answer".to_string(), "done".to_string()]);
    turn
}

/// `chat`'s updates from `from` on as names (`joined`, `transcript N`
/// with its message count and whether it says a turn runs, each event's
/// name, `list running|idle` for this chat's row of each list), and the
/// position of the first `joined`.
fn chat_view(updates: &[Update], chat: &str) -> Vec<String> {
    updates
        .iter()
        .filter_map(|u| match u {
            Update::TurnJoined { chat: c, .. } if c == chat => Some("joined".into()),
            Update::Transcript {
                chat: c,
                messages,
                header,
                ..
            } if c == chat => Some(format!(
                "transcript {} {}",
                messages.len(),
                if header.running_since.is_some() {
                    "running"
                } else {
                    "idle"
                }
            )),
            Update::Turn { chat: c, event } if c == chat => Some(event_name(event)),
            Update::Conversations(list) => list
                .conversations
                .iter()
                .find(|row| row.summary.id.0 == chat)
                .map(|row| {
                    if row.running_since.is_some() {
                        "list running".into()
                    } else {
                        "list idle".into()
                    }
                }),
            _ => None,
        })
        .collect()
}

/// Checks that `view` (a chat's updates from a join's marker on) is the
/// marker, the snapshot's transcript, the whole turn once in order, then
/// the end's transcript with no turn running and lists after it that say
/// none runs; a list before the end says the turn runs.
fn assert_joined(view: &[String], position: &str) {
    // Lists before the marker are the list's own (the send's row lists
    // its turn before the turn's first event).
    let marker = view
        .iter()
        .position(|v| v == "joined")
        .unwrap_or_else(|| panic!("{position}: no marker: {view:?}"));
    assert!(
        view[..marker].iter().all(|v| v.starts_with("list ")),
        "{position}: {view:?}"
    );
    let view = &view[marker..];
    let without_lists: Vec<&str> = view
        .iter()
        .filter(|v| !v.starts_with("list "))
        .map(String::as_str)
        .collect();
    let mut expected = vec!["joined", "transcript 1 running"];
    let turn = whole_turn();
    expected.extend(turn.iter().map(String::as_str));
    expected.push("transcript 2 idle");
    assert_eq!(without_lists, expected, "{position}: {view:?}");
    let end = view.iter().position(|v| v == "transcript 2 idle").unwrap();
    for (i, item) in view.iter().enumerate() {
        match item.as_str() {
            "list running" => assert!(
                i < end,
                "{position}: a running list after the end: {view:?}"
            ),
            "list idle" => assert!(i > end, "{position}: an idle list before the end: {view:?}"),
            _ => {}
        }
    }
    assert!(
        view[end + 1..].iter().any(|v| v == "list idle"),
        "{position}: a list after the end: {view:?}"
    );
}

/// A join after the turn's snapshot, after each numbered event, and after
/// the end, of a turn the window's own send started; and one made at once
/// after the send (before the turn's first event, or after the snapshot,
/// as the runtime has it: both are checked against the same rule).
#[test]
fn a_join_at_each_position_of_a_sends_turn_gives_the_snapshot_then_every_event_once() {
    let app = App::new("send");
    // The send's own view: the marker, the transcript, every event once.
    let allow = app.script("own/whole");
    let chat = app.send("own/whole");
    allow.send_replace(ALL);
    app.ended(&chat);
    assert_joined(&chat_view(&app.updates(), &chat), "the send itself");
    // Joins at each position: after the snapshot (0 events numbered), after
    // e1 .. eSTEPS.
    for position in 0..=STEPS {
        let model = format!("own/at-{position}");
        let allow = app.script(&model);
        let chat = app.send(&model);
        allow.send_replace(position);
        app.events_at_least(&chat, 1 + position);
        let from = app.join(&chat);
        allow.send_replace(ALL);
        app.ended(&chat);
        let view = chat_view(&app.updates()[from..], &chat);
        assert_joined(&view, &format!("after e{position}"));
        // After the end.
        let from = app.join(&chat);
        let after = app.updates()[from..]
            .iter()
            .filter(|u| !matches!(u, Update::Conversations(_)))
            .cloned()
            .collect::<Vec<_>>();
        assert!(
            matches!(&after[..], [Update::TurnNotRunning { chat: c }] if *c == chat),
            "after the end: {after:?}"
        );
    }
}

/// A join made in the same task right after the send was taken, before
/// the turn's first event when the runtime has not run the turn yet: said
/// with that event, followed by the whole turn once.
#[test]
fn a_join_right_after_the_send_gives_the_whole_turn_once() {
    let app = App::new("at-once");
    let mut before_first = 0;
    for attempt in 0..20 {
        let model = format!("own/once-{attempt}");
        let allow = app.script(&model);
        let from = app.updates().len();
        let early = app.on({
            let model = model.clone();
            move |core| async move {
                core.command(Command::Send(Outgoing {
                    target: Target::NewChat {
                        provider: "openrouter".into(),
                        model,
                    },
                    text: "Build a hut".into(),
                    attachments: Vec::new(),
                    selections: Vec::new(),
                    prompts: Vec::new(),
                    effort: None,
                    resolution: Resolution::Standard,
                    edit_of: None,
                }))
                .await;
                // The chat the send made is the newest row.
                let chat = core.conversations().await.unwrap().conversations[0]
                    .summary
                    .id
                    .0
                    .clone();
                core.command(Command::JoinTurn { chat: chat.clone() }).await;
                chat
            }
        });
        let chat = early;
        allow.send_replace(ALL);
        app.ended(&chat);
        let view = chat_view(&app.updates()[from..], &chat);
        // The send's own marker and the join's: a join before the turn's
        // first event is said with that event, one marker for both; after
        // it, its own marker and the replay.
        let markers = view.iter().filter(|v| *v == "joined").count();
        assert!(markers == 1 || markers == 2, "{view:?}");
        if markers == 1 {
            before_first += 1;
        }
        let last = view.iter().rposition(|v| v == "joined").unwrap();
        assert_joined(&view[last..], &format!("attempt {attempt}"));
        if markers == 2 {
            // The events before the join's marker are a prefix of the turn.
            let first: Vec<&str> = view[..last]
                .iter()
                .filter(|v| {
                    !v.starts_with("list ") && !v.starts_with("transcript") && *v != "joined"
                })
                .map(String::as_str)
                .collect();
            let turn = whole_turn();
            assert_eq!(
                first,
                turn[..first.len()]
                    .iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>()
            );
        }
    }
    println!("joins before the turn's first event: {before_first} of 20");
}

/// A bench round's turn (no request started it): a join gives the same.
#[test]
fn a_join_of_a_bench_rounds_turn_gives_the_snapshot_then_every_event_once() {
    let app = App::new("bench");
    let allow_a = app.script("bench/a");
    let allow_b = app.script("bench/b");
    let bench = app.on(|core| async move {
        core.create_bench(BenchRequest {
            members: ["bench/a", "bench/b"]
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
    let members: Vec<String> = app
        .core
        .store()
        .bench_members(&bench)
        .unwrap()
        .iter()
        .map(|m| m.lock().unwrap().id.0.clone())
        .collect();
    let chat = members[0].clone();
    allow_a.send_replace(2);
    app.events_at_least(&chat, 3);
    let from = app.join(&chat);
    allow_a.send_replace(ALL);
    allow_b.send_replace(ALL);
    app.ended(&chat);
    app.ended(&members[1]);
    assert_joined(&chat_view(&app.updates()[from..], &chat), "a bench round");
    // The other member's events reached the window once each, in order,
    // with no marker (no one joined it).
    let other: Vec<String> = chat_view(&app.updates(), &members[1])
        .into_iter()
        .filter(|v| !v.starts_with("list ") && !v.starts_with("transcript"))
        .collect();
    assert_eq!(other, whole_turn());
}

/// Two chats' turns interleaved, step by step: a join of one in the middle
/// neither drops nor repeats nor reorders the other's events.
#[test]
fn a_join_of_one_chat_leaves_another_chats_turn_as_it_was() {
    let app = App::new("two");
    let allow_a = app.script("two/a");
    let allow_b = app.script("two/b");
    let a = app.send("two/a");
    let b = app.send("two/b");
    let mut join_from = None;
    for step in 1..=STEPS {
        allow_a.send_replace(step);
        app.events_at_least(&a, 1 + step);
        allow_b.send_replace(step);
        app.events_at_least(&b, 1 + step);
        if step == STEPS / 2 {
            join_from = Some(app.join(&a));
        }
    }
    allow_a.send_replace(ALL);
    allow_b.send_replace(ALL);
    app.ended(&a);
    app.ended(&b);
    let updates = app.updates();
    assert_joined(&chat_view(&updates[join_from.unwrap()..], &a), "chat a");
    // B from its send's marker: the whole turn once, in order.
    assert_joined(&chat_view(&updates, &b), "chat b");
}
