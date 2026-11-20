//! The page's shared controls (inventory `style-viewer-routes.md` 1.17 and
//! 1.18, `components.md` B.4, B.5, G.3 and N.1), each one call:
//!
//! | Control | Call | Page |
//! |---|---|---|
//! | raised button | `Button::new(id, label)` | `.btn` |
//! | primary button | `.kind(Kind::Primary)` | `.btn.gold` |
//! | danger button | `.kind(Kind::Danger)` | `.btn.danger` |
//! | ghost (composer trigger) | `.kind(Kind::Ghost)` | the model, effort and resolution triggers |
//! | header action | `.kind(Kind::Header)` | `.header-action` |
//! | quiet header action | `.kind(Kind::Quiet)` | `.header-action.quiet` |
//! | icon button | `Button::icon(id, Icon, label)` | `.icon-btn` |
//! | switch | [`Toggle`] | `Toggle` (`button.toggle`, `role="switch"`) |
//! | copy | [`CopyButton`] | `CopyButton` (`.copy-btn`) |
//! | confirm | [`ConfirmButton`] | `ConfirmButton` |
//! | tooltip | `.tip(text)` on a [`Button`], or [`tip`] on any element with a role (its description); [`pointer_tip`] on what has no node | `[data-tip]` |
//! | anchored popover, menu | [`popover`] | `usePopover` and its pickers |
//! | selectable line (a failure where a list's items would be) | [`selectable::selectable`] | the page's text, which a browser lets one select |
//!
//! Every control is a focus stop with the crate's focus ring
//! ([`super::ring`]), activates on Enter and Space (the `Part` key context)
//! and on a click or an assistive technology's press, and names itself:
//! an icon button's label is its accessible name, never the icon.
//!
//! What GPUI cannot draw, and so differs from the page: an element has no
//! transform, so a pressed control shows `--control-pressed` (or
//! `--primary-pressed`) with its face inset half a pixel instead of
//! `scale(0.97)`; hover colours change at once instead of over their
//! 110 to 160 ms transitions; the switch's thumb moves at once instead of
//! over 150 ms. The popover's and the tooltip's entries and exits are
//! animated as the page's are.

pub mod popover;
// chat-fix: selectable failure lines.
pub mod selectable;

use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::icons::{self, Icon};
use super::theme::{self, TextKind, colour, radius, shadow};
use super::{Activate, Press, ring};

/// What a switch does with its new value.
pub type Change = Rc<dyn Fn(bool, &mut Window, &mut App)>;
/// Told what was copied.
pub type Copied = Rc<dyn Fn(&str, &mut App)>;

/// A button's look.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// `.btn`: the raised control, `--control-face`.
    Raised,
    /// `.btn.gold`: `--bone` with `--primary-face`.
    Primary,
    /// `.btn.danger`: `--redstone`.
    Danger,
    /// The composer's ghost triggers: transparent, no shadow.
    Ghost,
    /// `.header-action`: the pane header's primary action.
    Header,
    /// `.header-action.quiet`: transparent with a `--line` ring.
    Quiet,
    /// `.icon-btn`: 32 by 32, an icon alone (its label is its name).
    Icon,
    // bench agent: a vote (`.bench-vote`), picked in `--done`.
    /// `.bench-vote`: 24 px, 7 px radius, ui 500 12 px `--dim-text`; with
    /// `pressed`, `--done` on its 10% tint with a 45% ring.
    Vote,
    /// `.composer-toggle`: a 28 px pill, ui 12.5 px `--dim-text`; with
    /// `pressed` (`.on`), `--bone` inside a `--bevel-light` ring.
    Toggle,
    /// `.round-switch button`: 24 px high, at least 26 wide, ui 500 12 px
    /// `--dim-text`; with `pressed` (`.active`), `--bone` on `--raised` with
    /// `--control-face`.
    Segment,
    /// `.bench-status` as a button (a failed or stopped round's retry): 18
    /// px, 5 px radius, `--err-soft`; hover `--bone` on `--well`.
    Status,
    /// `.bench-open`: 26 px, 6 px radius, `--dim-text`; hover `--bone` on
    /// `--well`.
    Open,
    /// `.bench-vote.bench-stop`: 29 by 24, 7 px radius, `--dim-text`; hover
    /// `--err-text` on `--hover`.
    Stop,
    /// `.chat-header button.crumb`: the header's text, no padding or fill,
    /// `--ash` (its ring's border taken back from its sides).
    Crumb,
    /// `.user-block .mtool`: 27 px, 6 px radius, `--dim-text`; hover
    /// `--bone` on `--well`.
    Tool,
    /// `.side-filter-button`: 26 px, 6 px radius, `--dim-text`; hover (and
    /// open) `--bone` on `--hover`; with `pressed` (`.active`) `--bone`.
    Filter,
    // pane agent: the live world's controls.
    /// `.live-controls button`: 28 px, 7 px radius, the bar's `--ash`, a
    /// 14 px icon; hover `--line`; disabled at 0.4 opacity.
    Live,
}

