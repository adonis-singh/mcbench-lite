//! Search everything, the palette (`.cmdk`, inventory `app.md` 4.2 and
//! 11.2, `style-viewer-routes.md` 1.16): the `search` bind (Mod+K) opens it
//! over the window as the `command` layer and closes it again; Escape, a
//! press on the scrim outside it, or running an item close it. Its field
//! has focus before its first frame, so no key typed after the shortcut is
//! lost. Builds and benches come from the core's search, both scopes, 90 ms
//! after the last key; an answer for an older query is dropped. Up and
//! Down move the highlight, wrapping round; Enter runs the highlighted
//! item; moving the pointer over an item highlights it. The groups, their
//! ranking and limits are [`crate::state::palette`]. The highlight is a
//! selected option of the results' list box, and a reader hears it: the
//! window's polite region says its title whenever it moves.

use std::time::Duration;

use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::super::controls::fonts;
use super::super::icons::{self, Icon};
use super::super::keys;
use super::super::theme::{self, TextKind, colour, radius, shadow};
use super::{PaletteDown, PaletteRun, PaletteUp, Sidebar};
use crate::state::command::{highlighted, stepped};
use crate::state::keymap::BindAction;
use crate::state::layers::Layer;
use crate::state::palette::{self, Detail, Glyph, Group, Item, Run, Sources};
use crate::state::place::Place;
use crate::{SearchHit, SearchScope};

/// The palette's wait after the last key.
pub const DEBOUNCE: Duration = Duration::from_millis(90);

/// The palette.
pub struct Palette {
    sidebar: WeakEntity<Sidebar>,
    open: bool,
    input: Option<Entity<InputState>>,
    query: String,
    active: Option<String>,
    found_build: Option<Vec<SearchHit>>,
    found_bench: Option<Vec<SearchHit>>,
    // chat-fix: a failed search's sentence, selectable.
    failed: Option<String>,
    failed_text: crate::view::controls::selectable::Selectable,
    requests: (Option<u64>, Option<u64>),
    typed: u64,
    /// The Settings row the last setting item asked for (Settings' panes
    /// are a later milestone; they scroll to it and light it up).
    pub asked_setting: Option<String>,
    announced: Option<String>,
    opens: u32,
    scroll: ScrollHandle,
    /// Where user-visible actions are recorded.
    pub log: Option<crate::ActionLog>,
    _subscription: Option<Subscription>,
}

impl Palette {
    /// A closed palette for `sidebar`.
    pub fn new(sidebar: WeakEntity<Sidebar>, cx: &mut App) -> Self {
        Palette {
            sidebar,
            open: false,
            input: None,
            query: String::new(),
            active: None,
            found_build: None,
            found_bench: None,
            failed: None,
            failed_text: crate::view::controls::selectable::Selectable::new(cx),
            requests: (None, None),
            typed: 0,
            asked_setting: None,
            announced: None,
            opens: 0,
            scroll: ScrollHandle::new(),
            log: None,
            _subscription: None,
        }
    }

    /// Open.
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// The `search` bind: opens a closed palette, closes an open one.
    pub fn toggle(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.open {
            self.close(window, cx);
        } else {
            self.show(window, cx);
        }
    }

    /// Opens it with an empty query, its field focused.
    pub fn show(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let input = match &self.input {
            Some(input) => input.clone(),
            None => {
                let input = cx.new(|cx| {
                    InputState::new(window, cx)
                        .placeholder("Search builds, benches, prompts and settings")
                });
                let subscription = cx.subscribe_in(
                    &input,
                    window,
                    |this: &mut Self, input, event: &InputEvent, _, cx| {
                        if matches!(event, InputEvent::Change) {
                            let text = input.read(cx).value().to_string();
                            this.typed(text, cx);
                        }
                    },
                );
                self._subscription = Some(subscription);
                self.input = Some(input.clone());
                input
            }
        };
        self.open = true;
        self.opens += 1;
        self.query.clear();
        self.active = None;
        self.found_build = None;
        self.found_bench = None;
        self.failed = None;
        self.requests = (None, None);
        self.announced = None;
        input.update(cx, |i, cx| {
            i.set_value("", window, cx);
            i.focus(window, cx);
        });
        let this = cx.entity().downgrade();
        keys::open_layer(cx, Layer::Command, move |window, cx| {
            let _ = this.update(cx, |p, cx| p.close(window, cx));
        });
        self.log("opened", serde_json::json!({ "layer": "command" }));
        cx.notify();
    }

