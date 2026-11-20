//! The chat column as today's page has it (inventory `app.md` 5 and 6): a
//! full streamed turn settling into its fold, plan, warning and answer; a
//! stopped turn; a refused send; joining a running turn; edit and branch
//! with its arrows; an edit whose file cannot be read back; attachments and
//! their limits; drafts per place; the lightbox; the held send; typing
//! anywhere going to the composer. Each through the window as a user or a
//! screen reader drives it, read back from the accessibility tree.

mod common;

use common::*;
use gpui::accesskit::Role;
use gpui::{TestAppContext, VisualTestContext};
use mcbench_ui::dev::standin::{self, StandIn};
use mcbench_ui::dev::turns::Script;
use mcbench_ui::state::place::Place;

fn lamp() -> Option<Place> {
    Some(Place::Chat {
        id: "chat-lamp".into(),
    })
}

fn pane(
    shell: &ShellUnderTest,
    vcx: &mut VisualTestContext,
) -> gpui::Entity<mcbench_ui::view::chat::ChatPane> {
    vcx.update(|_, cx| shell.entity.read(cx).chat())
}

fn composer_text(shell: &ShellUnderTest, vcx: &mut VisualTestContext) -> String {
    let pane = pane(shell, vcx);
    vcx.update(|_, cx| pane.read(cx).composer().read(cx).value().to_string())
}

/// Focuses the composer, types `text` and presses Enter.
fn send(shell: &ShellUnderTest, vcx: &mut VisualTestContext, text: &str) {
    type_in(shell, vcx, text);
    vcx.simulate_keystrokes("enter");
    pump(shell, vcx);
}

fn type_in(shell: &ShellUnderTest, vcx: &mut VisualTestContext, text: &str) {
    let pane = pane(shell, vcx);
    vcx.update(|window, cx| pane.update(cx, |p, cx| p.focus_composer(window, cx)));
    vcx.simulate_input(text);
    pump(shell, vcx);
}

/// The transcript's messages (the `Conversation` region's articles).
fn articles(vcx: &mut VisualTestContext) -> Vec<String> {
    fn walk(node: accesskit_consumer::Node<'_>, out: &mut Vec<String>) {
        if node.role() == Role::Article {
            out.push(node.label().unwrap_or_default());
        }
        for child in node.children() {
            walk(child, out);
        }
    }
    let tree = tree(vcx);
    let region = named(&nodes(&tree), Role::Region, "Conversation");
    let mut out = Vec::new();
    walk(region, &mut out);
    out
}

/// The window's notices (`showError`).
fn notices(vcx: &mut VisualTestContext) -> Vec<String> {
    let rows = articles(vcx);
    let tree = tree(vcx);
    of_role(&nodes(&tree), Role::Article)
        .iter()
        .filter_map(|n| n.label())
        .filter(|l| l.starts_with("Error, ") && !rows.contains(l))
        .collect()
}

fn press(shell: &ShellUnderTest, vcx: &mut VisualTestContext, role: Role, name: &str) {
    let tree = tree(vcx);
    let all = nodes(&tree);
    // A chat row's name is its title, then its status (the sidebar's).
    let found: Vec<_> = all
        .iter()
        .filter(|n| {
            n.role() == role
                && n.label().is_some_and(|l| {
                    l == name || (role == Role::ListItem && l.starts_with(&format!("{name}, ")))
                })
        })
        .collect();
    let node = *found
        .last()
        .unwrap_or_else(|| panic!("a {role:?} named {name:?}"));
    click(vcx, *node);
    pump(shell, vcx);
}

fn names(vcx: &mut VisualTestContext, role: Role) -> Vec<String> {
    let tree = tree(vcx);
    of_role(&nodes(&tree), role)
        .iter()
        .map(|n| n.label().unwrap_or_default())
        .collect()
}

fn logged(shell: &ShellUnderTest, action: &str) -> Vec<serde_json::Value> {
    log_lines(&shell.log)
        .into_iter()
        .filter(|a| a["action"] == action)
        .collect()
}