/// A focus handle kept for an element id across frames.
pub fn focus_for(id: impl Into<ElementId>, window: &mut Window, cx: &mut App) -> FocusHandle {
    window
        .use_keyed_state(id, cx, |_, cx| cx.focus_handle())
        .read(cx)
        .clone()
}

/// A button (see the module's table).
#[derive(IntoElement)]
pub struct Button {
    id: ElementId,
    label: SharedString,
    kind: Kind,
    icon: Option<(Icon, f32)>,
    show_label: bool,
    disabled: bool,
    armed: bool,
    pressed: bool,
    expanded: Option<bool>,
    tip: Option<SharedString>,
    menu_open: bool,
    focus: Option<FocusHandle>,
    on_press: Option<Press>,
    // bench agent: a toggle button's state, and a name other than its text.
    toggled: Option<bool>,
    name: Option<SharedString>,
    trailing: bool,
    // par-shell (for par-bench): a pointer-only tip on the label's text.
    label_tip: Option<SharedString>,
}

impl Button {
    /// A raised button showing `label`, which is also its name.
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Button {
            id: id.into(),
            label: label.into(),
            kind: Kind::Raised,
            icon: None,
            show_label: true,
            disabled: false,
            armed: false,
            pressed: false,
            expanded: None,
            tip: None,
            menu_open: false,
            focus: None,
            on_press: None,
            toggled: None,
            name: None,
            trailing: false,
            label_tip: None,
        }
    }

    /// An icon button (`.icon-btn`, 32 by 32, the icon at 18 px): `label` is
    /// its accessible name, as the page's `aria-label`.
    pub fn icon(id: impl Into<ElementId>, icon: Icon, label: impl Into<SharedString>) -> Self {
        Button {
            kind: Kind::Icon,
            icon: Some((icon, 18.)),
            show_label: false,
            ..Button::new(id, label)
        }
    }

    // pane agent
    /// A live world's control (`.live-controls button`): its icon alone.
    pub fn live(id: impl Into<ElementId>, icon: Icon, label: impl Into<SharedString>) -> Self {
        Button {
            kind: Kind::Live,
            icon: Some((icon, 14.)),
            show_label: false,
            ..Button::new(id, label)
        }
    }

    /// Its look.
    pub fn kind(mut self, kind: Kind) -> Self {
        self.kind = kind;
        self
    }

    /// An icon before the label, at `size` pixels.
    pub fn with_icon(mut self, icon: Icon, size: f32) -> Self {
        self.icon = Some((icon, size));
        self
    }

    /// Draws the icon only (the label stays its name).
    pub fn icon_only(mut self) -> Self {
        self.show_label = false;
        self
    }

    /// Disabled: not a focus stop, takes no press, reported disabled.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Armed (`.armed`: a quiet action asking for its confirmation).
    pub fn armed(mut self, armed: bool) -> Self {
        self.armed = armed;
        self
    }

    /// Pressed (a toggle button whose panel is open: `.share-button.active`).
    pub fn pressed(mut self, pressed: bool) -> Self {
        self.pressed = pressed;
        self
    }

    /// A popover trigger: reports expanded or collapsed and takes the
    /// Expand and Collapse actions as presses.
    pub fn expanded(mut self, expanded: bool) -> Self {
        self.expanded = Some(expanded);
        self
    }

    /// A tooltip (`data-tip`): on pointer hover after 500 ms, above the
    /// button and right-aligned; also its accessible description.
    pub fn tip(mut self, tip: impl Into<SharedString>) -> Self {
        self.tip = Some(tip.into());
        self
    }

    // par-shell (for par-bench: a bench chip's model id over its name).
    /// A tip on the label's text alone, by the pointer and describing no
    /// node (the page's `title` on an inner span): over the label it shows
    /// instead of the button's own [`Button::tip`], which shows over the
    /// rest of the button as before. The name and description do not
    /// change.
    pub fn label_tip(mut self, tip: impl Into<SharedString>) -> Self {
        self.label_tip = Some(tip.into());
        self
    }

    /// A menu inside the control is open: the tooltip is held back.
    pub fn menu_open(mut self, open: bool) -> Self {
        self.menu_open = open;
        self
    }

    /// Uses this focus handle (a control another part focuses, a popover's
    /// trigger) instead of one kept for its id.
    pub fn focus(mut self, focus: FocusHandle) -> Self {
        self.focus = Some(focus);
        self
    }

    // bench agent: toggle buttons and a name that is not the text shown.
    /// Shows its text only when `show` (an icon button that gains a word,
    /// the vote's `Best`).
    pub fn icon_only_unless(mut self, show: bool) -> Self {
        self.show_label = show;
        self
    }

    /// Draws its icon after its text (a menu trigger's chevron).
    pub fn icon_after(mut self) -> Self {
        self.trailing = true;
        self
    }

    /// A toggle button (`aria-pressed`): reported pressed or not.
    pub fn toggled(mut self, on: bool) -> Self {
        self.toggled = Some(on);
        self
    }

    /// Its accessible name when it differs from the text it shows (`Best`
    /// named `Take back the pick of Build A`).
    pub fn named(mut self, name: impl Into<SharedString>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// What a press does.
    pub fn on_press(mut self, press: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_press = Some(Rc::new(press));
        self
    }
}

