//! `turn-stream.test.js` (5): [`TurnStream`] and the holds.
//!
//! Today a listener is an HTTP response, which leaves the stream when it
//! closes; a [`TurnSink`] cannot close, so it stays until the turn ends and
//! is dropped then. The cases keep everything else of today's.

use super::*;

use std::panic::{AssertUnwindSafe, catch_unwind};

#[derive(Clone, Default)]
struct Lines(Arc<Mutex<Vec<Value>>>);

impl TurnSink for Lines {
    fn emit(&mut self, event: TurnEvent) {
        let value = event
            .to_json()
            .unwrap_or_else(|| serde_json::json!({ "type": "turn_snapshot" }));
        self.0.lock().unwrap().push(value);
    }
}

impl Lines {
    fn events(&self) -> Vec<Value> {
        self.0.lock().unwrap().clone()
    }
}

fn delta(arguments: &str) -> TurnEvent {
    TurnEvent::ToolCallDelta {
        index: 0,
        id: Some("call_1".into()),
        name: Some("build".into()),
        arguments: arguments.into(),
    }
}

#[test]
fn refresh_replays_the_snapshot_reasoning_and_partial_tool_input_exactly_once() {
    let turn = TurnStream::new(0);
    turn.push(TurnEvent::Snapshot);
    let first = Lines::default();
    turn.attach(Box::new(first.clone())).unwrap();
    turn.push(TurnEvent::ReasoningDelta("**Plan the layout**".into()));
    turn.push(TurnEvent::ReasoningDelta("\n\n**Choose materials**".into()));
    turn.push(delta(r#"{"program":"#));
    let seen = first.events();
    assert!(!turn.stopped());
    turn.push(delta(r#""code"}"#));
    let refreshed = Lines::default();
    turn.attach(Box::new(refreshed.clone())).unwrap();
    assert_eq!(refreshed.events()[..4], seen[..]);
    assert_eq!(refreshed.events().len(), 5);
    let joined = |kind: &str, key: &str| -> String {
        refreshed
            .events()
            .iter()
            .filter(|e| e["type"] == kind)
            .map(|e| e[key].as_str().unwrap().to_string())
            .collect()
    };
    assert_eq!(
        joined("reasoning_delta", "text"),
        "**Plan the layout**\n\n**Choose materials**"
    );
    assert_eq!(joined("tool_call_delta", "args"), r#"{"program":"code"}"#);
    turn.push(TurnEvent::Assistant {
        text: "Finished".into(),
        last: true,
    });
    assert_eq!(refreshed.events().last().unwrap()["text"], "Finished");
    let again = Lines::default();
    turn.attach(Box::new(again.clone())).unwrap();
    assert_eq!(again.events(), refreshed.events());
    turn.end();
    assert!(turn.state.lock().unwrap().listeners.is_empty());
}

#[test]
fn replay_captures_the_original_baseline_even_as_the_stored_transcript_changes() {
    // The snapshot carries no messages here (the core publishes the
    // transcript); what is replayed is the events as they were emitted.
    let turn = TurnStream::new(0);
    turn.push(TurnEvent::Snapshot);
    turn.push(TurnEvent::Assistant {
        text: "Starting".into(),
        last: false,
    });
    let refreshed = Lines::default();
    turn.attach(Box::new(refreshed.clone())).unwrap();
    let events = refreshed.events();
    assert_eq!(events[0]["type"], "turn_snapshot");
    assert_eq!(
        events.iter().filter(|e| e["type"] == "assistant").count(),
        1
    );
    turn.end();
}

#[test]
fn a_client_attached_while_the_prompt_is_accepted_receives_the_snapshot_when_ready() {
    let turn = TurnStream::new(0);
    let early = Lines::default();
    turn.attach(Box::new(early.clone())).unwrap();
    turn.push(TurnEvent::Snapshot);
    assert_eq!(early.events()[0]["type"], "turn_snapshot");
    turn.end();
    assert!(turn.state.lock().unwrap().listeners.is_empty());
}

#[test]
fn an_ended_turn_takes_no_more_events_or_clients() {
    let turn = TurnStream::new(0);
    turn.push(TurnEvent::Done);
    turn.end();
    assert!(!turn.state.lock().unwrap().open);
    let late = catch_unwind(AssertUnwindSafe(|| {
        turn.push(TurnEvent::Assistant {
            text: "late".into(),
            last: true,
        })
    }));
    assert!(late.is_err(), "This turn has ended");
    assert_eq!(
        turn.attach(Box::new(Lines::default())).unwrap_err(),
        "This turn has ended"
    );
    assert!(catch_unwind(AssertUnwindSafe(|| turn.end())).is_err());
    assert_eq!(turn.state.lock().unwrap().events.len(), 1);
}

struct Nothing;

#[async_trait]
impl Services for Nothing {
    fn provider(&self, _: &str) -> Result<Arc<dyn Provider>, String> {
        unimplemented!("no turn runs")
    }
    fn tools(&self) -> mcbench_tools::Tools {
        mcbench_tools::Tools::of(Vec::new())
    }
    fn title_model(&self) -> Option<(String, String, Option<String>)> {
        None
    }
    fn sync_attachments(&self, _: &str) -> Result<(), String> {
        Ok(())
    }
    async fn reset_sandbox(&self, _: &str) -> Result<(), String> {
        Ok(())
    }
    fn conversations_changed(&self) {}
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reservations_hold_keys_all_or_none_refuse_once_closed_and_idle_waits_for_holds_taken_later()
 {
    let root = std::env::temp_dir().join(format!("mcbench-holds-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let store = Arc::new(
        Store::open(
            &mcbench_store::DataFolder { root: root.clone() },
            mcbench_store::Options {
                host: Arc::new(mcbench_store::SystemHost),
                limits: mcbench_store::Limits::from_env(|_| None).unwrap(),
                chat_removed: Box::new(|_| {}),
            },
        )
        .unwrap(),
    );
    let turns = Turns::new(
        store,
        Arc::new(Nothing),
        TurnConfig {
            placement: Placement::Updates,
            network_access: true,
            budgets: Budgets::DEFAULT,
            max_context_bytes: crate::context::DEFAULT_MAX_BYTES,
            keep_exchanges: crate::context::KEEP_EXCHANGES,
            work: Arc::new(mcbench_tools::WorkFolders {
                root: root.join("sandbox"),
            }),
        },
    );
    let keys = |k: &[&str]| k.iter().map(|k| k.to_string()).collect::<Vec<_>>();
    let turn = turns.take(&keys(&["a"]), true, "busy").unwrap().remove(0);
    assert!(turns.turn("a").is_some());
    let refused = turns
        .take(&keys(&["b", "a"]), false, "a is busy")
        .err()
        .unwrap();
    assert_eq!(
        (refused.status, refused.message.as_str()),
        (409, "a is busy")
    );
    assert!(!turns.held("b"), "nothing is taken when one key is held");
    let edit = turns.take(&keys(&["b"]), false, "busy").unwrap().remove(0);
    assert!(turns.turn("b").is_none(), "an edit is not a turn");
    assert!(catch_unwind(AssertUnwindSafe(|| turns.release("b", &turn))).is_err());

    let waiting = {
        let turns = turns.clone();
        tokio::spawn(async move { turns.shutdown().await })
    };
    while !turns.closed() {
        tokio::task::yield_now().await;
    }
    turns.release("a", &turn);
    // `shutdown` closed the holds: a new one is refused, and the edit still
    // held is waited for.
    let closed = turns.take(&keys(&["c"]), false, "busy").err().unwrap();
    assert_eq!(closed.status, 503);
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    assert!(!waiting.is_finished());
    turns.release("b", &edit);
    waiting.await.unwrap();
    let _ = std::fs::remove_dir_all(&root);
}
