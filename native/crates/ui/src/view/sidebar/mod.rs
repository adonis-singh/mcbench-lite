//! The sidebar (inventory `app.md` 4, `style-viewer-routes.md` 1.5): the
//! brand row (the app's mark, the mode switcher with the other mode's
//! finished or working mark, New chat or New bench, the sidebar's toggle),
//! the search with its filters ([`find`]), the mode's sections (Build's
//! chat list [`chats`]; Bench's leaderboard row and bench list
//! [`benches`], and its prompts [`prompts`]), the mode's archived section
//! ([`archived`]), and Settings at its foot. Search everything, the palette
//! ([`palette`]), is the sidebar's too: the shell draws it over the window.
//!
//! A section is a file of its own in this folder drawing into the render
//! below. A section reads the lists from [`super::shell::lists::Lists`] and
//! navigates through the shell ([`super::shell::Shell::go`]); what it asks
//! of the core goes through `Sidebar::send`.
//!
//! The sidebar also marks seen what is on screen (`app.md` 2.7 and 7.7,
//! [`crate::state::seen`]): the open chat while it is unread, at a turn's
//! end, and every unread model of the bench whose comparison is open, each
//! only while the window is visible. Visible is GPUI's
//! `Window::is_visible` (`WindowVisibility`): the platform presents the
//! window's frames, so a minimised window, or one on another Space or
//! virtual desktop, is not; on macOS (`NSWindow.occlusionState`) neither is
//! one fully covered, on Windows (`WS_VISIBLE` and not iconic) and under a
//! compositing X11 window manager a covered window still is, on Wayland it
//! is the toplevel's `suspended` state (a compositor without it never
//! reports hidden). It is not whether the window is active: a window
//! beside the active one is looked at, as a page in a visible tab is.

pub mod archived;
pub mod benches;
pub mod chats;
pub mod dot;
pub mod find;
pub mod palette;
pub mod parts;
pub mod prompts;

use std::collections::HashSet;
use std::sync::Arc;

use crate::state::seen::{Listed, Look, Seen};
use std::time::Duration;

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::OpenChat;
use super::controls::{Button, fonts};
use super::icons::{self, Icon};
use super::keys;
use super::shell::Shell;
use super::shell::announce::Announcer;
use super::shell::lists::Lists;
use super::theme::{self, TextKind, colour, mix};
use crate::state::keymap::BindAction;
use crate::state::place::{Mode, Place};
use crate::state::sidebar::{DotHistory, other_mode, switch_title};
use crate::{ActionLog, Command, Core, Provider, SearchScope};

actions!(
    sidebar,
    [
        /// Escape in the sidebar's search: clears it, leaves the field,
        /// and goes on to the window's Escape.
        FindEscape,
        /// Escape in the prompts' search: ends it.
        PromptFindEscape,
        /// Up in the palette's field.
        PaletteUp,
        /// Down in the palette's field.
        PaletteDown,
        /// Enter in the palette's field.
        PaletteRun,
    ]
);

/// The sidebar's keys (`view::bind_keys` calls it).
pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("escape", FindEscape, Some("SideFind > Input")),
        KeyBinding::new("escape", PromptFindEscape, Some("PromptFind > Input")),
        KeyBinding::new("up", PaletteUp, Some("Palette > Input")),
        KeyBinding::new("down", PaletteDown, Some("Palette > Input")),
        KeyBinding::new("enter", PaletteRun, Some("Palette > Input")),
        KeyBinding::new("escape", super::CloseLayer, Some("Palette > Input")),
        KeyBinding::new("space", OpenChat, Some("ChatList")),
    ]);
}

/// The mode word's arrival (`mode-swap 700ms ease-out`).
pub const MODE_SWAP: Duration = Duration::from_millis(700);

