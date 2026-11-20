//! The controls gallery (`mcbench-ui-dev --gallery`): every shared control
//! of [`crate::view::controls`] in each of its states, every icon and logo,
//! for the platform checks in a real window and for `tests/controls.rs`.
//! Each press is logged (`pressed`, with the control's name).

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::ActionLog;
use crate::state::popover::MENU_EXIT;
use crate::view::controls::popover::{self, Menu, Placement, PopoverState};
use crate::view::controls::{Button, ConfirmButton, CopyButton, Kind, Toggle, tip};
use crate::view::icons::{self, Icon, Logo};
use crate::view::theme::{self, TextKind, colour};

/// The efforts the gallery's menu offers.
pub const EFFORTS: [&str; 3] = ["low", "medium", "high"];

/// The gallery.
pub struct Gallery {
    log: Option<ActionLog>,
    switch_on: bool,
    effort: usize,
    menu: Entity<PopoverState>,
    // pickers: the composer's pickers, made at the first frame (they need
    // the window).
    pickers: Option<Entity<super::gallery_pickers::PickerGallery>>,
    // settings agent: Settings' controls.
    settings: Entity<super::gallery_settings::SettingsGallery>,
    // pane agent: the 3D pane in its states, made at the first frame.
    pane: Option<Entity<super::gallery_pane::PaneGallery>>,
}

impl Gallery {
    /// The gallery, logging to `log`.
    pub fn new(log: Option<ActionLog>, cx: &mut Context<Self>) -> Self {
        // settings agent
        let log_for_settings = log.clone();
        Gallery {
            log,
            switch_on: true,
            effort: 1,
            menu: cx.new(|cx| PopoverState::new(MENU_EXIT, cx)),
            pickers: None,
            // pane agent
            pane: None,
            // settings agent
            settings: {
                let log = log_for_settings;
                cx.new(|cx| super::gallery_settings::SettingsGallery::new(log, cx))
            },
        }
    }

    /// The menu's popover.
    pub fn menu(&self) -> Entity<PopoverState> {
        self.menu.clone()
    }

    /// The effort chosen in the menu.
    pub fn effort(&self) -> &'static str {
        EFFORTS[self.effort]
    }

    /// The pickers' section (after the first frame).
    pub fn pickers(&self) -> Option<Entity<super::gallery_pickers::PickerGallery>> {
        self.pickers.clone()
    }

    /// The switch's value.
    pub fn switch_on(&self) -> bool {
        self.switch_on
    }

    fn press(&self, name: &str) -> impl Fn(&mut Window, &mut App) + 'static {
        let log = self.log.clone();
        let name = name.to_string();
        move |_, _| {
            if let Some(log) = &log {
                log.record("pressed", serde_json::json!({ "control": name }));
            }
        }
    }
}

fn section(title: &str, fonts: &theme::Fonts) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(10.))
        .child(theme::text(div(), TextKind::SideTitle, fonts).child(title.to_string()))
}

fn row() -> Div {
    div().flex().flex_wrap().items_center().gap(px(10.))
}