impl RenderOnce for Button {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        // par-shell: with a label tip, the button's own tip state is made
        // here, so the label's tip can hold it back.
        let outer_tip = (self.label_tip.is_some() && self.tip.is_some()).then(|| {
            window.use_keyed_state(
                ElementId::NamedChild(std::sync::Arc::new(self.id.clone()), "outer-tip".into()),
                cx,
                |_, _| TipState::default(),
            )
        });
        let focus = self
            .focus
            .clone()
            .unwrap_or_else(|| focus_for(self.id.clone(), window, cx))
            .tab_stop(!self.disabled);
        let fonts = fonts(cx);
        let group: SharedString = format!("button-{:?}", self.id).into();
        let kind = self.kind;
        let disabled = self.disabled;
        let text_kind = match kind {
            Kind::Header | Kind::Quiet => TextKind::HeaderAction,
            Kind::Ghost | Kind::Toggle => TextKind::PickerTrigger,
            Kind::Crumb => TextKind::Crumb,
            _ => TextKind::Button,
        };
        // The face's corners follow the control's.
        let corner = match kind {
            Kind::Vote => px(7.),
            Kind::Toggle => px(14.),
            Kind::Segment | Kind::Open | Kind::Tool | Kind::Filter => px(6.),
            Kind::Status => px(5.),
            Kind::Stop | Kind::Live => px(7.),
            _ => px(radius::SM),
        };
        let (fill, text_colour, face, pressed_face) = look(kind, self.armed, self.pressed);
        let element = ring(
            div()
                .id(self.id.clone())
                .group(group.clone())
                .role(Role::Button)
                .aria_label(self.name.clone().unwrap_or_else(|| self.label.clone()))
                .when_some(self.toggled, |d, on| {
                    d.aria_toggled(if on {
                        accesskit::Toggled::True
                    } else {
                        accesskit::Toggled::False
                    })
                })
                .when_some(self.tip.clone(), |d, tip| d.aria_description(tip))
                .when_some(self.expanded, |d, expanded| d.aria_expanded(expanded))
                .when(disabled, |d| {
                    d.a11y_synthetic_children(|builder| builder.parent_node().set_disabled())
                })
                .track_focus(&focus)
                .key_context("Part"),
        )
        .relative()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .gap(px(match kind {
            Kind::Header | Kind::Quiet => 7.,
            Kind::Ghost | Kind::Vote => 5.,
            _ => 6.,
        }))
        .map(|d| match kind {
            Kind::Icon => d.size(px(32.)).rounded(px(radius::SM)),
            Kind::Header | Kind::Quiet => d.h(px(30.)).px(px(12.)).rounded(px(radius::SM)),
            Kind::Vote => d.h(px(24.)).px(px(8.)).rounded(corner),
            Kind::Toggle => d.h(px(28.)).px(px(10.)).rounded(corner),
            Kind::Segment => d.h(px(24.)).min_w(px(26.)).px(px(6.)).rounded(corner),
            Kind::Status => d.size(px(18.)).rounded(corner),
            Kind::Open => d.size(px(26.)).rounded(corner),
            Kind::Tool => d.size(px(27.)).rounded(corner),
            Kind::Filter => d.size(px(26.)).rounded(corner),
            Kind::Live => d.size(px(28.)).rounded(corner),
            Kind::Stop => d.w(px(29.)).h(px(24.)).rounded(corner),
            Kind::Crumb => d.mx(px(-2.)).rounded(corner),
            Kind::Ghost => d
                .min_h(px(32.))
                .px(px(7.))
                .py(px(4.))
                .rounded(px(radius::SM)),
            _ => d
                .min_h(px(32.))
                .px(px(12.))
                .py(px(6.))
                .rounded(px(radius::SM)),
        });
        let element = theme::text(element, text_kind, &fonts)
            .text_color(text_colour)
            .map(|d| match kind {
                Kind::Vote | Kind::Segment => d.text_size(px(12.)),
                Kind::Toggle => d.font_weight(FontWeight::NORMAL),
                _ => d,
            });
        // The face: the fill and the shadow ring, a layer of its own so a
        // press can inset it (GPUI has no scale transform).
        let face_layer = div()
            .id("face")
            .absolute()
            .inset_0()
            .rounded(corner)
            .bg(fill)
            .shadow(face)
            .when(!disabled, |d| {
                d.group_active(group.clone(), move |s| {
                    s.top(px(0.5))
                        .bottom(px(0.5))
                        .left(px(0.5))
                        .right(px(0.5))
                        .shadow(pressed_face)
                })
            });
        let hover = hover_look(kind, self.armed);
        let element = element
            .child(face_layer)
            .when(disabled, |d| {
                let (fill, text) = disabled_look(kind);
                d.opacity(if kind == Kind::Live { 0.4 } else { 0.64 })
                    .text_color(text)
                    .child(div().absolute().inset_0().rounded(corner).bg(fill))
            })
            .when(!disabled, |d| {
                d.cursor_pointer().hover(move |s| match hover {
                    Hover::Fill(fill, text) => s.bg(fill).text_color(text),
                    Hover::Opacity(o) => s.opacity(o),
                })
            })
            .when_some(self.icon.filter(|_| !self.trailing), |d, (icon, size)| {
                d.child(div().relative().child(icons::icon(icon, size)))
            })
            .when(self.show_label, |d| {
                match (self.label_tip.clone(), outer_tip.clone()) {
                    // par-shell: the label's own tip holds the button's back
                    // while the pointer is over the label.
                    (Some(label_tip), outer) => d.child(TipHost {
                        element: div()
                            .id(ElementId::NamedChild(
                                std::sync::Arc::new(self.id.clone()),
                                "label".into(),
                            ))
                            .relative()
                            .child(self.label.clone()),
                        id: ElementId::NamedChild(
                            std::sync::Arc::new(self.id.clone()),
                            "label-tip".into(),
                        ),
                        text: label_tip,
                        describes: false,
                        by_pointer: true,
                        holds: outer,
                        own: None,
                    }),
                    (None, _) => d.child(div().relative().child(self.label.clone())),
                }
            })
            .when_some(self.icon.filter(|_| self.trailing), |d, (icon, size)| {
                d.child(div().relative().child(icons::icon(icon, size)))
            });
        let press = self.on_press.clone();
        let press_key = self.on_press.clone();
        let element = element
            .when_some(press.filter(|_| !disabled), |d, press| {
                // settings agent: an assistive technology's press runs the
                // button itself (GPUI's own falls back to a pointer click
                // at the element's centre, which misses a button scrolled
                // out of its pane and can hit whatever is drawn there).
                let a11y = press.clone();
                d.on_click(move |_, window, cx| press(window, cx))
                    .on_a11y_action(accesskit::Action::Click, move |_, window, cx| {
                        a11y(window, cx)
                    })
            })
            .when_some(press_key.filter(|_| !disabled), |d, press| {
                d.on_action(move |_: &Activate, window, cx| press(window, cx))
            })
            .when_some(
                self.on_press
                    .clone()
                    .filter(|_| self.expanded.is_some() && !disabled),
                |d, press| {
                    let collapse = press.clone();
                    let expanded = self.expanded == Some(true);
                    d.on_a11y_action(accesskit::Action::Expand, move |_, window, cx| {
                        if !expanded {
                            press(window, cx)
                        }
                    })
                    .on_a11y_action(
                        accesskit::Action::Collapse,
                        move |_, window, cx| {
                            if expanded {
                                collapse(window, cx)
                            }
                        },
                    )
                },
            );
        match self.tip.filter(|_| !self.menu_open) {
            Some(text) => TipHost {
                element,
                id: self.id,
                text,
                describes: true,
                by_pointer: false,
                holds: None,
                own: outer_tip,
            }
            .into_any_element(),
            None => element.into_any_element(),
        }
    }
}

