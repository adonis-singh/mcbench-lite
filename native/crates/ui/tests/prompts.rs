//! The prompt editor's AI controls, paste, tag suggestions, the list's drag
//! and the palette's untitled prompt, against the page (par-bench, the
//! parity pass: `components.md` I.5, I.6, I.8, `app.md` 8.4), over the
//! stand-in's `bench` fixture opened on the library (`Castle with a moat`
//! shown first).

mod common;

use std::time::Duration;

use common::*;
use gpui::accesskit::Role;
use gpui::{TestAppContext, VisualTestContext};
use mcbench_ui::state::place::Place;

fn settle(shell: &ShellUnderTest, vcx: &mut VisualTestContext) {
    pump(shell, vcx);
    vcx.executor().advance_clock(Duration::from_millis(400));
    pump(shell, vcx);
}

fn labels(vcx: &mut VisualTestContext, role: Role) -> Vec<String> {
    let tree = tree(vcx);
    of_role(&nodes(&tree), role)
        .iter()
        .filter_map(|n| n.label())
        .collect()
}

fn press(shell: &ShellUnderTest, vcx: &mut VisualTestContext, role: Role, label: &str) {
    let tree = tree(vcx);
    click(vcx, named(&nodes(&tree), role, label));
    settle(shell, vcx);
}

fn starting(vcx: &mut VisualTestContext, role: Role, start: &str) -> String {
    labels(vcx, role)
        .into_iter()
        .find(|l| l.starts_with(start))
        .unwrap_or_else(|| panic!("no {role:?} starting {start:?}"))
}

fn library(shell: &ShellUnderTest, vcx: &mut VisualTestContext) -> Vec<mcbench_ui::LibraryPrompt> {
    let library = vcx.update(|_, cx| shell.entity.read(cx).library());
    vcx.update(|_, cx| library.read(cx).list().to_vec())
}

fn focus_on(vcx: &mut VisualTestContext, role: Role, label: &str) {
    let tree = tree(vcx);
    act(
        vcx,
        named(&nodes(&tree), role, label),
        gpui::accesskit::Action::Focus,
    );
}

#[gpui::test]
fn the_generation_effort_is_offered_for_a_reasoning_model_and_goes_with_improve(
    cx: &mut TestAppContext,
) {
    let (shell, _core, vcx) = open_bench(cx, "prompts-effort", Some(Place::Prompt), &[]);
    settle(&shell, vcx);
    let trigger = starting(vcx, Role::Button, "Generation reasoning effort: ");
    press(&shell, vcx, Role::Button, &trigger);
    press(&shell, vcx, Role::ListBoxOption, "high");
    assert_eq!(
        starting(vcx, Role::Button, "Generation reasoning effort: "),
        "Generation reasoning effort: high"
    );
    press(&shell, vcx, Role::Button, "Improve with AI");
    let asked = log_lines(&shell.log)
        .into_iter()
        .find(|l| l["action"] == "improve")
        .expect("an improve");
    assert_eq!(asked["effort"], "high");
}

#[gpui::test]
fn the_ai_menu_s_items_say_what_each_does(cx: &mut TestAppContext) {
    let (shell, _core, vcx) = open_bench(cx, "prompts-ai-menu", Some(Place::Prompt), &[]);
    settle(&shell, vcx);
    press(&shell, vcx, Role::Button, "Choose what the AI works from");
    assert_eq!(
        labels(vcx, Role::MenuItemRadio),
        [
            "Improve the text Rewrites what is in the editor",
            "Write from a note Opens a box for a short description"
        ]
    );
}

