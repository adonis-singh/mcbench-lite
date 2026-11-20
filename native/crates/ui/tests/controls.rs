//! The shared controls' contract (`ACCESSIBILITY.md`, the controls rows)
//! checked in process on the gallery (`mcbench_ui::dev::gallery`): roles,
//! names and states; presses by click, key and assistive technology; the
//! switch, the copy button, the confirm button's arming per subject, the
//! tooltip's delay, and the popover's opening, keys, Escape, press outside
//! and focus.

mod common;

use std::time::Duration;

use common::*;
use gpui::accesskit::{Action, Role, Toggled};
use gpui::{Entity, TestAppContext, VisualTestContext};
use mcbench_ui::ActionLog;
use mcbench_ui::dev::gallery::Gallery;

fn open<'a>(
    cx: &'a mut TestAppContext,
    test: &str,
) -> (Entity<Gallery>, &'a mut VisualTestContext, TempDir) {
    quiet_crashes();
    let dir = TempDir::new(test);
    let log = ActionLog::create(&dir.0.join("actions.jsonl"));
    cx.update(mcbench_ui::view::setup);
    let (gallery, vcx) = cx.add_window_view(move |window, cx| {
        window.activate_a11y_for_test();
        Gallery::new(Some(log), cx)
    });
    vcx.run_until_parked();
    (gallery, vcx, dir)
}

fn pressed(dir: &TempDir) -> Vec<String> {
    log_lines(&dir.0.join("actions.jsonl"))
        .into_iter()
        .filter(|l| l["action"] == "pressed")
        .map(|l| l["control"].as_str().expect("a name").to_string())
        .collect()
}

#[gpui::test]
fn every_control_has_its_role_name_and_state(cx: &mut TestAppContext) {
    let (_, vcx, _dir) = open(cx, "roles");
    let tree = tree(vcx);
    let nodes = nodes(&tree);
    for name in [
        "Raised",
        "Primary",
        "Delete for good",
        "Run bench",
        "Share",
        "New chat",
    ] {
        let node = named(&nodes, Role::Button, name);
        assert!(!node.is_disabled(), "{name} is enabled");
        assert!(
            node.is_focusable(&|_| accesskit_consumer::FilterResult::Include),
            "{name} is a focus stop"
        );
    }
    for name in [
        "Raised, disabled",
        "Primary, disabled",
        "Run bench, disabled",
        "Settings, disabled",
    ] {
        assert!(
            named(&nodes, Role::Button, name).is_disabled(),
            "{name} is disabled"
        );
    }
    // An icon button's name is its label, not its icon.
    named(&nodes, Role::Button, "Collapse sidebar");
    first(&nodes, Role::Button, "Copy code");
    let switch = named(&nodes, Role::Switch, "Show the grid");
    assert_eq!(switch.toggled(), Some(Toggled::True));
    let trigger = named(&nodes, Role::Button, "Effort: medium");
    assert_eq!(trigger.data().is_expanded(), Some(false));
    assert_eq!(
        trigger.description().as_deref(),
        Some("How hard the model thinks"),
        "a tip is the control's description, never its name"
    );
    named(&nodes, Role::Button, "Remove");
    // Icons add no nodes: the gallery's 47 icons and 14 logos are not in the tree.
    assert!(
        of_role(&nodes, Role::Image).is_empty(),
        "icons are decoration"
    );
}

#[gpui::test]
fn a_button_presses_by_click_key_and_assistive_technology(cx: &mut TestAppContext) {
    let (_, vcx, dir) = open(cx, "press");
    let tree1 = tree(vcx);
    pointer_click(vcx, named(&nodes(&tree1), Role::Button, "Raised"));
    let tree2 = tree(vcx);
    click(vcx, named(&nodes(&tree2), Role::Button, "Primary"));
    // Keyboard: focus Run bench, then Enter and Space.
    let tree3 = tree(vcx);
    act(
        vcx,
        named(&nodes(&tree3), Role::Button, "Run bench"),
        Action::Focus,
    );
    vcx.simulate_keystrokes("enter");
    vcx.simulate_keystrokes("space");
    // A disabled button takes nothing.
    let tree4 = tree(vcx);
    click(vcx, named(&nodes(&tree4), Role::Button, "Raised, disabled"));
    assert_eq!(
        pressed(&dir),
        ["Raised", "Primary", "Run bench", "Run bench"]
    );
}

#[gpui::test]
fn the_switch_flips_and_keeps_its_name(cx: &mut TestAppContext) {
    let (gallery, vcx, _dir) = open(cx, "switch");
    let tree1 = tree(vcx);
    click(vcx, named(&nodes(&tree1), Role::Switch, "Show the grid"));
    assert!(!vcx.update(|_, cx| gallery.read(cx).switch_on()));
    let tree2 = tree(vcx);
    let switch = named(&nodes(&tree2), Role::Switch, "Show the grid");
    assert_eq!(switch.toggled(), Some(Toggled::False));
    act(vcx, switch, Action::Focus);
    vcx.simulate_keystrokes("space");
    assert!(vcx.update(|_, cx| gallery.read(cx).switch_on()));
}

