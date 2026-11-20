//! What the macOS pass of the shell and chat found in shared interface code
//! (chat-fix-2), each proved here in process: every button's tooltip is its
//! description; a reader's Click, Focus and ScrollIntoView on a node in a
//! row the transcript keeps outside its visible part; a failed search said
//! once; and the contract rows nothing showed before: a place not built
//! yet, `Loading…`, `No prompts match.`, a read that failed, the
//! resolution tag. (Set sizes are `tests/sets.rs`; the meter is
//! `tests/pickers.rs`.)

mod common;

use std::time::Duration;

use accesskit_consumer::{FilterResult, Node};
use common::*;
use gpui::accesskit::{Action, ActionRequest, NodeId, Role};
use gpui::{TestAppContext, VisualTestContext};
use mcbench_ui::dev::standin;
use mcbench_ui::state::place::Place;

fn lamp() -> Option<Place> {
    Some(Place::Chat {
        id: "chat-lamp".into(),
    })
}

fn include(_: &Node) -> FilterResult {
    FilterResult::Include
}

fn press(vcx: &mut VisualTestContext, role: Role, label: &str) {
    let tree = tree(vcx);
    click(vcx, named(&nodes(&tree), role, label));
}

fn focus_on(vcx: &mut VisualTestContext, role: Role, label: &str) {
    let tree = tree(vcx);
    act(vcx, named(&nodes(&tree), role, label), Action::Focus);
}

fn labels(vcx: &mut VisualTestContext, role: Role) -> Vec<String> {
    let tree = tree(vcx);
    of_role(&nodes(&tree), role)
        .iter()
        .filter_map(|n| n.label())
        .collect()
}

fn settle(shell: &ShellUnderTest, vcx: &mut VisualTestContext) {
    pump(shell, vcx);
    vcx.executor().advance_clock(Duration::from_millis(200));
    pump(shell, vcx);
}

/// The node the platform knows as `id` in `tree`.
fn by_id<'a>(tree: &'a accesskit_consumer::Tree, id: NodeId) -> Node<'a> {
    nodes(tree)
        .into_iter()
        .find(|n| n.locate().0 == id)
        .unwrap_or_else(|| panic!("node {id:?} is in the tree"))
}

fn request(vcx: &mut VisualTestContext, action: Action, node: NodeId) {
    vcx.update(|window, cx| {
        window.dispatch_a11y_action_for_test(
            ActionRequest {
                action,
                target_tree: gpui::accesskit::TreeId::ROOT,
                target_node: node,
                data: None,
            },
            cx,
        )
    });
    for _ in 0..3 {
        vcx.update(|window, _| window.refresh());
        vcx.run_until_parked();
    }
}

// Item 1: a tooltip is its button's description.

