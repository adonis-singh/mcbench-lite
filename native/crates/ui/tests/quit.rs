//! quit-ask: the question before the person's quit while builds run, in
//! process on the stand-in. The quit key and the menu's `Quit` (the
//! `Quit` action), the Window menu's `Close` (`CloseWindow`) and the
//! window's close request ask while a build runs and quit at once with
//! none; a quit the code asks for (`App::quit`) never asks. A quit taken is
//! the action log's `quit` line (the test platform's quit does nothing).

mod common;

use common::*;
use gpui::VisualTestContext;
use gpui::accesskit::Role;
use mcbench_ui::view::{CloseWindow, Quit};

const MOD: &str = if cfg!(target_os = "macos") {
    "cmd"
} else {
    "ctrl"
};

const ONE: (&str, &str) = (
    "A build is running",
    "Quitting stops it. What it built so far stays in the chat.",
);
const TWO: (&str, &str) = (
    "2 builds are running",
    "Quitting stops them. What they built so far stays in their chats.",
);

/// The quit paths that ask.
#[derive(Clone, Copy, Debug)]
enum How {
    /// Cmd-Q, Ctrl+Q.
    Key,
    /// The app menu's `Quit mcbench-lite` (the `Quit` action).
    Menu,
    /// The window's close request (title bar, WM_DELETE_WINDOW, ...).
    Close,
    /// The Window menu's `Close`, Cmd-W.
    CloseWindow,
}

const HOWS: [How; 4] = [How::Key, How::Menu, How::Close, How::CloseWindow];

/// Asks to quit `how`; for the close request, whether the window may close.
fn ask(vcx: &mut VisualTestContext, how: How) -> Option<bool> {
    let closes = match how {
        How::Key => {
            vcx.simulate_keystrokes(&format!("{MOD}-q"));
            None
        }
        How::Menu => {
            vcx.dispatch_action(Quit);
            None
        }
        How::Close => Some(vcx.simulate_close()),
        How::CloseWindow => {
            vcx.dispatch_action(CloseWindow);
            None
        }
    };
    vcx.run_until_parked();
    closes
}

/// The action log's quit lines (`quit`, `quit asked`, `quit kept`, ...).
fn quit_lines(shell: &ShellUnderTest) -> Vec<serde_json::Value> {
    log_lines(&shell.log)
        .into_iter()
        .filter(|l| l["action"].as_str().unwrap().starts_with("quit"))
        .collect()
}

fn question(vcx: &mut VisualTestContext) -> Option<(String, String)> {
    let tree = tree(vcx);
    let nodes = nodes(&tree);
    let dialogs = of_role(&nodes, Role::AlertDialog);
    assert!(dialogs.len() <= 1, "one question at most");
    dialogs
        .first()
        .map(|d| (d.label().unwrap(), d.description().unwrap()))
}

fn one_running<'a>(
    cx: &'a mut gpui::TestAppContext,
    test: &str,
) -> (ShellUnderTest, &'a mut VisualTestContext) {
    let (shell, core, vcx) = open_standin(cx, test, gate_fixture(), None);
    core.set_turn("chat-gate", true, false, None);
    pump(&shell, vcx);
    (shell, vcx)
}

#[gpui::test]
fn with_nothing_running_each_quit_path_quits_without_a_question(cx: &mut gpui::TestAppContext) {
    let (shell, _core, vcx) = open_standin(cx, "quit-none", gate_fixture(), None);
    // The close request last: it closes the window.
    for (i, how) in [How::Key, How::Menu, How::Close].into_iter().enumerate() {
        let closes = ask(vcx, how);
        assert_eq!(question(vcx), None, "{how:?}");
        if let How::Close = how {
            assert_eq!(closes, Some(true));
        }
        let lines = quit_lines(&shell);
        assert_eq!(lines.len(), i + 1, "{how:?}: {lines:?}");
        assert_eq!(lines[i]["asked"], false, "{how:?}");
    }
}

#[gpui::test]
fn the_window_menus_close_with_nothing_running_closes_without_a_question(
    cx: &mut gpui::TestAppContext,
) {
    let (shell, _core, vcx) = open_standin(cx, "quit-none-close", gate_fixture(), None);
    ask(vcx, How::CloseWindow);
    let lines = quit_lines(&shell);
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert_eq!(lines[0]["how"], "close-window");
    assert_eq!(lines[0]["asked"], false);
}

