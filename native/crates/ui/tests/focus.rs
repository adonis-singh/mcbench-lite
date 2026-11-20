//! Focus is never left on nothing (the Windows pass, W1): when the focused
//! element goes (a dismissed notice, the open chat archived from its row,
//! an archived item deleted, a change of place), focus moves to a defined
//! neighbour, and Tab goes on from there.

mod common;

use common::*;
use gpui::accesskit::{Action, Role};
use gpui::{TestAppContext, VisualTestContext};
use mcbench_ui::Core as _;
use mcbench_ui::state::place::Place;
use std::time::Duration;

fn settle(shell: &ShellUnderTest, vcx: &mut VisualTestContext) {
    pump(shell, vcx);
    vcx.executor().advance_clock(Duration::from_millis(300));
    pump(shell, vcx);
}

fn lamp() -> Option<Place> {
    Some(Place::Chat {
        id: "chat-lamp".into(),
    })
}

fn mod_key(key: &str) -> String {
    if cfg!(target_os = "macos") {
        format!("cmd-{key}")
    } else {
        format!("ctrl-{key}")
    }
}

/// A focused node with a role of its own (not the window), then Tab moves
/// focus on to another one.
fn tab_goes_on(shell: &ShellUnderTest, vcx: &mut VisualTestContext) {
    let before = focused(vcx);
    assert_ne!(before.0, Role::Window, "focus on a control, not the window");
    vcx.simulate_keystrokes("tab");
    settle(shell, vcx);
    let after = focused(vcx);
    assert_ne!(after.0, Role::Window, "Tab reaches a control");
    assert_ne!(after, before, "Tab moved focus");
}

fn focus_on(vcx: &mut VisualTestContext, role: Role, label: &str) {
    let tree = tree(vcx);
    act(vcx, named(&nodes(&tree), role, label), Action::Focus);
}

#[gpui::test]
fn dismissing_a_notice_puts_focus_in_the_composer(cx: &mut TestAppContext) {
    let (shell, core, vcx) = open_standin(cx, "focus-notice", gate_fixture(), lamp());
    core.refuse_sends_to("chat-lamp");
    let pane = vcx.update(|_, cx| shell.entity.read(cx).chat());
    vcx.update(|window, cx| pane.update(cx, |p, cx| p.focus_composer(window, cx)));
    vcx.simulate_input("hello");
    vcx.simulate_keystrokes("enter");
    settle(&shell, vcx);
    focus_on(vcx, Role::Button, "Dismiss error");
    vcx.simulate_keystrokes("enter");
    settle(&shell, vcx);
    let tree = tree(vcx);
    assert!(!has(&nodes(&tree), Role::Button, "Dismiss error"));
    assert_eq!(focused(vcx).0, Role::MultilineTextInput);
    tab_goes_on(&shell, vcx);
}

#[gpui::test]
fn archiving_the_focused_row_puts_focus_on_the_next_row(cx: &mut TestAppContext) {
    let (shell, _core, vcx) = open_standin(cx, "focus-archive", gate_fixture(), lamp());
    let tree = tree(vcx);
    let button = row(&nodes(&tree), "Redstone lamp with a lever")
        .children()
        .flat_map(|c| std::iter::once(c).chain(c.children()))
        .find(|n| n.label().as_deref() == Some("Archive build"))
        .expect("the row's archive button")
        .locate()
        .0;
    vcx.update(|window, cx| {
        window.dispatch_a11y_action_for_test(
            gpui::accesskit::ActionRequest {
                action: Action::Focus,
                target_tree: gpui::accesskit::TreeId::ROOT,
                target_node: button,
                data: None,
            },
            cx,
        )
    });
    pump(&shell, vcx);
    assert_eq!(focused(vcx), (Role::Button, Some("Archive build".into())));
    vcx.simulate_keystrokes("enter");
    settle(&shell, vcx);
    let (role, name) = focused(vcx);
    assert_eq!(role, Role::ListItem);
    assert!(
        name.as_deref()
            .is_some_and(|n| n.starts_with("Castle gate")),
        "{name:?}"
    );
    tab_goes_on(&shell, vcx);
}

