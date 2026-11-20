//! The window's layout (`ACCESSIBILITY.md` and `app.md` 3), in process: the
//! sidebar's width and collapse by pointer and keyboard, the split's drag
//! and its limits, the mode switcher, Settings as a layer,
//! and a resize that draws only the rows on screen.

mod common;

use common::*;
use gpui::accesskit::Role;
use gpui::{TestAppContext, point, px, size};
use mcbench_ui::state::layout;

const MOD: &str = if cfg!(target_os = "macos") {
    "cmd"
} else {
    "ctrl"
};

fn sidebar(shell: &ShellUnderTest, vcx: &mut gpui::VisualTestContext) -> (bool, f32) {
    vcx.update(|_, cx| {
        let l = shell.entity.read(cx).layout();
        (l.sidebar_open, l.sidebar_width)
    })
}

#[gpui::test]
fn the_sidebar_drags_between_its_limits_collapses_and_comes_back(cx: &mut TestAppContext) {
    let (shell, vcx) = open_shell(cx, "sidebar", Fixture::Gate, None);
    assert_eq!(sidebar(&shell, vcx), (true, 236.));
    let y = px(400.);
    // The edge is the 7 px at the sidebar's right.
    drag(vcx, point(px(233.), y), point(px(330.), y), 5);
    // The width is the pointer's x less the layout's left edge.
    assert_eq!(sidebar(&shell, vcx), (true, 330.));
    drag(vcx, point(px(324.), y), point(px(700.), y), 5);
    assert_eq!(sidebar(&shell, vcx), (true, layout::SIDEBAR_MAX));
    drag(vcx, point(px(417.), y), point(px(150.), y), 5);
    assert_eq!(sidebar(&shell, vcx), (true, layout::SIDEBAR_MIN));
    // Under 140 it collapses, keeping the width the drag began with.
    drag(vcx, point(px(197.), y), point(px(60.), y), 5);
    assert_eq!(sidebar(&shell, vcx), (false, layout::SIDEBAR_MIN));
    // A click on the strip at the window's left edge opens it again.
    vcx.simulate_click(point(px(3.), y), gpui::Modifiers::none());
    assert_eq!(sidebar(&shell, vcx), (true, layout::SIDEBAR_MIN));
    // A double-click on the edge collapses it.
    vcx.simulate_click(point(px(197.), y), gpui::Modifiers::none());
    vcx.simulate_event(gpui::MouseDownEvent {
        button: gpui::MouseButton::Left,
        position: point(px(197.), y),
        modifiers: gpui::Modifiers::none(),
        click_count: 2,
        first_mouse: false,
    });
    vcx.simulate_mouse_up(
        point(px(197.), y),
        gpui::MouseButton::Left,
        gpui::Modifiers::none(),
    );
    assert!(!sidebar(&shell, vcx).0, "a double-click collapses it");
}

#[gpui::test]
fn the_sidebar_shows_and_hides_by_keyboard(cx: &mut TestAppContext) {
    let (shell, vcx) = open_shell(cx, "sidebar-keys", Fixture::Gate, None);
    vcx.simulate_keystrokes(&format!("{MOD}-b"));
    assert_eq!(sidebar(&shell, vcx), (false, 236.));
    let tree1 = tree(vcx);
    let nodes1 = nodes(&tree1);
    assert!(!has(&nodes1, Role::List, "Chats"), "the sidebar is gone");
    // Its header buttons take its place.
    click(vcx, named(&nodes1, Role::Button, "Open sidebar"));
    assert_eq!(sidebar(&shell, vcx), (true, 236.));
    let tree2 = tree(vcx);
    click(vcx, named(&nodes(&tree2), Role::Button, "Collapse sidebar"));
    assert!(!sidebar(&shell, vcx).0);
    vcx.simulate_keystrokes(&format!("{MOD}-b"));
    assert!(sidebar(&shell, vcx).0);
}

#[gpui::test]
fn the_split_keeps_320_for_the_chat_and_360_for_the_world(cx: &mut TestAppContext) {
    let (shell, vcx) = open_shell(cx, "split", Fixture::Gate, None);
    let width = vcx.update(|window, _| f32::from(window.viewport_size().width));
    let chat = |vcx: &mut gpui::VisualTestContext| {
        vcx.update(|_, cx| shell.entity.read(cx).layout().chat_width)
    };
    assert_eq!(chat(vcx), None, "minmax(360px, 42%) until a drag");
    let divider = 236. + (width * 0.42).max(360.) + 2.;
    let y = px(300.);
    drag(vcx, point(px(divider), y), point(px(236. + 500.), y), 4);
    assert_eq!(chat(vcx), Some(500.));
    drag(vcx, point(px(236. + 502.), y), point(px(250.), y), 4);
    assert_eq!(chat(vcx), Some(layout::CHAT_MIN));
    drag(vcx, point(px(236. + 322.), y), point(px(width), y), 4);
    let most = (width - 236. - layout::DIVIDER - layout::WORLD_MIN).max(layout::CHAT_MIN);
    assert_eq!(chat(vcx), Some(most.round()));
}