impl Render for Gallery {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let fonts = crate::view::controls::fonts(cx);
        let entity = cx.entity();
        // pickers
        if self.pickers.is_none() {
            let log = self.log.clone();
            self.pickers =
                Some(cx.new(|cx| super::gallery_pickers::PickerGallery::new(log, window, cx)));
        }
        let pickers = self.pickers.clone().expect("made above");
        // pane agent
        let pane = self
            .pane
            .get_or_insert_with(|| cx.new(|cx| super::gallery_pane::PaneGallery::new(window, cx)))
            .clone();
        let buttons = row()
            .child(Button::new("raised", "Raised").on_press(self.press("Raised")))
            .child(Button::new("raised-off", "Raised, disabled").disabled(true))
            .child(
                Button::new("primary", "Primary")
                    .kind(Kind::Primary)
                    .on_press(self.press("Primary")),
            )
            .child(
                Button::new("primary-off", "Primary, disabled")
                    .kind(Kind::Primary)
                    .disabled(true),
            )
            .child(
                Button::new("danger", "Delete for good")
                    .kind(Kind::Danger)
                    .on_press(self.press("Delete for good")),
            )
            .child(
                Button::new("ghost", "Ghost")
                    .kind(Kind::Ghost)
                    .with_icon(Icon::ChevronDown, 12.)
                    .on_press(self.press("Ghost")),
            );
        let headers = row()
            .child(
                Button::new("header", "Run bench")
                    .kind(Kind::Header)
                    .on_press(self.press("Run bench")),
            )
            .child(
                Button::new("header-off", "Run bench, disabled")
                    .kind(Kind::Header)
                    .disabled(true),
            )
            .child(
                Button::new("quiet", "Share")
                    .kind(Kind::Quiet)
                    .with_icon(Icon::Share, 14.)
                    .on_press(self.press("Share")),
            )
            .child(
                Button::new("quiet-pressed", "Share, open")
                    .kind(Kind::Quiet)
                    .with_icon(Icon::Share, 14.)
                    .pressed(true)
                    .on_press(self.press("Share, open")),
            )
            .child(
                ConfirmButton::new(
                    "confirm",
                    "gallery:one",
                    Icon::X,
                    "Remove",
                    "Remove from the web?",
                    self.press("Remove from the web?"),
                )
                .title("Take the public page down"),
            );
        let small = row()
            .child(
                Button::icon("icon-compose", Icon::Compose, "New chat")
                    .on_press(self.press("New chat")),
            )
            .child(
                Button::icon("icon-panel", Icon::Panel, "Collapse sidebar")
                    .on_press(self.press("Collapse sidebar")),
            )
            .child(
                Button::icon("icon-settings", Icon::Settings, "Settings, disabled").disabled(true),
            )
            // par-shell: a button whose label has its own tip (a bench
            // chip: `Remove` over the button, the model id over its name).
            .child(
                Button::new("label-tip", "Claude Opus")
                    .with_icon(Icon::X, 12.)
                    .tip("Remove")
                    .label_tip("claude-opus-5-5")
                    .on_press(self.press("Claude Opus")),
            )
            .child(
                CopyButton::new(
                    "copy",
                    "world.set([0, 1, 0], 'redstone_lamp');",
                    "Copy code",
                )
                .on_copied({
                    let log = self.log.clone();
                    move |text, _| {
                        if let Some(log) = &log {
                            log.record("copied", serde_json::json!({ "text": text }));
                        }
                    }
                }),
            )
            .child({
                let entity = entity.clone();
                Toggle::new(
                    "switch",
                    "Show the grid",
                    self.switch_on,
                    move |on, _, cx| {
                        entity.update(cx, |g, cx| {
                            g.switch_on = on;
                            if let Some(log) = &g.log {
                                log.record("switched", serde_json::json!({ "on": on }));
                            }
                            cx.notify();
                        })
                    },
                )
            })
            .child(tip(
                theme::text(
                    div()
                        .id("tip-host")
                        .role(Role::Group)
                        .aria_label("Hover for a tip"),
                    TextKind::Button,
                    &fonts,
                )
                .px(px(10.))
                .py(px(6.))
                .rounded(px(theme::radius::SM))
                .bg(colour::raised())
                .child("Hover for a tip"),
                "tip-host",
                "Tokens used in this chat, and the context window",
            ));
        let mut menu = Menu::new(&self.menu, "Effort");
        for (index, effort) in EFFORTS.iter().enumerate() {
            let entity = entity.clone();
            menu = menu.item(*effort, index == self.effort, move |_, cx| {
                entity.update(cx, |g, cx| {
                    g.effort = index;
                    if let Some(log) = &g.log {
                        log.record("chose", serde_json::json!({ "effort": EFFORTS[index] }));
                    }
                    cx.notify();
                })
            });
        }
        let menu_element = menu.render(cx);
        let trigger = popover::trigger(
            &self.menu,
            Button::new("effort", format!("Effort: {}", self.effort()))
                .kind(Kind::Ghost)
                .with_icon(Icon::ChevronDown, 12.)
                .tip("How hard the model thinks"),
            cx,
        );
        let picker = div().relative().child(trigger).when_some(
            popover::panel(&self.menu, Placement::Below, menu_element, cx),
            |d, panel| d.child(panel),
        );
        let icon_row = Icon::ALL
            .into_iter()
            .fold(row().gap(px(8.)), |d, icon| d.child(icons::icon(icon, 16.)));
        let logo_row = Logo::ALL
            .into_iter()
            .fold(row().gap(px(8.)), |d, logo| d.child(icons::logo(logo, 18.)))
            .child(icons::app_mark(28.));
        div()
            .id("gallery")
            .key_context("Screen")
            .on_action(|_: &crate::view::FocusNext, window, cx| window.focus_next(cx))
            .on_action(|_: &crate::view::FocusPrevious, window, cx| window.focus_prev(cx))
            .on_action(|_: &crate::view::CloseLayer, window, cx| {
                crate::view::keys::close_top(window, cx)
            })
            .size_full()
            .overflow_y_scroll()
            .bg(colour::cave())
            .text_color(colour::bone())
            .font_family(fonts.ui.clone())
            .p(px(24.))
            .flex()
            .flex_col()
            .gap(px(22.))
            .child(section("Buttons", &fonts).child(buttons))
            .child(section("Header actions", &fonts).child(headers))
            .child(section("Icon buttons, copy, switch, tooltip", &fonts).child(small))
            .child(section("Popover menu", &fonts).child(row().child(picker)))
            .child(
                section("Icons", &fonts)
                    .text_color(colour::ash())
                    .child(icon_row),
            )
            .child(
                section("Logos", &fonts)
                    .text_color(colour::ash())
                    .child(logo_row),
            )
            // pickers
            .child(section("Pickers", &fonts).child(pickers))
            // bench agent: Bench's toggles (the vote and its pick, the tie,
            // Blind, the round switch) and a retry.
            .child(section("Bench", &fonts).child(bench_controls()))
            // pane agent: the 3D pane's states and the chips' squares.
            .child(section("3D pane", &fonts).child(pane))
            // settings agent: the theme (both themes' pictures) and
            // Settings' controls.
            .child(
                section("Settings", &fonts)
                    .child(row().child({
                        let light = crate::view::theme::mode() == crate::view::theme::Mode::Light;
                        Button::new(
                            "gallery-theme",
                            if light { "Dark theme" } else { "Light theme" },
                        )
                        .on_press(move |_, cx| {
                            crate::view::set_theme(
                                if light {
                                    crate::view::theme::Mode::Dark
                                } else {
                                    crate::view::theme::Mode::Light
                                },
                                cx,
                            )
                        })
                    }))
                    .child(self.settings.clone()),
            )
    }
}