    /// Closes it; focus goes to the composer, as after the window's other
    /// closes.
    pub fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.open {
            return;
        }
        self.open = false;
        self.typed += 1;
        keys::close_layer(cx, Layer::Command);
        self.log("closed", serde_json::json!({ "layer": "command" }));
        if let Some(sidebar) = self.sidebar.upgrade() {
            let shell = sidebar.read(cx).shell.clone();
            cx.defer_in(window, move |_, window, cx| {
                if let Some(shell) = shell.upgrade() {
                    let chat = shell.read(cx).chat();
                    chat.update(cx, |c, cx| c.focus_composer(window, cx));
                }
            });
        }
        cx.notify();
    }

    fn log(&self, action: &str, fields: serde_json::Value) {
        if let Some(log) = &self.log {
            log.record(action, fields);
        }
    }

    /// The query changed: the core is asked 90 ms after the last key.
    fn typed(&mut self, text: String, cx: &mut Context<Self>) {
        self.query = text;
        self.active = None;
        self.found_build = None;
        self.found_bench = None;
        self.failed = None;
        self.requests = (None, None);
        self.typed += 1;
        let query = self.query.trim().to_string();
        cx.notify();
        if query.is_empty() {
            return;
        }
        let typed = self.typed;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(DEBOUNCE).await;
            let _ = this.update(cx, |this, cx| {
                if this.typed != typed || !this.open {
                    return;
                }
                let Some(sidebar) = this.sidebar.upgrade() else {
                    return;
                };
                let (build, bench) = sidebar.update(cx, |s, cx| {
                    (
                        s.search(SearchScope::Build, query.clone(), cx),
                        s.search(SearchScope::Bench, query.clone(), cx),
                    )
                });
                this.requests = (Some(build), Some(bench));
            });
        })
        .detach();
    }

    /// An answer: it lands only for the query on show. Whether it was this
    /// palette's.
    /// A failed search lands as no results and its sentence, shown where
    /// the results would be.
    pub fn found(
        &mut self,
        request: u64,
        results: Result<Vec<SearchHit>, String>,
        cx: &mut Context<Self>,
    ) -> bool {
        // chat-fix-2: an answer to a query no longer on show changes
        // nothing, its failure included.
        let build = self.requests.0 == Some(request);
        if !build && self.requests.1 != Some(request) {
            return false;
        }
        let hits = match results {
            Ok(hits) => hits,
            Err(error) => {
                self.failed = Some(error);
                self.failed_text.clear();
                Vec::new()
            }
        };
        if build {
            self.found_build = Some(hits);
        } else {
            self.found_bench = Some(hits);
        }
        cx.notify();
        true
    }

    fn groups(&self, cx: &App) -> Vec<Group> {
        let Some(sidebar) = self.sidebar.upgrade() else {
            return Vec::new();
        };
        let s = sidebar.read(cx);
        let lists = s.lists.read(cx);
        let chats: Vec<(String, String)> = lists
            .chats()
            .iter()
            .map(|c| (c.id.0.clone(), c.title.clone()))
            .collect();
        let benches: Vec<(String, String)> = lists
            .benches()
            .iter()
            .map(|b| (b.id.clone(), b.title.clone()))
            .collect();
        let prompts: Vec<palette::Prompt> = s
            .prompts
            .library()
            .iter()
            .map(|p| palette::Prompt {
                id: p.id.clone(),
                title: p.title.clone(),
                text: p.text.clone(),
                tags: p.tags.clone(),
            })
            .collect();
        let providers: Vec<palette::ProviderRow> = s
            .providers
            .iter()
            .map(|p| palette::ProviderRow {
                id: p.id.clone(),
                label: p.label.clone(),
                harness: p.harness,
            })
            .collect();
        let sidebar_open = s
            .shell
            .upgrade()
            .is_some_and(|shell| shell.read(cx).layout().sidebar_open);
        // narrow agent
        let narrow = s
            .shell
            .upgrade()
            .is_some_and(|shell| shell.read(cx).is_narrow());
        let searching = !palette::words(&self.query).is_empty();
        palette::groups(&Sources {
            query: &self.query,
            mode: s.mode,
            chats: &chats,
            benches: &benches,
            found_build: if searching {
                self.found_build.as_deref()
            } else {
                None
            },
            found_bench: if searching {
                self.found_bench.as_deref()
            } else {
                None
            },
            prompts: &prompts,
            providers: &providers,
            sidebar_open,
            narrow,
            light: theme::mode() == theme::Mode::Light,
        })
    }

    fn keys_of(groups: &[Group]) -> Vec<String> {
        groups
            .iter()
            .flat_map(|g| g.items.iter().map(|i| i.key.clone()))
            .collect()
    }

    fn step(&mut self, by: isize, cx: &mut Context<Self>) {
        let keys = Self::keys_of(&self.groups(cx));
        if keys.is_empty() {
            return;
        }
        self.active = stepped(&keys, self.active.as_deref(), by).map(str::to_string);
        if let Some(at) = keys.iter().position(|k| Some(k) == self.active.as_ref()) {
            self.scroll.scroll_to_item(at);
        }
        cx.notify();
    }

    fn run_active(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let groups = self.groups(cx);
        let keys = Self::keys_of(&groups);
        let Some(active) = highlighted(&keys, self.active.as_deref()).map(str::to_string) else {
            return;
        };
        let item = groups
            .iter()
            .flat_map(|g| g.items.iter())
            .find(|i| i.key == active)
            .cloned()
            .expect("the highlighted item is listed");
        self.run(item, window, cx);
    }

    /// Runs an item: the palette closes first.
    fn run(&mut self, item: Item, window: &mut Window, cx: &mut Context<Self>) {
        self.close(window, cx);
        self.log("ran", serde_json::json!({ "item": item.key }));
        let Some(sidebar) = self.sidebar.upgrade() else {
            return;
        };
        let shell = sidebar.read(cx).shell.clone();
        let Some(shell) = shell.upgrade() else {
            return;
        };
        if let Run::Setting(title) = &item.run {
            self.asked_setting = Some(title.clone());
        }
        let run = item.run;
        cx.defer_in(window, move |_, window, cx| {
            shell.update(cx, |s, cx| match run {
                Run::Build(id) => s.go(Place::Chat { id }, window, cx),
                Run::Bench(bench) => s.go(Place::Compare { bench }, window, cx),
                Run::Prompt(id) => {
                    sidebar.update(cx, |sb, _| sb.prompts.active = Some(id));
                    s.go(Place::Prompt, window, cx)
                }
                // par-chat: New chat's model check.
                Run::NewChat => s.new_here(Place::NewChat { id: None }, window, cx),
                Run::NewBench => s.go(Place::NewBench, window, cx),
                Run::Switch(mode) => s.switch_mode(mode, window, cx),
                Run::Leaderboard => s.go(Place::Leaderboard, window, cx),
                Run::Sidebar => s.toggle_sidebar(window, cx),
                Run::Theme => s.flip_theme(cx),
                // settings agent: a setting row opens General and flashes
                // it; a provider row opens its pane.
                Run::Settings => s.open_settings(window, cx),
                Run::Setting(title) => s.open_settings_on(
                    Some(crate::state::settings::Pane::General),
                    Some(title),
                    window,
                    cx,
                ),
                Run::Provider(id) => s.open_settings_on(
                    Some(crate::state::settings::Pane::Provider(id)),
                    None,
                    window,
                    cx,
                ),
            })
        });
    }

    fn glyph(item: &Item) -> AnyElement {
        if let (Glyph::Provider, Some(provider)) = (item.glyph, &item.provider) {
            return icons::logo(crate::state::vendor::logo_name(provider), 15.);
        }
        let Glyph::Icon(name) = item.glyph else {
            unreachable!("a provider row names its provider")
        };
        let icon = match name {
            "build" => Icon::Build,
            "bench" => Icon::Bench,
            "prompts" => Icon::Prompts,
            "compose" => Icon::Compose,
            "swap" => Icon::Swap,
            "trophy" => Icon::Trophy,
            "panel" => Icon::Panel,
            "sun" => Icon::Sun,
            "moon" => Icon::Moon,
            "settings" => Icon::Settings,
            other => panic!("the palette has no icon {other}"),
        };
        icons::icon(icon, 15.).into_any_element()
    }

    /// Text with its runs marked (`mark`: `--bone`, weight 600).
    fn marked(parts: Vec<(String, bool)>) -> StyledText {
        let mut text = String::new();
        let mut highlights = Vec::new();
        for (part, matched) in parts {
            let start = text.len();
            text.push_str(&part);
            if matched {
                highlights.push((
                    start..text.len(),
                    HighlightStyle {
                        color: Some(colour::bone()),
                        font_weight: Some(FontWeight(600.)),
                        ..Default::default()
                    },
                ));
            }
        }
        StyledText::new(text).with_highlights(highlights)
    }

    fn plain(parts: &[(String, bool)]) -> String {
        parts.iter().map(|(t, _)| t.as_str()).collect()
    }
}