/// The sidebar.
pub struct Sidebar {
    pub(crate) shell: WeakEntity<Shell>,
    pub(crate) lists: Entity<Lists>,
    /// The place on screen (the open chat is marked current).
    pub(crate) place: Place,
    pub(crate) mode: Mode,
    pub(crate) chats: chats::ChatList,
    pub(crate) benches: benches::BenchList,
    pub(crate) find: find::Find,
    // chat-fix: a failed search's sentence, selectable.
    pub(crate) find_failed_text: crate::view::controls::selectable::Selectable,
    // chat-fix: whether animations for looks move (`view::motion`).
    pub(crate) moving: bool,
    pub(crate) archive: archived::Archived,
    pub(crate) prompts: prompts::Prompts,
    pub(crate) palette: Entity<palette::Palette>,
    pub(crate) providers: Vec<Provider>,
    // par-chat: the providers' read failed (`config.phase === 'failed'`),
    // and its `Try again`'s focus.
    pub(crate) models_failed: bool,
    pub(crate) models_retry: Option<FocusHandle>,
    pub(crate) dots: DotHistory,
    pub(crate) announcer: Option<Entity<Announcer>>,
    // chat-fix-2: the failed search's sentence last said by the field and
    // by the palette (each says it once).
    pub(crate) said_failure: (Option<String>, Option<String>),
    /// The running dots the pulse layer draws (the shell's).
    pub(crate) pulses: dot::Pulses,
    /// How many places were shown (a restore opens its item only if none
    /// was meanwhile).
    pub(crate) navigations: u64,
    /// The sidebar's width, for the filter menu.
    pub(crate) width: f32,
    core: Option<Arc<dyn Core>>,
    log: Option<ActionLog>,
    requests: u64,
    seen: Seen,
    /// The switcher was pressed once (the page's `swapped`): every later
    /// switch flashes the new word.
    swapped: bool,
    swaps: u32,
    listed: bool,
    renders: usize,
    /// A fixed now for the time left (tests); the clock when none.
    now: Option<i64>,
    mode_focus: FocusHandle,
    _observe: Subscription,
}

impl Sidebar {
    /// The sidebar over `lists`, navigating through `shell`.
    pub fn new(shell: WeakEntity<Shell>, lists: Entity<Lists>, cx: &mut Context<Self>) -> Self {
        let observe = cx.observe(&lists, |_, _, cx| cx.notify());
        let me = cx.entity().downgrade();
        let palette = cx.new(|cx| palette::Palette::new(me.clone(), cx));
        let toggle = me.clone();
        keys::on_bind(cx, BindAction::Search, move |window, cx| {
            let _ = toggle.update(cx, |s, cx| s.toggle_palette(window, cx));
        });
        Sidebar {
            shell,
            lists,
            place: Place::NewChat { id: None },
            mode: Mode::Build,
            chats: chats::ChatList::default(),
            benches: benches::BenchList::default(),
            find: find::Find::default(),
            find_failed_text: crate::view::controls::selectable::Selectable::new(cx),
            moving: false,
            archive: archived::Archived::default(),
            prompts: prompts::Prompts::default(),
            palette,
            providers: Vec::new(),
            models_failed: false,
            models_retry: None,
            dots: DotHistory::default(),
            announcer: None,
            said_failure: (None, None),
            pulses: dot::Pulses::default(),
            navigations: 0,
            width: crate::state::layout::SIDEBAR_WIDTH,
            core: None,
            log: None,
            requests: 0,
            seen: Seen::default(),
            swapped: false,
            swaps: 0,
            listed: false,
            renders: 0,
            now: None,
            mode_focus: cx.focus_handle().tab_stop(true),
            _observe: observe,
        }
    }

    /// The shell gives the sidebar the core, the action log and the live
    /// regions.
    pub fn attach(
        &mut self,
        core: Arc<dyn Core>,
        log: Option<ActionLog>,
        announcer: Entity<Announcer>,
        pulses: dot::Pulses,
        preferences: &crate::Preferences,
        cx: &mut Context<Self>,
    ) {
        self.pulses = pulses;
        self.benches.open = preferences.open_benches.clone();
        self.prompts.folded = !preferences.prompts_open;
        self.core = Some(core);
        self.palette.update(cx, |p, _| p.log = log.clone());
        self.log = log;
        self.announcer = Some(announcer);
    }

    /// Hands a command to the core.
    // par-chat (stage four's parity pass)
    /// Whether the providers' read failed (`Update::Providers(Err)`).
    pub fn set_models_failed(&mut self, failed: bool, cx: &mut Context<Self>) {
        self.models_failed = failed;
        cx.notify();
    }

