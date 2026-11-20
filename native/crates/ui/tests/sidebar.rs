//! The sidebar's contract (ACCESSIBILITY.md, the sidebar's rows): its
//! lists, their states and names, the archived section, the search and
//! its filters, Bench's list and prompts, and the palette, read as an
//! adapter reads the tree and driven by keys and a reader's actions.

mod common;

use std::time::Duration;

use common::*;
use gpui::accesskit::{Action, Role};
use gpui::{TestAppContext, VisualTestContext};
use mcbench_ui::state::place::Place;

/// Applies what the core published, then lets 200 ms pass (the searches'
/// 90 ms wait, a menu's 110 ms exit) and applies again.
fn settle(shell: &ShellUnderTest, vcx: &mut VisualTestContext) {
    pump(shell, vcx);
    vcx.executor().advance_clock(Duration::from_millis(200));
    pump(shell, vcx);
}

fn place(shell: &ShellUnderTest, vcx: &mut VisualTestContext) -> Place {
    vcx.update(|_, cx| shell.entity.read(cx).place().clone())
}

fn labels(vcx: &mut VisualTestContext, role: Role) -> Vec<String> {
    let tree = tree(vcx);
    of_role(&nodes(&tree), role)
        .iter()
        .filter_map(|n| n.label())
        .collect()
}

fn focus(vcx: &mut VisualTestContext, role: Role, label: &str) {
    let tree = tree(vcx);
    let node = named(&nodes(&tree), role, label);
    act(vcx, node, Action::Focus);
}

fn press(vcx: &mut VisualTestContext, role: Role, label: &str) {
    let tree = tree(vcx);
    let node = named(&nodes(&tree), role, label);
    click(vcx, node);
}

fn mod_key(key: &str) -> String {
    if cfg!(target_os = "macos") {
        format!("cmd-{key}")
    } else {
        format!("ctrl-{key}")
    }
}

#[gpui::test]
fn the_build_list_says_each_rows_status_in_its_name(cx: &mut TestAppContext) {
    let (shell, core, vcx) = open_standin(cx, "names", gate_fixture(), None);
    core.set_turn("chat-gate", true, false, None);
    pump(&shell, vcx);
    let tree = tree(vcx);
    let nodes = nodes(&tree);
    let list = named(&nodes, Role::List, "Chats");
    let rows: Vec<String> = list.children().filter_map(|r| r.label()).collect();
    assert_eq!(
        rows,
        [
            "Redstone lamp with a lever, open",
            "Castle gate, Working",
            "Wheat farm, Finished, not opened yet, Resolution: Very high (5\u{d7})",
        ]
    );
    // Archive buttons: the running chat's is disabled, and says why.
    let buttons: Vec<_> = of_role(&nodes, Role::Button)
        .into_iter()
        .filter(|b| b.label().as_deref() == Some("Archive build"))
        .collect();
    assert_eq!(buttons.len(), 3);
    assert!(buttons[1].is_disabled());
    assert_eq!(
        buttons[1].description().as_deref(),
        Some("Stop the running turn before archiving")
    );
    assert!(!buttons[0].is_disabled());
    let _ = shell;
}

#[gpui::test]
fn walking_the_list_opens_nothing_and_enter_or_space_opens_the_focused_row(
    cx: &mut TestAppContext,
) {
    let (shell, core, vcx) = open_standin(cx, "walk", gate_fixture(), None);
    core.set_turn("chat-gate", true, false, None);
    pump(&shell, vcx);
    focus(vcx, Role::ListItem, "Redstone lamp with a lever, open");
    vcx.simulate_keystrokes("down");
    pump(&shell, vcx);
    assert_eq!(focused(vcx).1.as_deref(), Some("Castle gate, Working"));
    assert_eq!(
        place(&shell, vcx),
        Place::Chat {
            id: "chat-lamp".into()
        },
        "walking opens nothing"
    );
    vcx.simulate_keystrokes("space");
    pump(&shell, vcx);
    assert_eq!(
        place(&shell, vcx),
        Place::Chat {
            id: "chat-gate".into()
        }
    );
    vcx.simulate_keystrokes("down");
    pump(&shell, vcx);
    vcx.simulate_keystrokes("enter");
    pump(&shell, vcx);
    assert_eq!(
        place(&shell, vcx),
        Place::Chat {
            id: "chat-farm".into()
        }
    );
    // Opening it marked it seen: its row is named by its title (and tag).
    let tree = tree(vcx);
    assert_eq!(
        row(&nodes(&tree), "Wheat farm").label().as_deref(),
        Some("Wheat farm, open, Resolution: Very high (5\u{d7})")
    );
    let seen = log_lines(&shell.log);
    assert!(
        seen.iter()
            .any(|l| l["action"] == "opened chat" && l["chat"] == "chat-farm")
    );
}

#[gpui::test]
fn a_chat_runs_then_finishes_unseen_and_its_name_follows(cx: &mut TestAppContext) {
    let (shell, core, vcx) = open_standin(cx, "turns", gate_fixture(), None);
    core.set_turn("chat-farm", false, false, None);
    pump(&shell, vcx);
    core.set_turn("chat-farm", true, false, None);
    pump(&shell, vcx);
    let tree1 = tree(vcx);
    assert!(
        row(&nodes(&tree1), "Wheat farm")
            .label()
            .is_some_and(|l| l.starts_with("Wheat farm, Working"))
    );
    core.set_turn("chat-farm", false, true, None);
    pump(&shell, vcx);
    let tree2 = tree(vcx);
    assert!(
        row(&nodes(&tree2), "Wheat farm")
            .label()
            .is_some_and(|l| l.starts_with("Wheat farm, Finished, not opened yet"))
    );
    // The open chat finishing is seen at once.
    core.set_turn("chat-lamp", true, false, None);
    pump(&shell, vcx);
    core.set_turn("chat-lamp", false, true, None);
    pump(&shell, vcx);
    pump(&shell, vcx);
    let tree3 = tree(vcx);
    assert_eq!(
        row(&nodes(&tree3), "Redstone lamp with a lever")
            .label()
            .as_deref(),
        Some("Redstone lamp with a lever, open")
    );
}

