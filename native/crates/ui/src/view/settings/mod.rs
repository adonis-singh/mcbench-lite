//! Settings: the layer over the window (inventory `app.md` 8.1 and 8.5,
//! `components.md` Part E, `style-viewer-routes.md` 1.10), its four phases
//! ([`crate::state::settings`]), the navigation, the panes, the save error
//! and the actions. Each pane is a file of its own:
//!
//! | Part | File | Page |
//! |---|---|---|
//! | the panel, its navigation and actions | this file, `nav.rs` | `App` 7046-7218, `SettingsNavItem` |
//! | a section and a row | `parts.rs` | `SettingsSection`, `SettingsRow` |
//! | Connect an AI | `connect.rs` | `ConnectPane` |
//! | General (title model, theme, sharing, published builds, shortcuts, danger zone) | `general.rs` | `App`'s General pane, `PublishedBuilds`, `BindsSection`, `DangerZone` |
//! | a provider's pane (account and sign-in, runtime) | `provider.rs` | `ProviderPane`, `SignInRows` |
//! | a provider's models | `models.rs` | `ModelsSection` |
//!
//! Settings open with [`SettingsLayer::open`] (the shell's
//! `open_settings`): the saved settings are read (`Command::ReadSettings`),
//! and the panel draws once they land (the page draws nothing while
//! opening). Focus goes to the panel's active navigation row; Tab stays
//! inside it; Escape (the `settings` layer) or a press outside it closes
//! it, dropping the draft, except while saving; focus then goes back to
//! the control that opened it.
//!
//! The API key a person types is a secret: it lives in the draft
//! ([`crate::state::settings::DraftProvider::api_key`]) and in the save's
//! `ProviderChange`, nowhere else. Its field is a password field (AccessKit
//! `PasswordInput`): it draws dots, exposes no value and no text to
//! assistive technology, refuses copy, cut and keyboard selection, and
//! takes a paste.

pub use parts::text_label;

mod connect;
mod gallery;
mod general;
mod models;
mod nav;
mod parts;
mod provider;

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::controls::{Button, Kind, fonts};
use super::icons::{self, Icon};
use super::pickers::model::{Look, ModelPicker};
use super::shell::announce::Announcer;
use super::theme::{self, TextKind, colour, shadow};
use super::{FocusNext, FocusPrevious, keys};
use crate::state::keymap::BindAction;
use crate::state::layers::Layer;
use crate::state::model_list::Drag;
use crate::state::published::{self, Published};
use crate::state::settings::{self as panel, Draft, Pane, Panel, Votes};
use crate::state::sign_in::{self, Login};
use crate::{ActionLog, Command, Core, Update};

/// How long a published build's `Copy link` reads `Copied` (`useCopy`).
const COPIED_FOR: std::time::Duration = std::time::Duration::from_millis(1500);
/// How long a row reached from the palette flashes (`setting-flash`).
const FLASH_FOR: std::time::Duration = std::time::Duration::from_millis(1400);
/// The window width at and below which the panel stacks its navigation
/// above its pane (`@media (max-width: 980px)`).
pub const NARROW: f32 = 980.;
/// The models list's row pitch (`.s-model`, `min-height: 40px`).
pub const MODEL_ROW: f32 = 40.;

/// The panes' key contexts and their keys.
pub fn bind_keys(cx: &mut App) {
    use super::{MoveFirst, MoveLast, MoveNext, MovePrevious};
    cx.bind_keys([
        KeyBinding::new("up", MovePrevious, Some("SettingsNav")),
        KeyBinding::new("down", MoveNext, Some("SettingsNav")),
        KeyBinding::new("home", MoveFirst, Some("SettingsNav")),
        KeyBinding::new("end", MoveLast, Some("SettingsNav")),
        KeyBinding::new("up", MovePrevious, Some("SettingsGrip")),
        KeyBinding::new("down", MoveNext, Some("SettingsGrip")),
    ]);
}

/// Told a text (a notice).
pub type OnText = Rc<dyn Fn(String, &mut App)>;
/// Told whether the theme is light.
pub type OnTheme = Rc<dyn Fn(bool, &mut App)>;
/// Told the shortcuts in force.
pub type OnBinds = Rc<dyn Fn(crate::state::keymap::Keymap, &mut App)>;
/// Told a published build's subject to open.
pub type OnGoTo = Rc<dyn Fn(published::Subject, &mut Window, &mut App)>;

/// What the shell does for Settings.
#[derive(Clone)]
pub struct Hooks {
    /// A notice (`showError`).
    pub notice: OnText,
    /// Settings closed (focus is back; the shell may open the chat model
    /// picker, `app.md` 2.5).
    pub closed: super::Press,
    /// The theme: light or dark, applied and saved at once.
    pub theme: OnTheme,
    /// The shortcuts, applied and saved at once.
    pub binds: OnBinds,
    /// A published build's chat or bench, opened (Settings close first).
    pub go_to: OnGoTo,
    /// Whether the theme is light now.
    pub light: Rc<dyn Fn(&App) -> bool>,
}

/// A model dragged by its grip.
#[derive(Clone, Debug)]
struct Dragging {
    model: String,
    start_y: f32,
    drag: Drag,
}

