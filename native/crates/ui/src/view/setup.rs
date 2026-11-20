//! The first start's screen (`SetupStatus`, `web/app.js:1184-1225`;
//! inventory `components.md` F.1, `app.md` 3.5 and 8.10,
//! `style-viewer-routes.md` 1.11): in Build on an empty transcript while
//! setup is not ready, or while a step needs the person (s4-needs). The heading, the bar and its count (a status the
//! reader hears change), `A step failed.` with `Try again`, the steps with
//! their marks, labels, values and notes (web addresses as links), and the
//! last line or `Connect an AI while this runs`.

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::controls::{Button, fonts};
use super::icons::{self, Icon};
use super::theme::{self, TextKind, colour};
use super::{Activate, Opener, Press, ring};
use crate::state::setup::{self, Part};
use mcbench_setup::Step;

/// What the screen does.
pub struct SetupActions {
    /// `Try again` (`Command::RetrySetup`).
    pub retry: Press,
    /// `Connect an AI while this runs` (Settings on Connect an AI).
    pub connect: Press,
    /// Opens a note's link.
    pub opener: Opener,
    /// Records a link's activation.
    pub log: Option<crate::ActionLog>,
    // settings agent: installing WSL.
    /// `Install WSL` (`Command::InstallWsl`).
    pub install_wsl: Press,
    /// `Restart now` (`Command::RestartComputer`).
    pub restart: Press,
}

/// The screen for `steps`; `no_ai` shows the connect link instead of the
/// last line.
pub fn setup_screen(
    steps: &[Step],
    no_ai: bool,
    actions: SetupActions,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let fonts = fonts(cx);
    let status = setup::setup_status(steps);
    let count = format!("{} of {} done", status.done, status.total);
    let fill = setup::fill(&status);
    let bar = div()
        .id("setup-bar")
        .role(Role::ProgressIndicator)
        .aria_label("Setup")
        .aria_numeric_value(status.done as f64)
        .aria_min_numeric_value(0.)
        .aria_max_numeric_value(status.total as f64)
        .aria_value(count.clone())
        .flex_1()
        .h(px(4.))
        .rounded(px(2.))
        .bg(theme::settings::bar_track())
        .child(
            div()
                .h_full()
                .rounded(px(2.))
                .bg(colour::ash())
                .w(relative(fill)),
        );
    let overall = div().flex().items_center().gap(px(10.)).child(bar).child(
        theme::text(
            div()
                .id("setup-count")
                .role(Role::Status)
                .aria_label(count.clone())
                .aria_value(count.clone()),
            TextKind::SettingsRowHelp,
            &fonts,
        )
        .text_color(colour::dim())
        .child(count),
    );
    let failed = status.failed.then(|| {
        let retry = actions.retry.clone();
        theme::text(
            div()
                .id("setup-failed")
                .role(Role::Alert)
                .aria_label("A step failed.")
                .flex()
                .items_center()
                .justify_center()
                .gap(px(10.)),
            TextKind::Error,
            &fonts,
        )
        .text_size(px(12.5))
        .child("A step failed.")
        .child(
            Button::new("setup-retry", "Try again").on_press(move |window, cx| retry(window, cx)),
        )
    });
    let list = steps_card(steps, &actions, window, cx);
    let last: AnyElement = if no_ai {
        let connect = actions.connect.clone();
        let focus = super::controls::focus_for("setup-connect", window, cx).tab_stop(true);
        let (click, key, a11y) = (connect.clone(), connect.clone(), connect);
        theme::text(
            ring(
                div()
                    .id("setup-connect")
                    .role(Role::Button)
                    .aria_label("Connect an AI while this runs")
                    .track_focus(&focus)
                    .key_context("Part")
                    .on_click(move |_, window, cx| click(window, cx))
                    .on_a11y_action(accesskit::Action::Click, move |_, window, cx| {
                        a11y(window, cx)
                    })
                    .on_action(move |_: &Activate, window, cx| key(window, cx)),
            )
            .self_center()
            .px(px(4.))
            .py(px(2.))
            .rounded(px(4.))
            .text_color(colour::ash())
            .underline()
            .cursor_pointer()
            .hover(|s| s.text_color(colour::bone())),
            TextKind::Button,
            &fonts,
        )
        .font_weight(FontWeight(400.))
        .child("Connect an AI while this runs")
        .into_any_element()
    } else {
        theme::text(
            div()
                .id("setup-last")
                .role(Role::Label)
                .aria_label("You can send your first build now. It starts when the app is ready.")
                .aria_value("You can send your first build now. It starts when the app is ready."),
            TextKind::EmptySub,
            &fonts,
        )
        .text_center()
        .child("You can send your first build now. It starts when the app is ready.")
        .into_any_element()
    };
    div()
        .flex_1()
        .flex()
        .items_center()
        .justify_center()
        .px(px(18.))
        .child(
            div()
                .id("setup")
                .role(Role::Region)
                .aria_label("Getting ready")
                .w(px(460.))
                .max_w_full()
                .py(px(24.))
                .flex()
                .flex_col()
                .gap(px(14.))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap(px(6.))
                        .child(
                            theme::text(
                                div()
                                    .id("setup-heading")
                                    .role(Role::Heading)
                                    .aria_label("Getting ready")
                                    .aria_level(2),
                                TextKind::SetupHeading,
                                &fonts,
                            )
                            .child("Getting ready"),
                        )
                        .child(
                            theme::text(
                                super::settings::text_label(
                                    "setup-sub",
                                    "The first start downloads and builds what the app needs. This happens once.",
                                ),
                                TextKind::EmptySub,
                                &fonts,
                            )
                            .text_center()
                            .line_height(relative(1.5))
                            .text_color(colour::ash()),
                        ),
                )
                .child(overall)
                .children(failed)
                .child(list)
                .child(last),
        )
        .into_any_element()
}