/// Hovers every enabled button on screen for 600 ms; each one a tooltip
/// shows over must be described by that tooltip's text. Returns the
/// buttons that showed one, with their tips.
fn hover_every_button(vcx: &mut VisualTestContext) -> Vec<(String, String)> {
    let targets: Vec<(NodeId, String, gpui::accesskit::Rect)> = {
        let tree = tree(vcx);
        let window = tree.state().root().bounding_box();
        of_role(&nodes(&tree), Role::Button)
            .into_iter()
            .filter(|b| !b.is_disabled() && !b.is_clipped_out())
            .filter_map(|b| {
                let bounds = b.bounding_box()?;
                let inside = window.is_none_or(|w| {
                    bounds.x0 >= w.x0 && bounds.y0 >= w.y0 && bounds.x1 <= w.x1 && bounds.y1 <= w.y1
                });
                (inside && bounds.width() > 0. && bounds.height() > 0.)
                    .then(|| (b.locate().0, b.label().unwrap_or_default(), bounds))
            })
            .collect()
    };
    let mut tipped = Vec::new();
    for (id, label, bounds) in targets {
        // par-shell: a point on the button's own left padding, not its
        // centre: an inner label's tip (`Button::label_tip`, the page's
        // `title` on an inner span) is the label's, by the pointer, and
        // describes no node; the button's own tip shows over the rest.
        let centre = {
            let c = centre_of(vcx, bounds);
            let scale = f64::from(vcx.update(|window, _| window.scale_factor()));
            gpui::point(gpui::px((bounds.x0 / scale) as f32 + 3.), c.y)
        };
        vcx.simulate_mouse_move(centre, None, gpui::Modifiers::none());
        let _ = tree(vcx);
        vcx.executor().advance_clock(Duration::from_millis(600));
        vcx.run_until_parked();
        let tree = tree(vcx);
        let all = nodes(&tree);
        let tips: Vec<String> = of_role(&all, Role::Tooltip)
            .iter()
            .filter_map(|t| t.label())
            .collect();
        if let [tip] = tips.as_slice() {
            let button = by_id(&tree, id);
            // par-shell: a tip that says the button's own name is no
            // description (a browser's accessible description leaves out a
            // `title` equal to the name: the resolution trigger's
            // `Resolution: <stop>`); any other tip is the description.
            let names_it = button.label().as_deref() == Some(tip.as_str());
            assert!(
                button.description().as_deref() == Some(tip.as_str())
                    || (names_it && button.description().is_none()),
                "the button {label:?} shows the tip {tip:?}: it is its description, or its name"
            );
            tipped.push((label.clone(), tip.clone()));
        }
        assert!(
            tips.len() <= 1,
            "one tip at a time over {label:?}: {tips:?}"
        );
    }
    vcx.simulate_mouse_move(
        gpui::point(gpui::px(1.), gpui::px(1.)),
        None,
        gpui::Modifiers::none(),
    );
    vcx.run_until_parked();
    tipped
}

#[gpui::test]
fn every_button_with_a_tooltip_is_described_by_it(cx: &mut TestAppContext) {
    let (shell, vcx) = open_shell(cx, "tips-chat", Fixture::Gate, lamp());
    vcx.simulate_resize(gpui::size(gpui::px(1280.), gpui::px(820.)));
    settle(&shell, vcx);
    // Editing a message shows Cancel edit, whose tip the macOS pass found
    // missing from its description.
    {
        let tree = tree(vcx);
        let last = of_role(&nodes(&tree), Role::Button)
            .into_iter()
            .rfind(|b| b.label().as_deref() == Some("Edit and branch"))
            .expect("an Edit and branch button");
        click(vcx, last);
    }
    settle(&shell, vcx);
    let tree1 = tree(vcx);
    let cancel = named(&nodes(&tree1), Role::Button, "Cancel edit");
    assert_eq!(cancel.description().as_deref(), Some("Cancel edit (Esc)"));
    let tipped = hover_every_button(vcx);
    assert!(
        tipped
            .iter()
            .any(|(b, t)| b == "Cancel edit" && t == "Cancel edit (Esc)"),
        "{tipped:?}"
    );
    assert!(tipped.len() >= 5, "buttons with tips: {tipped:?}");
}

#[gpui::test]
fn every_button_with_a_tooltip_in_the_gallery_is_described_by_it(cx: &mut TestAppContext) {
    quiet_crashes();
    let dir = TempDir::new("tips-gallery");
    let log = mcbench_ui::ActionLog::create(&dir.0.join("actions.jsonl"));
    cx.update(mcbench_ui::view::setup);
    let (_, vcx) = cx.add_window_view(move |window, cx| {
        window.activate_a11y_for_test();
        mcbench_ui::dev::gallery::Gallery::new(Some(log), cx)
    });
    vcx.run_until_parked();
    let tipped = hover_every_button(vcx);
    assert!(tipped.len() >= 3, "buttons with tips: {tipped:?}");
}

// Item 2: a node in a row kept outside the transcript's visible part.

/// The transcript list's visible top and bottom, device pixels: the
/// region's bounds.
fn viewport(vcx: &mut VisualTestContext) -> (f64, f64) {
    let tree = tree(vcx);
    let region = named(&nodes(&tree), Role::Region, "Conversation")
        .bounding_box()
        .expect("bounds");
    (region.y0, region.y1)
}