/// The Settings layer.
pub struct SettingsLayer {
    core: Arc<dyn Core>,
    log: Option<ActionLog>,
    opener: super::Opener,
    announcer: Entity<Announcer>,
    hooks: Hooks,
    panel: Panel,
    /// The row a palette item asked for, and when it started to flash.
    flash: Option<(String, Option<Instant>)>,
    published: Published,
    votes: Votes,
    votes_armed: bool,
    clearing: bool,
    logins: HashMap<String, Login>,
    /// A sign-in asked for from Connect an AI: its provider and method.
    start_with: Option<(String, Option<String>)>,
    recording: Option<BindAction>,
    /// The recording a press ended (its own button's click then does not
    /// start it again).
    ended_by_press: Option<BindAction>,
    /// What had focus when Settings opened.
    opened_from: Option<FocusHandle>,
    focus: FocusHandle,
    nav: HashMap<String, FocusHandle>,
    /// The navigation row that is the Tab stop (the focused one, else the
    /// active pane's).
    nav_current: Option<String>,
    keys_inputs: HashMap<String, Entity<InputState>>,
    repo: Option<Entity<InputState>>,
    filter: Option<Entity<InputState>>,
    filter_for: Option<String>,
    code: Option<Entity<InputState>>,
    /// The code field empties at the next frame (a code was sent).
    clear_code: bool,
    /// The window is 980 px wide or less (the stacked layout).
    narrow: bool,
    /// The sign-in's button, then its Cancel, then its install's status:
    /// one focus, kept as the rows change under it.
    sign_in_focus: FocusHandle,
    /// `Clear leaderboard`, then the confirm's `Cancel`: one focus.
    votes_focus: FocusHandle,
    grips: HashMap<String, FocusHandle>,
    dragging: Option<Dragging>,
    title_picker: Option<Entity<ModelPicker>>,
    effort: Entity<super::controls::popover::PopoverState>,
    scroll: ScrollHandle,
    copied: Option<(String, Instant)>,
    /// When the `Checked` text is next redrawn.
    clock: Option<(i64, Task<()>)>,
    /// The flashed row's bounds, for scrolling it into the middle.
    flash_bounds: Rc<std::cell::Cell<Option<Bounds<Pixels>>>>,
    /// Frame counts, for the dev binary's timings: when Settings were
    /// asked to open, and the key or press that started a filter.
    pending_open: Option<Instant>,
    pending_key: Option<Instant>,
    pending_drag: Option<Instant>,
    /// Frames painted since the panel opened (logged when it closes).
    frames: u64,
    /// Every key reaching the window (the filter's key-to-paint timing).
    _keys: Subscription,
    subscriptions: Vec<Subscription>,
    // par-shell (parity, `app.md` 8.1): the opening read held until the
    // title model's facts are in (`readSettings` loads them before
    // `open`), and the catalogue watched meanwhile.
    opening_read: Option<crate::SettingsView>,
    facts_wait: Option<Subscription>,
}

impl SettingsLayer {
    /// A closed layer.
    pub fn new(
        core: Arc<dyn Core>,
        log: Option<ActionLog>,
        opener: super::Opener,
        announcer: Entity<Announcer>,
        hooks: Hooks,
        cx: &mut Context<Self>,
    ) -> Self {
        SettingsLayer {
            core,
            log,
            opener,
            announcer,
            hooks,
            panel: Panel::Closed,
            flash: None,
            published: published::UNREAD,
            votes: Votes::Counting,
            votes_armed: false,
            clearing: false,
            logins: HashMap::new(),
            start_with: None,
            recording: None,
            ended_by_press: None,
            opened_from: None,
            focus: cx.focus_handle(),
            nav: HashMap::new(),
            nav_current: None,
            keys_inputs: HashMap::new(),
            repo: None,
            filter: None,
            filter_for: None,
            code: None,
            clear_code: false,
            narrow: false,
            sign_in_focus: cx.focus_handle(),
            votes_focus: cx.focus_handle(),
            grips: HashMap::new(),
            dragging: None,
            title_picker: None,
            effort: cx.new(|cx| {
                super::controls::popover::PopoverState::new(crate::state::popover::EFFORT_EXIT, cx)
            }),
            scroll: ScrollHandle::new(),
            copied: None,
            clock: None,
            flash_bounds: Rc::new(std::cell::Cell::new(None)),
            pending_open: None,
            pending_key: None,
            pending_drag: None,
            frames: 0,
            _keys: {
                let this = cx.entity().downgrade();
                cx.intercept_keystrokes(move |_, _, cx| {
                    let _ = this.update(cx, |s, _| s.key_arrived());
                })
            },
            subscriptions: Vec::new(),
            opening_read: None,
            facts_wait: None,
        }
    }

    fn log(&self, action: &str, fields: serde_json::Value) {
        if let Some(log) = &self.log {
            log.record(action, fields);
        }
    }

    /// The panel (its phase, pane and draft).
    pub fn panel(&self) -> &Panel {
        &self.panel
    }

    /// Whether the panel draws (open or saving).
    pub fn is_open(&self) -> bool {
        panel::panel_draft(&self.panel).is_some()
    }

    /// The published builds.
    pub fn published(&self) -> &Published {
        &self.published
    }

    /// A key reached the window (the filter's key-to-paint timing).
    pub fn key_arrived(&mut self) {
        if self.is_open() && self.pending_key.is_none() {
            self.pending_key = Some(Instant::now());
        }
    }

