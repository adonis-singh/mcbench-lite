//! The composer's pickers and meters (inventory `components.md` C.3, C.4,
//! D.1 to D.4 and G.3; `app.md` 2.5, 6.6 and 8.6; `style-viewer-routes.md`
//! 1.8, 1.9 and 1.17), each in its own file, with their pure state in
//! `state::{choice, favorites, meters, models, resolution}`:
//!
//! | Part | File | Page |
//! |---|---|---|
//! | the providers, catalogues, facts and favourites | [`catalog`] | `/api/config`, `shared.mjs`, `favorites.mjs` |
//! | the model picker | [`model`] | `ModelPicker` |
//! | the effort picker | [`effort`] | `EffortPicker` |
//! | the resolution picker and tag | [`resolution`] | `ResolutionPicker`, `ResolutionTag` |
//! | the context meter and limit chip | [`meters`] | `ContextMeter`, `LimitChip` |
//! | the footer's pickers and their choice | [`footer`] | `App`'s composer controls (6.6) |
//!
//! The composer's footer mounts them through
//! `view::chat::controls::footer_pickers`.
//!
//! Each popover is a [`PopoverState`] (open, closing, closed; focus back to
//! its trigger on close) drawn by [`panel`]: above its trigger and
//! right-aligned with it, 8 px off (the composer's pickers open upward,
//! `bottom: calc(100% + 8px); right: 0`); Escape inside closes it and goes
//! no further; a press outside it closes it, except on its own trigger,
//! whose click closes it (the page's root holds both): the shared popover
//! (`controls::popover`) with its `AboveRight` placement.
//!
//! What GPUI cannot draw, and so differs from the page: the popovers'
//! `scale(0.985)` and the chevron's turn; they fade and slide instead.

pub mod catalog;
pub mod effort;
pub mod footer;
pub mod meters;
pub mod model;
pub mod resolution;

use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::controls::popover::PopoverState;
use super::{Activate, theme};

/// Where an element was last drawn (the resolution slider's track).
pub type Bounds = Rc<Cell<gpui_kit::Bounds<Pixels>>>;

pub use super::controls::popover::Entry;

/// A trigger's element: a ghost control (`.model-picker-trigger`,
/// `.effort-picker-trigger`, `.resolution-picker-trigger`), a button
/// named `name` that says it opens `popup`, expanded while open, whose
/// press (click, Enter, Space, a reader's press, expand or collapse)
/// toggles its popover. Its children are what it shows.
pub fn trigger(
    id: impl Into<ElementId>,
    name: SharedString,
    popup: accesskit::HasPopup,
    state: &Entity<PopoverState>,
    on_press: super::Press,
    cx: &App,
) -> Stateful<Div> {
    let open = state.read(cx).is_open();
    let focus = state.read(cx).trigger();
    let (click, key, expand, collapse) = (
        on_press.clone(),
        on_press.clone(),
        on_press.clone(),
        on_press,
    );
    super::ring(
        div()
            .id(id.into())
            .role(Role::Button)
            .aria_label(name)
            .aria_expanded(open)
            .a11y_synthetic_children(move |builder| builder.parent_node().set_has_popup(popup))
            .track_focus(&focus.tab_stop(true))
            .key_context("Part")
            .on_click(move |_, window, cx| click(window, cx))
            .on_action(move |_: &Activate, window, cx| key(window, cx))
            .on_a11y_action(accesskit::Action::Expand, move |_, window, cx| {
                if !open {
                    expand(window, cx)
                }
            })
            .on_a11y_action(accesskit::Action::Collapse, move |_, window, cx| {
                if open {
                    collapse(window, cx)
                }
            }),
    )
    .relative()
    .flex()
    .flex_none()
    .items_center()
    .min_h(px(32.))
    .rounded(px(theme::radius::SM))
    .cursor_pointer()
    .text_color(theme::colour::ash())
    .hover(|s| {
        s.bg(theme::colour::hover())
            .text_color(theme::colour::bone())
    })
    .when(open, |d| {
        d.bg(theme::colour::well())
            .text_color(theme::colour::bone())
    })
    .child(super::controls::popover::record_trigger(state, cx))
}

/// The popover's panel when shown: the shared one
/// (`controls::popover::panel`), above the trigger, right-aligned with it,
/// 8 px off; put it inside the same `div().relative()` as the trigger.
pub fn panel(
    state: &Entity<PopoverState>,
    entry: Entry,
    content: AnyElement,
    cx: &mut App,
) -> Option<AnyElement> {
    super::controls::popover::panel(
        state,
        super::controls::popover::Placement::AboveRight(entry),
        content,
        cx,
    )
}

/// A new, empty bounds (the resolution slider's track).
pub fn bounds() -> Bounds {
    Rc::new(Cell::new(gpui_kit::Bounds::default()))
}

/// The word as `text-transform: capitalize` draws it (the effort's).
pub fn capitalized(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}