impl Render for Palette {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.open {
            return div().into_any_element();
        }
        let fonts = fonts(cx);
        let words = palette::words(&self.query);
        let groups = self.groups(cx);
        let keys = Self::keys_of(&groups);
        let active = highlighted(&keys, self.active.as_deref()).map(str::to_string);
        // The highlight belongs to its item, wherever later groups put it.
        if active != self.active {
            self.active = active.clone();
        }
        let active_title = groups
            .iter()
            .flat_map(|g| g.items.iter())
            .find(|i| Some(&i.key) == active.as_ref())
            .map(|i| i.title.clone());
        if active_title != self.announced {
            self.announced = active_title.clone();
            if let (Some(title), Some(sidebar)) = (active_title, self.sidebar.upgrade()) {
                let announcer = sidebar.read(cx).announcer.clone();
                cx.defer(move |cx| {
                    if let Some(a) = announcer {
                        a.update(cx, |a, cx| a.polite(title, cx));
                    }
                });
            }
        }
        let input = self.input.clone().expect("made when opened");
        let landed = self.found_build.is_some() && self.found_bench.is_some();
        let mut list = div()
            .id("palette-results")
            // It clips its rows: one scrolled out is not showing (W2).
            .a11y_synthetic_children(|builder| {
                builder.parent_node().set_clips_children();
            })
            .role(Role::ListBox)
            .aria_label("Results")
            .aria_size_of_set(keys.len())
            .track_scroll(&self.scroll)
            .overflow_y_scroll()
            .min_h_0()
            .px(px(8.))
            .pb(px(8.))
            .flex()
            .flex_col();
        let searching = !words.is_empty();
        if let Some(error) = self.failed.clone().filter(|_| searching) {
            let note = crate::view::controls::selectable::selectable(
                "palette-failed",
                error,
                font(fonts.ui.clone()),
                colour::err(),
                |p: &mut Self| &mut p.failed_text,
                self,
                cx,
            );
            list = list.child(
                theme::text(note, TextKind::EmptySub, &fonts)
                    .w_full()
                    .pt(px(18.))
                    .pb(px(10.))
                    .px(px(10.))
                    .text_center(),
            );
        }
        if keys.is_empty() && !(searching && self.failed.is_some()) {
            let empty = palette::empty_text(&self.query, landed);
            list = list.child(
                theme::text(
                    div()
                        .id("palette-empty")
                        .role(Role::Label)
                        .aria_label(empty.clone())
                        // chat-fix-2: AT-SPI names a label by its value.
                        .aria_value(empty.clone()),
                    TextKind::EmptySub,
                    &fonts,
                )
                .pt(px(30.))
                .pb(px(26.))
                .px(px(10.))
                .flex()
                .justify_center()
                .child(empty),
            );
        }
        let mut position = 0;
        for (g, group) in groups.iter().enumerate() {
            let mut block = div()
                .id(("palette-group", g))
                .role(Role::Group)
                .aria_label(group.title)
                .flex()
                .flex_col()
                .child(
                    theme::text(div(), TextKind::SettingsNavLabel, &fonts)
                        .pt(px(12.))
                        .px(px(10.))
                        .pb(px(5.))
                        .child(group.title),
                );
            for item in &group.items {
                let is_active = Some(&item.key) == active.as_ref();
                let title_parts = palette::mark_words(&item.title, &words);
                let detail: Option<Vec<(String, bool)>> = item.detail.as_ref().map(|d| match d {
                    Detail::Snippet(s) => crate::state::seam::snippet_parts(s),
                    Detail::Text(t) => palette::mark_words(t, &words),
                    Detail::Excerpt { cut, text } => {
                        let mut parts = palette::mark_words(text, &words);
                        if *cut {
                            parts.insert(0, ("\u{2026}".to_string(), false));
                        }
                        parts
                    }
                });
                let mut name = Self::plain(&title_parts);
                if let Some(d) = &detail {
                    name = format!("{name}, {}", Self::plain(d));
                }
                let hint = item
                    .hint
                    .map(|a: BindAction| keys::shown(cx, a))
                    .filter(|h| !h.is_empty());
                let hover_key = item.key.clone();
                let run = item.clone();
                block =
                    block.child(
                        div()
                            .id(ElementId::Name(format!("palette-{}", item.key).into()))
                            .role(Role::ListBoxOption)
                            .aria_label(name)
                            .aria_selected(is_active)
                            .aria_position_in_set(position)
                            .aria_size_of_set(keys.len())
                            .min_h(px(42.))
                            .px(px(10.))
                            .py(px(7.))
                            .flex()
                            .items_center()
                            .gap(px(11.))
                            .rounded(px(9.))
                            .text_color(colour::ash())
                            .when(is_active, |d| d.bg(colour::raised()))
                            .cursor_pointer()
                            .on_mouse_move(cx.listener(move |this, _, _, cx| {
                                if this.active.as_ref() != Some(&hover_key) {
                                    this.active = Some(hover_key.clone());
                                    cx.notify();
                                }
                            }))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.run(run.clone(), window, cx)
                            }))
                            .child(
                                div()
                                    .flex_none()
                                    .size(px(26.))
                                    .rounded(px(radius::ROW))
                                    .bg(colour::well())
                                    .shadow(shadow::line_ring())
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_color(if is_active {
                                        colour::bone()
                                    } else {
                                        colour::ash()
                                    })
                                    .child(Self::glyph(item)),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .gap(px(1.))
                                    .child(
                                        theme::text(div(), TextKind::SidebarRow, &fonts)
                                            .text_size(px(13.5))
                                            // `.cmdk-title`: the body's 400.
                                            .font_weight(FontWeight(400.))
                                            .text_color(colour::bone())
                                            .truncate()
                                            .child(Self::marked(title_parts)),
                                    )
                                    .when_some(detail, |d, detail| {
                                        d.child(
                                            theme::text(div(), TextKind::RowSnippet, &fonts)
                                                .truncate()
                                                .child(Self::marked(detail)),
                                        )
                                    }),
                            )
                            .when_some(hint, |d, hint| {
                                d.child(
                                    theme::text(div(), TextKind::ResolutionTag, &fonts)
                                        .flex_none()
                                        .px(px(6.))
                                        .py(px(2.))
                                        .rounded(px(5.))
                                        .shadow(shadow::line_ring())
                                        .text_size(px(11.))
                                        .child(hint),
                                )
                            }),
                    );
                position += 1;
            }
            list = list.child(block);
        }
        let dialog = div()
            .id("palette")
            .role(Role::Dialog)
            .aria_label("Search everything")
            .key_context("Palette")
            .on_action(cx.listener(|this, _: &PaletteUp, _, cx| this.step(-1, cx)))
            .on_action(cx.listener(|this, _: &PaletteDown, _, cx| this.step(1, cx)))
            .on_action(cx.listener(|this, _: &PaletteRun, window, cx| this.run_active(window, cx)))
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .w(px(640.))
            .max_w_full()
            // `max-height: min(540px, 72vh)`.
            .max_h(px(
                540_f32.min(0.72 * f32::from(window.viewport_size().height))
            ))
            .flex()
            .flex_col()
            .overflow_hidden()
            .rounded(px(14.))
            .bg(colour::popover())
            .shadow(shadow::panel())
            .child(
                div()
                    .flex_none()
                    .h(px(54.))
                    .pl(px(18.))
                    .pr(px(16.))
                    .flex()
                    .items_center()
                    .gap(px(11.))
                    .text_color(colour::dim())
                    .child(icons::icon(Icon::Search, 17.))
                    .child(
                        theme::text(div().flex_1().min_w_0(), TextKind::PaletteInput, &fonts)
                            .child(
                                Input::new(&input)
                                    .aria_label("Search everything")
                                    .appearance(false)
                                    .text_size(px(15.)),
                            ),
                    )
                    .child(
                        theme::text(div(), TextKind::ResolutionTag, &fonts)
                            .flex_none()
                            .px(px(6.))
                            .py(px(2.))
                            .rounded(px(5.))
                            .shadow(shadow::line_ring())
                            .text_size(px(11.))
                            .child("esc"),
                    ),
            )
            .child(list);
        div()
            .id("palette-scrim")
            .absolute()
            .inset_0()
            .bg(theme::sidebar::palette_scrim())
            .flex()
            .justify_center()
            .items_start()
            // `padding: 14vh 16px 16px` (a percentage would be of the width).
            .pt(px(0.14 * f32::from(window.viewport_size().height)))
            .px(px(16.))
            .pb(px(16.))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| this.close(window, cx)),
            )
            .child(dialog)
            .with_animation(
                ElementId::Name(format!("palette-in-{}", self.opens).into()),
                Animation::new(Duration::from_millis(120)),
                |d, t| d.opacity(t),
            )
            .into_any_element()
    }
}