/// Whether `id`'s centre is inside the transcript's visible part.
fn in_view(vcx: &mut VisualTestContext, id: NodeId) -> bool {
    let (top, bottom) = viewport(vcx);
    let tree = tree(vcx);
    let b = by_id(&tree, id).bounding_box().expect("bounds");
    let centre = (b.y0 + b.y1) / 2.;
    centre > top && centre < bottom
}

/// A button of a transcript row kept wholly above the list's visible part
/// (its id, its row's id and its name), after showing each row from the
/// chat's end up until a row above holds one.
fn button_above(shell: &ShellUnderTest, vcx: &mut VisualTestContext) -> (NodeId, NodeId, String) {
    let pane = vcx.update(|_, cx| shell.entity.read(cx).chat());
    let rows = vcx.update(|_, cx| pane.read(cx).row_count());
    for at in (1..rows).rev() {
        vcx.update(|window, cx| pane.update(cx, |p, cx| p.reveal_row(at, window, cx)));
        for _ in 0..3 {
            vcx.update(|window, _| window.refresh());
            vcx.run_until_parked();
        }
        let (top, _) = viewport(vcx);
        let tree = tree(vcx);
        for article in of_role(&nodes(&tree), Role::Article) {
            if article.bounding_box().expect("bounds").y1 > top {
                continue;
            }
            let mut inside = Vec::new();
            fn walk<'a>(n: Node<'a>, out: &mut Vec<Node<'a>>) {
                for c in n.children() {
                    out.push(c);
                    walk(c, out);
                }
            }
            walk(article, &mut inside);
            if let Some(button) = inside
                .iter()
                .find(|n| n.role() == Role::Button && !n.is_disabled())
            {
                return (
                    button.locate().0,
                    article.locate().0,
                    button.label().unwrap_or_default(),
                );
            }
        }
    }
    panic!("no row kept above the visible part holds a button");
}

fn small_lamp<'a>(
    cx: &'a mut TestAppContext,
    test: &str,
) -> (ShellUnderTest, &'a mut VisualTestContext) {
    let (shell, vcx) = open_shell(cx, test, Fixture::Gate, lamp());
    vcx.simulate_resize(gpui::size(gpui::px(1000.), gpui::px(380.)));
    settle(&shell, vcx);
    (shell, vcx)
}

#[gpui::test]
fn every_node_of_a_transcript_row_offers_scroll_into_view_and_it_brings_the_row_in(
    cx: &mut TestAppContext,
) {
    let (shell, vcx) = small_lamp(cx, "offscreen-scroll");
    {
        let tree = tree(vcx);
        let all = nodes(&tree);
        for article in of_role(&all, Role::Article) {
            assert!(article.supports_action(Action::ScrollIntoView, &include));
            fn walk(n: Node<'_>) {
                for c in n.children() {
                    assert!(
                        c.supports_action(Action::ScrollIntoView, &include),
                        "{:?} {:?} in a row offers ScrollIntoView",
                        c.role(),
                        c.label()
                    );
                    walk(c);
                }
            }
            walk(article);
        }
    }
    let (button, row, label) = button_above(&shell, vcx);
    assert!(!in_view(vcx, button), "{label} starts out of view");
    request(vcx, Action::ScrollIntoView, button);
    assert!(
        in_view(vcx, row) || in_view(vcx, button),
        "ScrollIntoView on {label} brings its row in"
    );
    assert!(in_view(vcx, button), "and {label} with it");
}

#[gpui::test]
fn a_click_on_a_button_of_a_row_out_of_view_scrolls_it_in_and_presses_it(cx: &mut TestAppContext) {
    let (shell, vcx) = small_lamp(cx, "offscreen-click");
    let (button, _, label) = button_above(&shell, vcx);
    let before = log_lines(&shell.log).len();
    let expanded_before = {
        let tree = tree(vcx);
        by_id(&tree, button).data().is_expanded()
    };
    request(vcx, Action::Click, button);
    pump(&shell, vcx);
    let after = log_lines(&shell.log);
    let shown = in_view(vcx, button);
    let at = {
        let tree = tree(vcx);
        by_id(&tree, button).bounding_box()
    };
    let view = viewport(vcx);
    assert!(
        after.len() > before,
        "the click on {label} did something: {:?} (in view {shown}, at {at:?}, viewport {view:?})",
        &after[before.min(after.len())..]
    );
    assert!(in_view(vcx, button), "{label} is in view");
    let tree = tree(vcx);
    let now = by_id(&tree, button);
    if let Some(was) = expanded_before {
        assert_eq!(now.data().is_expanded(), Some(!was), "{label} toggled");
    }
}