enum Hover {
    Fill(Hsla, Hsla),
    Opacity(f32),
}

/// A kind's fill, text colour, face and pressed face.
fn look(kind: Kind, armed: bool, pressed: bool) -> (Hsla, Hsla, Vec<BoxShadow>, Vec<BoxShadow>) {
    match kind {
        Kind::Raised => (
            colour::raised(),
            colour::bone(),
            shadow::control_face(),
            shadow::control_pressed(),
        ),
        Kind::Primary => (
            colour::bone(),
            colour::bedrock(),
            shadow::primary_face(),
            shadow::primary_pressed(),
        ),
        Kind::Danger => (
            colour::redstone(),
            colour::white(),
            shadow::control_face(),
            shadow::control_pressed(),
        ),
        Kind::Header => (
            colour::bone(),
            colour::bedrock(),
            shadow::primary_face(),
            shadow::primary_pressed(),
        ),
        Kind::Quiet if armed => (
            colour::none(),
            colour::bone(),
            shadow::bevel_ring(),
            shadow::bevel_ring(),
        ),
        Kind::Quiet if pressed => (
            colour::hover(),
            colour::bone(),
            shadow::line_ring(),
            shadow::line_ring(),
        ),
        Kind::Quiet => (
            colour::none(),
            colour::ash(),
            shadow::line_ring(),
            shadow::line_ring(),
        ),
        // bench agent: a pressed ghost toggle (the vote's pick, the round
        // on screen, Blind on) shows as held.
        Kind::Vote if pressed => (
            colour::done_fill(),
            colour::done(),
            shadow::done_ring(),
            shadow::done_ring(),
        ),
        Kind::Vote => (colour::none(), colour::dim(), Vec::new(), Vec::new()),
        Kind::Toggle if pressed => (
            colour::none(),
            colour::bone(),
            shadow::bevel_ring(),
            shadow::bevel_ring(),
        ),
        Kind::Toggle => (colour::none(), colour::dim(), Vec::new(), Vec::new()),
        Kind::Segment if pressed => (
            colour::raised(),
            colour::bone(),
            shadow::control_face(),
            shadow::control_face(),
        ),
        Kind::Segment => (colour::none(), colour::dim(), Vec::new(), Vec::new()),
        Kind::Status => (colour::none(), colour::err_soft(), Vec::new(), Vec::new()),
        Kind::Open | Kind::Stop | Kind::Tool => {
            (colour::none(), colour::dim(), Vec::new(), Vec::new())
        }
        Kind::Crumb => (colour::none(), colour::ash(), Vec::new(), Vec::new()),
        Kind::Filter if pressed => (colour::none(), colour::bone(), Vec::new(), Vec::new()),
        Kind::Filter => (colour::none(), colour::dim(), Vec::new(), Vec::new()),
        Kind::Ghost if pressed => (colour::hover(), colour::bone(), Vec::new(), Vec::new()),
        Kind::Ghost => (colour::none(), colour::ash(), Vec::new(), Vec::new()),
        Kind::Icon => (colour::none(), colour::ash(), Vec::new(), Vec::new()),
        Kind::Live => (colour::none(), colour::ash(), Vec::new(), Vec::new()),
    }
}

