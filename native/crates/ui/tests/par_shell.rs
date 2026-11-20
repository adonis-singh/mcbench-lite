//! The parity pass's shell findings (par-shell) that have no test file of
//! their own: a shell started with the viewer's selections in its draft.

mod common;

use common::*;
use gpui::TestAppContext;
use mcbench_ui::state::place::Place;

fn lamp_selection() -> mcbench_ui::Selection {
    mcbench_ui::Selection {
        letter: "A".into(),
        label: "Redstone Lamp, unlit".into(),
        min: [1, 1, 1],
        max: [1, 2, 1],
        blocks: 2,
        block: Some("minecraft:redstone_lamp".into()),
    }
}

/// Starts the shell at `start` with a selection in the first draft (the new
/// chat's): focus is on the composer; `chip` says the chip shows there (at
/// a chat the chat's own draft shows, the selection staying in the new
/// chat's).
fn starts_with_a_selection(cx: &mut TestAppContext, test: &str, start: Place, chip: bool) {
    let preferences = mcbench_ui::Preferences {
        last_model: Some(("anthropic".into(), mcbench_ui::dev::standin::MODEL.into())),
        ..mcbench_ui::Preferences::default()
    };
    let (shell, _core, vcx) = open_standin_selecting(
        cx,
        test,
        gate_fixture(),
        Some(start),
        preferences,
        vec![lamp_selection()],
    );
    for _ in 0..4 {
        pump(&shell, vcx);
    }
    let (role, name) = focused(vcx);
    assert_eq!(
        (role, name.as_deref()),
        (gpui::accesskit::Role::MultilineTextInput, Some("Message")),
        "the composer has focus, the chip beside it"
    );
    let tree = tree(vcx);
    assert_eq!(
        nodes(&tree).iter().any(|n| n
            .label()
            .is_some_and(|l| l.contains("Redstone Lamp, unlit"))),
        chip,
        "the chip on screen"
    );
}

#[gpui::test]
fn a_shell_started_with_a_selection_at_the_new_chat_keeps_focus_on_something(
    cx: &mut TestAppContext,
) {
    starts_with_a_selection(cx, "select-new", Place::NewChat { id: None }, true);
}

#[gpui::test]
fn a_shell_started_with_a_selection_at_a_chat_keeps_focus_on_something(cx: &mut TestAppContext) {
    starts_with_a_selection(
        cx,
        "select-chat",
        Place::Chat {
            id: "chat-gate".into(),
        },
        false,
    );
}
