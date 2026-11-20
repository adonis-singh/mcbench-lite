//! Settings and the setup screen (settings agent), in process over the
//! stand-in: the panel's phases, the navigation, the key field, a refused
//! save, a sign-in with its install, the models list, the shortcut
//! recorder, the danger zone, the published list, the setup screen; and
//! the stand-in's answers to the settings seam (each command ends in the
//! update that carries its state, `Update::Error` first on a failure).

mod common;

use std::sync::Arc;

use common::*;
use gpui::accesskit::{Role, Toggled};
use gpui::{TestAppContext, VisualTestContext};
use mcbench_ui::dev::standin::{
    CLEAR_FAILED, Play, REMOVE_FAILED, SAVE_REFUSED, SHARING_NOT_JOINED, SIGN_IN_URL, StandIn,
    USER_CODE,
};
use mcbench_ui::state::settings::Pane;
use mcbench_ui::{Command, Core as _, Update};

const KEY: &str = "sk-test-0000";

const MOD: &str = if cfg!(target_os = "macos") {
    "cmd"
} else {
    "ctrl"
};

fn settle(shell: &ShellUnderTest, vcx: &mut VisualTestContext) {
    for _ in 0..3 {
        pump(shell, vcx);
    }
}

fn press(vcx: &mut VisualTestContext, role: Role, name: &str) {
    let tree = tree(vcx);
    click(vcx, named(&nodes(&tree), role, name));
}

fn starting<'a>(
    nodes: &[accesskit_consumer::Node<'a>],
    role: Role,
    prefix: &str,
) -> Vec<accesskit_consumer::Node<'a>> {
    nodes
        .iter()
        .copied()
        .filter(|n| n.role() == role && n.label().is_some_and(|l| l.starts_with(prefix)))
        .collect()
}

fn open(shell: &ShellUnderTest, vcx: &mut VisualTestContext, pane: Option<Pane>) {
    vcx.update(|window, cx| {
        shell
            .entity
            .update(cx, |s, cx| s.open_settings_on(pane, None, window, cx))
    });
    settle(shell, vcx);
}

fn tab_selected(vcx: &mut VisualTestContext, prefix: &str) -> bool {
    let tree = tree(vcx);
    let nodes = nodes(&tree);
    let tabs = starting(&nodes, Role::Tab, prefix);
    assert_eq!(tabs.len(), 1, "one tab {prefix:?}");
    tabs[0].is_selected() == Some(true)
}

fn with_plays<'a>(
    cx: &'a mut TestAppContext,
    test: &str,
    plays: &[Play],
) -> (ShellUnderTest, Arc<StandIn>, &'a mut VisualTestContext) {
    let (shell, core, vcx) = open_standin(cx, test, gate_fixture(), None);
    for play in plays {
        core.settings_play(*play);
    }
    // The plays' providers reach the window as the core would publish them.
    core.send(Command::ReadSettings { refresh: true });
    settle(&shell, vcx);
    (shell, core, vcx)
}

fn has_text(vcx: &mut VisualTestContext, text: &str) -> bool {
    let tree = tree(vcx);
    nodes(&tree).iter().any(|n| {
        n.label().is_some_and(|l| l.contains(text))
            || n.value().is_some_and(|v| v.contains(text))
            || n.description().is_some_and(|d| d.contains(text))
    })
}

#[gpui::test]
fn settings_open_on_general_by_tab_and_close_to_their_opener(cx: &mut TestAppContext) {
    let (shell, _core, vcx) = open_standin(cx, "settings-open", gate_fixture(), None);
    let tree0 = tree(vcx);
    let opener = named(&nodes(&tree0), Role::Button, "Settings");
    act(vcx, opener, gpui::accesskit::Action::Focus);
    vcx.simulate_keystrokes("enter");
    settle(&shell, vcx);
    let tree1 = tree(vcx);
    let nodes1 = nodes(&tree1);
    let dialog = named(&nodes1, Role::Dialog, "Settings");
    assert!(dialog.is_modal());
    named(&nodes1, Role::TabList, "Settings sections");
    assert!(tab_selected(vcx, "General"));
    assert_eq!(focused(vcx), (Role::Tab, Some("General".into())));
    // Down moves focus and shows nothing; Enter shows.
    vcx.simulate_keystrokes("down");
    assert_eq!(focused(vcx).1.as_deref(), Some("OpenRouter, Key saved"));
    assert!(tab_selected(vcx, "General"));
    vcx.simulate_keystrokes("enter");
    settle(&shell, vcx);
    assert!(tab_selected(vcx, "OpenRouter"));
    // Tab stays inside the panel.
    for _ in 0..60 {
        vcx.simulate_keystrokes("tab");
        let tree = tree(vcx);
        let focus = tree.state().focus().expect("a focused node");
        let mut inside = false;
        let mut node = Some(focus);
        while let Some(n) = node {
            if n.role() == Role::Dialog && n.label().as_deref() == Some("Settings") {
                inside = true;
            }
            node = n.parent();
        }
        assert!(inside, "Tab left Settings for {:?}", focus.label());
    }
    vcx.simulate_keystrokes("escape");
    settle(&shell, vcx);
    let tree2 = tree(vcx);
    assert!(!has(&nodes(&tree2), Role::Dialog, "Settings"));
    assert_eq!(focused(vcx), (Role::Button, Some("Settings".into())));
}

#[gpui::test]
fn the_key_field_is_a_password_field_whose_value_is_nowhere_and_it_saves(cx: &mut TestAppContext) {
    let (shell, _core, vcx) = open_standin(cx, "settings-key", gate_fixture(), None);
    open(&shell, vcx, Some(Pane::Provider("openai".into())));
    let tree1 = tree(vcx);
    let field = named(&nodes(&tree1), Role::PasswordInput, "API key");
    act(vcx, field, gpui::accesskit::Action::Focus);
    vcx.simulate_input(KEY);
    settle(&shell, vcx);
    assert!(!has_text(vcx, KEY), "the tree holds no typed key");
    let debug =
        vcx.update(|_, cx| format!("{:?}", shell.entity.read(cx).settings().read(cx).panel()));
    assert!(
        !debug.contains(KEY) && debug.contains("***"),
        "Debug prints ***"
    );
    // Copy out of it is refused.
    vcx.simulate_keystrokes(&format!("{MOD}-a {MOD}-c"));
    let copied = vcx.update(|_, cx| cx.read_from_clipboard().and_then(|c| c.text()));
    assert!(!copied.is_some_and(|c| c.contains(KEY)));
    press(vcx, Role::Button, "Save changes");
    settle(&shell, vcx);
    let tree2 = tree(vcx);
    assert!(
        !has(&nodes(&tree2), Role::Dialog, "Settings"),
        "a save closes them"
    );
    open(&shell, vcx, None);
    let tree3 = tree(vcx);
    assert_eq!(
        starting(&nodes(&tree3), Role::Tab, "OpenAI, Key saved").len(),
        1
    );
    assert!(
        !log_lines(&shell.log)
            .iter()
            .any(|l| l.to_string().contains(KEY))
    );
}

