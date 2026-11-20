//! The join's order (`Command::JoinTurn`, `Update::TurnJoined`, agreed with
//! the stage-three session): the window applies a chat's turn events only
//! after that chat's `TurnJoined` and drops every other chat's; on
//! `TurnJoined` it resets the turn's view and rebuilds it from the
//! `Transcript` that follows (the chat at the turn's start), then the
//! replayed events, then the live ones, each once; `TurnNotRunning` makes
//! it `Open` the chat again; a turn ends with `Done`, the chat's
//! `Transcript`, then the list. The stand-in's turns go by hand here, so a
//! join lands before the first event, between two, after the last and after
//! the end, and two chats' turns interleave.

mod common;

use std::sync::{Arc, Mutex};

use common::*;
use gpui::accesskit::Role;
use gpui::{TestAppContext, VisualTestContext};
use mcbench_ui::dev::standin::StandIn;
use mcbench_ui::state::place::Place;
use mcbench_ui::state::transcript::{Item, apply_event, items_from_messages};
use mcbench_ui::{Command, Core, TurnEvent, Update};

/// The stand-in, with every command the window sent kept.
struct Recorder {
    inner: Arc<StandIn>,
    sent: Arc<Mutex<Vec<Command>>>,
}

impl Core for Recorder {
    fn preferences(&self) -> mcbench_ui::Preferences {
        self.inner.preferences()
    }
    fn attach(&self, window: Arc<dyn mcbench_ui::Window>) {
        self.inner.attach(window)
    }
    fn send(&self, command: Command) {
        self.sent.lock().expect("lock").push(command.clone());
        self.inner.send(command)
    }
    fn image_file(&self, address: &str) -> Result<std::path::PathBuf, String> {
        self.inner.image_file(address)
    }
    fn attachment_file(&self, address: &str) -> Result<mcbench_ui::AttachmentFile, String> {
        self.inner.attachment_file(address)
    }
    // pane agent
    fn block_icon(&self, id: &str) -> Result<Vec<u8>, String> {
        self.inner.block_icon(id)
    }
    fn shutdown(&self) {
        self.inner.shutdown()
    }
}

struct Screen {
    shell: ShellUnderTest,
    core: Arc<StandIn>,
    sent: Arc<Mutex<Vec<Command>>>,
}

fn open<'a>(cx: &'a mut TestAppContext, test: &str) -> (Screen, &'a mut VisualTestContext) {
    use mcbench_ui::dev::fixtures;
    use mcbench_ui::view::shell::{Shell, ShellOptions};
    quiet_crashes();
    let dir = TempDir::new(test);
    let picture = dir.0.join("lamp.png");
    std::fs::write(&picture, fixtures::PICTURE).expect("the picture");
    let texts = fixtures::Texts::new(&picture);
    let core = Arc::new(StandIn::new(
        gate_fixture(),
        texts.streamed.clone(),
        std::time::Duration::ZERO,
        None,
    ));
    core.turns_by_hand();
    let sent = Arc::new(Mutex::new(Vec::new()));
    let recorder: Arc<dyn Core> = Arc::new(Recorder {
        inner: core.clone(),
        sent: sent.clone(),
    });
    let log_path = dir.0.join("actions.jsonl");
    let log = mcbench_ui::ActionLog::create(&log_path);
    cx.update(mcbench_ui::view::setup);
    let inbox = Arc::new(Inbox::default());
    let for_shell = recorder.clone();
    let (entity, vcx) = cx.add_window_view(move |window, cx| {
        window.activate_a11y_for_test();
        Shell::new(
            ShellOptions {
                core: for_shell,
                log: Some(log),
                opener: std::rc::Rc::new(|_, _| {}),
                metrics: std::rc::Rc::new(std::cell::RefCell::new(
                    mcbench_ui::view::metrics::Metrics::new(std::time::Instant::now()),
                )),
                start: Some(Place::Chat {
                    id: "chat-lamp".into(),
                }),
                preferences: mcbench_ui::Preferences {
                    last_model: Some(("anthropic".into(), mcbench_ui::dev::standin::MODEL.into())),
                    ..mcbench_ui::Preferences::default()
                },
                picker: mcbench_ui::view::fixed_picker(Vec::new()),
                selections: Vec::new(),
            },
            window,
            cx,
        )
    });
    recorder.attach(inbox.clone());
    let shell = ShellUnderTest::new(entity, inbox, log_path, dir);
    pump(&shell, vcx);
    (Screen { shell, core, sent }, vcx)
}

