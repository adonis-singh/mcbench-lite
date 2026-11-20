//! A provider's pane (`ProviderPane` and `SignInRows`,
//! `web/app.js:934-1146`; inventory `components.md` 1.7, E.7, E.8): the
//! head with `Checked <ago>`, the Account section (the key field, or the
//! sign-in rows, or the account with `Sign out`), the Models section
//! ([`super::models`]) and the Runtime section.

use std::rc::Rc;

use gpui_kit::component::input::Input;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::parts::{Row, field, pane_head, pane_title, section, value, working_dots};
use super::{SettingsLayer, logo, now_ms};
use crate::state::settings::{self as panel, Draft};
use crate::state::sign_in::{self, Rows};
use crate::view::controls::{Button, CopyButton, Kind, fonts};
use crate::view::icons::Icon;
use crate::view::theme::{self, colour, shadow};
use crate::view::{Activate, ring};
use crate::{Command, ProviderSettings};

impl SettingsLayer {
    pub(super) fn render_provider(
        &mut self,
        draft: &Draft,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let fonts = fonts(cx);
        let entry = draft.provider(id).entry.clone();
        let checked = format!("Checked {}", panel::ago(draft.checked_at, now_ms()));
        let this = cx.entity().downgrade();
        let recheck = Button::new("recheck", checked)
            .kind(Kind::Ghost)
            .with_icon(Icon::Refresh, 13.)
            .tip("Check logins, keys and versions again")
            .on_press(move |_, cx| {
                let _ = this.update(cx, |s, _| s.recheck());
            });
        let mut head = vec![logo(id, 22.), pane_title(&entry.label, cx)];
        if let Some(version) = &entry.version {
            head.push(
                div()
                    .font_family(fonts.data.clone())
                    .text_size(px(12.))
                    .text_color(colour::dim())
                    .child(format!("v{version}"))
                    .into_any_element(),
            );
        }
        head.push(div().ml_auto().child(recheck).into_any_element());
        let account = self.account(draft, &entry, window, cx);
        let models = self.render_models(&entry, window, cx);
        let runtime = self.runtime(&entry, cx);
        div()
            .flex()
            .flex_col()
            .gap(px(26.))
            .child(pane_head(head))
            .child(account)
            .child(models)
            .child(runtime)
            .into_any_element()
    }

    fn account(
        &mut self,
        draft: &Draft,
        entry: &ProviderSettings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut rows: Vec<AnyElement> = Vec::new();
        if let Some(env_key) = &entry.env_key {
            let input = self
                .keys_inputs
                .get(&entry.id)
                .cloned()
                .expect("a key provider has its key field");
            let help = panel::key_help(entry.key_source, env_key, &draft.settings_file);
            rows.push(
                Row::new("row-api-key", "API key")
                    .first(true)
                    .help(help)
                    .control(key_field(&input, window, cx))
                    .render(cx),
            );
        } else if sign_in::is_signed_in(entry) {
            let (title, help) = sign_in::signed_in_row(entry);
            let this = cx.entity().downgrade();
            let id = entry.id.clone();
            rows.push(
                Row::new("row-account", title)
                    .first(true)
                    .help(help)
                    .control(Button::new("sign-out", "Sign out").on_press(move |_, cx| {
                        let id = id.clone();
                        let _ = this.update(cx, |s, _| {
                            s.log("sign out", serde_json::json!({ "provider": id }));
                            s.core.send(Command::SignOut { provider: id });
                        });
                    }))
                    .render(cx),
            );
        } else {
            rows.extend(self.sign_in_rows(entry, window, cx));
        }
        section("account", "Account", None, rows, cx)
    }

    /// `SignInRows`.
    fn sign_in_rows(
        &mut self,
        entry: &ProviderSettings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let login = self.logins.get(&entry.id).cloned().unwrap_or_default();
        let focus = self.sign_in_focus.clone();
        self.sign_in_rows_with(entry, login, focus, window, cx)
    }