fn hover_look(kind: Kind, armed: bool) -> Hover {
    match kind {
        Kind::Raised => Hover::Fill(colour::raised_hover(), colour::bone()),
        Kind::Primary => Hover::Fill(colour::primary_hover(), colour::bedrock()),
        Kind::Danger => Hover::Opacity(0.88),
        Kind::Header => Hover::Opacity(0.88),
        Kind::Quiet if armed => Hover::Fill(colour::none(), colour::bone()),
        Kind::Quiet => Hover::Fill(colour::hover(), colour::bone()),
        Kind::Ghost => Hover::Fill(colour::hover(), colour::bone()),
        Kind::Icon => Hover::Fill(colour::raised(), colour::bone()),
        Kind::Vote | Kind::Toggle => Hover::Fill(colour::hover(), colour::bone()),
        Kind::Segment => Hover::Fill(colour::none(), colour::bone()),
        Kind::Status | Kind::Open | Kind::Tool => Hover::Fill(colour::well(), colour::bone()),
        Kind::Stop => Hover::Fill(colour::hover(), colour::err()),
        Kind::Crumb => Hover::Fill(colour::none(), colour::bone()),
        Kind::Filter => Hover::Fill(colour::hover(), colour::bone()),
        Kind::Live => Hover::Fill(colour::line(), colour::ash()),
    }
}

/// A disabled control's fill and text: `--raised` and `--dim-text` (the
/// quiet action stays transparent).
fn disabled_look(kind: Kind) -> (Hsla, Hsla) {
    match kind {
        Kind::Quiet
        | Kind::Ghost
        | Kind::Icon
        | Kind::Vote
        | Kind::Toggle
        | Kind::Segment
        | Kind::Status
        | Kind::Open
        | Kind::Stop
        | Kind::Crumb
        | Kind::Tool
        | Kind::Filter => (colour::none(), colour::dim()),
        Kind::Live => (colour::none(), colour::ash()),
        _ => (colour::raised(), colour::dim()),
    }
}

/// The families the theme resolved (gpui-kit's theme holds them).
pub fn fonts(cx: &App) -> theme::Fonts {
    let kit = cx.global::<gpui_kit::component::Theme>();
    theme::Fonts {
        ui: kit.font_family.clone(),
        data: kit.mono_font_family.clone(),
    }
}

/// A switch (`Toggle`, `web/app.js:628-637`): `role="switch"`, checked when
/// on, named by `label` (it shows no text). A click flips it and goes no
/// further (a switch in a clickable row does not press the row).
#[derive(IntoElement)]
pub struct Toggle {
    id: ElementId,
    label: SharedString,
    on: bool,
    on_change: Change,
}