#[gpui::test]
fn the_mode_switcher_and_settings_navigate(cx: &mut TestAppContext) {
    let (shell, vcx) = open_shell(cx, "navigate", Fixture::Gate, None);
    let tree1 = tree(vcx);
    let switcher = named(&nodes(&tree1), Role::Button, "mcbuild");
    assert_eq!(
        switcher.description().as_deref(),
        Some(if cfg!(target_os = "macos") {
            "Switch to Bench (⌘2)"
        } else {
            "Switch to Bench (Ctrl+2)"
        })
    );
    click(vcx, switcher);
    pump(&shell, vcx);
    let tree2 = tree(vcx);
    let nodes2 = nodes(&tree2);
    named(&nodes2, Role::Region, "New bench");
    named(&nodes2, Role::Button, "mcbench");
    named(&nodes2, Role::Button, "New bench");
    assert_eq!(
        vcx.window_title().as_deref(),
        Some("New bench \u{b7} mcbench-lite")
    );
    // Back to Build: the chat it was on.
    vcx.simulate_keystrokes(&format!("{MOD}-1"));
    pump(&shell, vcx);
    assert_eq!(
        vcx.window_title().as_deref(),
        Some("Redstone lamp with a lever \u{b7} mcbench-lite")
    );
    // Settings: a layer over the place, no place and no history entry;
    // shortcuts wait under it; Escape closes it, focus back on what opened
    // it (settings agent: the panel draws once the core's read lands, with
    // focus on its active pane's tab).
    let tree3 = tree(vcx);
    act(
        vcx,
        named(&nodes(&tree3), Role::Button, "Settings"),
        gpui::accesskit::Action::Focus,
    );
    vcx.simulate_keystrokes("enter");
    pump(&shell, vcx);
    let tree4 = tree(vcx);
    named(&nodes(&tree4), Role::Dialog, "Settings");
    assert_eq!(focused(vcx), (Role::Tab, Some("General".into())));
    vcx.simulate_keystrokes(&format!("{MOD}-2"));
    assert_eq!(
        vcx.window_title().as_deref(),
        Some("Redstone lamp with a lever \u{b7} mcbench-lite"),
        "Settings changes no place, and shortcuts wait under it"
    );
    vcx.simulate_keystrokes("escape");
    pump(&shell, vcx);
    let tree5 = tree(vcx);
    assert!(
        !has(&nodes(&tree5), Role::Dialog, "Settings"),
        "Escape closes it"
    );
    assert_eq!(focused(vcx), (Role::Button, Some("Settings".into())));
}

#[gpui::test]
fn a_resize_draws_only_the_rows_on_screen(cx: &mut TestAppContext) {
    let (shell, vcx) = open_shell(cx, "resize", Fixture::Long, None);
    let chat = vcx.update(|_, cx| shell.entity.read(cx).chat());
    let drawn = |vcx: &mut gpui::VisualTestContext| vcx.update(|_, cx| chat.read(cx).rows_drawn());
    let rows = vcx.update(|_, cx| chat.read(cx).render_count());
    assert!(rows > 0);
    let before = drawn(vcx);
    let mut per_resize = Vec::new();
    for (w, h) in [(1200., 800.), (1100., 760.), (1300., 820.), (1000., 700.)] {
        let start = drawn(vcx);
        vcx.simulate_resize(size(px(w), px(h)));
        vcx.run_until_parked();
        per_resize.push(drawn(vcx) - start);
    }
    // And the sidebar and the split, dragged.
    let start = drawn(vcx);
    drag(vcx, point(px(233.), px(300.)), point(px(300.), px(300.)), 6);
    per_resize.push(drawn(vcx) - start);
    eprintln!("rows drawn at open {before}, per resize {per_resize:?}");
    for count in per_resize {
        assert!(
            count < 60,
            "a resize lays out the rows on screen and their neighbours, not the 400 ({count})"
        );
    }
}
