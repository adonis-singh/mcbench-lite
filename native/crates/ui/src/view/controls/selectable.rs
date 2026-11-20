//! A line of text that can be selected and copied as the transcript's text
//! is (chat-fix): a failure shown where a list's items would be (the model
//! picker's catalogue error, a failed search in the palette and the
//! sidebar). A press and drag selects; the press puts focus on it (it is
//! not a Tab stop), so Ctrl+A or Cmd-A selects it all and Ctrl+C or Cmd-C
//! copies the selection (the `PickerNote` key context). Its node is a
//! `Label` named by the text, the text as runs, as the transcript's leaves.

use std::cell::RefCell;
use std::rc::Rc;

use gpui_kit::*;

use super::super::text_leaf::{HitMap, LeafA11y, TextLeaf};
use super::super::{CopySelection, SelectAll, colour};
use crate::state::selection::{Point as TextPoint, Selection};

/// The selection of one such line, kept by the view that draws it.
pub struct Selectable {
    selection: Option<Selection>,
    dragging: bool,
    hits: Rc<RefCell<HitMap>>,
    focus: FocusHandle,
}

impl Selectable {
    /// Nothing selected.
    pub fn new(cx: &mut App) -> Self {
        Selectable {
            selection: None,
            dragging: false,
            hits: Rc::new(RefCell::new(HitMap::default())),
            focus: cx.focus_handle(),
        }
    }

    /// Forgets the selection (the text changed).
    pub fn clear(&mut self) {
        self.selection = None;
        self.dragging = false;
    }
}

/// `text` in `color` and `font` as a selectable `Label`, in a box with id
/// `id` that the caller lays out and styles (size, padding, alignment).
/// `state` finds the [`Selectable`] in the view `T` that draws it.
pub fn selectable<T: 'static>(
    id: &'static str,
    text: String,
    font: Font,
    color: Hsla,
    state: fn(&mut T) -> &mut Selectable,
    view: &mut T,
    cx: &mut Context<T>,
) -> Stateful<Div> {
    let own = state(view);
    own.hits.borrow_mut().clear();
    let len = text.len();
    let selected = own.selection.and_then(|s| s.range_in(0, 0, len));
    let leaf = TextLeaf::new(
        SharedString::from(format!("{id}-text")),
        Role::Label,
        LeafA11y {
            label: Some(text.clone()),
            ..LeafA11y::default()
        },
        text.clone().into(),
        vec![TextRun {
            len,
            font,
            color,
            background_color: None,
            underline: None,
            strikethrough: None,
        }],
        Vec::new(),
        Rc::new(|d| d),
        selected,
        colour::selection(),
        own.hits.clone(),
        (0, 0),
    );
    let focus = own.focus.clone();
    div()
        .id(id)
        .key_context("PickerNote")
        .track_focus(&focus)
        .on_action(cx.listener(move |this, _: &SelectAll, _, cx| {
            let point = |offset| TextPoint {
                row: 0,
                leaf: 0,
                offset,
            };
            state(this).selection = Some(Selection {
                anchor: point(0),
                focus: point(len),
            });
            cx.notify();
        }))
        .on_action(cx.listener(move |this, _: &CopySelection, _, cx| {
            if let Some(range) = state(this).selection.and_then(|s| s.range_in(0, 0, len)) {
                cx.write_to_clipboard(ClipboardItem::new_string(text[range].to_string()));
            }
        }))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                let own = state(this);
                let point = own.hits.borrow().point_at(event.position);
                if let Some(point) = point {
                    own.selection = Some(Selection::caret(point));
                    own.dragging = true;
                }
                window.focus(&own.focus, cx);
                cx.stop_propagation();
                cx.notify();
            }),
        )
        .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, _, cx| {
            let own = state(this);
            if !own.dragging || event.pressed_button != Some(MouseButton::Left) {
                return;
            }
            let point = own.hits.borrow().point_at(event.position);
            if let (Some(point), Some(selection)) = (point, own.selection.as_mut()) {
                selection.focus = point;
                cx.notify();
            }
        }))
        .on_mouse_up(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseUpEvent, _, _| state(this).dragging = false),
        )
        .min_w_0()
        .whitespace_normal()
        .cursor_text()
        .child(leaf)
}