fn items(screen: &Screen, vcx: &mut VisualTestContext) -> Vec<Item> {
    vcx.update(|_, cx| {
        let chat = screen.shell.entity.read(cx).chat();
        chat.read(cx).items().to_vec()
    })
}

/// What the chat shows when it applied the turn's snapshot and then each
/// of its events once.
fn expected(screen: &Screen, chat: &str) -> Vec<Item> {
    let (snapshot, events) = screen.core.live_turn(chat).expect("a running turn");
    let Update::Transcript { messages, .. } = snapshot else {
        panic!("a snapshot");
    };
    let mut items = items_from_messages(&mcbench_ui::state::seam::from_stored(&messages));
    for event in &events {
        apply_event(&mut items, event);
    }
    items
}

fn go(screen: &Screen, vcx: &mut VisualTestContext, chat: &str) {
    vcx.update(|window, cx| {
        screen.shell.entity.update(cx, |s, cx| {
            s.go(Place::Chat { id: chat.into() }, window, cx)
        })
    });
    pump(&screen.shell, vcx);
}

fn advance(screen: &Screen, vcx: &mut VisualTestContext, chat: &str, events: usize) {
    assert!(screen.core.advance(chat, events), "the turn still runs");
    pump(&screen.shell, vcx);
}

fn opened(screen: &Screen, chat: &str) -> usize {
    screen
        .sent
        .lock()
        .expect("lock")
        .iter()
        .filter(|c| matches!(c, Command::Open { chat: c } if c == chat))
        .count()
}

/// How many events a turn of the stand-in publishes before its `Done`.
fn events_in_a_turn(screen: &Screen, vcx: &mut VisualTestContext) -> usize {
    screen.core.start_turn("chat-farm");
    let mut events = 0;
    while screen.core.advance("chat-farm", 1) {
        events += 1;
    }
    pump(&screen.shell, vcx);
    events
}

#[gpui::test]
fn a_join_lands_before_between_and_after_the_events_and_after_the_end(cx: &mut TestAppContext) {
    let (screen, vcx) = open(cx, "turns-join-at");
    // Another chat's whole turn ran meanwhile: none of it reached the
    // lamp's transcript.
    let lamp = items(&screen, vcx);
    let all = events_in_a_turn(&screen, vcx);
    assert!(all > 4, "{all}");
    assert_eq!(
        items(&screen, vcx),
        lamp,
        "another chat's events are dropped"
    );

    for at in [0, 3, all] {
        go(&screen, vcx, "chat-lamp");
        screen.core.start_turn("chat-gate");
        if at > 0 {
            advance(&screen, vcx, "chat-gate", at);
        }
        assert_eq!(items(&screen, vcx), lamp, "not joined: dropped (at {at})");
        // Opening the running chat joins it: the snapshot, the events so
        // far, each once.
        go(&screen, vcx, "chat-gate");
        assert_eq!(
            items(&screen, vcx),
            expected(&screen, "chat-gate"),
            "joined at {at}"
        );
        if at < all {
            advance(&screen, vcx, "chat-gate", 1);
            assert_eq!(
                items(&screen, vcx),
                expected(&screen, "chat-gate"),
                "a live event after the join at {at}"
            );
        }
        // The end: `Done`, the transcript as stored, the list.
        let reads = opened(&screen, "chat-gate");
        screen.core.finish("chat-gate");
        pump(&screen.shell, vcx);
        vcx.update(|_, cx| {
            let chat = screen.shell.entity.read(cx).chat();
            assert!(!chat.read(cx).busy(cx), "the turn ended (at {at})");
        });
        assert_eq!(
            opened(&screen, "chat-gate"),
            reads,
            "after Done the core sends the transcript; the window asks for none"
        );
        let shown = items(&screen, vcx);
        assert!(
            matches!(shown.last(), Some(Item::Assistant { live: false, .. })),
            "the stored answer: {:?}",
            shown.last()
        );
    }

    // After the end: the list still says it runs, the core has no turn,
    // `TurnNotRunning`: the window reads the chat again.
    go(&screen, vcx, "chat-lamp");
    screen.core.set_turn("chat-gate", true, false, None);
    pump(&screen.shell, vcx);
    let reads = opened(&screen, "chat-gate");
    go(&screen, vcx, "chat-gate");
    assert_eq!(
        opened(&screen, "chat-gate"),
        reads + 2,
        "Open, then Open again"
    );
    vcx.update(|_, cx| {
        let chat = screen.shell.entity.read(cx).chat();
        assert!(matches!(
            chat.read(cx).phase(),
            mcbench_ui::state::turn::Turn::Idle { .. }
                | mcbench_ui::state::turn::Turn::Detached { .. }
        ));
    });
}