#[gpui::test]
fn archiving_the_open_chat_moves_to_the_next_row_and_restore_brings_it_back(
    cx: &mut TestAppContext,
) {
    let (shell, _core, vcx) = open_standin(cx, "archive", gate_fixture(), None);
    let tree1 = tree(vcx);
    let archive = of_role(&nodes(&tree1), Role::Button)
        .into_iter()
        .find(|b| b.label().as_deref() == Some("Archive build"))
        .expect("the first row's archive button");
    click(vcx, archive);
    settle(&shell, vcx);
    // The next row below takes the open chat's place.
    assert_eq!(
        place(&shell, vcx),
        Place::Chat {
            id: "chat-gate".into()
        }
    );
    assert!(
        !labels(vcx, Role::ListItem)
            .iter()
            .any(|l| l.starts_with("Redstone"))
    );
    // The archived section: closed, then read.
    let tree2 = tree(vcx);
    let toggle = named(&nodes(&tree2), Role::Button, "Show archived");
    assert_eq!(toggle.data().is_expanded(), Some(false));
    click(vcx, toggle);
    settle(&shell, vcx);
    let tree3 = tree(vcx);
    let n3 = nodes(&tree3);
    assert_eq!(
        named(&n3, Role::Button, "Hide archived")
            .data()
            .is_expanded(),
        Some(true)
    );
    let archived = named(&n3, Role::List, "Archived");
    let titles: Vec<String> = archived.children().filter_map(|r| r.label()).collect();
    assert_eq!(titles.len(), 2);
    assert!(
        titles
            .iter()
            .any(|t| t == "Redstone lamp with a lever, 7 days left")
    );
    assert!(titles.iter().any(|t| t.starts_with("Old windmill, ")));
    // Restore opens it once listed.
    let restore = of_role(&n3, Role::Button)
        .into_iter()
        .find(|b| b.label().as_deref() == Some("Restore"))
        .expect("a restore button");
    click(vcx, restore);
    settle(&shell, vcx);
    settle(&shell, vcx);
    assert!(
        labels(vcx, Role::ListItem)
            .iter()
            .any(|l| l.starts_with("Redstone"))
    );
    assert_eq!(
        place(&shell, vcx),
        Place::Chat {
            id: "chat-lamp".into()
        }
    );
}

#[gpui::test]
fn delete_asks_first_for_four_seconds(cx: &mut TestAppContext) {
    let (shell, _core, vcx) = open_standin(cx, "delete", gate_fixture(), None);
    press(vcx, Role::Button, "Show archived");
    settle(&shell, vcx);
    press(vcx, Role::Button, "Delete permanently");
    pump(&shell, vcx);
    let tree1 = tree(vcx);
    let n1 = nodes(&tree1);
    named(&n1, Role::Button, "Delete for good");
    // Arming says the armed text once, in the polite region (the one
    // status there is).
    let said = |vcx: &mut VisualTestContext| {
        let tree = tree(vcx);
        of_role(&nodes(&tree), Role::Status)
            .iter()
            .filter(|n| n.live() == gpui::accesskit::Live::Polite)
            .map(|n| n.label())
            .collect::<Vec<_>>()
    };
    assert_eq!(said(vcx), [Some("Delete for good?".to_string())]);
    assert!(
        labels(vcx, Role::ListItem)
            .iter()
            .any(|l| l == "Old windmill, Delete for good?")
    );
    let statuses = {
        let tree = tree(vcx);
        of_role(&nodes(&tree), Role::Status).len()
    };
    // After 4 s it asks again, and disarming says nothing new.
    vcx.executor().advance_clock(Duration::from_millis(4000));
    pump(&shell, vcx);
    assert_eq!(said(vcx), [Some("Delete for good?".to_string())]);
    assert_eq!(
        {
            let tree = tree(vcx);
            of_role(&nodes(&tree), Role::Status).len()
        },
        statuses
    );
    press(vcx, Role::Button, "Delete permanently");
    pump(&shell, vcx);
    vcx.executor().advance_clock(Duration::from_millis(3000));
    press(vcx, Role::Button, "Delete for good");
    settle(&shell, vcx);
    // The windmill is gone and the section, with nothing left, closed.
    assert!(
        !labels(vcx, Role::ListItem)
            .iter()
            .any(|l| l.starts_with("Old windmill"))
    );
    assert!(
        !labels(vcx, Role::Button)
            .iter()
            .any(|l| l == "Show archived" || l == "Hide archived")
    );
}

#[gpui::test]
fn a_failed_archive_read_says_so_and_can_be_tried_again(cx: &mut TestAppContext) {
    let (shell, core, vcx) = open_standin(cx, "archive-fails", gate_fixture(), None);
    core.fail_archive_reads(true);
    press(vcx, Role::Button, "Show archived");
    settle(&shell, vcx);
    let tree1 = tree(vcx);
    named(
        &nodes(&tree1),
        Role::Alert,
        "Archived items failed to load.",
    );
    core.fail_archive_reads(false);
    press(vcx, Role::Button, "Try again");
    settle(&shell, vcx);
    assert!(
        labels(vcx, Role::ListItem)
            .iter()
            .any(|l| l.starts_with("Old windmill"))
    );
}