#[gpui::test]
fn a_refused_save_opens_again_with_its_error_and_a_cancel_drops_the_draft(cx: &mut TestAppContext) {
    let (shell, _core, vcx) = with_plays(cx, "settings-refused", &[Play::SaveRefused]);
    open(&shell, vcx, None);
    press(vcx, Role::Switch, "Offer QwenCloud in the model picker");
    press(vcx, Role::Button, "Save changes");
    settle(&shell, vcx);
    let tree1 = tree(vcx);
    let nodes1 = nodes(&tree1);
    named(&nodes1, Role::Dialog, "Settings");
    named(&nodes1, Role::Alert, SAVE_REFUSED);
    // The draft is kept with the error.
    let qwen = named(&nodes1, Role::Switch, "Offer QwenCloud in the model picker");
    assert_eq!(qwen.toggled(), Some(Toggled::True));
    press(vcx, Role::Button, "Dismiss error");
    let tree2 = tree(vcx);
    assert!(!has(&nodes(&tree2), Role::Alert, SAVE_REFUSED));
    press(vcx, Role::Button, "Cancel");
    settle(&shell, vcx);
    open(&shell, vcx, None);
    let tree3 = tree(vcx);
    let qwen = named(
        &nodes(&tree3),
        Role::Switch,
        "Offer QwenCloud in the model picker",
    );
    assert_eq!(
        qwen.toggled(),
        Some(Toggled::False),
        "Cancel dropped the draft"
    );
}

#[gpui::test]
fn a_claude_code_sign_in_installs_first_then_waits_for_the_pasted_code(cx: &mut TestAppContext) {
    let (shell, core, vcx) = with_plays(cx, "settings-claude", &[Play::ClaudeSignIn]);
    open(&shell, vcx, Some(Pane::Provider("claude-code".into())));
    assert!(has_text(
        vcx,
        "The app installs Claude Code 2.1.311 first, a 236 MB download."
    ));
    press(vcx, Role::Button, "Claude subscription");
    settle(&shell, vcx);
    assert!(has_text(
        vcx,
        "Installing Claude Code 2.1.311. Starting the download. The sign-in starts when it is done."
    ));
    core.settings_tick();
    settle(&shell, vcx);
    assert!(has_text(
        vcx,
        "Downloading, 0 of 236 MB. The sign-in starts when it is done."
    ));
    core.settings_tick();
    settle(&shell, vcx);
    assert!(has_text(
        vcx,
        "Downloading, 60 of 236 MB. The sign-in starts when it is done."
    ));
    for _ in 0..4 {
        core.settings_tick();
        settle(&shell, vcx);
    }
    // Installed: Settings read again, the remembered sign-in starts.
    assert!(
        has_text(vcx, "Starting the sign-in"),
        "the sign-in followed the install"
    );
    core.settings_tick();
    settle(&shell, vcx);
    let tree1 = tree(vcx);
    named(&nodes(&tree1), Role::Link, SIGN_IN_URL);
    core.settings_tick();
    settle(&shell, vcx);
    let tree2 = tree(vcx);
    let entry = named(
        &nodes(&tree2),
        Role::TextInput,
        "Code from the sign-in page",
    );
    let submit = named(&nodes(&tree2), Role::Button, "Submit");
    assert!(submit.is_disabled(), "Submit waits for a code");
    act(vcx, entry, gpui::accesskit::Action::Focus);
    vcx.simulate_input("abc-123");
    vcx.simulate_keystrokes("enter");
    settle(&shell, vcx);
    let tree3 = tree(vcx);
    let nodes3 = nodes(&tree3);
    named(&nodes3, Role::Button, "Sign out");
    named(&nodes3, Role::Group, "builder@example.com");
    assert_eq!(
        starting(
            &nodes3,
            Role::Tab,
            "Claude Code v2.1.311, Signed in \u{b7} Max"
        )
        .len(),
        1
    );
}

#[gpui::test]
fn a_chatgpt_sign_in_shows_its_one_time_code_and_can_be_cancelled(cx: &mut TestAppContext) {
    let (shell, core, vcx) = with_plays(cx, "settings-chatgpt", &[Play::ChatgptSignIn]);
    open(&shell, vcx, Some(Pane::Connect));
    let tree0 = tree(vcx);
    click(
        vcx,
        starting(&nodes(&tree0), Role::Button, "ChatGPT, Sign in")[0],
    );
    settle(&shell, vcx);
    assert!(tab_selected(vcx, "ChatGPT"), "Connect opened its pane");
    assert!(
        focused(vcx).1.is_some_and(|n| n.starts_with("ChatGPT")),
        "a reader's press puts focus on the opened pane's tab: {:?}",
        focused(vcx)
    );
    core.settings_tick();
    settle(&shell, vcx);
    let tree1 = tree(vcx);
    named(&nodes(&tree1), Role::Label, USER_CODE);
    let account = named(&nodes(&tree1), Role::Group, "Account");
    let cancel: Vec<_> = nodes(&tree1)
        .into_iter()
        .filter(|n| n.role() == Role::Button && n.label().as_deref() == Some("Cancel"))
        .filter(|n| {
            let mut p = n.parent();
            while let Some(q) = p {
                if q.id() == account.id() {
                    return true;
                }
                p = q.parent();
            }
            false
        })
        .collect();
    click(vcx, cancel[0]);
    settle(&shell, vcx);
    let tree2 = tree(vcx);
    named(&nodes(&tree2), Role::Button, "Sign in");
}