#[gpui::test]
fn two_chats_turns_interleaved_only_the_joined_one_shows(cx: &mut TestAppContext) {
    let (screen, vcx) = open(cx, "turns-interleaved");
    screen.core.start_turn("chat-gate");
    screen.core.start_turn("chat-farm");
    advance(&screen, vcx, "chat-farm", 2);
    advance(&screen, vcx, "chat-gate", 1);
    go(&screen, vcx, "chat-gate");
    for _ in 0..3 {
        advance(&screen, vcx, "chat-farm", 1);
        advance(&screen, vcx, "chat-gate", 1);
        assert_eq!(items(&screen, vcx), expected(&screen, "chat-gate"));
    }
    screen.core.finish("chat-farm");
    pump(&screen.shell, vcx);
    assert_eq!(items(&screen, vcx), expected(&screen, "chat-gate"));
    screen.core.finish("chat-gate");
    pump(&screen.shell, vcx);
}

#[gpui::test]
fn a_turn_of_the_windows_own_send_joined_again_is_rebuilt_not_doubled(cx: &mut TestAppContext) {
    let (screen, vcx) = open(cx, "turns-own-send");
    let tree = tree(vcx);
    let composer = named(&nodes(&tree), Role::MultilineTextInput, "Message");
    act(vcx, composer, gpui::accesskit::Action::Focus);
    vcx.simulate_input("One more lamp");
    vcx.simulate_keystrokes("enter");
    pump(&screen.shell, vcx);
    // The send's `TurnJoined` and snapshot: the message is in it, stored.
    assert_eq!(items(&screen, vcx), expected(&screen, "chat-lamp"));
    advance(&screen, vcx, "chat-lamp", 3);
    assert_eq!(items(&screen, vcx), expected(&screen, "chat-lamp"));
    // Away and back: the join replays from the snapshot, nothing twice.
    go(&screen, vcx, "chat-gate");
    advance(&screen, vcx, "chat-lamp", 2);
    go(&screen, vcx, "chat-lamp");
    assert_eq!(items(&screen, vcx), expected(&screen, "chat-lamp"));
    advance(&screen, vcx, "chat-lamp", 1);
    assert_eq!(items(&screen, vcx), expected(&screen, "chat-lamp"));
    screen.core.finish("chat-lamp");
    pump(&screen.shell, vcx);
    let user_messages = items(&screen, vcx)
        .iter()
        .filter(|i| matches!(i, Item::User { text, .. } if text == "One more lamp"))
        .count();
    assert_eq!(user_messages, 1);
}