#[gpui::test]
fn search_narrows_the_list_by_rank_with_its_snippet_and_escape_clears_it(cx: &mut TestAppContext) {
    let (shell, _core, vcx) = open_standin(cx, "search", gate_fixture(), None);
    focus(vcx, Role::TextInput, "Search chats");
    vcx.simulate_input("portcullis");
    settle(&shell, vcx);
    let rows = labels(vcx, Role::ListItem);
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert!(
        rows[0].starts_with("Castle gate, ") && rows[0].contains("portcullis"),
        "the snippet is in the name: {rows:?}"
    );
    focus(vcx, Role::TextInput, "Search chats");
    vcx.simulate_input(" moat");
    settle(&shell, vcx);
    let tree1 = tree(vcx);
    named(&nodes(&tree1), Role::Label, "No matches");
    vcx.simulate_keystrokes("escape");
    pump(&shell, vcx);
    assert_eq!(
        labels(vcx, Role::ListItem).len(),
        3,
        "the search is cleared"
    );
}

#[gpui::test]
fn an_answer_to_an_older_search_is_dropped(cx: &mut TestAppContext) {
    let (shell, _core, vcx) = open_standin(cx, "old-answer", gate_fixture(), None);
    focus(vcx, Role::TextInput, "Search chats");
    vcx.simulate_input("gate");
    settle(&shell, vcx);
    assert_eq!(labels(vcx, Role::ListItem).len(), 1);
    let sidebar = vcx.update(|_, cx| shell.entity.read(cx).sidebar());
    vcx.update(|_, cx| {
        sidebar.update(cx, |s, cx| {
            s.found(
                0,
                Ok(vec![mcbench_ui::SearchHit {
                    id: "chat-farm".into(),
                    snippet: None,
                }]),
                cx,
            )
        })
    });
    pump(&shell, vcx);
    assert!(
        labels(vcx, Role::ListItem)[0].starts_with("Castle gate"),
        "request 0 is not the field's"
    );
}

#[gpui::test]
fn the_filter_menu_takes_focus_wraps_and_gives_it_back(cx: &mut TestAppContext) {
    let (shell, _core, vcx) = open_standin(cx, "filters", gate_fixture(), None);
    press(vcx, Role::Button, "Filter");
    pump(&shell, vcx);
    let tree1 = tree(vcx);
    let n1 = nodes(&tree1);
    let menu = named(&n1, Role::Menu, "Filter");
    let groups: Vec<String> = menu.children().filter_map(|g| g.label()).collect();
    assert_eq!(groups, ["Size", "Prompt", "Model"]);
    assert_eq!(
        focused(vcx),
        (Role::MenuItemCheckBox, Some("Standard".into()))
    );
    vcx.simulate_keystrokes("up");
    pump(&shell, vcx);
    let last = focused(vcx).1.expect("a value");
    assert_eq!(
        last, "claude-opus-5-5",
        "Up from the first wraps to the last"
    );
    vcx.simulate_keystrokes("down");
    pump(&shell, vcx);
    assert_eq!(focused(vcx).1.as_deref(), Some("Standard"));
    // The prompts the chats came from, one deleted from the library.
    let items = labels(vcx, Role::MenuItemCheckBox);
    assert!(items.contains(&"Wheat field".to_string()));
    assert!(items.contains(&"Red barn".to_string()));
    // Choose Very high: the menu stays open; the list narrows.
    press(vcx, Role::MenuItemCheckBox, "Very high (5\u{d7})");
    pump(&shell, vcx);
    assert_eq!(labels(vcx, Role::ListItem).len(), 1);
    vcx.simulate_keystrokes("escape");
    settle(&shell, vcx);
    assert_eq!(
        focused(vcx),
        (Role::Button, Some("Filter, 1 chosen".into()))
    );
    // A chip per value, named by what it removes.
    press(vcx, Role::Button, "Filter, 1 chosen");
    pump(&shell, vcx);
    press(vcx, Role::MenuItemCheckBox, "Wheat field");
    pump(&shell, vcx);
    // The menu's exit takes 110 ms; it takes no input meanwhile.
    vcx.simulate_keystrokes("escape");
    settle(&shell, vcx);
    let chips = labels(vcx, Role::Button)
        .into_iter()
        .filter(|l| l.starts_with("Remove filter"))
        .collect::<Vec<_>>();
    assert_eq!(
        chips,
        [
            "Remove filter Size: Very high (5\u{d7})",
            "Remove filter Prompt: Wheat field"
        ]
    );
    // Removing the first moves focus to the next chip; the last to the
    // filter button.
    press(vcx, Role::Button, "Remove filter Size: Very high (5\u{d7})");
    pump(&shell, vcx);
    assert_eq!(
        focused(vcx).1.as_deref(),
        Some("Remove filter Prompt: Wheat field")
    );
    press(vcx, Role::Button, "Remove filter Prompt: Wheat field");
    pump(&shell, vcx);
    assert_eq!(focused(vcx), (Role::Button, Some("Filter".into())));
    assert_eq!(labels(vcx, Role::ListItem).len(), 3);
}