#[gpui::test]
fn a_full_turn_settles_into_its_fold_plan_warning_and_answer(cx: &mut TestAppContext) {
    let (shell, vcx) = open_shell_with(cx, "chat-full", lamp(), Vec::new(), |core, _| {
        core.set_script(Script::Full)
    });
    send(&shell, vcx, "Make it brighter");
    let rows = articles(vcx);
    let at = rows
        .iter()
        .rposition(|r| r == "You, Make it brighter")
        .expect("the sent message");
    let after = &rows[at + 1..];
    // User message, `Worked · n steps` fold, plan card, warnings, answer.
    assert_eq!(after.len(), 4, "{after:#?}");
    assert!(
        after[0].starts_with("claude-opus-5-5, work"),
        "{}",
        after[0]
    );
    // As stored, a message's notices come before its plan (`itemsFromMessages`).
    assert!(
        after[1].contains("Warning: The model fell back to a smaller context window"),
        "{}",
        after[1]
    );
    assert!(after[2].contains("Tasks, 3 of 3"), "{}", after[2]);
    assert!(after[3].starts_with("claude-opus-5-5, "), "{}", after[3]);
    let folds = names(vcx, Role::Button);
    assert!(
        folds.iter().any(|n| n == "Worked \u{b7} 4 steps"),
        "{folds:?}"
    );
    // The fold opens on the turn's work: each tool's row as on screen.
    press(&shell, vcx, Role::Button, "Worked \u{b7} 4 steps");
    let buttons = names(vcx, Role::Button);
    for row in [
        "Built: a stone base and a lamp",
        "Minecraft test completed: 2 tests: input and block-state observations",
        "Inspected build: (iso_se)",
        "Command failed: cat /work/notes.txt",
        "Reasoning",
    ] {
        assert!(buttons.iter().any(|n| n == row), "{row} in {buttons:#?}");
    }
    // The shot, kept by the store and drawn from its file.
    assert!(names(vcx, Role::Image).iter().any(|n| n == "Picture 1"));
    // The answer was announced once the turn ended.
    let status = {
        let tree = tree(vcx);
        of_role(&nodes(&tree), Role::Status)
            .iter()
            .find_map(|n| n.label())
    };
    assert!(status.is_some_and(|s| !s.is_empty()));
    // The shot opens in the lightbox; Escape closes it.
    press(&shell, vcx, Role::Image, "Picture 1");
    assert!(names(vcx, Role::Dialog).iter().any(|n| n == "Picture"));
    let (role, name) = focused(vcx);
    assert_eq!((role, name.as_deref()), (Role::Button, Some("Close")));
    vcx.simulate_keystrokes("escape");
    pump(&shell, vcx);
    assert!(names(vcx, Role::Dialog).is_empty());
    assert!(!logged(&shell, "closed").is_empty());
}

#[gpui::test]
fn a_stopped_turn_cancels_its_running_tool_and_says_so(cx: &mut TestAppContext) {
    let (shell, vcx) = open_shell_with(cx, "chat-stop", lamp(), Vec::new(), |core, _| {
        core.set_script(Script::Full);
        // Stopped while the Minecraft test runs.
        core.stop_after(43);
    });
    send(&shell, vcx, "Make it brighter");
    let rows = articles(vcx);
    assert_eq!(
        rows.last().map(String::as_str),
        Some("claude-opus-5-5, (stopped by user)")
    );
    press(&shell, vcx, Role::Button, "Worked \u{b7} 4 steps");
    let buttons = names(vcx, Role::Button);
    assert!(
        buttons
            .iter()
            .any(|n| n.starts_with("Minecraft test cancelled")),
        "{buttons:#?}"
    );
}

#[gpui::test]
fn a_refused_send_puts_the_message_and_the_transcript_back(cx: &mut TestAppContext) {
    let (shell, vcx) = open_shell_with(cx, "chat-refused", lamp(), Vec::new(), |core, _| {
        core.refuse_sends_to("chat-lamp")
    });
    let before = articles(vcx);
    send(&shell, vcx, "One more lamp");
    assert_eq!(articles(vcx), before);
    assert_eq!(composer_text(&shell, vcx), "One more lamp");
    assert_eq!(notices(vcx), [format!("Error, {}", standin::REFUSAL)]);
    let pane = pane(&shell, vcx);
    assert_eq!(
        vcx.update(|_, cx| pane.read(cx).phase().clone()),
        mcbench_ui::state::turn::IDLE
    );
}

