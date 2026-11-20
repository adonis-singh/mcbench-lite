//! A provider's Models section (`ModelsSection`, `web/app.js:731-875`;
//! inventory `components.md` E.5; `style-viewer-routes.md` 1.10 `.s-model`):
//! the filter, hide or show every match, the count, up to 150 rows (grip,
//! star, name, id, `Fast`, switch), and the reorder by the grip's drag or
//! its Up and Down keys ([`crate::state::model_list`]).
//!
//! Accessibility: the rows are a list named `Models` whose items are named
//! by the model's name and id and carry their position; a grip is a button
//! named as the page's aria-label (`Reorder <name>: drag, or use the up and
//! down arrow keys`); after a move by keys the grip keeps focus, the item's
//! position changes, and the polite region says `<name> moved to <n> of
//! <m>`. A star is a toggle button `Favorite <name>`, a switch `Show <name>
//! in the model picker`. The 150 rows are all laid out (not virtual).

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::parts::{field, section};
use super::{Dragging, MODEL_ROW, SettingsLayer};
use crate::ProviderSettings;
use crate::state::model_list::{self, Drag, MODEL_ROWS};
use crate::state::models::Catalogue;
use crate::view::controls::{Toggle, fonts};
use crate::view::icons::{self, Icon};
use crate::view::pickers::catalog::Catalog;
use crate::view::theme::{self, TextKind, colour};
use crate::view::{Activate, MoveNext, MovePrevious, ring};

impl SettingsLayer {
    /// Puts `model` at row `to` of the rows shown and saves the order in
    /// the draft; by keys, the move is said.
    pub(super) fn place_model(
        &mut self,
        model: &str,
        to: usize,
        by_keys: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(crate::state::settings::Pane::Provider(id)) = self.panel.view().cloned() else {
            return;
        };
        let Some(catalogue) = ready(&id, cx) else {
            return;
        };
        let entry = self.draft().expect("open").provider(&id).entry.clone();
        let query = self.filter_query(cx);
        let section =
            model_list::section(&catalogue, &entry.model_order, &entry.hidden_models, &query);
        let order = section.place(model, to);
        self.log(
            "reordered",
            serde_json::json!({ "provider": id, "model": model, "to": to }),
        );
        self.edit(|d| d.provider_mut(&id).entry.model_order = order, cx);
        if by_keys {
            let rows = section.shown().len();
            if let Some(m) = catalogue.iter().find(|m| m.id == model) {
                let line = model_list::moved_line(m, to, rows);
                self.announcer.update(cx, |a, cx| a.polite(line, cx));
            }
        }
    }

    /// The filter's text.
    fn filter_query(&self, cx: &App) -> String {
        self.filter
            .as_ref()
            .map(|f| f.read(cx).value().to_string())
            .unwrap_or_default()
    }