#[gpui::test]
fn a_chosen_model_no_listed_row_has_leaves_and_comes_back(cx: &mut TestAppContext) {
    let (shell, _core, vcx) = open_standin(cx, "in-effect", gate_fixture(), None);
    press(vcx, Role::Button, "Filter");
    pump(&shell, vcx);
    press(vcx, Role::MenuItemCheckBox, "Red barn");
    pump(&shell, vcx);
    vcx.simulate_keystrokes("escape");
    settle(&shell, vcx);
    assert!(
        labels(vcx, Role::Button)
            .contains(&"Remove filter Prompt: Red barn (deleted from the library)".to_string())
    );
    // Wheat farm, the one chat sent from it, is archived: the chip goes.
    let tree1 = tree(vcx);
    let farm_archive = of_role(&nodes(&tree1), Role::Button)
        .into_iter()
        .find(|b| b.label().as_deref() == Some("Archive build"))
        .expect("Wheat farm's archive button");
    click(vcx, farm_archive);
    settle(&shell, vcx);
    assert!(
        !labels(vcx, Role::Button)
            .iter()
            .any(|l| l.starts_with("Remove filter"))
    );
    assert_eq!(labels(vcx, Role::ListItem).len(), 2, "no filter in effect");
    // Restored, it comes back.
    press(vcx, Role::Button, "Show archived");
    settle(&shell, vcx);
    let tree2 = tree(vcx);
    let restore = of_role(&nodes(&tree2), Role::Button)
        .into_iter()
        .find(|b| b.label().as_deref() == Some("Restore"))
        .expect("restore");
    click(vcx, restore);
    settle(&shell, vcx);
    settle(&shell, vcx);
    assert!(
        labels(vcx, Role::Button)
            .iter()
            .any(|l| l.starts_with("Remove filter Prompt: Red barn"))
    );
}

#[gpui::test]
fn bench_lists_folds_its_models_and_its_prompts(cx: &mut TestAppContext) {
    let (shell, core, vcx) = open_standin(cx, "bench", gate_fixture(), None);
    // The other mode's mark: a model finished unseen.
    core.set_turn("member-a", false, true, None);
    pump(&shell, vcx);
    let tree0 = tree(vcx);
    let switch = named(&nodes(&tree0), Role::Button, "mcbuild");
    assert!(
        switch
            .description()
            .is_some_and(|d| d.starts_with("Switch to Bench \u{b7} 1 finished")),
        "{:?}",
        switch.description()
    );
    vcx.simulate_keystrokes(&mod_key("2"));
    settle(&shell, vcx);
    let tree1 = tree(vcx);
    let n1 = nodes(&tree1);
    named(&n1, Role::Button, "Leaderboard");
    let benches = named(&n1, Role::List, "Benches");
    let bench = benches.children().next().expect("a bench");
    assert_eq!(
        bench.label().as_deref(),
        Some("Towers and a bridge, Finished, not opened yet")
    );
    let fold = named(&n1, Role::Button, "Show models");
    assert_eq!(fold.data().is_expanded(), Some(false));
    act(vcx, fold, Action::Expand);
    pump(&shell, vcx);
    let tree2 = tree(vcx);
    let n2 = nodes(&tree2);
    assert_eq!(
        named(&n2, Role::Button, "Hide models").data().is_expanded(),
        Some(true)
    );
    let models: Vec<String> = named(&n2, Role::List, "Towers and a bridge models")
        .children()
        .filter_map(|m| m.label())
        .collect();
    assert_eq!(
        models,
        [
            "Build A, Stopped or failed",
            "Build B, Finished, not opened yet"
        ]
    );
    assert_eq!(
        place(&shell, vcx),
        Place::NewBench,
        "folding opens no bench"
    );
    press(vcx, Role::ListItem, "Build B, Finished, not opened yet");
    settle(&shell, vcx);
    assert_eq!(
        place(&shell, vcx),
        Place::Member {
            bench: "bench-towers".into(),
            member: "member-a".into()
        }
    );
    // The prompts below, by title; a row opens the prompt place.
    let prompts = labels(vcx, Role::ListItem);
    assert!(
        prompts.contains(&"Wheat field".to_string()) && prompts.contains(&"Two towers".to_string())
    );
    press(vcx, Role::ListItem, "Two towers");
    pump(&shell, vcx);
    assert_eq!(place(&shell, vcx), Place::Prompt);
    // The search button grows into its field.
    press(vcx, Role::Button, "Search prompts");
    pump(&shell, vcx);
    assert_eq!(
        focused(vcx),
        (Role::TextInput, Some("Search prompts".into()))
    );
    vcx.simulate_input("tower");
    pump(&shell, vcx);
    let after: Vec<String> = labels(vcx, Role::ListItem)
        .into_iter()
        .filter(|l| l == "Wheat field" || l == "Two towers")
        .collect();
    assert_eq!(after, ["Two towers"]);
}

#[gpui::test]
fn archiving_the_open_bench_moves_to_the_new_bench(cx: &mut TestAppContext) {
    let (shell, _core, vcx) = open_standin(
        cx,
        "bench-archive",
        gate_fixture(),
        Some(Place::Compare {
            bench: "bench-towers".into(),
        }),
    );
    settle(&shell, vcx);
    press(vcx, Role::Button, "Archive bench");
    settle(&shell, vcx);
    assert_eq!(place(&shell, vcx), Place::NewBench);
    press(vcx, Role::Button, "Show archived");
    settle(&shell, vcx);
    let rows = labels(vcx, Role::ListItem);
    assert!(
        rows.iter()
            .any(|r| r == "Towers and a bridge, 2 models \u{b7} 7 days left"),
        "{rows:?}"
    );
    assert!(
        rows.iter()
            .any(|r| r.starts_with("Old lighthouse, 3 models \u{b7} "))
    );
}

