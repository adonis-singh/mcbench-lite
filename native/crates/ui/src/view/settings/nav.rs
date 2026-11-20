//! Settings' navigation (`.settings-nav`, `SettingsNavItem`,
//! `web/app.js:712-729` and 7049-7073): Connect an AI, General, then the
//! providers and the harnesses, each with its logo, label, version, the
//! core's summary line and its switch.
//!
//! Accessibility: a vertical tab list named `Settings sections` whose tabs
//! are named by what they show (`Claude Code v2.1.311, Signed in · Max`),
//! the active pane's selected (the page marks it only by a class); it is
//! one Tab stop, walked with Up, Down, Home and End, which move focus and
//! show nothing; Enter, Space or a click show the pane. Each provider's
//! switch is named `Offer <label> in the model picker`.

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{SettingsLayer, logo};
use crate::state::settings::{Draft, Pane, usable};
use crate::view::controls::{Toggle, fonts};
use crate::view::icons::{self, Icon};
use crate::view::theme::{self, TextKind, colour};
use crate::view::{Activate, MoveFirst, MoveLast, MoveNext, MovePrevious, ring};

/// One navigation row.
struct Item {
    pane: Pane,
    name: String,
}

impl SettingsLayer {
    /// The rows in order.
    fn nav_items(draft: &Draft) -> Vec<Item> {
        let mut items = vec![
            Item {
                pane: Pane::Connect,
                name: "Connect an AI".into(),
            },
            Item {
                pane: Pane::General,
                name: "General".into(),
            },
        ];
        for harness in [false, true] {
            for p in draft
                .providers
                .iter()
                .filter(|p| p.entry.harness == harness)
            {
                let e = &p.entry;
                let version = e
                    .version
                    .as_ref()
                    .map(|v| format!(" v{v}"))
                    .unwrap_or_default();
                items.push(Item {
                    pane: Pane::Provider(e.id.clone()),
                    name: format!("{}{version}, {}", e.label, e.summary),
                });
            }
        }
        items
    }

    /// Moves focus along the rows (Up, Down, Home, End).
    fn nav_step(&mut self, step: crate::state::Step, window: &mut Window, cx: &mut Context<Self>) {
        let Some(draft) = self.draft().cloned() else {
            return;
        };
        let items = Self::nav_items(&draft);
        let current = items.iter().position(|i| {
            self.nav
                .get(i.pane.id())
                .is_some_and(|f| f.is_focused(window))
        });
        let Some(next) = crate::state::step(current, items.len(), step) else {
            return;
        };
        let id = items[next].pane.id().to_string();
        self.nav_current = Some(id.clone());
        let focus = self.nav_focus(&id, cx);
        window.focus(&focus, cx);
        cx.notify();
    }