    /// `Models failed to load.` with `Try again` (`role="alert"`,
    /// `web/app.js` 6636-6638): `Try again` reads the providers again and
    /// the line goes while that read is on its way (`loadConfig`'s
    /// `loading`).
    fn models_failed_line(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.models_failed {
            return None;
        }
        let retry = self
            .models_retry
            .get_or_insert_with(|| cx.focus_handle().tab_stop(true))
            .clone();
        let again = |this: &mut Self, cx: &mut Context<Self>| {
            this.models_failed = false;
            this.log("read providers", serde_json::json!({}));
            this.send(Command::ReadProviders);
            cx.notify();
        };
        Some(
            parts::side_empty("Models failed to load. ", cx)
                .id("models-failed")
                .role(Role::Alert)
                .aria_label("Models failed to load.")
                .flex()
                .child(
                    super::ring(
                        div()
                            .id("models-retry")
                            .role(Role::Button)
                            .aria_label("Try again")
                            .track_focus(&retry)
                            .key_context("Part")
                            .on_click(cx.listener(move |this, _, _, cx| again(this, cx)))
                            .on_action(
                                cx.listener(move |this, _: &super::Activate, _, cx| {
                                    again(this, cx)
                                }),
                            ),
                    )
                    .text_color(colour::ash())
                    .underline()
                    .hover(|s| s.text_color(colour::bone()))
                    .cursor_pointer()
                    .child("Try again"),
                )
                .into_any_element(),
        )
    }
    // end of par-chat's block

    pub(crate) fn send(&self, command: Command) {
        self.core
            .as_ref()
            .expect("the shell attached the sidebar to the core")
            .send(command);
    }

    pub(crate) fn log(&self, action: &str, fields: serde_json::Value) {
        if let Some(log) = &self.log {
            log.record(action, fields);
        }
    }

    /// Asks the core to search; the request's number.
    pub(crate) fn search(
        &mut self,
        scope: SearchScope,
        query: String,
        _cx: &mut Context<Self>,
    ) -> u64 {
        self.requests += 1;
        let request = self.requests;
        self.send(Command::Search {
            scope,
            query,
            request,
        });
        request
    }

    /// The providers (`Update::Providers`): the filter labels and the
    /// palette's Settings rows name them.
    pub fn set_providers(&mut self, providers: Vec<Provider>, cx: &mut Context<Self>) {
        self.providers = providers;
        cx.notify();
    }

    /// The shell says which place is on screen.
    pub fn set_place(
        &mut self,
        place: Place,
        mode: Mode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if place != self.place {
            self.navigations += 1;
        }
        let switched = mode != self.mode;
        self.place = place;
        self.mode = mode;
        if switched {
            // The search text and the filters are the mode's own.
            self.clear_find(true, window, cx);
            if self.swapped {
                self.swaps += 1;
            }
        }
        self.mark_seen(window, cx);
        cx.notify();
    }

    /// Fixes now (epoch ms) for the archived rows' time left: a test's
    /// clock, so no test depends on today's date.
    pub fn set_now(&mut self, now: i64, cx: &mut Context<Self>) {
        self.now = Some(now);
        cx.notify();
    }

    /// Keeps which benches are open and whether the prompts are folded in
    /// the preferences (saved by the shell).
    pub(crate) fn keep_folds(&self, cx: &mut Context<Self>) {
        let Some(shell) = self.shell.upgrade() else {
            return;
        };
        let (open, prompts_open) = (self.benches.open.clone(), !self.prompts.folded);
        cx.defer(move |cx| shell.update(cx, |s, _| s.keep_sidebar(open, prompts_open)));
    }

    /// How many times the sidebar has rendered.
    pub fn render_count(&self) -> usize {
        self.renders
    }

    /// The open chat's id (a Build chat, the new chat's row, a bench
    /// model's chat).
    pub(crate) fn open_chat(&self) -> Option<String> {
        match &self.place {
            Place::Chat { id } => Some(id.clone()),
            Place::NewChat { id: Some(id) } => Some(id.clone()),
            Place::Member { member, .. } => Some(member.clone()),
            _ => None,
        }
    }