#[gpui::test]
fn focus_on_a_button_of_a_row_out_of_view_scrolls_it_in_and_focuses_it(cx: &mut TestAppContext) {
    let (shell, vcx) = small_lamp(cx, "offscreen-focus");
    let (button, row, label) = button_above(&shell, vcx);
    request(vcx, Action::Focus, button);
    {
        let tree = tree(vcx);
        let focus = tree.state().focus().expect("a focused node");
        assert_eq!(focus.locate().0, button, "{label} has focus");
    }
    assert!(in_view(vcx, button), "{label} is in view");
    let _ = row;
}

// Item 6: a failed search is said once.

fn polite(vcx: &mut VisualTestContext) -> Option<String> {
    let tree = tree(vcx);
    let all = nodes(&tree);
    of_role(&all, Role::Status)
        .into_iter()
        .find(|s| s.live() == gpui::accesskit::Live::Polite)
        .and_then(|s| s.label())
}

#[gpui::test]
fn a_failed_search_is_said_once_when_it_appears(cx: &mut TestAppContext) {
    let (shell, core, vcx) = open_standin(cx, "failed-search-said", gate_fixture(), lamp());
    settle(&shell, vcx);
    core.fail_searches(Some(standin::SEARCH_FAILURE.to_string()));
    focus_on(vcx, Role::TextInput, "Search chats");
    vcx.simulate_input("zz");
    settle(&shell, vcx);
    assert!(labels(vcx, Role::Label).contains(&standin::SEARCH_FAILURE.to_string()));
    assert_eq!(polite(vcx).as_deref(), Some(standin::SEARCH_FAILURE));
    // Something else is said; the next query fails the same way and the
    // sentence is not said again.
    vcx.update(|window, cx| {
        shell.entity.update(cx, |s, cx| {
            s.go(
                Place::Chat {
                    id: "chat-gate".into(),
                },
                window,
                cx,
            )
        })
    });
    settle(&shell, vcx);
    assert_eq!(polite(vcx).as_deref(), Some("Castle gate"));
    focus_on(vcx, Role::TextInput, "Search chats");
    vcx.simulate_input("q");
    settle(&shell, vcx);
    assert_eq!(
        polite(vcx).as_deref(),
        Some("Castle gate"),
        "the same failure again is not said again"
    );
    // A search that answers ends it; a failure after it is said again.
    core.fail_searches(None);
    focus_on(vcx, Role::TextInput, "Search chats");
    vcx.simulate_input("r");
    settle(&shell, vcx);
    core.fail_searches(Some(standin::SEARCH_FAILURE.to_string()));
    focus_on(vcx, Role::TextInput, "Search chats");
    vcx.simulate_input("s");
    settle(&shell, vcx);
    assert_eq!(polite(vcx).as_deref(), Some(standin::SEARCH_FAILURE));
}

#[gpui::test]
fn a_failed_search_in_the_palette_is_said(cx: &mut TestAppContext) {
    let (shell, core, vcx) = open_standin(cx, "failed-palette-said", gate_fixture(), lamp());
    settle(&shell, vcx);
    core.fail_searches(Some(standin::SEARCH_FAILURE.to_string()));
    vcx.update(|window, cx| {
        let sidebar = shell.entity.read(cx).sidebar();
        sidebar.update(cx, |s, cx| s.toggle_palette(window, cx));
    });
    settle(&shell, vcx);
    vcx.simulate_input("gate");
    settle(&shell, vcx);
    assert!(labels(vcx, Role::Label).contains(&standin::SEARCH_FAILURE.to_string()));
    assert_eq!(polite(vcx).as_deref(), Some(standin::SEARCH_FAILURE));
}

// Item 7: the rows nothing showed.

