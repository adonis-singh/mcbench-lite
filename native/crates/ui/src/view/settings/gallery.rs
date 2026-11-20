//! Settings' controls for the controls gallery (`mcbench-ui-dev
//! --gallery`, `dev::gallery_settings`): the navigation with its switches,
//! a row of each control kind, the key field, a sign-in row in each state,
//! the shortcut recorder recording, the danger zone armed, and a setup step
//! in each state. The layer is opened on a draft of the stand-in's
//! settings without its overlay or its `settings` key layer; the pieces are
//! drawn inline.

use gpui_kit::*;

use super::parts::{Row, field, section, value};
use super::provider::key_field;
use super::{Pane, SettingsLayer};
use crate::state::keymap::BindAction;
use crate::state::settings::{self as panel, Panel, Votes};
use crate::state::sign_in::{After, Login};
use crate::view::controls::{Button, CopyButton, Toggle, fonts};
use crate::{InstallView, Progress, SettingsView, SignInState, SignInView};

impl SettingsLayer {
    /// Opens the panel on `view` for the gallery: no overlay, no key layer;
    /// the recorder shown recording, the danger zone armed.
    pub fn gallery_open(
        &mut self,
        view: SettingsView,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.panel = Panel::Open {
            view: Pane::General,
            draft: panel::draft_of(view),
            error: String::new(),
        };
        self.make_inputs(window, cx);
        self.votes = Votes::Count(12);
        self.votes_armed = true;
        self.recording = Some(BindAction::Sidebar);
    }

    /// The gallery's pieces.
    pub fn render_gallery(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let draft = self
            .draft()
            .cloned()
            .expect("gallery_open ran before render_gallery");
        let fonts = fonts(cx);
        let _ = &fonts;
        let nav = div()
            .h(px(700.))
            .w(px(300.))
            .rounded(px(10.))
            .overflow_hidden()
            .child(self.render_nav(&draft, &Pane::Provider("claude-code".into()), window, cx));
        let repo = self.repo.clone().expect("the repository field");
        let log = self.log.clone();
        let kinds = section(
            "gallery-rows",
            "Rows",
            None,
            vec![
                Row::new("g-button", "A row with a button")
                    .first(true)
                    .help("Its help under its title.")
                    .control(Button::new("g-row-button", "Sign out"))
                    .render(cx),
                Row::new("g-switch", "A row with a switch")
                    .control(Toggle::new(
                        "g-row-switch",
                        "Offer it in the model picker",
                        true,
                        |_, _, _| {},
                    ))
                    .render(cx),
                Row::new("g-value", "A row with a value")
                    .control(value("Claude Code CLI over MCP".into(), false, cx))
                    .render(cx),
                Row::new("g-mono", "A row with a monospace value")
                    .control(value("stand-in/claude".into(), true, cx))
                    .render(cx),
                Row::new("g-field", "A row with a text field")
                    .control(field(
                        &repo,
                        "GitHub repository for shared builds",
                        300.,
                        34.,
                        true,
                        true,
                        window,
                        cx,
                    ))
                    .render(cx),
                Row::new("g-copy", "A row with a copy button")
                    .control(
                        CopyButton::new("g-row-copy", "WXYZ-2026", "Copy code").on_copied(
                            move |text, _| {
                                if let Some(log) = &log {
                                    log.record("copied", serde_json::json!({ "text": text }));
                                }
                            },
                        ),
                    )
                    .render(cx),
            ],
            cx,
        );
        let key = self
            .keys_inputs
            .get("openrouter")
            .cloned()
            .expect("OpenRouter's key field");
        let key_row = section(
            "gallery-key",
            "Account",
            None,
            vec![
                Row::new("g-key", "API key")
                    .first(true)
                    .help(panel::key_help(
                        Some(crate::KeySource::Saved),
                        "OPENROUTER_API_KEY",
                        &draft.settings_file,
                    ))
                    .control(key_field(&key, window, cx))
                    .render(cx),
            ],
            cx,
        );
        let claude = draft.provider("claude-code").entry.clone();
        let mut not_installed = claude.clone();
        not_installed.connected = false;
        not_installed.signed_in = false;
        not_installed.account = None;
        not_installed
            .sign_in
            .as_mut()
            .expect("Claude Code has a sign-in")
            .installed = false;
        let mut login_only = not_installed.clone();
        login_only.signed_in = true;
        login_only.account = Some("builder@example.com".into());
        login_only.plan = Some("Max".into());
        let codex = draft.provider("codex-cli").entry.clone();
        let view = |state, url: Option<&str>, code: Option<&str>, wants: bool| SignInView {
            state,
            url: url.map(str::to_string),
            user_code: code.map(str::to_string),
            wants_input: wants,
            message: None,
            error: None,
        };
        let states: Vec<(&str, crate::ProviderSettings, Login)> = vec![
            (
                "Not signed in, its CLI to install first",
                not_installed.clone(),
                Login::default(),
            ),
            (
                "Installing, with its download",
                not_installed.clone(),
                Login {
                    sign_in: None,
                    install: Some(InstallView {
                        state: "running".into(),
                        progress: Some(Progress {
                            done: 84_000_000,
                            total: Some(236_418_112),
                        }),
                        error: None,
                    }),
                    after: Some(After::SignIn(Some("claudeai".into()))),
                },
            ),
            (
                "Starting",
                claude.clone(),
                Login {
                    sign_in: Some(view(SignInState::Starting, None, None, false)),
                    ..Login::default()
                },
            ),
            (
                "Waiting, wanting the pasted code",
                not_installed.clone(),
                Login {
                    sign_in: Some(view(
                        SignInState::Waiting,
                        Some("https://claude.ai/oauth/authorize?code=true"),
                        None,
                        true,
                    )),
                    ..Login::default()
                },
            ),
            (
                "Waiting, with a one-time code",
                codex,
                Login {
                    sign_in: Some(view(
                        SignInState::Waiting,
                        Some("https://auth.openai.com/codex/device"),
                        Some("WXYZ-2026"),
                        false,
                    )),
                    ..Login::default()
                },
            ),
            ("A login whose CLI is missing", login_only, Login::default()),
            (
                "An install that failed",
                not_installed,
                Login {
                    install: Some(InstallView {
                        state: "failed".into(),
                        progress: None,
                        error: Some("the download's checksum did not match".into()),
                    }),
                    ..Login::default()
                },
            ),
        ];
        let mut sign_ins: Vec<AnyElement> = Vec::new();
        for (index, (title, entry, login)) in states.into_iter().enumerate() {
            let focus = cx.focus_handle();
            let rows = self.sign_in_rows_with(&entry, login, focus, window, cx);
            sign_ins.push(
                div()
                    .id(("g-sign-in", index))
                    .child(section(
                        SharedString::from(format!("g-sign-in-{index}")),
                        title,
                        None,
                        rows,
                        cx,
                    ))
                    .into_any_element(),
            );
        }
        let shortcuts = self.shortcuts(window, cx);
        let danger = self.danger_zone(window, cx);
        let steps = setup_steps();
        let setup = crate::view::setup::setup_screen(
            &steps,
            true,
            crate::view::setup::SetupActions {
                retry: std::rc::Rc::new(|_, _| {}),
                connect: std::rc::Rc::new(|_, _| {}),
                opener: self.opener.clone(),
                log: self.log.clone(),
                install_wsl: std::rc::Rc::new(|_, _| {}),
                restart: std::rc::Rc::new(|_, _| {}),
            },
            window,
            cx,
        );
        div()
            .flex()
            .flex_col()
            .gap(px(22.))
            .child(
                div().flex().gap(px(22.)).items_start().child(nav).child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(22.))
                        .child(kinds)
                        .child(key_row)
                        .child(shortcuts)
                        .child(danger),
                ),
            )
            .child(div().flex().flex_col().gap(px(22.)).children(sign_ins))
            .child(div().w(px(520.)).child(setup))
            .into_any_element()
    }
}

