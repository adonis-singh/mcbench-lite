//! The chat column's parity with today's page (stage four's parity pass,
//! the audit's chat findings): a model picked in an open chat goes with
//! the send and becomes the chat's (`web/app.js` `send`, `server/index.js`
//! `handleChat`), and the route's refusals of the pair come back as the
//! page's notices. Each through the window as a user or a screen reader
//! drives it, read back from the accessibility tree.

mod common;

use std::sync::{Arc, Mutex};

use common::*;
use gpui::accesskit::Action;
use gpui::accesskit::Role;
use gpui::{TestAppContext, VisualTestContext};
use mcbench_ui::Target;
use mcbench_ui::dev::standin;
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
    let pane = pane(shell, vcx);
    vcx.update(|window, cx| pane.update(cx, |p, cx| p.focus_composer(window, cx)));
    vcx.simulate_input(text);
    pump(shell, vcx);
    vcx.simulate_keystrokes("enter");
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

/// The picker's pair.
fn picked(shell: &ShellUnderTest, vcx: &mut VisualTestContext) -> (String, String) {
    let pickers = vcx.update(|_, cx| shell.entity.read(cx).chat().read(cx).pickers());
    vcx.update(|_, cx| {
        let choice = pickers.read(cx).choice();
        (choice.provider.clone(), choice.model.clone())
    })
}

/// Picks `model` of `provider` in the composer's picker.
fn pick(shell: &ShellUnderTest, vcx: &mut VisualTestContext, provider: &str, model: &str) {
    let pickers = vcx.update(|_, cx| shell.entity.read(cx).chat().read(cx).pickers());
    vcx.update(|_, cx| pickers.update(cx, |p, cx| p.pick_model(provider.into(), model.into(), cx)));
    pump(shell, vcx);
}

/// The chat target of every send recorded, as (id, provider, model).
fn chat_sends(sent: &Arc<Mutex<Vec<Target>>>) -> Vec<(String, String, String)> {
    sent.lock()
        .expect("lock")
        .iter()
        .filter_map(|t| match t {
            Target::Chat {
                id,
                provider,
                model,
            } => Some((id.clone(), provider.clone(), model.clone())),
            _ => None,
        })
        .collect()
}

const OTHER: (&str, &str) = ("claude-code", "claude-haiku-5-5");

#[gpui::test]
fn a_model_picked_in_an_open_chat_goes_with_the_send_and_becomes_the_chats(
    cx: &mut TestAppContext,
) {
    let sent = Arc::new(Mutex::new(None));
    let (shell, vcx) = open_shell_with(cx, "par-chat-pair", lamp(), Vec::new(), |core, _| {
        *sent.lock().expect("lock") = Some(core.record_sends())
    });
    let sent = sent.lock().expect("lock").take().expect("recorded");
    assert_eq!(
        picked(&shell, vcx),
        ("anthropic".into(), standin::MODEL.into())
    );
    pick(&shell, vcx, OTHER.0, OTHER.1);
    // The trigger reads the new model at once, before any send (the page's
    // `provider`, `model` state is the picker's).
    assert!(
        names(vcx, Role::Button)
            .iter()
            .any(|n| *n == format!("Chat model: {}", OTHER.1))
    );
    send(&shell, vcx, "One more lamp");
    assert_eq!(
        chat_sends(&sent),
        [("chat-lamp".into(), OTHER.0.into(), OTHER.1.into())]
    );
    assert!(notices(vcx).is_empty(), "{:?}", notices(vcx));
    // The chat's own model is now the one sent: away and back, the picker
    // opens on it.
    press(&shell, vcx, Role::ListItem, "Castle gate");
    assert_eq!(
        picked(&shell, vcx),
        ("anthropic".into(), standin::MODEL.into())
    );
    press(&shell, vcx, Role::ListItem, "Redstone lamp with a lever");
    assert_eq!(picked(&shell, vcx), (OTHER.0.into(), OTHER.1.into()));
}