impl Toggle {
    /// A switch showing `on`; `on_change` gets the new value.
    pub fn new(
        id: impl Into<ElementId>,
        label: impl Into<SharedString>,
        on: bool,
        on_change: impl Fn(bool, &mut Window, &mut App) + 'static,
    ) -> Self {
        Toggle {
            id: id.into(),
            label: label.into(),
            on,
            on_change: Rc::new(on_change),
        }
    }
}

impl RenderOnce for Toggle {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let focus = focus_for(self.id.clone(), window, cx).tab_stop(true);
        let on = self.on;
        let group: SharedString = format!("toggle-{:?}", self.id).into();
        let change = self.on_change.clone();
        let change_key = self.on_change.clone();
        // settings agent: an assistive technology's press flips it itself.
        let change_a11y = self.on_change.clone();
        ring(
            div()
                .id(self.id.clone())
                .group(group.clone())
                .role(Role::Switch)
                .aria_label(self.label.clone())
                .aria_toggled(if on {
                    accesskit::Toggled::True
                } else {
                    accesskit::Toggled::False
                })
                .track_focus(&focus)
                .key_context("Part")
                .on_click(move |_, window, cx| {
                    cx.stop_propagation();
                    change(!on, window, cx)
                })
                .on_action(move |_: &Activate, window, cx| change_key(!on, window, cx))
                .on_a11y_action(accesskit::Action::Click, move |_, window, cx| {
                    change_a11y(!on, window, cx)
                }),
        )
        // The ring's 2 px border sits outside the 34 by 20 track.
        .w(px(34. + 4.))
        .h(px(20. + 4.))
        .flex_none()
        .rounded_full()
        .cursor_pointer()
        .child(
            div()
                .size_full()
                .p(px(2.))
                .rounded_full()
                .bg(if on {
                    colour::gold()
                } else {
                    colour::toggle_track()
                })
                .flex()
                .child(
                    div()
                        .id("thumb")
                        .h(px(16.))
                        .w(px(16.))
                        .rounded_full()
                        .when(on, |d| d.ml(px(14.)))
                        .bg(if on {
                            colour::bedrock()
                        } else {
                            colour::bone()
                        })
                        .when(!on, |d| d.shadow(shadow::thumb()))
                        // While pressed the thumb widens to 19 px (on, it
                        // stretches left).
                        .group_active(group, move |s| {
                            let s = s.w(px(19.));
                            if on { s.ml(px(11.)) } else { s }
                        }),
                ),
        )
    }
}

/// How long a copy button shows its check (`CopyButton`, 1,200 ms).
pub const COPIED_FOR: Duration = Duration::from_millis(1200);

/// `CopyButton` (`web/app.js:639-649`): copies `text`; for 1,200 ms after
/// it shows the check and its tip reads `Copied`; its name stays `label`.
#[derive(IntoElement)]
pub struct CopyButton {
    id: ElementId,
    text: SharedString,
    label: SharedString,
    on_copied: Option<Copied>,
}

impl CopyButton {
    /// Copies `text`; `label` is its name and tip.
    pub fn new(
        id: impl Into<ElementId>,
        text: impl Into<SharedString>,
        label: impl Into<SharedString>,
    ) -> Self {
        CopyButton {
            id: id.into(),
            text: text.into(),
            label: label.into(),
            on_copied: None,
        }
    }

    /// Called after each copy (the dev binary logs it).
    pub fn on_copied(mut self, f: impl Fn(&str, &mut App) + 'static) -> Self {
        self.on_copied = Some(Rc::new(f));
        self
    }
}

impl RenderOnce for CopyButton {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let copied_at = window.use_keyed_state(
            ElementId::NamedChild(std::sync::Arc::new(self.id.clone()), "copied".into()),
            cx,
            |_, _| None::<Instant>,
        );
        let copied = copied_at
            .read(cx)
            .is_some_and(|at| cx.background_executor().now() < at + COPIED_FOR);
        let text = self.text.clone();
        let on_copied = self.on_copied.clone();
        let state = copied_at.clone();
        Button::icon(
            self.id,
            if copied { Icon::Check } else { Icon::Copy },
            self.label.clone(),
        )
        .with_icon(if copied { Icon::Check } else { Icon::Copy }, 13.)
        .tip(if copied {
            SharedString::from("Copied")
        } else {
            self.label
        })
        .kind(Kind::Icon)
        .on_press(move |_, cx| {
            cx.write_to_clipboard(ClipboardItem::new_string(text.to_string()));
            if let Some(f) = &on_copied {
                f(&text, cx);
            }
            state.update(cx, |at, cx| {
                *at = Some(cx.background_executor().now());
                cx.notify();
            });
            let state = state.clone();
            cx.spawn(async move |cx| {
                cx.background_executor().timer(COPIED_FOR).await;
                state.update(cx, |_, cx| cx.notify());
            })
            .detach();
        })
        .into_copy()
    }
}

