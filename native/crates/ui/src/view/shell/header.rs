//! The chat column's pane header (`.chat-header`, inventory `app.md` 3.5):
//! from the left, when the sidebar is closed, `Open sidebar` and `New chat`
//! (or `New bench`); the place's title, which flexes and ellipsizes; then
//! the places for the place's controls, in the page's order: the round
//! switch, Reveal models, Fork to Build, Run bench, the model warnings,
//! Share. Each is added by its milestone in [`actions()`], one line per
//! control, beside the others.

use gpui_kit::*;

use super::Shell;
use crate::state::place::{self, Mode, Place};
use crate::view::controls::{Button, fonts};
use crate::view::icons::Icon;
use crate::view::theme::{self, TextKind, colour};

/// The header for the place on screen.
pub fn pane_header(shell: &Shell, window: &mut Window, cx: &mut Context<Shell>) -> AnyElement {
    let fonts = fonts(cx);
    let title = place::place_title(shell.place(), shell.lists.read(cx)).unwrap_or_default();
    // narrow agent: at 980 px and below `min-height: 49px; padding: 8px 12px`.
    let narrow = shell.is_narrow();
    let mut header = div()
        .flex_none()
        .min_h(px(if narrow { 49. } else { 52. }))
        .flex()
        .items_center()
        .gap(px(6.))
        .px(px(12.))
        .py(px(if narrow { 8. } else { 9. }))
        .border_b_1()
        .border_color(colour::line());
    // narrow agent: a narrow window has the bar instead.
    if !shell.layout().sidebar_open && !shell.is_narrow() {
        let this = cx.entity().downgrade();
        let other = cx.entity().downgrade();
        let (label, new_place) = match shell.mode() {
            Mode::Build => ("New chat", Place::NewChat { id: None }),
            Mode::Bench => ("New bench", Place::NewBench),
        };
        header = header
            .child(
                Button::icon("open-sidebar", Icon::Panel, "Open sidebar").on_press(
                    move |window, cx| {
                        let _ = this.update(cx, |s, cx| s.toggle_sidebar(window, cx));
                    },
                ),
            )
            .child(
                Button::icon("header-new", Icon::Compose, label).on_press(move |window, cx| {
                    let place = new_place.clone();
                    // par-chat: New chat's model check.
                    let _ = other.update(cx, |s, cx| s.new_here(place, window, cx));
                }),
            );
    }
    // bench agent: a member's and the library's breadcrumbs.
    let title = match crate::view::bench::header::crumbs(shell, cx) {
        Some(crumbs) => crumbs,
        None => theme::text(
            div().flex_1().min_w_0().truncate(),
            TextKind::PaneTitle,
            &fonts,
        )
        .child(title)
        .into_any_element(),
    };
    header
        .child(title)
        .children(actions(shell, window, cx))
        .into_any_element()
}

/// The place's controls at the header's right. None are built yet: the
/// share panel, the bench controls and the model warnings add theirs here.
pub fn actions(shell: &Shell, window: &mut Window, cx: &mut Context<Shell>) -> Vec<AnyElement> {
    // bench agent: the round switch, Reveal, Fork to Build, Run bench.
    let mut actions = crate::view::bench::header::actions(shell, cx);
    // par-chat: the model warnings, after Run bench and before Share.
    actions.extend(model_warning_element(shell, cx));
    // share agent: the Share button and its panel, last (`web/app.js` 6734).
    let panel = crate::view::share::installed(
        shell.options.core.clone(),
        shell.options.opener.clone(),
        shell.options.log.clone(),
        shell.announcer.clone(),
        cx,
    );
    actions.extend(crate::view::share::button(shell, panel, window, cx));
    actions
}

// par-chat (stage four's parity pass)
/// The header's model warning for the picker's model, as the page decides
/// it from the model's facts (`web/app.js` 6728-6733): `unknown model` when
/// no source describes it, `no tools` when one does and it cannot call
/// tools (`tools` false or null); none while the facts are not known.
pub fn model_warning(facts: Option<&crate::ModelFacts>) -> Option<(&'static str, &'static str)> {
    let facts = facts?;
    if !facts.known {
        return Some((UNKNOWN_MODEL, UNKNOWN_MODEL_TITLE));
    }
    (facts.tools != Some(true)).then_some((NO_TOOLS, NO_TOOLS_TITLE))
}

/// `unknown model`.
pub const UNKNOWN_MODEL: &str = "unknown model";
/// Its title.
pub const UNKNOWN_MODEL_TITLE: &str = "This model is not in the provider's catalog";
/// `no tools`.
pub const NO_TOOLS: &str = "no tools";
/// Its title.
pub const NO_TOOLS_TITLE: &str = "This model cannot call tools — the builder needs tool calling";

/// The warning (`.cap-warn`, a span with its title) in Build, for the
/// model the picker has now (the page's `modelCaps` follows the picker, not
/// the chat's stored model): its text is a label for assistive technology,
/// its title a pointer tip, as the page's `title`.
fn model_warning_element(shell: &Shell, cx: &mut Context<Shell>) -> Option<AnyElement> {
    if shell.mode() != Mode::Build {
        return None;
    }
    let pickers = shell.chat.read(cx).pickers();
    let choice = pickers.read(cx).choice().clone();
    if !choice.chosen() {
        return None;
    }
    let catalog = crate::view::pickers::catalog::Catalog::installed(cx);
    let (text, title) = model_warning(catalog.read(cx).facts(&choice.provider, &choice.model))?;
    let fonts = fonts(cx);
    let warn = theme::text(div(), TextKind::CapWarn, &fonts)
        .id("cap-warn")
        .role(Role::Label)
        .aria_label(text)
        .aria_value(text)
        .flex_none()
        .rounded(px(6.))
        .bg(colour::cap_warn_fill())
        .px(px(7.))
        .py(px(3.))
        .child(text);
    Some(crate::view::controls::pointer_tip(warn, "cap-warn-tip", title).into_any_element())
}
// end of par-chat's block