#[gpui::test]
fn the_note_box_closes_on_escape_and_on_a_press_outside(cx: &mut TestAppContext) {
    let (shell, _core, vcx) = open_bench(cx, "prompts-note", Some(Place::Prompt), &[]);
    settle(&shell, vcx);
    press(&shell, vcx, Role::Button, "Choose what the AI works from");
    press(
        &shell,
        vcx,
        Role::MenuItemRadio,
        "Write from a note Opens a box for a short description",
    );
    press(&shell, vcx, Role::Button, "Write with AI");
    assert!(labels(vcx, Role::MultilineTextInput).contains(&"Note for the AI".to_string()));
    assert_eq!(focused(vcx).1.as_deref(), Some("Note for the AI"));
    vcx.simulate_keystrokes("escape");
    settle(&shell, vcx);
    assert!(!labels(vcx, Role::MultilineTextInput).contains(&"Note for the AI".to_string()));
    press(&shell, vcx, Role::Button, "Write with AI");
    assert!(labels(vcx, Role::MultilineTextInput).contains(&"Note for the AI".to_string()));
    let tree1 = tree(vcx);
    let body = named(&nodes(&tree1), Role::MultilineTextInput, "Prompt text");
    pointer_click(vcx, body);
    settle(&shell, vcx);
    assert!(!labels(vcx, Role::MultilineTextInput).contains(&"Note for the AI".to_string()));
}

#[gpui::test]
fn a_copied_message_pasted_in_the_body_brings_its_text_and_files(cx: &mut TestAppContext) {
    let (shell, _core, vcx) = open_bench(cx, "prompts-paste", Some(Place::Prompt), &[]);
    settle(&shell, vcx);
    let file = mcbench_ui::OutgoingFile {
        name: "plan.png".into(),
        content_type: "image/png".into(),
        size: mcbench_ui::dev::fixtures::PICTURE.len() as u64,
        data: std::sync::Arc::from(mcbench_ui::dev::fixtures::PICTURE),
    };
    let metadata = mcbench_ui::state::clipboard::message_metadata(
        " with banners",
        std::slice::from_ref(&file),
    );
    vcx.write_to_clipboard(gpui::ClipboardItem::new_string_with_json_metadata(
        " with banners".to_string(),
        metadata,
    ));
    focus_on(vcx, Role::MultilineTextInput, "Prompt text");
    // The end of the text: Cmd-Down on macOS, Ctrl-End elsewhere (the
    // input's own bindings).
    vcx.simulate_keystrokes(if cfg!(target_os = "macos") {
        "cmd-down"
    } else {
        "ctrl-end"
    });
    vcx.dispatch_action(gpui_kit::component::input::Paste);
    settle(&shell, vcx);
    let castle = library(&shell, vcx)
        .into_iter()
        .find(|p| p.id == "prompt-castle")
        .expect("kept");
    assert!(castle.text.ends_with(" with banners"), "{:?}", castle.text);
    let names: Vec<&str> = castle.attachments.iter().map(|a| a.name.as_str()).collect();
    assert_eq!(names, ["plan.png"]);
}

#[gpui::test]
fn the_tag_field_suggests_the_other_prompts_tags(cx: &mut TestAppContext) {
    let (shell, _core, vcx) = open_bench(cx, "prompts-tag-suggest", Some(Place::Prompt), &[]);
    settle(&shell, vcx);
    focus_on(vcx, Role::TextInput, "Add tag");
    vcx.simulate_input("fa");
    settle(&shell, vcx);
    assert!(labels(vcx, Role::ListBox).contains(&"Tag suggestions".to_string()));
    assert_eq!(labels(vcx, Role::ListBoxOption), ["farm"]);
    vcx.simulate_keystrokes("down");
    vcx.simulate_keystrokes("enter");
    settle(&shell, vcx);
    let castle = library(&shell, vcx)
        .into_iter()
        .find(|p| p.id == "prompt-castle")
        .expect("kept");
    assert_eq!(castle.tags, ["castle", "stone", "farm"]);
    assert!(!labels(vcx, Role::ListBox).contains(&"Tag suggestions".to_string()));
}