#[gpui::test]
fn an_edit_carries_the_model_picked(cx: &mut TestAppContext) {
    let sent = Arc::new(Mutex::new(None));
    let (shell, vcx) = open_shell_with(cx, "par-chat-edit-pair", lamp(), Vec::new(), |core, _| {
        *sent.lock().expect("lock") = Some(core.record_sends())
    });
    let sent = sent.lock().expect("lock").take().expect("recorded");
    press(&shell, vcx, Role::Button, "Edit and branch");
    pick(&shell, vcx, OTHER.0, OTHER.1);
    let pane = pane(&shell, vcx);
    vcx.update(|window, cx| pane.update(cx, |p, cx| p.send(window, cx)));
    pump(&shell, vcx);
    assert_eq!(
        chat_sends(&sent),
        [("chat-lamp".into(), OTHER.0.into(), OTHER.1.into())]
    );
    assert!(
        articles(vcx)
            .iter()
            .any(|r| r == "You, Thanks. Is the base still three by three?")
    );
}

#[gpui::test]
fn an_unknown_provider_is_refused_with_the_routes_text_and_nothing_changes(
    cx: &mut TestAppContext,
) {
    let (shell, vcx) =
        open_shell_with(cx, "par-chat-no-provider", lamp(), Vec::new(), |core, _| {
            core.forget_provider(OTHER.0)
        });
    let before = articles(vcx);
    pick(&shell, vcx, OTHER.0, OTHER.1);
    send(&shell, vcx, "One more lamp");
    assert_eq!(
        notices(vcx),
        [format!("Error, {}", standin::UNKNOWN_PROVIDER)]
    );
    assert_eq!(articles(vcx), before);
    assert_eq!(composer_text(&shell, vcx), "One more lamp");
}

#[gpui::test]
fn an_empty_model_is_refused_with_the_routes_text(cx: &mut TestAppContext) {
    let (shell, vcx) = open_shell_with(cx, "par-chat-no-model", lamp(), Vec::new(), |core, _| {
        core.blank_models()
    });
    let before = articles(vcx);
    send(&shell, vcx, "One more lamp");
    assert_eq!(notices(vcx), [format!("Error, {}", standin::NO_MODEL)]);
    assert_eq!(articles(vcx), before);
    assert_eq!(composer_text(&shell, vcx), "One more lamp");
}

// Findings 2, 5, 6: New chat with no model, Escape in the sidebar search,
// the chat on screen chosen again.

fn no_model() -> mcbench_ui::Preferences {
    mcbench_ui::Preferences::default()
}

fn statuses(vcx: &mut VisualTestContext) -> Vec<String> {
    names(vcx, Role::Status)
}

#[gpui::test]
fn new_chat_with_no_model_says_choose_a_model_before_starting_a_chat(cx: &mut TestAppContext) {
    let (shell, vcx) = open_shell_preferring(
        cx,
        "par-chat-new-no-model",
        Some(Place::NewChat { id: None }),
        Vec::new(),
        no_model(),
        |_, _| {},
    );
    let place = vcx.update(|_, cx| shell.entity.read(cx).place().clone());
    press(&shell, vcx, Role::Button, "New chat");
    assert_eq!(
        notices(vcx),
        [format!(
            "Error, {}",
            mcbench_ui::view::shell::NO_MODEL_FOR_NEW_CHAT
        )]
    );
    assert_eq!(
        vcx.update(|_, cx| shell.entity.read(cx).place().clone()),
        place
    );
    // With a model, New chat opens the new chat and no notice comes.
    pick(&shell, vcx, OTHER.0, OTHER.1);
    press(&shell, vcx, Role::Button, "Dismiss error");
    press(&shell, vcx, Role::Button, "New chat");
    assert!(notices(vcx).is_empty(), "{:?}", notices(vcx));
}

