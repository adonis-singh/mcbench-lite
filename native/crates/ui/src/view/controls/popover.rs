//! The anchored popover every picker shares (`usePopover`, inventory
//! `components.md` G.3) and the menu inside most of them.
//!
//! A [`PopoverState`] entity per picker holds its open, closing and closed
//! phases ([`crate::state::popover`]) and the focus of its trigger and items.
//! [`panel`] draws the popover anchored to its trigger when shown: it opens
//! at once, anchored to the trigger's corner; Escape inside it closes it and
//! goes no further (no layer under it closes); a press outside it closes it,
//! but not one on its own trigger, whose click closes it;
//! closing plays its exit (110 or 120 ms, the picker's) and takes no input
//! meanwhile; focus goes to the first item on open and back to the trigger
//! on close. [`Menu`] is a list of items whose Up and Down wrap round and
//! whose Home and End go to the ends; Enter, Space or a click chooses an
//! item and closes the menu.
//!
//! Differs from today's page, by the stage-four brief: focus returns to the
//! trigger on close (the page leaves it where it was).
//!
//! A picker in one line: keep `cx.new(|cx| PopoverState::new(MENU_EXIT, cx))`,
//! draw the trigger with `trigger(&state, Button::new(..), cx)` and the
//! panel with `panel(&state, Placement::Below, content, window, cx)` next to
//! it inside one `div().relative()`.

use std::rc::Rc;
use std::time::Duration;

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::super::theme::{self, TextKind, colour, radius, shadow};
use super::super::{Activate, Dismiss, MoveFirst, MoveLast, MoveNext, MovePrevious, ring};
use super::{Button, fonts};
use crate::state::Step;
use crate::state::popover::{Phase, Popover, menu_step};

/// The entry of a menu (`effort-menu-in`, 150 ms).
pub const ENTER: Duration = Duration::from_millis(150);

/// One picker's popover.
pub struct PopoverState {
    popover: Popover,
    trigger: FocusHandle,
    items: Vec<FocusHandle>,
    /// Where the trigger was last drawn: a press on it is not a press
    /// outside the popover (the trigger's own click closes it).
    trigger_bounds: Rc<std::cell::Cell<Bounds<Pixels>>>,
}

impl PopoverState {
    /// A closed popover whose exit lasts `exit`
    /// ([`crate::state::popover::MENU_EXIT`] and the others).
    pub fn new(exit: Duration, cx: &mut Context<Self>) -> Self {
        PopoverState {
            popover: Popover::new(exit),
            trigger: cx.focus_handle(),
            items: Vec::new(),
            trigger_bounds: Rc::new(std::cell::Cell::new(Bounds::default())),
        }
    }

    /// Open (taking input).
    pub fn is_open(&self) -> bool {
        self.popover.is_open()
    }

    /// Where it is at `now` (pickers: the composer's pickers draw their own
    /// panels from it, `view::pickers`).
    pub fn phase(&self, now: std::time::Instant) -> Phase {
        self.popover.phase(now)
    }

    /// Its exit time (pickers).
    pub fn exit_time(&self) -> Duration {
        self.popover.exit()
    }

    /// Where the trigger was last drawn ([`record_trigger`] keeps it).
    pub fn trigger_bounds(&self) -> Rc<std::cell::Cell<Bounds<Pixels>>> {
        self.trigger_bounds.clone()
    }

    /// The trigger's focus handle (give it to the trigger's [`Button`]).
    pub fn trigger(&self) -> FocusHandle {
        self.trigger.clone()
    }

    /// Item `index`'s focus handle.
    pub fn item(&mut self, index: usize, cx: &mut Context<Self>) -> FocusHandle {
        while self.items.len() <= index {
            self.items.push(cx.focus_handle());
        }
        self.items[index].clone()
    }

    /// Opens at once and focuses the first item.
    pub fn open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.popover.open();
        let first = self.item(0, cx);
        window.focus(&first, cx);
        cx.notify();
    }

    /// Starts the exit and puts focus back on the trigger; a popover already
    /// closing or closed does nothing more.
    pub fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.popover.close(cx.background_executor().now()) {
            return;
        }
        window.focus(&self.trigger, cx);
        let exit = self.popover.exit();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(exit).await;
            let _ = this.update(cx, |_, cx| cx.notify());
        })
        .detach();
        cx.notify();
    }

    // sidebar agent: Tab out of the find filter's menu closes it and leaves
    // focus where Tab took it (`FindFilter`'s `onFocusOut`).
    /// Starts the exit and leaves focus where it is.
    pub fn close_keeping_focus(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if !self.popover.close(cx.background_executor().now()) {
            return;
        }
        let exit = self.popover.exit();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(exit).await;
            let _ = this.update(cx, |_, cx| cx.notify());
        })
        .detach();
        cx.notify();
    }

    /// The trigger's press: opens a closed popover, closes an open one.
    pub fn toggle(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.popover.is_open() {
            self.close(window, cx);
        } else {
            self.open(window, cx);
        }
    }

    /// Moves focus among `count` items by `step`, wrapping round.
    pub fn step(&mut self, count: usize, step: Step, window: &mut Window, cx: &mut Context<Self>) {
        let current = self.items.iter().position(|h| h.is_focused(window));
        if let Some(next) = menu_step(current, count, step) {
            let handle = self.item(next, cx);
            window.focus(&handle, cx);
        }
    }
}

