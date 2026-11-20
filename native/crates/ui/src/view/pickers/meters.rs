//! The context meter and the limit chip (`ContextMeter`, `LimitChip`,
//! `web/app.js:184-196` and 289-295; inventory `components.md` C.3 and
//! C.4; `style-viewer-routes.md` the composer's `.context-meter` and
//! `.limit-chip`): a 16 px ring in a 26 px box whose arc is the share of
//! the context the last request filled (from 12 o'clock, clockwise; `--star`
//! above 85 %), with its tip; and the chip near or at a rate limit with
//! the reset time as its tip. The state is `state::meters`.
//!
//! Accessibility (the page gives the meter nothing): the meter is a meter
//! named `Context · <used> of <window> tokens`, its value the share in
//! percent (0 to 100), so a reader says the percentage once, from the
//! value, and its tip describes nothing (the name and the value say it);
//! the chip a label named by its text, described by its tip. Both are focus
//! stops, so a keyboard reaches what the pointer's tips say.

use gpui_kit::*;

use crate::state::meters::{Chip, Context};
use crate::view::controls::{focus_for, fonts, pointer_tip, tip};
use crate::view::ring;
use crate::view::theme::{self, TextKind, colour};

/// The meter.
pub fn context_meter(
    id: impl Into<SharedString>,
    meter: &Context,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let id: SharedString = id.into();
    let focus = focus_for(id.clone(), window, cx).tab_stop(true);
    let share = meter.share;
    let high = meter.high;
    let ring_path = canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            let centre = bounds.center();
            let r = px(6.5);
            let stroke = px(2.);
            let circle = |from: f32, to: f32| {
                let mut path = PathBuilder::stroke(stroke);
                let steps = (((to - from) * 64.).ceil() as usize).max(1);
                for i in 0..=steps {
                    let t = from + (to - from) * i as f32 / steps as f32;
                    let angle = t * std::f32::consts::TAU - std::f32::consts::FRAC_PI_2;
                    let p = point(centre.x + r * angle.cos(), centre.y + r * angle.sin());
                    if i == 0 {
                        path.move_to(p);
                    } else {
                        path.line_to(p);
                    }
                }
                path.build()
            };
            if let Ok(track) = circle(0., 1.) {
                window.paint_path(track, colour::meter_track());
            }
            if share > 0.
                && let Ok(fill) = circle(0., share)
            {
                window.paint_path(fill, colour::meter_fill(high));
            }
        },
    )
    .size(px(16.));
    let element = ring(
        div()
            .id(id.clone())
            .role(Role::Meter)
            .aria_label(meter.name.clone())
            .aria_numeric_value((share * 100.).round() as f64)
            .aria_min_numeric_value(0.)
            .aria_max_numeric_value(100.)
            .track_focus(&focus),
    )
    .size(px(26.))
    .rounded(px(7.))
    .flex()
    .items_center()
    .justify_center()
    .hover(|s| s.bg(colour::hover()))
    .child(ring_path);
    pointer_tip(
        element,
        SharedString::from(format!("{id}-tip")),
        meter.tip.clone(),
    )
    .into_any_element()
}

/// The chip.
pub fn limit_chip(
    id: impl Into<SharedString>,
    chip: &Chip,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let id: SharedString = id.into();
    let fonts = fonts(cx);
    let focus = focus_for(id.clone(), window, cx).tab_stop(true);
    let element = theme::text(
        ring(
            div()
                .id(id.clone())
                .role(Role::Label)
                // A label's name over AT-SPI is its value (AccessKit).
                .aria_label(chip.text.clone())
                .aria_value(chip.text.clone())
                .track_focus(&focus),
        )
        .h(px(22.))
        .px(px(8.))
        .rounded(px(6.))
        .flex()
        .items_center()
        .bg(colour::limit_fill(chip.rejected)),
        TextKind::LimitChip,
        &fonts,
    )
    .text_color(colour::limit_text(chip.rejected))
    .child(chip.text.clone());
    match &chip.tip {
        Some(text) => tip(
            element,
            SharedString::from(format!("{id}-tip")),
            text.clone(),
        )
        .into_any_element(),
        None => element.into_any_element(),
    }
}