#[gpui::test]
fn escape_in_the_sidebar_search_leaves_an_edit_in_the_composer(cx: &mut TestAppContext) {
    let (shell, vcx) = open_shell_with(cx, "par-chat-find-escape", lamp(), Vec::new(), |_, _| {});
    let editing = "Editing a message: sending it starts a new branch";
    press(&shell, vcx, Role::Button, "Edit and branch");
    assert!(statuses(vcx).iter().any(|n| n == editing));
    let tree = tree(vcx);
    let field = named(&nodes(&tree), Role::TextInput, "Search chats");
    act(vcx, field, Action::Focus);
    vcx.simulate_input("gate");
    pump(&shell, vcx);
    vcx.simulate_keystrokes("escape");
    pump(&shell, vcx);
    assert!(
        statuses(vcx).iter().any(|n| n == editing),
        "the edit stays: {:?}",
        statuses(vcx)
    );
    assert_eq!(
        composer_text(&shell, vcx),
        "Thanks. Is the base still three by three?"
    );
    // Escape outside a field still cancels it.
    let pane = pane(&shell, vcx);
    vcx.update(|window, cx| pane.update(cx, |p, cx| p.focus_composer(window, cx)));
    vcx.simulate_keystrokes("escape");
    pump(&shell, vcx);
    assert!(!statuses(vcx).iter().any(|n| n == editing));
}

#[gpui::test]
fn choosing_the_chat_on_screen_after_a_failed_read_reads_it_again(cx: &mut TestAppContext) {
    let gate = Some(Place::Chat {
        id: "chat-gate".into(),
    });
    let (shell, core, vcx) = open_standin(cx, "par-chat-retry-row", gate_fixture(), gate);
    core.fail_reads_of("chat-lamp");
    press(&shell, vcx, Role::ListItem, "Redstone lamp with a lever");
    let failed = format!("This chat could not be read: {}", standin::UNREADABLE);
    assert!(names(vcx, Role::Alert).contains(&failed));
    core.heal_reads_of("chat-lamp");
    press(&shell, vcx, Role::ListItem, "Redstone lamp with a lever");
    assert!(
        articles(vcx)
            .iter()
            .any(|r| r == "You, Thanks. Is the base still three by three?"),
        "{:?}",
        articles(vcx)
    );
}

// Finding 3: the header's model warnings.

#[gpui::test]
fn the_header_warns_of_an_unknown_model_and_of_one_without_tools(cx: &mut TestAppContext) {
    use mcbench_ui::ModelFacts;
    use mcbench_ui::view::shell::header::{NO_TOOLS, UNKNOWN_MODEL};
    let (shell, vcx) = open_shell_with(cx, "par-chat-cap-warn", lamp(), Vec::new(), |core, _| {
        core.describe_model("anthropic", standin::MODEL, ModelFacts::unknown());
        core.describe_model(
            "claude-code",
            "claude-no-tools",
            ModelFacts {
                known: true,
                tools: None,
                ..ModelFacts::unknown()
            },
        );
    });
    let labels = |vcx: &mut VisualTestContext| names(vcx, Role::Label);
    assert!(
        labels(vcx).iter().any(|n| n == UNKNOWN_MODEL),
        "{:?}",
        labels(vcx)
    );
    // The warning follows the picker, before any send.
    pick(&shell, vcx, OTHER.0, OTHER.1);
    assert!(
        !labels(vcx)
            .iter()
            .any(|n| n == UNKNOWN_MODEL || n == NO_TOOLS)
    );
    pick(&shell, vcx, "claude-code", "claude-no-tools");
    assert!(
        labels(vcx).iter().any(|n| n == NO_TOOLS),
        "{:?}",
        labels(vcx)
    );
}

// Seam item 1: a new chat's row is made at New chat (`createdChat`).

fn mod_key(key: &str) -> String {
    if cfg!(target_os = "macos") {
        format!("cmd-{key}")
    } else {
        format!("ctrl-{key}")
    }
}

fn place_of(shell: &ShellUnderTest, vcx: &mut VisualTestContext) -> Place {
    vcx.update(|_, cx| shell.entity.read(cx).place().clone())
}

/// The new chat's kept row, from the place on screen.
fn new_row(shell: &ShellUnderTest, vcx: &mut VisualTestContext) -> String {
    match place_of(shell, vcx) {
        Place::NewChat { id: Some(id) } => id,
        other => panic!("the new chat with its row, not {other:?}"),
    }
}

fn world_view(shell: &ShellUnderTest, vcx: &mut VisualTestContext) -> Option<String> {
    vcx.update(|_, cx| shell.entity.read(cx).live_view(cx).view.map(|v| v.0))
}

fn castle() -> Option<Place> {
    Some(Place::Chat {
        id: "chat-gate".into(),
    })
}

