//! A Settings section and row (`SettingsSection`, `SettingsRow`,
//! `web/app.js:651-671`; `style-viewer-routes.md` 1.10 `.s-section`,
//! `.s-card`, `.s-row`), the panes' head, a value, a text field.

use gpui_kit::component::input::{Input, InputState};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::view::controls::fonts;
use crate::view::theme::{self, TextKind, colour, shadow};

thread_local! {
    static NARROW: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// The window is 980 px wide or less: rows stack their control under
/// their text (`.s-row` at 980 px and below). Set by the layer each frame.
pub fn set_narrow(narrow: bool) {
    NARROW.with(|n| n.set(narrow));
}

fn narrow() -> bool {
    NARROW.with(|n| n.get())
}

/// A section: its head (`h4` and an action at the right), then a card
/// holding the rows. A group named by its title.
pub fn section(
    id: impl Into<ElementId>,
    title: &str,
    action: Option<AnyElement>,
    rows: Vec<AnyElement>,
    cx: &App,
) -> AnyElement {
    let fonts = fonts(cx);
    div()
        .id(id.into())
        .role(Role::Group)
        .aria_label(title.to_string())
        .flex()
        .flex_col()
        .gap(px(10.))
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .px(px(2.))
                .child(
                    theme::text(
                        div()
                            .id("section-head")
                            .role(Role::Heading)
                            .aria_label(title.to_string())
                            .aria_level(4),
                        TextKind::SettingsSectionHead,
                        &fonts,
                    )
                    .text_color(theme::settings::section_head())
                    .child(title.to_string()),
                )
                .children(action),
        )
        .child(
            div()
                .rounded(px(12.))
                .bg(theme::settings::card())
                .shadow(theme::settings::card_ring())
                .flex()
                .flex_col()
                .children(rows),
        )
        .into_any_element()
}

/// A row's help: plain text, or text with a failure after it.
pub enum Help {
    /// None.
    None,
    /// Text.
    Text(String),
    /// Any element (a link line, a failure line).
    Element(AnyElement),
}

/// A row: its title and help on the left, its control on the right; rows
/// after the first draw a hairline above. `flash` draws the palette's
/// flash (`setting-flash`, 1.4 s) at `t` of its run.
pub struct Row {
    /// The element id.
    pub id: ElementId,
    /// The title, as the row's name.
    pub title: String,
    /// The help.
    pub help: Help,
    /// The control.
    pub control: Option<AnyElement>,
    /// The first row of its card.
    pub first: bool,
    /// The flash's progress, while it runs.
    pub flash: Option<f32>,
    /// The control's tip (`data-tip`), when it has one.
    pub tip: Option<&'static str>,
}

impl Row {
    /// A row titled `title`.
    pub fn new(id: impl Into<ElementId>, title: impl Into<String>) -> Row {
        Row {
            id: id.into(),
            title: title.into(),
            help: Help::None,
            control: None,
            first: false,
            flash: None,
            tip: None,
        }
    }

    /// Its help text.
    pub fn help(mut self, help: impl Into<String>) -> Row {
        self.help = Help::Text(help.into());
        self
    }

    /// Its help as an element.
    pub fn help_element(mut self, help: AnyElement) -> Row {
        self.help = Help::Element(help);
        self
    }

    /// Its control.
    pub fn control(mut self, control: impl IntoElement) -> Row {
        self.control = Some(control.into_any_element());
        self
    }

    /// The first row of its card.
    pub fn first(mut self, first: bool) -> Row {
        self.first = first;
        self
    }

    /// Its tip.
    pub fn tip(mut self, tip: &'static str) -> Row {
        self.tip = Some(tip);
        self
    }

    /// Flashing at `t`.
    pub fn flash(mut self, t: Option<f32>) -> Row {
        self.flash = t;
        self
    }