    pub(super) fn render_models(
        &mut self,
        entry: &ProviderSettings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let fonts = fonts(cx);
        let empty_line = |id: &'static str, text: String, error: bool| {
            theme::text(
                div()
                    .id(id)
                    .role(if error { Role::Alert } else { Role::Label })
                    .aria_label(text.clone())
                    .aria_value(text.clone()),
                TextKind::SettingsRowHelp,
                &fonts,
            )
            .p(px(18.))
            .text_size(px(12.5))
            .text_color(if error { colour::err() } else { colour::dim() })
            .child(text)
            .into_any_element()
        };
        if !entry.connected {
            let line = crate::state::settings::models_not_connected(entry);
            return section(
                "models",
                "Models",
                None,
                vec![empty_line("models-not-connected", line, false)],
                cx,
            );
        }
        let id = entry.id.clone();
        let catalog = Catalog::installed(cx);
        let (models, error) = match catalog.read(cx).catalogues().get(&id) {
            Some(Catalogue::Ready(models)) => (Some(models.clone()), None),
            Some(Catalogue::Failed(error)) => (Some(Vec::new()), Some(error.clone())),
            Some(Catalogue::Loading) | None => (None, None),
        };
        let favorites = catalog.read(cx).favorites();
        let query = self.filter_query(cx);
        let section_state = model_list::section(
            models.as_deref().unwrap_or_default(),
            &entry.model_order,
            &entry.hidden_models,
            &query,
        );
        let filter = self.filter.clone().expect("open settings have the filter");
        let link_text = section_state.link(&query);
        let link = {
            let disabled = section_state.matches.is_empty();
            let focus =
                crate::view::controls::focus_for("models-link", window, cx).tab_stop(!disabled);
            let this = cx.entity().downgrade();
            let hidden = section_state.toggle_matches(&entry.hidden_models);
            let pid = id.clone();
            let press = std::rc::Rc::new(move |_: &mut Window, cx: &mut App| {
                let (hidden, pid) = (hidden.clone(), pid.clone());
                let _ = this.update(cx, |s, cx| {
                    s.log("hide or show", serde_json::json!({ "provider": pid }));
                    s.edit(|d| d.provider_mut(&pid).entry.hidden_models = hidden, cx)
                });
            });
            let (click, key, a11y) = (press.clone(), press.clone(), press);
            theme::text(
                ring(
                    div()
                        .id("models-link")
                        .role(Role::Button)
                        .aria_label(link_text.clone())
                        .when(disabled, |d| {
                            d.a11y_synthetic_children(|b| b.parent_node().set_disabled())
                        })
                        .track_focus(&focus)
                        .key_context("Part")
                        .when(!disabled, |d| {
                            d.on_click(move |_, window, cx| click(window, cx))
                                .on_a11y_action(accesskit::Action::Click, move |_, window, cx| {
                                    a11y(window, cx)
                                })
                                .on_action(move |_: &Activate, window, cx| key(window, cx))
                        }),
                )
                .rounded(px(4.))
                .text_color(colour::ash())
                .when(disabled, |d| d.opacity(0.4))
                .when(!disabled, |d| {
                    d.cursor_pointer().hover(|s| s.text_color(colour::bone()))
                }),
                TextKind::Button,
                &fonts,
            )
            .text_size(px(12.5))
            .child(link_text)
        };
        let count = models
            .as_ref()
            .map(|_| section_state.count(&entry.hidden_models))
            .unwrap_or_default();
        let toolbar = div()
            .flex()
            .items_center()
            .gap(px(14.))
            .pt(px(12.))
            .px(px(18.))
            .pb(px(8.))
            .child(field(
                &filter,
                "Filter models",
                260.,
                32.,
                false,
                false,
                window,
                cx,
            ))
            .child(link)
            .child(
                theme::text(
                    div()
                        .id("models-count")
                        .role(Role::Label)
                        .aria_label(count.clone())
                        .aria_value(count.clone()),
                    TextKind::SettingsRowHelp,
                    &fonts,
                )
                .ml_auto()
                .text_color(colour::dim())
                .child(count),
            );
        let mut card: Vec<AnyElement> = vec![
            theme::text(
                super::parts::text_label(
                    "models-intro",
                    "Hidden models stay out of the model picker. Order and favorites apply wherever this provider is listed.",
                ),
                TextKind::SettingsRowHelp,
                &fonts,
            )
            .pt(px(16.))
            .px(px(18.))
            .text_size(px(12.5))
            .line_height(relative(1.45))
            .text_color(colour::dim())
            .into_any_element(),
            toolbar.into_any_element(),
        ];
        if models.is_none() {
            card.push(empty_line(
                "models-loading",
                "Loading models\u{2026}".into(),
                false,
            ));
        }
        if let Some(error) = error {
            card.push(empty_line("models-error", error, true));
        }
        let shown = section_state.shown();
        let rows = shown.len();
        let dragging = self.dragging.clone();
        let mut list = div()
            .id("models-list")
            .role(Role::List)
            .aria_label("Models")
            .aria_size_of_set(rows)
            .pt(px(2.))
            .px(px(8.))
            .pb(px(10.))
            .flex()
            .flex_col();
        for (row, model) in shown.into_iter().enumerate() {
            let hidden = entry.hidden_models.contains(&model.id);
            let favorite = favorites.iter().any(|(p, m)| *p == id && *m == model.id);
            let name = model_list::row_name(model).to_string();
            let key = format!("{id}::{}", model.id);
            let grip_focus = self
                .grips
                .entry(key.clone())
                .or_insert_with(|| cx.focus_handle())
                .clone()
                .tab_stop(true);
            let offset = dragging.as_ref().map(|d| d.drag.offset(row)).unwrap_or(0.);
            let dragged = dragging.as_ref().is_some_and(|d| d.model == model.id);
            let grip = {
                let (up, down) = (cx.entity().downgrade(), cx.entity().downgrade());
                let (up_id, down_id) = (model.id.clone(), model.id.clone());
                let press = cx.entity().downgrade();
                let press_id = model.id.clone();
                ring(
                    div()
                        .id(SharedString::from(format!("grip-{}", model.id)))
                        .role(Role::Button)
                        .aria_label(model_list::grip_name(model))
                        .aria_description("Drag to reorder")
                        .track_focus(&grip_focus)
                        .key_context("SettingsGrip")
                        .on_action(move |_: &MovePrevious, _, cx| {
                            if row > 0 {
                                let id = up_id.clone();
                                let _ =
                                    up.update(cx, |s, cx| s.place_model(&id, row - 1, true, cx));
                            }
                        })
                        .on_action(move |_: &MoveNext, _, cx| {
                            if row + 1 < rows {
                                let id = down_id.clone();
                                let _ =
                                    down.update(cx, |s, cx| s.place_model(&id, row + 1, true, cx));
                            }
                        })
                        .on_mouse_down(MouseButton::Left, move |event, window, cx| {
                            let id = press_id.clone();
                            let y = f32::from(event.position.y);
                            let _ = press.update(cx, |s, cx| {
                                s.dragging = Some(Dragging {
                                    model: id,
                                    start_y: y,
                                    drag: Drag::at(row, rows, MODEL_ROW, 0.),
                                });
                                cx.notify();
                            });
                            let _ = window;
                        }),
                )
                .w(px(18.))
                .h(px(28.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(5.))
                .cursor(CursorStyle::OpenHand)
                .text_color(colour::ash())
                .opacity(if dragged { 1. } else { 0.5 })
                .hover(|s| s.opacity(1.).text_color(colour::bone()))
                .child(icons::icon(Icon::Grip, 14.))
            };
            let star = {
                let focus = crate::view::controls::focus_for(
                    SharedString::from(format!("star-{}", model.id)),
                    window,
                    cx,
                )
                .tab_stop(true);
                let catalog = catalog.clone();
                let (pid, mid) = (id.clone(), model.id.clone());
                let log = self.log.clone();
                let press = std::rc::Rc::new(move |_: &mut Window, cx: &mut App| {
                    if let Some(log) = &log {
                        log.record("star", serde_json::json!({ "provider": pid, "model": mid }));
                    }
                    catalog.update(cx, |c, cx| c.toggle_favorite(&pid, &mid, cx));
                });
                let (click, key, a11y) = (press.clone(), press.clone(), press);
                ring(
                    div()
                        .id(SharedString::from(format!("star-{}", model.id)))
                        .role(Role::Button)
                        .aria_label(format!("Favorite {name}"))
                        .aria_description(if favorite {
                            "Remove favorite"
                        } else {
                            "Add favorite"
                        })
                        .aria_toggled(if favorite {
                            accesskit::Toggled::True
                        } else {
                            accesskit::Toggled::False
                        })
                        .track_focus(&focus)
                        .key_context("Part")
                        .on_click(move |_, window, cx| click(window, cx))
                        .on_a11y_action(accesskit::Action::Click, move |_, window, cx| {
                            a11y(window, cx)
                        })
                        .on_action(move |_: &Activate, window, cx| key(window, cx)),
                )
                .size(px(26.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(6.))
                .cursor_pointer()
                .text_color(if favorite {
                    colour::star()
                } else {
                    colour::dim()
                })
                .hover(|s| {
                    s.text_color(if favorite {
                        colour::star()
                    } else {
                        colour::ash()
                    })
                })
                .child(icons::icon(Icon::Star, 14.))
            };
            let fast = model
                .facts
                .fast
                .as_ref()
                .filter(|(fast, _)| *fast)
                .map(|(_, note)| {
                    let tip = note.clone().unwrap_or_else(|| "Fast mode available".into());
                    crate::view::controls::tip(
                        theme::text(
                            div()
                                .id(SharedString::from(format!("fast-{}", model.id)))
                                .role(Role::Label)
                                .aria_label("Fast")
                                .aria_value("Fast"),
                            TextKind::Tag,
                            &fonts,
                        )
                        .h(px(20.))
                        .px(px(7.))
                        .flex()
                        .items_center()
                        .rounded(px(6.))
                        .bg(colour::raised())
                        .text_color(colour::ash())
                        .child("Fast"),
                        SharedString::from(format!("fast-tip-{}", model.id)),
                        tip,
                    )
                });
            let toggle = {
                let this = cx.entity().downgrade();
                let (pid, mid) = (id.clone(), model.id.clone());
                Toggle::new(
                    SharedString::from(format!("show-{}", model.id)),
                    model_list::toggle_name(model),
                    !hidden,
                    move |on, _, cx| {
                        let (pid, mid) = (pid.clone(), mid.clone());
                        let _ = this.update(cx, |s, cx| {
                            s.log("toggled", serde_json::json!({ "model": mid, "on": on }));
                            s.edit(
                                |d| {
                                    let e = &mut d.provider_mut(&pid).entry;
                                    e.hidden_models =
                                        model_list::set_hidden(&e.hidden_models, &[mid], !on);
                                },
                                cx,
                            )
                        });
                    },
                )
            };
            let item = div()
                .id(SharedString::from(format!("model-{}", model.id)))
                .role(Role::ListItem)
                .aria_label(format!("{name}, {}", model.id))
                .aria_position_in_set(row)
                .aria_size_of_set(rows)
                .relative()
                .top(px(offset))
                .min_h(px(MODEL_ROW))
                .pl(px(4.))
                .pr(px(10.))
                .flex()
                .items_center()
                .gap(px(10.))
                .rounded(px(8.))
                .when(dragging.is_none(), |d| {
                    d.hover(|s| s.bg(colour::hover_well()))
                })
                .when(dragged, |d| {
                    d.bg(colour::raised()).shadow(theme::settings::dragged())
                })
                .child(grip)
                .child(star)
                .child(
                    div()
                        .flex()
                        .flex_1()
                        .min_w_0()
                        .items_center()
                        .gap(px(10.))
                        .when(hidden, |d| d.opacity(0.45))
                        .child(
                            theme::text(div(), TextKind::SettingsValue, &fonts)
                                .flex_none()
                                .text_color(colour::bone())
                                .child(name.clone()),
                        )
                        // At 980 px and below the id and the
                        // capabilities are not shown.
                        .when(!self.narrow, |d| {
                            d.child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .font_family(fonts.data.clone())
                                    .text_size(px(11.5))
                                    .text_color(colour::dim())
                                    .child(model.id.clone()),
                            )
                            .children(fast)
                        }),
                )
                .child(toggle);
            // The dragged row paints after its siblings, over the rows it
            // passes (GPUI has no z-index in a flow).
            list = if dragged {
                list.child(deferred(item).with_priority(1))
            } else {
                list.child(item)
            };
        }
        if let Some(more) = section_state.more() {
            list = list.child(empty_line("models-more", more, false));
        }
        if models.is_some() && section_state.matches.is_empty() {
            list = list.child(empty_line("models-none", "No models match.".into(), false));
        }
        let _ = MODEL_ROWS;
        card.push(list.into_any_element());
        // The section's card holds the intro, the toolbar and the list.
        section("models", "Models", None, card, cx)
    }
}

/// `provider`'s catalogue when it has arrived.
fn ready(provider: &str, cx: &App) -> Option<Vec<crate::ModelInfo>> {
    match Catalog::installed(cx).read(cx).catalogues().get(provider) {
        Some(Catalogue::Ready(models)) => Some(models.clone()),
        _ => None,
    }
}