/// A setup step in each state: done, running with download progress,
/// waiting, failed (a note with a link), and an optional one unavailable.
fn setup_steps() -> Vec<mcbench_setup::Step> {
    use mcbench_setup::{Bytes, Step, StepState};
    let step = |id: &str, title: &str, doing: &str, optional, state| Step {
        id: id.into(),
        title: title.into(),
        doing: doing.into(),
        optional,
        state,
        needs: None,
    };
    vec![
        step("data", "Minecraft's data", "Unpacking Minecraft's data", false, StepState::Done { seconds: 1.2 }),
        step(
            "textures",
            "Minecraft's textures",
            "Downloading Minecraft's textures from Mojang",
            false,
            StepState::Running {
                detail: Some("Downloading the client jar".into()),
                progress: Some(Bytes {
                    done: 27_000_000,
                    total: Some(64_300_000),
                }),
            },
        ),
        step("bake", "Baked textures", "Baking the block textures", false, StepState::Waiting),
        step(
            "engine",
            "The engine",
            "Building the engine",
            false,
            StepState::Failed {
                error: "fetch failed: see https://www.minecraft.net/en-us/download/server. (a gallery error)".into(),
            },
        ),
        step(
            "shell",
            "Model's shell",
            "Downloading the model's shell",
            true,
            StepState::Unavailable {
                error: "To use bash and build, Windows needs WSL. Install it here: Windows asks for an administrator's permission once, and for a restart when its virtualisation feature was off.".into(),
            },
        ),
    ]
    .into_iter()
    .map(|mut s| {
        // The shell's step needs WSL: its row shows `Install WSL`.
        if s.id == "shell" {
            s.needs = Some(mcbench_setup::Needs::InstallWsl);
        }
        s
    })
    .collect()
}
