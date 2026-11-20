//! The narrow window (980 px and below; `style.css` S:1478-1528, inventory
//! `components.md` G.1, `style-viewer-routes.md` 1.20), read as an adapter
//! reads the tree: the sidebar as a bar over the world, the world, the chat
//! under it, and the list as a drawer over the page, at 980, 979, 800 and
//! 640 px wide (the window's narrowest), across a widening and a narrowing.

mod common;

use std::time::Duration;

use accesskit_consumer::Node;
use common::*;
use gpui::accesskit::{Action, Role, Toggled};
use gpui::{TestAppContext, VisualTestContext, px, size};
use mcbench_ui::state::place::Place;

const HEIGHT: f32 = 820.;

fn settle(shell: &ShellUnderTest, vcx: &mut VisualTestContext) {
    pump(shell, vcx);
    vcx.executor().advance_clock(Duration::from_millis(200));
    pump(shell, vcx);
}

fn resize(shell: &ShellUnderTest, vcx: &mut VisualTestContext, width: f32) {
    vcx.simulate_resize(size(px(width), px(HEIGHT)));
    settle(shell, vcx);
    // The shell draws again with the new width; the sidebar and the chat
    // after it.
    settle(shell, vcx);
}

fn mod_key(key: &str) -> String {
    if cfg!(target_os = "macos") {
        format!("cmd-{key}")
    } else {
        format!("ctrl-{key}")
    }
}

/// The node `role` named `label`, if there is exactly one.
fn find(vcx: &mut VisualTestContext, role: Role, label: &str) -> bool {
    let tree = tree(vcx);
    has(&nodes(&tree), role, label)
}

fn toggle_state(vcx: &mut VisualTestContext, label: &str) -> Option<bool> {
    let tree = tree(vcx);
    let node = named(&nodes(&tree), Role::Button, label);
    node.data().is_expanded()
}

fn press(vcx: &mut VisualTestContext, role: Role, label: &str) {
    let tree = tree(vcx);
    let node = named(&nodes(&tree), role, label);
    click(vcx, node);
}

fn within(node: Node<'_>, ancestor_role: Role, ancestor_label: &str) -> bool {
    let mut at = node.parent();
    while let Some(n) = at {
        if n.role() == ancestor_role && n.label().as_deref() == Some(ancestor_label) {
            return true;
        }
        at = n.parent();
    }
    false
}

fn focus_in_drawer(vcx: &mut VisualTestContext, name: &str) -> bool {
    let tree = tree(vcx);
    let focus = tree.state().focus().expect("a focused node");
    within(focus, Role::Complementary, name)
}

fn bounds(vcx: &mut VisualTestContext, role: Role, label: &str) -> gpui::accesskit::Rect {
    let tree = tree(vcx);
    named(&nodes(&tree), role, label)
        .bounding_box()
        .expect("bounds")
}

#[gpui::test]
fn the_sidebar_is_a_bar_over_the_world_at_980_and_below(cx: &mut TestAppContext) {
    let (shell, _core, vcx) = open_standin(cx, "narrow-bar", gate_fixture(), None);
    settle(&shell, vcx);
    // Wide: the sidebar's column with its list and its button.
    assert!(find(vcx, Role::Button, "Collapse sidebar"));
    assert!(find(vcx, Role::TextInput, "Search chats"));
    for width in [980., 979., 800., 640.] {
        resize(&shell, vcx, width);
        // The bar: the button named for the list, collapsed; the mode, New
        // chat and Settings; no list until the drawer opens.
        assert!(find(vcx, Role::Button, "Chats"), "{width}");
        assert_eq!(toggle_state(vcx, "Chats"), Some(false), "{width}");
        assert!(find(vcx, Role::Button, "New chat"), "{width}");
        assert!(find(vcx, Role::Button, "Settings"), "{width}");
        assert!(!find(vcx, Role::TextInput, "Search chats"), "{width}");
        assert!(!find(vcx, Role::Button, "Collapse sidebar"), "{width}");
        assert!(!find(vcx, Role::Button, "Open sidebar"), "{width}");
        // The world under the bar (38% of the height), the chat under it.
        let scale = f64::from(vcx.update(|window, _| window.scale_factor()));
        let world = bounds(vcx, Role::Canvas, "Live Minecraft world");
        let top = world.y0 / scale;
        let tall = (world.y1 - world.y0) / scale;
        assert!(
            (top - 52.).abs() < 1.5,
            "{width}: the world starts at {top}"
        );
        assert!(
            (tall - f64::from(HEIGHT) * 0.38).abs() < 1.5,
            "{width}: the world is {tall} high"
        );
        let message = bounds(vcx, Role::MultilineTextInput, "Message");
        assert!(
            message.y0 > world.y1,
            "{width}: the composer under the world"
        );
        assert!(
            ((message.x1 - message.x0) / scale) > f64::from(width) - 60.,
            "{width}: the chat column takes the width"
        );
    }
}