#[gpui::test]
fn an_event_for_the_open_chat_before_its_turn_joined_is_dropped(cx: &mut TestAppContext) {
    let (screen, vcx) = open(cx, "turns-stray");
    let before = items(&screen, vcx);
    let apply = |vcx: &mut VisualTestContext, update: Update| {
        vcx.update(|window, cx| {
            screen
                .shell
                .entity
                .update(cx, |s, cx| s.apply(update, window, cx))
        });
        vcx.run_until_parked();
    };
    apply(
        vcx,
        Update::Turn {
            chat: "chat-lamp".into(),
            event: TurnEvent::AssistantDelta("stray".into()),
        },
    );
    assert_eq!(items(&screen, vcx), before, "not joined: dropped");
    // Joined: the snapshot, then the event applies.
    let since = 1_800_000_000_000;
    apply(
        vcx,
        Update::TurnJoined {
            chat: "chat-lamp".into(),
            since,
        },
    );
    screen.core.send(Command::Open {
        chat: "chat-lamp".into(),
    });
    pump(&screen.shell, vcx);
    apply(
        vcx,
        Update::Turn {
            chat: "chat-lamp".into(),
            event: TurnEvent::AssistantDelta("joined".into()),
        },
    );
    let after = items(&screen, vcx);
    assert_eq!(after.len(), before.len() + 1);
    assert!(
        matches!(after.last(), Some(Item::Assistant { text, live: true, .. }) if text == "joined")
    );
}

fn joins(screen: &Screen, chat: &str) -> usize {
    screen
        .sent
        .lock()
        .expect("lock")
        .iter()
        .filter(|c| matches!(c, Command::JoinTurn { chat: c } if c == chat))
        .count()
}

/// The `You, <text>` messages the last frame's tree holds.
fn shown_user(vcx: &mut VisualTestContext, text: &str) -> usize {
    let tree = tree(vcx);
    of_role(&nodes(&tree), Role::Article)
        .iter()
        .filter(|a| a.label().as_deref() == Some(&format!("You, {text}")))
        .count()
}

#[gpui::test]
fn a_send_shows_its_message_once_in_every_frame_and_gets_one_turn_joined(cx: &mut TestAppContext) {
    // The send's own marker: `TurnJoined`, then the chat as stored at the
    // turn's start (with the message), then events. Applied one update at
    // a time, a frame drawn after each: the message is on screen once in
    // every frame, from the send on.
    let (screen, vcx) = open(cx, "turns-send-frames");
    let tree0 = tree(vcx);
    let composer = named(&nodes(&tree0), Role::MultilineTextInput, "Message");
    act(vcx, composer, gpui::accesskit::Action::Focus);
    vcx.simulate_input("One more lamp");
    vcx.simulate_keystrokes("enter");
    vcx.run_until_parked();
    assert_eq!(
        shown_user(vcx, "One more lamp"),
        1,
        "the optimistic message"
    );
    let mut kinds = Vec::new();
    loop {
        let updates: Vec<Update> = std::mem::take(&mut *screen.shell.inbox.0.lock().expect("lock"));
        if updates.is_empty() {
            break;
        }
        for update in updates {
            kinds.push(match &update {
                Update::TurnJoined { .. } => "joined",
                Update::Transcript { .. } => "transcript",
                Update::Conversations(_) => "list",
                Update::Turn { .. } => "event",
                _ => "other",
            });
            vcx.update(|window, cx| {
                screen
                    .shell
                    .entity
                    .update(cx, |s, cx| s.apply(update, window, cx))
            });
            vcx.update(|window, _| window.refresh());
            vcx.run_until_parked();
            assert_eq!(
                shown_user(vcx, "One more lamp"),
                1,
                "after {kinds:?}: the message once, never gone"
            );
        }
    }
    assert_eq!(
        kinds.iter().filter(|k| **k == "joined").count(),
        1,
        "one TurnJoined for the send's turn: {kinds:?}"
    );
    assert_eq!(
        joins(&screen, "chat-lamp"),
        0,
        "the window asked for no join"
    );
    assert_eq!(items(&screen, vcx), expected(&screen, "chat-lamp"));
    screen.core.finish("chat-lamp");
    pump(&screen.shell, vcx);
    assert_eq!(shown_user(vcx, "One more lamp"), 1);
}

