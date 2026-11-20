//! The General pane (`app.md` 8.5; `components.md` E.3, K.4, N.3): the
//! title model and its effort, the theme, the repository for shared builds
//! and the published builds, the shortcuts, and the danger zone. No
//! Account section: there is no hosted version.

use std::rc::Rc;
use std::time::Instant;

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::parts::{Row, field, pane_head, pane_title, section};
use super::{COPIED_FOR, FLASH_FOR, SettingsLayer, now_ms};
use crate::state::keymap::{BindAction, format_bind};
use crate::state::published::{self, View};
use crate::state::settings::{self as panel, Draft};
use crate::view::controls::{Button, ConfirmButton, Kind, fonts};
use crate::view::icons::{self, Icon};
use crate::view::keys;
use crate::view::theme::{self, TextKind, colour, shadow};
use crate::view::{Activate, ring};

impl SettingsLayer {
    /// The flash's progress for the row titled `title`, while it runs; the
    /// first frame starts it and keeps the frames coming until it ends.
    fn flash_of(&mut self, title: &str, window: &mut Window) -> Option<f32> {
        let (row, started) = self.flash.as_mut()?;
        if row != title {
            return None;
        }
        let at = *started.get_or_insert_with(Instant::now);
        let t = at.elapsed().as_secs_f32() / FLASH_FOR.as_secs_f32();
        if t >= 1. {
            self.flash = None;
            return None;
        }
        window.request_animation_frame();
        Some(t)
    }

    /// A row of the General pane, with its flash and, on the flash's first
    /// frame, its bounds for scrolling it to the middle.
    fn general_row(&mut self, row: Row, window: &mut Window, cx: &App) -> AnyElement {
        let title = row.title.clone();
        let first_frame = matches!(&self.flash, Some((r, None)) if *r == title);
        let flash = self.flash_of(&title, window);
        let element = row.flash(flash).render(cx);
        if !first_frame {
            return element;
        }
        let bounds = self.flash_bounds.clone();
        div()
            .relative()
            .child(element)
            .child(
                canvas(move |b, _, _| bounds.set(Some(b)), |_, _, _, _| {})
                    .absolute()
                    .inset_0(),
            )
            .into_any_element()
    }

    pub(super) fn render_general(
        &mut self,
        draft: &Draft,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut sections: Vec<AnyElement> = vec![pane_head(vec![pane_title("General", cx)])];
        sections.push(self.conversations(draft, window, cx));
        sections.push(self.appearance(window, cx));
        sections.push(self.sharing(window, cx));
        sections.push(self.shortcuts(window, cx));
        sections.push(self.danger_zone(window, cx));
        div()
            .flex()
            .flex_col()
            .gap(px(26.))
            .children(sections)
            .into_any_element()
    }

    /// `Conversations`: the title model, its effort, `Use the chat's own
    /// model`.
    fn conversations(
        &mut self,
        draft: &Draft,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let picker = self
            .title_picker
            .clone()
            .expect("open settings have their title picker");
        let facts = draft.title_model.as_ref().and_then(|t| {
            crate::view::pickers::catalog::Catalog::installed(cx)
                .read(cx)
                .facts(&t.provider, &t.model)
                .cloned()
        });
        let effort = match (&draft.title_model, &facts) {
            (Some(title), Some(facts)) if facts.reasoning && !facts.efforts.is_empty() => {
                let this = cx.entity().downgrade();
                let on_change: crate::view::pickers::effort::OnEffort =
                    Rc::new(move |effort, _, cx| {
                        let _ = this.update(cx, |s, cx| {
                            s.edit(
                                |d| {
                                    d.title_model
                                        .as_mut()
                                        .expect("a title model has an effort")
                                        .effort = effort
                                },
                                cx,
                            )
                        });
                    });
                let current = (!title.effort.is_empty()).then_some(title.effort.as_str());
                Some(
                    div()
                        .w(px(112.))
                        .child(crate::view::pickers::effort::effort_picker_as(
                            "title-effort",
                            "Title model reasoning effort",
                            true,
                            &self.effort,
                            facts,
                            current,
                            on_change,
                            cx,
                        ))
                        .into_any_element(),
                )
            }
            _ => None,
        };
        let own = draft.title_model.as_ref().map(|_| {
            let this = cx.entity().downgrade();
            crate::view::chat::composer::text_link(
                "own-model",
                "Use the chat's own model",
                move |_, cx| {
                    let _ =
                        this.update(cx, |s, cx| s.edit(|d| panel::with_title_model(d, None), cx));
                },
                window,
                cx,
            )
        });
        let controls = div()
            .flex()
            .items_center()
            .justify_end()
            .gap(px(8.))
            .text_size(px(12.5))
            .child(picker)
            .children(effort)
            .children(own);
        let row = Row::new("row-title-model", "Title model")
            .first(true)
            .tip("Names each new chat from its first message")
            .control(controls);
        let row = self.general_row(row, window, cx);
        section("conversations", "Conversations", None, vec![row], cx)
    }