    /// `SignInRows` for `login`, the row's controls sharing `focus`.
    pub(super) fn sign_in_rows_with(
        &mut self,
        entry: &ProviderSettings,
        login: crate::state::sign_in::Login,
        focus: FocusHandle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let fonts = fonts(cx);
        let id = entry.id.clone();
        let cancel = |cx: &mut Context<Self>| {
            let this = cx.entity().downgrade();
            let id = id.clone();
            Button::new("sign-in-cancel", "Cancel")
                .focus(focus.clone())
                .on_press(move |_, cx| {
                    let id = id.clone();
                    let _ = this.update(cx, |s, _| {
                        s.log("cancel sign-in", serde_json::json!({ "provider": id }));
                        s.core.send(Command::CancelSignIn { provider: id });
                    });
                })
        };
        let failure_help = |help: String, failure: Option<String>| {
            div()
                .flex()
                .flex_col()
                .child(super::parts::text_label("help", help))
                .when_some(failure, |d, failure| {
                    d.child(
                        div()
                            .id("sign-in-error")
                            .role(Role::Alert)
                            .aria_label(failure.clone())
                            .mt(px(6.))
                            .text_color(colour::err())
                            .child(failure),
                    )
                })
                .into_any_element()
        };
        match sign_in::rows(entry, &login) {
            Rows::Installing { title, help } => vec![
                crate::view::ring(div().id("installing"))
                    .role(Role::Status)
                    .aria_label(format!("{title}. {help}"))
                    // It holds the focus the button that started it had,
                    // so the reader hears it change (not a Tab stop).
                    .track_focus(&focus.clone().tab_stop(false))
                    .child(
                        Row::new("row-installing", title)
                            .first(true)
                            .help(help)
                            .control(working_dots())
                            .render(cx),
                    )
                    .into_any_element(),
            ],
            Rows::Starting => vec![
                Row::new("row-starting", "Starting the sign-in")
                    .first(true)
                    .help("Waiting for the sign-in address.")
                    .control(cancel(cx))
                    .render(cx),
            ],
            Rows::Waiting {
                next,
                url,
                user_code,
                wants_input,
            } => {
                let mut rows = Vec::new();
                let link = {
                    let focus =
                        crate::view::controls::focus_for("sign-in-link", window, cx).tab_stop(true);
                    let (opener, log, address) =
                        (self.opener.clone(), self.log.clone(), url.clone());
                    let press = Rc::new(move |_: &mut Window, cx: &mut App| {
                        if let Some(log) = &log {
                            log.record("link activated", serde_json::json!({ "url": address }));
                        }
                        opener(&address, cx);
                    });
                    let (click, key, a11y) = (press.clone(), press.clone(), press);
                    ring(
                        div()
                            .id("sign-in-link")
                            .role(Role::Link)
                            .aria_label(url.clone())
                            .aria_value(url.clone())
                            .track_focus(&focus)
                            .key_context("Part")
                            .on_click(move |_, window, cx| click(window, cx))
                            .on_a11y_action(accesskit::Action::Click, move |_, window, cx| {
                                a11y(window, cx)
                            })
                            .on_action(move |_: &Activate, window, cx| key(window, cx)),
                    )
                    .min_w_0()
                    .rounded(px(3.))
                    .font_family(fonts.data.clone())
                    .text_size(px(12.))
                    .text_color(colour::bone())
                    .underline()
                    .cursor_pointer()
                    .truncate()
                    .child(url.clone())
                };
                let log = self.log.clone();
                let help = div()
                    .flex()
                    .flex_col()
                    .child(super::parts::text_label("help", next))
                    .child(
                        div()
                            .mt(px(6.))
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .min_w_0()
                            .child(link)
                            .child(
                                CopyButton::new("copy-address", url.clone(), "Copy address")
                                    .on_copied(move |text, _| {
                                        if let Some(log) = &log {
                                            log.record(
                                                "copied",
                                                serde_json::json!({ "text": text }),
                                            );
                                        }
                                    }),
                            ),
                    )
                    .into_any_element();
                rows.push(
                    Row::new("row-waiting", "Open the sign-in page")
                        .first(true)
                        .help_element(help)
                        .control(cancel(cx))
                        .render(cx),
                );
                if let Some(code) = user_code {
                    let log = self.log.clone();
                    rows.push(
                        Row::new("row-user-code", "One-time code")
                            .help("Enter this code on the sign-in page.")
                            .control(
                                div()
                                    .id("user-code")
                                    .role(Role::Group)
                                    .aria_label(format!("One-time code, {code}"))
                                    .h(px(32.))
                                    .pl(px(11.))
                                    .pr(px(4.))
                                    .flex()
                                    .items_center()
                                    .gap(px(2.))
                                    .rounded(px(8.))
                                    .bg(colour::raised())
                                    .shadow(shadow::control_face())
                                    .child(
                                        div()
                                            .id("user-code-text")
                                            .role(Role::Label)
                                            .aria_label(code.clone())
                                            .aria_value(code.clone())
                                            .font_family(fonts.data.clone())
                                            .text_size(px(12.))
                                            .text_color(colour::bone())
                                            .child(code.clone()),
                                    )
                                    .child(
                                        CopyButton::new("copy-code", code.clone(), "Copy code")
                                            .on_copied(move |text, _| {
                                                if let Some(log) = &log {
                                                    log.record(
                                                        "copied",
                                                        serde_json::json!({ "text": text }),
                                                    );
                                                }
                                            }),
                                    ),
                            )
                            .render(cx),
                    );
                }
                if let Some(help) = wants_input {
                    let input = self
                        .code
                        .clone()
                        .expect("open settings have the code field");
                    let empty = input.read(cx).value().trim().is_empty();
                    let this = cx.entity().downgrade();
                    let form = div()
                        .flex()
                        .flex_wrap()
                        .justify_end()
                        .gap(px(8.))
                        .child(field(
                            &input,
                            "Code from the sign-in page",
                            240.,
                            34.,
                            true,
                            true,
                            window,
                            cx,
                        ))
                        .child(
                            Button::new("submit-code", "Submit")
                                .kind(Kind::Primary)
                                .disabled(empty)
                                .on_press(move |_, cx| {
                                    let _ = this.update(cx, |s, cx| s.submit_code(cx));
                                }),
                        );
                    rows.push(
                        Row::new("row-code", "Code from the sign-in page")
                            .help(help)
                            .control(form)
                            .render(cx),
                    );
                }
                rows
            }
            Rows::LoginOnly {
                title,
                help,
                failure,
                button,
            } => {
                let this = cx.entity().downgrade();
                let id = entry.id.clone();
                vec![
                    Row::new("row-login-only", title)
                        .first(true)
                        .help_element(failure_help(help, failure))
                        .control(
                            Button::new("install-cli", button)
                                .kind(Kind::Primary)
                                .focus(focus.clone())
                                .on_press(move |_, cx| {
                                    let id = id.clone();
                                    let _ = this.update(cx, |s, cx| s.begin_sign_in(&id, None, cx));
                                }),
                        )
                        .render(cx),
                ]
            }
            Rows::NotSignedIn {
                help,
                failure,
                starts,
            } => {
                let mut buttons = div().flex().flex_wrap().justify_end().gap(px(8.));
                for (index, start) in starts.into_iter().enumerate() {
                    let this = cx.entity().downgrade();
                    let id = entry.id.clone();
                    let method = start.method.clone();
                    let mut button = Button::new(
                        SharedString::from(format!("sign-in-{index}")),
                        start.label.clone(),
                    )
                    .kind(if start.primary {
                        Kind::Primary
                    } else {
                        Kind::Raised
                    })
                    .on_press(move |_, cx| {
                        let (id, method) = (id.clone(), method.clone());
                        let _ = this.update(cx, |s, cx| s.begin_sign_in(&id, method, cx));
                    });
                    if index == 0 {
                        button = button.focus(focus.clone());
                    }
                    buttons = buttons.child(button);
                }
                vec![
                    Row::new("row-not-signed-in", "Not signed in")
                        .first(true)
                        .help_element(failure_help(help, failure))
                        .control(buttons)
                        .render(cx),
                ]
            }
        }
    }

