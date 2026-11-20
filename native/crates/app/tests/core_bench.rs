//! Bench and the prompt library through the core (`Core::command` and
//! `mcbench_ui::Core::send`), each with a recording `Window` and a fake
//! model, no network: every command of `ui-seam-bench-prompts.md` on
//! success and on real refusals, checked against the endings
//! `src/core/commands.rs` states, and the details the core publishes
//! unasked (`src/core/bench.rs`).
//!
//! The model answers `Done.` at once, or waits behind its gate, or fails
//! (a model named `fail/...`); a request with no tools is a title (`A
//! title`) or, for a model named `gen/...`, an improve (`gen/ok` answers,
//! `gen/slow` waits until it is cancelled). A test never sleeps to let
//! something happen: it blocks on the window's condition variable until
//! what it waits for has arrived, and it asserts the order of the updates
//! that must be there and the state at the end, never how many arrived
//! where timing decides that. Each test has its own data folder in
//! `TMPDIR`, removed when it ends.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

use mcbench_lite::core::backend::Backend;
use mcbench_lite::core::{Core, Parts};
use mcbench_providers::{
    AssistantMessage, CompletionRequest, Delta, ModelFacts, ModelInfo, Outcome, Provider,
    ProviderError, ProviderId, Telemetry, Usage,
};
use mcbench_store::json::Json;
use mcbench_ui::{
    BenchChoice, BenchDetails, Command, Generated, LibraryPrompt, MessageId, Outgoing,
    OutgoingFile, PromptLink, Resolution, Selection, Subject, Target, Update, VotePick, Window,
};

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
            usage: Some(Usage {
                input_tokens: Some(12.0),
                output_tokens: Some(3.0),
                cached_input_tokens: None,
                cache_write_tokens: None,
                reasoning_tokens: None,
            }),
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

/// The models (see the module's doc).
#[derive(Default)]
struct Models {
    gates: Mutex<HashMap<String, watch::Receiver<bool>>>,
    /// `gen/slow` calls started, and how many saw their cancel; told on
    /// each change.
    slow: Mutex<(usize, usize)>,
    slow_changed: Condvar,
}

impl Models {
    fn slow_count(&self, change: impl FnOnce(&mut (usize, usize))) {
        change(&mut self.slow.lock().unwrap());
        self.slow_changed.notify_all();
    }

    /// Blocks until `started` slow calls have started (a minute at most: a hang, not a slow machine).
    fn wait_started(&self, started: usize) {
        let slow = self.slow.lock().unwrap();
        let (slow, timeout) = self
            .slow_changed
            .wait_timeout_while(slow, Duration::from_secs(60), |s| s.0 < started)
            .unwrap();
        drop(slow);
        assert!(!timeout.timed_out(), "{started} slow calls did not start");
    }
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
            return match request.model.as_str() {
                "gen/ok" => Ok(said("  A better prompt  ")),
                "gen/slow" => {
                    self.slow_count(|s| s.0 += 1);
                    cancel.cancelled().await;
                    self.slow_count(|s| s.1 += 1);
                    Err(ProviderError::new("This operation was aborted"))
                }
                _ => Ok(said("A title")),
            };
        }
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

/// The window: every update, and a condition variable the test waits on.
#[derive(Default)]
struct Recording {
    updates: Mutex<Vec<Update>>,
    arrived: Condvar,
}