    /// Opens Settings on `view` (none: Connect an AI while no AI is usable,
    /// General once one is), and flashes the General row `flash` when a
    /// palette item asked for it (`openSettings`, `openSetting`). Open
    /// already: turns to `view`, keeping the draft. Being saved: a notice.
    pub fn open(
        &mut self,
        view: Option<Pane>,
        flash: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let first = || {
            let providers = super::pickers::catalog::Catalog::installed(cx)
                .read(cx)
                .providers()
                .to_vec();
            panel::first_pane_of(&providers)
        };
        match &self.panel {
            Panel::Saving { .. } => {
                (self.hooks.notice)("Settings are being saved".into(), cx);
            }
            Panel::Open { .. } => {
                let view = view.unwrap_or_else(first);
                self.show_pane(view, window, cx);
            }
            Panel::Opening { .. } => {
                let view = view.unwrap_or_else(first);
                self.panel = Panel::Opening { view };
            }
            Panel::Closed => {
                let view = view.unwrap_or_else(first);
                self.opened_from = window.focused(cx);
                self.pending_open = Some(Instant::now());
                self.panel = panel::panel_to(&self.panel, Panel::Opening { view })
                    .unwrap_or_else(|e| panic!("{e}"));
                self.core.send(Command::ReadSettings { refresh: false });
                self.log("opened", serde_json::json!({ "layer": "settings" }));
            }
        }
        self.flash = flash.map(|row| (row, None));
        cx.notify();
    }

    /// Turns to `view` without counting as an edit (`showSettingsPane`);
    /// focus goes to its tab.
    pub fn show_pane(&mut self, view: Pane, window: &mut Window, cx: &mut Context<Self>) {
        let next = match &self.panel {
            Panel::Open { draft, error, .. } => Panel::Open {
                view: view.clone(),
                draft: draft.clone(),
                error: error.clone(),
            },
            Panel::Saving { .. } => return,
            other => panic!(
                "Settings turn to a pane only when open, not {}",
                other.phase()
            ),
        };
        self.panel = panel::panel_to(&self.panel, next).unwrap_or_else(|e| panic!("{e}"));
        self.nav_current = Some(view.id().to_string());
        self.log("settings pane", serde_json::json!({ "pane": view.id() }));
        // Keyboard focus goes to the shown pane's tab, whatever opened it
        // (a pointer, a key, a reader's press), as when Settings open.
        let focus = self.nav_focus(view.id(), cx);
        window.focus(&focus, cx);
        if let Pane::Provider(id) = &view {
            self.pane_opened(id.clone(), window, cx);
        }
        self.scroll.set_offset(point(px(0.), px(0.)));
        cx.notify();
    }

    /// A provider's pane is on screen: its models are asked for, its
    /// filter starts empty, and a sign-in asked for from Connect an AI
    /// starts (`startWith`).
    fn pane_opened(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        let connected = self
            .draft()
            .map(|d| d.provider(&id).entry.connected)
            .unwrap_or(false);
        if connected {
            super::pickers::catalog::Catalog::installed(cx).update(cx, |c, _| c.ask_models(&id));
        }
        if self.filter_for.as_deref() != Some(id.as_str()) {
            self.filter_for = Some(id.clone());
            if let Some(filter) = &self.filter {
                filter.update(cx, |f, cx| f.set_value("", window, cx));
            }
            if let Some(code) = &self.code {
                code.update(cx, |f, cx| f.set_value("", window, cx));
            }
        }
        if let Some((provider, method)) = self.start_with.clone()
            && provider == id
        {
            self.start_with = None;
            self.begin_sign_in(&id, method, cx);
        }
    }

    /// The draft shown.
    pub fn draft(&self) -> Option<&Draft> {
        panel::panel_draft(&self.panel)
    }

    /// Edits the draft (`editSettings`): only open Settings are edited, and
    /// an edit lets go of the last save's error.
    pub fn edit(&mut self, change: impl FnOnce(&mut Draft), cx: &mut Context<Self>) {
        self.panel = panel::panel_edit(&self.panel, change).unwrap_or_else(|e| panic!("{e}"));
        self.sync_title_picker(cx);
        cx.notify();
    }

