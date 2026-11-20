//! gpui-base's editable spans (`native/vendor/README.md`, patch 49), in the
//! composer of a window over the stand-in's `bench` fixture: one test per
//! point of the patch, (a) to (g). The composer holds "Build a " + an empty
//! `size` box + " tower".

mod common;

use std::time::Duration;

use common::*;
use gpui::{Entity, TestAppContext, VisualTestContext, px};
use gpui_kit::component::input::{InlineField, InlineFieldError, TextareaState};
use mcbench_ui::state::place::Place;

/// The text box's own undo and redo (gpui-base `src/input/base/state.rs`
/// key bindings): Cmd-Z and Cmd-Shift-Z on macOS, Ctrl-Z and Ctrl-Y
/// elsewhere.
const UNDO: &str = if cfg!(target_os = "macos") {
    "cmd-z"
} else {
    "ctrl-z"
};
const REDO: &str = if cfg!(target_os = "macos") {
    "cmd-shift-z"
} else {
    "ctrl-y"
};

fn settle(shell: &ShellUnderTest, vcx: &mut VisualTestContext) {
    pump(shell, vcx);
    vcx.executor().advance_clock(Duration::from_millis(400));
    pump(shell, vcx);
}

/// The composer with "Build a  tower" and an empty `size` box at byte 8,
/// focused, the caret at the end of the text (outside the box).
fn composer_with_a_box<'a>(
    cx: &'a mut TestAppContext,
    test: &str,
) -> (
    ShellUnderTest,
    &'a mut VisualTestContext,
    Entity<TextareaState>,
) {
    let (shell, _core, vcx) = open_bench(cx, test, Some(Place::NewBench), &[]);
    settle(&shell, vcx);
    let chat = vcx.update(|_, cx| shell.entity.read(cx).chat());
    let composer = vcx.update(|_, cx| chat.read(cx).composer());
    vcx.update(|window, cx| {
        composer.update(cx, |c, cx| {
            c.focus(window, cx);
            c.replace_with_fields(
                "Build a  tower",
                vec![(8..8, InlineField::new("tower", "size"))],
                window,
                cx,
            )
            .expect("the box fits");
        })
    });
    settle(&shell, vcx);
    (shell, vcx, composer)
}

fn read<T>(
    vcx: &mut VisualTestContext,
    composer: &Entity<TextareaState>,
    f: impl FnOnce(&TextareaState) -> T,
) -> T {
    vcx.update(|_, cx| f(composer.read(cx)))
}

fn edit(
    vcx: &mut VisualTestContext,
    composer: &Entity<TextareaState>,
    f: impl FnOnce(&mut TextareaState, &mut gpui::Window, &mut gpui::Context<TextareaState>),
) {
    vcx.update(|window, cx| composer.update(cx, |c, cx| f(c, window, cx)));
}

fn value(vcx: &mut VisualTestContext, composer: &Entity<TextareaState>) -> String {
    read(vcx, composer, |c| c.value().to_string())
}

fn range(vcx: &mut VisualTestContext, composer: &Entity<TextareaState>) -> std::ops::Range<usize> {
    read(vcx, composer, |c| c.fields()[0].range())
}