    pub(super) fn render_nav(
        &mut self,
        draft: &Draft,
        view: &Pane,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let fonts = fonts(cx);
        let items = Self::nav_items(draft);
        let count = items.len();
        let current = self
            .nav_current
            .clone()
            .unwrap_or_else(|| view.id().to_string());
        let mut nav = div()
            .id("settings-nav")
            .role(Role::TabList)
            .aria_label("Settings sections")
            .aria_orientation(accesskit::Orientation::Vertical)
            .key_context("SettingsNav")
            .on_action(cx.listener(|this, _: &MovePrevious, window, cx| {
                this.nav_step(crate::state::Step::Previous, window, cx)
            }))
            .on_action(cx.listener(|this, _: &MoveNext, window, cx| {
                this.nav_step(crate::state::Step::Next, window, cx)
            }))
            .on_action(cx.listener(|this, _: &MoveFirst, window, cx| {
                this.nav_step(crate::state::Step::First, window, cx)
            }))
            .on_action(cx.listener(|this, _: &MoveLast, window, cx| {
                this.nav_step(crate::state::Step::Last, window, cx)
            }))
            .flex_none()
            .map(|d| {
                if self.narrow {
                    d.w_full()
                        .max_h(px(0.38 * f32::from(_window.viewport_size().height)))
                } else {
                    d.w(px(300.)).h_full()
                }
            })
            .overflow_y_scroll()
            .pt(px(20.))
            .px(px(12.))
            .pb(px(16.))
            .flex()
            .flex_col()
            .gap(px(2.))
            .bg(colour::bedrock())
            .child(
                theme::text(
                    div()
                        .id("settings-nav-title")
                        .role(Role::Heading)
                        .aria_label("Settings")
                        .aria_level(2),
                    TextKind::SettingsNavTitle,
                    &fonts,
                )
                .px(px(10.))
                .pb(px(10.))
                .child("Settings"),
            );
        let label = |text: &'static str| {
            theme::text(
                div()
                    .id(text)
                    .role(Role::Label)
                    .aria_label(text)
                    .aria_value(text),
                TextKind::SettingsNavLabel,
                &fonts,
            )
            .px(px(10.))
            .pt(px(14.))
            .pb(px(6.))
            .child(text)
        };
        let first_harness = items.iter().position(
            |i| matches!(&i.pane, Pane::Provider(id) if draft.provider(id).entry.harness),
        );
        for (index, item) in items.into_iter().enumerate() {
            if index == 2 {
                nav = nav.child(label("Providers"));
            }
            if Some(index) == first_harness {
                nav = nav.child(label("Harnesses"));
            }
            let id = item.pane.id().to_string();
            let active = &item.pane == view;
            let focus = self.nav_focus(&id, cx).tab_stop(id == current);
            let pane = item.pane.clone();
            let this = cx.entity().downgrade();
            let show = std::rc::Rc::new(move |window: &mut Window, cx: &mut App| {
                let pane = pane.clone();
                let _ = this.update(cx, |s, cx| {
                    s.nav_current = Some(pane.id().to_string());
                    s.show_pane(pane, window, cx)
                });
            });
            let (click, key, a11y) = (show.clone(), show.clone(), show);
            let row = ring(
                div()
                    .id(SharedString::from(format!("nav-{id}")))
                    .role(Role::Tab)
                    .aria_label(item.name.clone())
                    .aria_selected(active)
                    .aria_position_in_set(index)
                    .aria_size_of_set(count)
                    .track_focus(&focus)
                    .key_context("Part")
                    .on_click(move |_, window, cx| click(window, cx))
                    .on_a11y_action(accesskit::Action::Click, move |_, window, cx| {
                        a11y(window, cx)
                    })
                    .on_action(move |_: &Activate, window, cx| key(window, cx)),
            )
            .w_full()
            .px(px(10.))
            .py(px(8.))
            .rounded(px(9.))
            .flex()
            .items_center()
            .gap(px(12.))
            .cursor_pointer()
            .hover(|s| s.bg(colour::hover_well()))
            .when(active, |d| d.bg(colour::raised()));
            let row = match &item.pane {
                Pane::Connect | Pane::General => {
                    let (icon, name) = if item.pane == Pane::Connect {
                        (Icon::Plug, "Connect an AI")
                    } else {
                        (Icon::Wrench, "General")
                    };
                    row.min_h(px(40.))
                        .child(
                            div()
                                .w(px(18.))
                                .flex()
                                .justify_center()
                                .text_color(if active {
                                    colour::bone()
                                } else {
                                    colour::ash()
                                })
                                .child(icons::icon(icon, 16.)),
                        )
                        .child(
                            theme::text(div(), TextKind::SettingsNavItem, &fonts)
                                .flex_1()
                                .child(name),
                        )
                }
                Pane::Provider(pid) => {
                    let e = &draft.provider(pid).entry;
                    let off = !e.enabled;
                    let muted = !usable(e);
                    let pid_toggle = pid.clone();
                    let this = cx.entity().downgrade();
                    row.min_h(px(54.))
                        .child(
                            div()
                                .w(px(18.))
                                .flex()
                                .justify_center()
                                .when(off, |d| d.opacity(0.45))
                                .child(logo(pid, 18.)),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .gap(px(2.))
                                .when(off, |d| d.opacity(0.45))
                                .child(
                                    theme::text(div(), TextKind::SettingsNavItem, &fonts)
                                        .flex()
                                        .items_baseline()
                                        .gap(px(7.))
                                        .child(e.label.clone())
                                        .when_some(e.version.clone(), |d, v| {
                                            d.child(
                                                div()
                                                    .font_family(fonts.data.clone())
                                                    .text_size(px(11.))
                                                    .font_weight(FontWeight(400.))
                                                    .text_color(colour::dim())
                                                    .child(format!("v{v}")),
                                            )
                                        }),
                                )
                                .child(
                                    theme::text(div(), TextKind::SettingsNavItem, &fonts)
                                        .text_size(px(12.))
                                        .font_weight(FontWeight(400.))
                                        .text_color(if muted {
                                            colour::dim()
                                        } else {
                                            colour::ash()
                                        })
                                        .truncate()
                                        .child(e.summary.clone()),
                                ),
                        )
                        .child(Toggle::new(
                            SharedString::from(format!("offer-{pid}")),
                            format!("Offer {} in the model picker", e.label),
                            e.enabled,
                            move |on, _, cx| {
                                let id = pid_toggle.clone();
                                let _ = this.update(cx, |s, cx| {
                                    s.log(
                                        "toggled",
                                        serde_json::json!({ "provider": id, "on": on }),
                                    );
                                    s.edit(|d| d.provider_mut(&id).entry.enabled = on, cx)
                                });
                            },
                        ))
                }
            };
            nav = nav.child(row);
        }
        nav.into_any_element()
    }
}
