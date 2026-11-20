//! The window's `first_frame` line (pane agent): written once, as the
//! window paints its first frame (not at quit), before any `metrics`
//! line, counted from the process's start (`crash::process_started`, taken
//! in `crash::install`).

mod common;

use common::*;

#[gpui::test]
fn the_first_frame_is_logged_once_as_it_paints(cx: &mut gpui::TestAppContext) {
    quiet_crashes();
    let started = mcbench_ui::crash::process_started();
    let before_ms = started.elapsed().as_secs_f64() * 1e3;
    let (shell, vcx) = open_shell(cx, "first-frame", Fixture::Gate, None);
    // More frames: still one line.
    for _ in 0..3 {
        vcx.update(|window, _| window.refresh());
        vcx.run_until_parked();
    }
    let lines = log_lines(&shell.log);
    let firsts: Vec<_> = lines
        .iter()
        .filter(|l| l["action"] == "first_frame")
        .collect();
    assert_eq!(firsts.len(), 1, "one first_frame line: {lines:?}");
    let first = firsts[0];
    let since = first["since_start_ms"].as_f64().expect("since_start_ms");
    assert!(
        since >= before_ms,
        "{since} ms from the start, at least the {before_ms} ms before the window opened"
    );
    assert!(first["epoch_ms"].as_u64().expect("epoch_ms") > 1_700_000_000_000);
    let at = lines
        .iter()
        .position(|l| l["action"] == "first_frame")
        .expect("its place");
    assert!(
        lines[..at].iter().all(|l| l["action"] != "metrics"),
        "no metrics line before it"
    );
    assert!(
        lines.iter().all(|l| l["action"] != "metrics"),
        "metrics is written at quit, not before"
    );
    let _ = vcx;
}