#[gpui::test]
fn failures_say_why_where_they_happened(cx: &mut TestAppContext) {
    let (shell, core, vcx) = with_plays(
        cx,
        "settings-failures",
        &[
            Play::ClaudeSignIn,
            Play::InstallFails,
            Play::RemoveFails,
            Play::ClearFails,
            Play::VotesOne,
        ],
    );
    open(&shell, vcx, Some(Pane::Provider("claude-code".into())));
    press(vcx, Role::Button, "Claude subscription");
    for _ in 0..4 {
        core.settings_tick();
        settle(&shell, vcx);
    }
    assert!(
        has_text(vcx, "Claude Code 2.1.311 was not installed:"),
        "a failed install says why"
    );
    open(&shell, vcx, Some(Pane::General));
    press(vcx, Role::Button, "Clear leaderboard");
    let tree1 = tree(vcx);
    named(&nodes(&tree1), Role::Group, "Delete the only vote?");
    press(vcx, Role::Button, "Delete votes");
    settle(&shell, vcx);
    let tree2 = tree(vcx);
    named(&nodes(&tree2), Role::Group, "Delete the only vote?");
    assert!(has_text(vcx, CLEAR_FAILED), "the notice says why");
    let tree3 = tree(vcx);
    click(vcx, starting(&nodes(&tree3), Role::Button, "Remove")[0]);
    press(vcx, Role::Button, "Remove from the web?");
    settle(&shell, vcx);
    let tree4 = tree(vcx);
    named(&nodes(&tree4), Role::Alert, REMOVE_FAILED);
}

#[gpui::test]
fn the_models_list_filters_hides_stars_and_reorders_by_keys(cx: &mut TestAppContext) {
    let (shell, _core, vcx) = open_standin(cx, "settings-models", gate_fixture(), None);
    open(&shell, vcx, Some(Pane::Provider("openrouter".into())));
    let tree1 = tree(vcx);
    let filter = named(&nodes(&tree1), Role::TextInput, "Filter models");
    act(vcx, filter, gpui::accesskit::Action::Focus);
    vcx.simulate_input("claude");
    settle(&shell, vcx);
    press(vcx, Role::Button, "Hide matches");
    let tree2 = tree(vcx);
    named(&nodes(&tree2), Role::Button, "Show matches");
    assert_eq!(
        starting(&nodes(&tree2), Role::Label, "16 of 20 shown").len(),
        1
    );
    for _ in 0..6 {
        vcx.simulate_keystrokes("backspace");
    }
    settle(&shell, vcx);
    let tree3 = tree(vcx);
    let grips = starting(&nodes(&tree3), Role::Button, "Reorder ");
    let first = grips[0].label().expect("a name");
    act(vcx, grips[0], gpui::accesskit::Action::Focus);
    vcx.simulate_keystrokes("down");
    settle(&shell, vcx);
    let tree4 = tree(vcx);
    let grips = starting(&nodes(&tree4), Role::Button, "Reorder ");
    assert_eq!(grips[1].label(), Some(first.clone()));
    assert_eq!(focused(vcx), (Role::Button, Some(first)));
    assert!(has_text(vcx, "moved to 2 of 20"), "the move is said");
    let star = starting(&nodes(&tree4), Role::Button, "Favorite ")[0];
    let before = star.toggled();
    click(vcx, star);
    settle(&shell, vcx);
    let tree5 = tree(vcx);
    assert_ne!(
        starting(&nodes(&tree5), Role::Button, "Favorite ")[0].toggled(),
        before
    );
}

#[gpui::test]
fn the_shortcut_recorder_records_unbinds_the_taken_one_and_escape_cancels(cx: &mut TestAppContext) {
    let (shell, _core, vcx) = open_standin(cx, "settings-binds", gate_fixture(), None);
    open(&shell, vcx, Some(Pane::General));
    let tree1 = tree(vcx);
    let sidebar = starting(&nodes(&tree1), Role::Button, "Show or hide the sidebar")[0];
    act(vcx, sidebar, gpui::accesskit::Action::Focus);
    vcx.simulate_keystrokes("space");
    let tree2 = tree(vcx);
    let recording = starting(&nodes(&tree2), Role::Button, "Show or hide the sidebar")[0];
    assert!(recording.label().unwrap().contains("recording"));
    vcx.simulate_keystrokes("escape");
    let tree3 = tree(vcx);
    named(&nodes(&tree3), Role::Dialog, "Settings");
    assert!(
        starting(&nodes(&tree3), Role::Button, "Show or hide the sidebar")[0]
            .label()
            .unwrap()
            .ends_with(if cfg!(target_os = "macos") {
                "\u{2318}B"
            } else {
                "Ctrl+B"
            })
    );
    vcx.simulate_keystrokes("space");
    vcx.simulate_keystrokes(&format!("{MOD}-k"));
    settle(&shell, vcx);
    let tree4 = tree(vcx);
    assert!(
        starting(&nodes(&tree4), Role::Button, "Search everything")[0]
            .label()
            .unwrap()
            .ends_with("Not set")
    );
    named(&nodes(&tree4), Role::Button, "Reset all");
    let binds = vcx.update(|_, cx| shell.entity.read(cx).preferences().binds.clone());
    assert_eq!(binds.get("sidebar").map(String::as_str), Some("Mod+K"));
    assert_eq!(binds.get("search").map(String::as_str), Some(""));
}

#[gpui::test]
fn the_published_list_shows_the_cores_failure(cx: &mut TestAppContext) {
    let (shell, _core, vcx) = with_plays(cx, "settings-shares", &[Play::SharesFailed]);
    open(&shell, vcx, Some(Pane::General));
    let tree1 = tree(vcx);
    named(&nodes(&tree1), Role::Alert, SHARING_NOT_JOINED);
    named(&nodes(&tree1), Role::Button, "Retry");
}

#[gpui::test]
fn the_setup_screen_counts_fails_and_tries_again(cx: &mut TestAppContext) {
    let (shell, core, vcx) = open_standin(
        cx,
        "settings-setup",
        gate_fixture(),
        Some(mcbench_ui::state::place::Place::NewChat { id: None }),
    );
    core.settings_play(Play::SetupFails);
    settle(&shell, vcx);
    let tree1 = tree(vcx);
    named(&nodes(&tree1), Role::Status, "0 of 2 done");
    for _ in 0..6 {
        core.settings_tick();
        settle(&shell, vcx);
    }
    let tree2 = tree(vcx);
    named(&nodes(&tree2), Role::Alert, "A step failed.");
    named(
        &nodes(&tree2),
        Role::Link,
        "https://www.minecraft.net/en-us/download/server",
    );
    press(vcx, Role::Button, "Try again");
    settle(&shell, vcx);
    let tree3 = tree(vcx);
    assert!(!has(&nodes(&tree3), Role::Alert, "A step failed."));
    for _ in 0..12 {
        core.settings_tick();
        settle(&shell, vcx);
    }
    let tree4 = tree(vcx);
    assert!(
        !has(&nodes(&tree4), Role::Heading, "Getting ready"),
        "ready: the screen goes"
    );
}

