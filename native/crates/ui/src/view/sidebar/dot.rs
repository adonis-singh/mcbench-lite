//! The status dot (`.conv-dot`, inventory `style-viewer-routes.md` 1.5 and
//! 1.19): 7 by 7, hollow while idle or halted (it keeps its place), solid
//! `--waiting` with its glow while finished and unseen, easing in from grey
//! over 700 ms when it arrives from running (`conv-arrive`), and pulsing
//! between two greys while running (`conv-pulse`, 0.9 s each way,
//! ease-in-out). Every running dot pulses on one clock, whenever it began
//! (the page re-times each animation to the page's time zero): the phase
//! is read from [`EPOCH`]. The pulse advances in steps of [`STEP`] (20 a
//! second), not every display frame, and only while a dot runs; between
//! steps nothing is drawn. A running dot is drawn by the window's pulse
//! layer ([`layer`]), not by its row: the row draws the dot's place and
//! records where it is ([`Pulses`]), and each step redraws only the shell,
//! whose sidebar and chat pane are cached views and are not rendered again.
//! The page's reduced-motion pulse (3 s) is not offered: GPUI does not
//! report the setting. It moves only while the window is active and on a
//! hardware adapter ([`crate::view::motion`]); else it rests on its mid
//! colour and asks for no step (the page pulses always).

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use gpui_kit::*;

use crate::state::sidebar::Dot;
use crate::view::theme::{mix, sidebar as tokens};

/// The pulse's clock's zero: the first running dot drawn.
pub static EPOCH: OnceLock<Instant> = OnceLock::new();

/// One way of the pulse (`0.9s ... alternate`).
pub const PULSE: Duration = Duration::from_millis(900);

/// How often the pulse advances: 20 steps a second.
pub const STEP: Duration = Duration::from_millis(50);

/// The running dots the last sidebar paint drew: each dot's bounds and the
/// clip it was drawn under; and whether a step is waiting.
#[derive(Default)]
pub struct PulseDots {
    dots: Vec<(Bounds<Pixels>, ContentMask<Pixels>)>,
    scheduled: bool,
}

/// The running dots, shared by the sidebar (which records them as it
/// paints) and the shell's pulse layer (which draws them).
pub type Pulses = Rc<RefCell<PulseDots>>;

/// Forgets the recorded dots: the sidebar draws again and records them.
pub fn clear(pulses: &Pulses) {
    pulses.borrow_mut().dots.clear();
}

/// The time to the pulse's next step on the shared clock.
pub fn until_step(now: Instant) -> Duration {
    let epoch = *EPOCH.get_or_init(|| now);
    let step = STEP.as_nanos();
    let into = now.saturating_duration_since(epoch).as_nanos() % step;
    Duration::from_nanos((step - into) as u64)
}

/// The pulse layer: over the window, it draws every recorded running dot
/// in the pulse's colour now, and while there is one, asks for the next
/// step: a notify of `shell` (the root view) when the step's time comes.
pub fn layer(pulses: Pulses, shell: EntityId) -> AnyElement {
    canvas(
        |_, _, _| {},
        move |_, _, window, cx| {
            let now = cx.background_executor().now();
            // It pulses only while it moves (`view::motion`); else it rests
            // on its mid colour and asks for no step.
            let moving = crate::view::motion::moving(window, cx);
            let colour = if moving {
                pulse_colour(now)
            } else {
                rest_colour()
            };
            let mut state = pulses.borrow_mut();
            for (bounds, mask) in &state.dots {
                window.with_content_mask(Some(*mask), |window| {
                    window.paint_quad(fill(*bounds, colour).corner_radii(bounds.size.width / 2.));
                });
            }
            if state.dots.is_empty() || state.scheduled || !moving {
                return;
            }
            state.scheduled = true;
            let wait = until_step(now);
            let pulses = pulses.clone();
            window
                .spawn(cx, async move |cx| {
                    cx.background_executor().timer(wait).await;
                    pulses.borrow_mut().scheduled = false;
                    let _ = cx.update(|_, cx| cx.notify(shell));
                })
                .detach();
        },
    )
    .absolute()
    .inset_0()
    .into_any_element()
}