#[gpui::test]
fn with_one_build_running_each_asking_path_asks_with_focus_on_keep_building(
    cx: &mut gpui::TestAppContext,
) {
    let (shell, vcx) = one_running(cx, "quit-one");
    for how in HOWS {
        let before = focused(vcx);
        let closes = ask(vcx, how);
        if let How::Close = how {
            assert_eq!(closes, Some(false), "the window stays open");
        }
        assert_eq!(question(vcx), Some((ONE.0.into(), ONE.1.into())), "{how:?}");
        let tree = tree(vcx);
        let nodes = nodes(&tree);
        assert!(has(&nodes, Role::Button, "Quit"), "{how:?}");
        assert_eq!(
            focused(vcx),
            (Role::Button, Some("Keep building".into())),
            "{how:?}"
        );
        // Escape keeps the app open, focus back where it was.
        vcx.simulate_keystrokes("escape");
        vcx.run_until_parked();
        assert_eq!(question(vcx), None, "{how:?}");
        assert_eq!(focused(vcx), before, "{how:?}");
    }
    assert!(
        quit_lines(&shell).iter().all(|l| l["action"] != "quit"),
        "nothing quit"
    );
}

#[gpui::test]
fn two_builds_a_chat_and_a_bench_model_ask_in_the_plural(cx: &mut gpui::TestAppContext) {
    let (shell, core, vcx) = open_with_lists(cx, "quit-two", true, None, false);
    core.set_turn("chat-gate", true, false, None);
    pump(&shell, vcx);
    ask(vcx, How::Key);
    assert_eq!(question(vcx), Some((TWO.0.into(), TWO.1.into())));
}

#[gpui::test]
fn keep_building_escape_and_a_second_request_leave_the_app_open(cx: &mut gpui::TestAppContext) {
    let (shell, vcx) = one_running(cx, "quit-keep");
    let before = focused(vcx);
    // Keep building, by the keyboard (focus is on it).
    ask(vcx, How::Key);
    vcx.simulate_keystrokes("enter");
    vcx.run_until_parked();
    assert_eq!(question(vcx), None);
    assert_eq!(focused(vcx), before);
    // Keep building, by an assistive technology's press.
    ask(vcx, How::Menu);
    let tree = tree(vcx);
    let keep = named(&nodes(&tree), Role::Button, "Keep building");
    click(vcx, keep);
    assert_eq!(question(vcx), None);
    assert_eq!(focused(vcx), before);
    // A second request while it is up does nothing new: one question, the
    // window kept, focus where it is.
    ask(vcx, How::Key);
    vcx.simulate_keystrokes("tab");
    vcx.run_until_parked();
    assert_eq!(focused(vcx), (Role::Button, Some("Quit".into())));
    for how in HOWS {
        let closes = ask(vcx, how);
        if let How::Close = how {
            assert_eq!(closes, Some(false));
        }
        assert_eq!(question(vcx), Some((ONE.0.into(), ONE.1.into())), "{how:?}");
        assert_eq!(focused(vcx), (Role::Button, Some("Quit".into())), "{how:?}");
    }
    // Tab stays inside the question.
    vcx.simulate_keystrokes("tab");
    vcx.run_until_parked();
    assert_eq!(focused(vcx), (Role::Button, Some("Keep building".into())));
    vcx.simulate_keystrokes("shift-tab");
    vcx.run_until_parked();
    assert_eq!(focused(vcx), (Role::Button, Some("Quit".into())));
    vcx.simulate_keystrokes("escape");
    vcx.run_until_parked();
    assert_eq!(question(vcx), None);
    assert_eq!(focused(vcx), before);
    let lines = quit_lines(&shell);
    assert!(lines.iter().all(|l| l["action"] != "quit"), "{lines:?}");
    assert_eq!(
        lines
            .iter()
            .filter(|l| l["action"] == "quit asked again")
            .count(),
        4
    );
}

#[gpui::test]
fn the_question_stays_when_the_build_ends_and_quit_quits(cx: &mut gpui::TestAppContext) {
    let (shell, core, vcx) = open_standin(cx, "quit-quit", gate_fixture(), None);
    core.set_turn("chat-gate", true, false, None);
    pump(&shell, vcx);
    ask(vcx, How::Close);
    core.set_turn("chat-gate", false, true, None);
    pump(&shell, vcx);
    assert_eq!(question(vcx), Some((ONE.0.into(), ONE.1.into())));
    let tree = tree(vcx);
    let go = named(&nodes(&tree), Role::Button, "Quit");
    click(vcx, go);
    let lines = quit_lines(&shell);
    let last = lines.last().unwrap();
    assert_eq!(last["action"], "quit");
    assert_eq!(last["asked"], true);
}

#[gpui::test]
fn a_quit_the_code_asks_for_never_asks(cx: &mut gpui::TestAppContext) {
    let (shell, vcx) = one_running(cx, "quit-code");
    vcx.update(|_, cx| cx.quit());
    vcx.run_until_parked();
    assert_eq!(question(vcx), None);
    assert!(quit_lines(&shell).is_empty());
}