    /// `Appearance`: the theme, applied at once (not part of the draft).
    fn appearance(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let fonts = fonts(cx);
        let light = (self.hooks.light)(cx);
        let button = |id: &'static str,
                      label: &'static str,
                      icon: Icon,
                      on: bool,
                      set_light: bool,
                      window: &mut Window,
                      cx: &mut Context<Self>| {
            let focus = crate::view::controls::focus_for(id, window, cx).tab_stop(true);
            let theme_hook = self.hooks.theme.clone();
            let log = self.log.clone();
            let press = Rc::new(move |_: &mut Window, cx: &mut App| {
                if let Some(log) = &log {
                    log.record("theme", serde_json::json!({ "light": set_light }));
                }
                theme_hook(set_light, cx);
            });
            let (click, key, a11y) = (press.clone(), press.clone(), press);
            theme::text(
                ring(
                    div()
                        .id(id)
                        .role(Role::Button)
                        .aria_label(label)
                        .aria_toggled(if on {
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
                .min_h(px(30.))
                .px(px(10.))
                .py(px(5.))
                .flex()
                .items_center()
                .gap(px(6.))
                .rounded(px(6.))
                .cursor_pointer()
                .text_color(colour::ash())
                .hover(|s| s.text_color(colour::bone()))
                .when(on, |d| {
                    d.bg(colour::raised())
                        .text_color(colour::bone())
                        .shadow(shadow::control_face())
                }),
                TextKind::SmallTrigger,
                &fonts,
            )
            .child(icons::icon(icon, 14.))
            .child(label)
            .into_any_element()
        };
        let light_button = button("theme-light", "Light", Icon::Sun, light, true, window, cx);
        let dark_button = button("theme-dark", "Dark", Icon::Moon, !light, false, window, cx);
        let group = div()
            .id("color-theme")
            .role(Role::Group)
            .aria_label("Color theme")
            .flex()
            .gap(px(3.))
            .p(px(3.))
            .rounded(px(9.))
            .bg(colour::well())
            .shadow(shadow::control_face())
            .child(light_button)
            .child(dark_button);
        let row = Row::new("row-theme", "Theme")
            .first(true)
            .tip("Applies right away")
            .control(group);
        let row = self.general_row(row, window, cx);
        section("appearance", "Appearance", None, vec![row], cx)
    }

    /// `Sharing`: the repository field and the published builds.
    fn sharing(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let repo = self
            .repo
            .clone()
            .expect("open settings have the repository field");
        let row = Row::new("row-share-repo", "GitHub repository for shared builds")
            .first(true)
            .help("A public repository with GitHub Pages on. Needs git and gh signed in on this machine.")
            .control(field(
                &repo,
                "GitHub repository for shared builds",
                300.,
                34.,
                true,
                true,
                window,
                cx,
            ));
        let row = row.render(cx);
        let builds = self.published_builds(window, cx);
        section("sharing", "Sharing", None, vec![row, builds], cx)
    }

    /// The published builds (`PublishedBuilds`).
    fn published_builds(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let fonts = fonts(cx);
        let note = |id: &'static str| {
            theme::text(div().id(id), TextKind::SettingsRowHelp, &fonts)
                .text_size(px(12.5))
                .text_color(colour::dim())
                .py(px(4.))
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(10.))
        };
        let view = published::view(&self.published).unwrap_or_else(|e| panic!("{e}"));
        let body: AnyElement = match view {
            View::Loading => note("published-loading")
                .role(Role::Status)
                .aria_label("Loading published builds\u{2026}")
                .child("Loading published builds\u{2026}")
                .into_any_element(),
            View::Failed => {
                let error = self.published.error.clone().unwrap_or_default();
                let this = cx.entity().downgrade();
                note("published-failed")
                    .role(Role::Alert)
                    .aria_label(error.clone())
                    .text_color(colour::err())
                    .child(error)
                    .child(
                        Button::new("published-retry", "Retry").on_press(move |_, cx| {
                            let _ = this.update(cx, |s, cx| {
                                s.published = published::reading(&s.published)
                                    .unwrap_or_else(|e| panic!("{e}"));
                                s.log("retry published", serde_json::json!({}));
                                s.core.send(crate::Command::ReadShares);
                                cx.notify();
                            });
                        }),
                    )
                    .into_any_element()
            }
            View::NoRepo => note("published-no-repo")
                .role(Role::Label)
                .aria_label("Set a repository to publish builds.")
                .aria_value("Set a repository to publish builds.")
                .child("Set a repository to publish builds.")
                .into_any_element(),
            View::Empty => note("published-empty")
                .role(Role::Label)
                .aria_label("No builds are published.")
                .aria_value("No builds are published.")
                .child("No builds are published.")
                .into_any_element(),
            View::List => self.published_list(window, cx),
        };
        div()
            .id("published-builds")
            .flex()
            .flex_col()
            .gap(px(4.))
            .pt(px(4.))
            .px(px(16.))
            .pb(px(14.))
            .child(body)
            .into_any_element()
    }

    fn published_list(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let fonts = fonts(cx);
        let data = self
            .published
            .data
            .clone()
            .expect("a published list holds its data");
        let repo = data
            .repo
            .clone()
            .expect("a published list has its repository");
        let now = now_ms();
        let repo_url = format!("https://github.com/{repo}");
        let opener = self.opener.clone();
        let log = self.log.clone();
        let link = {
            let focus =
                crate::view::controls::focus_for("published-repo", window, cx).tab_stop(true);
            let url = repo_url.clone();
            let (opener, log) = (opener.clone(), log.clone());
            let press = Rc::new(move |_: &mut Window, cx: &mut App| {
                if let Some(log) = &log {
                    log.record("link activated", serde_json::json!({ "url": url }));
                }
                opener(&url, cx);
            });
            let (click, key, a11y) = (press.clone(), press.clone(), press);
            ring(
                div()
                    .id("published-repo")
                    .role(Role::Link)
                    .aria_label(repo.clone())
                    .aria_value(repo_url.clone())
                    .track_focus(&focus)
                    .key_context("Part")
                    .on_click(move |_, window, cx| click(window, cx))
                    .on_a11y_action(accesskit::Action::Click, move |_, window, cx| {
                        a11y(window, cx)
                    })
                    .on_action(move |_: &Activate, window, cx| key(window, cx)),
            )
            .rounded(px(3.))
            .text_color(colour::ash())
            .cursor_pointer()
            .hover(|s| s.text_color(colour::bone()))
            .child(repo.clone())
        };
        let count = published::count_line(data.builds.len());
        let mut rows = div().flex().flex_col();
        let total = data.builds.len();
        for (index, build) in data.builds.iter().enumerate() {
            let removing = self.published.removing.contains(&build.share_id);
            let error = self.published.errors.get(&build.share_id).cloned();
            let bench = build.kind == "bench";
            let subject = published::subject(build).unwrap_or_else(|e| panic!("{e}"));
            let meta = published::meta(build, now);
            let share = build.share_id.clone();
            let copied = self
                .copied
                .as_ref()
                .is_some_and(|(id, at)| *id == share && at.elapsed() < COPIED_FOR);
            let open = {
                let url = build.url.clone();
                let (opener, log) = (opener.clone(), log.clone());
                Button::new(SharedString::from(format!("open-{share}")), "Open")
                    .kind(Kind::Quiet)
                    .with_icon(Icon::Open, 14.)
                    .on_press(move |_, cx| {
                        if let Some(log) = &log {
                            log.record("link activated", serde_json::json!({ "url": url }));
                        }
                        opener(&url, cx);
                    })
            };
            let copy = {
                let url = build.url.clone();
                let this = cx.entity().downgrade();
                let id = share.clone();
                Button::new(
                    SharedString::from(format!("copy-{share}")),
                    if copied { "Copied" } else { "Copy link" },
                )
                .kind(Kind::Quiet)
                .with_icon(Icon::Copy, 14.)
                .on_press(move |_, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(url.clone()));
                    let id = id.clone();
                    let _ = this.update(cx, |s, cx| {
                        s.log("copied", serde_json::json!({ "text": url }));
                        s.copied = Some((id, Instant::now()));
                        cx.notify();
                        cx.spawn(async |this, cx| {
                            cx.background_executor().timer(COPIED_FOR).await;
                            let _ = this.update(cx, |_, cx| cx.notify());
                        })
                        .detach();
                    });
                })
            };
            let go = subject.clone().filter(|_| !build.archived).map(|subject| {
                let this = cx.entity().downgrade();
                Button::new(
                    SharedString::from(format!("go-{share}")),
                    if bench { "Go to bench" } else { "Go to chat" },
                )
                .kind(Kind::Quiet)
                .with_icon(Icon::Forward, 14.)
                .on_press(move |window, cx| {
                    let subject = subject.clone();
                    let _ = this.update(cx, |s, cx| {
                        s.log(
                            "go to",
                            serde_json::json!({ "subject": format!("{subject:?}") }),
                        );
                        s.close(window, cx);
                        let go_to = s.hooks.go_to.clone();
                        window.defer(cx, move |window, cx| go_to(subject, window, cx));
                    });
                })
            });
            let remove = {
                let this = cx.entity().downgrade();
                let id = share.clone();
                ConfirmButton::new(
                    SharedString::from(format!("remove-{share}")),
                    format!("published:{share}"),
                    Icon::X,
                    if removing {
                        "Removing\u{2026}"
                    } else {
                        "Remove"
                    },
                    "Remove from the web?",
                    move |_, cx| {
                        let id = id.clone();
                        let _ = this.update(cx, |s, cx| {
                            s.published = published::removing(&s.published, &id)
                                .unwrap_or_else(|e| panic!("{e}"));
                            s.log("remove published", serde_json::json!({ "share": id }));
                            s.core.send(crate::Command::RemoveShare { share_id: id });
                            cx.notify();
                        });
                    },
                )
                .title("Take the public page down")
                .disabled(removing)
            };
            let name = format!(
                "{}{}, {meta}",
                if bench { "Bench, " } else { "" },
                build.title
            );
            rows = rows.child(
                div()
                    .id(SharedString::from(format!("published-{share}")))
                    .role(Role::ListItem)
                    .aria_label(name)
                    .aria_position_in_set(index)
                    .aria_size_of_set(total)
                    .py(px(10.))
                    .flex()
                    .flex_col()
                    .gap(px(7.))
                    .when(index > 0, |d| {
                        d.border_t_1().border_color(theme::settings::hairline())
                    })
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(3.))
                            .when(removing, |d| d.opacity(0.55))
                            .child(
                                theme::text(div(), TextKind::SettingsRowTitle, &fonts)
                                    .text_size(px(13.))
                                    .flex()
                                    .items_center()
                                    .gap(px(7.))
                                    .when(bench, |d| {
                                        d.child(
                                            theme::text(div(), TextKind::ResolutionTag, &fonts)
                                                .px(px(5.))
                                                .rounded(px(5.))
                                                .shadow(theme::sidebar::tag_ring())
                                                .child("Bench"),
                                        )
                                    })
                                    // par-shell (N.3): the full title on hover.
                                    .child(crate::view::controls::pointer_tip(
                                        div()
                                            .id(SharedString::from(format!(
                                                "published-title-{share}"
                                            )))
                                            .min_w_0()
                                            .truncate()
                                            .child(build.title.clone()),
                                        SharedString::from(format!("published-title-tip-{share}")),
                                        build.title.clone(),
                                    )),
                            )
                            .child({
                                // par-shell (N.3): the labels and the date
                                // on hover.
                                let parts = published::meta_parts(build, now);
                                let first = div()
                                    .id(SharedString::from(format!("published-first-{share}")))
                                    .child(parts.first);
                                let first: AnyElement = match parts.first_tip {
                                    Some(tip) => crate::view::controls::pointer_tip(
                                        first,
                                        SharedString::from(format!("published-first-tip-{share}")),
                                        tip,
                                    )
                                    .into_any_element(),
                                    None => first.into_any_element(),
                                };
                                theme::text(div(), TextKind::SettingsRowHelp, &fonts)
                                    .text_color(colour::dim())
                                    .flex()
                                    .flex_wrap()
                                    .child(first)
                                    .child("\u{a0}\u{b7} published\u{a0}")
                                    .child(crate::view::controls::pointer_tip(
                                        div()
                                            .id(SharedString::from(format!(
                                                "published-ago-{share}"
                                            )))
                                            .child(parts.ago),
                                        SharedString::from(format!("published-ago-tip-{share}")),
                                        published::published_at_tip(build.published_at),
                                    ))
                                    .child(parts.tail.replacen(' ', "\u{a0}", 1))
                            })
                            .when_some(error, |d, error| {
                                d.child(
                                    theme::text(
                                        div()
                                            .id("row-error")
                                            .role(Role::Alert)
                                            .aria_label(error.clone()),
                                        TextKind::SettingsRowHelp,
                                        &fonts,
                                    )
                                    .text_color(colour::err())
                                    .child(error),
                                )
                            }),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap(px(6.))
                            .child(open)
                            .child(copy)
                            .children(go)
                            .child(remove),
                    ),
            );
        }
        div()
            .flex()
            .flex_col()
            .child(
                theme::text(div(), TextKind::SettingsRowHelp, &fonts)
                    .text_size(px(12.5))
                    .text_color(colour::dim())
                    .py(px(4.))
                    .flex()
                    .gap(px(4.))
                    .child(super::parts::text_label("published-count", count))
                    .child(link),
            )
            .child(
                rows.id("published-list")
                    .role(Role::List)
                    .aria_label("Published builds"),
            )
            .into_any_element()
    }

    /// `Shortcuts` (`BindsSection`): one row per action with its key; a
    /// press records the next combination; `Reset all` when any differs.
    pub(super) fn shortcuts(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let fonts = fonts(cx);
        let keymap = keys::keymap(cx).clone();
        let action = keymap.changed().then(|| {
            let this = cx.entity().downgrade();
            let binds = self.hooks.binds.clone();
            Button::new("reset-binds", "Reset all")
                .kind(Kind::Ghost)
                .on_press(move |_, cx| {
                    let _ = this.update(cx, |s, cx| {
                        s.stop_recording(cx);
                        s.log("reset shortcuts", serde_json::json!({}));
                        cx.notify();
                    });
                    binds(crate::state::keymap::Keymap::default(), cx);
                })
                .into_any_element()
        });
        let mut rows = Vec::new();
        for (index, bind) in BindAction::ALL.into_iter().enumerate() {
            let combo = keymap.bind(bind).to_string();
            let recording = self.recording == Some(bind);
            let text = if recording {
                "Press a shortcut".to_string()
            } else {
                format_bind(&combo, keys::MAC)
            };
            let tip = if recording {
                "Press a shortcut, or Escape to cancel"
            } else {
                "Click to change"
            };
            let id = SharedString::from(format!("bind-{}", bind.id()));
            let focus = crate::view::controls::focus_for(id.clone(), window, cx).tab_stop(true);
            let this = cx.entity().downgrade();
            let press = Rc::new(move |_: &mut Window, cx: &mut App| {
                let _ = this.update(cx, |s, cx| s.toggle_recording(bind, cx));
            });
            let (click, key, a11y) = (press.clone(), press.clone(), press);
            let unset = combo.is_empty();
            let name = if recording {
                format!(
                    "{}, recording: press a shortcut, or Escape to cancel",
                    bind.label()
                )
            } else {
                format!("{}: {text}", bind.label())
            };
            let element = theme::text(
                ring(
                    div()
                        .id(id.clone())
                        .role(Role::Button)
                        .aria_label(name)
                        .aria_toggled(if recording {
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
                .min_w(px(64.))
                .h(px(28. + 4.))
                .px(px(10.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(7.))
                .bg(colour::well())
                .shadow(if recording {
                    theme::settings::bind_listen(0.6)
                } else {
                    theme::settings::bind_key()
                })
                .cursor_pointer()
                .hover(|s| s.shadow(theme::settings::bind_hover())),
                TextKind::Button,
                &fonts,
            )
            .text_size(px(12.5))
            .text_color(if recording || unset {
                colour::ash()
            } else {
                colour::bone()
            })
            .when(unset && !recording, |d| d.text_color(colour::dim()))
            .child(text);
            let element = crate::view::controls::tip(element, id, tip);
            let row = Row::new(
                SharedString::from(format!("row-bind-{}", bind.id())),
                bind.label(),
            )
            .first(index == 0)
            .control(element);
            rows.push(self.general_row(row, window, cx));
        }
        section("shortcuts", "Shortcuts", action, rows, cx)
    }

    /// `Danger zone` (`DangerZone`).
    pub(super) fn danger_zone(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let fonts = fonts(cx);
        let control: AnyElement = if self.votes_armed {
            let question = panel::votes_question(&self.votes);
            let cancel = {
                let this = cx.entity().downgrade();
                Button::new("votes-cancel", "Cancel")
                    .focus(self.votes_focus.clone())
                    .disabled(self.clearing)
                    .on_press(move |_, cx| {
                        let _ = this.update(cx, |s, cx| {
                            s.votes_armed = false;
                            cx.notify();
                        });
                    })
            };
            let delete = {
                let this = cx.entity().downgrade();
                Button::new("votes-delete", "Delete votes")
                    .kind(Kind::Danger)
                    .disabled(self.clearing)
                    .on_press(move |_, cx| {
                        let _ = this.update(cx, |s, cx| {
                            s.clearing = true;
                            s.log("clear votes", serde_json::json!({}));
                            s.core.send(crate::Command::ClearVotes);
                            cx.notify();
                        });
                    })
            };
            theme::text(
                div()
                    .id("danger-confirm")
                    .role(Role::Group)
                    .aria_label(question.clone())
                    .flex()
                    .items_center()
                    .gap(px(8.)),
                TextKind::Error,
                &fonts,
            )
            .child(question)
            .child(cancel)
            .child(delete)
            .into_any_element()
        } else {
            let (label, enabled) = panel::votes_button(&self.votes);
            let this = cx.entity().downgrade();
            Button::new("clear-votes", label)
                .kind(Kind::Danger)
                .focus(self.votes_focus.clone())
                .disabled(!enabled)
                .on_press(move |_, cx| {
                    let _ = this.update(cx, |s, cx| {
                        s.votes_armed = true;
                        cx.notify();
                    });
                })
                .into_any_element()
        };
        let row = Row::new("row-clear-votes", "Clear leaderboard")
            .first(true)
            .help("Deletes every vote, blind or not, so the leaderboard starts over. Benches and their builds stay.")
            .control(control);
        let row = self.general_row(row, window, cx);
        section("danger-zone", "Danger zone", None, vec![row], cx)
    }
}