#[gpui::test]
fn new_chat_makes_its_row_at_once_and_shows_its_empty_world(cx: &mut TestAppContext) {
    let (shell, core, vcx) = open_standin(cx, "par-chat-row", gate_fixture(), castle());
    press(&shell, vcx, Role::Button, "New chat");
    assert_eq!(core.creates(), 1);
    let id = new_row(&shell, vcx);
    assert_eq!(world_view(&shell, vcx), Some(id.clone()));
    // The core's answer for a chat with no message: shown, no block,
    // nothing running.
    let state = vcx.update(|_, cx| {
        let world = shell.entity.read(cx).world();
        world.read(cx).state().cloned()
    });
    let state = state.expect("the live world's state");
    assert_eq!(state.view.map(|v| v.0), Some(id.clone()));
    assert_eq!(state.phase, mcbench_ui::LivePhase::Shown);
    assert_eq!((state.blocks, state.running), (0, None));
    // The row is not listed: it has no message.
    let listed = vcx.update(|_, cx| {
        shell
            .entity
            .read(cx)
            .lists_entity()
            .read(cx)
            .chat(&id)
            .is_some()
    });
    assert!(!listed);
    // Away and back by New chat: the same row, none made.
    press(&shell, vcx, Role::ListItem, "Castle gate");
    press(&shell, vcx, Role::Button, "New chat");
    assert_eq!(core.creates(), 1);
    assert_eq!(new_row(&shell, vcx), id);
    // Shown by any other way (`showNewChat`), it has its row too.
    press(&shell, vcx, Role::ListItem, "Castle gate");
    vcx.update(|window, cx| {
        shell
            .entity
            .update(cx, |s, cx| s.go(Place::NewChat { id: None }, window, cx))
    });
    pump(&shell, vcx);
    assert_eq!(new_row(&shell, vcx), id);
    assert_eq!(core.creates(), 1);
}

#[gpui::test]
fn the_shortcut_the_header_the_palette_and_the_missing_screen_make_the_row(
    cx: &mut TestAppContext,
) {
    let (shell, core, vcx) = open_standin(cx, "par-chat-row-callers", gate_fixture(), castle());
    // The `new` shortcut.
    vcx.simulate_keystrokes(&mod_key("shift-o"));
    pump(&shell, vcx);
    assert_eq!(core.creates(), 1);
    let id = new_row(&shell, vcx);
    // On the untouched new chat, the header's New chat (the sidebar
    // closed) only focuses the composer: the same row.
    vcx.simulate_keystrokes(&mod_key("b"));
    pump(&shell, vcx);
    press(&shell, vcx, Role::Button, "New chat");
    assert_eq!((core.creates(), new_row(&shell, vcx)), (1, id.clone()));
    vcx.simulate_keystrokes(&mod_key("b"));
    pump(&shell, vcx);
    // The palette's New chat.
    press(&shell, vcx, Role::ListItem, "Castle gate");
    vcx.simulate_keystrokes(&mod_key("k"));
    pump(&shell, vcx);
    vcx.simulate_input("New chat");
    pump(&shell, vcx);
    vcx.simulate_keystrokes("enter");
    pump(&shell, vcx);
    assert_eq!((core.creates(), new_row(&shell, vcx)), (1, id));
}

#[gpui::test]
fn the_missing_screens_new_chat_makes_the_row(cx: &mut TestAppContext) {
    let nowhere = Some(Place::Chat {
        id: "nowhere".into(),
    });
    let (shell, core, vcx) = open_standin(cx, "par-chat-row-missing", gate_fixture(), nowhere);
    assert!(matches!(place_of(&shell, vcx), Place::Missing { .. }));
    press(&shell, vcx, Role::Button, "New chat");
    assert_eq!(core.creates(), 1);
    new_row(&shell, vcx);
}