#[gpui::test]
fn field_a_typing_stays_inside_and_an_edge_steps_out(cx: &mut TestAppContext) {
    let (shell, vcx, composer) = composer_with_a_box(cx, "field-a");
    assert_eq!(range(vcx, &composer), 8..8);
    assert_eq!(read(vcx, &composer, |c| c.active_field()), None);
    edit(vcx, &composer, |c, _, cx| c.focus_field(0, cx).unwrap());
    vcx.simulate_input("huge");
    settle(&shell, vcx);
    assert_eq!(value(vcx, &composer), "Build a huge tower");
    assert_eq!(range(vcx, &composer), 8..12);
    // Right at its end steps out at that edge; typing then is outside.
    vcx.simulate_keystrokes("right");
    assert_eq!(
        read(vcx, &composer, |c| (c.active_field(), c.cursor())),
        (None, 12)
    );
    vcx.simulate_input("!");
    settle(&shell, vcx);
    assert_eq!(value(vcx, &composer), "Build a huge! tower");
    assert_eq!(range(vcx, &composer), 8..12);
    // From outside the box is one unit.
    vcx.simulate_keystrokes("left left");
    assert_eq!(read(vcx, &composer, |c| c.cursor()), 8);
    edit(vcx, &composer, |c, _, cx| c.set_selected_range(10..10, cx));
    assert_eq!(read(vcx, &composer, |c| c.selected_range()), 12..12);
    // Inside, Left at its start steps out there, and Backspace at its
    // start deletes nothing outside.
    edit(vcx, &composer, |c, _, cx| {
        c.focus_field(0, cx).unwrap();
        c.set_selected_range(8..8, cx);
    });
    assert_eq!(read(vcx, &composer, |c| c.active_field()), Some(0));
    vcx.simulate_keystrokes("backspace");
    assert_eq!(value(vcx, &composer), "Build a huge! tower");
    vcx.simulate_keystrokes("left");
    assert_eq!(
        read(vcx, &composer, |c| (c.active_field(), c.cursor())),
        (None, 8)
    );
    // A selection reaching into it takes the whole box, and deleting it
    // removes the box.
    edit(vcx, &composer, |c, _, cx| c.set_selected_range(5..10, cx));
    assert_eq!(read(vcx, &composer, |c| c.selected_range()), 5..12);
    vcx.simulate_keystrokes("backspace");
    assert_eq!(value(vcx, &composer), "Build! tower");
    assert!(read(vcx, &composer, |c| c.fields().is_empty()));
}

#[gpui::test]
fn field_b_an_empty_box_is_drawn_as_wide_as_its_name(cx: &mut TestAppContext) {
    let (shell, vcx, composer) = composer_with_a_box(cx, "field-b");
    let empty = read(vcx, &composer, |c| c.field_bounds());
    assert_eq!(empty.len(), 1);
    let (index, bounds) = empty[0];
    assert_eq!(index, 0);
    // The name "size" and a `0` at 14 px, plus 6 px inside each side.
    assert!(bounds.size.width > px(30.), "{bounds:?}");
    edit(vcx, &composer, |c, _, cx| c.focus_field(0, cx).unwrap());
    vcx.simulate_input("an enormous, very tall");
    settle(&shell, vcx);
    let grown = read(vcx, &composer, |c| c.field_bounds());
    assert!(
        grown[0].1.size.width > bounds.size.width + px(60.),
        "{grown:?}"
    );
    // The text after the box moved right by the box's growth.
    assert_eq!(grown[0].1.origin.x, bounds.origin.x);
}

#[gpui::test]
fn field_c_a_paste_stays_in_the_box_on_one_line(cx: &mut TestAppContext) {
    let (shell, vcx, composer) = composer_with_a_box(cx, "field-c");
    edit(vcx, &composer, |c, window, cx| {
        c.focus_field(0, cx).unwrap();
        c.replace("very\nlarge", window, cx);
    });
    settle(&shell, vcx);
    assert_eq!(value(vcx, &composer), "Build a verylarge tower");
    assert_eq!(range(vcx, &composer), 8..17);
    // The text is the value, so a copy of it is plain text.
    let text = value(vcx, &composer);
    assert_eq!(&text[range(vcx, &composer)], "verylarge");
}