#[gpui::test]
fn a_dropped_prompt_settles_for_150_ms_before_the_order_changes(cx: &mut TestAppContext) {
    let (shell, _core, vcx) = open_bench(cx, "prompts-drop-settle", Some(Place::NewBench), &[]);
    settle(&shell, vcx);
    let before: Vec<String> = library(&shell, vcx).into_iter().map(|p| p.id).collect();
    let tree1 = tree(vcx);
    let first = named(&nodes(&tree1), Role::ListItem, "Castle with a moat");
    let from = centre_of(vcx, first.bounding_box().expect("bounds"));
    // Two rows down, as `a_prompt_dragged_down_two_rows_lands_there`.
    drag(vcx, from, gpui::point(from.x, from.y + gpui::px(72.)), 12);
    pump(&shell, vcx);
    vcx.executor().advance_clock(Duration::from_millis(100));
    pump(&shell, vcx);
    let settling: Vec<String> = library(&shell, vcx).into_iter().map(|p| p.id).collect();
    assert_eq!(settling, before, "not yet: it settles first");
    vcx.executor().advance_clock(Duration::from_millis(100));
    pump(&shell, vcx);
    let after: Vec<String> = library(&shell, vcx).into_iter().map(|p| p.id).collect();
    assert_eq!(after[2], before[0], "{after:?}");
}

#[gpui::test]
fn the_palette_names_an_untitled_prompt_s_edit_button_edit_prompt(cx: &mut TestAppContext) {
    let (shell, _core, vcx) = open_bench(cx, "prompts-untitled", Some(Place::NewBench), &[]);
    settle(&shell, vcx);
    let library_entity = vcx.update(|_, cx| shell.entity.read(cx).library());
    vcx.update(|_, cx| {
        library_entity.update(cx, |l, cx| {
            let id = l.create(cx);
            l.update(&id, |p| p.text = "A lone tree".into(), cx);
        })
    });
    settle(&shell, vcx);
    let chat = vcx.update(|_, cx| shell.entity.read(cx).chat());
    vcx.update(|window, cx| chat.update(cx, |c, cx| c.focus_composer(window, cx)));
    vcx.simulate_input("/");
    settle(&shell, vcx);
    let edits = labels(vcx, Role::Button)
        .into_iter()
        .filter(|b| b.starts_with("Edit "))
        .collect::<Vec<_>>();
    assert!(edits.contains(&"Edit prompt".to_string()), "{edits:?}");
    assert!(!edits.contains(&"Edit Untitled prompt".to_string()));
}

#[gpui::test]
fn a_new_prompt_s_id_is_a_uuid(cx: &mut TestAppContext) {
    let (shell, _core, vcx) = open_bench(cx, "prompts-uuid", Some(Place::NewBench), &[]);
    settle(&shell, vcx);
    let library_entity = vcx.update(|_, cx| shell.entity.read(cx).library());
    let id = vcx.update(|_, cx| library_entity.update(cx, |l, cx| l.create(cx)));
    let parts: Vec<&str> = id.split('-').collect();
    assert_eq!(
        parts.iter().map(|p| p.len()).collect::<Vec<_>>(),
        [8, 4, 4, 4, 12],
        "{id}"
    );
    assert!(id.chars().all(|c| c == '-' || c.is_ascii_hexdigit()));
    assert_eq!(&parts[2][..1], "4", "version 4");
}

#[gpui::test]
fn a_model_s_prompt_says_who_added_it_and_the_generation_model_falls_back_to_build_s(
    cx: &mut TestAppContext,
) {
    let (shell, _core, vcx) = open_bench(cx, "prompts-added-by", Some(Place::Prompt), &[]);
    settle(&shell, vcx);
    let added = library(&shell, vcx)
        .into_iter()
        .find(|p| p.added_by.is_some())
        .expect("a prompt a model added");
    let library_entity = vcx.update(|_, cx| shell.entity.read(cx).library());
    let id = added.id.clone();
    vcx.update(|_, cx| library_entity.update(cx, |l, cx| l.open(id, cx)));
    settle(&shell, vcx);
    let tree1 = tree(vcx);
    assert!(
        nodes(&tree1).iter().any(|n| n.role() == Role::Note
            && n.label()
                .is_some_and(|l| l.starts_with("Added by ") && l.ends_with(" in a chat"))),
        "the Added by mark"
    );
    // No generation model kept: Build's last model (claude-opus-5-5).
    let trigger = starting(vcx, Role::Button, "Generation model");
    assert!(trigger.contains("claude-opus-5-5"), "{trigger}");
}