    /// Closes open Settings, dropping the draft (`closeSettings`); Settings
    /// being saved stay.
    pub fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !matches!(self.panel, Panel::Open { .. } | Panel::Opening { .. }) {
            return;
        }
        let was_open = matches!(self.panel, Panel::Open { .. });
        self.panel = panel::panel_to(&self.panel, Panel::Closed).unwrap_or_else(|e| panic!("{e}"));
        self.ended(was_open, window, cx);
    }

    /// The panel went to closed: the layer goes, the published list is let
    /// go of, the inputs dropped, focus goes back where it was.
    fn ended(&mut self, was_drawn: bool, window: &mut Window, cx: &mut Context<Self>) {
        keys::close_layer(cx, Layer::Settings);
        self.stop_recording(cx);
        if self.published.phase != published::Phase::Unread {
            self.published = published::closed(&self.published).unwrap_or_else(|e| panic!("{e}"));
        }
        self.keys_inputs.clear();
        self.repo = None;
        self.filter = None;
        self.filter_for = None;
        self.code = None;
        self.title_picker = None;
        self.subscriptions.clear();
        self.dragging = None;
        self.clock = None;
        self.votes_armed = false;
        self.flash = None;
        // A key that closed Settings is no key to a frame of theirs.
        self.pending_key = None;
        self.nav_current = None;
        if was_drawn {
            let frames = std::mem::take(&mut self.frames);
            self.log(
                "settings-timing",
                serde_json::json!({ "what": "frames", "frames": frames }),
            );
            if let Some(from) = self.opened_from.take() {
                window.focus(&from, cx);
            }
            self.log("closed", serde_json::json!({ "layer": "settings" }));
            let closed = self.hooks.closed.clone();
            window.defer(cx, move |window, cx| closed(window, cx));
        }
        cx.notify();
    }

    /// Saves the draft (`saveSettings`).
    pub fn save(&mut self, cx: &mut Context<Self>) {
        let Panel::Open { view, draft, .. } = &self.panel else {
            return;
        };
        let change = panel::change_of(draft);
        self.panel = panel::panel_to(
            &self.panel,
            Panel::Saving {
                view: view.clone(),
                draft: draft.clone(),
            },
        )
        .unwrap_or_else(|e| panic!("{e}"));
        self.log("save settings", serde_json::json!({}));
        self.core.send(Command::SaveSettings(change));
        cx.notify();
    }

    /// Checks again (`recheckSettings`): logins, keys and versions read
    /// again; the draft keeps its edits.
    pub fn recheck(&mut self) {
        self.log("recheck", serde_json::json!({}));
        self.core.send(Command::ReadSettings { refresh: true });
    }

    /// The one sign-in button (`begin`) of `id`.
    fn begin_sign_in(&mut self, id: &str, method: Option<String>, cx: &mut Context<Self>) {
        let Some(draft) = self.draft() else {
            return;
        };
        let entry = draft.provider(id).entry.clone();
        let login = self.logins.entry(id.to_string()).or_default();
        let command = sign_in::begin(&entry, login, method);
        self.log("sign in", serde_json::json!({ "provider": id }));
        self.core.send(command);
        cx.notify();
    }

    /// Takes the core's update for Settings.
    pub fn apply(&mut self, update: Update, window: &mut Window, cx: &mut Context<Self>) {
        match update {
            Update::Settings(Ok(view)) => {
                // par-shell (`checkRef`): a read older than the settings on
                // show (a check that a later check, a save or an opening
                // came after) lands nowhere.
                if self
                    .draft()
                    .is_some_and(|draft| view.checked_at < draft.checked_at)
                {
                    return;
                }
                self.take_logins(&view);
                match &self.panel {
                    Panel::Opening { .. } => {
                        // par-shell: the title model's facts first, as
                        // `readSettings` loads them before opening.
                        let catalog = super::pickers::catalog::Catalog::installed(cx);
                        let waiting = view
                            .title_model
                            .clone()
                            .filter(|t| !catalog.read(cx).facts_settled(&t.provider, &t.model));
                        self.opening_read = Some(view);
                        match waiting {
                            Some(title) => {
                                catalog
                                    .update(cx, |c, _| c.ask_facts(&title.provider, &title.model));
                                self.facts_wait =
                                    Some(cx.observe_in(&catalog, window, |this, _, window, cx| {
                                        this.open_when_facts_are_in(window, cx)
                                    }));
                            }
                            None => self.open_when_facts_are_in(window, cx),
                        }
                    }
                    Panel::Open {
                        view: pane,
                        draft,
                        error,
                    } => {
                        // `Checked …` (a later read): the shown provider's
                        // catalogue is read again, the list kept on show.
                        if view.checked_at != draft.checked_at
                            && let Pane::Provider(id) = pane
                            && draft.provider(id).entry.connected
                        {
                            let id = id.clone();
                            super::pickers::catalog::Catalog::installed(cx)
                                .update(cx, |c, _| c.ask_models_again(&id));
                        }
                        self.panel = Panel::Open {
                            view: pane.clone(),
                            draft: panel::refreshed(draft, view),
                            error: error.clone(),
                        };
                        self.sync_title_picker(cx);
                    }
                    Panel::Saving { view: pane, draft } => {
                        self.panel = Panel::Saving {
                            view: pane.clone(),
                            draft: panel::refreshed(draft, view),
                        };
                    }
                    Panel::Closed => {}
                }
                self.follow_installs(cx);
            }
            Update::Settings(Err(error)) => {
                if let Panel::Opening { .. } = self.panel {
                    self.panel = panel::panel_to(&self.panel, Panel::Closed)
                        .unwrap_or_else(|e| panic!("{e}"));
                    self.ended(false, window, cx);
                }
                (self.hooks.notice)(error, cx);
            }
            Update::SettingsSaved(result) => {
                let Panel::Saving { view, draft } = &self.panel else {
                    return;
                };
                match result {
                    Ok(()) => {
                        self.panel = panel::panel_to(&self.panel, Panel::Closed)
                            .unwrap_or_else(|e| panic!("{e}"));
                        self.ended(true, window, cx);
                    }
                    Err(error) => {
                        self.panel = panel::panel_to(
                            &self.panel,
                            Panel::Open {
                                view: view.clone(),
                                draft: draft.clone(),
                                error,
                            },
                        )
                        .unwrap_or_else(|e| panic!("{e}"));
                    }
                }
            }
            Update::SignIn { provider, state } => {
                self.logins.entry(provider).or_default().sign_in = state;
            }
            Update::Install { program, state } => {
                let ids: Vec<String> = self
                    .draft()
                    .map(|d| {
                        d.providers
                            .iter()
                            .filter(|p| {
                                p.entry
                                    .sign_in
                                    .as_ref()
                                    .is_some_and(|o| o.program.name == program)
                            })
                            .map(|p| p.entry.id.clone())
                            .collect()
                    })
                    .unwrap_or_default();
                for id in ids {
                    sign_in::install_changed(self.logins.entry(id).or_default(), state.clone());
                }
            }
            Update::Leaderboard(result) => match result {
                Ok(board) => {
                    if self.clearing && board.votes == 0 {
                        self.votes_armed = false;
                    }
                    self.clearing = false;
                    self.votes = Votes::Count(board.votes);
                }
                Err(error) => {
                    // par-shell (E.3): a failed delete keeps the count and
                    // its question (`clear` leaves `votes` as it was); only
                    // a failed count leaves it unknown.
                    if !std::mem::replace(&mut self.clearing, false) {
                        self.votes = Votes::Unknown(error.clone());
                    }
                    (self.hooks.notice)(error, cx);
                }
            },
            Update::Shares(result) => {
                if self.published.phase == published::Phase::Unread {
                    return;
                }
                let reading = if self.published.phase == published::Phase::Loading {
                    self.published.clone()
                } else {
                    published::reading(&self.published).unwrap_or_else(|e| panic!("{e}"))
                };
                self.published = match result {
                    Ok(data) => published::read(&reading, data),
                    Err(error) => published::failed(&reading, &error),
                }
                .unwrap_or_else(|e| panic!("{e}"));
            }
            Update::ShareRemoved { share_id, result } => {
                if !self.published.removing.contains(&share_id) {
                    return;
                }
                self.published = match result {
                    Ok(()) => published::removed(&self.published, &share_id),
                    Err(error) => published::kept(&self.published, &share_id, &error),
                }
                .unwrap_or_else(|e| panic!("{e}"));
            }
            other => panic!("not a Settings update: {other:?}"),
        }
        cx.notify();
    }

    /// The sign-ins and installs a read carries.
    fn take_logins(&mut self, view: &crate::SettingsView) {
        for p in &view.providers {
            if let Some(offer) = &p.sign_in {
                let login = self.logins.entry(p.id.clone()).or_default();
                login.sign_in = offer.current.clone();
                if let Some(install) = &offer.install {
                    login.install = Some(install.clone());
                }
            }
        }
    }

    /// Settings read again: a CLI now installed starts the sign-in its
    /// install remembered.
    fn follow_installs(&mut self, cx: &mut Context<Self>) {
        let Some(draft) = self.draft() else {
            return;
        };
        let entries: Vec<crate::ProviderSettings> = draft
            .providers
            .iter()
            .filter(|p| p.entry.sign_in.is_some())
            .map(|p| p.entry.clone())
            .collect();
        for entry in entries {
            let login = self.logins.entry(entry.id.clone()).or_default();
            if let Some(command) = sign_in::installed(&entry, login) {
                self.core.send(command);
            }
        }
        cx.notify();
    }

    /// The panel opened: the layer, the inputs, the reads that go with it,
    /// and focus on the active navigation row.
    /// par-shell: opens the held read once the title model's facts have
    /// arrived or failed (a failure opens them too, as the page's `catch`).
    fn open_when_facts_are_in(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Panel::Opening { view: pane } = &self.panel else {
            return;
        };
        let Some(read) = self.opening_read.as_ref() else {
            return;
        };
        if let Some(title) = &read.title_model
            && !super::pickers::catalog::Catalog::installed(cx)
                .read(cx)
                .facts_settled(&title.provider, &title.model)
        {
            return;
        }
        let pane = pane.clone();
        let read = self.opening_read.take().expect("held above");
        self.facts_wait = None;
        self.panel = panel::panel_to(
            &self.panel,
            Panel::Open {
                view: pane.clone(),
                draft: panel::draft_of(read),
                error: String::new(),
            },
        )
        .unwrap_or_else(|e| panic!("{e}"));
        self.opened(pane, window, cx);
        cx.notify();
    }

    fn opened(&mut self, view: Pane, window: &mut Window, cx: &mut Context<Self>) {
        let this = cx.entity().downgrade();
        keys::open_layer(cx, Layer::Settings, move |window, cx| {
            let _ = this.update(cx, |s, cx| s.close(window, cx));
        });
        self.make_inputs(window, cx);
        self.published = published::reading(&self.published).unwrap_or_else(|e| panic!("{e}"));
        self.core.send(Command::ReadShares);
        self.votes = Votes::Counting;
        self.core.send(Command::ReadLeaderboard);
        if let Some(title) = self.draft().and_then(|d| d.title_model.clone()) {
            super::pickers::catalog::Catalog::installed(cx)
                .update(cx, |c, _| c.ask_facts(&title.provider, &title.model));
        }
        self.nav_current = Some(view.id().to_string());
        if let Pane::Provider(id) = &view {
            self.pane_opened(id.clone(), window, cx);
        }
        let focus = self.nav_focus(view.id(), cx);
        window.focus(&focus, cx);
    }

    /// The text fields of an opened draft: each key provider's key, the
    /// repository, a provider's filter and the sign-in code.
    fn make_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(draft) = self.draft().cloned() else {
            return;
        };
        for p in draft.providers.iter().filter(|p| p.entry.env_key.is_some()) {
            let id = p.entry.id.clone();
            let placeholder = panel::key_placeholder(p.entry.key_source);
            let input = cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(placeholder)
                    .masked(true)
            });
            let edited = id.clone();
            self.subscriptions.push(cx.subscribe_in(
                &input,
                window,
                move |this: &mut Self, input, event: &InputEvent, _, cx| {
                    if let InputEvent::Change = event {
                        let text = input.read(cx).value().to_string();
                        let id = edited.clone();
                        this.edit(move |d| d.provider_mut(&id).api_key = text, cx);
                    }
                },
            ));
            self.keys_inputs.insert(id, input);
        }
        let repo_text = draft.share_repo.clone().unwrap_or_default();
        let repo = cx.new(|cx| {
            let mut state = InputState::new(window, cx).placeholder("owner/name");
            state.set_value(repo_text.as_str(), window, cx);
            state
        });
        self.subscriptions.push(cx.subscribe_in(
            &repo,
            window,
            |this: &mut Self, input, event: &InputEvent, _, cx| {
                if let InputEvent::Change = event {
                    let text = input.read(cx).value().to_string();
                    this.edit(move |d| panel::with_share_repo(d, &text), cx);
                }
            },
        ));
        self.repo = Some(repo);
        let filter = cx.new(|cx| InputState::new(window, cx).placeholder("Filter models"));
        self.subscriptions.push(cx.subscribe_in(
            &filter,
            window,
            |this: &mut Self, _, event: &InputEvent, _, cx| {
                if let InputEvent::Change = event {
                    cx.notify();
                    let _ = this;
                }
            },
        ));
        self.filter = Some(filter);
        let code = cx.new(|cx| InputState::new(window, cx).placeholder("Paste the code"));
        self.subscriptions.push(cx.subscribe_in(
            &code,
            window,
            |this: &mut Self, _, event: &InputEvent, _, cx| match event {
                InputEvent::Change => cx.notify(),
                InputEvent::PressEnter { .. } => this.submit_code(cx),
                _ => {}
            },
        ));
        self.code = Some(code);
        let catalog = super::pickers::catalog::Catalog::installed(cx);
        let this = cx.entity().downgrade();
        let picker = cx.new(|cx| {
            let on_change: super::pickers::model::OnChange =
                Rc::new(move |provider, model, _, cx| {
                    let _ = this.update(cx, |s, cx| {
                        s.edit(|d| panel::with_title_model(d, Some((provider, model))), cx);
                        let title = s.draft().and_then(|d| d.title_model.clone());
                        if let Some(title) = title {
                            super::pickers::catalog::Catalog::installed(cx)
                                .update(cx, |c, _| c.ask_facts(&title.provider, &title.model));
                        }
                    });
                });
            ModelPicker::new(catalog, on_change, window, cx)
        });
        let this = cx.entity().downgrade();
        let set_up: super::Press = Rc::new(move |window, cx| {
            let _ = this.update(cx, |s, cx| s.show_pane(Pane::Connect, window, cx));
        });
        picker.update(cx, |p, cx| {
            p.set_look(
                Look {
                    label: "Title model",
                    empty: "The chat's own model",
                    below: true,
                    asks: false,
                    set_up: Some(set_up),
                    providers: Some(pickers_providers(&draft)),
                },
                cx,
            );
            p.set_value(
                draft
                    .title_model
                    .as_ref()
                    .map(|t| (t.provider.clone(), t.model.clone())),
                cx,
            );
        });
        self.title_picker = Some(picker);
    }

    /// The title picker follows the draft (its model and its providers).
    fn sync_title_picker(&mut self, cx: &mut Context<Self>) {
        let (Some(picker), Some(draft)) = (self.title_picker.clone(), self.draft().cloned()) else {
            return;
        };
        picker.update(cx, |p, cx| {
            p.set_value(
                draft
                    .title_model
                    .as_ref()
                    .map(|t| (t.provider.clone(), t.model.clone())),
                cx,
            );
        });
        let providers = pickers_providers(&draft);
        picker.update(cx, |p, cx| p.set_draft_providers(providers, cx));
    }

    /// Submits the pasted sign-in code (Enter or Submit).
    fn submit_code(&mut self, cx: &mut Context<Self>) {
        let (Some(code), Some(Pane::Provider(id))) =
            (self.code.clone(), self.panel.view().cloned())
        else {
            return;
        };
        let text = code.read(cx).value().to_string();
        if text.trim().is_empty() {
            return;
        }
        self.log("sign-in code", serde_json::json!({ "provider": id }));
        self.core.send(Command::SignInInput { provider: id, text });
        self.clear_code = true;
        cx.notify();
    }

    /// The focus handle of a navigation row.
    fn nav_focus(&mut self, id: &str, cx: &mut Context<Self>) -> FocusHandle {
        self.nav
            .entry(id.to_string())
            .or_insert_with(|| cx.focus_handle())
            .clone()
    }

    /// Starts or stops the shortcut recorder on `action`.
    fn toggle_recording(&mut self, action: BindAction, cx: &mut Context<Self>) {
        if self.ended_by_press.take() == Some(action) || self.recording == Some(action) {
            self.stop_recording(cx);
            cx.notify();
            return;
        }
        self.recording = Some(action);
        let this = cx.entity().downgrade();
        keys::record(cx, move |combo, _, cx| {
            let _ = this.update(cx, |s, cx| s.recorded(combo, cx));
        });
        self.log("recording", serde_json::json!({ "action": action.id() }));
        cx.notify();
    }

    fn stop_recording(&mut self, cx: &mut Context<Self>) {
        if self.recording.take().is_some() {
            keys::stop_recording(cx);
        }
    }

    /// A key while recording: a combination sets the action's shortcut and
    /// unbinds any other action that had it; Escape cancels.
    fn recorded(&mut self, combo: Option<String>, cx: &mut Context<Self>) {
        let Some(action) = self.recording else {
            return;
        };
        self.stop_recording(cx);
        if let Some(combo) = combo {
            let mut keymap = keys::keymap(cx).clone();
            for (other, bound) in keymap.0.iter_mut() {
                if *other != action && *bound == combo {
                    bound.clear();
                }
            }
            keymap.0.insert(action, combo.clone());
            self.log(
                "shortcut",
                serde_json::json!({ "action": action.id(), "combo": combo }),
            );
            (self.hooks.binds)(keymap, cx);
        } else {
            self.log("recording cancelled", serde_json::json!({}));
        }
        cx.notify();
    }

    /// A control under focus went: focus goes to the active navigation
    /// row.
    pub fn focus_active(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(view) = self.panel.view().cloned() else {
            return;
        };
        self.nav_current = Some(view.id().to_string());
        let focus = self.nav_focus(view.id(), cx);
        window.focus(&focus, cx);
        cx.notify();
    }

    /// Takes focus out of the panel's last stop to its first, and back
    /// (Tab stays inside the layer).
    fn trap(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        for _ in 0..2000 {
            if forward {
                window.focus_next(cx);
            } else {
                window.focus_prev(cx);
            }
            if self.focus.contains_focused(window, cx) {
                return;
            }
        }
        panic!("no focus stop inside Settings");
    }
}