#[gpui::test]
fn the_palette_opens_over_everything_and_runs_what_it_highlights(cx: &mut TestAppContext) {
    let (shell, _core, vcx) = open_standin(cx, "palette", gate_fixture(), None);
    vcx.simulate_keystrokes(&mod_key("k"));
    pump(&shell, vcx);
    let tree1 = tree(vcx);
    let n1 = nodes(&tree1);
    named(&n1, Role::Dialog, "Search everything");
    assert_eq!(
        focused(vcx),
        (Role::TextInput, Some("Search everything".into()))
    );
    let groups: Vec<String> = of_role(&n1, Role::Group)
        .iter()
        .filter_map(|g| g.label())
        .filter(|l| ["Recent builds", "Recent benches", "Actions"].contains(&l.as_str()))
        .collect();
    assert_eq!(groups, ["Recent builds", "Recent benches", "Actions"]);
    let selected: Vec<_> = of_role(&n1, Role::ListBoxOption)
        .into_iter()
        .filter(|o| o.is_selected() == Some(true))
        .collect();
    assert_eq!(selected.len(), 1);
    assert_eq!(
        selected[0].label().as_deref(),
        Some("Redstone lamp with a lever")
    );
    vcx.simulate_keystrokes("up");
    pump(&shell, vcx);
    let tree2 = tree(vcx);
    let selected = of_role(&nodes(&tree2), Role::ListBoxOption)
        .into_iter()
        .find(|o| o.is_selected() == Some(true))
        .and_then(|o| o.label());
    assert_eq!(
        selected.as_deref(),
        Some("Settings"),
        "Up wraps to the last"
    );
    // A query: the core's builds arrive after 90 ms.
    vcx.simulate_input("gate");
    pump(&shell, vcx);
    let tree3 = tree(vcx);
    named(&nodes(&tree3), Role::Label, "Searching\u{2026}");
    settle(&shell, vcx);
    let tree4 = tree(vcx);
    let n4 = nodes(&tree4);
    let builds = of_role(&n4, Role::Group)
        .into_iter()
        .find(|g| g.label().as_deref() == Some("Builds"))
        .expect("the core's builds");
    assert_eq!(
        builds
            .children()
            .filter_map(|b| b.label())
            .next()
            .as_deref()
            .map(|l| l.starts_with("Castle gate")),
        Some(true)
    );
    vcx.simulate_keystrokes("enter");
    settle(&shell, vcx);
    assert_eq!(
        place(&shell, vcx),
        Place::Chat {
            id: "chat-gate".into()
        }
    );
    assert!(!has(&nodes(&tree(vcx)), Role::Dialog, "Search everything"));
    // Nothing matches.
    vcx.simulate_keystrokes(&mod_key("k"));
    pump(&shell, vcx);
    vcx.simulate_input("zzzq");
    settle(&shell, vcx);
    named(
        &nodes(&tree(vcx)),
        Role::Label,
        "Nothing matches \u{201c}zzzq\u{201d}",
    );
    vcx.simulate_keystrokes("escape");
    pump(&shell, vcx);
    assert!(!has(&nodes(&tree(vcx)), Role::Dialog, "Search everything"));
}

#[gpui::test]
fn a_setting_found_in_the_palette_opens_settings_and_names_its_row(cx: &mut TestAppContext) {
    let (shell, _core, vcx) = open_standin(cx, "palette-setting", gate_fixture(), None);
    vcx.simulate_keystrokes(&mod_key("k"));
    pump(&shell, vcx);
    vcx.simulate_input("title model");
    settle(&shell, vcx);
    vcx.simulate_keystrokes("enter");
    settle(&shell, vcx);
    let tree1 = tree(vcx);
    named(&nodes(&tree1), Role::Dialog, "Settings");
    let sidebar = vcx.update(|_, cx| shell.entity.read(cx).sidebar());
    let asked = vcx.update(|_, cx| sidebar.read(cx).palette().read(cx).asked_setting.clone());
    assert_eq!(asked.as_deref(), Some("Title model"));
}

#[gpui::test]
fn the_missing_screen_restores_an_archived_chat_and_opens_it(cx: &mut TestAppContext) {
    let (shell, _core, vcx) = open_standin(
        cx,
        "missing",
        gate_fixture(),
        Some(Place::Chat {
            id: "chat-windmill".into(),
        }),
    );
    settle(&shell, vcx);
    press(vcx, Role::Button, "Restore");
    settle(&shell, vcx);
    settle(&shell, vcx);
    assert_eq!(
        place(&shell, vcx),
        Place::Chat {
            id: "chat-windmill".into()
        }
    );
}