// The stand-in's answers to the settings seam.

fn kinds(updates: &[Update]) -> Vec<&'static str> {
    updates
        .iter()
        .map(|u| match u {
            Update::Providers(_) => "providers",
            Update::Favorites(_) => "favorites",
            Update::Settings(Ok(_)) => "settings",
            Update::Settings(Err(_)) => "settings-err",
            Update::SettingsSaved(Ok(())) => "saved",
            Update::SettingsSaved(Err(_)) => "refused",
            Update::SignIn { .. } => "sign-in",
            Update::Install { .. } => "install",
            Update::Leaderboard(_) => "leaderboard",
            Update::Shares(_) => "shares",
            Update::ShareRemoved { .. } => "removed",
            Update::Setup(_) => "setup",
            Update::Error(_) => "error",
            _ => "other",
        })
        .collect()
}

#[test]
fn every_settings_command_ends_in_the_update_that_carries_its_state() {
    let core = StandIn::new(
        gate_fixture(),
        String::new(),
        std::time::Duration::ZERO,
        None,
    );
    let inbox = Arc::new(Inbox::default());
    core.attach(inbox.clone());
    let take = || std::mem::take(&mut *inbox.0.lock().expect("lock"));
    take();
    core.send(Command::ReadSettings { refresh: false });
    assert_eq!(kinds(&take()), ["settings"]);
    core.send(Command::ReadSettings { refresh: true });
    assert_eq!(kinds(&take()), ["providers", "settings"]);
    core.send(Command::SignIn {
        provider: "claude-code".into(),
        method: None,
    });
    assert_eq!(
        kinds(&take()),
        ["error", "sign-in"],
        "a refusal, then the unchanged state"
    );
    core.send(Command::InstallCli {
        program: "claude".into(),
    });
    assert_eq!(
        kinds(&take()),
        ["error", "settings"],
        "installed already, no install ever started: fresh settings, no install"
    );
    core.send(Command::CancelSignIn {
        provider: "codex".into(),
    });
    assert_eq!(
        kinds(&take()),
        ["error", "sign-in", "sign-in"],
        "ChatGPT and Codex CLI share it"
    );
    core.send(Command::SignOut {
        provider: "codex".into(),
    });
    assert_eq!(kinds(&take()), ["providers", "settings"]);
    core.send(Command::RetrySetup);
    assert_eq!(kinds(&take()), ["error", "setup"]);
    core.send(Command::ReadLeaderboard);
    assert_eq!(kinds(&take()), ["leaderboard"]);
    core.send(Command::ClearVotes);
    assert!(matches!(&take()[..], [Update::Leaderboard(Ok(b))] if b.votes == 0));
    core.send(Command::ReadShares);
    assert_eq!(kinds(&take()), ["shares"]);
    core.send(Command::RemoveShare {
        share_id: "5f2a9c01b7de".into(),
    });
    // share agent: the chat's page went with it, `ShareChanged` between
    // (`ui-seam-share.md` answer 4).
    assert_eq!(kinds(&take()), ["removed", "other", "shares"]);
    core.settings_play(Play::SaveRefused);
    core.send(Command::SaveSettings(mcbench_ui::SettingsChange {
        title_model: None,
        share_repo: None,
        providers: Vec::new(),
    }));
    assert_eq!(kinds(&take()), ["refused"]);
    core.settings_play(Play::ReadFails);
    core.send(Command::ReadSettings { refresh: false });
    assert_eq!(kinds(&take()), ["settings-err"]);
    core.settings_play(Play::ClearFails);
    core.send(Command::ClearVotes);
    assert_eq!(kinds(&take()), ["error", "leaderboard"]);
    core.settings_play(Play::SharesFailed);
    core.send(Command::RemoveShare {
        share_id: "9e33c7a0d215".into(),
    });
    assert!(
        matches!(&take()[..], [Update::ShareRemoved { result: Err(e), .. }] if e == SHARING_NOT_JOINED)
    );
}

#[gpui::test]
fn the_panel_lies_inside_the_window_at_the_stylesheets_size(cx: &mut TestAppContext) {
    let (shell, _core, vcx) = open_standin(cx, "settings-bounds", gate_fixture(), None);
    for (w, h) in [(1280., 820.), (640., 420.), (1920., 1080.), (980., 700.)] {
        vcx.simulate_resize(gpui::size(gpui::px(w), gpui::px(h)));
        open(&shell, vcx, None);
        let scale = f64::from(vcx.update(|window, _| window.scale_factor()));
        let tree = tree(vcx);
        let dialog = named(&nodes(&tree), Role::Dialog, "Settings");
        let b = dialog.bounding_box().expect("bounds");
        let (x0, y0, x1, y1) = (b.x0 / scale, b.y0 / scale, b.x1 / scale, b.y1 / scale);
        let (w, h) = (f64::from(w), f64::from(h));
        assert!(
            x0 >= 0. && y0 >= 0. && x1 <= w && y1 <= h,
            "{w}x{h}: {x0},{y0} to {x1},{y1}"
        );
        let expected_w = (1100f64).min(w - 32.);
        let expected_h = if w <= 980. {
            h - 32.
        } else {
            (780f64).min(h - 48.)
        };
        assert!(
            ((x1 - x0) - expected_w).abs() < 1.5,
            "{w}x{h}: width {}",
            x1 - x0
        );
        assert!(
            ((y1 - y0) - expected_h).abs() < 1.5,
            "{w}x{h}: height {}",
            y1 - y0
        );
        vcx.simulate_keystrokes("escape");
        settle(&shell, vcx);
    }
}

#[gpui::test]
fn the_gallery_shows_settings_controls_in_their_states(cx: &mut TestAppContext) {
    quiet_crashes();
    cx.update(mcbench_ui::view::setup);
    let (_gallery, vcx) = cx.add_window_view(|window, cx| {
        window.activate_a11y_for_test();
        mcbench_ui::dev::gallery::Gallery::new(None, cx)
    });
    for _ in 0..4 {
        vcx.run_until_parked();
        let _ = tree(vcx);
    }
    let tree = tree(vcx);
    let nodes = nodes(&tree);
    named(&nodes, Role::TabList, "Settings sections");
    named(&nodes, Role::Switch, "Offer OpenRouter in the model picker");
    named(&nodes, Role::PasswordInput, "API key");
    assert_eq!(
        starting(&nodes, Role::Status, "Installing Claude Code 2.1.311").len(),
        1
    );
    named(&nodes, Role::Label, "WXYZ-2026");
    named(&nodes, Role::TextInput, "Code from the sign-in page");
    assert!(!starting(&nodes, Role::Button, "Show or hide the sidebar, recording").is_empty());
    named(&nodes, Role::Group, "Delete all 12 votes?");
    let steps = named(&nodes, Role::List, "Setup steps");
    assert_eq!(steps.children().count(), 5);
    assert!(has(&nodes, Role::Alert, "A step failed."));
}