#[gpui::test]
fn the_drawer_opens_from_the_button_keeps_tab_and_escape_gives_focus_back(cx: &mut TestAppContext) {
    let (shell, _core, vcx) = open_standin(cx, "narrow-drawer", gate_fixture(), None);
    settle(&shell, vcx);
    resize(&shell, vcx, 800.);
    press(vcx, Role::Button, "Chats");
    settle(&shell, vcx);
    // A named layer over the page with the list in it; the button inside
    // it, an x named Close the list, expanded, and focused.
    assert!(find(vcx, Role::Complementary, "Chats"));
    assert!(find(vcx, Role::TextInput, "Search chats"));
    assert_eq!(toggle_state(vcx, "Close the list"), Some(true));
    assert_eq!(focused(vcx).1.as_deref(), Some("Close the list"));
    assert!(focus_in_drawer(vcx, "Chats"));
    // Tab and Shift+Tab stay in it.
    for _ in 0..30 {
        vcx.simulate_keystrokes("tab");
        assert!(
            focus_in_drawer(vcx, "Chats"),
            "Tab left the drawer: {:?}",
            focused(vcx)
        );
    }
    for _ in 0..30 {
        vcx.simulate_keystrokes("shift-tab");
        assert!(
            focus_in_drawer(vcx, "Chats"),
            "Shift+Tab left the drawer: {:?}",
            focused(vcx)
        );
    }
    // Escape closes it; focus is back on the button that opened it.
    vcx.simulate_keystrokes("escape");
    settle(&shell, vcx);
    assert!(!find(vcx, Role::Complementary, "Chats"));
    assert_eq!(focused(vcx), (Role::Button, Some("Chats".to_string())));
}

#[gpui::test]
fn the_sidebar_bind_opens_the_drawer_and_closing_it_gives_focus_back(cx: &mut TestAppContext) {
    let (shell, _core, vcx) = open_standin(cx, "narrow-bind", gate_fixture(), None);
    settle(&shell, vcx);
    resize(&shell, vcx, 640.);
    let tree1 = tree(vcx);
    act(
        vcx,
        named(&nodes(&tree1), Role::MultilineTextInput, "Message"),
        Action::Focus,
    );
    vcx.simulate_keystrokes(&mod_key("b"));
    settle(&shell, vcx);
    assert!(find(vcx, Role::Complementary, "Chats"));
    assert!(focus_in_drawer(vcx, "Chats"), "{:?}", focused(vcx));
    // The bind closes the drawer it opened; focus goes back to the composer.
    vcx.simulate_keystrokes(&mod_key("b"));
    settle(&shell, vcx);
    assert!(!find(vcx, Role::Complementary, "Chats"));
    assert_eq!(focused(vcx).1.as_deref(), Some("Message"));
}