#[gpui::test]
fn the_first_message_goes_to_the_row_and_settles_it(cx: &mut TestAppContext) {
    let (shell, core, vcx) = open_standin(cx, "par-chat-row-settle", gate_fixture(), castle());
    let sent = core.record_sends();
    press(&shell, vcx, Role::Button, "New chat");
    let id = new_row(&shell, vcx);
    send(&shell, vcx, "Build a tower");
    assert_eq!(
        chat_sends(&sent),
        [(id.clone(), "anthropic".into(), standin::MODEL.into())]
    );
    assert_eq!(place_of(&shell, vcx), Place::Chat { id: id.clone() });
    // The next new chat has a row of its own.
    press(&shell, vcx, Role::Button, "New chat");
    assert_eq!(core.creates(), 2);
    assert_ne!(new_row(&shell, vcx), id);
}

#[gpui::test]
fn a_failed_row_is_a_notice_and_the_next_new_chat_tries_again(cx: &mut TestAppContext) {
    let (shell, core, vcx) = open_standin(cx, "par-chat-row-fails", gate_fixture(), castle());
    core.fail_creates(Some("The disk is full"));
    press(&shell, vcx, Role::Button, "New chat");
    assert_eq!(notices(vcx), ["Error, The disk is full"]);
    assert_eq!(place_of(&shell, vcx), Place::NewChat { id: None });
    assert_eq!(world_view(&shell, vcx), None);
    core.fail_creates(None);
    press(&shell, vcx, Role::ListItem, "Castle gate");
    press(&shell, vcx, Role::Button, "New chat");
    assert_eq!(core.creates(), 2);
    new_row(&shell, vcx);
}

#[gpui::test]
fn a_send_while_the_row_is_being_made_goes_to_it_once_made(cx: &mut TestAppContext) {
    let (shell, core, vcx) = open_standin(cx, "par-chat-row-race", gate_fixture(), castle());
    let sent = core.record_sends();
    core.hold_creates();
    press(&shell, vcx, Role::Button, "New chat");
    send(&shell, vcx, "Build a tower");
    assert!(
        sent.lock().expect("lock").is_empty(),
        "the send waits for the row"
    );
    assert!(articles(vcx).iter().any(|r| r == "You, Build a tower"));
    core.release_creates();
    pump(&shell, vcx);
    let sends = chat_sends(&sent);
    assert_eq!(sends.len(), 1, "{sends:?}");
    assert_eq!(
        place_of(&shell, vcx),
        Place::Chat {
            id: sends[0].0.clone()
        }
    );
}

#[gpui::test]
fn a_send_waiting_for_a_row_that_fails_comes_back(cx: &mut TestAppContext) {
    let (shell, core, vcx) = open_standin(cx, "par-chat-row-race-fails", gate_fixture(), castle());
    let sent = core.record_sends();
    core.hold_creates();
    core.fail_creates(Some("The disk is full"));
    press(&shell, vcx, Role::Button, "New chat");
    send(&shell, vcx, "Build a tower");
    core.release_creates();
    pump(&shell, vcx);
    assert!(sent.lock().expect("lock").is_empty());
    assert_eq!(notices(vcx), ["Error, The disk is full"]);
    assert_eq!(composer_text(&shell, vcx), "Build a tower");
    assert!(!articles(vcx).iter().any(|r| r == "You, Build a tower"));
}

#[gpui::test]
fn a_send_from_a_new_chat_with_no_row_makes_its_own(cx: &mut TestAppContext) {
    let (shell, core, vcx) = open_standin(
        cx,
        "par-chat-row-none",
        gate_fixture(),
        Some(Place::NewChat { id: None }),
    );
    let sent = core.record_sends();
    assert_eq!(core.creates(), 0, "the start is not New chat");
    send(&shell, vcx, "Build a tower");
    assert!(matches!(
        &sent.lock().expect("lock")[..],
        [Target::NewChat { .. }]
    ));
}

// Seam item 2: a failed providers read.

const UNREADABLE_CONFIG: &str = "settings.json could not be read";