#[gpui::test]
fn a_place_not_built_yet_is_a_region_named_by_its_title(cx: &mut TestAppContext) {
    let (shell, vcx) = open_shell(cx, "unbuilt-place", Fixture::Gate, lamp());
    settle(&shell, vcx);
    for (place, title) in [
        (Place::NewBench, "New bench"),
        (Place::Leaderboard, "Leaderboard"),
        (Place::Prompt, "Prompts"),
    ] {
        vcx.update(|window, cx| shell.entity.update(cx, |s, cx| s.go(place, window, cx)));
        settle(&shell, vcx);
        assert!(
            labels(vcx, Role::Region).contains(&title.to_string()),
            "{title}: {:?}",
            labels(vcx, Role::Region)
        );
    }
}

#[gpui::test]
fn the_archived_section_says_loading_until_the_archive_is_read(cx: &mut TestAppContext) {
    let (shell, _core, vcx) = open_standin(cx, "archive-loading", gate_fixture(), lamp());
    settle(&shell, vcx);
    // The core's answer waits in the inbox: the read is under way.
    press(vcx, Role::Button, "Show archived");
    let tree1 = tree(vcx);
    named(&nodes(&tree1), Role::Label, "Loading\u{2026}");
    pump(&shell, vcx);
    let tree2 = tree(vcx);
    assert!(!has(&nodes(&tree2), Role::Label, "Loading\u{2026}"));
    assert!(!labels(vcx, Role::ListItem).is_empty());
}

#[gpui::test]
fn a_prompt_search_that_finds_nothing_says_no_prompts_match(cx: &mut TestAppContext) {
    let (shell, _core, vcx) = open_standin(cx, "no-prompts", gate_fixture(), Some(Place::NewBench));
    settle(&shell, vcx);
    press(vcx, Role::Button, "Search prompts");
    pump(&shell, vcx);
    vcx.simulate_input("zzqq");
    settle(&shell, vcx);
    let tree1 = tree(vcx);
    named(&nodes(&tree1), Role::Label, "No prompts match.");
}

#[gpui::test]
fn a_chat_that_cannot_be_read_says_so_and_can_be_read_again(cx: &mut TestAppContext) {
    let (shell, vcx) = open_shell_with(cx, "unreadable", None, Vec::new(), |core, _| {
        core.fail_reads_of("chat-farm")
    });
    vcx.update(|window, cx| {
        shell.entity.update(cx, |s, cx| {
            s.go(
                Place::Chat {
                    id: "chat-farm".into(),
                },
                window,
                cx,
            )
        })
    });
    settle(&shell, vcx);
    let text = format!("This chat could not be read: {}", standin::UNREADABLE);
    assert!(
        labels(vcx, Role::Alert).contains(&text),
        "{:?}",
        labels(vcx, Role::Alert)
    );
    press(vcx, Role::Button, "Read again");
    settle(&shell, vcx);
    assert!(
        log_lines(&shell.log)
            .iter()
            .any(|l| l["action"] == "read again" && l["chat"] == "chat-farm")
    );
    assert!(labels(vcx, Role::Alert).contains(&text), "it fails again");
}

#[gpui::test]
fn the_resolution_tag_is_a_label_described_by_its_title(cx: &mut TestAppContext) {
    quiet_crashes();
    let dir = TempDir::new("resolution-tag");
    let log = mcbench_ui::ActionLog::create(&dir.0.join("actions.jsonl"));
    cx.update(mcbench_ui::view::setup);
    let (_, vcx) = cx.add_window_view(move |window, cx| {
        window.activate_a11y_for_test();
        mcbench_ui::dev::gallery::Gallery::new(Some(log), cx)
    });
    vcx.run_until_parked();
    let tree = tree(vcx);
    let all = nodes(&tree);
    for (tag, title) in [
        ("2\u{d7}", "Resolution: High (2\u{d7})"),
        ("5\u{d7}", "Resolution: Very high (5\u{d7})"),
        ("10\u{d7}", "Resolution: Maximum (10\u{d7})"),
    ] {
        let node = named(&all, Role::Label, tag);
        assert_eq!(node.description().as_deref(), Some(title));
    }
}