    /// The row.
    pub fn render(self, cx: &App) -> AnyElement {
        let fonts = fonts(cx);
        let help = match self.help {
            Help::None => None,
            Help::Text(text) => Some(
                theme::text(
                    div()
                        .id("help")
                        .role(Role::Label)
                        .aria_label(text.clone())
                        .aria_value(text.clone()),
                    TextKind::SettingsRowHelp,
                    &fonts,
                )
                .text_color(theme::settings::help())
                .max_w(px(62. * 6.4))
                .child(text)
                .into_any_element(),
            ),
            Help::Element(element) => Some(
                theme::text(div(), TextKind::SettingsRowHelp, &fonts)
                    .text_color(theme::settings::help())
                    .max_w(px(62. * 6.4))
                    .child(element)
                    .into_any_element(),
            ),
        };
        let stacked = narrow();
        let control = self.control.map(|c| {
            let holder = div()
                .id("control")
                .flex_none()
                .max_w(relative(if stacked { 1. } else { 0.6 }))
                .flex()
                .items_center()
                .map(|d| {
                    if stacked {
                        d.justify_start()
                    } else {
                        d.justify_end()
                    }
                })
                .child(c);
            holder.into_any_element()
        });
        // A group named by its title, so its title is read with the
        // control inside it; its help a label under it.
        let narrow = narrow();
        let row = div()
            .id(self.id)
            .role(Role::Group)
            .aria_label(self.title.clone())
            .relative()
            .min_h(px(56.))
            .px(px(16.))
            .py(px(12.))
            .flex()
            .map(|d| {
                if narrow {
                    d.flex_col().items_start().gap(px(10.))
                } else {
                    d.items_center().justify_between().gap(px(32.))
                }
            })
            .when(!self.first, |d| {
                d.border_t_1().border_color(theme::settings::hairline())
            })
            .when_some(self.flash, |d, t| {
                // 8% bone over the first 35%, then fading out.
                let strength = if t < 0.35 { 1. } else { (1. - t) / 0.65 };
                d.bg(theme::settings::flash().opacity(strength))
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(3.))
                    .child(
                        theme::text(div(), TextKind::SettingsRowTitle, &fonts)
                            .child(self.title.clone()),
                    )
                    .children(help),
            )
            .children(control);
        // The row's tip (`data-tip` on its control) goes on the row's own
        // node, the one it describes.
        match self.tip {
            Some(tip) => crate::view::controls::tip(row, "row-tip", tip).into_any_element(),
            None => row.into_any_element(),
        }
    }
}

/// A value at a row's end (`.s-value`), monospace when `mono`, selectable
/// as a label.
pub fn value(text: String, mono: bool, cx: &App) -> AnyElement {
    let fonts = fonts(cx);
    let element = theme::text(
        div()
            .id("value")
            .role(Role::Label)
            .aria_label(text.clone())
            .aria_value(text.clone()),
        TextKind::SettingsValue,
        &fonts,
    )
    .text_right();
    if mono {
        element
            .font_family(fonts.data.clone())
            .text_size(px(12.5))
            .child(text)
            .into_any_element()
    } else {
        element.child(text).into_any_element()
    }
}

/// A text field (`.s-input`, `.s-filter`): `width` wide, `height` high, on
/// `--raised` with the field face, the gold inset ring when focused (with
/// the glow for `.s-input`), the text monospace when `mono`.
#[allow(clippy::too_many_arguments)]
pub fn field(
    input: &Entity<InputState>,
    name: &str,
    width: f32,
    height: f32,
    mono: bool,
    glow: bool,
    window: &Window,
    cx: &App,
) -> Div {
    let fonts = fonts(cx);
    let focused = input.read(cx).focus_handle(cx).is_focused(window);
    // The input draws its own 11 px inset (`padding: 0 11px`).
    let text = Input::new(input)
        .aria_label(name.to_string())
        .appearance(false)
        .text_size(px(if mono { 12.5 } else { 13. }));
    div()
        .w(px(width))
        .max_w_full()
        .h(px(height))
        .flex()
        .items_center()
        .rounded(px(8.))
        .bg(colour::raised())
        .shadow(if focused {
            theme::settings::input_focus(glow)
        } else {
            shadow::field_face()
        })
        .font_family(if mono {
            fonts.data.clone()
        } else {
            fonts.ui.clone()
        })
        .text_size(px(if mono { 12.5 } else { 13. }))
        .text_color(colour::bone())
        .child(div().flex_1().min_w_0().child(text))
}

/// A line of text assistive technology reads (a label named and valued
/// by its text).
pub fn text_label(id: impl Into<ElementId>, text: impl Into<SharedString>) -> Stateful<Div> {
    let text: SharedString = text.into();
    div()
        .id(id.into())
        .role(Role::Label)
        .aria_label(text.clone())
        .aria_value(text.clone())
        .child(text)
}

/// The panes' head (`.pane-head`): its children, then room for the close
/// button.
pub fn pane_head(children: Vec<AnyElement>) -> AnyElement {
    div()
        .flex()
        .items_center()
        .gap(px(11.))
        .min_h(px(32.))
        .pr(px(40.))
        .children(children)
        .into_any_element()
}

/// A pane's title (`.pane-head h3`), a heading of level 3.
pub fn pane_title(title: &str, cx: &App) -> AnyElement {
    let fonts = fonts(cx);
    theme::text(
        div()
            .id("pane-title")
            .role(Role::Heading)
            .aria_label(title.to_string())
            .aria_level(3),
        TextKind::SettingsPaneTitle,
        &fonts,
    )
    .child(title.to_string())
    .into_any_element()
}

/// The working dots (`.working-dots`), still (as the composer's held line).
pub fn working_dots() -> AnyElement {
    div()
        .flex()
        .gap(px(3.))
        .children((0..3).map(|_| div().size(px(4.)).rounded_full().bg(colour::ash())))
        .into_any_element()
}
