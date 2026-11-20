//! Animations for looks (`view::motion`, the Windows pass's W6): the
//! running dot pulses 20 steps a second only while the window is active and
//! on a hardware adapter; inactive, nothing is drawn (the dot rests, the
//! working line's clock stops); on a software adapter the dot never steps
//! and only the clock's second (and the focused composer's caret) redraw.

mod common;

use std::time::Duration;

use common::*;
use gpui::{TestAppContext, VisualTestContext};
use mcbench_ui::state::place::Place;
use mcbench_ui::view::motion::{Adapter, set_adapter};

fn frames(vcx: &mut VisualTestContext, seconds: u64) -> usize {
    vcx.update(|window, _| window.take_a11y_updates_sent_for_test());
    for _ in 0..seconds * 20 {
        vcx.executor().advance_clock(Duration::from_millis(50));
        vcx.run_until_parked();
    }
    vcx.update(|window, _| window.take_a11y_updates_sent_for_test())
        .len()
}

fn running_chat<'a>(
    cx: &'a mut TestAppContext,
    test: &str,
) -> (ShellUnderTest, &'a mut VisualTestContext) {
    let (shell, core, vcx) = open_standin(
        cx,
        test,
        gate_fixture(),
        Some(Place::Chat {
            id: "chat-gate".into(),
        }),
    );
    core.set_turn("chat-gate", true, false, None);
    pump(&shell, vcx);
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
    pump(&shell, vcx);
    (shell, vcx)
}

#[gpui::test]
fn the_dot_pulses_only_in_an_active_window_on_a_hardware_adapter(cx: &mut TestAppContext) {
    let (shell, vcx) = running_chat(cx, "motion-hardware");
    // Active, hardware (the test platform reports no adapter): 20 a second.
    vcx.update(|window, _| window.activate_window());
    pump(&shell, vcx);
    let active = frames(vcx, 1);
    assert!(active >= 18, "the pulse steps: {active} frames in 1 s");
    // Inactive: nothing at all, the clock included.
    vcx.deactivate_window();
    pump(&shell, vcx);
    // The step already asked for lands (one frame), then nothing.
    vcx.executor().advance_clock(Duration::from_millis(100));
    vcx.run_until_parked();
    let inactive = frames(vcx, 3);
    assert_eq!(inactive, 0, "an inactive window draws nothing");
    // Active again: it moves again.
    vcx.update(|window, _| window.activate_window());
    pump(&shell, vcx);
    assert!(frames(vcx, 1) >= 18);
}

#[gpui::test]
fn on_a_software_adapter_the_dot_rests_and_only_the_clock_ticks(cx: &mut TestAppContext) {
    let (shell, vcx) = running_chat(cx, "motion-software");
    vcx.update(|_, cx| set_adapter(cx, Adapter::Software));
    vcx.update(|window, _| window.activate_window());
    vcx.update(|window, _| window.refresh());
    pump(&shell, vcx);
    let software = frames(vcx, 3);
    // The clock's second and the composer's caret (500 ms, not an
    // animation for looks), 3 a second, against the pulse's 20.
    assert!(
        software <= 9,
        "no pulse; the clock and the caret alone: {software} frames in 3 s"
    );
    // Inactive there too: nothing.
    vcx.deactivate_window();
    pump(&shell, vcx);
    vcx.executor().advance_clock(Duration::from_millis(100));
    vcx.run_until_parked();
    assert_eq!(frames(vcx, 3), 0);
}