#[gpui::test]
fn not_running_reads_the_chat_again_once_and_joins_only_a_new_turn(cx: &mut TestAppContext) {
    // The list says the gate runs, the core has no turn for it: the join is
    // answered `TurnNotRunning`, the window opens the chat again once. A
    // transcript whose header still says it runs, or the same list again,
    // starts no second join; a list with a new turn does.
    let (screen, vcx) = open(cx, "turns-not-running");
    screen.core.set_turn("chat-gate", true, false, None);
    pump(&screen.shell, vcx);
    let opens = opened(&screen, "chat-gate");
    go(&screen, vcx, "chat-gate");
    assert_eq!(joins(&screen, "chat-gate"), 1);
    assert_eq!(
        opened(&screen, "chat-gate"),
        opens + 2,
        "Open, then Open again once"
    );
    let since = vcx.update(|_, cx| {
        let chat = screen.shell.entity.read(cx).chat();
        match chat.read(cx).phase() {
            mcbench_ui::state::turn::Turn::Idle { ended } => *ended,
            other => panic!("idle after the turn was not running, not {other:?}"),
        }
    });
    // A transcript still saying a turn runs.
    let Update::Transcript {
        chat,
        mut header,
        messages,
        tree,
    } = screen.core.transcript_for_test("chat-gate")
    else {
        panic!("a transcript");
    };
    header.running_since = since;
    vcx.update(|window, cx| {
        screen.shell.entity.update(cx, |s, cx| {
            s.apply(
                Update::Transcript {
                    chat,
                    header,
                    messages,
                    tree,
                },
                window,
                cx,
            )
        })
    });
    pump(&screen.shell, vcx);
    // The same list again.
    screen.core.send(Command::MarkSeen {
        chat: "chat-farm".into(),
    });
    pump(&screen.shell, vcx);
    assert_eq!(
        joins(&screen, "chat-gate"),
        1,
        "no second join for the same turn"
    );
    assert_eq!(opened(&screen, "chat-gate"), opens + 2, "no loop of reads");
    // A new turn in the list: joined.
    std::thread::sleep(std::time::Duration::from_millis(2));
    screen.core.start_turn("chat-gate");
    pump(&screen.shell, vcx);
    assert_eq!(joins(&screen, "chat-gate"), 2);
    assert_eq!(items(&screen, vcx), expected(&screen, "chat-gate"));
    screen.core.finish("chat-gate");
    pump(&screen.shell, vcx);
}

fn marked_seen(screen: &Screen, chat: &str) -> usize {
    screen
        .sent
        .lock()
        .expect("lock")
        .iter()
        .filter(|c| matches!(c, Command::MarkSeen { chat: c } if c == chat))
        .count()
}

// seen: the window marks a chat seen where the page does (`state::seen`).

/// The status a Build row's name says ("Finished, not opened yet" while
/// unread and not open).
fn says_unread(vcx: &mut VisualTestContext, title: &str) -> bool {
    let tree = tree(vcx);
    row(&nodes(&tree), title)
        .label()
        .is_some_and(|l| l.contains("Finished, not opened yet"))
}

/// Navigates without applying what the core answers.
fn go_only(screen: &Screen, vcx: &mut VisualTestContext, place: Place) {
    vcx.update(|window, cx| {
        screen
            .shell
            .entity
            .update(cx, |s, cx| s.go(place, window, cx))
    });
    vcx.run_until_parked();
}

fn hide(vcx: &mut VisualTestContext, hidden: bool) {
    vcx.simulate_visibility_change(if hidden {
        gpui::WindowVisibility::Hidden
    } else {
        gpui::WindowVisibility::Visible
    });
    vcx.run_until_parked();
}