impl Window for Recording {
    fn publish(&self, update: Update) {
        self.updates.lock().unwrap().push(update);
        self.arrived.notify_all();
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
            std::env::temp_dir().join(format!("mcbench-core-bench-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&data);
        std::fs::create_dir_all(&data).unwrap();
        // A saved key, so OpenRouter is configured for an improve.
        std::fs::write(
            data.join("settings.json"),
            r#"{"providers":{"openrouter":{"apiKey":"synthetic-test-key","enabled":true}}}"#,
        )
        .unwrap();
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
        let app = App {
            core,
            models,
            window,
            data,
        };
        app.on(|core| async move { core.started().await });
        mcbench_ui::Core::attach(&app.core, app.window.clone());
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

    fn len(&self) -> usize {
        self.window.updates.lock().unwrap().len()
    }

    fn all(&self) -> Vec<Update> {
        self.window.updates.lock().unwrap().clone()
    }

    /// Runs `command` to its end; every update from before it on.
    fn run(&self, command: Command) -> Vec<Update> {
        let from = self.len();
        self.on(move |core| async move { core.command(command).await });
        self.all()[from..].to_vec()
    }

    /// Sends `command` as the window does (`mcbench_ui::Core::send`).
    fn send(&self, command: Command) {
        mcbench_ui::Core::send(&self.core, command);
    }

    /// Blocks until `done` holds of the updates from `from` on (a minute at
    /// most); those updates.
    fn wait(&self, from: usize, what: &str, done: impl Fn(&[Update]) -> bool) -> Vec<Update> {
        let updates = self.window.updates.lock().unwrap();
        let (updates, timeout) = self
            .window
            .arrived
            .wait_timeout_while(updates, Duration::from_secs(60), |u| {
                !done(&u[from.min(u.len())..])
            })
            .unwrap();
        assert!(
            !timeout.timed_out(),
            "{what}: {:?}",
            kinds(&updates[from.min(updates.len())..])
        );
        updates[from..].to_vec()
    }

    fn members(&self, bench: &str) -> Vec<String> {
        self.core
            .store()
            .bench_members(bench)
            .unwrap()
            .iter()
            .map(|m| m.lock().unwrap().id.0.clone())
            .collect()
    }

    /// Waits until every model of `bench` has ended its turn and its
    /// titles are made.
    fn settle(&self, bench: &str) {
        let members = self.members(bench);
        self.on(move |core| async move {
            for member in members {
                core.turns().free(&member).await;
            }
            core.turns().titles_settled().await;
        });
    }

    /// The bench's details read now.
    fn details(&self, bench: &str) -> BenchDetails {
        let bench = bench.to_string();
        self.on(move |core| async move { core.bench(&bench).await.unwrap() })
    }

    /// A new bench of `models` sent from the window, its id once the core
    /// says it is sent.
    fn new_bench(&self, models: &[&str], blind: bool) -> String {
        let from = self.len();
        self.send(Command::Send(bench_message(
            Target::NewBench {
                members: models.iter().map(|m| choice(m)).collect(),
                blind,
            },
            "A hut",
        )));
        let updates = self.wait(from, "the bench sent", |u| {
            u.iter().any(|u| matches!(u, Update::BenchSent { .. }))
        });
        updates
            .iter()
            .find_map(|u| match u {
                Update::BenchSent { bench, .. } => Some(bench.clone()),
                _ => None,
            })
            .unwrap()
    }
}

impl Drop for App {
    fn drop(&mut self) {
        self.core.shutdown();
        // As the other core test files: the core is still held here (its
        // fields drop after this), so on Windows its database file is open
        // and the folder cannot be removed yet. The run's scratch goes with
        // the machine's temporary folder.
        let _ = std::fs::remove_dir_all(&self.data);
    }
}

fn choice(model: &str) -> BenchChoice {
    BenchChoice {
        provider: "openrouter".into(),
        model: model.into(),
        effort: None,
    }
}

fn bench_message(target: Target, text: &str) -> Outgoing {
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

/// A short name for each update.
fn kinds(updates: &[Update]) -> Vec<String> {
    updates
        .iter()
        .map(|u| match u {
            Update::Error(e) => format!("error: {e}"),
            Update::Bench {
                bench,
                details: Ok(d),
            } => format!(
                "bench {bench}: {} rounds, {} votes, revealed {}",
                d.rounds.len(),
                d.votes.len(),
                d.revealed
            ),
            Update::Bench {
                bench,
                details: Err(e),
            } => format!("bench {bench} failed: {e}"),
            Update::BenchSent { bench, round, .. } => format!("sent {bench} round {round}"),
            Update::SendRefused { error, .. } => format!("refused: {error}"),
            Update::Leaderboard(Ok(board)) => format!("leaderboard {} votes", board.votes),
            Update::Forked { chat, .. } => format!("forked {chat:?}"),
            Update::PromptsSaved(saved) => {
                format!("prompts saved {:?}", saved.as_ref().map(Vec::len))
            }
            Update::Generated { request, outcome } => format!("generated {request}: {outcome:?}"),
            other => format!("{other:?}").chars().take(40).collect(),
        })
        .collect()
}

/// The index of the first update from `from` on that `pick` holds of.
fn find(updates: &[Update], from: usize, pick: impl Fn(&Update) -> bool) -> Option<usize> {
    (from..updates.len()).find(|&i| pick(&updates[i]))
}

/// What an update is looked for by, and its name for a failure.
type Pick<'a> = (&'a str, &'a dyn Fn(&Update) -> bool);

/// Asserts that a command ended in updates holding each of `picks` in this
/// order (other updates may lie between); the index of each. Searched from
/// the end: the last update the last pick holds of, the last one before it
/// the pick before holds of, and so on, so an update published unasked
/// among the command's own (a turn's end, a list's details) is not taken
/// for its ending.
fn in_order(updates: &[Update], picks: &[Pick]) -> Vec<usize> {
    let mut before = updates.len();
    let mut found = Vec::new();
    for (what, pick) in picks.iter().rev() {
        let i = (0..before)
            .rev()
            .find(|&i| pick(&updates[i]))
            .unwrap_or_else(|| panic!("no {what} before {before}: {:?}", kinds(updates)));
        found.push(i);
        before = i;
    }
    found.reverse();
    found
}

fn is_bench(id: &str) -> impl Fn(&Update) -> bool + '_ {
    move |u| matches!(u, Update::Bench { bench, .. } if bench == id)
}

fn bench_ok(update: &Update) -> &BenchDetails {
    match update {
        Update::Bench {
            details: Ok(details),
            ..
        } => details,
        other => panic!("not a bench's details: {other:?}"),
    }
}

fn is_list(u: &Update) -> bool {
    matches!(u, Update::Conversations(_))
}

fn is_error(text: &'static str) -> impl Fn(&Update) -> bool {
    move |u| matches!(u, Update::Error(e) if e == text)
}

/// The last update `pick` holds of.
fn last(updates: &[Update], pick: impl Fn(&Update) -> bool) -> &Update {
    updates.iter().rev().find(|u| pick(u)).expect("an update")
}

/// The window's model of a bench's members on a list: running or not.
fn running(update: &Update, bench: &str) -> Vec<bool> {
    let Update::Conversations(list) = update else {
        panic!("not a list")
    };
    let mut members: Vec<_> = list
        .conversations
        .iter()
        .filter(|row| row.summary.bench_id.as_deref() == Some(bench))
        .collect();
    members.sort_by_key(|row| row.summary.bench_slot);
    members
        .iter()
        .map(|row| row.running_since.is_some())
        .collect()
}

/// `Target::NewBench`: the list that has the bench with its models
/// running, then its details with the round, then `BenchSent`; each change
/// of the details after (the models' turns ending) is published after a
/// list, and the last details published are the bench's as stored.
#[test]
fn a_new_bench_is_listed_then_detailed_then_sent_and_its_turns_end_in_its_details() {
    let app = App::new("new");
    let gates = [app.gate("slow/a"), app.gate("slow/b")];
    let from = app.len();
    let bench = app.new_bench(&["slow/a", "slow/b"], true);
    let updates = app.all()[from..].to_vec();
    let sent = find(&updates, 0, |u| matches!(u, Update::BenchSent { .. })).unwrap();
    let lists_it = |u: &Update| matches!(u, Update::Conversations(list) if list.benches.iter().any(|b| b.id == bench));
    // The list the send published itself is the last before `BenchSent`;
    // the details went out after the first list that had the bench.
    let listed = (0..sent)
        .rev()
        .find(|&i| lists_it(&updates[i]))
        .expect("a list with the bench before BenchSent");
    assert_eq!(running(&updates[listed], &bench), [true, true]);
    let first_listed = find(&updates, 0, lists_it).unwrap();
    let detailed = find(&updates, first_listed, is_bench(&bench))
        .filter(|&i| i < sent)
        .expect("its details after a list with it and before BenchSent");
    assert_eq!(bench_ok(&updates[detailed]).rounds.len(), 1);
    assert!(matches!(
        &updates[sent],
        Update::BenchSent { target: Target::NewBench { blind: true, .. }, bench: b, round: 1 } if *b == bench
    ));

    for gate in &gates {
        gate.send_replace(true);
    }
    app.settle(&bench);
    let stored = app.details(&bench);
    assert!(
        stored
            .stats
            .iter()
            .all(|(_, rounds)| rounds.len() == 1 && rounds[0].output_tokens == Some(3))
    );
    let updates = app.wait(from, "the ended turns' details", |u| {
        u.iter()
            .rev()
            .find(|u| is_bench(&bench)(u))
            .is_some_and(|u| *bench_ok(u) == stored)
            && u.iter()
                .rev()
                .find(|u| is_list(u))
                .is_some_and(|u| running(u, &bench) == [false, false])
    });
    // Every details' update follows the list of its change: the first has
    // a list before it, and each later one a list between it and the
    // details before it, wherever `BenchSent` falls among them (a list
    // published before `BenchSent` can have its details after it: the
    // list's publisher reads them while the send publishes `BenchSent`).
    let mut previous: Option<usize> = None;
    for i in (0..updates.len()).filter(|&i| is_bench(&bench)(&updates[i])) {
        let since = previous.map_or(0, |p| p + 1);
        assert!(
            (since..i).any(|j| is_list(&updates[j])),
            "details at {i} with no list since {since}: {:?}",
            kinds(&updates)
        );
        previous = Some(i);
    }
}

/// `Core::bench`'s JSON is today's route: the stored rounds as stored
/// (content with a file, the library prompts, a round's two versions),
/// then `stats`, byte for byte.
#[test]
fn a_benchs_details_are_the_routes_json_byte_for_byte() {
    let app = App::new("json");
    let from = app.len();
    app.send(Command::Send(bench_message(
        Target::NewBench {
            members: vec![choice("own/a"), choice("own/b")],
            blind: false,
        },
        "A hut",
    )));
    let bench = app
        .wait(from, "the bench sent", |u| {
            u.iter().any(|u| matches!(u, Update::BenchSent { .. }))
        })
        .iter()
        .find_map(|u| match u {
            Update::BenchSent { bench, .. } => Some(bench.clone()),
            _ => None,
        })
        .unwrap();
    app.settle(&bench);
    let round = app.details(&bench).rounds[0].id.clone();
    let mut edit = bench_message(Target::Bench(bench.clone()), "A barn");
    edit.edit_of = Some(MessageId(round.clone()));
    let refused = app.run(Command::Send(edit));
    assert!(
        !refused
            .iter()
            .any(|u| matches!(u, Update::SendRefused { .. })),
        "{:?}",
        kinds(&refused)
    );
    app.settle(&bench);
    // The next round with a file and the library prompt it came from.
    let mut next = bench_message(Target::Bench(bench.clone()), "A tower");
    next.attachments = vec![OutgoingFile {
        name: "notes.txt".into(),
        content_type: "text/plain".into(),
        size: 5,
        data: b"hello".to_vec().into(),
    }];
    next.prompts = vec![PromptLink {
        id: "p1".into(),
        title: "Tower".into(),
    }];
    let sent = app.run(Command::Send(next));
    assert!(
        sent.iter()
            .any(|u| matches!(u, Update::BenchSent { round: 2, .. })),
        "{:?}",
        kinds(&sent)
    );
    app.settle(&bench);

    let read = app.run(Command::ReadBench {
        bench: bench.clone(),
    });
    let details = bench_ok(last(&read, is_bench(&bench))).clone();
    assert_eq!(details.rounds.len(), 2);
    assert_eq!(details.rounds[0].branch.count, 2);
    assert_eq!(details.rounds[0].branch.index, 1);
    assert_eq!(
        details.rounds[0].branch.previous_id.as_deref(),
        Some(round.as_str())
    );
    let stored = app.core.store().bench(&bench).unwrap().unwrap();
    let mut stats = mcbench_store::json::Object::new();
    for member in app.core.store().bench_members(&bench).unwrap() {
        let member = member.lock().unwrap();
        let rounds = mcbench_lite::core::routes::benches::round_stats(&member.messages).unwrap();
        stats.insert(
            member.id.0.clone(),
            Json::Array(
                rounds
                    .iter()
                    .map(|r| {
                        let n = |v: Option<u64>| v.map_or(Json::Null, Json::from);
                        mcbench_store::object! {
                            "ms" => n(r.ms), "inputTokens" => n(r.input_tokens), "outputTokens" => n(r.output_tokens),
                        }
                    })
                    .collect(),
            ),
        );
    }
    let mut votes = mcbench_store::json::Object::new();
    for (round, vote) in &stored.votes {
        votes.insert(
            round.clone(),
            mcbench_store::object! { "pick" => vote.pick.clone(), "blind" => vote.blind },
        );
    }
    let route = mcbench_store::object! {
        "id" => stored.id.as_str(), "title" => stored.title.as_str(), "createdAt" => stored.created_at.as_str(),
        "head" => stored.head.clone(), "rounds" => Json::Array(stored.rounds.clone()),
        "blind" => stored.blind, "revealed" => stored.revealed, "order" => stored.order.clone(),
        "votes" => Json::Object(votes), "stats" => Json::Object(stats),
    };
    let ours = mcbench_store::json::stringify(&details.to_json());
    assert_eq!(ours, mcbench_store::json::stringify(&route));
    assert!(
        ours.contains(r#""prompts":[{"id":"p1","title":"Tower"}]"#),
        "{ours}"
    );
    assert!(ours.contains(r#""type":"attachment""#), "{ours}");
}

/// A bench's sends refused before any turn: `SendRefused` with the
/// route's sentence (or the core's, naming the field the window got
/// wrong), and no `BenchSent`.
#[test]
fn bench_sends_are_refused_with_their_sentences() {
    let app = App::new("refused");
    let gate = app.gate("slow/a");
    let bench = app.new_bench(&["slow/a", "own/b"], false);
    let round = app.details(&bench).rounds[0].id.clone();
    let new = |members: Vec<BenchChoice>| Target::NewBench {
        members,
        blind: false,
    };
    let with = |target: Target, change: &dyn Fn(&mut Outgoing)| {
        let mut message = bench_message(target, "A hut");
        change(&mut message);
        Command::Send(message)
    };
    let selection = Selection {
        letter: "A".into(),
        label: "A".into(),
        min: [0, 0, 0],
        max: [1, 1, 1],
        blocks: 1,
        block: Some("minecraft:stone".into()),
    };
    let cases: Vec<(Command, String)> = vec![
        (
            with(new(vec![choice("own/a")]), &|_| {}),
            "A bench runs 2 to 8 models".into(),
        ),
        (
            with(new(vec![choice("own/a"), choice("own/b")]), &|m| {
                m.effort = Some("high".into())
            }),
            "effort is not sent with a bench's round: each model has its own".into(),
        ),
        (
            with(new(vec![choice("own/a"), choice("own/b")]), &|m| {
                m.selections = vec![selection.clone()]
            }),
            "Selections belong to one build, not a bench".into(),
        ),
        (
            with(new(vec![choice("own/a"), choice("own/b")]), &|m| {
                m.text = "   ".into()
            }),
            "Message text or an attachment is required".into(),
        ),
        (
            with(Target::Bench("nobody".into()), &|_| {}),
            "Bench not found".into(),
        ),
        (
            with(Target::Bench(bench.clone()), &|m| {
                m.resolution = Resolution::High
            }),
            "resolution 2 is not this bench's (1)".into(),
        ),
        (
            with(Target::Bench(bench.clone()), &|m| {
                m.effort = Some(String::new())
            }),
            "effort is not sent with a bench's round: each model has its own".into(),
        ),
        (
            with(Target::Bench(bench.clone()), &|m| {
                m.selections = vec![selection.clone()]
            }),
            "Selections belong to one build, not a bench".into(),
        ),
        (
            with(Target::Bench(bench.clone()), &|m| {
                m.edit_of = Some(MessageId(round.clone()))
            }),
            "Stop the running models before editing a round".into(),
        ),
    ];
    for (command, sentence) in cases {
        let updates = app.run(command);
        let refusals: Vec<&String> = updates
            .iter()
            .filter_map(|u| match u {
                Update::SendRefused { error, .. } => Some(error),
                _ => None,
            })
            .collect();
        assert_eq!(refusals, [&sentence], "{:?}", kinds(&updates));
        assert!(
            !updates
                .iter()
                .any(|u| matches!(u, Update::BenchSent { .. }))
        );
    }
    gate.send_replace(true);
    app.settle(&bench);
}

/// `Vote`: the details with the vote, then the leaderboard read fresh;
/// taken back the same; a refusal is `Error` then the unchanged details.
#[test]
fn votes_end_in_the_details_then_the_leaderboard() {
    let app = App::new("votes");
    let bench = app.new_bench(&["own/a", "own/b"], true);
    app.settle(&bench);
    let members = app.members(&bench);
    let round = app.details(&bench).rounds[0].id.clone();
    let vote = |pick: VotePick| Command::Vote {
        bench: bench.clone(),
        round: round.clone(),
        pick,
    };

    let updates = app.run(vote(VotePick::Member(members[0].clone())));
    let at = in_order(
        &updates,
        &[
            ("the details", &is_bench(&bench)),
            ("the leaderboard", &|u| matches!(u, Update::Leaderboard(_))),
        ],
    );
    let details = bench_ok(&updates[at[0]]);
    assert_eq!(details.votes.len(), 1);
    assert_eq!(
        details.votes[0].1.pick.as_deref(),
        Some(members[0].as_str())
    );
    let Update::Leaderboard(Ok(board)) = &updates[at[1]] else {
        panic!("{:?}", kinds(&updates))
    };
    assert_eq!(board.votes, 1);
    assert_eq!(board.entries, app.core.store().leaderboard().unwrap());
    assert_eq!(board.entries.len(), 2);
    assert!(!updates.iter().any(|u| matches!(u, Update::Error(_))));

    let updates = app.run(vote(VotePick::Tie));
    let at = in_order(
        &updates,
        &[
            ("the details", &is_bench(&bench)),
            ("the leaderboard", &|u| matches!(u, Update::Leaderboard(_))),
        ],
    );
    assert_eq!(bench_ok(&updates[at[0]]).votes[0].1.pick, None);

    let updates = app.run(vote(VotePick::TakeBack));
    let at = in_order(
        &updates,
        &[
            ("the details", &is_bench(&bench)),
            ("the leaderboard", &|u| matches!(u, Update::Leaderboard(_))),
        ],
    );
    assert!(bench_ok(&updates[at[0]]).votes.is_empty());
    assert!(matches!(&updates[at[1]], Update::Leaderboard(Ok(board)) if board.votes == 0));

    let before = app.details(&bench);
    for (command, sentence) in [
        (
            Command::Vote {
                bench: bench.clone(),
                round: "nothing".into(),
                pick: VotePick::Tie,
            },
            "Unknown round",
        ),
        (
            vote(VotePick::Member("nobody".into())),
            "Pick a model that finished this round",
        ),
        (
            Command::Vote {
                bench: "nobody".into(),
                round: round.clone(),
                pick: VotePick::Tie,
            },
            "Bench not found",
        ),
    ] {
        let updates = app.run(command);
        let at = in_order(
            &updates,
            &[
                ("the refusal", &is_error(sentence)),
                ("the details", &|u| matches!(u, Update::Bench { .. })),
            ],
        );
        assert!(
            !updates.iter().any(|u| matches!(u, Update::Leaderboard(_))),
            "{:?}",
            kinds(&updates)
        );
        if let Update::Bench {
            details: Ok(details),
            ..
        } = &updates[at[1]]
        {
            assert_eq!(*details, before);
        }
    }

    // An open bench, and a blind one whose models still run.
    let open = app.new_bench(&["own/a", "own/b"], false);
    app.settle(&open);
    let open_round = app.details(&open).rounds[0].id.clone();
    let updates = app.run(Command::Vote {
        bench: open.clone(),
        round: open_round,
        pick: VotePick::Tie,
    });
    in_order(
        &updates,
        &[
            (
                "the refusal",
                &is_error("Votes are cast while the models' names are hidden"),
            ),
            ("the details", &is_bench(&open)),
        ],
    );
    let gate = app.gate("slow/a");
    let busy = app.new_bench(&["slow/a", "own/b"], true);
    let busy_round = app.details(&busy).rounds[0].id.clone();
    let updates = app.run(Command::Vote {
        bench: busy.clone(),
        round: busy_round,
        pick: VotePick::Tie,
    });
    in_order(
        &updates,
        &[
            (
                "the refusal",
                &is_error("Vote once every model has finished this round"),
            ),
            ("the details", &is_bench(&busy)),
        ],
    );
    gate.send_replace(true);
    app.settle(&busy);
}

/// `Reveal`: the list with the bench revealed, then its details; a
/// failure is `Error` first.
#[test]
fn reveal_ends_in_the_list_then_the_details() {
    let app = App::new("reveal");
    let bench = app.new_bench(&["own/a", "own/b"], true);
    app.settle(&bench);
    let updates = app.run(Command::Reveal {
        bench: bench.clone(),
    });
    let at = in_order(
        &updates,
        &[
            ("the list", &|u| {
                matches!(u, Update::Conversations(list)
                    if list.benches.iter().any(|b| b.id == bench && b.revealed))
            }),
            ("the details", &is_bench(&bench)),
        ],
    );
    assert!(bench_ok(&updates[at[1]]).revealed);
    assert!(!updates.iter().any(|u| matches!(u, Update::Error(_))));

    let updates = app.run(Command::Reveal {
        bench: "nobody".into(),
    });
    let at = in_order(
        &updates,
        &[
            ("the refusal", &is_error("Bench not found")),
            ("the list", &is_list),
            ("the details", &is_bench("nobody")),
        ],
    );
    assert!(
        matches!(&updates[at[2]], Update::Bench { details: Err(e), .. } if e == "Bench not found")
    );
}

/// `BenchBranch`: the list, then the details on the line followed; while
/// a model runs it is refused, then the list and the unchanged details.
#[test]
fn a_branch_switch_ends_in_the_list_then_the_details() {
    let app = App::new("branch");
    let bench = app.new_bench(&["own/a", "own/b"], false);
    app.settle(&bench);
    let first = app.details(&bench).rounds[0].id.clone();
    let mut edit = bench_message(Target::Bench(bench.clone()), "A barn");
    edit.edit_of = Some(MessageId(first.clone()));
    let updates = app.run(Command::Send(edit));
    assert!(
        updates
            .iter()
            .any(|u| matches!(u, Update::BenchSent { round: 1, .. }))
    );
    app.settle(&bench);
    assert_ne!(app.details(&bench).rounds[0].id, first);

    let updates = app.run(Command::BenchBranch {
        bench: bench.clone(),
        round: first.clone(),
    });
    let at = in_order(
        &updates,
        &[("the list", &is_list), ("the details", &is_bench(&bench))],
    );
    assert_eq!(bench_ok(&updates[at[1]]).rounds[0].id, first);
    assert!(!updates.iter().any(|u| matches!(u, Update::Error(_))));
    app.settle(&bench);

    let updates = app.run(Command::BenchBranch {
        bench: bench.clone(),
        round: "nothing".into(),
    });
    in_order(
        &updates,
        &[
            ("the refusal", &is_error("Unknown round")),
            ("the list", &is_list),
            ("the details", &is_bench(&bench)),
        ],
    );

    let gate = app.gate("slow/a");
    let busy = app.new_bench(&["slow/a", "own/b"], false);
    let busy_round = app.details(&busy).rounds[0].id.clone();
    let before = app.details(&busy);
    let updates = app.run(Command::BenchBranch {
        bench: busy.clone(),
        round: busy_round,
    });
    let at = in_order(
        &updates,
        &[
            (
                "the refusal",
                &is_error("Stop the running models before switching branches"),
            ),
            ("the list", &is_list),
            ("the details", &is_bench(&busy)),
        ],
    );
    assert_eq!(bench_ok(&updates[at[2]]).rounds, before.rounds);
    gate.send_replace(true);
    app.settle(&busy);
}

/// `RetryMember`: the list (the model running again); a refusal is
/// `Error` first.
#[test]
fn a_retry_ends_in_the_list() {
    let app = App::new("retry");
    let bench = app.new_bench(&["fail/a", "own/b"], false);
    app.settle(&bench);
    let members = app.members(&bench);
    let last_error = || {
        let live = app.core.store().conversation(&members[0]).unwrap().unwrap();
        let conversation = live.lock().unwrap();
        conversation
            .messages
            .iter()
            .rev()
            .find(|m| matches!(m.message, mcbench_store::Message::Error(_)))
            .map(|m| m.id.0.clone())
            .expect("the failed round's error")
    };
    let failed = last_error();
    let updates = app.run(Command::RetryMember {
        bench: bench.clone(),
        member: members[0].clone(),
    });
    in_order(&updates, &[("the list", &is_list)]);
    assert!(!updates.iter().any(|u| matches!(u, Update::Error(_))));
    app.settle(&bench);
    assert_ne!(
        last_error(),
        failed,
        "the retried round ran and failed again"
    );

    for (member, sentence) in [
        (
            members[1].clone(),
            "Only a failed or stopped model can be retried",
        ),
        ("nobody".to_string(), "This model is not in the bench"),
    ] {
        let updates = app.run(Command::RetryMember {
            bench: bench.clone(),
            member,
        });
        in_order(
            &updates,
            &[("the refusal", &is_error(sentence)), ("the list", &is_list)],
        );
    }
}

/// `Fork`: the list with the new chat, then `Forked` with its id; a
/// failure is `Forked` with the route's sentence.
#[test]
fn a_fork_ends_in_forked() {
    let app = App::new("fork");
    let bench = app.new_bench(&["own/a", "own/b"], true);
    app.settle(&bench);
    let members = app.members(&bench);
    let updates = app.run(Command::Fork {
        member: members[0].clone(),
        round: 1,
    });
    let forked = find(&updates, 0, |u| matches!(u, Update::Forked { .. })).unwrap();
    let Update::Forked {
        member,
        chat: Ok(chat),
    } = &updates[forked]
    else {
        panic!("{:?}", kinds(&updates))
    };
    assert_eq!(member, &members[0]);
    assert!((0..forked).any(|i| {
        matches!(&updates[i], Update::Conversations(list)
            if list.conversations.iter().any(|row| row.summary.id.0 == *chat))
    }));
    assert!(app.core.store().bench(&bench).unwrap().unwrap().revealed);

    for (member, round, sentence) in [
        (members[0].clone(), 9, "Unknown round"),
        ("nobody".to_string(), 1, "Bench chat not found"),
    ] {
        let updates = app.run(Command::Fork {
            member: member.clone(),
            round,
        });
        let at = in_order(
            &updates,
            &[("forked", &|u| matches!(u, Update::Forked { .. }))],
        );
        assert!(
            matches!(&updates[at[0]], Update::Forked { member: m, chat: Err(e) } if *m == member && e == sentence),
            "{:?}",
            kinds(&updates)
        );
    }
}

fn prompt(id: &str, title: &str) -> LibraryPrompt {
    LibraryPrompt {
        id: id.into(),
        title: title.into(),
        text: format!("{title}, please"),
        tags: vec!["houses".into()],
        favorite: true,
        attachments: Vec::new(),
        used_at: None,
        added_by: None,
    }
}

/// `SavePrompts`: `PromptsSaved` with the library as saved, and no
/// `Prompts` of its own; a library that cannot be written is
/// `PromptsSaved(Err)`.
#[test]
fn saving_the_library_ends_in_prompts_saved() {
    let app = App::new("prompts");
    let library = vec![prompt("p1", "A hut"), prompt("p2", "A barn")];
    let updates = app.run(Command::SavePrompts(library.clone()));
    let saved = updates
        .iter()
        .find_map(|u| match u {
            Update::PromptsSaved(saved) => Some(saved.clone()),
            _ => None,
        })
        .expect("PromptsSaved");
    let saved = saved.unwrap();
    assert_eq!(
        saved.iter().map(|p| p.title.as_str()).collect::<Vec<_>>(),
        ["A hut", "A barn"]
    );
    assert_eq!(
        saved,
        app.on(|core| async move { core.prompts().await.unwrap() })
    );
    assert!(!updates.iter().any(|u| matches!(u, Update::Prompts(_))));

    // The library's file is a folder now: the save fails at the disk.
    let file = app.core.store().folder().prompts();
    std::fs::remove_file(&file).unwrap();
    std::fs::create_dir_all(file.join("in-the-way")).unwrap();
    let updates = app.run(Command::SavePrompts(library));
    assert!(
        updates
            .iter()
            .any(|u| matches!(u, Update::PromptsSaved(Err(e)) if !e.is_empty())),
        "{:?}",
        kinds(&updates)
    );
    assert!(!updates.iter().any(|u| matches!(u, Update::Prompts(_))));
}

fn generate(request: u64, provider: &str, model: &str, text: &str) -> Command {
    Command::GeneratePrompt {
        request,
        provider: provider.into(),
        model: model.into(),
        effort: None,
        text: text.into(),
    }
}

fn generated(updates: &[Update], request: u64) -> Vec<Generated> {
    updates
        .iter()
        .filter_map(|u| match u {
            Update::Generated {
                request: r,
                outcome,
            } if *r == request => Some(outcome.clone()),
            _ => None,
        })
        .collect()
}

/// `GeneratePrompt`: the text, or the route's sentence; `CancelGenerate`
/// stops the model's call and the request ends in `Cancelled`, also when
/// the cancel is sent before the request's task has run; a cancel for a
/// request that has ended publishes nothing; the core's shutdown cancels
/// every request under way.
#[test]
fn a_prompt_request_ends_in_generated_and_a_cancel_stops_it() {
    let app = App::new("generate");
    let updates = app.run(generate(1, "openrouter", "gen/ok", "a hut"));
    assert_eq!(
        generated(&updates, 1),
        [Generated::Text("A better prompt".into())]
    );
    for (request, provider, text, sentence) in [
        (
            2,
            "nobody",
            "a hut",
            "Choose a configured provider".to_string(),
        ),
        (
            3,
            "openrouter",
            "   ",
            "Write something to improve first".to_string(),
        ),
        (4, "groq", "a hut", "Groq is not configured".to_string()),
    ] {
        let updates = app.run(generate(request, provider, "gen/ok", text));
        assert_eq!(generated(&updates, request), [Generated::Failed(sentence)]);
    }

    // Cancelled while the model's call waits.
    let from = app.len();
    app.send(generate(5, "openrouter", "gen/slow", "a hut"));
    app.models.wait_started(1);
    app.send(Command::CancelGenerate { request: 5 });
    let updates = app.wait(from, "the cancelled request", |u| {
        !generated(u, 5).is_empty()
    });
    assert_eq!(generated(&updates, 5), [Generated::Cancelled]);
    assert_eq!(app.models.slow.lock().unwrap().1, 1);

    // Cancelled as soon as it is sent.
    let from = app.len();
    app.send(generate(6, "openrouter", "gen/slow", "a hut"));
    app.send(Command::CancelGenerate { request: 6 });
    let updates = app.wait(from, "the request cancelled at once", |u| {
        !generated(u, 6).is_empty()
    });
    assert_eq!(generated(&updates, 6), [Generated::Cancelled]);

    // A cancel for a request that has ended publishes nothing.
    let updates = app.run(Command::CancelGenerate { request: 1 });
    assert!(generated(&updates, 1).is_empty(), "{:?}", kinds(&updates));

    // The core's shutdown cancels a request under way, which ends.
    let from = app.len();
    let before = app.models.slow.lock().unwrap().0;
    app.send(generate(7, "openrouter", "gen/slow", "a hut"));
    app.models.wait_started(before + 1);
    app.core.shutdown();
    assert_eq!(generated(&app.all()[from..], 7), [Generated::Cancelled]);
}

/// `ReadBench`: the details, or the core's sentence for a bench that is
/// not there; `ClearVotes`: the leaderboard, then the details of every
/// bench read since the core started, a deleted one no more.
#[test]
fn clearing_the_votes_publishes_every_bench_read() {
    let app = App::new("clear");
    let updates = app.run(Command::ReadBench {
        bench: "nobody".into(),
    });
    assert!(matches!(
        last(&updates, is_bench("nobody")),
        Update::Bench { details: Err(e), .. } if e == "Bench not found"
    ));
    let a = app.new_bench(&["own/a", "own/b"], true);
    let b = app.new_bench(&["own/c", "own/d"], true);
    app.settle(&a);
    app.settle(&b);
    for bench in [&a, &b] {
        let updates = app.run(Command::ReadBench {
            bench: bench.clone(),
        });
        assert_eq!(
            *bench_ok(last(&updates, is_bench(bench))),
            app.details(bench)
        );
        let round = app.details(bench).rounds[0].id.clone();
        app.run(Command::Vote {
            bench: bench.clone(),
            round,
            pick: VotePick::Tie,
        });
    }
    let updates = app.run(Command::ClearVotes);
    let at = in_order(
        &updates,
        &[
            (
                "the leaderboard",
                &|u| matches!(u, Update::Leaderboard(Ok(board)) if board.votes == 0),
            ),
            ("a's details", &is_bench(&a)),
        ],
    );
    assert!(bench_ok(&updates[at[1]]).votes.is_empty());
    let b_at = find(&updates, at[0], is_bench(&b)).expect("b's details");
    assert!(bench_ok(&updates[b_at]).votes.is_empty());

    app.run(Command::Archive(Subject::Bench(b.clone())));
    let deleted = app.run(Command::Delete(Subject::Bench(b.clone())));
    assert!(
        !deleted.iter().any(|u| matches!(u, Update::Error(_))),
        "{:?}",
        kinds(&deleted)
    );
    let updates = app.run(Command::ClearVotes);
    assert!(find(&updates, 0, is_bench(&a)).is_some());
    assert!(
        find(&updates, 0, is_bench(&b)).is_none(),
        "{:?}",
        kinds(&updates)
    );
}