#[test]
fn the_wsl_plays_end_each_command_in_the_setup_steps() {
    use mcbench_ui::dev::standin::{WSL_INSTALLING, WSL_NEEDED, WSL_REFUSED};
    let shell_note = |updates: &[Update]| -> Option<String> {
        updates.iter().rev().find_map(|u| match u {
            Update::Setup(steps) => steps.iter().find(|s| s.id == "shell").map(|s| {
                mcbench_ui::state::setup::step_row(s)
                    .note
                    .unwrap_or_else(|| mcbench_ui::state::setup::step_row(s).value)
            }),
            _ => None,
        })
    };
    for (play, ends) in [
        (Play::SetupWsl, "Done"),
        (Play::SetupWslRefused, WSL_REFUSED),
    ] {
        let core = StandIn::new(
            gate_fixture(),
            String::new(),
            std::time::Duration::ZERO,
            None,
        );
        let inbox = Arc::new(Inbox::default());
        core.attach(inbox.clone());
        let take = || std::mem::take(&mut *inbox.0.lock().expect("lock"));
        take();
        core.settings_play(play);
        assert_eq!(shell_note(&take()).as_deref(), Some(WSL_NEEDED));
        core.send(Command::InstallWsl);
        let started = take();
        assert_eq!(kinds(&started), ["setup"]);
        assert_eq!(shell_note(&started).as_deref(), Some(WSL_INSTALLING));
        // The installer runs two steps.
        core.settings_tick();
        core.settings_tick();
        assert_eq!(shell_note(&take()).as_deref(), Some(ends));
        core.send(Command::InstallWsl);
        let again = take();
        if play == Play::SetupWsl {
            assert_eq!(kinds(&again), ["error", "setup"], "WSL is ready: refused");
        }
    }
    let core = StandIn::new(
        gate_fixture(),
        String::new(),
        std::time::Duration::ZERO,
        None,
    );
    let inbox = Arc::new(Inbox::default());
    core.attach(inbox.clone());
    let take = || std::mem::take(&mut *inbox.0.lock().expect("lock"));
    core.settings_play(Play::SetupWslRestart);
    take();
    core.send(Command::RestartComputer);
    // Windows took the request and nothing follows: nothing is published.
    assert!(take().is_empty());
    assert_eq!(core.restarts_asked(), 1);
}

#[gpui::test]
fn the_shell_row_installs_wsl_and_keeps_focus_through_the_install(cx: &mut TestAppContext) {
    let (shell, core, vcx) = open_standin(
        cx,
        "settings-wsl",
        gate_fixture(),
        Some(mcbench_ui::state::place::Place::NewChat { id: None }),
    );
    core.settings_play(Play::SetupWsl);
    settle(&shell, vcx);
    let tree1 = tree(vcx);
    let install = named(&nodes(&tree1), Role::Button, "Install WSL");
    act(vcx, install, gpui::accesskit::Action::Focus);
    vcx.simulate_keystrokes("enter");
    settle(&shell, vcx);
    let tree2 = tree(vcx);
    assert!(
        !has(&nodes(&tree2), Role::Button, "Install WSL"),
        "no button while the installer runs"
    );
    let (role, name) = focused(vcx);
    assert_eq!(role, Role::ListItem);
    assert!(
        name.unwrap().contains("Installing WSL"),
        "focus stays on the row"
    );
    core.settings_tick();
    core.settings_tick();
    settle(&shell, vcx);
    assert!(has_text(vcx, "Model's shell, Optional, Done"));
}

#[gpui::test]
fn restart_now_is_one_press_and_the_row_stays_usable_when_nothing_follows(cx: &mut TestAppContext) {
    let (shell, core, vcx) = open_standin(
        cx,
        "settings-wsl-restart",
        gate_fixture(),
        Some(mcbench_ui::state::place::Place::NewChat { id: None }),
    );
    core.settings_play(Play::SetupWslRestart);
    settle(&shell, vcx);
    assert!(!has_text(vcx, "Restart Windows now?"), "no confirmation");
    press(vcx, Role::Button, "Restart now");
    settle(&shell, vcx);
    assert_eq!(core.restarts_asked(), 1, "one press sends it");
    // No update follows: the row is as it was, its button there and usable.
    assert!(has_text(vcx, mcbench_ui::dev::standin::WSL_RESTART));
    let tree = tree(vcx);
    let button = named(&nodes(&tree), Role::Button, "Restart now");
    assert!(!button.is_disabled());
    press(vcx, Role::Button, "Restart now");
    settle(&shell, vcx);
    assert_eq!(core.restarts_asked(), 2, "and it can be pressed again");
}