impl Button {
    /// The copy button's box: 26 by 26, radius 6, `--dim-text`, hover
    /// `--bone` on `--hover` (`.copy-btn`).
    fn into_copy(self) -> CopyLook {
        CopyLook(self)
    }
}

/// A [`Button`] drawn as `.copy-btn`.
#[derive(IntoElement)]
struct CopyLook(Button);

impl RenderOnce for CopyLook {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        div()
            .size(px(26.))
            .flex()
            .items_center()
            .justify_center()
            .text_color(colour::dim())
            .child(self.0)
    }
}

/// `ConfirmButton` (`web/app.js:2825-2842`): a quiet action with an icon
/// that cannot be undone. The first press arms it for `subject` (it reads
/// `confirm_label`), a second within 4 s acts; it disarms after 4 s. Its
/// name is the label it shows.
#[derive(IntoElement)]
pub struct ConfirmButton {
    id: ElementId,
    subject: SharedString,
    icon: Icon,
    label: SharedString,
    confirm_label: SharedString,
    title: Option<SharedString>,
    disabled: bool,
    on_confirm: Press,
}

impl ConfirmButton {
    /// A confirm button for `subject`.
    pub fn new(
        id: impl Into<ElementId>,
        subject: impl Into<SharedString>,
        icon: Icon,
        label: impl Into<SharedString>,
        confirm_label: impl Into<SharedString>,
        on_confirm: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        ConfirmButton {
            id: id.into(),
            subject: subject.into(),
            icon,
            label: label.into(),
            confirm_label: confirm_label.into(),
            title: None,
            disabled: false,
            on_confirm: Rc::new(on_confirm),
        }
    }

