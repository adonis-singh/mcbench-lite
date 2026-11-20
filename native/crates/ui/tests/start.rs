//! The window's start place when the core's first lists come late (the
//! real core's cold start sends its setup and the pane first): a chat, a
//! bench's comparison or a member opens at once, typed text lands in that
//! place's own draft, and the lists then let it go on, or show the missing
//! place; one who moved meanwhile stays where they went; the lists arriving
//! first (the warm order) open the window as before. A recorded difference
//! from the page's `startPlace` (`inventory/README.md`, 2026-11-18).

mod common;

use std::time::Duration;

use common::*;
use gpui::VisualTestContext;
use gpui::accesskit::Role;
use mcbench_ui::state::place::Place;

fn settle(shell: &ShellUnderTest, vcx: &mut VisualTestContext) {
    pump(shell, vcx);
    vcx.executor().advance_clock(Duration::from_millis(200));
    pump(shell, vcx);
}

fn place(shell: &ShellUnderTest, vcx: &mut VisualTestContext) -> Place {
    vcx.update(|_, cx| shell.entity.read(cx).place().clone())
}

fn composer_text(shell: &ShellUnderTest, vcx: &mut VisualTestContext) -> String {
    vcx.update(|_, cx| {
        let chat = shell.entity.read(cx).chat();
        let composer = chat.read(cx).composer();
        composer.read(cx).value().to_string()
    })
}

fn logged(shell: &ShellUnderTest, action: &str) -> Vec<serde_json::Value> {
    log_lines(&shell.log)
        .into_iter()
        .filter(|a| a["action"] == action)
        .collect()
}

fn gate() -> Place {
    Place::Chat {
        id: "chat-gate".into(),
    }
}

#[gpui::test]
fn a_chat_opens_before_the_lists_and_keeps_what_was_typed(cx: &mut gpui::TestAppContext) {
    let (shell, core, vcx) = open_with_lists(cx, "start-chat", false, Some(gate()), true);
    settle(&shell, vcx);
    // The chat's place at once: its transcript read, the composer live.
    assert_eq!(place(&shell, vcx), gate());
    assert_eq!(logged(&shell, "start place").len(), 1);
    let tree1 = tree(vcx);
    assert!(has(&nodes(&tree1), Role::MultilineTextInput, "Message"));
    assert_eq!(focused(vcx).1.as_deref(), Some("Message"));
    vcx.simulate_input("Make it taller");
    settle(&shell, vcx);
    // The lists name it: it goes on, with no second open and the draft kept.
    core.release_lists();
    settle(&shell, vcx);
    assert_eq!(place(&shell, vcx), gate());
    assert_eq!(logged(&shell, "opened chat").len(), 1);
    assert_eq!(composer_text(&shell, vcx), "Make it taller");
    assert_eq!(focused(vcx).1.as_deref(), Some("Message"));
    // The text was the chat's draft, not the new chat's.
    vcx.update(|window, cx| {
        shell
            .entity
            .update(cx, |s, cx| s.go(Place::NewChat { id: None }, window, cx))
    });
    settle(&shell, vcx);
    assert_eq!(composer_text(&shell, vcx), "");
    vcx.update(|window, cx| shell.entity.update(cx, |s, cx| s.go(gate(), window, cx)));
    settle(&shell, vcx);
    assert_eq!(composer_text(&shell, vcx), "Make it taller");
}

#[gpui::test]
fn a_comparison_opens_before_the_lists_and_takes_its_newest_round(cx: &mut gpui::TestAppContext) {
    let castle = Place::Compare {
        bench: "bench-castle".into(),
    };
    let (shell, core, vcx) = open_with_lists(cx, "start-compare", true, Some(castle.clone()), true);
    settle(&shell, vcx);
    assert_eq!(place(&shell, vcx), castle);
    core.release_lists();
    settle(&shell, vcx);
    assert_eq!(place(&shell, vcx), castle);
    let (round, rounds) = vcx.update(|_, cx| {
        let shell = shell.entity.read(cx);
        let rounds = shell
            .lists_entity()
            .read(cx)
            .benches()
            .iter()
            .find(|b| b.id == "bench-castle")
            .expect("listed")
            .rounds;
        (shell.bench_pane().read(cx).round_of("bench-castle"), rounds)
    });
    assert_eq!(round, rounds.max(1));
    assert!(
        labels(vcx)
            .iter()
            .any(|l| l == "Castle with a moat comparison")
    );
}

#[gpui::test]
fn a_member_opens_before_the_lists(cx: &mut gpui::TestAppContext) {
    let member = Place::Member {
        bench: "bench-castle".into(),
        member: "castle-1".into(),
    };
    let (shell, core, vcx) = open_with_lists(cx, "start-member", true, Some(member.clone()), true);
    settle(&shell, vcx);
    assert_eq!(place(&shell, vcx), member);
    core.release_lists();
    settle(&shell, vcx);
    assert_eq!(place(&shell, vcx), member);
}

#[gpui::test]
fn a_chat_the_lists_do_not_have_is_the_missing_place(cx: &mut gpui::TestAppContext) {
    let gone = Place::Chat {
        id: "chat-gone".into(),
    };
    let (shell, core, vcx) = open_with_lists(cx, "start-missing", false, Some(gone.clone()), true);
    settle(&shell, vcx);
    assert_eq!(place(&shell, vcx), gone);
    core.release_lists();
    settle(&shell, vcx);
    assert!(
        matches!(place(&shell, vcx), Place::Missing { .. }),
        "{:?}",
        place(&shell, vcx)
    );
}

#[gpui::test]
fn one_who_moved_before_the_lists_stays_where_they_went(cx: &mut gpui::TestAppContext) {
    let (shell, core, vcx) = open_with_lists(cx, "start-moved", false, Some(gate()), true);
    settle(&shell, vcx);
    vcx.update(|window, cx| {
        shell
            .entity
            .update(cx, |s, cx| s.go(Place::NewChat { id: None }, window, cx))
    });
    settle(&shell, vcx);
    core.release_lists();
    settle(&shell, vcx);
    assert_eq!(place(&shell, vcx), Place::NewChat { id: None });
}

#[gpui::test]
fn the_lists_first_open_the_window_as_before(cx: &mut gpui::TestAppContext) {
    let (shell, _core, vcx) = open_with_lists(cx, "start-warm", false, Some(gate()), false);
    settle(&shell, vcx);
    assert_eq!(place(&shell, vcx), gate());
    // No start place opened ahead of the lists: the one open, after them.
    assert!(logged(&shell, "start place").is_empty());
    assert_eq!(logged(&shell, "opened chat").len(), 1);
    let first = log_lines(&shell.log)
        .into_iter()
        .position(|a| a["action"] == "opened chat")
        .expect("opened");
    assert!(first > 0);
    assert_eq!(focused(vcx).1.as_deref(), Some("Message"));
}

#[gpui::test]
fn the_landing_and_the_fixed_places_still_wait_for_the_lists(cx: &mut gpui::TestAppContext) {
    let (shell, core, vcx) = open_with_lists(cx, "start-landing", false, None, true);
    settle(&shell, vcx);
    assert!(logged(&shell, "start place").is_empty());
    assert_eq!(place(&shell, vcx), Place::NewChat { id: None });
    core.release_lists();
    settle(&shell, vcx);
    // The landing: the newest chat.
    assert!(matches!(place(&shell, vcx), Place::Chat { .. }));
}

fn labels(vcx: &mut VisualTestContext) -> Vec<String> {
    let tree = tree(vcx);
    nodes(&tree).iter().filter_map(|n| n.label()).collect()
}