#[gpui::test]
fn a_failed_providers_read_says_models_failed_to_load_and_try_again_reads_them(
    cx: &mut TestAppContext,
) {
    let (shell, vcx) = open_shell_with(cx, "par-chat-providers", lamp(), Vec::new(), |core, _| {
        core.fail_providers_read(UNREADABLE_CONFIG)
    });
    assert!(
        names(vcx, Role::Alert)
            .iter()
            .any(|n| n == "Models failed to load.")
    );
    // The page's `showError(error)`: the read's text is a notice too.
    assert_eq!(notices(vcx), [format!("Error, {UNREADABLE_CONFIG}")]);
    assert!(
        log_lines(&shell.log)
            .iter()
            .any(|l| l["action"] == "providers failed" && l["error"] == UNREADABLE_CONFIG)
    );
    // In Bench too.
    vcx.simulate_keystrokes(&mod_key("2"));
    pump(&shell, vcx);
    assert!(
        names(vcx, Role::Alert)
            .iter()
            .any(|n| n == "Models failed to load.")
    );
    vcx.simulate_keystrokes(&mod_key("1"));
    pump(&shell, vcx);
    press(&shell, vcx, Role::Button, "Try again");
    assert!(
        !names(vcx, Role::Alert)
            .iter()
            .any(|n| n == "Models failed to load.")
    );
    assert!(
        log_lines(&shell.log)
            .iter()
            .any(|l| l["action"] == "read providers")
    );
    // The providers arrived: the catalogue has them.
    let catalog = vcx.update(|_, cx| mcbench_ui::view::pickers::catalog::Catalog::installed(cx));
    assert!(vcx.update(|_, cx| catalog.read(cx).providers_arrived()));
}

// The draft's selection chip (`SelectionChip`, `web/app.js` 2531-2555).

/// The shell on the lamp chat, its draft holding one selection as the
/// viewer names it (`addSelection`).
fn open_selecting<'a>(
    cx: &'a mut TestAppContext,
    test: &str,
) -> (ShellUnderTest, &'a mut VisualTestContext) {
    let (shell, vcx) = open_shell_with(cx, test, lamp(), Vec::new(), |_, _| {});
    let selection = mcbench_ui::Selection {
        letter: "A".into(),
        label: "Redstone Lamp, unlit".into(),
        min: [1, 1, 1],
        max: [1, 2, 1],
        blocks: 2,
        block: Some("minecraft:redstone_lamp".into()),
    };
    vcx.update(|_, cx| {
        shell
            .entity
            .update(cx, |s, cx| s.add_selection(selection, cx))
    });
    pump(&shell, vcx);
    (shell, vcx)
}

const CHIP: &str = "Redstone Lamp, unlit, 1\u{d7}2\u{d7}1";

fn chip_bounds(vcx: &mut VisualTestContext) -> Option<gpui::accesskit::Rect> {
    let tree = tree(vcx);
    nodes(&tree)
        .iter()
        .find(|n| n.role() == Role::Group && n.label().as_deref() == Some(CHIP))
        .map(|n| n.bounding_box().expect("bounds"))
}

#[gpui::test]
fn a_right_click_removes_the_drafts_selection_chip(cx: &mut TestAppContext) {
    let (shell, vcx) = open_selecting(cx, "par-chat-chip-right");
    let bounds = chip_bounds(vcx).expect("the draft's chip");
    let centre = centre_of(vcx, bounds);
    vcx.simulate_mouse_down(centre, gpui::MouseButton::Right, gpui::Modifiers::none());
    vcx.simulate_mouse_up(centre, gpui::MouseButton::Right, gpui::Modifiers::none());
    pump(&shell, vcx);
    assert!(chip_bounds(vcx).is_none(), "the chip is gone");
}

#[gpui::test]
fn a_selection_chips_title_is_its_hover_tip_and_its_description(cx: &mut TestAppContext) {
    let (_shell, vcx) = open_selecting(cx, "par-chat-chip-tip");
    let tree1 = tree(vcx);
    let chip = named(&nodes(&tree1), Role::Group, CHIP);
    assert_eq!(chip.description().as_deref(), Some("2 blocks"));
    let centre = centre_of(vcx, chip.bounding_box().expect("bounds"));
    drop(tree1);
    vcx.simulate_mouse_move(centre, None, gpui::Modifiers::none());
    let _ = tree(vcx);
    vcx.executor()
        .advance_clock(std::time::Duration::from_millis(501));
    vcx.run_until_parked();
    let tree2 = tree(vcx);
    assert!(has(&nodes(&tree2), Role::Tooltip, "2 blocks"));
}

// Finding 15: the composer's and transcript's untested rows.

