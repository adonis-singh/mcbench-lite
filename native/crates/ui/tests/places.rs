//! Places and history in the window (`state/place.rs`; `app.md` 10): Back
//! and Forward by key and by the mouse's buttons, a start place given as an
//! address (`--open`), the missing screen, and the new chat becoming its
//! chat without a history entry of its own.

mod common;

use common::*;
use gpui::accesskit::Role;
use gpui::{TestAppContext, VisualTestContext};
use mcbench_ui::state::address::address_place;
use mcbench_ui::state::place::Place;

const BACK: &str = if cfg!(target_os = "macos") {
    "cmd-["
} else {
    "alt-left"
};
const FORWARD: &str = if cfg!(target_os = "macos") {
    "cmd-]"
} else {
    "alt-right"
};

fn title(vcx: &mut VisualTestContext) -> String {
    vcx.window_title().expect("a title")
}

fn open_row(shell: &ShellUnderTest, vcx: &mut VisualTestContext, name: &str) {
    let tree = tree(vcx);
    click(vcx, row(&nodes(&tree), name));
    pump(shell, vcx);
}

#[gpui::test]
fn back_and_forward_move_through_the_places_opened(cx: &mut TestAppContext) {
    let (shell, vcx) = open_shell(cx, "history", Fixture::Gate, None);
    assert_eq!(title(vcx), "Redstone lamp with a lever · mcbench-lite");
    open_row(&shell, vcx, "Castle gate");
    open_row(&shell, vcx, "Wheat farm");
    vcx.simulate_keystrokes(BACK);
    pump(&shell, vcx);
    assert_eq!(title(vcx), "Castle gate · mcbench-lite");
    vcx.simulate_keystrokes(BACK);
    pump(&shell, vcx);
    assert_eq!(title(vcx), "Redstone lamp with a lever · mcbench-lite");
    vcx.simulate_keystrokes(BACK);
    assert_eq!(
        title(vcx),
        "Redstone lamp with a lever · mcbench-lite",
        "nothing before the first"
    );
    vcx.simulate_keystrokes(FORWARD);
    pump(&shell, vcx);
    assert_eq!(title(vcx), "Castle gate · mcbench-lite");
    // The mouse's back and forward buttons.
    let at = gpui::point(gpui::px(600.), gpui::px(300.));
    let nav = |d| gpui::MouseButton::Navigate(d);
    vcx.simulate_mouse_down(
        at,
        nav(gpui::NavigationDirection::Back),
        gpui::Modifiers::none(),
    );
    pump(&shell, vcx);
    assert_eq!(title(vcx), "Redstone lamp with a lever · mcbench-lite");
    vcx.simulate_mouse_down(
        at,
        nav(gpui::NavigationDirection::Forward),
        gpui::Modifiers::none(),
    );
    pump(&shell, vcx);
    assert_eq!(title(vcx), "Castle gate · mcbench-lite");
    // A new place after going back drops the forward entries.
    vcx.simulate_keystrokes(BACK);
    pump(&shell, vcx);
    open_row(&shell, vcx, "Wheat farm");
    vcx.simulate_keystrokes(FORWARD);
    assert_eq!(title(vcx), "Wheat farm · mcbench-lite");
}

#[gpui::test]
fn a_start_address_opens_its_place(cx: &mut TestAppContext) {
    let (_shell, vcx) = open_shell(cx, "open", Fixture::Gate, address_place("/chat/chat-gate"));
    assert_eq!(title(vcx), "Castle gate · mcbench-lite");
    let tree = tree(vcx);
    let row = row(&nodes(&tree), "Castle gate");
    assert_eq!(row.label().as_deref(), Some("Castle gate, open"));
    assert_eq!(row.description(), None);
}

#[test]
fn settings_is_no_place_and_has_no_address() {
    // Settings, the search panel and the archived section open over a
    // place (`web/address.mjs`).
    assert_eq!(address_place("/settings"), None);
}

#[gpui::test]
fn an_address_the_lists_do_not_have_shows_the_missing_screen(cx: &mut TestAppContext) {
    let (shell, vcx) = open_shell(cx, "missing", Fixture::Gate, address_place("/chat/gone"));
    assert_eq!(title(vcx), "Not found · mcbench-lite");
    let tree1 = tree(vcx);
    let nodes1 = nodes(&tree1);
    named(&nodes1, Role::Region, "There is no chat at this address");
    let current = vcx.update(|_, cx| shell.entity.read(cx).place().clone());
    assert!(matches!(current, Place::Missing { ref id, .. } if id == "gone"));
    // The missing screen's New chat (the sidebar has one too), after it in
    // document order.
    let news: Vec<_> = nodes1
        .iter()
        .filter(|n| n.role() == Role::Button && n.label().as_deref() == Some("New chat"))
        .copied()
        .collect();
    assert_eq!(news.len(), 2);
    click(vcx, news[1]);
    pump(&shell, vcx);
    assert_eq!(title(vcx), "New build · mcbench-lite");
    // A bench model's chat is not a Build chat; a bench the lists do not
    // have is missing as a bench.
    vcx.simulate_keystrokes(BACK);
    pump(&shell, vcx);
    assert_eq!(title(vcx), "Not found · mcbench-lite");
}

#[gpui::test]
fn the_new_chat_becomes_its_chat_in_the_same_history_entry(cx: &mut TestAppContext) {
    let (shell, vcx) = open_shell(cx, "new-chat", Fixture::Gate, None);
    let tree1 = tree(vcx);
    click(vcx, named(&nodes(&tree1), Role::Button, "New chat"));
    pump(&shell, vcx);
    assert_eq!(title(vcx), "New build · mcbench-lite");
    vcx.simulate_keystrokes("h i enter");
    pump(&shell, vcx);
    let place = vcx.update(|_, cx| shell.entity.read(cx).place().clone());
    assert!(
        matches!(place, Place::Chat { .. }),
        "the new chat is a chat now: {place:?}"
    );
    assert_eq!(title(vcx), "hi · mcbench-lite");
    vcx.simulate_keystrokes(BACK);
    pump(&shell, vcx);
    assert_eq!(
        title(vcx),
        "Redstone lamp with a lever · mcbench-lite",
        "Back skips the new chat's entry: it became the chat"
    );
    let remembered = vcx.update(|_, cx| shell.entity.read(cx).place_to_remember());
    assert_eq!(
        remembered,
        Place::Chat {
            id: "chat-lamp".into()
        }
    );
}
