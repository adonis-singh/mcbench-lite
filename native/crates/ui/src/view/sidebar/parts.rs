//! Pieces the sidebar's sections share: a row's small icon button
//! (`.conv-item .archive`, `.row-icon`), the resolution tag
//! (`ResolutionTag`), a row's title with its search snippet
//! (`.conv-text`), a section's fold title (`.side-title`), and the empty
//! line (`.side-empty`).

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::super::controls::{fonts, tip};
use super::super::icons::{self, Icon};
use super::super::theme::{self, TextKind, colour, radius};
use super::super::{Activate, Press, ring};
use crate::state::seam::snippet_parts;

/// When a row's icon button shows (the archive button only on the row's
/// hover or its own focus; an archived row's buttons always).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Shown {
    /// On the row's hover (`group`) or its own keyboard focus.
    OnHover,
    /// Always.
    Always,
}

/// A row's 24 by 24 icon button (`focus` says whether it is a Tab stop):
/// named `label`, its tip `title`; disabled
/// keeps its place and is hidden (`visibility: hidden`), with `why` as its
/// description; armed is `--err-text` on its tint.
#[allow(clippy::too_many_arguments)]
pub fn row_icon(
    id: impl Into<ElementId>,
    group: &str,
    focus: &FocusHandle,
    icon: Icon,
    label: impl Into<SharedString>,
    title: impl Into<SharedString>,
    disabled: Option<&str>,
    armed: bool,
    shown: Shown,
    press: Press,
) -> AnyElement {
    let id: ElementId = id.into();
    let label: SharedString = label.into();
    let title: SharedString = title.into();
    let press_key = press.clone();
    let group: SharedString = group.to_string().into();
    let why = disabled.map(|w| SharedString::from(w.to_string()));
    let element = ring(
        div()
            .id(id.clone())
            .role(Role::Button)
            .aria_label(label)
            .when_some(why.clone(), |d, why| {
                d.aria_description(why)
                    .a11y_synthetic_children(|builder| builder.parent_node().set_disabled())
            })
            .track_focus(&if why.is_some() {
                focus.clone().tab_stop(false)
            } else {
                focus.clone()
            })
            .key_context("Part"),
    )
    .flex_none()
    .size(px(24. + 4.))
    .rounded(px(radius::ICON))
    .flex()
    .items_center()
    .justify_center()
    .text_color(if armed { colour::err() } else { colour::dim() })
    .when(armed, |d| d.bg(colour::armed_fill()))
    .child(icons::icon(icon, 14.));
    let element = if why.is_some() {
        element.invisible()
    } else {
        element
            .cursor_pointer()
            .when(!armed, |d| {
                d.hover(|s| s.text_color(colour::bone()).bg(colour::bone_7()))
            })
            .when(shown == Shown::OnHover, |d| {
                d.opacity(0.)
                    .group_hover(group.clone(), |s| s.opacity(1.))
                    .focus_visible(|s| s.opacity(1.))
            })
            .on_click(move |_, window, cx| {
                cx.stop_propagation();
                press(window, cx)
            })
            .on_action(move |_: &Activate, window, cx| press_key(window, cx))
    };
    // A disabled button is hidden: no tip, and its reason is its
    // description.
    if why.is_some() {
        return element.into_any_element();
    }
    tip(
        element,
        ElementId::NamedChild(std::sync::Arc::new(id), "tip".into()),
        title,
    )
    .into_any_element()
}

/// `ResolutionTag`: nothing at Standard, else the short tag with its title.
pub fn resolution_tag(value: i64, cx: &App) -> Option<AnyElement> {
    let (short, _title) = crate::state::resolution::tag(value)?;
    let fonts = fonts(cx);
    Some(
        theme::text(div(), TextKind::ResolutionTag, &fonts)
            .flex_none()
            .px(px(5.))
            .rounded(px(5.))
            .shadow(theme::sidebar::tag_ring())
            .child(short)
            .into_any_element(),
    )
}

/// A row's title with its search snippet under it (the matches marked
/// `--bone`): `.conv-text`.
pub fn row_text(title: &str, snippet: Option<&str>, cx: &App) -> AnyElement {
    let fonts = fonts(cx);
    div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .gap(px(2.))
        .child(div().truncate().child(title.to_string()))
        .when_some(snippet, |d, snippet| {
            d.child(
                theme::text(div(), TextKind::RowSnippet, &fonts)
                    .truncate()
                    .child(marked(&crate::state::sidebar::sidebar_snippet(snippet))),
            )
        })
        .into_any_element()
}

/// A snippet's text with its matches drawn `--bone`.
pub fn marked(snippet: &str) -> StyledText {
    let mut text = String::new();
    let mut highlights = Vec::new();
    for (part, matched) in snippet_parts(snippet) {
        let start = text.len();
        text.push_str(&part);
        if matched {
            highlights.push((
                start..text.len(),
                HighlightStyle {
                    color: Some(colour::bone()),
                    ..Default::default()
                },
            ));
        }
    }
    StyledText::new(text).with_highlights(highlights)
}

