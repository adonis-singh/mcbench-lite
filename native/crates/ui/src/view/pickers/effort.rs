//! The effort picker (`EffortPicker`, `web/app.js:1540-1583`; inventory
//! `components.md` 1.4 and D.4; `style-viewer-routes.md` 1.9): shown only
//! when the chosen model's facts say it reasons and list efforts
//! ([`crate::state::models::effort_shown`]). The trigger shows the current
//! effort ([`crate::state::models::current_effort`]: the chosen one if
//! listed, else the model's default, else the first); the list is the
//! model's efforts in the core's order, their words verbatim (drawn
//! capitalized, as the stylesheet does). A choice closes the list.
//!
//! Accessibility: the trigger is a button named `Reasoning effort: <word>`
//! that opens a list box, expanded while open; the list box is named
//! `Reasoning effort`, its options by their words, the current one
//! selected, with position and size. Up and Down wrap, Home and End go to
//! the ends, Enter or Space chooses (the page has only Tab), Escape closes
//! only the list; focus goes to the current option on open and back to the
//! trigger on close.

use std::rc::Rc;

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{Entry, capitalized, panel, trigger};
use crate::ModelFacts;
use crate::state::Step;
use crate::state::models::{EFFORT_LABEL, current_effort};
use crate::view::controls::fonts;
use crate::view::controls::popover::PopoverState;
use crate::view::icons::{self, Icon};
use crate::view::theme::{self, TextKind, colour, radius, shadow};
use crate::view::{Activate, MoveFirst, MoveLast, MoveNext, MovePrevious, ring};

/// What choosing an effort does.
pub type OnEffort = Rc<dyn Fn(String, &mut Window, &mut App)>;

/// The trigger's name: `Reasoning effort: <word>`.
pub fn trigger_name(current: &str) -> String {
    format!("{EFFORT_LABEL}: {current}")
}

/// The effort picker for a model with `facts` and the chosen `effort`.
pub fn effort_picker(
    state: &Entity<PopoverState>,
    facts: &ModelFacts,
    effort: Option<&str>,
    on_change: OnEffort,
    cx: &mut App,
) -> AnyElement {
    effort_picker_as(
        "effort-trigger",
        EFFORT_LABEL,
        false,
        state,
        facts,
        effort,
        on_change,
        cx,
    )
}

// settings agent: Settings' title model effort (`Title model reasoning
// effort`), its trigger 112 px wide, opening down.
/// [`effort_picker`] with its own id and label (`label`), opening under
/// its trigger when `below`.
#[allow(clippy::too_many_arguments)]
pub fn effort_picker_as(
    id: &'static str,
    label: &'static str,
    below: bool,
    state: &Entity<PopoverState>,
    facts: &ModelFacts,
    effort: Option<&str>,
    on_change: OnEffort,
    cx: &mut App,
) -> AnyElement {
    let fonts = fonts(cx);
    let current = current_effort(facts, effort).to_string();
    let selected = facts
        .efforts
        .iter()
        .position(|e| *e == current)
        .expect("the current effort is one of the model's");
    let toggler = state.clone();
    let press: crate::view::Press = Rc::new(move |window, cx| {
        toggler.update(cx, |s, cx| {
            if s.is_open() {
                s.close(window, cx);
            } else {
                s.open(window, cx);
                let item = s.item(selected, cx);
                window.focus(&item, cx);
            }
        })
    });
    let element = trigger(
        id,
        format!("{label}: {current}").into(),
        accesskit::HasPopup::Listbox,
        state,
        press,
        cx,
    )
    .justify_between()
    .gap(px(7.))
    // chat-fix-2: in the composer `min-width: 0` (`.composer-controls
    // .effort-picker-trigger`, S:1847-1851), not the base 76 px.
    .px(px(7.))
    .py(px(4.));
    let trigger = theme::text(element, TextKind::SmallTrigger, &fonts)
        .child(capitalized(&current))
        .child(
            div()
                .opacity(0.5)
                .mr(px(-3.))
                .child(icons::icon(Icon::ChevronDown, 12.)),
        );
    let count = facts.efforts.len();
    let mut list = div()
        .id("effort-list")
        // It clips its rows: one scrolled out is not showing (W2).
        .a11y_synthetic_children(|builder| {
            builder.parent_node().set_clips_children();
        })
        .role(Role::ListBox)
        .aria_label(label)
        .aria_size_of_set(count)
        .key_context("Menu")
        .w(px(174.))
        .p(px(5.))
        .rounded(px(radius::MENU))
        .border_1()
        .border_color(colour::line())
        .bg(colour::well())
        .shadow(shadow::menu())
        .flex()
        .flex_col();
    for (s, step) in [
        (state.clone(), Step::Previous),
        (state.clone(), Step::Next),
        (state.clone(), Step::First),
        (state.clone(), Step::Last),
    ] {
        let mover = move |window: &mut Window, cx: &mut App| {
            s.update(cx, |p, cx| p.step(count, step, window, cx))
        };
        list = match step {
            Step::Previous => list.on_action(move |_: &MovePrevious, w, cx| mover(w, cx)),
            Step::Next => list.on_action(move |_: &MoveNext, w, cx| mover(w, cx)),
            Step::First => list.on_action(move |_: &MoveFirst, w, cx| mover(w, cx)),
            _ => list.on_action(move |_: &MoveLast, w, cx| mover(w, cx)),
        };
    }
    for (index, word) in facts.efforts.iter().enumerate() {
        let focus = state.update(cx, |s, cx| s.item(index, cx));
        let chosen = index == selected;
        let choose = {
            let (state, on_change, word) = (state.clone(), on_change.clone(), word.clone());
            Rc::new(move |window: &mut Window, cx: &mut App| {
                on_change(word.clone(), window, cx);
                state.update(cx, |s, cx| s.close(window, cx));
            })
        };
        let (click, key) = (choose.clone(), choose);
        let option = ring(
            div()
                .id(("effort-option", index))
                .role(Role::ListBoxOption)
                .aria_label(word.clone())
                .aria_selected(chosen)
                .aria_position_in_set(index)
                .aria_size_of_set(count)
                .track_focus(&focus)
                .key_context("Part")
                .on_click(move |_, window, cx| click(window, cx))
                .on_action(move |_: &Activate, window, cx| key(window, cx)),
        )
        .min_h(px(34.))
        .px(px(8.))
        .py(px(7.))
        .rounded(px(radius::ROW))
        .flex()
        .items_center()
        .gap(px(8.))
        .cursor_pointer()
        .hover(|s| s.bg(colour::raised()).text_color(colour::bone()));
        list = list.child(
            theme::text(option, TextKind::EffortOption, &fonts)
                .when(chosen, |d| d.text_color(colour::bone()))
                .child(div().flex_1().min_w_0().truncate().child(capitalized(word)))
                .when(chosen, |d| {
                    d.child(
                        div()
                            .text_color(colour::gold())
                            .child(icons::icon(Icon::Check, 14.)),
                    )
                }),
        );
    }
    div()
        .relative()
        .child(trigger)
        .children(if below {
            crate::view::controls::popover::panel(
                state,
                crate::view::controls::popover::Placement::Below,
                list.into_any_element(),
                cx,
            )
        } else {
            panel(state, Entry::Up, list.into_any_element(), cx)
        })
        .into_any_element()
}