#[gpui::test]
fn a_running_turn_is_joined_when_its_chat_opens(cx: &mut TestAppContext) {
    let (shell, vcx) = open_shell_with(
        cx,
        "chat-join",
        Some(Place::Chat {
            id: "chat-gate".into(),
        }),
        Vec::new(),
        |core: &StandIn, _| {
            core.mark_running("chat-gate");
            core.run_turn_on_join("chat-gate")
        },
    );
    assert!(
        !logged(&shell, "joined").is_empty(),
        "the running turn was joined"
    );
    let rows = articles(vcx);
    let answer = rows.last().expect("rows");
    assert!(answer.starts_with("claude-opus-5-5, "), "{rows:#?}");
    let pane = pane(&shell, vcx);
    assert!(matches!(
        vcx.update(|_, cx| pane.read(cx).phase().clone()),
        mcbench_ui::state::turn::Turn::Idle { ended: Some(_) }
    ));
}

#[gpui::test]
fn edit_and_branch_replaces_the_later_transcript_and_the_arrows_switch_back(
    cx: &mut TestAppContext,
) {
    let (shell, vcx) = open_shell_with(cx, "chat-edit", lamp(), Vec::new(), |_, _| {});
    let original = "You, Thanks. Is the base still three by three?";
    assert!(articles(vcx).iter().any(|r| r == original));
    press(&shell, vcx, Role::Button, "Edit and branch");
    assert_eq!(
        composer_text(&shell, vcx),
        "Thanks. Is the base still three by three?"
    );
    assert!(
        names(vcx, Role::Status)
            .iter()
            .any(|n| n == "Editing a message: sending it starts a new branch")
    );
    // Cancel brings back what the composer held.
    press(&shell, vcx, Role::Button, "Cancel edit");
    assert_eq!(composer_text(&shell, vcx), "");
    press(&shell, vcx, Role::Button, "Edit and branch");
    let pane = pane(&shell, vcx);
    vcx.update(|window, cx| {
        pane.update(cx, |p, cx| {
            p.composer().update(cx, |c, cx| {
                c.set_value("Is the base four by four?", window, cx)
            })
        })
    });
    vcx.update(|window, cx| pane.update(cx, |p, cx| p.send(window, cx)));
    pump(&shell, vcx);
    let rows = articles(vcx);
    assert!(!rows.iter().any(|r| r == original), "{rows:#?}");
    assert!(rows.iter().any(|r| r == "You, Is the base four by four?"));
    assert!(
        names(vcx, Role::Group)
            .iter()
            .any(|n| n == "Conversation branch, 2 of 2")
    );
    press(&shell, vcx, Role::Button, "Previous branch");
    assert!(articles(vcx).iter().any(|r| r == original));
    assert!(
        names(vcx, Role::Group)
            .iter()
            .any(|n| n == "Conversation branch, 1 of 2")
    );
    assert!(!logged(&shell, "branch").is_empty());
}

#[gpui::test]
fn an_edit_that_fails_to_send_brings_the_later_transcript_back(cx: &mut TestAppContext) {
    let (shell, vcx) = open_shell_with(cx, "chat-edit-refused", lamp(), Vec::new(), |core, _| {
        core.refuse_sends_to("chat-lamp")
    });
    let before = articles(vcx);
    press(&shell, vcx, Role::Button, "Edit and branch");
    let pane = pane(&shell, vcx);
    vcx.update(|window, cx| pane.update(cx, |p, cx| p.send(window, cx)));
    pump(&shell, vcx);
    assert_eq!(articles(vcx), before);
    // The edit is back in the composer.
    assert!(
        names(vcx, Role::Status)
            .iter()
            .any(|n| n == "Editing a message: sending it starts a new branch")
    );
}

