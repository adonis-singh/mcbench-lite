//! The resolution picker and tag (`ResolutionPicker`, `ResolutionTag`,
//! `web/app.js:203-287`; inventory `components.md` 1.4, D.1 and D.2;
//! `app.md` 6.6; `style-viewer-routes.md` 1.9): the trigger shows the
//! stop's short label (`1×` to `10×`, in `--bone` above Standard); its
//! popover holds the head (`Resolution` and the stop's label), a slider
//! over the four stops (the arrows, Home, End and the Page keys step it;
//! each step changes the resolution at once and the popover stays open),
//! a tick per stop, and the stop's help. The slider takes focus on open.
//! Read only (a bench's comparison), the trigger is disabled and opens
//! nothing. The tag is a list row's, above Standard only.
//!
//! Accessibility: the trigger is a button named `Resolution: <label>` that
//! opens a dialog, expanded while open; the dialog is named `Resolution`;
//! the slider is a slider named `Resolution`, its value the stop's place (0
//! to 3, step 1) and its value text the stop's label, described by the
//! stop's help, taking the increment, decrement and set-value actions; each
//! tick a button named by its text.

use std::rc::Rc;

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{Bounds, Entry, panel, trigger};
use crate::Resolution;
use crate::state::resolution::{tag, trigger_name};
use crate::view::controls::fonts;
use crate::view::controls::popover::PopoverState;
use crate::view::icons::{self, Icon};
use crate::view::theme::{self, TextKind, colour, radius, shadow};
use crate::view::{Activate, ring};

/// What choosing a stop does.
pub type OnResolution = Rc<dyn Fn(Resolution, &mut Window, &mut App)>;

/// The stop a key moves the slider to from `at` (the native range's keys:
/// Right and Up on, Left and Down back, Home and End, Page Up and Page
/// Down a step); none for any other key.
pub fn stepped(at: Resolution, key: &str) -> Option<Resolution> {
    let index = at.index();
    let last = Resolution::ALL.len() - 1;
    let next = match key {
        "right" | "up" | "pageup" => (index + 1).min(last),
        "left" | "down" | "pagedown" => index.saturating_sub(1),
        "home" => 0,
        "end" => last,
        _ => return None,
    };
    Some(Resolution::ALL[next])
}

/// The stop nearest a pointer at `x` on a track from `left`, `width` wide.
pub fn at_pointer(x: f32, left: f32, width: f32) -> Resolution {
    let last = (Resolution::ALL.len() - 1) as f32;
    let t = ((x - left) / width.max(1.)).clamp(0., 1.);
    Resolution::ALL[(t * last).round() as usize]
}

/// The picker; `read_only` draws the bench's stop.
pub fn resolution_picker(
    state: &Entity<PopoverState>,
    track: &Bounds,
    value: Resolution,
    read_only: bool,
    on_change: OnResolution,
    cx: &mut App,
) -> AnyElement {
    let fonts = fonts(cx);
    let raised = value != Resolution::Standard;
    if read_only {
        let element = theme::text(
            ring(
                div()
                    .id("resolution-trigger")
                    .role(Role::Button)
                    .aria_label(trigger_name(value, true))
                    .a11y_synthetic_children(|builder| builder.parent_node().set_disabled())
                    .track_focus(&state.read(cx).trigger().tab_stop(false)),
            )
            .flex()
            .items_center()
            // chat-fix-2: in the composer `min-width: 0` (`.composer-controls
            // .resolution-picker-trigger`, S:1847-1851), not the base 76 px.
            .min_h(px(32.))
            .px(px(7.))
            .py(px(4.))
            .rounded(px(radius::SM)),
            TextKind::SmallTrigger,
            &fonts,
        )
        .when(raised, |d| d.text_color(colour::bone()))
        .child(value.short());
        // par-shell (D.1): the page's `title` on hover.
        return crate::view::controls::pointer_tip(
            element,
            "resolution-trigger-tip",
            trigger_name(value, true),
        )
        .into_any_element();
    }
    let toggler = state.clone();
    let slider_focus = state.update(cx, |s, cx| s.item(0, cx));
    let press: crate::view::Press =
        Rc::new(move |window, cx| toggler.update(cx, |s, cx| s.toggle(window, cx)));
    let element = trigger(
        "resolution-trigger",
        trigger_name(value, false).into(),
        accesskit::HasPopup::Dialog,
        state,
        press,
        cx,
    )
    .justify_between()
    .gap(px(7.))
    // chat-fix-2: in the composer `min-width: 0` (`.composer-controls
    // .resolution-picker-trigger`, S:1847-1851), not the base 76 px.
    .px(px(7.))
    .py(px(4.));
    let trigger = theme::text(element, TextKind::SmallTrigger, &fonts)
        .when(raised, |d| d.text_color(colour::bone()))
        .child(value.short())
        .child(
            div()
                .opacity(0.5)
                .mr(px(-3.))
                .child(icons::icon(Icon::ChevronDown, 12.)),
        );
    // par-shell (D.1): the page's `title` on hover.
    let trigger = crate::view::controls::pointer_tip(
        trigger,
        "resolution-trigger-tip",
        trigger_name(value, false),
    );
    let ticks: Vec<FocusHandle> = (1..=Resolution::ALL.len())
        .map(|i| state.update(cx, |s, cx| s.item(i, cx)))
        .collect();
    let dialog = resolution_dialog(value, &slider_focus, &ticks, track, on_change, cx);
    div()
        .relative()
        .child(trigger)
        .children(panel(state, Entry::Up, dialog, cx))
        .into_any_element()
}