fn no_model_prefs() -> mcbench_ui::Preferences {
    mcbench_ui::Preferences::default()
}

#[gpui::test]
fn a_send_with_no_model_says_choose_a_model_or_connect_an_ai_first(cx: &mut TestAppContext) {
    let (shell, vcx) = open_shell_preferring(
        cx,
        "par-chat-no-model-send",
        Some(Place::NewChat { id: None }),
        Vec::new(),
        no_model_prefs(),
        |_, _| {},
    );
    send(&shell, vcx, "A tower");
    assert_eq!(notices(vcx), ["Error, Choose a model before sending"]);
    press(&shell, vcx, Role::Button, "Dismiss error");
    // No provider connected: the other text.
    let none = mcbench_ui::Provider {
        id: "anthropic".into(),
        label: "Anthropic".into(),
        connected: false,
        auth: mcbench_ui::Auth::ApiKey,
        harness: false,
        enabled: true,
        hidden_models: Vec::new(),
        model_order: Vec::new(),
    };
    vcx.update(|window, cx| {
        shell.entity.update(cx, |s, cx| {
            s.apply(mcbench_ui::Update::Providers(Ok(vec![none])), window, cx)
        })
    });
    pump(&shell, vcx);
    vcx.simulate_keystrokes("enter");
    pump(&shell, vcx);
    assert_eq!(notices(vcx), ["Error, Connect an AI in Settings first"]);
    assert_eq!(composer_text(&shell, vcx), "A tower");
}

#[gpui::test]
fn copy_message_checks_for_1200_ms_and_a_paste_brings_the_message_back(cx: &mut TestAppContext) {
    let (shell, vcx) = open_shell_with(cx, "par-chat-copy", lamp(), Vec::new(), |_, _| {});
    let pane = pane(&shell, vcx);
    press(&shell, vcx, Role::Button, "Copy message");
    let copied = vcx.update(|_, cx| cx.read_from_clipboard().and_then(|c| c.text()));
    assert_eq!(
        copied.as_deref(),
        Some("Thanks. Is the base still three by three?")
    );
    assert!(vcx.update(|_, cx| pane.read(cx).shows_copied()));
    vcx.executor()
        .advance_clock(std::time::Duration::from_millis(1199));
    vcx.run_until_parked();
    assert!(
        vcx.update(|_, cx| pane.read(cx).shows_copied()),
        "still at 1,199 ms"
    );
    vcx.executor()
        .advance_clock(std::time::Duration::from_millis(2));
    vcx.run_until_parked();
    assert!(
        !vcx.update(|_, cx| pane.read(cx).shows_copied()),
        "gone at 1,200 ms"
    );
    // The window's paste path: the copied message comes back as itself.
    vcx.update(|window, cx| pane.update(cx, |p, cx| p.focus_composer(window, cx)));
    vcx.simulate_keystrokes(&mod_key("v"));
    pump(&shell, vcx);
    assert_eq!(
        composer_text(&shell, vcx),
        "Thanks. Is the base still three by three?"
    );
    assert!(
        log_lines(&shell.log)
            .iter()
            .any(|l| l["action"] == "pasted message")
    );
}

fn picks(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let a = dir.join("notes.txt");
    std::fs::write(&a, "the base is stone").expect("written");
    vec![a]
}

#[gpui::test]
fn a_picked_file_shows_its_tile_and_remove_attachment_takes_it_away(cx: &mut TestAppContext) {
    let dir = TempDir::new("par-chat-picks");
    let (shell, vcx) = open_shell_with(cx, "par-chat-remove", lamp(), picks(&dir.0), |_, _| {});
    press(&shell, vcx, Role::Button, "Attach file");
    assert!(
        names(vcx, Role::Group)
            .iter()
            .any(|n| n == "notes.txt, plain")
    );
    press(&shell, vcx, Role::Button, "Remove attachment");
    assert!(
        !names(vcx, Role::Group)
            .iter()
            .any(|n| n.starts_with("notes.txt"))
    );
    drop(dir);
}