fn write_files(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let a = dir.join("notes.txt");
    std::fs::write(&a, "the base is stone").expect("written");
    let b = dir.join("door.png");
    std::fs::write(&b, mcbench_ui::dev::fixtures::PICTURE).expect("written");
    vec![a, b]
}

#[gpui::test]
fn attachments_are_read_sent_and_kept_and_an_edit_loses_only_the_file_it_cannot_read(
    cx: &mut TestAppContext,
) {
    let folder = std::sync::Arc::new(std::sync::Mutex::new(std::path::PathBuf::new()));
    let folder_in = folder.clone();
    // The files the picker gives, in a folder of their own.
    let dir = TempDir::new("chat-attach-picks");
    let files = write_files(&dir.0);
    let (shell, vcx) = open_shell_with(cx, "chat-attach", lamp(), files, move |_, test| {
        *folder_in.lock().expect("lock") = test.join("files");
    });
    press(&shell, vcx, Role::Button, "Attach file");
    let groups = names(vcx, Role::Group);
    assert!(
        groups.iter().any(|n| n == "notes.txt, plain"),
        "{groups:#?}"
    );
    // A picture is sent as JPEG under its own name.
    assert!(groups.iter().any(|n| n == "door.png, jpeg"), "{groups:#?}");
    send(&shell, vcx, "Here are the plans");
    let buttons = names(vcx, Role::Button);
    assert!(buttons.iter().any(|n| n == "door.png"), "{buttons:#?}");
    assert!(names(vcx, Role::Link).iter().any(|n| n == "notes.txt"));
    // The stored notes go missing; the edit keeps the picture and names
    // the file it lost.
    let stored = folder
        .lock()
        .expect("lock")
        .join("attachments/chat-lamp/0-notes.txt");
    std::fs::remove_file(&stored).expect("the stored file");
    press(&shell, vcx, Role::Button, "Edit and branch");
    let groups = names(vcx, Role::Group);
    assert!(groups.iter().any(|n| n == "door.png, jpeg"), "{groups:#?}");
    assert!(
        !groups.iter().any(|n| n.starts_with("notes.txt")),
        "{groups:#?}"
    );
    let shown = notices(vcx);
    assert!(
        shown
            .iter()
            .any(|n| n == "Error, Could not load \"notes.txt\" (Attachment not found)"),
        "{shown:#?}"
    );
    drop(dir);
}

#[gpui::test]
fn a_ninth_file_is_refused_with_the_pages_text(cx: &mut TestAppContext) {
    let dir = TempDir::new("chat-limit-picks");
    let files: Vec<std::path::PathBuf> = (0..9)
        .map(|n| {
            let path = dir.0.join(format!("f{n}.txt"));
            std::fs::write(&path, "x").expect("written");
            path
        })
        .collect();
    let (shell, vcx) = open_shell_with(cx, "chat-limit", lamp(), files, |_, _| {});
    press(&shell, vcx, Role::Button, "Attach file");
    assert_eq!(
        notices(vcx),
        ["Error, A message supports at most 8 attachments"]
    );
    assert!(
        !names(vcx, Role::Group)
            .iter()
            .any(|n| n.starts_with("f0.txt"))
    );
}

#[gpui::test]
fn each_chat_keeps_its_own_draft(cx: &mut TestAppContext) {
    let (shell, vcx) = open_shell_with(cx, "chat-drafts", lamp(), Vec::new(), |_, _| {});
    type_in(&shell, vcx, "a second lamp");
    press(&shell, vcx, Role::ListItem, "Castle gate");
    assert_eq!(composer_text(&shell, vcx), "");
    type_in(&shell, vcx, "a taller gate");
    press(&shell, vcx, Role::ListItem, "Redstone lamp with a lever");
    assert_eq!(composer_text(&shell, vcx), "a second lamp");
    press(&shell, vcx, Role::ListItem, "Castle gate");
    assert_eq!(composer_text(&shell, vcx), "a taller gate");
}