#[gpui::test]
fn the_scrim_and_a_chosen_row_close_the_drawer(cx: &mut TestAppContext) {
    let (shell, _core, vcx) = open_standin(cx, "narrow-close", gate_fixture(), None);
    settle(&shell, vcx);
    resize(&shell, vcx, 800.);
    press(vcx, Role::Button, "Chats");
    settle(&shell, vcx);
    // A press on the scrim, right of the 340 px drawer.
    vcx.simulate_click(gpui::point(px(600.), px(400.)), gpui::Modifiers::none());
    settle(&shell, vcx);
    assert!(!find(vcx, Role::Complementary, "Chats"));
    // Choosing a row opens it and closes the drawer.
    press(vcx, Role::Button, "Chats");
    settle(&shell, vcx);
    let tree1 = tree(vcx);
    let row = of_role(&nodes(&tree1), Role::ListItem)
        .into_iter()
        .find(|n| n.label().is_some_and(|l| l.starts_with("Castle gate")))
        .expect("the Castle gate row");
    click(vcx, row);
    settle(&shell, vcx);
    assert!(!find(vcx, Role::Complementary, "Chats"));
    let place = vcx.update(|_, cx| shell.entity.read(cx).place().clone());
    assert!(matches!(place, Place::Chat { .. }), "{place:?}");
    let (role, name) = focused(vcx);
    assert_ne!(role, Role::Window, "focus is on a real control: {name:?}");
}

#[gpui::test]
fn crossing_980_keeps_the_drawer_asked_and_focus_on_a_control(cx: &mut TestAppContext) {
    let (shell, core, vcx) = open_standin(cx, "narrow-cross", gate_fixture(), None);
    // A turn running in the open chat's neighbour all the while.
    core.set_turn("chat-gate", true, false, None);
    settle(&shell, vcx);
    resize(&shell, vcx, 979.);
    press(vcx, Role::Button, "Chats");
    settle(&shell, vcx);
    assert!(find(vcx, Role::Complementary, "Chats"));
    // Widened: the sidebar's column again, no drawer, focus on a control.
    resize(&shell, vcx, 981.);
    assert!(!find(vcx, Role::Complementary, "Chats"));
    assert!(find(vcx, Role::Button, "Collapse sidebar"));
    assert!(find(vcx, Role::TextInput, "Search chats"));
    let (role, _) = focused(vcx);
    assert_ne!(role, Role::Window);
    // Escape has no drawer to close wide.
    let open = vcx.update(|_, cx| {
        mcbench_ui::view::keys::is_open(cx, mcbench_ui::state::layers::Layer::Drawer)
    });
    assert!(!open);
    // Narrowed again, the drawer is back, as the page's state keeps it.
    resize(&shell, vcx, 979.);
    assert!(find(vcx, Role::Complementary, "Chats"));
    vcx.simulate_keystrokes("escape");
    settle(&shell, vcx);
    assert!(!find(vcx, Role::Complementary, "Chats"));
    assert_eq!(toggle_state(vcx, "Chats"), Some(false));
    let _ = Toggled::False;
}

#[gpui::test]
fn the_prompt_library_has_the_bar_and_the_editor_only(cx: &mut TestAppContext) {
    let (shell, _core, vcx) = open_bench(cx, "narrow-prompts", Some(Place::Prompt), &[]);
    settle(&shell, vcx);
    resize(&shell, vcx, 800.);
    assert!(find(vcx, Role::Button, "Benches and prompts"));
    assert!(!find(vcx, Role::Canvas, "Live Minecraft world"));
}

#[gpui::test]
fn the_world_filling_the_window_keeps_filling_it_across_980(cx: &mut TestAppContext) {
    let (shell, _core, vcx) = open_standin(cx, "narrow-fill", gate_fixture(), None);
    settle(&shell, vcx);
    resize(&shell, vcx, 800.);
    press(vcx, Role::Button, "Fill the window");
    settle(&shell, vcx);
    let full = |vcx: &mut VisualTestContext| {
        vcx.update(|_, cx| shell.entity.read(cx).world().read(cx).full())
    };
    assert!(full(vcx));
    for width in [1280., 640., 979.] {
        resize(&shell, vcx, width);
        assert!(full(vcx), "{width}");
        let scale = f64::from(vcx.update(|window, _| window.scale_factor()));
        let world = bounds(vcx, Role::Canvas, "Live Minecraft world");
        assert!(
            ((world.x1 - world.x0) / scale - f64::from(width)).abs() < 1.5,
            "{width}: the world fills the window's width"
        );
    }
    // Escape gives the world back its row.
    vcx.simulate_keystrokes("escape");
    settle(&shell, vcx);
    assert!(!full(vcx));
}