    /// Marks seen what is on screen (`POST .../seen`, `app.md` 2.7 and
    /// 7.7, [`crate::state::seen`] cases 1 and 3): the open chat and, on a
    /// comparison, the bench's models, while the window is visible.
    fn mark_seen(&mut self, window: &Window, cx: &mut Context<Self>) {
        let lists = self.lists.read(cx);
        let unread = |id: &str| lists.chat(id).map(|c| (id.to_string(), c.unread));
        let open = self.open_chat().and_then(|id| unread(&id));
        let compare = match &self.place {
            Place::Compare { bench } => Some((
                bench.clone(),
                lists
                    .all()
                    .iter()
                    .filter(|c| c.summary.bench_id.as_deref() == Some(bench.as_str()))
                    .map(|c| (c.summary.id.0.clone(), c.summary.unread))
                    .collect(),
            )),
            _ => None,
        };
        let marks = self.seen.look(Look {
            visible: window.is_visible(),
            open,
            compare,
        });
        for chat in marks {
            self.send(Command::MarkSeen { chat });
        }
    }

    /// A new list arrived (the shell calls it at once, before anything
    /// looks at it): the marks it answers end.
    pub fn seen_listed(&mut self, cx: &mut Context<Self>) {
        let lists = self.lists.read(cx);
        self.seen.listed(lists.all().iter().map(|c| Listed {
            id: &c.summary.id.0,
            unread: c.summary.unread,
            running: c.running_since.is_some(),
        }));
    }

    /// The core published an error: the next list ends the marks on their
    /// way (a failed mark ends in `Error`, then the list).
    pub fn seen_error(&mut self) {
        self.seen.error();
    }