/// A snippet's plain text (for a row's accessible name).
pub fn plain(snippet: &str) -> String {
    snippet_parts(&crate::state::sidebar::sidebar_snippet(snippet))
        .into_iter()
        .map(|(t, _)| t)
        .collect()
}

/// The line `.side-empty` draws (`No matches`, `Loading…`).
pub fn side_empty(text: impl Into<SharedString>, cx: &App) -> Div {
    let fonts = fonts(cx);
    theme::text(div(), TextKind::BenchMember, &fonts)
        .px(px(10.))
        .py(px(8.))
        .text_color(colour::dim())
        .child(text.into())
}

/// A `.side-empty` line a reader finds: a label named by its text.
pub fn side_note(id: impl Into<ElementId>, text: &str, cx: &App) -> Stateful<Div> {
    side_empty(text.to_string(), cx)
        .id(id.into())
        .role(Role::Label)
        .aria_label(text.to_string())
        // chat-fix-2: a label's name over AT-SPI is its value (AccessKit):
        // without it Orca's tree read `No matches` as a nameless label.
        .aria_value(text.to_string())
}

/// A fold's title button (`.side-title`): the chevron (down when open, as
/// the page turns it 90 degrees) and its text; named by its text, with its
/// expanded state.
pub fn side_title(
    id: impl Into<ElementId>,
    focus: &FocusHandle,
    text: impl Into<SharedString>,
    open: bool,
    press: Press,
    cx: &App,
) -> AnyElement {
    side_title_marked(id, focus, text, open, press, &[], cx)
}

// bench agent: the prompts fold's title carries its chosen tags' dots.
/// [`side_title`] with `marks` after its text (`.side-tags`): a 6 px dot
/// in each colour, 3 px apart, named `Filtered by tag`.
pub fn side_title_marked(
    id: impl Into<ElementId>,
    focus: &FocusHandle,
    text: impl Into<SharedString>,
    open: bool,
    press: Press,
    marks: &[Hsla],
    cx: &App,
) -> AnyElement {
    let fonts = fonts(cx);
    let text: SharedString = text.into();
    let press_key = press.clone();
    let expand = press.clone();
    let collapse = press.clone();
    theme::text(
        ring(
            div()
                .id(id.into())
                .role(Role::Button)
                .aria_label(text.clone())
                .aria_expanded(open)
                .track_focus(focus)
                .key_context("Part")
                .on_click(move |_, window, cx| press(window, cx))
                .on_action(move |_: &Activate, window, cx| press_key(window, cx))
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
        ),
        TextKind::SideTitle,
        &fonts,
    )
    .flex_none()
    .h(px(28. + 4.))
    .px(px(6.))
    .flex()
    .items_center()
    .gap(px(6.))
    .rounded(px(radius::ROW))
    .cursor_pointer()
    .hover(|s| s.text_color(colour::bone()))
    .child(icons::icon(
        if open {
            Icon::ChevronDown
        } else {
            Icon::ChevronRight
        },
        12.,
    ))
    .child(text)
    .when(!marks.is_empty(), |d| {
        d.child(
            div()
                .id("side-tags")
                .role(Role::Image)
                .aria_label("Filtered by tag")
                .flex()
                .items_center()
                .gap(px(3.))
                .children(
                    marks
                        .iter()
                        .map(|&c| div().size(px(6.)).rounded_full().bg(c)),
                ),
        )
    })
    .into_any_element()
}

/// A bench's fold (`.bench-caret`): 20 by 20, the chevron alone (down when
/// open), named `Show models` or `Hide models` with its expanded state; a
/// press folds without reaching the row.
pub fn caret(
    id: impl Into<ElementId>,
    focus: &FocusHandle,
    open: bool,
    press: Press,
) -> AnyElement {
    let press_key = press.clone();
    let expand = press.clone();
    let collapse = press.clone();
    ring(
        div()
            .id(id.into())
            .role(Role::Button)
            .aria_label(if open { "Hide models" } else { "Show models" })
            .aria_expanded(open)
            .track_focus(focus)
            .key_context("Part")
            .on_click(move |_, window, cx| {
                cx.stop_propagation();
                press(window, cx)
            })
            .on_action(move |_: &Activate, window, cx| press_key(window, cx))
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
    .flex_none()
    .size(px(20. + 4.))
    .rounded(px(5.))
    .flex()
    .items_center()
    .justify_center()
    .text_color(colour::dim())
    .cursor_pointer()
    .hover(|s| s.text_color(colour::bone()).bg(colour::bone_7()))
    .child(icons::icon(
        if open {
            Icon::ChevronDown
        } else {
            Icon::ChevronRight
        },
        13.,
    ))
    .into_any_element()
}