// bench agent: Bench's controls in each of their states.
fn bench_controls() -> Div {
    use crate::view::controls::Kind;
    let toggle =
        |id: &'static str, text: &'static str, name: &'static str, icon: Icon, on: bool| {
            Button::new(id, text)
                .named(name)
                .kind(if id == "gallery-blind" {
                    Kind::Toggle
                } else {
                    Kind::Vote
                })
                .with_icon(icon, 13.)
                .icon_only_unless(on)
                .toggled(on)
                .pressed(on)
                .tip(name)
                .on_press(|_, _| {})
        };
    div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(px(8.))
        .child(toggle(
            "gallery-vote",
            "",
            "Pick Build A as the best build",
            Icon::Trophy,
            false,
        ))
        .child(toggle(
            "gallery-picked",
            "Best",
            "Take back the pick of Build B",
            Icon::Trophy,
            true,
        ))
        .child(toggle(
            "gallery-tie",
            "Tie",
            "Take back the tie",
            Icon::Equal,
            true,
        ))
        .child(toggle(
            "gallery-blind",
            "Blind",
            "Blind",
            Icon::EyeOff,
            true,
        ))
        .child(
            div()
                .flex()
                .gap(px(2.))
                .p(px(2.))
                .rounded(px(8.))
                .bg(crate::view::theme::colour::well())
                .shadow(crate::view::theme::shadow::line_ring())
                .child(
                    Button::new("gallery-round-1", "1")
                        .named("Round 1")
                        .kind(Kind::Segment)
                        .toggled(true)
                        .pressed(true)
                        .on_press(|_, _| {}),
                )
                .child(
                    Button::new("gallery-round-2", "2")
                        .named("Round 2")
                        .kind(Kind::Segment)
                        .toggled(false)
                        .on_press(|_, _| {}),
                ),
        )
        .child(Button::icon("gallery-retry", Icon::Refresh, "Retry Build C").on_press(|_, _| {}))
}