#[gpui::test]
fn a_running_dot_steps_twenty_times_a_second_and_redraws_only_the_shell(cx: &mut TestAppContext) {
    let (shell, core, vcx) = open_standin(cx, "pulse", gate_fixture(), None);
    // The pulse moves only in an active window (`view::motion`); the test
    // platform opens it inactive.
    vcx.update(|window, _| window.activate_window());
    core.set_turn("chat-farm", true, false, None);
    settle(&shell, vcx);
    // Focus on a row, not in the composer, whose caret would blink.
    focus(vcx, Role::ListItem, "Castle gate");
    settle(&shell, vcx);
    let counts = |vcx: &mut VisualTestContext| {
        vcx.update(|_, cx| {
            let s = shell.entity.read(cx);
            (
                s.render_count(),
                s.sidebar().read(cx).render_count(),
                s.chat().read(cx).render_count(),
            )
        })
    };
    let (shell0, sidebar0, chat0) = counts(vcx);
    for _ in 0..20 {
        vcx.executor()
            .advance_clock(mcbench_ui::view::sidebar::dot::STEP);
        vcx.run_until_parked();
    }
    let (shell1, sidebar1, chat1) = counts(vcx);
    assert!(
        (19..=21).contains(&(shell1 - shell0)),
        "a second of pulse is 20 steps: {}",
        shell1 - shell0
    );
    assert_eq!(sidebar1, sidebar0, "the sidebar's frame is reused");
    assert_eq!(chat1, chat0, "the chat pane's frame is reused");
    // Nothing runs: no step.
    core.set_turn("chat-farm", false, false, None);
    settle(&shell, vcx);
    let (shell2, ..) = counts(vcx);
    for _ in 0..20 {
        vcx.executor()
            .advance_clock(mcbench_ui::view::sidebar::dot::STEP);
        vcx.run_until_parked();
    }
    assert!(counts(vcx).0 - shell2 <= 1, "no steps once nothing runs");
}

#[gpui::test]
fn the_archived_windmill_has_five_days_left_on_the_fixtures_today(cx: &mut TestAppContext) {
    let (shell, _core, vcx) = open_standin(cx, "days-left", gate_fixture(), None);
    let sidebar = vcx.update(|_, cx| shell.entity.read(cx).sidebar());
    vcx.update(|_, cx| sidebar.update(cx, |s, cx| s.set_now(mcbench_ui::dev::fixtures::TODAY, cx)));
    press(vcx, Role::Button, "Show archived");
    settle(&shell, vcx);
    // Archived 2026-11-15 10:00, deleted 7 days on: 5 days left on the 17th.
    assert!(
        labels(vcx, Role::ListItem).contains(&"Old windmill, 5 days left".to_string()),
        "{:?}",
        labels(vcx, Role::ListItem)
    );
}

#[gpui::test]
fn the_benches_folded_open_and_the_prompts_fold_are_kept_in_the_preferences(
    cx: &mut TestAppContext,
) {
    let (shell, _core, vcx) = open_standin(cx, "folds", gate_fixture(), None);
    vcx.simulate_keystrokes(&mod_key("2"));
    settle(&shell, vcx);
    press(vcx, Role::Button, "Show models");
    press(vcx, Role::Button, "Prompts");
    settle(&shell, vcx);
    let kept = vcx.update(|_, cx| {
        let p = shell.entity.read(cx).preferences().clone();
        (p.open_benches, p.prompts_open)
    });
    assert_eq!(kept, (vec!["bench-towers".to_string()], false));
}

#[gpui::test]
fn the_kept_folds_come_back_at_the_next_start(cx: &mut TestAppContext) {
    let preferences = mcbench_ui::Preferences {
        bench_mode: true,
        open_benches: vec!["bench-towers".into(), "bench-gone".into()],
        prompts_open: false,
        ..mcbench_ui::Preferences::default()
    };
    let (shell, _core, vcx) =
        open_standin_with(cx, "folds-back", gate_fixture(), None, preferences);
    settle(&shell, vcx);
    let tree = tree(vcx);
    let n = nodes(&tree);
    assert_eq!(
        named(&n, Role::Button, "Hide models").data().is_expanded(),
        Some(true)
    );
    assert_eq!(
        named(&n, Role::Button, "Prompts").data().is_expanded(),
        Some(false)
    );
    // A bench no longer listed is dropped, and that is kept.
    let kept = vcx.update(|_, cx| shell.entity.read(cx).preferences().open_benches.clone());
    assert_eq!(kept, ["bench-towers"]);
}

#[gpui::test]
fn the_open_bench_and_model_are_current_and_open_is_in_their_names(cx: &mut TestAppContext) {
    let (shell, _core, vcx) = open_standin(cx, "bench-open", gate_fixture(), None);
    vcx.simulate_keystrokes(&mod_key("2"));
    settle(&shell, vcx);
    let bench = |vcx: &mut VisualTestContext| {
        let tree = tree(vcx);
        let nodes = nodes(&tree);
        let b = named(&nodes, Role::List, "Benches")
            .children()
            .next()
            .expect("a bench");
        (
            b.label().unwrap_or_default(),
            b.aria_current(),
            b.description(),
        )
    };
    let says_open = |name: &str| name.split(", ").any(|part| part == "open");
    let (name, current, _) = bench(vcx);
    assert!(!says_open(&name), "{name}");
    assert_eq!(current, None);
    press(vcx, Role::ListItem, &name);
    settle(&shell, vcx);
    assert_eq!(
        place(&shell, vcx),
        Place::Compare {
            bench: "bench-towers".into()
        }
    );
    let (name, current, description) = bench(vcx);
    assert!(name.starts_with("Towers and a bridge, open"), "{name}");
    assert_eq!(current, Some(gpui::accesskit::AriaCurrent::Page));
    assert_eq!(description, None);
    press(vcx, Role::Button, "Show models");
    settle(&shell, vcx);
    let member = labels(vcx, Role::ListItem)
        .into_iter()
        .find(|l| l.starts_with("Build A"))
        .expect("Build A");
    press(vcx, Role::ListItem, &member);
    settle(&shell, vcx);
    let tree = tree(vcx);
    let nodes = nodes(&tree);
    let models: Vec<_> = named(&nodes, Role::List, "Towers and a bridge models")
        .children()
        .map(|m| (m.label().unwrap_or_default(), m.aria_current()))
        .collect();
    assert!(models[0].0.starts_with("Build A, open"), "{models:?}");
    assert_eq!(models[0].1, Some(gpui::accesskit::AriaCurrent::Page));
    assert!(
        !says_open(&models[1].0) && models[1].1.is_none(),
        "{models:?}"
    );
    let (name, current, _) = bench(vcx);
    assert!(!says_open(&name) && current.is_none(), "{name}");
}