#[gpui::test]
fn a_send_before_setup_is_ready_is_held_and_goes_once_it_is(cx: &mut TestAppContext) {
    use mcbench_setup::{Step, StepState};
    let (shell, vcx) = open_shell_with(cx, "chat-held", lamp(), Vec::new(), |_, _| {});
    let step = |state| Step {
        id: "engine".into(),
        title: "The engine".into(),
        doing: "Building the engine".into(),
        optional: false,
        state,
        needs: None,
    };
    let apply = |vcx: &mut VisualTestContext, update| {
        vcx.update(|window, cx| shell.entity.update(cx, |s, cx| s.apply(update, window, cx)));
        pump(&shell, vcx);
    };
    apply(
        vcx,
        mcbench_ui::Update::Setup(vec![step(StepState::Waiting)]),
    );
    send(&shell, vcx, "Hold this");
    assert!(logged(&shell, "sent").is_empty());
    assert!(
        names(vcx, Role::Status)
            .iter()
            .any(|n| n == "Sends when the app is ready. 0 of 1 done.")
    );
    apply(
        vcx,
        mcbench_ui::Update::Setup(vec![step(StepState::Failed {
            error: "no network".into(),
        })]),
    );
    // No steps are on screen in a chat with messages: nothing names them.
    assert!(
        names(vcx, Role::Status)
            .iter()
            .any(|n| n == "Not sent yet: a step failed.")
    );
    apply(
        vcx,
        mcbench_ui::Update::Setup(vec![step(StepState::Done { seconds: 1. })]),
    );
    assert_eq!(logged(&shell, "sent").len(), 1);
    assert!(articles(vcx).iter().any(|r| r == "You, Hold this"));
}

#[gpui::test]
fn typing_anywhere_goes_into_the_composer(cx: &mut TestAppContext) {
    let (shell, vcx) = open_shell_with(cx, "chat-type-ahead", lamp(), Vec::new(), |_, _| {});
    // Focus on a chat row (no text field).
    vcx.simulate_keystrokes("tab");
    pump(&shell, vcx);
    let (role, _) = focused(vcx);
    assert_ne!(role, Role::MultilineTextInput);
    vcx.simulate_keystrokes("a shift-b");
    pump(&shell, vcx);
    assert_eq!(composer_text(&shell, vcx), "aB");
    let (role, name) = focused(vcx);
    assert_eq!(
        (role, name.as_deref()),
        (Role::MultilineTextInput, Some("Message"))
    );
}

#[gpui::test]
fn a_dismissed_stored_error_stays_dismissed(cx: &mut TestAppContext) {
    let (shell, vcx) = open_shell_with(
        cx,
        "chat-dismiss",
        Some(Place::Chat {
            id: standin::FAILING_CHAT.into(),
        }),
        Vec::new(),
        |_, _| {},
    );
    send(&shell, vcx, "hi");
    let error = format!("Error, {}", standin::FAILURE);
    assert!(articles(vcx).contains(&error));
    press(&shell, vcx, Role::Button, "Dismiss error");
    press(&shell, vcx, Role::ListItem, "Castle gate");
    press(&shell, vcx, Role::ListItem, "Wheat farm");
    assert!(
        !articles(vcx).contains(&error),
        "the store keeps the dismissal"
    );
}

/// Makes a second version of the last question (Edit and branch, sent).
fn second_version(shell: &ShellUnderTest, vcx: &mut VisualTestContext) {
    press(shell, vcx, Role::Button, "Edit and branch");
    let pane = pane(shell, vcx);
    vcx.update(|window, cx| {
        pane.update(cx, |p, cx| {
            p.composer().update(cx, |c, cx| {
                c.set_value("Is the base four by four?", window, cx)
            })
        })
    });
    vcx.update(|window, cx| pane.update(cx, |p, cx| p.send(window, cx)));
    pump(shell, vcx);
}

fn previous_branch_enabled(vcx: &mut VisualTestContext) -> bool {
    let tree = tree(vcx);
    let arrows: Vec<bool> = of_role(&nodes(&tree), Role::Button)
        .iter()
        .filter(|n| n.label().as_deref() == Some("Previous branch"))
        .map(|n| !n.is_disabled())
        .collect();
    assert_eq!(arrows.len(), 1, "one Previous branch arrow");
    arrows[0]
}