/// The steps' card (`.setup-steps`): each step's mark, label, value, note
/// (web addresses as links) and the one button its need names. The empty
/// Build chat of a ready app shows it alone with the steps that need the
/// person (s4-needs).
pub fn steps_card(
    steps: &[Step],
    actions: &SetupActions,
    window: &mut Window,
    cx: &mut App,
) -> Stateful<Div> {
    let fonts = fonts(cx);
    let mut list = div()
        .id("setup-steps")
        .role(Role::List)
        .aria_label("Setup steps")
        .aria_size_of_set(steps.len())
        .py(px(6.))
        .rounded(px(10.))
        .bg(colour::well())
        .shadow(theme::settings::inset_line())
        .flex()
        .flex_col();
    for (index, step) in steps.iter().enumerate() {
        let row = setup::step_row(step);
        let failed = row.state == "failed";
        // settings agent: the one button a step's need names. It and the
        // row share one focus, so focus stays as the row changes under it
        // (the installer running shows the working dots and no button).
        let need_focus = super::controls::focus_for(
            SharedString::from(format!("setup-needs-{}", row.id)),
            window,
            cx,
        );
        let need = setup::step_needs(step).map(|need| {
            let id = SharedString::from(format!("setup-need-{}", row.id));
            let (label, press) = match need {
                setup::StepNeeds::InstallWsl => (setup::INSTALL_WSL, actions.install_wsl.clone()),
                setup::StepNeeds::Restart => (setup::RESTART_NOW, actions.restart.clone()),
            };
            Button::new(id, label)
                .kind(super::controls::Kind::Primary)
                .focus(need_focus.clone())
                .on_press(move |window, cx| press(window, cx))
        });
        let mark: AnyElement = match row.state {
            "waiting" => div()
                .size(px(6.))
                .rounded_full()
                .border_1()
                .border_color(colour::dim())
                .into_any_element(),
            "running" => div()
                .flex()
                .gap(px(3.))
                .children((0..3).map(|_| div().size(px(4.)).rounded_full().bg(colour::ash())))
                .into_any_element(),
            "done" => icons::icon(Icon::Check, 13.).into_any_element(),
            "failed" => icons::icon(Icon::X, 13.).into_any_element(),
            _ => div()
                .w(px(8.))
                .h(px(1.5))
                .bg(colour::dim())
                .into_any_element(),
        };
        let label_colour = match row.state {
            "running" => colour::bone(),
            "done" => colour::dim(),
            "failed" => colour::err(),
            _ => colour::ash(),
        };
        let mut name = row.label.clone();
        if row.optional {
            name.push_str(", Optional");
        }
        if !row.value.is_empty() {
            name.push_str(&format!(", {}", row.value));
        }
        if let Some(note) = &row.note {
            name.push_str(&format!(", {note}"));
        }
        let note = row.note.clone().map(|note| {
            // The note flows word by word; a link keeps the punctuation
            // that follows it on its line.
            let mut atoms: Vec<Vec<AnyElement>> = Vec::new();
            let mut glue = false;
            for (n, part) in setup::link_parts(&note).into_iter().enumerate() {
                match part {
                    Part::Text(text) => {
                        let starts_word = !text.starts_with(char::is_whitespace);
                        for (w, word) in text.split_whitespace().enumerate() {
                            let element = div().child(word.to_string()).into_any_element();
                            match atoms.last_mut() {
                                Some(atom) if w == 0 && glue && starts_word => atom.push(element),
                                _ => atoms.push(vec![element]),
                            }
                        }
                        glue = false;
                    }
                    Part::Url(url) => {
                        let id = SharedString::from(format!("setup-link-{index}-{n}"));
                        let focus =
                            super::controls::focus_for(id.clone(), window, cx).tab_stop(true);
                        let (opener, log, address) =
                            (actions.opener.clone(), actions.log.clone(), url.clone());
                        let press = std::rc::Rc::new(move |_: &mut Window, cx: &mut App| {
                            if let Some(log) = &log {
                                log.record("link activated", serde_json::json!({ "url": address }));
                            }
                            opener(&address, cx);
                        });
                        let (click, key, a11y) = (press.clone(), press.clone(), press);
                        atoms.push(vec![
                            ring(
                                div()
                                    .id(id)
                                    .role(Role::Link)
                                    .aria_label(url.clone())
                                    .aria_value(url.clone())
                                    .track_focus(&focus)
                                    .key_context("Part")
                                    .on_click(move |_, window, cx| click(window, cx))
                                    .on_a11y_action(
                                        accesskit::Action::Click,
                                        move |_, window, cx| a11y(window, cx),
                                    )
                                    .on_action(move |_: &Activate, window, cx| key(window, cx)),
                            )
                            .rounded(px(3.))
                            .underline()
                            .cursor_pointer()
                            .child(url)
                            .into_any_element(),
                        ]);
                        glue = true;
                    }
                }
            }
            div()
                .flex()
                .flex_wrap()
                .gap_x(px(3.))
                .mt(px(3.))
                .text_size(px(12.))
                .line_height(relative(1.45))
                .text_color(if failed { colour::err() } else { colour::dim() })
                .children(atoms.into_iter().map(|atom| div().flex().children(atom)))
        });
        list = list.child(
            theme::text(
                div()
                    .id(SharedString::from(format!("setup-step-{}", row.id)))
                    .when(need.is_none(), |d| {
                        d.track_focus(&need_focus.clone().tab_stop(false))
                    })
                    .role(Role::ListItem)
                    .aria_label(name)
                    .aria_position_in_set(index)
                    .aria_size_of_set(steps.len())
                    .px(px(14.))
                    .py(px(6.))
                    .flex()
                    .items_start()
                    .gap(px(10.)),
                TextKind::SettingsValue,
                &fonts,
            )
            .text_size(px(13.))
            .child(
                div()
                    .w(px(16.))
                    .h(px(18.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(if failed { colour::err() } else { colour::dim() })
                    .child(mark),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(10.))
                            .child(
                                div()
                                    .flex()
                                    .items_baseline()
                                    .text_color(label_colour)
                                    .child(row.label.clone())
                                    .when(row.optional, |d| {
                                        d.child(
                                            div()
                                                .ml(px(8.))
                                                .text_size(px(11.5))
                                                .text_color(colour::dim())
                                                .child("Optional"),
                                        )
                                    }),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(match row.state {
                                        "running" => colour::ash(),
                                        "failed" => colour::err(),
                                        _ => colour::dim(),
                                    })
                                    .child(row.value.clone()),
                            ),
                    )
                    .children(note)
                    .children(need.map(|b| div().mt(px(6.)).child(b))),
            ),
        );
    }
    list
}