fn resolution_dialog(
    value: Resolution,
    slider_focus: &FocusHandle,
    tick_focus: &[FocusHandle],
    track: &Bounds,
    on_change: OnResolution,
    cx: &mut App,
) -> AnyElement {
    let fonts = fonts(cx);
    let index = value.index() as f32;
    let last = (Resolution::ALL.len() - 1) as f32;
    let share = index / last;
    let (key_change, inc, dec, set, down, drag) = (
        on_change.clone(),
        on_change.clone(),
        on_change.clone(),
        on_change.clone(),
        on_change.clone(),
        on_change.clone(),
    );
    let (down_track, drag_track) = (track.clone(), track.clone());
    let cell = track.clone();
    let slider = div()
        .id("resolution-slider")
        .role(Role::Slider)
        .aria_label("Resolution")
        .aria_numeric_value(index as f64)
        .aria_min_numeric_value(0.)
        .aria_max_numeric_value(last as f64)
        .aria_numeric_value_step(1.)
        .aria_value(value.label())
        .aria_description(value.help())
        .aria_orientation(accesskit::Orientation::Horizontal)
        .track_focus(&slider_focus.clone().tab_stop(true))
        .on_key_down(move |event, window, cx| {
            if let Some(next) = stepped(value, event.keystroke.key.as_str()) {
                cx.stop_propagation();
                if next != value {
                    key_change(next, window, cx);
                }
            }
        })
        .on_a11y_action(accesskit::Action::Increment, move |_, window, cx| {
            inc(stepped(value, "right").expect("a step"), window, cx)
        })
        .on_a11y_action(accesskit::Action::Decrement, move |_, window, cx| {
            dec(stepped(value, "left").expect("a step"), window, cx)
        })
        .on_a11y_action(accesskit::Action::SetValue, move |data, window, cx| {
            let Some(accesskit::ActionData::NumericValue(n)) = data else {
                panic!("the slider's set value carries a number, not {data:?}")
            };
            let index = (n.round().clamp(0., 3.)) as usize;
            set(Resolution::ALL[index], window, cx)
        })
        .on_mouse_down(MouseButton::Left, move |event, window, cx| {
            let b = down_track.get();
            down(
                at_pointer(
                    f32::from(event.position.x),
                    f32::from(b.origin.x),
                    f32::from(b.size.width),
                ),
                window,
                cx,
            );
        })
        .on_mouse_move(move |event, window, cx| {
            if event.pressed_button == Some(MouseButton::Left) {
                let b = drag_track.get();
                let next = at_pointer(
                    f32::from(event.position.x),
                    f32::from(b.origin.x),
                    f32::from(b.size.width),
                );
                if next != value {
                    drag(next, window, cx);
                }
            }
        })
        .mt(px(10.))
        .h(px(20.))
        .relative()
        .rounded(px(6.))
        .border_1()
        .border_color(colour::none())
        .focus_visible(|s| s.border_color(colour::slider_ring()))
        .cursor_pointer()
        .child(
            div()
                .absolute()
                .left(px(8.))
                .right(px(8.))
                .top(px(8.))
                .h(px(4.))
                .rounded_full()
                .bg(colour::raised())
                .child(
                    canvas(move |b, _, _| cell.set(b), |_, _, _, _| {})
                        .absolute()
                        .size_full(),
                )
                .child(
                    div()
                        .absolute()
                        .left_0()
                        .top_0()
                        .h_full()
                        .w(relative(share))
                        .rounded_full()
                        .bg(colour::bone()),
                )
                .child(
                    div()
                        .absolute()
                        .top(px(-5.))
                        .left(relative(share))
                        .ml(px(-7.))
                        .size(px(14.))
                        .rounded_full()
                        .bg(colour::bone())
                        .shadow(shadow::thumb()),
                ),
        );
    let ticks = Resolution::ALL.iter().enumerate().fold(
        div().relative().h(px(20.)).mx(px(8.)).mt(px(2.)),
        |d, (i, stop)| {
            let stop = *stop;
            let (click, key) = (on_change.clone(), on_change.clone());
            let tick = theme::text(
                ring(
                    div()
                        .id(("resolution-tick", i))
                        .role(Role::Button)
                        .aria_label(stop.tick())
                        .track_focus(&tick_focus[i].clone().tab_stop(true))
                        .key_context("Part")
                        .on_click(move |_, window, cx| click(stop, window, cx))
                        .on_action(move |_: &Activate, window, cx| key(stop, window, cx)),
                )
                .absolute()
                .px(px(4.))
                .py(px(2.))
                .rounded(px(5.))
                .cursor_pointer()
                .hover(|s| s.bg(colour::raised()).text_color(colour::bone())),
                TextKind::ResolutionTick,
                &fonts,
            )
            .when(stop == value, |d| d.text_color(colour::bone()))
            .map(|d| match i {
                0 => d.left(px(-12.)),
                3 => d.right(px(-12.)),
                _ => d.left(relative(i as f32 / 3.)).ml(px(-10.)),
            })
            .child(stop.tick());
            d.child(tick)
        },
    );
    let longest = Resolution::Maximum.help();
    let help = theme::text(
        div().relative().mt(px(8.)),
        TextKind::ResolutionHelp,
        &fonts,
    )
    .child(div().invisible().child(longest))
    .child(
        div()
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .child(value.help()),
    );
    theme::text(
        div()
            .id("resolution-dialog")
            .role(Role::Dialog)
            .aria_label("Resolution")
            .w(px(240.))
            .pt(px(10.))
            .px(px(12.))
            .pb(px(11.))
            .rounded(px(radius::MENU))
            .border_1()
            .border_color(colour::line())
            .bg(colour::well())
            .shadow(shadow::menu())
            .flex()
            .flex_col(),
        TextKind::SmallTrigger,
        &fonts,
    )
    .child(
        theme::text(
            div().flex().items_baseline().justify_between(),
            TextKind::ResolutionHead,
            &fonts,
        )
        .child("Resolution")
        .child(div().text_color(colour::bone()).child(value.label())),
    )
    .child(slider)
    .child(ticks)
    .child(help)
    .into_any_element()
}

/// A list row's tag (`ResolutionTag`): nothing at Standard, else the short
/// label, its title `Resolution: <label>` (its description).
pub fn resolution_tag(value: Resolution, cx: &App) -> Option<AnyElement> {
    let (short, title) = tag(i64::from(value.value()))?;
    let fonts = fonts(cx);
    Some(
        theme::text(
            div()
                .id(("resolution-tag", value.value() as usize))
                .role(Role::Label)
                .aria_label(short)
                .aria_value(short)
                .aria_description(title)
                .flex_none(),
            TextKind::ResolutionTag,
            &fonts,
        )
        .child(short)
        .into_any_element(),
    )
}