    /// The Runtime section.
    fn runtime(&mut self, entry: &ProviderSettings, cx: &mut Context<Self>) -> AnyElement {
        let mut rows: Vec<Row> = Vec::new();
        if let Some(runtime) = &entry.runtime {
            rows.push(
                Row::new("row-binary", "Binary")
                    .help(panel::binary_help(&runtime.binary_env, &runtime.binary))
                    .control(value(
                        runtime
                            .binary_value
                            .clone()
                            .unwrap_or_else(|| runtime.binary.clone()),
                        true,
                        cx,
                    )),
            );
            rows.push(
                Row::new("row-home", "Config directory")
                    .help(panel::home_help(&runtime.home_env))
                    .control(value(runtime.resolved_home.clone(), true, cx)),
            );
        }
        if let Some(endpoint) = &entry.endpoint {
            rows.push(Row::new("row-endpoint", "Endpoint").control(value(
                endpoint.clone(),
                true,
                cx,
            )));
        }
        rows.push(Row::new("row-protocol", "Protocol").control(value(
            entry.protocol.clone(),
            false,
            cx,
        )));
        if !entry.harness && entry.id != "codex" {
            rows.push(Row::new("row-loop", panel::LOOP.0).help(panel::LOOP.1));
        }
        for (index, (title, help)) in panel::runtime_notes(&entry.id).iter().enumerate() {
            rows.push(
                Row::new(SharedString::from(format!("row-note-{index}")), *title).help(*help),
            );
        }
        let rows = rows
            .into_iter()
            .enumerate()
            .map(|(i, r)| r.first(i == 0).render(cx))
            .collect();
        section("runtime", "Runtime", None, rows, cx)
    }
}

