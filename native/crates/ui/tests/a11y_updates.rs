//! What the window hands the platform's accessibility adapter frame by frame
//! (vendor patch 36): a frame that changes nothing in the tree hands it
//! nothing, so the sidebar's running dot (20 steps a second) and the working
//! line's clock (a redraw a second) cost the adapter nothing while the tree
//! is unchanged, and a real change still sends one update, of the nodes that
//! changed, in the frame that makes it.

mod common;

use std::time::Duration;

use common::*;
use gpui::VisualTestContext;
use gpui::accesskit::Role;
use mcbench_ui::state::place::Place;

fn sent(vcx: &mut VisualTestContext) -> Vec<Option<usize>> {
    vcx.update(|window, _| window.take_a11y_updates_sent_for_test())
}

fn working_line(vcx: &mut VisualTestContext) -> Option<String> {
    let tree = tree(vcx);
    of_role(&nodes(&tree), Role::Group)
        .iter()
        .filter_map(|n| n.label())
        .find(|l| l.starts_with("Working for "))
}

#[gpui::test]
fn a_pulse_step_or_a_clock_tick_sends_nothing_and_a_change_sends_one(
    cx: &mut gpui::TestAppContext,
) {
    let (shell, core, vcx) = open_standin(
        cx,
        "a11y-updates",
        gate_fixture(),
        Some(Place::Chat {
            id: "chat-gate".into(),
        }),
    );
    // The test platform opens the window inactive; animations move only
    // in an active one (`view::motion`).
    vcx.update(|window, _| window.activate_window());
    // The open chat runs: its row's dot pulses and the working line shows.
    core.set_turn("chat-gate", true, false, None);
    pump(&shell, vcx);
    // The turn as joined (the stand-in at zero delay has none to join).
    let since = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("after 1970")
        .as_millis() as i64;
    vcx.update(|window, cx| {
        shell.entity.update(cx, |s, cx| {
            s.apply(
                mcbench_ui::Update::TurnJoined {
                    chat: "chat-gate".into(),
                    since,
                },
                window,
                cx,
            )
        })
    });
    vcx.update(|window, _| window.refresh());
    vcx.run_until_parked();
    let line = working_line(vcx).expect("the working line");
    let whole = vcx
        .update(|window, _| window.a11y_tree_update_for_test())
        .expect("a tree")
        .nodes
        .len();
    sent(vcx);

    // A second of pulse steps (50 ms each): every one draws a frame and
    // none sends anything.
    for _ in 0..20 {
        vcx.executor().advance_clock(Duration::from_millis(50));
        vcx.run_until_parked();
    }
    let pulse = sent(vcx);
    assert!(
        pulse.len() >= 20,
        "the pulse draws a frame a step: {pulse:?}"
    );
    // The clock's tick fell in that second too. The line's text is the
    // wall clock's: within the same second it is unchanged and nothing is
    // sent; when the second turned, one update of the line alone.
    let after = working_line(vcx).expect("the working line");
    if after == line {
        assert!(pulse.iter().all(Option::is_none), "{pulse:?}");
    } else {
        let some: Vec<usize> = pulse.iter().flatten().copied().collect();
        assert_eq!(some.len(), 1, "{pulse:?}");
        assert!(some[0] <= 3, "only the line's nodes: {pulse:?} of {whole}");
    }

    // A real change (a key in the composer) sends one update in the frame
    // that makes it, of the nodes that changed only.
    vcx.simulate_input("x");
    vcx.run_until_parked();
    let typed = sent(vcx);
    let some: Vec<usize> = typed.iter().flatten().copied().collect();
    assert_eq!(some.len(), 1, "one update: {typed:?}");
    // The frames before the key reached the composer sent nothing; the
    // last frame drawn, which holds the key, sent it.
    assert!(typed.last().is_some_and(Option::is_some), "{typed:?}");
    let tree = tree(vcx);
    let composer = named(&nodes(&tree), Role::MultilineTextInput, "Message");
    assert_eq!(composer.value().as_deref(), Some("x"));
    assert!(
        some.iter().all(|n| *n < whole / 4),
        "only what changed: {typed:?} of {whole}"
    );
    eprintln!("whole tree {whole} nodes; pulse second {pulse:?}; typed {typed:?}");
}