#[gpui::test]
fn opening_an_unread_chat_marks_it_seen_once_and_its_dot_clears_only_with_the_list(
    cx: &mut TestAppContext,
) {
    let (screen, vcx) = open(cx, "seen-open");
    assert_eq!(marked_seen(&screen, "chat-lamp"), 0, "a read chat");
    assert!(says_unread(vcx, "Wheat farm"));
    go_only(
        &screen,
        vcx,
        Place::Chat {
            id: "chat-farm".into(),
        },
    );
    assert_eq!(marked_seen(&screen, "chat-farm"), 1, "opening it marks it");
    // Back to another chat before the core's list came: still unread.
    go_only(
        &screen,
        vcx,
        Place::Chat {
            id: "chat-lamp".into(),
        },
    );
    assert!(says_unread(vcx, "Wheat farm"), "the list has not said so");
    // And to it again: the mark is on its way, none is sent again.
    go_only(
        &screen,
        vcx,
        Place::Chat {
            id: "chat-farm".into(),
        },
    );
    assert_eq!(marked_seen(&screen, "chat-farm"), 1, "no second mark");
    pump(&screen.shell, vcx);
    go(&screen, vcx, "chat-lamp");
    assert!(!says_unread(vcx, "Wheat farm"), "the list said it is seen");
    assert_eq!(marked_seen(&screen, "chat-farm"), 1);
    assert_eq!(
        opened(&screen, "chat-farm"),
        2,
        "opening reads the transcript only"
    );
}

#[gpui::test]
fn the_open_chat_turning_unread_is_marked_once(cx: &mut TestAppContext) {
    let (screen, vcx) = open(cx, "seen-turning");
    go(&screen, vcx, "chat-farm");
    assert_eq!(marked_seen(&screen, "chat-farm"), 1);
    // A turn finishes unseen while it is open (from elsewhere): one more.
    screen.core.set_turn("chat-farm", false, true, None);
    pump(&screen.shell, vcx);
    pump(&screen.shell, vcx);
    assert_eq!(marked_seen(&screen, "chat-farm"), 2);
    // The same list again sends nothing.
    screen.core.set_turn("chat-farm", false, false, None);
    pump(&screen.shell, vcx);
    assert_eq!(marked_seen(&screen, "chat-farm"), 2);
}

#[gpui::test]
fn a_hidden_window_marks_nothing_until_it_is_visible_again(cx: &mut TestAppContext) {
    let (screen, vcx) = open(cx, "seen-hidden");
    hide(vcx, true);
    go(&screen, vcx, "chat-farm");
    screen.core.set_turn("chat-lamp", false, true, None);
    pump(&screen.shell, vcx);
    assert_eq!(
        marked_seen(&screen, "chat-farm"),
        0,
        "hidden: not looked at"
    );
    go(&screen, vcx, "chat-lamp");
    assert!(says_unread(vcx, "Wheat farm"), "left unread");
    assert_eq!(marked_seen(&screen, "chat-lamp"), 0);
    // Visible again: the chat on screen is marked, the other is not.
    hide(vcx, false);
    pump(&screen.shell, vcx);
    assert_eq!(marked_seen(&screen, "chat-lamp"), 1);
    assert_eq!(marked_seen(&screen, "chat-farm"), 0);
    hide(vcx, true);
    hide(vcx, false);
    pump(&screen.shell, vcx);
    assert_eq!(marked_seen(&screen, "chat-lamp"), 1, "read now: no mark");
    // An inactive window that is visible still looks.
    vcx.deactivate_window();
    go(&screen, vcx, "chat-farm");
    assert_eq!(marked_seen(&screen, "chat-farm"), 1, "inactive but visible");
}

#[gpui::test]
fn a_turns_end_marks_the_chat_seen_once_while_visible(cx: &mut TestAppContext) {
    let (screen, vcx) = open(cx, "seen-turn-end");
    // chat-lamp is open and read.
    screen.core.start_turn("chat-lamp");
    pump(&screen.shell, vcx);
    assert_eq!(joins(&screen, "chat-lamp"), 1);
    screen.core.finish("chat-lamp");
    pump(&screen.shell, vcx);
    pump(&screen.shell, vcx);
    // The end marks it; the list after the end (unread until the mark
    // landed) sends no second one.
    assert_eq!(marked_seen(&screen, "chat-lamp"), 1);
    go(&screen, vcx, "chat-farm");
    assert!(!says_unread(vcx, "Redstone lamp with a lever"));
    // Hidden: the end marks nothing, and the chat stays unread until the
    // window is visible again.
    go(&screen, vcx, "chat-lamp");
    hide(vcx, true);
    screen.core.start_turn("chat-lamp");
    pump(&screen.shell, vcx);
    screen.core.finish("chat-lamp");
    pump(&screen.shell, vcx);
    pump(&screen.shell, vcx);
    assert_eq!(marked_seen(&screen, "chat-lamp"), 1, "hidden");
    hide(vcx, false);
    pump(&screen.shell, vcx);
    assert_eq!(marked_seen(&screen, "chat-lamp"), 2, "visible again");
}