#[gpui::test]
fn a_refused_branch_switch_keeps_the_branch_and_frees_the_arrows(cx: &mut TestAppContext) {
    let (shell, vcx) = open_shell_with(cx, "chat-branch-refused", lamp(), Vec::new(), |core, _| {
        core.fail_next_branch(standin::BranchFailure::Refused(
            "Stop the running turn before switching branches".into(),
        ))
    });
    second_version(&shell, vcx);
    let before = articles(vcx);
    assert!(previous_branch_enabled(vcx));
    press(&shell, vcx, Role::Button, "Previous branch");
    assert_eq!(articles(vcx), before, "the branch on screen is unchanged");
    assert_eq!(
        notices(vcx),
        ["Error, Stop the running turn before switching branches"]
    );
    assert!(
        previous_branch_enabled(vcx),
        "the unchanged transcript frees the arrows"
    );
}

#[gpui::test]
fn a_branch_switch_on_a_chat_that_cannot_be_read_frees_the_arrows(cx: &mut TestAppContext) {
    let (shell, vcx) = open_shell_with(
        cx,
        "chat-branch-unreadable",
        lamp(),
        Vec::new(),
        |core, _| {
            core.fail_next_branch(standin::BranchFailure::Unreadable(
                "the store is gone".into(),
            ))
        },
    );
    second_version(&shell, vcx);
    press(&shell, vcx, Role::Button, "Previous branch");
    assert!(
        names(vcx, Role::Alert)
            .iter()
            .any(|n| n == "This chat could not be read: the store is gone")
    );
    assert!(
        previous_branch_enabled(vcx),
        "the failed read frees the arrows"
    );
}

#[gpui::test]
fn files_the_core_refuses_go_back_to_the_draft_with_its_sentence(cx: &mut TestAppContext) {
    let dir = TempDir::new("chat-core-limit-picks");
    let files = write_files(&dir.0);
    let refusal = "Attachment \"door.png\" exceeds 100 MB";
    let (shell, vcx) = open_shell_with(cx, "chat-core-limit", lamp(), files, move |core, _| {
        core.refuse_sends_with("chat-lamp", refusal)
    });
    let before = articles(vcx);
    press(&shell, vcx, Role::Button, "Attach file");
    send(&shell, vcx, "Here are the plans");
    assert_eq!(articles(vcx), before);
    assert_eq!(notices(vcx), [format!("Error, {refusal}")]);
    assert_eq!(composer_text(&shell, vcx), "Here are the plans");
    let groups = names(vcx, Role::Group);
    assert!(
        groups.iter().any(|n| n == "notes.txt, plain"),
        "{groups:#?}"
    );
    assert!(groups.iter().any(|n| n == "door.png, jpeg"), "{groups:#?}");
    drop(dir);
}

#[gpui::test]
fn a_new_chat_keeps_the_model_of_the_chat_just_left(cx: &mut TestAppContext) {
    let (shell, vcx) = open_shell_with(cx, "chat-new-model", lamp(), Vec::new(), |_, _| {});
    let pane = pane(&shell, vcx);
    let pickers = vcx.update(|_, cx| pane.read(cx).pickers());
    let model = |vcx: &mut VisualTestContext| {
        vcx.update(|_, cx| {
            let choice = pickers.read(cx).choice();
            (choice.provider.clone(), choice.model.clone())
        })
    };
    // At the start: the last model of the preferences.
    assert_eq!(model(vcx), ("anthropic".into(), standin::MODEL.into()));
    // Another model picked in this chat becomes the last model too.
    vcx.update(|_, cx| {
        pickers.update(cx, |p, cx| {
            p.pick_model("claude-code".into(), "claude-haiku-5-5".into(), cx)
        })
    });
    // Leaving for a chat of its own model, then the new chat: it keeps the
    // model of the chat just left, not the last one picked.
    press(&shell, vcx, Role::ListItem, "Castle gate");
    assert_eq!(model(vcx), ("anthropic".into(), standin::MODEL.into()));
    press(&shell, vcx, Role::Button, "New chat");
    assert_eq!(model(vcx), ("anthropic".into(), standin::MODEL.into()));
}