/// The trigger: `button` with the popover's focus, its expanded state, and
/// a press that opens or closes it, its bounds kept for the panel.
pub fn trigger(state: &Entity<PopoverState>, button: Button, cx: &App) -> AnyElement {
    let open = state.read(cx).is_open();
    let recorder = record_trigger(state, cx);
    let state = state.clone();
    div()
        .relative()
        .child(
            button
                .focus(state.read(cx).trigger())
                .expanded(open)
                .menu_open(open)
                .on_press(move |window, cx| state.update(cx, |s, cx| s.toggle(window, cx))),
        )
        .child(recorder)
        .into_any_element()
}

/// Keeps where a trigger is drawn: put it, absolute, inside the trigger
/// (every trigger does, so a press on it is not a press outside).
pub fn record_trigger(state: &Entity<PopoverState>, cx: &App) -> impl IntoElement {
    let cell = state.read(cx).trigger_bounds();
    canvas(move |b, _, _| cell.set(b), |_, _, _, _| {})
        .absolute()
        .top_0()
        .left_0()
        .size_full()
}

/// How a popover above its trigger enters (the composer's pickers).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Entry {
    /// `surface-in`, 170 ms (the model picker).
    Surface,
    /// `effort-menu-up-in`, 150 ms, from 4 px below (effort, resolution).
    Up,
}

impl Entry {
    fn duration(self) -> Duration {
        match self {
            Entry::Surface => Duration::from_millis(170),
            Entry::Up => Duration::from_millis(150),
        }
    }
}

/// Where the panel opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Placement {
    /// Under the trigger, right-aligned, 6 px off (`.tag-menu`).
    Below,
    /// Over the trigger, left-aligned, 6 px off.
    Above,
    /// Over the trigger, right-aligned, 8 px off (the composer's pickers,
    /// `bottom: calc(100% + 8px); right: 0`), entering as `Entry` says.
    AboveRight(Entry),
}

/// The popover's panel when shown, anchored to its trigger: put it inside
/// the same `div().relative()` as the trigger.
pub fn panel(
    state: &Entity<PopoverState>,
    placement: Placement,
    content: AnyElement,
    cx: &mut App,
) -> Option<AnyElement> {
    let phase = state.read(cx).popover.phase(cx.background_executor().now());
    if phase == Phase::Closed {
        return None;
    }
    let closing = matches!(phase, Phase::Closing(_));
    let exit = state.read(cx).popover.exit();
    let escape = state.clone();
    let outside = state.clone();
    let trigger = state.read(cx).trigger_bounds();
    let body = div()
        .id("popover-panel")
        .key_context("Popover")
        .occlude()
        .on_action(move |_: &Dismiss, window, cx| {
            escape.update(cx, |s, cx| s.close(window, cx));
        })
        .on_mouse_down_out(move |event, window, cx| {
            // A press on the trigger is its own: its click closes it.
            if !trigger.get().contains(&event.position) {
                outside.update(cx, |s, cx| s.close(window, cx));
            }
        })
        .child(content);
    if let Placement::AboveRight(entry) = placement {
        let lift = match entry {
            Entry::Surface => 0.,
            Entry::Up => 4.,
        };
        let animated = if closing {
            body.with_animation("picker-out", Animation::new(exit), move |d, t| {
                d.opacity(1. - t).mb(px(-3. * t * (lift / 4.)))
            })
            .into_any_element()
        } else {
            body.with_animation(
                "picker-in",
                Animation::new(entry.duration()),
                move |d, t| {
                    let t = theme::ease(t);
                    d.opacity(t).mb(px(-lift * (1. - t)))
                },
            )
            .into_any_element()
        };
        return Some(
            div()
                .absolute()
                .top_0()
                .right_0()
                .size_0()
                .child(
                    deferred(
                        anchored()
                            .position_mode(AnchoredPositionMode::Local)
                            .anchor(Anchor::BottomRight)
                            .position(point(px(0.), px(-8.)))
                            .snap_to_window_with_margin(px(8.))
                            .child(animated),
                    )
                    .priority(2),
                )
                .into_any_element(),
        );
    }
    // Entry: 150 ms in, 4 px slide and fading in; exit: the picker's time,
    // fading out (`effort-menu-in` / `-out`).
    let animated = if closing {
        body.with_animation("popover-out", Animation::new(exit), move |d, t| {
            let t = theme::ease(t);
            d.opacity(1. - t).mt(px(-4. * t))
        })
        .into_any_element()
    } else {
        body.with_animation("popover-in", Animation::new(ENTER), |d, t| {
            let t = theme::ease(t);
            d.opacity(t).mt(px(-4. * (1. - t)))
        })
        .into_any_element()
    };
    let (corner, at, anchor) = match placement {
        Placement::Below => (Anchor::TopRight, point(px(0.), px(6.)), "bottom-right"),
        Placement::Above | Placement::AboveRight(_) => {
            (Anchor::BottomLeft, point(px(0.), px(-6.)), "top-left")
        }
    };
    let holder = div().absolute().size_0().map(|d| match anchor {
        "bottom-right" => d.bottom_0().right_0(),
        _ => d.top_0().left_0(),
    });
    Some(
        holder
            .child(
                deferred(
                    anchored()
                        .position_mode(AnchoredPositionMode::Local)
                        .anchor(corner)
                        .position(at)
                        .snap_to_window_with_margin(px(8.))
                        .child(animated),
                )
                .priority(2),
            )
            .into_any_element(),
    )
}