/// The draft's providers as the pickers list them.
fn pickers_providers(draft: &Draft) -> Vec<crate::Provider> {
    draft
        .providers
        .iter()
        .map(|p| crate::Provider {
            id: p.entry.id.clone(),
            label: p.entry.label.clone(),
            connected: p.entry.connected,
            auth: p.entry.auth,
            harness: p.entry.harness,
            enabled: p.entry.enabled,
            hidden_models: p.entry.hidden_models.clone(),
            model_order: p.entry.model_order.clone(),
        })
        .collect()
}

impl Render for SettingsLayer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(draft) = self.draft().cloned() else {
            return div().into_any_element();
        };
        let fonts = fonts(cx);
        let saving = matches!(self.panel, Panel::Saving { .. });
        let view = self
            .panel
            .view()
            .cloned()
            .expect("open settings are on a pane");
        let error = match &self.panel {
            Panel::Open { error, .. } if !error.is_empty() => Some(error.clone()),
            _ => None,
        };
        if self.clear_code {
            self.clear_code = false;
            if let Some(code) = self.code.clone() {
                code.update(cx, |c, cx| c.set_value("", window, cx));
            }
        }
        self.tick_clock(&view, &draft, cx);
        // The stylesheet's sizes (`.settings-panel.sidebar`): at most 1100
        // by 780, 32 px inside the window's width and 48 inside its height;
        // at 980 px wide and below, 32 px inside its height and the
        // navigation above the pane, at most 38% of the height. (The
        // 560 px bottom sheet is below the window's least width, 640.)
        let width = f32::from(window.viewport_size().width);
        let height = f32::from(window.viewport_size().height);
        self.narrow = width <= NARROW;
        parts::set_narrow(self.narrow);
        let narrow = self.narrow;
        let nav = self.render_nav(&draft, &view, window, cx);
        let pane = match &view {
            Pane::General => self.render_general(&draft, window, cx),
            Pane::Connect => self.render_connect(&draft, window, cx),
            Pane::Provider(id) => self.render_provider(&draft, id, window, cx),
        };
        let pane_title = match &view {
            Pane::General => "General".to_string(),
            Pane::Connect => "Connect an AI".to_string(),
            Pane::Provider(id) => draft.provider(id).entry.label.clone(),
        };
        let this = cx.entity().downgrade();
        let close = Button::icon("settings-close", Icon::X, "Close settings")
            .with_icon(Icon::X, 17.)
            .tip("Close settings")
            .disabled(saving)
            .on_press(move |window, cx| {
                let _ = this.update(cx, |s, cx| s.close(window, cx));
            });
        let error_row = error.map(|text| {
            let this = cx.entity().downgrade();
            theme::text(
                div()
                    .id("settings-error")
                    .role(Role::Alert)
                    .aria_label(text.clone())
                    .mx(px(32.))
                    .mb(px(6.))
                    .px(px(11.))
                    .py(px(8.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(theme::settings::error_border())
                    .bg(theme::settings::error_fill())
                    .flex()
                    .items_start()
                    .justify_between()
                    .gap(px(10.)),
                TextKind::Error,
                &fonts,
            )
            .text_size(px(12.))
            .child(div().flex_1().min_w_0().child(text))
            .child(
                Button::icon("settings-error-dismiss", Icon::X, "Dismiss error")
                    .with_icon(Icon::X, 13.)
                    .tip("Dismiss error")
                    .on_press(move |_, cx| {
                        let _ = this.update(cx, |s, cx| s.edit(|_| {}, cx));
                    }),
            )
        });
        let save_this = cx.entity().downgrade();
        let cancel_this = cx.entity().downgrade();
        let actions = div()
            .flex()
            .justify_end()
            .gap(px(8.))
            .pt(px(12.))
            .px(px(22.))
            .pb(px(18.))
            .child(
                Button::new("settings-cancel", "Cancel")
                    .disabled(saving)
                    .on_press(move |window, cx| {
                        let _ = cancel_this.update(cx, |s, cx| s.close(window, cx));
                    }),
            )
            .child(
                Button::new(
                    "settings-save",
                    if saving {
                        "Saving\u{2026}"
                    } else {
                        "Save changes"
                    },
                )
                .kind(Kind::Primary)
                .disabled(saving)
                .on_press(move |_, cx| {
                    let _ = save_this.update(cx, |s, cx| s.save(cx));
                }),
            );
        let flash_bounds = self.flash_bounds.clone();
        let scroll = self.scroll.clone();
        let this = cx.entity().downgrade();
        let scroller = canvas(
            |_, _, _| {},
            move |bounds, _, window, cx| {
                // The flashed row, scrolled to the middle once laid out.
                if let Some(row) = flash_bounds.take() {
                    let middle = bounds.origin.y + bounds.size.height / 2.;
                    let row_middle = row.origin.y + row.size.height / 2.;
                    let offset = scroll.offset();
                    scroll.set_offset(point(offset.x, offset.y - (row_middle - middle)));
                    let _ = this.update(cx, |_, cx| cx.notify());
                    window.refresh();
                }
            },
        )
        .absolute()
        .inset_0();
        let painted = {
            let this = cx.entity().downgrade();
            canvas(
                |_, _, _| {},
                move |_, _, _, cx| {
                    let _ = this.update(cx, |s, _| s.painted());
                },
            )
            .absolute()
            .size_0()
        };
        let main = div()
            .relative()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .when(!narrow, |d| d.h_full())
            .flex()
            .flex_col()
            .child(div().absolute().top(px(14.)).right(px(14.)).child(close))
            .child(
                div()
                    .id("settings-pane")
                    .role(Role::TabPanel)
                    .aria_label(pane_title)
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .relative()
                    .child(scroller)
                    .child(
                        div()
                            .pt(px(if narrow { 18. } else { 22. }))
                            .px(px(if narrow { 16. } else { 32. }))
                            .pb(px(if narrow { 24. } else { 28. }))
                            .flex()
                            .flex_col()
                            .gap(px(26.))
                            .child(pane),
                    ),
            )
            .children(error_row)
            .child(actions);
        let dialog = div()
            .id("settings")
            .role(Role::Dialog)
            .aria_label("Settings")
            .a11y_synthetic_children(move |builder| {
                builder.parent_node().set_modal();
                if saving {
                    builder.parent_node().set_busy();
                }
            })
            .track_focus(&self.focus)
            .key_context("Settings")
            .on_action(cx.listener(|this, _: &FocusNext, window, cx| {
                cx.stop_propagation();
                this.trap(true, window, cx)
            }))
            .on_action(cx.listener(|this, _: &FocusPrevious, window, cx| {
                cx.stop_propagation();
                this.trap(false, window, cx)
            }))
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            // A grip's drag follows the pointer over the panel, and ends
            // where it is let go.
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                this.drag_moved(f32::from(event.position.y), cx)
            }))
            .capture_any_mouse_up(cx.listener(|this, event: &MouseUpEvent, _, cx| {
                this.drag_ended(f32::from(event.position.y), cx)
            }))
            .w(px((1100f32).min(width - 32.)))
            .h(px(if narrow {
                height - 32.
            } else {
                (780f32).min(height - 48.)
            }))
            .rounded(px(14.))
            .border_1()
            .border_color(colour::line())
            .bg(colour::cave())
            .shadow(shadow::panel())
            .overflow_hidden()
            .flex()
            .when(narrow, |d| d.flex_col())
            .child(nav)
            .child(main)
            .child(painted);
        let this = cx.entity().downgrade();
        let recording = self.recording;
        div()
            .id("settings-scrim")
            .absolute()
            .inset_0()
            .bg(theme::settings::scrim())
            .flex()
            .items_center()
            .justify_center()
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                let _ = this.update(cx, |s, cx| s.close(window, cx));
            })
            .capture_any_mouse_down(cx.listener(move |this, _, _, cx| {
                // A press anywhere ends the recording (its own button's
                // click then does not start it again).
                if let Some(action) = recording {
                    this.ended_by_press = Some(action);
                    this.stop_recording(cx);
                    cx.notify();
                } else {
                    this.ended_by_press = None;
                }
            }))
            .child(dialog)
            .with_animation(
                "settings-in",
                Animation::new(std::time::Duration::from_millis(150)),
                |d, t| d.opacity(theme::ease(t)),
            )
            .into_any_element()
    }
}