#[gpui::test]
fn favourites_the_core_publishes_are_applied_and_a_failed_star_goes_back(cx: &mut TestAppContext) {
    let (shell, vcx) = open_shell_with(cx, "chat-favourites", lamp(), Vec::new(), |_, _| {});
    let catalog = vcx.update(|_, cx| mcbench_ui::view::pickers::catalog::Catalog::installed(cx));
    pump(&shell, vcx);
    let loaded = vcx.update(|_, cx| catalog.read(cx).favorites());
    assert!(!loaded.is_empty(), "the core's list at attach");
    // Published while no star is being saved (another window's change).
    let list = vec![("anthropic".to_string(), "claude-haiku-5-5".to_string())];
    vcx.update(|window, cx| {
        shell.entity.update(cx, |s, cx| {
            s.apply(mcbench_ui::Update::Favorites(list.clone()), window, cx)
        })
    });
    pump(&shell, vcx);
    assert_eq!(vcx.update(|_, cx| catalog.read(cx).favorites()), list);
    // A save the core refuses: its sentence, then its unchanged list (the
    // stand-in's own, which the list published above did not change), and
    // the star goes back.
    let (provider, model) = standin::STAR_FAILS;
    vcx.update(|_, cx| catalog.update(cx, |c, cx| c.toggle_favorite(provider, model, cx)));
    pump(&shell, vcx);
    assert_eq!(vcx.update(|_, cx| catalog.read(cx).favorites()), loaded);
    assert_eq!(notices(vcx), [format!("Error, {}", standin::STAR_FAILURE)]);
}

/// Whether the button named `name` says it is expanded.
fn expanded(vcx: &mut VisualTestContext, name: &str) -> bool {
    let tree = tree(vcx);
    named(&nodes(&tree), Role::Button, name)
        .data()
        .is_expanded()
        .expect("a popover's trigger says whether it is expanded")
}

fn pointer_on(shell: &ShellUnderTest, vcx: &mut VisualTestContext, name: &str) {
    let tree = tree(vcx);
    let node = named(&nodes(&tree), Role::Button, name);
    pointer_click(vcx, node);
    pump(shell, vcx);
}

#[gpui::test]
fn a_press_on_an_open_popovers_trigger_closes_it(cx: &mut TestAppContext) {
    let (shell, vcx) = open_shell_with(cx, "chat-popover-trigger", lamp(), Vec::new(), |_, _| {});
    // The shared popover with the shared trigger (the sidebar's filter) and
    // with a picker's own trigger (the effort picker): one press opens it,
    // the next press on its trigger closes it and it stays closed.
    for name in ["Filter", "Reasoning effort: medium"] {
        assert!(!expanded(vcx, name), "{name} starts closed");
        pointer_on(&shell, vcx, name);
        assert!(expanded(vcx, name), "{name} opens on a press");
        pointer_on(&shell, vcx, name);
        vcx.run_until_parked();
        assert!(
            !expanded(vcx, name),
            "{name} closes on a press of its trigger"
        );
    }
}