    /// Its title (`title`, unchanged while armed): the tooltip.
    pub fn title(mut self, title: impl Into<SharedString>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Disabled.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

impl RenderOnce for ConfirmButton {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let state = window.use_keyed_state(
            ElementId::NamedChild(std::sync::Arc::new(self.id.clone()), "confirm".into()),
            cx,
            |_, _| crate::state::confirm::Confirm::default(),
        );
        let armed = state
            .read(cx)
            .armed(&self.subject, cx.background_executor().now());
        let subject = self.subject.to_string();
        let confirm = self.on_confirm.clone();
        let button = Button::new(
            self.id,
            if armed {
                self.confirm_label
            } else {
                self.label
            },
        )
        .kind(Kind::Quiet)
        .with_icon(self.icon, 14.)
        .armed(armed)
        .disabled(self.disabled)
        .on_press(move |window, cx| {
            let now = cx.background_executor().now();
            let pressed = state.update(cx, |c, _| c.press(&subject, now));
            match pressed {
                crate::state::confirm::Press::Confirmed => confirm(window, cx),
                crate::state::confirm::Press::Armed => {
                    let state = state.clone();
                    cx.spawn(async move |cx| {
                        cx.background_executor()
                            .timer(crate::state::confirm::ARMED_FOR)
                            .await;
                        state.update(cx, |_, cx| cx.notify());
                    })
                    .detach();
                }
            }
            cx.refresh_windows();
        });
        match self.title {
            Some(title) => button.tip(title),
            None => button,
        }
    }
}

/// The tooltip's delay (`transition-delay: 500ms` on hover).
pub const TIP_DELAY: Duration = Duration::from_millis(500);
/// The tooltip's fade in (`opacity 120ms ease, transform 120ms ease`).
pub const TIP_FADE: Duration = Duration::from_millis(120);

/// Whether a tooltip shows: hovered since when, and shown.
#[derive(Default)]
pub struct TipState {
    hovered: bool,
    shown: bool,
    // par-shell: an inner tip (a button's label's) is under the pointer, so
    // this one is held back.
    held: bool,
    // pane agent: where the pointer last moved over the element (window
    // coordinates), for a tip shown by the pointer.
    at: Point<Pixels>,
}

/// Puts the tooltip `text` on `element` (the page's `data-tip`): it shows
/// on pointer hover only, after 500 ms, fading in over 120 ms; it hides at
/// once when the pointer leaves; it sits above the element, right-aligned
/// with it, 8 px off, at most 260 px wide. Keyboard focus never shows it
/// (the page has no `:focus` rule for tips).
///
/// The text is also `element`'s accessible description, so `element` is
/// the node it describes: it has a role (a button's own element, never a
/// box around it, whose description no reader would hear). An element
/// with no role panics here; an element the pointer alone uses takes
/// [`pointer_tip`].
pub fn tip(
    element: Stateful<Div>,
    id: impl Into<ElementId>,
    text: impl Into<SharedString>,
) -> impl IntoElement {
    let id: ElementId = id.into();
    let text: SharedString = text.into();
    assert!(
        element.a11y_role().is_some(),
        "the tip {text:?} ({id:?}) goes on an element with no role: put it on the node it describes"
    );
    TipHost {
        element,
        id,
        text,
        describes: true,
        by_pointer: false,
        holds: None,
        own: None,
    }
}

// chat-fix-2: a tip on what has no node (the sidebar's edge, the divider;
// the meter, whose name and value say what its tip says).
/// A tooltip as [`tip`], for the pointer alone: it describes no node.
pub fn pointer_tip(
    element: Stateful<Div>,
    id: impl Into<ElementId>,
    text: impl Into<SharedString>,
) -> impl IntoElement {
    TipHost {
        element,
        id: id.into(),
        text: text.into(),
        describes: false,
        by_pointer: false,
        holds: None,
        own: None,
    }
}

// pane agent (first real look): the divider's tip by the pointer, as a
// browser shows the page's `title="Drag to resize"`, not at the top of a
// window-tall element.
/// A tooltip as [`pointer_tip`], shown below and right of where the
/// pointer is over the element.
pub fn cursor_tip(
    element: Stateful<Div>,
    id: impl Into<ElementId>,
    text: impl Into<SharedString>,
) -> impl IntoElement {
    TipHost {
        element,
        id: id.into(),
        text: text.into(),
        describes: false,
        by_pointer: true,
        holds: None,
        own: None,
    }
}

#[derive(IntoElement)]
struct TipHost {
    element: Stateful<Div>,
    id: ElementId,
    text: SharedString,
    describes: bool,
    by_pointer: bool,
    // par-shell: the outer tip this one holds back while hovered, and a
    // state made by the caller (a button whose label holds it back).
    holds: Option<Entity<TipState>>,
    own: Option<Entity<TipState>>,
}

impl RenderOnce for TipHost {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let state = match self.own.clone() {
            Some(own) => own,
            None => window.use_keyed_state(
                ElementId::NamedChild(std::sync::Arc::new(self.id.clone()), "tip".into()),
                cx,
                |_, _| TipState::default(),
            ),
        };
        let shown = state.read(cx).shown && !state.read(cx).held;
        let holds = self.holds.clone();
        let at = state.read(cx).at;
        let fonts = fonts(cx);
        let hover_state = state.clone();
        let move_state = state.clone();
        let text = self.text.clone();
        let by_pointer = self.by_pointer;
        self.element
            .when(self.describes, |d| d.aria_description(text))
            .when(by_pointer, |d| {
                d.on_mouse_move(move |event: &MouseMoveEvent, _, cx| {
                    move_state.update(cx, |s, _| s.at = event.position);
                })
            })
            .on_hover(move |hovered, _, cx| {
                let hovered = *hovered;
                if let Some(outer) = &holds {
                    outer.update(cx, |s, cx| {
                        s.held = hovered;
                        cx.notify();
                    });
                }
                hover_state.update(cx, |s, cx| {
                    s.hovered = hovered;
                    if !hovered && s.shown {
                        s.shown = false;
                        cx.notify();
                    }
                });
                if hovered {
                    let state = hover_state.clone();
                    cx.spawn(async move |cx| {
                        cx.background_executor().timer(TIP_DELAY).await;
                        state.update(cx, |s, cx| {
                            if s.hovered && !s.shown {
                                s.shown = true;
                                cx.notify();
                            }
                        });
                    })
                    .detach();
                }
            })
            .when(shown, |d| {
                let bubble = theme::text(
                    div()
                        .id("tip-bubble")
                        .role(Role::Tooltip)
                        .aria_label(self.text.clone())
                        .max_w(px(260.))
                        .px(px(9.))
                        .py(px(5.))
                        .rounded(px(radius::TIP))
                        .bg(colour::popover())
                        .shadow(shadow::tooltip()),
                    TextKind::Tooltip,
                    &fonts,
                )
                .child(self.text.clone());
                if by_pointer {
                    // Below and right of the pointer, as a browser's title.
                    return d.child(
                        deferred(
                            anchored()
                                .position_mode(AnchoredPositionMode::Window)
                                .anchor(Anchor::TopLeft)
                                .position(point(at.x + px(12.), at.y + px(20.)))
                                .snap_to_window()
                                .child(bubble),
                        )
                        .priority(3),
                    );
                }
                d.child(
                    // A zero-size anchor at the element's top right corner;
                    // the bubble's bottom right sits 8 px above it.
                    div().absolute().top_0().right_0().size_0().child(
                        deferred(
                            anchored()
                                .position_mode(AnchoredPositionMode::Local)
                                .anchor(Anchor::BottomRight)
                                .position(point(px(0.), px(-8.)))
                                .snap_to_window()
                                .child(bubble.with_animation(
                                    ElementId::NamedChild(
                                        std::sync::Arc::new(self.id.clone()),
                                        "tip-in".into(),
                                    ),
                                    Animation::new(TIP_FADE),
                                    |d, t| d.opacity(t).mt(px(2. * (1. - t))),
                                )),
                        )
                        .priority(3),
                    ),
                )
            })
    }
}