#[gpui::test]
fn a_failed_mark_shows_its_error_and_is_not_sent_again_until_something_changes(
    cx: &mut TestAppContext,
) {
    let (screen, vcx) = open(cx, "seen-failed");
    screen
        .core
        .fail_next_seen("The chat could not be marked seen");
    go(&screen, vcx, "chat-farm");
    assert_eq!(marked_seen(&screen, "chat-farm"), 1);
    pump(&screen.shell, vcx);
    assert_eq!(marked_seen(&screen, "chat-farm"), 1, "no retry loop");
    let tree1 = tree(vcx);
    assert!(
        has(
            &nodes(&tree1),
            Role::Alert,
            "The chat could not be marked seen"
        ) || nodes(&tree1).iter().any(|n| n
            .label()
            .is_some_and(|l| l.contains("The chat could not be marked seen"))),
        "the error shows"
    );
    // The window hidden and shown again: marked again, this time it holds.
    hide(vcx, true);
    hide(vcx, false);
    pump(&screen.shell, vcx);
    assert_eq!(marked_seen(&screen, "chat-farm"), 2);
    go(&screen, vcx, "chat-lamp");
    assert!(!says_unread(vcx, "Wheat farm"));
}

#[gpui::test]
fn the_comparison_marks_every_unread_model_once_and_their_dots_follow_the_list(
    cx: &mut TestAppContext,
) {
    use gpui::accesskit::Action;
    let (screen, vcx) = open(cx, "seen-compare");
    screen.core.set_turn("member-a", false, true, None);
    pump(&screen.shell, vcx);
    let compare = Place::Compare {
        bench: "bench-towers".into(),
    };
    go_only(&screen, vcx, compare.clone());
    assert_eq!(marked_seen(&screen, "member-a"), 1);
    assert_eq!(marked_seen(&screen, "member-b"), 0, "read: not marked");
    let tree1 = tree(vcx);
    let fold = named(&nodes(&tree1), Role::Button, "Show models");
    act(vcx, fold, Action::Expand);
    vcx.run_until_parked();
    let models = |vcx: &mut VisualTestContext| -> Vec<String> {
        let tree = tree(vcx);
        named(&nodes(&tree), Role::List, "Towers and a bridge models")
            .children()
            .filter_map(|m| m.label())
            .collect()
    };
    assert!(
        models(vcx).contains(&"Build B, Finished, not opened yet".to_string()),
        "unread until the list says so: {:?}",
        models(vcx)
    );
    pump(&screen.shell, vcx);
    assert!(
        !models(vcx).iter().any(|m| m.contains("not opened yet")),
        "{:?}",
        models(vcx)
    );
    // A model finishing unseen while the comparison is open: marked once.
    screen.core.set_turn("member-b", false, true, None);
    pump(&screen.shell, vcx);
    pump(&screen.shell, vcx);
    assert_eq!(marked_seen(&screen, "member-b"), 1);
    assert_eq!(marked_seen(&screen, "member-a"), 1);
}

// seen: the seam's orders (`Command::Archive` and `Command::Open` in
// `lib.rs`): the window ends the same whichever comes first.

/// Applies what the core published, in the order `order` gives it.
fn apply_in(screen: &Screen, vcx: &mut VisualTestContext, order: impl FnOnce(&mut Vec<Update>)) {
    let mut updates: Vec<Update> = std::mem::take(&mut *screen.shell.inbox.0.lock().expect("lock"));
    order(&mut updates);
    for update in updates {
        vcx.update(|window, cx| {
            screen
                .shell
                .entity
                .update(cx, |s, cx| s.apply(update, window, cx))
        });
        vcx.run_until_parked();
    }
    pump(&screen.shell, vcx);
}