/// The arrival's easing (`conv-arrive 700ms ease-out`).
pub const ARRIVE: Duration = Duration::from_millis(700);

/// CSS `ease-in-out` (`cubic-bezier(0.42, 0, 0.58, 1)`) at `t`.
fn ease_in_out(t: f32) -> f32 {
    bezier(0.42, 0., 0.58, 1., t)
}

/// CSS `ease-out` (`cubic-bezier(0, 0, 0.58, 1)`) at `t`.
fn ease_out(t: f32) -> f32 {
    bezier(0., 0., 0.58, 1., t)
}

fn bezier(x1: f32, y1: f32, x2: f32, y2: f32, t: f32) -> f32 {
    let curve = |a: f32, b: f32, s: f32| {
        3. * (1. - s) * (1. - s) * s * a + 3. * (1. - s) * s * s * b + s * s * s
    };
    let slope = |a: f32, b: f32, s: f32| {
        3. * (1. - s) * (1. - s) * a + 6. * (1. - s) * s * (b - a) + 3. * s * s * (1. - b)
    };
    let t = t.clamp(0., 1.);
    let mut s = t;
    for _ in 0..8 {
        let d = slope(x1, x2, s);
        if d.abs() < 1e-6 {
            break;
        }
        s = (s - (curve(x1, x2, s) - t) / d).clamp(0., 1.);
    }
    curve(y1, y2, s)
}

/// How far along the pulse is at `now` on the shared clock: 0 at its light
/// grey, 1 at its dark grey.
pub fn pulse_phase(now: Instant) -> f32 {
    let epoch = *EPOCH.get_or_init(|| now);
    let t = now.saturating_duration_since(epoch).as_secs_f32() / PULSE.as_secs_f32();
    let cycle = t.floor();
    let frac = t - cycle;
    ease_in_out(if (cycle as u64).is_multiple_of(2) {
        frac
    } else {
        1. - frac
    })
}

/// The pulse's colour at `now`.
pub fn pulse_colour(now: Instant) -> Hsla {
    mix(
        tokens::pulse_dark(),
        pulse_phase(now),
        tokens::pulse_light(),
    )
    .into()
}

/// The pulse's colour at rest: its mid point.
pub fn rest_colour() -> Hsla {
    mix(tokens::pulse_dark(), 0.5, tokens::pulse_light()).into()
}

/// A dot of `size` pixels (7, or 6 on the mode switcher); `arrival` is the
/// arrival's number when it arrived from running (keyed by `key`). A
/// running dot is its place, recorded in `pulses` as it paints; the pulse
/// layer draws it.
/// The arrival eases in only when `moving` (`view::motion`); else it is
/// drawn as arrived.
pub fn dot(
    key: &str,
    state: Dot,
    arrival: Option<u32>,
    size: f32,
    pulses: Pulses,
    moving: bool,
) -> AnyElement {
    let arrival = arrival.filter(|_| moving);
    let base = div().flex_none().size(px(size)).rounded_full();
    match state {
        Dot::Idle | Dot::Halted => base.into_any_element(),
        Dot::Running => base
            .child(
                canvas(
                    |_, _, _| {},
                    move |bounds, _, window, _| {
                        let mask = window.content_mask();
                        pulses.borrow_mut().dots.push((bounds, mask));
                    },
                )
                .size_full(),
            )
            .into_any_element(),
        Dot::Unread => match arrival {
            None => base
                .bg(Hsla::from(tokens::waiting()))
                .shadow(tokens::waiting_glow(1.))
                .into_any_element(),
            Some(number) => base
                .with_animation(
                    ElementId::Name(format!("arrive-{key}-{number}").into()),
                    Animation::new(ARRIVE),
                    move |d, t| {
                        let t = ease_out(t);
                        d.bg(Hsla::from(mix(tokens::waiting(), t, tokens::arrive_from())))
                            .shadow(tokens::waiting_glow(t))
                    },
                )
                .into_any_element(),
        },
    }
}