#[test]
fn the_stand_in_matches_the_cores_endings_for_shared_logins_repeats_and_ends() {
    let core = StandIn::new(
        gate_fixture(),
        String::new(),
        std::time::Duration::ZERO,
        None,
    );
    let inbox = Arc::new(Inbox::default());
    core.attach(inbox.clone());
    let take = || std::mem::take(&mut *inbox.0.lock().expect("lock"));
    core.settings_play(Play::ChatgptSignIn);
    take();
    // (a) One login: every change is published for ChatGPT and Codex CLI.
    core.send(Command::SignIn {
        provider: "codex".into(),
        method: None,
    });
    let started = take();
    let providers: Vec<&str> = started
        .iter()
        .filter_map(|u| match u {
            Update::SignIn { provider, .. } => Some(provider.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(providers, ["codex", "codex-cli"]);
    // (b) Its end (here cancelled): both, then the providers, then the
    // settings; cancelling it again changes nothing and publishes the
    // unchanged state again, with no error.
    core.send(Command::CancelSignIn {
        provider: "codex".into(),
    });
    assert_eq!(
        kinds(&take()),
        ["sign-in", "sign-in", "providers", "settings"]
    );
    core.send(Command::CancelSignIn {
        provider: "codex".into(),
    });
    assert_eq!(kinds(&take()), ["sign-in", "sign-in"]);
    // (b) A retry's steps can arrive twice.
    core.settings_play(Play::SetupFails);
    for _ in 0..7 {
        core.settings_tick();
    }
    take();
    core.send(Command::RetrySetup);
    assert_eq!(kinds(&take()), ["setup", "setup"]);
}

#[gpui::test]
fn a_refused_install_puts_the_row_back_to_its_button_from_the_settings(cx: &mut TestAppContext) {
    let (shell, core, vcx) = with_plays(
        cx,
        "settings-install-refused",
        &[Play::ClaudeSignIn, Play::InstallRefused],
    );
    open(&shell, vcx, Some(Pane::Provider("claude-code".into())));
    press(vcx, Role::Button, "Claude subscription");
    settle(&shell, vcx);
    assert!(
        has_text(vcx, mcbench_ui::dev::standin::INSTALL_REFUSED),
        "the notice says why"
    );
    assert!(!has_text(vcx, "Installing Claude Code"));
    let first = tree(vcx);
    named(&nodes(&first), Role::Button, "Claude subscription");
    // The same settings arriving twice more change nothing.
    core.send(Command::ReadSettings { refresh: false });
    core.send(Command::ReadSettings { refresh: false });
    settle(&shell, vcx);
    let again = tree(vcx);
    named(&nodes(&again), Role::Button, "Claude subscription");
    assert!(tab_selected(vcx, "Claude Code"));
}

#[test]
fn a_refused_install_ends_in_its_install_when_one_exists_then_fresh_settings() {
    let core = StandIn::new(
        gate_fixture(),
        String::new(),
        std::time::Duration::ZERO,
        None,
    );
    let inbox = Arc::new(Inbox::default());
    core.attach(inbox.clone());
    let take = || std::mem::take(&mut *inbox.0.lock().expect("lock"));
    core.settings_play(Play::ClaudeSignIn);
    take();
    // None started: the error, then fresh settings.
    core.settings_play(Play::InstallRefused);
    core.send(Command::InstallCli {
        program: "claude".into(),
    });
    assert_eq!(kinds(&take()), ["error", "settings"]);
    // One exists: the error, its install, then fresh settings, last.
    let core = StandIn::new(
        gate_fixture(),
        String::new(),
        std::time::Duration::ZERO,
        None,
    );
    let inbox = Arc::new(Inbox::default());
    core.attach(inbox.clone());
    let take = || std::mem::take(&mut *inbox.0.lock().expect("lock"));
    core.settings_play(Play::ClaudeSignIn);
    core.send(Command::InstallCli {
        program: "claude".into(),
    });
    take();
    core.settings_play(Play::InstallRefused);
    core.send(Command::InstallCli {
        program: "claude".into(),
    });
    let refused = take();
    assert_eq!(kinds(&refused), ["error", "install", "settings"]);
}

// s4-needs: a step that needs the person, with setup otherwise ready (the
// shell is optional), in the empty Build chat's steps and over every
// composer.

/// The line over the composer: the Status whose name is the step's text.
fn needs_line(vcx: &mut VisualTestContext, text: &str) -> bool {
    let tree = tree(vcx);
    nodes(&tree)
        .iter()
        .any(|n| n.role() == Role::Status && n.label().as_deref() == Some(text))
}

/// The setup steps' rows on screen, by name.
fn step_rows(vcx: &mut VisualTestContext) -> Vec<String> {
    let tree = tree(vcx);
    nodes(&tree)
        .iter()
        .filter(|n| {
            n.role() == Role::ListItem
                && n.parent()
                    .is_some_and(|p| p.label().as_deref() == Some("Setup steps"))
        })
        .filter_map(|n| n.label())
        .collect()
}

/// The empty chat of a ready app: its own heading, no first-start screen.
fn ready_heading(vcx: &mut VisualTestContext) -> bool {
    let tree = tree(vcx);
    let nodes = nodes(&tree);
    has(&nodes, Role::Heading, "What should we build?")
        && !has_text(vcx, "Getting ready")
        && !has_text(vcx, "The first start downloads")
        && !nodes.iter().any(|n| n.role() == Role::ProgressIndicator)
        && !has_text(vcx, " of 2 done")
}

#[gpui::test]
fn an_optional_step_needing_the_person_keeps_the_ready_empty_chat_with_its_row_and_holds_nothing(
    cx: &mut TestAppContext,
) {
    use mcbench_ui::dev::standin::{WSL_INSTALLING, WSL_NEEDED, WSL_REFUSED};
    let (shell, core, vcx) = open_standin(
        cx,
        "needs-empty",
        gate_fixture(),
        Some(mcbench_ui::state::place::Place::NewChat { id: None }),
    );
    core.settings_play(Play::SetupWslOptionalRefused);
    settle(&shell, vcx);
    // Ready otherwise: the empty chat's own heading, the card with only the
    // step that needs the person, its button, the ready chat's last line;
    // no failure, no held line, no second line over the composer.
    assert!(
        ready_heading(vcx),
        "the ready chat's heading, not Getting ready"
    );
    let rows = step_rows(vcx);
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert!(
        rows[0].starts_with("Model's shell") && rows[0].contains(WSL_NEEDED),
        "{rows:?}"
    );
    let tree1 = tree(vcx);
    assert!(has(&nodes(&tree1), Role::Button, "Install WSL"));
    // `Or have several models build it in` (text) with its `Bench` link.
    assert!(has(&nodes(&tree1), Role::Button, "Bench"));
    assert!(!has_text(vcx, "You can send your first build now"));
    assert!(!needs_line(vcx, WSL_NEEDED), "the card says it already");
    // The installer runs: its row with the dots, no button; then it is
    // refused: the button again, and still no failure (the step is
    // optional).
    press(vcx, Role::Button, "Install WSL");
    settle(&shell, vcx);
    let tree2 = tree(vcx);
    assert!(!has(&nodes(&tree2), Role::Button, "Install WSL"));
    assert!(ready_heading(vcx));
    let rows = step_rows(vcx);
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert!(
        rows[0].contains(WSL_INSTALLING),
        "the running row stays: {rows:?}"
    );
    core.settings_tick();
    core.settings_tick();
    settle(&shell, vcx);
    assert!(has_text(vcx, WSL_REFUSED));
    let tree3 = tree(vcx);
    assert!(has(&nodes(&tree3), Role::Button, "Install WSL"));
    assert!(!has_text(vcx, "A step failed."));
    assert!(!has(&nodes(&tree3), Role::Button, "Try again"));
    assert!(ready_heading(vcx));
}

#[gpui::test]
fn while_setup_is_not_ready_the_empty_chat_shows_every_step_under_getting_ready(
    cx: &mut TestAppContext,
) {
    let (shell, core, vcx) = open_standin(
        cx,
        "needs-not-ready",
        gate_fixture(),
        Some(mcbench_ui::state::place::Place::NewChat { id: None }),
    );
    core.settings_play(Play::SetupWsl);
    settle(&shell, vcx);
    assert!(has_text(vcx, "Getting ready"));
    assert!(has_text(vcx, "1 of 2 done"));
    let tree = tree(vcx);
    assert!(!has(&nodes(&tree), Role::Heading, "What should we build?"));
    assert_eq!(step_rows(vcx).len(), 3, "every step: {:?}", step_rows(vcx));
}

#[gpui::test]
fn the_steps_go_from_the_empty_chat_once_nothing_needs_the_person(cx: &mut TestAppContext) {
    let (shell, core, vcx) = open_standin(
        cx,
        "needs-empty-done",
        gate_fixture(),
        Some(mcbench_ui::state::place::Place::NewChat { id: None }),
    );
    core.settings_play(Play::SetupWslOptional);
    settle(&shell, vcx);
    press(vcx, Role::Button, "Install WSL");
    core.settings_tick();
    core.settings_tick();
    settle(&shell, vcx);
    assert!(!has_text(vcx, "Getting ready"));
    assert!(has_text(vcx, "What should we build?"));
    assert!(
        step_rows(vcx).is_empty(),
        "no card once nothing needs the person"
    );
}

#[gpui::test]
fn a_chat_with_messages_shows_the_step_over_the_composer_before_its_text_box(
    cx: &mut TestAppContext,
) {
    use mcbench_ui::dev::standin::{WSL_INSTALLING, WSL_NEEDED, WSL_REFUSED};
    let (shell, core, vcx) = open_standin(
        cx,
        "needs-chat",
        gate_fixture(),
        Some(mcbench_ui::state::place::Place::Chat {
            id: "chat-gate".into(),
        }),
    );
    core.settings_play(Play::SetupWslOptionalRefused);
    settle(&shell, vcx);
    assert!(needs_line(vcx, WSL_NEEDED));
    assert!(!has_text(vcx, "Getting ready"));
    assert!(!has_text(vcx, "Sends when the app is ready"));
    // The button comes before the composer's text box in the Tab order.
    let tree1 = tree(vcx);
    let install = named(&nodes(&tree1), Role::Button, "Install WSL");
    act(vcx, install, gpui::accesskit::Action::Focus);
    vcx.simulate_keystrokes("tab");
    settle(&shell, vcx);
    assert_eq!(
        focused(vcx),
        (Role::MultilineTextInput, Some("Message".to_string()))
    );
    vcx.simulate_keystrokes("shift-tab");
    settle(&shell, vcx);
    assert_eq!(
        focused(vcx),
        (Role::Button, Some("Install WSL".to_string()))
    );
    // The installer runs: the line says so, with no button, and keeps the
    // focus; refused, its button is back.
    vcx.simulate_keystrokes("enter");
    settle(&shell, vcx);
    assert!(needs_line(vcx, WSL_INSTALLING));
    let tree2 = tree(vcx);
    assert!(!has(&nodes(&tree2), Role::Button, "Install WSL"));
    assert_eq!(
        focused(vcx),
        (Role::Status, Some(WSL_INSTALLING.to_string()))
    );
    core.settings_tick();
    core.settings_tick();
    settle(&shell, vcx);
    assert!(needs_line(vcx, WSL_REFUSED));
    let tree3 = tree(vcx);
    assert!(has(&nodes(&tree3), Role::Button, "Install WSL"));
}

#[gpui::test]
fn the_line_goes_from_a_chat_once_the_step_is_done(cx: &mut TestAppContext) {
    use mcbench_ui::dev::standin::WSL_NEEDED;
    let (shell, core, vcx) = open_standin(
        cx,
        "needs-chat-done",
        gate_fixture(),
        Some(mcbench_ui::state::place::Place::Chat {
            id: "chat-gate".into(),
        }),
    );
    // Not ready otherwise either: the line shows the same.
    core.settings_play(Play::SetupWsl);
    settle(&shell, vcx);
    assert!(needs_line(vcx, WSL_NEEDED));
    press(vcx, Role::Button, "Install WSL");
    core.settings_tick();
    core.settings_tick();
    settle(&shell, vcx);
    let tree = tree(vcx);
    assert!(
        !nodes(&tree)
            .iter()
            .any(|n| n.role() == Role::Status && n.label().is_some_and(|l| l.contains("WSL"))),
        "no line once nothing needs the person"
    );
}

#[gpui::test]
fn a_bench_shows_the_restart_over_its_composer(cx: &mut TestAppContext) {
    use mcbench_ui::dev::standin::WSL_RESTART;
    use mcbench_ui::state::place::Place;
    for (n, place) in [
        Place::NewBench,
        Place::Compare {
            bench: "bench-castle".into(),
        },
    ]
    .into_iter()
    .enumerate()
    {
        let (shell, core, vcx) = open_bench(cx, &format!("needs-bench-{n}"), Some(place), &[]);
        core.settings_play(Play::SetupWslOptionalRestart);
        settle(&shell, vcx);
        assert!(needs_line(vcx, WSL_RESTART), "{n}");
        press(vcx, Role::Button, "Restart now");
        settle(&shell, vcx);
        assert_eq!(core.restarts_asked(), 1, "{n}");
        assert!(needs_line(vcx, WSL_RESTART), "nothing followed: {n}");
        let tree = tree(vcx);
        assert!(!named(&nodes(&tree), Role::Button, "Restart now").is_disabled());
    }
}

// par-shell (parity pass): `app.md` 8.1 (`checkRef`, the title model's
// facts before opening), `components.md` E.3 (the danger zone's question
// after a failed delete), N.3 (the published rows' hovers).

/// Takes what the core published and has not been applied.
fn take(shell: &ShellUnderTest) -> Vec<Update> {
    std::mem::take(&mut *shell.inbox.0.lock().expect("lock"))
}

fn apply(shell: &ShellUnderTest, vcx: &mut VisualTestContext, update: Update) {
    vcx.update(|window, cx| shell.entity.update(cx, |s, cx| s.apply(update, window, cx)));
    vcx.run_until_parked();
}

fn settings_open(vcx: &mut VisualTestContext) -> bool {
    let tree = tree(vcx);
    nodes(&tree)
        .iter()
        .any(|n| n.role() == Role::Dialog && n.label().as_deref() == Some("Settings"))
}

#[gpui::test]
fn settings_open_only_once_the_title_models_facts_are_read(cx: &mut TestAppContext) {
    let (shell, _core, vcx) = with_plays(cx, "settings-title-facts", &[]);
    vcx.update(|window, cx| {
        shell
            .entity
            .update(cx, |s, cx| s.open_settings_on(None, None, window, cx))
    });
    vcx.run_until_parked();
    // The read, as if a title model were saved.
    let mut held = Vec::new();
    for update in take(&shell) {
        match update {
            Update::Settings(Ok(mut view)) => {
                view.title_model = Some(mcbench_ui::TitleModel {
                    provider: "anthropic".into(),
                    model: "claude-title-test".into(),
                    effort: String::new(),
                });
                apply(&shell, vcx, Update::Settings(Ok(view)));
            }
            other => held.push(other),
        }
    }
    let asked: Vec<Update> = take(&shell);
    assert!(!settings_open(vcx), "not open before the facts");
    for update in held.into_iter().chain(asked) {
        apply(&shell, vcx, update);
    }
    settle(&shell, vcx);
    assert!(settings_open(vcx), "open once they are in (or failed)");
}

#[gpui::test]
fn a_check_older_than_the_settings_on_show_lands_nowhere(cx: &mut TestAppContext) {
    let (shell, core, vcx) = with_plays(cx, "settings-check-ref", &[]);
    open(&shell, vcx, Some(Pane::Provider("openai".into())));
    core.send(Command::ReadSettings { refresh: true });
    let mut fresh = None;
    for update in take(&shell) {
        if let Update::Settings(Ok(view)) = update {
            fresh = Some(view);
        }
    }
    let fresh = fresh.expect("a check's answer");
    apply(&shell, vcx, Update::Settings(Ok(fresh.clone())));
    settle(&shell, vcx);
    assert!(has_text(vcx, "Checked just now"));
    // An older check arriving after it: an hour older, and with OpenAI's
    // key gone; neither shows.
    let mut stale = fresh;
    stale.checked_at -= 3_600_000;
    for p in &mut stale.providers {
        if p.id == "openai" {
            p.connected = !p.connected;
        }
    }
    let before = {
        let tree = tree(vcx);
        starting(&nodes(&tree), Role::Tab, "OpenAI")[0].label()
    };
    apply(&shell, vcx, Update::Settings(Ok(stale)));
    settle(&shell, vcx);
    assert!(
        has_text(vcx, "Checked just now"),
        "the older check is dropped"
    );
    let tree = tree(vcx);
    assert_eq!(
        starting(&nodes(&tree), Role::Tab, "OpenAI")[0].label(),
        before
    );
}

#[gpui::test]
fn a_failed_vote_delete_keeps_its_question_even_when_the_count_fails_with_it(
    cx: &mut TestAppContext,
) {
    let (shell, _core, vcx) = with_plays(
        cx,
        "settings-votes-failed",
        &[Play::ClearFails, Play::VotesOne],
    );
    open(&shell, vcx, Some(Pane::General));
    press(vcx, Role::Button, "Clear leaderboard");
    let tree1 = tree(vcx);
    let delete = named(&nodes(&tree1), Role::Button, "Delete votes");
    // The press, with the core's answer held: its count failing too.
    vcx.update(|window, cx| {
        window.dispatch_a11y_action_for_test(
            gpui::accesskit::ActionRequest {
                action: gpui::accesskit::Action::Click,
                target_tree: gpui::accesskit::TreeId::ROOT,
                target_node: delete.locate().0,
                data: None,
            },
            cx,
        )
    });
    vcx.run_until_parked();
    for update in take(&shell) {
        let update = match update {
            Update::Leaderboard(_) => Update::Leaderboard(Err(CLEAR_FAILED.into())),
            other => other,
        };
        apply(&shell, vcx, update);
    }
    settle(&shell, vcx);
    let tree2 = tree(vcx);
    named(&nodes(&tree2), Role::Group, "Delete the only vote?");
    assert!(!has_text(vcx, "Delete every vote?"));
}

#[gpui::test]
fn a_published_rows_title_labels_and_date_show_on_hover(cx: &mut TestAppContext) {
    let (shell, _core, vcx) = with_plays(cx, "settings-published-tips", &[]);
    open(&shell, vcx, Some(Pane::General));
    let tips = {
        let tree = tree(vcx);
        let rows: Vec<String> = of_role(&nodes(&tree), Role::ListItem)
            .into_iter()
            .filter(|n| {
                n.parent()
                    .is_some_and(|p| p.label().as_deref() == Some("Published builds"))
            })
            .filter_map(|n| n.label())
            .collect();
        assert!(!rows.is_empty(), "published rows");
        rows
    };
    // Hover the first row's two lines at a few points (title, then the
    // meta line's labels and date); the tips seen.
    let row = {
        let tree = tree(vcx);
        let list = named(&nodes(&tree), Role::List, "Published builds");
        list.children()
            .find(|n| n.role() == Role::ListItem)
            .and_then(|n| n.bounding_box())
            .expect("a row's bounds")
    };
    let scale = f64::from(vcx.update(|window, _| window.scale_factor()));
    let row = gpui::accesskit::Rect {
        x0: row.x0 / scale,
        y0: row.y0 / scale,
        x1: row.x1 / scale,
        y1: row.y1 / scale,
    };
    let mut seen: Vec<String> = Vec::new();
    for dy in [18., 36.] {
        for dx in [8., 30., 60., 100., 140., 180., 220.] {
            vcx.simulate_mouse_move(
                gpui::point(
                    gpui::px((row.x0 + dx) as f32),
                    gpui::px((row.y0 + dy) as f32),
                ),
                None,
                gpui::Modifiers::none(),
            );
            let _ = tree(vcx);
            vcx.executor()
                .advance_clock(std::time::Duration::from_millis(600));
            vcx.run_until_parked();
            let tree = tree(vcx);
            for t in of_role(&nodes(&tree), Role::Tooltip) {
                if let Some(l) = t.label()
                    && !seen.contains(&l)
                {
                    seen.push(l);
                }
            }
        }
    }
    let titles: Vec<&str> = tips
        .iter()
        .map(|r| r.split(", ").next().unwrap_or(""))
        .collect();
    assert!(
        seen.iter().any(|t| titles
            .iter()
            .any(|title| t == title || title.ends_with(t.as_str()))),
        "a full title on hover: {seen:?} of {titles:?}"
    );
    assert!(
        seen.iter()
            .any(|t| t.contains('/') && (t.ends_with(" AM") || t.ends_with(" PM"))),
        "a date on hover: {seen:?}"
    );
}