#[gpui::test]
fn after_settings_close_the_model_picker_opens_by_itself_then_the_composer_takes_focus(
    cx: &mut TestAppContext,
) {
    // No model chosen yet, an AI connected, the new chat on screen.
    let (shell, vcx) = open_shell_preferring(
        cx,
        "chat-ask-model",
        Some(Place::NewChat { id: None }),
        Vec::new(),
        mcbench_ui::Preferences::default(),
        |_, _| {},
    );
    vcx.update(|window, cx| shell.entity.update(cx, |s, cx| s.open_settings(window, cx)));
    pump(&shell, vcx);
    vcx.update(|window, cx| {
        shell
            .entity
            .update(cx, |s, cx| s.close_settings(window, cx))
    });
    pump(&shell, vcx);
    assert!(
        names(vcx, Role::Dialog).iter().any(|n| n == "Chat model"),
        "the model picker opened by itself: {:?}",
        names(vcx, Role::Dialog)
    );
    let option = {
        let tree = tree(vcx);
        of_role(&nodes(&tree), Role::ListBoxOption)
            .first()
            .map(|n| n.locate().0)
            .expect("a model to choose")
    };
    vcx.update(|window, cx| {
        window.dispatch_a11y_action_for_test(
            gpui::accesskit::ActionRequest {
                action: gpui::accesskit::Action::Click,
                target_tree: gpui::accesskit::TreeId::ROOT,
                target_node: option,
                data: None,
            },
            cx,
        )
    });
    pump(&shell, vcx);
    let (role, name) = focused(vcx);
    assert_eq!(
        (role, name.as_deref()),
        (Role::MultilineTextInput, Some("Message"))
    );
    let pane = pane(&shell, vcx);
    assert!(vcx.update(|_, cx| pane.read(cx).pickers().read(cx).choice().chosen()));
}

#[gpui::test]
fn typing_in_an_open_pickers_search_stays_there(cx: &mut TestAppContext) {
    let (shell, vcx) = open_shell_with(cx, "chat-type-picker", lamp(), Vec::new(), |_, _| {});
    press(&shell, vcx, Role::Button, "Chat model: claude-opus-5-5");
    assert!(names(vcx, Role::Dialog).iter().any(|n| n == "Chat model"));
    vcx.simulate_keystrokes("h a i");
    pump(&shell, vcx);
    assert_eq!(
        composer_text(&shell, vcx),
        "",
        "the composer took none of it"
    );
    let (role, _) = focused(vcx);
    assert_eq!(role, Role::TextInput, "the picker's search keeps focus");
    let search = {
        let tree = tree(vcx);
        let all = nodes(&tree);
        let node = all
            .iter()
            .find(|n| n.role() == Role::TextInput && n.is_focused())
            .expect("the focused search");
        node.value().unwrap_or_default()
    };
    assert_eq!(search, "hai");
}

#[gpui::test]
fn a_dismissal_the_core_refuses_brings_the_error_back_with_its_sentence(cx: &mut TestAppContext) {
    let (shell, vcx) = open_shell_with(
        cx,
        "chat-dismiss-refused",
        Some(Place::Chat {
            id: standin::FAILING_CHAT.into(),
        }),
        Vec::new(),
        |core, _| core.fail_next_dismiss("Wait for the running turn before dismissing an error"),
    );
    send(&shell, vcx, "hi");
    let error = format!("Error, {}", standin::FAILURE);
    press(&shell, vcx, Role::Button, "Dismiss error");
    assert!(
        articles(vcx).contains(&error),
        "the unchanged transcript shows it again"
    );
    assert_eq!(
        notices(vcx),
        ["Error, Wait for the running turn before dismissing an error"]
    );
}

#[gpui::test]
fn the_stand_ins_pending_setup_holds_a_send_until_it_finishes(cx: &mut TestAppContext) {
    // `mcbench-ui-dev --setup-pending`: the stand-in publishes the app's
    // steps with one still running, so a send is held with its line and
    // `Don't send`; finishing the step sends it.
    let (shell, core, vcx) = open_standin(cx, "chat-setup-pending", gate_fixture(), lamp());
    core.setup_pending();
    pump(&shell, vcx);
    send(&shell, vcx, "Hold this");
    assert!(logged(&shell, "sent").is_empty());
    assert!(
        names(vcx, Role::Status)
            .iter()
            .any(|n| n == "Sends when the app is ready. 1 of 2 done."),
        "{:?}",
        names(vcx, Role::Status)
    );
    assert!(names(vcx, Role::Button).iter().any(|n| n == "Don't send"));
    core.finish_setup();
    pump(&shell, vcx);
    pump(&shell, vcx);
    assert_eq!(logged(&shell, "sent").len(), 1);
    assert!(
        !names(vcx, Role::Button).iter().any(|n| n == "Don't send"),
        "the line goes with the send"
    );
}