#[gpui::test]
fn copy_writes_the_clipboard_and_shows_the_check_for_a_moment(cx: &mut TestAppContext) {
    let (_, vcx, _dir) = open(cx, "copy");
    let tree1 = tree(vcx);
    click(vcx, first(&nodes(&tree1), Role::Button, "Copy code"));
    let copied = vcx.update(|_, cx| cx.read_from_clipboard().and_then(|c| c.text()));
    assert_eq!(
        copied.as_deref(),
        Some("world.set([0, 1, 0], 'redstone_lamp');")
    );
    let tree2 = tree(vcx);
    let button = first(&nodes(&tree2), Role::Button, "Copy code");
    assert_eq!(
        button.description().as_deref(),
        Some("Copied"),
        "the tip reads Copied"
    );
    vcx.executor().advance_clock(Duration::from_millis(1200));
    vcx.run_until_parked();
    let tree3 = tree(vcx);
    let button = first(&nodes(&tree3), Role::Button, "Copy code");
    assert_eq!(button.description().as_deref(), Some("Copy code"));
}

#[gpui::test]
fn the_confirm_button_asks_first_and_disarms_after_four_seconds(cx: &mut TestAppContext) {
    let (_, vcx, dir) = open(cx, "confirm");
    let tree1 = tree(vcx);
    click(vcx, named(&nodes(&tree1), Role::Button, "Remove"));
    assert!(pressed(&dir).is_empty(), "the first press only arms it");
    let tree2 = tree(vcx);
    click(
        vcx,
        named(&nodes(&tree2), Role::Button, "Remove from the web?"),
    );
    assert_eq!(pressed(&dir), ["Remove from the web?"]);
    let tree3 = tree(vcx);
    click(vcx, named(&nodes(&tree3), Role::Button, "Remove"));
    vcx.executor().advance_clock(Duration::from_millis(4000));
    vcx.run_until_parked();
    let tree4 = tree(vcx);
    named(&nodes(&tree4), Role::Button, "Remove");
    assert_eq!(pressed(&dir).len(), 1, "it disarmed without acting");
}

#[gpui::test]
fn the_tooltip_shows_after_half_a_second_of_hover_and_hides_at_once(cx: &mut TestAppContext) {
    let (_, vcx, _dir) = open(cx, "tip");
    let tip = "Tokens used in this chat, and the context window";
    let shown = |vcx: &mut VisualTestContext| {
        let tree = tree(vcx);
        has(&nodes(&tree), Role::Tooltip, tip)
    };
    let host = {
        let tree = tree(vcx);
        let nodes = nodes(&tree);
        let found = nodes
            .iter()
            .find(|n| n.label().as_deref() == Some("Hover for a tip"))
            .expect("the tip's host");
        found.bounding_box().expect("bounds")
    };
    let centre = centre_of(vcx, host);
    vcx.simulate_mouse_move(centre, None, gpui::Modifiers::none());
    // A frame hit-tests the pointer (a real window draws one on the move).
    let _ = tree(vcx);
    vcx.executor().advance_clock(Duration::from_millis(499));
    vcx.run_until_parked();
    assert!(!shown(vcx), "not before 500 ms");
    vcx.executor().advance_clock(Duration::from_millis(2));
    vcx.run_until_parked();
    assert!(shown(vcx), "shown after 500 ms of hover");
    vcx.simulate_mouse_move(
        gpui::point(gpui::px(2.), gpui::px(2.)),
        None,
        gpui::Modifiers::none(),
    );
    vcx.run_until_parked();
    assert!(!shown(vcx), "hidden at once when the pointer leaves");
}