    /// The window's visibility changed: the open chat or the comparison is
    /// looked at again.
    pub fn visibility_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.mark_seen(window, cx);
    }

    /// A turn of the open chat ended (`turnEnded`, case 2): marked seen if
    /// the window is visible.
    pub fn turn_ended(&mut self, chat: String, window: &mut Window, _cx: &mut Context<Self>) {
        if self.seen.turn_ended(&chat, window.is_visible()) {
            self.send(Command::MarkSeen { chat });
        }
    }

    /// The list changed (the shell calls it after each new list).
    pub fn lists_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.listed {
            self.listed = true;
            // The library is read once the core is attached and listing.
            self.send(Command::ReadPrompts);
        }
        let listed: HashSet<String> = self
            .lists
            .read(cx)
            .benches()
            .iter()
            .map(|b| b.id.clone())
            .collect();
        let before = self.benches.open.len();
        self.benches.open.retain(|id| listed.contains(id));
        if self.benches.open.len() != before {
            self.keep_folds(cx);
        }
        self.mark_seen(window, cx);
        self.archive_lists_changed(window, cx);
        cx.notify();
    }

    pub(crate) fn go(&self, place: Place, window: &mut Window, cx: &mut App) {
        if let Some(shell) = self.shell.upgrade() {
            shell.update(cx, |s, cx| s.go(place, window, cx));
        }
    }

    fn switch_mode(&mut self, mode: Mode, window: &mut Window, cx: &mut Context<Self>) {
        self.swapped = true;
        if let Some(shell) = self.shell.upgrade() {
            shell.update(cx, |s, cx| s.switch_mode(mode, window, cx));
        }
    }

    fn brand(&self, look: SidebarLook, cx: &mut Context<Self>) -> AnyElement {
        let fonts = fonts(cx);
        let mode = self.mode;
        let other = mode.other();
        let other_bind = match other {
            Mode::Build => BindAction::Build,
            Mode::Bench => BindAction::Bench,
        };
        let lists = self.lists.read(cx);
        let (waiting, dot) = other_mode(mode, lists.all(), lists.benches());
        // `ModeSwitcher`: `mc` and the mode word; its title says the switch,
        // the other mode's finished or working items, and its shortcut.
        let title = switch_title(mode, waiting, dot, &keys::shown(cx, other_bind));
        let switch_dot = dot.map(|d| {
            div().absolute().top(px(-3.)).right(px(-5.)).child(dot::dot(
                &format!("switch:{}", other.word()),
                d,
                None,
                6.,
                self.pulses.clone(),
                self.moving,
            ))
        });
        let switcher = super::ring(
            div()
                .id("mode-switcher")
                .role(Role::Button)
                .aria_label(format!("mc{}", mode.word()))
                .aria_description(title.clone())
                .track_focus(&self.mode_focus)
                .key_context("Part")
                .on_click(
                    cx.listener(move |this, _, window, cx| this.switch_mode(other, window, cx)),
                )
                .on_action(cx.listener(move |this, _: &super::Activate, window, cx| {
                    this.switch_mode(other, window, cx)
                })),
        );
        let word = theme::text(div(), TextKind::ModeWord, &fonts)
            .id(ElementId::Name(format!("mode-word-{}", self.swaps).into()))
            .hover(|s| s.text_color(colour::bone()))
            .child(mode.word());
        let word = if self.swaps > 0 {
            // A new word arrives pure white, then settles (0% to 25% white).
            word.with_animation(
                ElementId::Name(format!("mode-swap-{}", self.swaps).into()),
                Animation::new(MODE_SWAP),
                |d, t| {
                    let p = theme::palette();
                    let settle = ((t - 0.25) / 0.75).clamp(0., 1.);
                    let eased = 1. - (1. - settle) * (1. - settle);
                    let to = theme::fade(p.bone, 0.58);
                    d.text_color(Hsla::from(mix(to, eased, p.bone)))
                },
            )
            .into_any_element()
        } else {
            word.into_any_element()
        };
        let switcher = theme::text(switcher, TextKind::Wordmark, &fonts)
            .flex()
            .items_center()
            .rounded(px(theme::radius::ICON))
            .cursor_pointer()
            .child(div().child("mc"))
            .child(word)
            .child(
                div()
                    .relative()
                    .ml(px(5.))
                    .text_color(colour::mode_word())
                    .child(icons::icon(Icon::Swap, 12.))
                    .children(switch_dot),
            );
        let switcher = super::controls::tip(switcher, "mode-switcher-tip", title);
        let new_label = match mode {
            Mode::Build => "New chat",
            Mode::Bench => "New bench",
        };
        let new_place = match mode {
            Mode::Build => Place::NewChat { id: None },
            Mode::Bench => Place::NewBench,
        };
        let shell_new = self.shell.clone();
        let shell_toggle = self.shell.clone();
        let new = Button::icon("new-chat", Icon::Compose, new_label).on_press(move |window, cx| {
            if let Some(shell) = shell_new.upgrade() {
                let place = new_place.clone();
                // par-chat: New chat's model check.
                shell.update(cx, |s, cx| s.new_here(place, window, cx));
            }
        });
        // narrow agent: the button is named for what it does
        // (`sidebarLabel`), an x while the drawer shows, and says whether
        // the drawer is open; one focus handle in every look.
        let label = crate::state::layout::sidebar_toggle_label(
            look.width,
            look.drawer,
            mode == Mode::Bench,
        );
        let toggle = Button::icon(
            "sidebar-toggle",
            if look.drawer { Icon::X } else { Icon::Panel },
            label,
        )
        .focus(look.toggle.clone());
        let toggle = if look.narrow {
            toggle.expanded(look.drawer)
        } else {
            toggle
        };
        let pressed = look.toggle.clone();
        let toggle = toggle.on_press(move |window, cx| {
            // A press is on the button: the drawer it opens gives focus
            // back to it.
            window.focus(&pressed, cx);
            if let Some(shell) = shell_toggle.upgrade() {
                shell.update(cx, |s, cx| s.toggle_sidebar(window, cx));
            }
        });
        if look.narrow {
            // `.brand` at 980 px and below: no padding, a 6 px gap, its
            // actions joining it with the button first (`order: -1`).
            return div()
                .min_h(px(38.))
                .flex()
                .items_center()
                .gap(px(6.))
                .child(toggle)
                .child(icons::app_mark(28.))
                .child(switcher)
                .child(new)
                .into_any_element();
        }
        div()
            .min_h(px(40.))
            .flex()
            .items_center()
            .gap(px(9.))
            .pt(px(2.))
            .px(px(4.))
            .pb(px(4.))
            .child(icons::app_mark(28.))
            .child(switcher)
            .child(div().ml_auto().flex().child(new).child(toggle))
            .into_any_element()
    }
}