impl Sidebar {
    /// Opens or closes the palette (the `search` bind).
    pub fn toggle_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // chat-fix-2: a palette opened again says its failure again.
        self.said_failure.1 = None;
        let palette = self.palette.clone();
        palette.update(cx, |p, cx| p.toggle(window, cx));
    }

    /// Routes a search's answer to the field or the palette that asked.
    pub fn found(
        &mut self,
        request: u64,
        results: Result<Vec<SearchHit>, String>,
        cx: &mut Context<Self>,
    ) {
        // chat-fix-2: a failed search's sentence is a status, said once in
        // the polite region when it appears, not again while later queries
        // of the same field (or of the palette) fail the same way; a search
        // that answers ends it.
        let failure = results.as_ref().err().cloned();
        let in_find = self.find_found(request, results.clone());
        let landed = in_find
            || self
                .palette
                .update(cx, |p, cx| p.found(request, results, cx));
        if landed {
            let said = if in_find {
                &mut self.said_failure.0
            } else {
                &mut self.said_failure.1
            };
            match failure {
                Some(sentence) if said.as_ref() != Some(&sentence) => {
                    *said = Some(sentence.clone());
                    if let Some(announcer) = self.announcer.clone() {
                        announcer.update(cx, |a, cx| a.polite(sentence, cx));
                    }
                }
                Some(_) => {}
                None => *said = None,
            }
        }
        cx.notify();
    }

    /// The palette, drawn over the window by the shell.
    pub fn palette(&self) -> Entity<Palette> {
        self.palette.clone()
    }
}