#[gpui::test]
fn the_transcript_follows_new_content_only_within_48_px_of_its_end(cx: &mut TestAppContext) {
    let long = Some(Place::Chat {
        id: "chat-long".into(),
    });
    let (shell, core, vcx) = open_standin(
        cx,
        "par-chat-follow",
        mcbench_ui::dev::fixtures::long(),
        long,
    );
    core.turns_by_hand();
    let pane = pane(&shell, vcx);
    let at_end = |vcx: &mut VisualTestContext| {
        let (offset, max) = vcx.update(|_, cx| pane.read(cx).transcript_scroll());
        max > 0. && offset >= max - 1.
    };
    send(&shell, vcx, "One more line");
    assert!(at_end(vcx), "a send goes to the end");
    core.advance("chat-long", 2);
    pump(&shell, vcx);
    assert!(at_end(vcx), "within 48 px it follows");
    // Scrolled up 600 px, new content leaves the view where it is.
    let tree1 = tree(vcx);
    let region = named(&nodes(&tree1), Role::Region, "Conversation");
    let centre = centre_of(vcx, region.bounding_box().expect("bounds"));
    drop(tree1);
    vcx.simulate_event(gpui::ScrollEvent {
        position: centre,
        delta: gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.), gpui::px(600.))),
        ..Default::default()
    });
    pump(&shell, vcx);
    let (before, _) = vcx.update(|_, cx| pane.read(cx).transcript_scroll());
    assert!(!at_end(vcx));
    core.advance("chat-long", 2);
    pump(&shell, vcx);
    let (after, _) = vcx.update(|_, cx| pane.read(cx).transcript_scroll());
    assert_eq!(before, after, "the view stays");
    core.finish("chat-long");
    pump(&shell, vcx);
}

#[gpui::test]
fn a_comparisons_composer_follows_up_with_every_model(cx: &mut TestAppContext) {
    let castle = Some(Place::Compare {
        bench: "bench-castle".into(),
    });
    let (_shell, _core, vcx) = open_bench(cx, "par-chat-follow-up", castle, &[]);
    let tree1 = tree(vcx);
    let placeholder = nodes(&tree1)
        .iter()
        .find(|n| n.label().as_deref() == Some("Message"))
        .and_then(|n| n.placeholder().map(str::to_string));
    drop(tree1);
    assert_eq!(placeholder.as_deref(), Some("Follow up with every model"));
    assert!(
        names(vcx, Role::Button)
            .iter()
            .any(|n| n == "Send to every model")
    );
}

#[gpui::test]
fn a_live_stack_folds_its_earlier_calls_under_previous_tool_calls(cx: &mut TestAppContext) {
    let (shell, core, vcx) = open_standin(cx, "par-chat-previous", gate_fixture(), lamp());
    let dir = TempDir::new("par-chat-previous-files");
    let picture = dir.0.join("lamp.png");
    std::fs::write(&picture, mcbench_ui::dev::fixtures::PICTURE).expect("the picture");
    core.keep_files(&dir.0.join("files"), &picture);
    core.set_script(mcbench_ui::dev::turns::Script::Full);
    core.turns_by_hand();
    send(&shell, vcx, "Build it");
    let fold = |vcx: &mut VisualTestContext| {
        names(vcx, Role::Button)
            .into_iter()
            .find(|n| n.starts_with('+') && n.contains("previous tool call"))
    };
    let mut found = None;
    for _ in 0..200 {
        if !core.advance("chat-lamp", 1) {
            break;
        }
        pump(&shell, vcx);
        found = fold(vcx);
        if found.is_some() {
            break;
        }
    }
    let label = found.expect("a live stack with an earlier call");
    assert!(
        label == "+1 previous tool call" || label.ends_with(" previous tool calls"),
        "{label}"
    );
    let tree1 = tree(vcx);
    let node = named(&nodes(&tree1), Role::Button, &label);
    assert_eq!(
        node.description().as_deref(),
        Some("Show previous tool calls")
    );
    assert_eq!(node.data().is_expanded(), Some(false));
    drop(tree1);
    press(&shell, vcx, Role::Button, &label);
    let tree2 = tree(vcx);
    assert_eq!(
        named(&nodes(&tree2), Role::Button, &label)
            .data()
            .is_expanded(),
        Some(true)
    );
    drop(tree2);
    core.finish("chat-lamp");
    pump(&shell, vcx);
}