impl Render for Sidebar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.renders += 1;
        self.moving = crate::view::motion::moving(window, cx);
        self.ensure_find(window, cx);
        // Its running dots are recorded again as it paints.
        dot::clear(&self.pulses);
        // narrow agent: how the sidebar is drawn: a column (wide), a bar or
        // a drawer (narrow).
        let look = {
            let shell = self.shell.upgrade().expect("the sidebar's shell");
            let shell = shell.read(cx);
            self.width = shell.layout().sidebar_width;
            SidebarLook {
                width: shell.width(),
                narrow: shell.is_narrow(),
                drawer: shell.drawer_shown(),
                toggle: shell.toggle_focus(),
            }
        };
        let brand = self.brand(look.clone(), cx);
        if look.narrow && !look.drawer {
            return self.bar(brand, cx);
        }
        let (offer, labels) = self.offer(cx);
        let shown = self.shown_filters(&offer);
        let narrowed = self.find.found.is_some() || shown.filtering();
        let find = self.find_row(&offer, &labels, &shown, window, cx);
        let chips = self.chips(&labels, &shown, cx);
        let mut sections: Vec<AnyElement> = Vec::new();
        match self.mode {
            Mode::Build => {
                sections.push(self.chat_list(&shown, narrowed, window, cx));
                sections.extend(self.archived_section(Mode::Build, cx));
            }
            Mode::Bench => {
                sections.extend(self.bench_list(&shown, narrowed, window, cx));
                sections.extend(self.archived_section(Mode::Bench, cx));
                sections.extend(self.prompts_section(window, cx));
                // par-chat: under the prompts, before the space.
                sections.extend(self.models_failed_line(cx));
                // The space under Bench's sections.
                sections.push(div().flex_1().into_any_element());
            }
        }
        // par-chat: in Build after the archived section (the page's order).
        if self.mode == Mode::Build {
            sections.extend(self.models_failed_line(cx));
        }
        let shell = self.shell.clone();
        let settings =
            Button::icon("settings", Icon::Settings, "Settings").on_press(move |window, cx| {
                if let Some(shell) = shell.upgrade() {
                    shell.update(cx, |s, cx| s.open_settings(window, cx));
                }
            });
        div()
            .size_full()
            .flex()
            .flex_col()
            .gap(px(8.))
            .pt(px(10.))
            .px(px(9.))
            .pb(px(9.))
            .bg(colour::bedrock())
            .border_r_1()
            .border_color(colour::line())
            .child(brand)
            .child(find)
            .children(chips)
            .children(sections)
            .child(
                div()
                    .mt_auto()
                    .flex()
                    .justify_start()
                    // narrow agent: in the drawer `margin: auto 0 0`.
                    .when(!look.drawer, |d| d.pt(px(6.)))
                    .pl(px(2.))
                    .child(settings),
            )
            .into_any_element()
    }
}

// narrow agent: the sidebar at 980 px and below.
/// How the sidebar is drawn this frame.
#[derive(Clone)]
struct SidebarLook {
    /// The window's width.
    width: f32,
    /// The window is narrow: a bar, or the drawer.
    narrow: bool,
    /// The drawer shows: the column over the page.
    drawer: bool,
    /// The sidebar button's focus handle.
    toggle: FocusHandle,
}

impl Sidebar {
    /// The bar (`.layout > .sidebar` at 980 px and below): the brand and
    /// Settings in one row, `gap: 6px`, padding `6px 10px`, a line under
    /// it; the list is in the drawer.
    fn bar(&self, brand: AnyElement, cx: &mut Context<Self>) -> AnyElement {
        let shell = self.shell.clone();
        let settings =
            Button::icon("settings", Icon::Settings, "Settings").on_press(move |window, cx| {
                if let Some(shell) = shell.upgrade() {
                    shell.update(cx, |s, cx| s.open_settings(window, cx));
                }
            });
        let _ = cx;
        div()
            .size_full()
            .flex()
            .items_center()
            .gap(px(6.))
            .px(px(10.))
            .py(px(6.))
            .bg(colour::bedrock())
            .border_b_1()
            .border_color(colour::line())
            .child(brand)
            .child(div().ml_auto().flex().child(settings))
            .into_any_element()
    }
}