/// The API key field: a password field (dots, no value or text for
/// assistive technology, no copy, cut or keyboard selection; paste works),
/// monospace, 300 px.
pub(super) fn key_field(
    input: &Entity<gpui_kit::component::input::InputState>,
    window: &Window,
    cx: &App,
) -> AnyElement {
    use gpui_kit::base::actions::{SelectDown, SelectLeft, SelectRight, SelectUp};
    use gpui_kit::component::input::{
        Copy, Cut, SelectAll, SelectToEnd, SelectToEndOfLine, SelectToNextWordEnd,
        SelectToPreviousWordStart, SelectToStart, SelectToStartOfLine,
    };
    let fonts = fonts(cx);
    let focused = input.read(cx).focus_handle(cx).is_focused(window);
    let refuse = |d: Stateful<Div>| {
        d.capture_action(|_: &SelectAll, _, cx| cx.stop_propagation())
            .capture_action(|_: &SelectLeft, _, cx| cx.stop_propagation())
            .capture_action(|_: &SelectRight, _, cx| cx.stop_propagation())
            .capture_action(|_: &SelectUp, _, cx| cx.stop_propagation())
            .capture_action(|_: &SelectDown, _, cx| cx.stop_propagation())
            .capture_action(|_: &SelectToStart, _, cx| cx.stop_propagation())
            .capture_action(|_: &SelectToEnd, _, cx| cx.stop_propagation())
            .capture_action(|_: &SelectToStartOfLine, _, cx| cx.stop_propagation())
            .capture_action(|_: &SelectToEndOfLine, _, cx| cx.stop_propagation())
            .capture_action(|_: &SelectToNextWordEnd, _, cx| cx.stop_propagation())
            .capture_action(|_: &SelectToPreviousWordStart, _, cx| cx.stop_propagation())
            .capture_action(|_: &Copy, _, cx| cx.stop_propagation())
            .capture_action(|_: &Cut, _, cx| cx.stop_propagation())
    };
    refuse(
        div()
            .id("api-key-field")
            .w(px(300.))
            .max_w_full()
            .h(px(34.))
            .flex()
            .items_center()
            .rounded(px(8.))
            .bg(colour::raised())
            .shadow(if focused {
                theme::settings::input_focus(true)
            } else {
                shadow::field_face()
            })
            .font_family(fonts.data.clone())
            .text_size(px(12.5))
            .text_color(colour::bone()),
    )
    .child(
        div().flex_1().min_w_0().child(
            Input::new(input)
                .aria_label("API key")
                .content_type(gpui_kit::component::input::InputContentType::Password)
                .appearance(false)
                .text_size(px(12.5)),
        ),
    )
    .into_any_element()
}