#[gpui::test]
fn deleting_an_archived_item_puts_focus_on_its_neighbour(cx: &mut TestAppContext) {
    let (shell, core, vcx) = open_standin(cx, "focus-delete", gate_fixture(), lamp());
    core.send(mcbench_ui::Command::Archive(mcbench_ui::Subject::Chat(
        "chat-farm".into(),
    )));
    settle(&shell, vcx);
    focus_on(vcx, Role::Button, "Show archived");
    vcx.simulate_keystrokes("enter");
    settle(&shell, vcx);
    // Two archived rows; delete the first: focus to the row now there.
    let tree1 = tree(vcx);
    let first = named(&nodes(&tree1), Role::List, "Archived")
        .children()
        .next()
        .expect("a row");
    let first_name = first.label().unwrap_or_default();
    let delete = first
        .children()
        .flat_map(|c| std::iter::once(c).chain(c.children()))
        .find(|n| n.label().as_deref() == Some("Delete permanently"))
        .expect("its delete")
        .locate()
        .0;
    vcx.update(|window, cx| {
        window.dispatch_a11y_action_for_test(
            gpui::accesskit::ActionRequest {
                action: Action::Focus,
                target_tree: gpui::accesskit::TreeId::ROOT,
                target_node: delete,
                data: None,
            },
            cx,
        )
    });
    pump(&shell, vcx);
    vcx.simulate_keystrokes("enter");
    pump(&shell, vcx);
    vcx.simulate_keystrokes("enter");
    settle(&shell, vcx);
    let names: Vec<String> = {
        let tree = tree(vcx);
        named(&nodes(&tree), Role::List, "Archived")
            .children()
            .filter_map(|r| r.label())
            .collect()
    };
    assert_eq!(names.len(), 1, "{names:?}");
    assert!(!names.contains(&first_name));
    assert_eq!(focused(vcx), (Role::Button, Some("Restore".into())));
    tab_goes_on(&shell, vcx);
    // The last one: focus to the list's current row (the section goes).
    focus_on(vcx, Role::Button, "Delete permanently");
    vcx.simulate_keystrokes("enter");
    pump(&shell, vcx);
    vcx.simulate_keystrokes("enter");
    settle(&shell, vcx);
    assert_eq!(focused(vcx).0, Role::ListItem);
    tab_goes_on(&shell, vcx);
}

#[gpui::test]
fn a_change_of_place_puts_focus_on_the_new_places_first_control(cx: &mut TestAppContext) {
    let (shell, _core, vcx) = open_standin(cx, "focus-place", gate_fixture(), lamp());
    assert_eq!(focused(vcx).0, Role::MultilineTextInput);
    // bench agent: the new bench is the composer's, as Build's new chat.
    vcx.simulate_keystrokes(&mod_key("2"));
    settle(&shell, vcx);
    let (role, _) = focused(vcx);
    assert_eq!(role, Role::MultilineTextInput);
    tab_goes_on(&shell, vcx);
    // Back to a chat: the composer.
    vcx.simulate_keystrokes(&mod_key("1"));
    settle(&shell, vcx);
    let (role, _) = focused(vcx);
    assert_ne!(role, Role::Window);
    tab_goes_on(&shell, vcx);
}

#[gpui::test]
fn a_notice_is_in_the_tab_order_where_it_is_drawn_before_the_transcript(cx: &mut TestAppContext) {
    // W3: the notice sits over the column's top: Tab reaches it after the
    // pane's header and before the transcript, not after Send.
    let (shell, core, vcx) = open_standin(cx, "focus-notice-order", gate_fixture(), lamp());
    core.refuse_sends_to("chat-lamp");
    let pane = vcx.update(|_, cx| shell.entity.read(cx).chat());
    vcx.update(|window, cx| pane.update(cx, |p, cx| p.focus_composer(window, cx)));
    vcx.simulate_input("hello");
    vcx.simulate_keystrokes("enter");
    settle(&shell, vcx);
    vcx.update(|window, cx| pane.update(cx, |p, cx| p.focus_composer(window, cx)));
    pump(&shell, vcx);
    vcx.simulate_keystrokes("shift-tab");
    pump(&shell, vcx);
    assert_eq!(
        focused(vcx).0,
        Role::Article,
        "the transcript before the composer"
    );
    vcx.simulate_keystrokes("shift-tab");
    pump(&shell, vcx);
    assert_eq!(focused(vcx), (Role::Button, Some("Dismiss error".into())));
    // And forward from the composer's last control, no notice.
    let mut walked = Vec::new();
    vcx.update(|window, cx| pane.update(cx, |p, cx| p.focus_composer(window, cx)));
    pump(&shell, vcx);
    for _ in 0..6 {
        vcx.simulate_keystrokes("tab");
        pump(&shell, vcx);
        walked.push(focused(vcx));
    }
    let send = walked
        .iter()
        .position(|(_, n)| n.as_deref() == Some("Send message"))
        .expect("Send in the walk");
    assert_ne!(
        walked.get(send + 1).and_then(|(_, n)| n.clone()).as_deref(),
        Some("Dismiss error"),
        "{walked:?}"
    );
}