#[gpui::test]
fn tab_out_of_the_filter_menu_goes_on_from_its_button(cx: &mut TestAppContext) {
    let (shell, _core, vcx) = open_standin(cx, "filter-tab", gate_fixture(), None);
    let menu_open = |vcx: &mut VisualTestContext| {
        let tree = tree(vcx);
        has(&nodes(&tree), Role::Menu, "Filter")
    };
    // Tab: the stop after the Filter button, the list's current row.
    press(vcx, Role::Button, "Filter");
    pump(&shell, vcx);
    vcx.simulate_keystrokes("down");
    vcx.simulate_keystrokes("tab");
    settle(&shell, vcx);
    assert!(!menu_open(vcx), "Tab closes the menu");
    assert_eq!(
        focused(vcx),
        (
            Role::ListItem,
            Some("Redstone lamp with a lever, open".into())
        )
    );
    // Shift+Tab: the stop before it, the search field.
    press(vcx, Role::Button, "Filter");
    pump(&shell, vcx);
    vcx.simulate_keystrokes("shift-tab");
    settle(&shell, vcx);
    assert!(!menu_open(vcx), "Shift+Tab closes the menu");
    assert_eq!(focused(vcx), (Role::TextInput, Some("Search chats".into())));
    // With a value chosen, the stop after the button is the first chip.
    press(vcx, Role::Button, "Filter");
    pump(&shell, vcx);
    press(vcx, Role::MenuItemCheckBox, "Very high (5\u{d7})");
    pump(&shell, vcx);
    vcx.simulate_keystrokes("tab");
    settle(&shell, vcx);
    assert!(!menu_open(vcx));
    assert_eq!(
        focused(vcx),
        (
            Role::Button,
            Some("Remove filter Size: Very high (5\u{d7})".into())
        )
    );
}

/// The archived section's rows, by name.
fn archived(vcx: &mut VisualTestContext) -> Vec<String> {
    let tree = tree(vcx);
    let nodes = nodes(&tree);
    if !has(&nodes, Role::List, "Archived") {
        return Vec::new();
    }
    named(&nodes, Role::List, "Archived")
        .children()
        .filter_map(|r| r.label())
        .collect()
}

fn archive_row(shell: &ShellUnderTest, vcx: &mut VisualTestContext, title: &str) {
    let tree = tree(vcx);
    let nodes = nodes(&tree);
    let button = row(&nodes, title)
        .children()
        .flat_map(|c| std::iter::once(c).chain(c.children()))
        .find(|n| n.role() == Role::Button && n.label().as_deref() == Some("Archive build"))
        .expect("the row's archive button");
    click(vcx, button);
    settle(shell, vcx);
}

fn press_in(vcx: &mut VisualTestContext, title: &str, button: &str) {
    let tree = tree(vcx);
    let nodes = nodes(&tree);
    let item = named(&nodes, Role::List, "Archived")
        .children()
        .find(|r| {
            r.label()
                .is_some_and(|l| l.starts_with(&format!("{title}, ")))
        })
        .expect("the archived row");
    let found = item
        .children()
        .flat_map(|c| std::iter::once(c).chain(c.children()))
        .find(|n| n.role() == Role::Button && n.label().as_deref() == Some(button))
        .expect("the row's button");
    click(vcx, found);
}

#[gpui::test]
fn after_a_delete_restoring_another_keeps_the_rest(cx: &mut TestAppContext) {
    // macOS pass: after a delete, a restore once read back an empty
    // archived list. Three archived chats; delete one, restore another: the
    // section keeps the third and the restored one is listed again.
    let (shell, _core, vcx) = open_standin(cx, "archive-delete-restore", gate_fixture(), None);
    archive_row(&shell, vcx, "Castle gate");
    archive_row(&shell, vcx, "Wheat farm");
    press(vcx, Role::Button, "Show archived");
    settle(&shell, vcx);
    let names = archived(vcx);
    assert_eq!(names.len(), 3, "{names:?}");
    press_in(vcx, "Old windmill", "Delete permanently");
    pump(&shell, vcx);
    press_in(vcx, "Old windmill", "Delete for good");
    settle(&shell, vcx);
    let names = archived(vcx);
    assert_eq!(names.len(), 2, "{names:?}");
    assert!(!names.iter().any(|n| n.starts_with("Old windmill")));
    press_in(vcx, "Castle gate", "Restore");
    settle(&shell, vcx);
    let names = archived(vcx);
    assert_eq!(names.len(), 1, "the rest stays: {names:?}");
    assert!(names[0].starts_with("Wheat farm, "), "{names:?}");
    assert!(
        labels(vcx, Role::ListItem)
            .iter()
            .any(|l| l.starts_with("Castle gate"))
    );
    // Archiving one more while the section is open lists it there too.
    archive_row(&shell, vcx, "Redstone lamp with a lever");
    let names = archived(vcx);
    assert_eq!(names.len(), 2, "{names:?}");
}

