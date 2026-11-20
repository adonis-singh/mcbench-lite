//! Every list and menu says its set's size (W4): each list-like node
//! carries `size_of_set` on itself (the Windows adapter reads an item's
//! from its container) and each of its items its position and the same
//! size, so a reader says "n of m". One assertion over every node of a
//! list-like role, on each screen that shows lists: the chat screen, the
//! filter menu, the archived section, Bench's list with its models, the
//! palette, the model picker (its rail and list) and the effort list.

mod common;

use accesskit_consumer::Node;
use common::*;
use gpui::accesskit::{Action, Role};
use gpui::{TestAppContext, VisualTestContext};
use std::time::Duration;

const LISTS: &[Role] = &[Role::List, Role::ListBox, Role::Menu, Role::TabList];
const ITEMS: &[Role] = &[
    Role::ListItem,
    Role::ListBoxOption,
    Role::MenuItem,
    Role::MenuItemCheckBox,
    Role::MenuItemRadio,
    Role::Tab,
];

// chat-fix-2: a group that states its own set's size (the filter menu's
// `Size`, `Prompt` and `Model`) is a set of its own.
/// Whether `node` holds a set: a list-like role, or a group with its own
/// set size.
fn is_set(node: Node<'_>) -> bool {
    LISTS.contains(&node.role()) || (node.role() == Role::Group && node.size_of_set().is_some())
}

/// The items that belong to `list`: its descendants of an item role with
/// no set between.
fn items<'a>(list: Node<'a>, out: &mut Vec<Node<'a>>) {
    for child in list.children() {
        if ITEMS.contains(&child.role()) {
            out.push(child);
        } else if !is_set(child) {
            items(child, out);
        }
    }
}

/// Every list-like node says its size; every item its position and that
/// size. Returns how many lists it checked.
fn every_set_has_its_size(vcx: &mut VisualTestContext, screen: &str) -> usize {
    let tree = tree(vcx);
    let nodes = nodes(&tree);
    let mut lists = 0;
    for list in nodes.iter().filter(|n| is_set(**n)) {
        // chat-fix-2: a list's own children are its items (or groups of
        // them, or a line saying it is empty), never a control beside an
        // item: VoiceOver counts a list's children ("1 of 4" for two models
        // and their stars).
        if matches!(list.role(), Role::List | Role::ListBox) {
            for child in list.children() {
                assert!(
                    ITEMS.contains(&child.role())
                        || matches!(child.role(), Role::Group | Role::Label),
                    "{screen}: {:?} {:?} holds a {:?} {:?} beside its items",
                    list.role(),
                    list.label(),
                    child.role(),
                    child.label()
                );
            }
        }
        let mut found = Vec::new();
        items(*list, &mut found);
        if found.is_empty() {
            continue;
        }
        lists += 1;
        let size = list.size_of_set();
        assert!(
            size.is_some_and(|s| s >= found.len()),
            "{screen}: {:?} {:?} has set size {size:?} for {} items",
            list.role(),
            list.label(),
            found.len()
        );
        // chat-fix-2: no two items share a position, and each is below the
        // size.
        let mut positions: Vec<usize> = found.iter().filter_map(|i| i.position_in_set()).collect();
        positions.sort_unstable();
        positions.dedup();
        assert!(
            positions.len() == found.len() && positions.iter().all(|p| Some(*p) < size),
            "{screen}: {:?} {:?}: positions {positions:?} of {} items, size {size:?}",
            list.role(),
            list.label(),
            found.len()
        );
        for item in &found {
            assert!(
                item.position_in_set().is_some() && item.size_of_set() == size,
                "{screen}: {:?} {:?} in {:?}: position {:?}, size {:?} (the list's {size:?})",
                item.role(),
                item.label(),
                list.label(),
                item.position_in_set(),
                item.size_of_set()
            );
        }
    }
    lists
}

fn settle(shell: &ShellUnderTest, vcx: &mut VisualTestContext) {
    pump(shell, vcx);
    vcx.executor().advance_clock(Duration::from_millis(300));
    pump(shell, vcx);
}

fn press(vcx: &mut VisualTestContext, role: Role, label: &str) {
    let tree = tree(vcx);
    click(vcx, named(&nodes(&tree), role, label));
}

fn mod_key(key: &str) -> String {
    if cfg!(target_os = "macos") {
        format!("cmd-{key}")
    } else {
        format!("ctrl-{key}")
    }
}

#[gpui::test]
fn every_list_and_menu_says_its_size_on_every_screen(cx: &mut TestAppContext) {
    let (shell, _core, vcx) = open_standin(cx, "sets", gate_fixture(), None);
    let mut checked = every_set_has_its_size(vcx, "chat screen");
    // The filter menu.
    press(vcx, Role::Button, "Filter");
    pump(&shell, vcx);
    checked += every_set_has_its_size(vcx, "filter menu");
    vcx.simulate_keystrokes("escape");
    settle(&shell, vcx);
    // The archived section.
    press(vcx, Role::Button, "Show archived");
    settle(&shell, vcx);
    checked += every_set_has_its_size(vcx, "archived section");
    // The model picker: its rail and its list; then a search.
    let tree1 = tree(vcx);
    let trigger = of_role(&nodes(&tree1), Role::Button)
        .into_iter()
        .find(|b| b.label().is_some_and(|l| l.starts_with("Chat model")))
        .expect("the model picker's trigger")
        .locate()
        .0;
    vcx.update(|window, cx| {
        window.dispatch_a11y_action_for_test(
            gpui::accesskit::ActionRequest {
                action: Action::Click,
                target_tree: gpui::accesskit::TreeId::ROOT,
                target_node: trigger,
                data: None,
            },
            cx,
        )
    });
    settle(&shell, vcx);
    checked += every_set_has_its_size(vcx, "model picker");
    vcx.simulate_input("gemini");
    settle(&shell, vcx);
    checked += every_set_has_its_size(vcx, "model search");
    vcx.simulate_keystrokes("escape");
    settle(&shell, vcx);
    // The effort list.
    let tree2 = tree(vcx);
    let effort = of_role(&nodes(&tree2), Role::Button)
        .into_iter()
        .find(|b| b.label().is_some_and(|l| l.starts_with("Reasoning effort")))
        .expect("the effort trigger")
        .locate()
        .0;
    vcx.update(|window, cx| {
        window.dispatch_a11y_action_for_test(
            gpui::accesskit::ActionRequest {
                action: Action::Click,
                target_tree: gpui::accesskit::TreeId::ROOT,
                target_node: effort,
                data: None,
            },
            cx,
        )
    });
    settle(&shell, vcx);
    checked += every_set_has_its_size(vcx, "effort list");
    vcx.simulate_keystrokes("escape");
    settle(&shell, vcx);
    // The palette.
    vcx.simulate_keystrokes(&mod_key("k"));
    settle(&shell, vcx);
    checked += every_set_has_its_size(vcx, "palette");
    vcx.simulate_keystrokes("escape");
    settle(&shell, vcx);
    // Bench: its list with a bench's models shown.
    vcx.simulate_keystrokes(&mod_key("2"));
    settle(&shell, vcx);
    press(vcx, Role::Button, "Show models");
    settle(&shell, vcx);
    checked += every_set_has_its_size(vcx, "bench list");
    assert!(checked >= 10, "lists checked: {checked}");
}