impl SettingsLayer {
    /// A frame was painted: the timings the dev binary logs.
    fn painted(&mut self) {
        self.frames += 1;
        if let Some(at) = self.pending_open.take() {
            self.log(
                "settings-timing",
                serde_json::json!({ "what": "open", "ms": at.elapsed().as_secs_f64() * 1000. }),
            );
        }
        if let Some(at) = self.pending_key.take() {
            self.log(
                "settings-timing",
                serde_json::json!({ "what": "key", "ms": at.elapsed().as_secs_f64() * 1000. }),
            );
        }
        if let Some(at) = self.pending_drag.take() {
            self.log(
                "settings-timing",
                serde_json::json!({ "what": "drag", "ms": at.elapsed().as_secs_f64() * 1000. }),
            );
        }
    }

    /// The `Checked` clock: while a provider's pane is on screen, one redraw
    /// when its text next changes, and none otherwise.
    fn tick_clock(&mut self, view: &Pane, draft: &Draft, cx: &mut Context<Self>) {
        if !matches!(view, Pane::Provider(_)) {
            self.clock = None;
            return;
        }
        let now = now_ms();
        let next = panel::ago_changes_at(draft.checked_at, now);
        if self.clock.as_ref().is_some_and(|(at, _)| *at == next) {
            return;
        }
        let wait = std::time::Duration::from_millis((next - now).max(1) as u64);
        let task = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(wait).await;
            let _ = this.update(cx, |_, cx| cx.notify());
        });
        self.clock = Some((next, task));
    }

    fn drag_moved(&mut self, y: f32, cx: &mut Context<Self>) {
        let Some(dragging) = self.dragging.as_mut() else {
            return;
        };
        let d = dragging.drag;
        dragging.drag = Drag::at(d.from, d.rows, d.height, y - dragging.start_y);
        self.pending_drag.get_or_insert_with(Instant::now);
        cx.notify();
    }

    fn drag_ended(&mut self, y: f32, cx: &mut Context<Self>) {
        let Some(dragging) = self.dragging.take() else {
            return;
        };
        let d = dragging.drag;
        let drag = Drag::at(d.from, d.rows, d.height, y - dragging.start_y);
        let to = drag.to();
        if to != drag.from {
            self.place_model(&dragging.model, to, false, cx);
        }
        cx.notify();
    }
}

/// Milliseconds since the epoch.
fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("the clock is after 1970")
        .as_millis() as i64
}

/// A provider's logo at `size`.
fn logo(id: &str, size: f32) -> AnyElement {
    icons::logo(crate::state::vendor::logo_name(id), size)
}