#[gpui::test]
fn deleting_the_only_archived_chat_closes_the_section_and_a_new_archive_reopens_it(
    cx: &mut TestAppContext,
) {
    // The macOS pass's sequence: the only archived chat deleted, then
    // another archived. The section closed with nothing left; opened again
    // it lists the new one.
    let (shell, _core, vcx) = open_standin(cx, "archive-only", gate_fixture(), None);
    press(vcx, Role::Button, "Show archived");
    settle(&shell, vcx);
    press_in(vcx, "Old windmill", "Delete permanently");
    pump(&shell, vcx);
    press_in(vcx, "Old windmill", "Delete for good");
    settle(&shell, vcx);
    assert!(
        !labels(vcx, Role::Button)
            .iter()
            .any(|l| l == "Hide archived" || l == "Show archived")
    );
    archive_row(&shell, vcx, "Wheat farm");
    press(vcx, Role::Button, "Show archived");
    settle(&shell, vcx);
    let names = archived(vcx);
    assert_eq!(names, ["Wheat farm, 7 days left"], "{names:?}");
}

#[gpui::test]
fn a_failed_search_says_why_where_its_results_would_be(cx: &mut TestAppContext) {
    // `Update::Found` carries a `Result`: the sidebar's search and the
    // palette show the core's sentence where their results would be, as a
    // selectable label, and the next search that works takes it away.
    use mcbench_ui::dev::standin::SEARCH_FAILURE;
    let (shell, core, vcx) = open_standin(cx, "search-fails", gate_fixture(), None);
    core.fail_searches(Some(SEARCH_FAILURE.to_string()));
    focus(vcx, Role::TextInput, "Search chats");
    vcx.simulate_input("gate");
    settle(&shell, vcx);
    let tree1 = tree(vcx);
    let n1 = nodes(&tree1);
    named(&n1, Role::Label, SEARCH_FAILURE);
    assert!(!has(&n1, Role::Label, "No matches"));
    let chats: Vec<String> = named(&n1, Role::List, "Chats")
        .children()
        .filter_map(|r| r.label())
        .collect();
    assert_eq!(chats, [SEARCH_FAILURE], "the sentence in place of the rows");
    // It is text a press focuses and Ctrl+A, Ctrl+C copy.
    let note = named(&n1, Role::Label, SEARCH_FAILURE);
    pointer_click(vcx, note);
    vcx.simulate_keystrokes(&mod_key("a"));
    vcx.simulate_keystrokes(&mod_key("c"));
    vcx.run_until_parked();
    assert_eq!(
        vcx.read_from_clipboard()
            .and_then(|item| item.text())
            .as_deref(),
        Some(SEARCH_FAILURE)
    );
    // Searching again once it works: the results, no sentence.
    core.fail_searches(None);
    focus(vcx, Role::TextInput, "Search chats");
    vcx.simulate_input("s");
    settle(&shell, vcx);
    let tree2 = tree(vcx);
    assert!(!has(&nodes(&tree2), Role::Label, SEARCH_FAILURE));
    vcx.simulate_keystrokes("escape");
    settle(&shell, vcx);

    // The palette.
    core.fail_searches(Some(SEARCH_FAILURE.to_string()));
    vcx.simulate_keystrokes(&mod_key("k"));
    pump(&shell, vcx);
    vcx.simulate_input("lever");
    settle(&shell, vcx);
    let tree3 = tree(vcx);
    let n3 = nodes(&tree3);
    let results = named(&n3, Role::ListBox, "Results");
    assert!(
        results.children().any(|c| c.role() == Role::Label
            && c.label().as_deref() == Some(SEARCH_FAILURE)
            || c.children()
                .any(|g| g.label().as_deref() == Some(SEARCH_FAILURE))),
        "the sentence is in the results"
    );
    let _ = shell;
}

#[gpui::test]
fn an_armed_delete_reached_by_tab_survives_re_renders_and_its_announcement(
    cx: &mut TestAppContext,
) {
    // The Windows pass saw the armed delete disarm at once when focus came
    // by Tab (W7). Tab to it, Enter: it stays armed through re-renders, the
    // window's refreshes and the polite announcement, until 4 s.
    let (shell, _core, vcx) = open_standin(cx, "delete-by-tab", gate_fixture(), None);
    press(vcx, Role::Button, "Show archived");
    settle(&shell, vcx);
    focus(vcx, Role::Button, "Restore");
    vcx.simulate_keystrokes("tab");
    pump(&shell, vcx);
    assert_eq!(
        focused(vcx),
        (Role::Button, Some("Delete permanently".into())),
        "Tab reaches Delete permanently"
    );
    vcx.simulate_keystrokes("enter");
    pump(&shell, vcx);
    let armed = |vcx: &mut VisualTestContext| {
        labels(vcx, Role::Button)
            .iter()
            .any(|l| l == "Delete for good")
    };
    assert!(armed(vcx), "armed");
    for _ in 0..39 {
        vcx.executor().advance_clock(Duration::from_millis(100));
        vcx.update(|window, _| window.refresh());
        pump(&shell, vcx);
        assert!(armed(vcx), "still armed before 4 s");
    }
    assert_eq!(
        focused(vcx),
        (Role::Button, Some("Delete for good".into())),
        "focus stays on it"
    );
    // A second press within the 4 s deletes.
    vcx.simulate_keystrokes("enter");
    settle(&shell, vcx);
    assert!(
        !labels(vcx, Role::ListItem)
            .iter()
            .any(|l| l.starts_with("Old windmill"))
    );
}