#[gpui::test]
fn the_popover_opens_on_its_first_item_wraps_and_closes_back_to_its_trigger(
    cx: &mut TestAppContext,
) {
    let (gallery, vcx, _dir) = open(cx, "popover");
    let tree1 = tree(vcx);
    click(vcx, named(&nodes(&tree1), Role::Button, "Effort: medium"));
    let tree2 = tree(vcx);
    let nodes2 = nodes(&tree2);
    named(&nodes2, Role::Menu, "Effort");
    assert_eq!(
        named(&nodes2, Role::Button, "Effort: medium")
            .data()
            .is_expanded(),
        Some(true)
    );
    assert_eq!(
        named(&nodes2, Role::MenuItemRadio, "medium").toggled(),
        Some(Toggled::True)
    );
    assert_eq!(
        focused(vcx),
        (Role::MenuItemRadio, Some("low".into())),
        "focus on the first item"
    );
    vcx.simulate_keystrokes("up");
    assert_eq!(
        focused(vcx).1.as_deref(),
        Some("high"),
        "Up from the first wraps to the last"
    );
    vcx.simulate_keystrokes("down");
    assert_eq!(
        focused(vcx).1.as_deref(),
        Some("low"),
        "Down from the last wraps to the first"
    );
    vcx.simulate_keystrokes("end");
    assert_eq!(focused(vcx).1.as_deref(), Some("high"));
    // Escape closes it, and goes no further; focus is back on the trigger.
    vcx.simulate_keystrokes("escape");
    vcx.executor().advance_clock(Duration::from_millis(110));
    vcx.run_until_parked();
    let tree3 = tree(vcx);
    assert!(
        !has(&nodes(&tree3), Role::Menu, "Effort"),
        "closed after its 110 ms exit"
    );
    assert_eq!(focused(vcx), (Role::Button, Some("Effort: medium".into())));
    // Choosing an item closes it with the choice made.
    let tree4 = tree(vcx);
    click(vcx, named(&nodes(&tree4), Role::Button, "Effort: medium"));
    vcx.simulate_keystrokes("down down enter");
    vcx.executor().advance_clock(Duration::from_millis(110));
    vcx.run_until_parked();
    assert_eq!(vcx.update(|_, cx| gallery.read(cx).effort()), "high");
    let tree5 = tree(vcx);
    assert!(!has(&nodes(&tree5), Role::Menu, "Effort"));
    // A press outside closes it.
    let tree6 = tree(vcx);
    click(vcx, named(&nodes(&tree6), Role::Button, "Effort: high"));
    let tree7 = tree(vcx);
    assert!(has(&nodes(&tree7), Role::Menu, "Effort"));
    vcx.simulate_click(
        gpui::point(gpui::px(4.), gpui::px(4.)),
        gpui::Modifiers::none(),
    );
    vcx.executor().advance_clock(Duration::from_millis(110));
    vcx.run_until_parked();
    let tree8 = tree(vcx);
    assert!(
        !has(&nodes(&tree8), Role::Menu, "Effort"),
        "a press outside closes it"
    );
}

#[gpui::test]
fn the_trigger_takes_expand_and_collapse(cx: &mut TestAppContext) {
    let (_, vcx, _dir) = open(cx, "expand");
    let tree1 = tree(vcx);
    act(
        vcx,
        named(&nodes(&tree1), Role::Button, "Effort: medium"),
        Action::Expand,
    );
    let tree2 = tree(vcx);
    assert!(has(&nodes(&tree2), Role::Menu, "Effort"));
    let tree3 = tree(vcx);
    act(
        vcx,
        named(&nodes(&tree3), Role::Button, "Effort: medium"),
        Action::Collapse,
    );
    vcx.executor().advance_clock(Duration::from_millis(110));
    vcx.run_until_parked();
    let tree4 = tree(vcx);
    assert!(!has(&nodes(&tree4), Role::Menu, "Effort"));
}

// settings agent: the gallery's Settings section has copy buttons of its
// own (a sign-in's one-time code); the shared controls come first.
fn first<'a>(
    nodes: &[accesskit_consumer::Node<'a>],
    role: Role,
    label: &str,
) -> accesskit_consumer::Node<'a> {
    *nodes
        .iter()
        .find(|n| n.role() == role && n.label().as_deref() == Some(label))
        .unwrap_or_else(|| panic!("no {role:?} named {label:?}"))
}

// par-shell (for par-bench): a label's own tip.
#[gpui::test]
fn a_labels_own_tip_shows_over_the_label_and_the_buttons_over_the_rest(cx: &mut TestAppContext) {
    let (_, vcx, _dir) = open(cx, "label-tip");
    let tips = |vcx: &mut VisualTestContext, x: f32, y: f32| {
        vcx.simulate_mouse_move(
            gpui::point(gpui::px(x), gpui::px(y)),
            None,
            gpui::Modifiers::none(),
        );
        let _ = tree(vcx);
        vcx.executor().advance_clock(Duration::from_millis(600));
        vcx.run_until_parked();
        let tree = tree(vcx);
        let shown: Vec<String> = of_role(&nodes(&tree), Role::Tooltip)
            .iter()
            .filter_map(|t| t.label())
            .collect();
        shown
    };
    let (bounds, before) = {
        let tree = tree(vcx);
        let b = named(&nodes(&tree), Role::Button, "Claude Opus");
        (
            b.bounding_box().expect("bounds"),
            (b.label(), b.description()),
        )
    };
    let scale = f64::from(vcx.update(|window, _| window.scale_factor()));
    let y = ((bounds.y0 + bounds.y1) / 2. / scale) as f32;
    let (left, right) = ((bounds.x0 / scale) as f32, (bounds.x1 / scale) as f32);
    assert_eq!(
        tips(vcx, right - 22., y),
        ["claude-opus-5-5"],
        "over the label"
    );
    vcx.simulate_mouse_move(
        gpui::point(gpui::px(1.), gpui::px(1.)),
        None,
        gpui::Modifiers::none(),
    );
    vcx.run_until_parked();
    assert_eq!(tips(vcx, left + 9., y), ["Remove"], "over the icon");
    let tree = tree(vcx);
    let b = named(&nodes(&tree), Role::Button, "Claude Opus");
    assert_eq!((b.label(), b.description()), before);
    assert_eq!(before.1.as_deref(), Some("Remove"));
}