/// One item of a [`Menu`].
pub struct MenuItem {
    /// Its text, which is its name.
    pub label: SharedString,
    /// The chosen one (a check, and checked to assistive technology).
    pub selected: bool,
    /// What choosing it does (the menu then closes).
    pub on_choose: super::super::Press,
}

/// A menu (`.tag-menu`, the effort menu): `role="menu"` with
/// `menuitemradio` items, one checked.
pub struct Menu {
    state: Entity<PopoverState>,
    label: SharedString,
    items: Vec<MenuItem>,
}

impl Menu {
    /// A menu named `label` for the popover `state`.
    pub fn new(state: &Entity<PopoverState>, label: impl Into<SharedString>) -> Self {
        Menu {
            state: state.clone(),
            label: label.into(),
            items: Vec::new(),
        }
    }

    /// Adds an item.
    pub fn item(
        mut self,
        label: impl Into<SharedString>,
        selected: bool,
        on_choose: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        self.items.push(MenuItem {
            label: label.into(),
            selected,
            on_choose: Rc::new(on_choose),
        });
        self
    }

    /// The menu's element (give it to [`panel`]).
    pub fn render(self, cx: &mut App) -> AnyElement {
        let count = self.items.len();
        let fonts = fonts(cx);
        let state = self.state.clone();
        let mut list = div()
            .id("menu")
            // It clips its rows: one scrolled out is not showing (W2).
            .a11y_synthetic_children(|builder| {
                builder.parent_node().set_clips_children();
            })
            .role(Role::Menu)
            .aria_label(self.label)
            .aria_size_of_set(count)
            .key_context("Menu")
            .w(px(190.))
            .max_h(px(260.))
            .overflow_y_scroll()
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
        for (index, item) in self.items.into_iter().enumerate() {
            let focus = self.state.update(cx, |s, cx| s.item(index, cx));
            let choose = item.on_choose.clone();
            let choose_key = item.on_choose.clone();
            let closer = self.state.clone();
            let closer_key = self.state.clone();
            let row = ring(
                div()
                    .id(("menu-item", index))
                    .role(Role::MenuItemRadio)
                    .aria_label(item.label.clone())
                    .aria_toggled(if item.selected {
                        accesskit::Toggled::True
                    } else {
                        accesskit::Toggled::False
                    })
                    .aria_position_in_set(index)
                    .aria_size_of_set(count)
                    .track_focus(&focus)
                    .key_context("Part")
                    .on_click(move |_, window, cx| {
                        choose(window, cx);
                        closer.update(cx, |s, cx| s.close(window, cx));
                    })
                    .on_action(move |_: &Activate, window, cx| {
                        choose_key(window, cx);
                        closer_key.update(cx, |s, cx| s.close(window, cx));
                    }),
            )
            .min_h(px(30.))
            .px(px(8.))
            .py(px(5.))
            .rounded(px(radius::ROW))
            .flex()
            .items_center()
            .gap(px(9.))
            .cursor_pointer()
            .hover(|s| s.bg(colour::raised()).text_color(colour::bone()))
            .when(item.selected, |d| d.text_color(colour::bone()))
            .child(div().flex_1().min_w_0().truncate().child(item.label))
            .when(item.selected, |d| {
                d.child(super::super::icons::icon(
                    super::super::icons::Icon::Check,
                    14.,
                ))
            });
            list = list.child(
                theme::text(row, TextKind::MenuItem, &fonts)
                    .when(item.selected, |d| d.text_color(colour::bone())),
            );
        }
        list.into_any_element()
    }
}