#[gpui::test]
fn field_d_undo_and_redo_put_back_the_value_and_the_box(cx: &mut TestAppContext) {
    let (shell, vcx, composer) = composer_with_a_box(cx, "field-d");
    edit(vcx, &composer, |c, _, cx| c.focus_field(0, cx).unwrap());
    vcx.simulate_input("huge");
    settle(&shell, vcx);
    vcx.simulate_keystrokes(UNDO);
    assert_eq!(value(vcx, &composer), "Build a  tower");
    assert_eq!(range(vcx, &composer), 8..8);
    vcx.simulate_keystrokes(REDO);
    assert_eq!(value(vcx, &composer), "Build a huge tower");
    assert_eq!(range(vcx, &composer), 8..12);
    // Deleting the box from outside and undoing it brings it back whole.
    edit(vcx, &composer, |c, _, cx| c.set_selected_range(12..12, cx));
    vcx.simulate_keystrokes("backspace");
    assert_eq!(value(vcx, &composer), "Build a  tower");
    assert!(read(vcx, &composer, |c| c.fields().is_empty()));
    vcx.simulate_keystrokes(UNDO);
    assert_eq!(value(vcx, &composer), "Build a huge tower");
    assert_eq!(range(vcx, &composer), 8..12);
}

#[gpui::test]
fn field_e_a_composition_in_the_box_stays_in_it(cx: &mut TestAppContext) {
    use gpui::EntityInputHandler as _;
    let (shell, vcx, composer) = composer_with_a_box(cx, "field-e");
    edit(vcx, &composer, |c, window, cx| {
        c.focus_field(0, cx).unwrap();
        c.replace_and_mark_text_in_range(None, "に", None, window, cx);
    });
    assert_eq!(range(vcx, &composer), 8..11);
    assert_eq!(read(vcx, &composer, |c| c.active_field()), Some(0));
    edit(vcx, &composer, |c, window, cx| {
        c.replace_and_mark_text_in_range(None, "にほ", None, window, cx);
        // Fields cannot be added while a composition is open.
        assert_eq!(
            c.replace_with_fields("x", vec![], window, cx),
            Err(InlineFieldError::UnsupportedMode)
        );
    });
    assert_eq!(range(vcx, &composer), 8..14);
    edit(vcx, &composer, |c, window, cx| {
        c.replace_text_in_range(None, "日本", window, cx);
    });
    settle(&shell, vcx);
    assert_eq!(value(vcx, &composer), "Build a 日本 tower");
    assert_eq!(range(vcx, &composer), 8..14);
    assert_eq!(read(vcx, &composer, |c| c.active_field()), Some(0));
}

#[gpui::test]
fn field_f_the_text_box_stays_one_text_with_the_field_named(cx: &mut TestAppContext) {
    let (shell, vcx, composer) = composer_with_a_box(cx, "field-f");
    edit(vcx, &composer, |c, _, cx| c.focus_field(0, cx).unwrap());
    vcx.simulate_input("huge");
    settle(&shell, vcx);
    // One text input named Message, its value the plain text, the caret in
    // the box after what was typed.
    let tree = tree(vcx);
    let focus = tree.state().focus().expect("focus");
    assert_eq!(focus.role(), gpui::accesskit::Role::MultilineTextInput);
    assert_eq!(focus.label().as_deref(), Some("Message"));
    assert_eq!(value(vcx, &composer), "Build a huge tower");
    let named = read(vcx, &composer, |c| {
        c.active_field()
            .map(|ix| c.fields()[ix].field().name().to_string())
    });
    assert_eq!(named.as_deref(), Some("size"));
}

#[gpui::test]
fn field_g_the_value_is_the_text_a_send_reads(cx: &mut TestAppContext) {
    let (shell, vcx, composer) = composer_with_a_box(cx, "field-g");
    edit(vcx, &composer, |c, window, cx| {
        c.set_selected_range(0..0, cx);
        c.replace("Please ", window, cx);
    });
    assert_eq!(range(vcx, &composer), 15..15);
    edit(vcx, &composer, |c, _, cx| c.focus_field(0, cx).unwrap());
    vcx.simulate_input("tall");
    settle(&shell, vcx);
    assert_eq!(value(vcx, &composer), "Please Build a tall tower");
    assert_eq!(
        read(vcx, &composer, |c| c.fields()[0].field().id().to_string()),
        "tower"
    );
    edit(vcx, &composer, |c, window, cx| {
        assert_eq!(
            c.replace_with_fields("a", vec![(0..2, InlineField::new("x", "y"))], window, cx),
            Err(InlineFieldError::InvalidRange)
        );
    });
}