/// Every list row's name, and the place.
fn screen_state(screen: &Screen, vcx: &mut VisualTestContext) -> (Vec<String>, Place) {
    let tree = tree(vcx);
    let rows = of_role(&nodes(&tree), Role::ListItem)
        .iter()
        .filter_map(|n| n.label())
        .collect();
    let place = vcx.update(|_, cx| screen.shell.entity.read(cx).place().clone());
    (rows, place)
}

fn archive_with(cx: &mut TestAppContext, reversed: bool) -> (Vec<String>, Place) {
    let (screen, vcx) = open(
        cx,
        if reversed {
            "order-archive-b"
        } else {
            "order-archive-a"
        },
    );
    let tree0 = tree(vcx);
    click(vcx, named(&nodes(&tree0), Role::Button, "Show archived"));
    pump(&screen.shell, vcx);
    // The open chat archived (from elsewhere): the list, then the archive.
    screen.core.send(Command::Archive(mcbench_ui::Subject::Chat(
        "chat-lamp".into(),
    )));
    let kinds: Vec<bool> = screen
        .shell
        .inbox
        .0
        .lock()
        .expect("lock")
        .iter()
        .map(|u| matches!(u, Update::Conversations(_)))
        .collect();
    assert_eq!(
        kinds,
        [true, false],
        "the core's order: the list, then the archive"
    );
    apply_in(&screen, vcx, |u| {
        if reversed {
            u.reverse()
        }
    });
    screen_state(&screen, vcx)
}

#[gpui::test]
fn an_archive_ends_the_same_with_the_list_or_the_archive_first(cx: &mut TestAppContext) {
    let first = archive_with(cx, false);
    assert!(
        first
            .0
            .iter()
            .any(|r| r.starts_with("Redstone lamp with a lever, 7 days left")),
        "the archived row: {:?}",
        first.0
    );
    assert!(
        !first
            .0
            .iter()
            .any(|r| r.starts_with("Redstone lamp with a lever, open"))
    );
    assert_ne!(
        first.1,
        Place::Chat {
            id: "chat-lamp".into()
        },
        "the open chat moved on"
    );
    assert_eq!(archive_with(cx, true), first);
}

fn open_with(cx: &mut TestAppContext, reversed: bool) -> (Vec<String>, Vec<Item>, usize) {
    let (screen, vcx) = open(
        cx,
        if reversed {
            "order-open-b"
        } else {
            "order-open-a"
        },
    );
    go_only(
        &screen,
        vcx,
        Place::Chat {
            id: "chat-farm".into(),
        },
    );
    // The transcript (Open), then the list (MarkSeen); reversed, the list
    // came first.
    let kinds: Vec<&str> = screen
        .shell
        .inbox
        .0
        .lock()
        .expect("lock")
        .iter()
        .map(|u| match u {
            Update::Transcript { .. } => "transcript",
            Update::Conversations(_) => "list",
            _ => "other",
        })
        // pane agent: opening a chat also shows its world (`Live(Show)`).
        .filter(|k| *k != "other")
        .collect();
    assert_eq!(kinds, ["transcript", "list"]);
    apply_in(&screen, vcx, |u| {
        if reversed {
            u.reverse()
        }
    });
    let items = items(&screen, vcx);
    go(&screen, vcx, "chat-lamp");
    let (rows, _) = screen_state(&screen, vcx);
    (rows, items, marked_seen(&screen, "chat-farm"))
}

#[gpui::test]
fn opening_ends_the_same_with_the_transcript_or_the_list_first(cx: &mut TestAppContext) {
    let first = open_with(cx, false);
    assert!(!first.1.is_empty(), "the transcript shows");
    assert!(
        first
            .0
            .iter()
            .any(|r| r == "Wheat farm, Resolution: Very high (5\u{d7})"),
        "seen: {:?}",
        first.0
    );
    assert_eq!(first.2, 1, "one mark");
    let second = open_with(cx, true);
    assert_eq!(second, first);
}
