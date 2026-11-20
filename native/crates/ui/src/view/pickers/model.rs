//! The model picker (`ModelPicker`, `web/app.js:1303-1538`; inventory
//! `components.md` 1.3 and D.3; `style-viewer-routes.md` 1.9): the trigger
//! with the chosen model's logo and label, and its popover: the provider
//! rail, the search, the list of the tab or the search, and the footer.
//! The rules are `state::models`; the data the [`Catalog`].
//!
//! Keyboard: the search has focus on open, so typing searches; Down from
//! the search goes to the first model, Up and Down move between models
//! (Home, End), Up from the first goes back to the search; Enter or Space
//! on a model chooses it; typing on a model goes on in the search; Escape
//! closes only the picker; Tab walks the rail, the search, the models and
//! their stars. The page has only Tab and Escape; the arrows and Enter are
//! the stage-four brief's.
//!
//! Accessibility: the trigger is a button named `Chat model: <label>` (or
//! the empty label) that opens a dialog, expanded while open, its full id
//! as its description; the popover is a dialog named `Chat model`; the
//! rail a tab list named `Providers` whose tabs are named by their titles
//! (`OpenAI — not configured` says why it is disabled) with the active one
//! selected; `Set up providers` a button; the search a text input named
//! `Search models`; the list a list box whose options carry their name,
//! the chosen one selected, and their position and size; each option's
//! star a toggle button named `Favorite <model>`, pressed when starred,
//! described `Add favorite` or `Remove favorite`.

use std::collections::HashMap;
use std::rc::Rc;
use std::time::Instant;

use gpui_kit::component::input::{Input, InputEvent, InputState, MoveDown, MoveUp};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::catalog::Catalog;
use super::{Entry, panel, trigger};
use crate::ActionLog;
use crate::state::models::{self, Listing, Tab};
use crate::state::popover::MODEL_EXIT;
use crate::state::{Step, step};
use crate::view::controls::fonts;
use crate::view::controls::popover::PopoverState;
use crate::view::icons::{self, Icon};
use crate::view::theme::{self, TextKind, colour, radius, shadow};
use crate::view::{Activate, MoveFirst, MoveLast, MoveNext, MovePrevious, ring};

/// What choosing a model does: its provider and id.
pub type OnChange = Rc<dyn Fn(String, String, &mut Window, &mut App)>;

/// The picker's label (`label: 'Chat model'`).
pub const LABEL: &str = "Chat model";
/// The footer's label.
pub const FOOTER: &str = "Compare several in Bench";
/// The rail's last button.
pub const SET_UP: &str = "Set up providers";
/// The search's accessible name.
pub const SEARCH_NAME: &str = "Search models";

/// A row's height (`.model-picker-row`, `min-height: 34px`).
const ROW: f32 = 34.;

/// What a measurement waits to see painted.
#[derive(Clone, Copy, Debug)]
enum Pending {
    Open(Instant),
    Key(Instant),
    Scroll(Instant),
}

/// The model picker.
pub struct ModelPicker {
    catalog: Entity<Catalog>,
    popover: Entity<PopoverState>,
    search: Entity<InputState>,
    query: String,
    tab: Option<Tab>,
    value: Option<(String, String)>,
    on_change: OnChange,
    /// The footer (`Compare several in Bench`) is shown.
    footer: bool,
    list: ListState,
    keys: Vec<String>,
    listing: Listing,
    rows: HashMap<String, FocusHandle>,
    stars: HashMap<String, FocusHandle>,
    tabs: HashMap<String, FocusHandle>,
    set_up: FocusHandle,
    footer_focus: FocusHandle,
    connect: FocusHandle,
    log: Option<ActionLog>,
    pending: Vec<Pending>,
    /// The providers whose catalogues were last asked for.
    asked: Vec<String>,
    /// It opened by itself (Settings closed with no model chosen): a model
    /// chosen then hands focus to the composer.
    opened_by_itself: bool,
    // chat-fix: the catalogue's error, selectable.
    note: crate::view::controls::selectable::Selectable,
    // settings agent: Settings' title model picker is this picker with its
    // own label and empty label, no footer, opening down, not opening by
    // itself, its set-up button turning Settings to a pane, and the draft's
    // providers in place of the window's.
    look: Look,
    // bench agent: the bench models picker's multi-select (`selected`,
    // `onToggle`): a choice toggles a model and the picker stays open.
    multi: Option<Vec<(String, String)>>,
    _subscriptions: Vec<Subscription>,
}

// settings agent: the look of a picker that is not the composer's.
/// What a model picker shows and does besides its models.
#[derive(Clone)]
pub struct Look {
    /// `label`: the trigger's name before its model, the popover's and
    /// the list's names.
    pub label: &'static str,
    /// `emptyLabel`: the trigger with no model chosen.
    pub empty: &'static str,
    /// Opens under its trigger (Settings) instead of over it.
    pub below: bool,
    /// Opens by itself when Settings close with no model chosen (the
    /// composer's only).
    pub asks: bool,
    /// `onSetup`: what `Set up providers` and `Connect an AI` do (none:
    /// the catalogue's hook, which opens Settings).
    pub set_up: Option<crate::view::Press>,
    /// The providers to list (Settings' draft); none: the catalogue's.
    pub providers: Option<Vec<crate::Provider>>,
}

impl Look {
    /// The composer's chat model picker.
    pub fn chat() -> Look {
        Look {
            label: LABEL,
            empty: models::EMPTY_LABEL,
            below: false,
            asks: true,
            set_up: None,
            providers: None,
        }
    }
}

fn row_key(provider: &str, model: &str) -> String {
    crate::state::favorites::favorite_key(provider, model)
}

fn tab_key(tab: &Tab) -> String {
    match tab {
        Tab::Favorites => "favorites".into(),
        Tab::Provider(id) => id.clone(),
    }
}

impl ModelPicker {
    /// A closed picker over `catalog`; `on_change` takes a chosen model.
    pub fn new(
        catalog: Entity<Catalog>,
        on_change: OnChange,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let search =
            cx.new(|cx| InputState::new(window, cx).placeholder(models::SEARCH_PLACEHOLDER));
        let typed = cx.subscribe_in(&search, window, |this: &mut Self, _, event, _, cx| {
            if let InputEvent::Change = event {
                this.query = this.search.read(cx).value().to_string();
                this.refresh(cx);
                cx.notify();
            }
        });
        let observed = cx.observe(&catalog, |this: &mut Self, _, cx| {
            this.refresh(cx);
            cx.notify();
        });
        let popover = cx.new(|cx| PopoverState::new(MODEL_EXIT, cx));
        let list = ListState::new(0, ListAlignment::Top, px(ROW * 4.)).keep_neighbours();
        let picker = cx.entity().downgrade();
        list.set_scroll_handler(move |_, _, cx| {
            let _ = picker.update(cx, |this, _| {
                if !this.pending.iter().any(|p| matches!(p, Pending::Scroll(_))) {
                    this.pending.push(Pending::Scroll(Instant::now()));
                }
            });
        });
        ModelPicker {
            catalog,
            popover,
            search,
            query: String::new(),
            tab: None,
            value: None,
            on_change,
            footer: true,
            list,
            keys: Vec::new(),
            listing: Listing {
                shown: Vec::new(),
                models: Vec::new(),
                loading: false,
                error: None,
                empty: None,
            },
            rows: HashMap::new(),
            stars: HashMap::new(),
            tabs: HashMap::new(),
            set_up: cx.focus_handle().tab_stop(true),
            footer_focus: cx.focus_handle().tab_stop(true),
            connect: cx.focus_handle().tab_stop(true),
            log: None,
            pending: Vec::new(),
            asked: Vec::new(),
            opened_by_itself: false,
            note: crate::view::controls::selectable::Selectable::new(cx),
            look: Look::chat(),
            multi: None,
            _subscriptions: vec![typed, observed],
        }
    }

    /// Records the open, key and scroll timings in `log`
    /// (`{"action":"picker-timing", "what": ..., "ms": ...}`).
    pub fn set_log(&mut self, log: Option<ActionLog>) {
        self.log = log;
    }

    /// A key reached the window (for the key-to-paint timing).
    pub fn key_arrived(&mut self, cx: &App) {
        if self.popover.read(cx).is_open()
            && !self.pending.iter().any(|p| matches!(p, Pending::Key(_)))
        {
            self.pending.push(Pending::Key(Instant::now()));
        }
    }

    // settings agent
    /// Gives the picker another look (Settings' title model): no footer.
    pub fn set_look(&mut self, look: Look, cx: &mut Context<Self>) {
        self.footer = false;
        self.look = look;
        self.refresh(cx);
        cx.notify();
    }

    /// Settings' draft changed: its providers are listed.
    pub fn set_draft_providers(&mut self, providers: Vec<crate::Provider>, cx: &mut Context<Self>) {
        if self.look.providers.as_ref() != Some(&providers) {
            self.look.providers = Some(providers);
            self.refresh(cx);
            cx.notify();
        }
    }

    /// The providers listed: the look's, else the catalogue's.
    fn providers(&self, cx: &App) -> Vec<crate::Provider> {
        match &self.look.providers {
            Some(p) => p.clone(),
            None => self.catalog.read(cx).providers().to_vec(),
        }
    }

    /// What `Set up providers` and `Connect an AI` do.
    fn set_up_press(&self, cx: &App) -> crate::view::Press {
        match &self.look.set_up {
            Some(press) => press.clone(),
            None => self.catalog.read(cx).hooks().open_settings,
        }
    }

    /// The model chosen, none when none is.
    pub fn set_value(&mut self, value: Option<(String, String)>, cx: &mut Context<Self>) {
        if self.value != value {
            self.value = value;
            cx.notify();
        }
    }

    /// The model chosen.
    pub fn value(&self) -> Option<&(String, String)> {
        self.value.as_ref()
    }

    /// The popover.
    pub fn popover(&self) -> Entity<PopoverState> {
        self.popover.clone()
    }

    /// The search's text (kept while the picker is closed).
    pub fn query(&self) -> &str {
        &self.query
    }

    /// The tab shown.
    pub fn tab(&self) -> Option<&Tab> {
        self.tab.as_ref()
    }

    /// What the list shows now.
    pub fn listing(&self) -> &Listing {
        &self.listing
    }

    /// Open.
    pub fn is_open(&self, cx: &App) -> bool {
        self.popover.read(cx).is_open()
    }

    fn chosen_provider(&self) -> String {
        self.value.as_ref().map(|v| v.0.clone()).unwrap_or_default()
    }

    /// Recomputes the listing from the catalogue, asks for the catalogues
    /// it shows, and tells the list when its rows changed.
    fn refresh(&mut self, cx: &mut Context<Self>) {
        let providers = self.providers(cx);
        let catalog = self.catalog.read(cx);
        let favorites = catalog.favorites();
        self.tab = models::settled_tab(
            self.tab.take(),
            &providers,
            &self.chosen_provider(),
            &favorites,
        );
        let listing = models::listing(
            &providers,
            catalog.catalogues(),
            &favorites,
            self.tab.as_ref(),
            &self.query,
            true,
        );
        let keys: Vec<String> = listing
            .models
            .iter()
            .map(|l| row_key(&l.provider, &l.model.id))
            .collect();
        if keys != self.keys {
            for key in &keys {
                self.rows
                    .entry(key.clone())
                    .or_insert_with(|| cx.focus_handle().tab_stop(true));
                self.stars
                    .entry(key.clone())
                    .or_insert_with(|| cx.focus_handle().tab_stop(true));
            }
            let handles: Vec<Option<FocusHandle>> =
                keys.iter().map(|k| self.rows.get(k).cloned()).collect();
            self.list.reset(0);
            self.list.splice_focusable(0..0, handles);
            self.keys = keys;
        }
        let shown = listing.shown.clone();
        self.listing = listing;
        // The catalogues shown are asked for when the set shown changes
        // while open, or on opening (`useShared`'s effect): not on every
        // answer, or a failing one would be asked again at once, forever.
        if self.popover.read(cx).is_open() && self.asked != shown {
            self.catalog.update(cx, |c, _| {
                for id in &shown {
                    c.ask_models(id);
                }
            });
            self.asked = shown;
        }
    }

    /// Opens on the opening tab with focus in the search (the trigger's
    /// press, or the picker opening by itself).
    pub fn open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let providers = self.providers(cx);
        let catalog = self.catalog.read(cx);
        self.tab = models::opening_tab(&providers, &self.chosen_provider(), &catalog.favorites());
        self.popover.update(cx, |p, cx| p.open(window, cx));
        self.pending.push(Pending::Open(Instant::now()));
        self.asked.clear();
        self.refresh(cx);
        let search = self.search.clone();
        search.update(cx, |s, cx| s.focus(window, cx));
        cx.notify();
    }

    /// Closes with its exit; focus goes back to the trigger.
    pub fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.opened_by_itself = false;
        self.popover.update(cx, |p, cx| p.close(window, cx));
        cx.notify();
    }

    /// The trigger's press.
    pub fn toggle(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.popover.read(cx).is_open() {
            self.close(window, cx);
        } else {
            self.open(window, cx);
        }
    }

    /// A tab's press: a disabled provider does nothing; the search clears.
    pub fn choose_tab(&mut self, tab: Tab, window: &mut Window, cx: &mut Context<Self>) {
        if let Tab::Provider(id) = &tab {
            let usable = self
                .providers(cx)
                .iter()
                .any(|p| &p.id == id && models::usable(p));
            if !usable {
                return;
            }
        }
        self.tab = Some(tab);
        self.query.clear();
        self.search.update(cx, |s, cx| s.set_value("", window, cx));
        self.refresh(cx);
        cx.notify();
    }

    // bench agent: multi-select.
    /// A multi-select picker (the bench models): `selected` is what the line-up
    /// holds; a choice calls the change (a toggle) and the picker stays open.
    pub fn set_selected(&mut self, selected: Vec<(String, String)>, cx: &mut Context<Self>) {
        self.multi = Some(selected);
        self.footer = false;
        cx.notify();
    }

    /// Chooses a model: the change, and the picker closes (in multi-select
    /// it toggles the model and stays open).
    pub fn choose(
        &mut self,
        provider: String,
        model: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let on_change = self.on_change.clone();
        if self.multi.is_some() {
            window.defer(cx, move |window, cx| on_change(provider, model, window, cx));
            return;
        }
        let after_ask = self
            .opened_by_itself
            .then(|| self.catalog.read(cx).hooks().chosen_after_ask);
        self.close(window, cx);
        if let Some(after_ask) = after_ask {
            after_ask(window, cx);
        }
        // After this update: the change reaches the picker's owner, which
        // sets the picker's value.
        window.defer(cx, move |window, cx| on_change(provider, model, window, cx));
    }

    fn focused_row(&self, window: &Window) -> Option<usize> {
        self.keys.iter().position(|k| {
            self.rows.get(k).is_some_and(|h| h.is_focused(window))
                || self.stars.get(k).is_some_and(|h| h.is_focused(window))
        })
    }

    fn focus_row(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(handle) = self.keys.get(index).and_then(|k| self.rows.get(k)).cloned() {
            self.list.scroll_to_reveal_item(index);
            window.focus(&handle, cx);
            cx.notify();
        }
    }

    fn step_rows(&mut self, by: Step, window: &mut Window, cx: &mut Context<Self>) {
        let current = self.focused_row(window);
        if current == Some(0) && by == Step::Previous {
            let search = self.search.clone();
            search.update(cx, |s, cx| s.focus(window, cx));
            return;
        }
        if let Some(next) = step(current, self.keys.len(), by) {
            self.focus_row(next, window, cx);
        }
    }

    /// Typing on a model goes on in the search.
    fn type_on(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        let search = self.search.clone();
        search.update(cx, |s, cx| {
            s.focus(window, cx);
            s.insert(text.to_string(), window, cx);
        });
        self.query = self.search.read(cx).value().to_string();
        self.refresh(cx);
        cx.notify();
    }

    fn painted(&mut self) {
        let Some(log) = &self.log else {
            self.pending.clear();
            return;
        };
        let now = Instant::now();
        for pending in self.pending.drain(..) {
            let (what, at) = match pending {
                Pending::Open(at) => ("open", at),
                Pending::Key(at) => ("key", at),
                Pending::Scroll(at) => ("scroll", at),
            };
            log.record(
                "picker-timing",
                serde_json::json!({
                    "what": what,
                    "ms": (now - at).as_secs_f64() * 1000.,
                    "listed": self.listing.models.len(),
                }),
            );
        }
    }

    fn render_trigger(&self, cx: &mut Context<Self>) -> AnyElement {
        let fonts = fonts(cx);
        let (name, description, logo, label) = match &self.value {
            Some((provider, model)) => (
                format!("{}: {}", self.look.label, models::model_label(model)),
                Some(model.clone()),
                Some(crate::state::vendor::vendor_icon(provider, model)),
                models::model_label(model).to_string(),
            ),
            None => (
                self.look.empty.to_string(),
                None,
                None,
                self.look.empty.to_string(),
            ),
        };
        let picker = cx.entity().downgrade();
        let press: crate::view::Press = Rc::new(move |window, cx| {
            let _ = picker.update(cx, |p, cx| p.toggle(window, cx));
        });
        let empty = self.value.is_none();
        // par-shell (D.3): the page's `title`, the model's full id or the
        // empty label, on hover.
        let title: SharedString = self
            .value
            .as_ref()
            .map(|(_, model)| model.clone())
            .unwrap_or_else(|| self.look.empty.to_string())
            .into();
        let element = trigger(
            "model-trigger",
            name.into(),
            accesskit::HasPopup::Dialog,
            &self.popover,
            press,
            cx,
        )
        .when_some(description, |d, text| d.aria_description(text))
        .gap(px(5.))
        .px(px(6.))
        .py(px(4.))
        .min_w_0();
        let element = theme::text(element, TextKind::PickerTrigger, &fonts)
            .when(empty, |d| d.text_color(colour::dim()))
            .when_some(logo, |d, logo| d.child(icons::logo(logo, 14.)))
            .child(div().min_w_0().truncate().child(label))
            .child(
                div()
                    .opacity(0.5)
                    .mr(px(-3.))
                    .child(icons::icon(Icon::ChevronDown, 12.)),
            );
        crate::view::controls::pointer_tip(element, "model-trigger-tip", title).into_any_element()
    }

    /// The popover's height with the whole rail in one line (`--rail`).
    fn rail_one_line(&self, cx: &App) -> f32 {
        let entries = models::rail(&self.providers(cx));
        let dividers = entries.iter().filter(|e| e.divider_before).count();
        let items = 1 + entries.len() + dividers;
        6. + ROW * (1 + entries.len()) as f32
            + 7. * dividers as f32
            + 3. * items.saturating_sub(1) as f32
            + 42.
            + 2.
    }

    fn render_rail(&mut self, height: f32, cx: &mut Context<Self>) -> AnyElement {
        let fonts = fonts(cx);
        let entries = models::rail(&self.providers(cx));
        let searching = !self.query.trim().is_empty();
        let active = |tab: &Tab| !searching && self.tab.as_ref() == Some(tab);
        let picker = cx.entity().downgrade();
        let mut items: Vec<(Option<Tab>, AnyElement)> = Vec::new();
        let count = entries.len() + 1;
        let fav = Tab::Favorites;
        let focus = self
            .tabs
            .entry(tab_key(&fav))
            .or_insert_with(|| cx.focus_handle().tab_stop(true))
            .clone();
        items.push((
            Some(fav.clone()),
            crate::view::controls::pointer_tip(
                rail_button(
                    "rail-favorites",
                    "Favorites".into(),
                    active(&fav),
                    false,
                    0,
                    count,
                    &focus,
                    {
                        let picker = picker.clone();
                        Rc::new(move |window, cx| {
                            let _ =
                                picker.update(cx, |p, cx| p.choose_tab(Tab::Favorites, window, cx));
                        })
                    },
                )
                .child(icons::icon(Icon::Star, 16.)),
                "rail-favorites-tip",
                "Favorites",
            )
            .into_any_element(),
        ));
        for (index, entry) in entries.iter().enumerate() {
            if entry.divider_before {
                // par-shell (D.3): `role="separator" title="Harnesses"`.
                items.push((
                    None,
                    crate::view::controls::pointer_tip(
                        div()
                            .id("rail-divider")
                            .role(Role::Splitter)
                            .aria_label("Harnesses")
                            .h(px(1.))
                            .mx(px(6.))
                            .my(px(3.))
                            .bg(colour::line()),
                        "rail-divider-tip",
                        "Harnesses",
                    )
                    .into_any_element(),
                ));
            }
            let tab = Tab::Provider(entry.id.clone());
            let focus = self
                .tabs
                .entry(entry.id.clone())
                .or_insert_with(|| cx.focus_handle().tab_stop(true))
                .clone();
            let id = entry.id.clone();
            let picker = picker.clone();
            let button = rail_button(
                ("rail-provider", index),
                entry.title.clone().into(),
                active(&tab),
                entry.disabled,
                index + 1,
                count,
                &focus,
                Rc::new(move |window, cx| {
                    let _ = picker.update(cx, |p, cx| {
                        p.choose_tab(Tab::Provider(id.clone()), window, cx)
                    });
                }),
            )
            .child(icons::logo(crate::state::vendor::logo_name(&entry.id), 18.))
            .when(entry.harness, |d| {
                d.child(
                    theme::text(
                        div().absolute().right(px(3.)).bottom(px(2.)).opacity(0.9),
                        TextKind::HarnessBadge,
                        &fonts,
                    )
                    .child("\u{203a}_"),
                )
            });
            // par-shell (D.3): the tab's title (why it is greyed) on hover.
            let button = crate::view::controls::pointer_tip(
                button,
                SharedString::from(format!("rail-provider-tip-{index}")),
                entry.title.clone(),
            );
            items.push((Some(tab), button.into_any_element()));
        }
        // The rail's lines: one where the popover can grow to it, else the
        // fewest its items fit in, shared out evenly (`--lines`,
        // `--per-line`); Set up providers spans them at the bottom.
        let available = height - 6. - 42.;
        let heights: Vec<f32> = items
            .iter()
            .map(|(tab, _)| if tab.is_some() { ROW } else { 7. })
            .collect();
        let (lines, per_line) = models::rail_lines(&heights, 3., available);
        let mut columns: Vec<Div> = (0..lines)
            .map(|_| div().w(px(31.)).flex().flex_col().gap(px(3.)))
            .collect();
        for (index, (_, element)) in items.into_iter().enumerate() {
            let column = index / per_line;
            let c = std::mem::replace(&mut columns[column], div());
            columns[column] = c.child(element);
        }
        let set_up = {
            let picker = picker.clone();
            let open_settings = self.set_up_press(cx);
            let press = move |window: &mut Window, cx: &mut App| {
                let _ = picker.update(cx, |p, cx| p.close(window, cx));
                open_settings(window, cx);
            };
            let press_key = press.clone();
            ring(
                div()
                    .id("rail-set-up")
                    .role(Role::Button)
                    .aria_label(SET_UP)
                    .track_focus(&self.set_up)
                    .key_context("Part")
                    .on_click(move |_, window, cx| press(window, cx))
                    .on_action(move |_: &Activate, window, cx| press_key(window, cx)),
            )
            .mt_auto()
            .mx(px(-6.))
            .min_h(px(42.))
            .flex()
            .items_center()
            .justify_center()
            .border_t_1()
            .border_color(colour::line())
            .bg(colour::bedrock())
            .text_color(colour::dim())
            .cursor_pointer()
            .hover(|s| s.bg(colour::raised()).text_color(colour::bone()))
            .child(icons::icon(Icon::Plus, 16.))
        };
        div()
            .flex()
            .flex_col()
            .flex_none()
            .h_full()
            .pt(px(6.))
            .px(px(6.))
            .bg(colour::bedrock())
            .border_r_1()
            .border_color(colour::line())
            .child(
                div()
                    .id("rail")
                    .role(Role::TabList)
                    .aria_label("Providers")
                    // The set's size on the container too (W4): the
                    // Windows adapter reads an item's from its container.
                    .aria_size_of_set(count)
                    .aria_orientation(accesskit::Orientation::Vertical)
                    .flex()
                    .gap(px(6.))
                    .children(columns),
            )
            .child(set_up)
            .into_any_element()
    }

    fn render_row(
        &mut self,
        index: usize,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let fonts = fonts(cx);
        let Some(listed) = self.listing.models.get(index).cloned() else {
            return div().into_any_element();
        };
        let providers = self.providers(cx);
        let catalog = self.catalog.read(cx);
        let searching = !self.query.trim().is_empty();
        let key = row_key(&listed.provider, &listed.model.id);
        let favorite = catalog
            .favorites()
            .iter()
            .any(|(p, m)| *p == listed.provider && *m == listed.model.id);
        let provider_label = providers
            .iter()
            .find(|p| p.id == listed.provider)
            .map(|p| p.label.clone())
            .unwrap_or_default();
        let name = models::row_name(&listed, &providers, searching);
        let pair = (listed.provider.clone(), listed.model.id.clone());
        let chosen = match &self.multi {
            Some(selected) => selected.contains(&pair),
            None => self.value.as_ref() == Some(&pair),
        };
        let multi = self.multi.is_some();
        let count = self.keys.len();
        let row_focus = self
            .rows
            .get(&key)
            .cloned()
            .expect("a listed row has its focus");
        let star_focus = self
            .stars
            .get(&key)
            .cloned()
            .expect("a listed row has its star");
        let picker = cx.entity().downgrade();
        let (p, m) = (listed.provider.clone(), listed.model.id.clone());
        let choose = {
            let picker = picker.clone();
            let (p, m) = (p.clone(), m.clone());
            Rc::new(move |window: &mut Window, cx: &mut App| {
                let _ = picker.update(cx, |this, cx| this.choose(p.clone(), m.clone(), window, cx));
            })
        };
        let star = {
            let catalog = self.catalog.clone();
            Rc::new(move |_: &mut Window, cx: &mut App| {
                catalog.update(cx, |c, cx| c.toggle_favorite(&p, &m, cx));
            })
        };
        let (choose_click, choose_key) = (choose.clone(), choose);
        let (star_click, star_key) = (star.clone(), star);
        let group: SharedString = format!("model-row-{index}").into();
        let star_button = ring(
            div()
                .id(("model-star", index))
                .role(Role::Button)
                .aria_label(models::star_name(&listed))
                .aria_toggled(if favorite {
                    accesskit::Toggled::True
                } else {
                    accesskit::Toggled::False
                })
                .aria_description(models::star_title(favorite))
                .track_focus(&star_focus)
                .key_context("Part")
                .on_click(move |_, window, cx| {
                    cx.stop_propagation();
                    star_click(window, cx)
                })
                .on_action(move |_: &Activate, window, cx| star_key(window, cx)),
        )
        .flex_none()
        .px(px(8.))
        .py(px(7.))
        .rounded(px(radius::ROW))
        .cursor_pointer()
        .text_color(if favorite {
            colour::star()
        } else {
            colour::dim()
        })
        .opacity(if favorite { 1. } else { 0. })
        .group_hover(group.clone(), |s| s.opacity(1.))
        .focus_visible(|s| s.opacity(1.))
        .hover(|s| s.text_color(colour::bone()))
        .child(icons::icon(Icon::Star, 14.));
        // par-shell (D.3): `Add favorite` / `Remove favorite` on hover.
        let star_button = crate::view::controls::pointer_tip(
            star_button,
            ("model-star-tip", index),
            models::star_title(favorite),
        );
        let option = ring(
            div()
                .id(("model-option", index))
                .role(Role::ListBoxOption)
                .aria_label(name)
                .aria_selected(chosen)
                // bench agent: multi-select reports each option checked.
                .when(multi, |d| {
                    d.aria_toggled(if chosen {
                        accesskit::Toggled::True
                    } else {
                        accesskit::Toggled::False
                    })
                })
                .aria_description(listed.model.id.clone())
                .aria_position_in_set(index)
                .aria_size_of_set(count)
                .track_focus(&row_focus)
                // chat-fix-2: a reader's ScrollIntoView on the option or its
                // star brings the row into view (and a Click or Focus on one
                // scrolls it first).
                .on_a11y_action(accesskit::Action::ScrollIntoView, {
                    let picker = cx.entity().downgrade();
                    move |_, _, cx| {
                        let _ = picker.update(cx, |this, cx| {
                            this.list.scroll_to_reveal_item(index);
                            cx.notify();
                        });
                    }
                })
                .key_context("Part")
                .on_click(move |_, window, cx| choose_click(window, cx))
                .on_action(move |_: &Activate, window, cx| choose_key(window, cx)),
        )
        .flex_1()
        .min_w_0()
        .flex()
        .items_center()
        .gap(px(8.))
        .pl(px(8.))
        .py(px(7.))
        .rounded(px(radius::ROW))
        .cursor_pointer()
        // par-shell (D.3): the model's full id on hover (`title=${model.id}`),
        // over the logo and the name (the star has its own).
        .child(crate::view::controls::pointer_tip(
            div()
                .id(("model-id", index))
                .flex_1()
                .min_w_0()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(icons::logo(
                    crate::state::vendor::vendor_icon(&listed.provider, &listed.model.id),
                    16.,
                ))
                .child(
                    theme::text(
                        div().flex_1().min_w_0().truncate(),
                        TextKind::MenuItem,
                        &fonts,
                    )
                    .when(chosen, |d| {
                        d.text_color(colour::bone()).font_weight(FontWeight(500.))
                    })
                    .child(models::display_name(&listed.model.name).to_string()),
                ),
            ("model-id-tip", index),
            listed.model.id.clone(),
        ))
        // bench agent: multi-select's check mark.
        .when(multi, |d| {
            d.child(
                div()
                    .flex_none()
                    .text_color(if chosen {
                        colour::bone()
                    } else {
                        colour::none()
                    })
                    .child(icons::icon(Icon::Check, 13.)),
            )
        })
        .when(searching, |d| {
            d.child(
                theme::text(div().flex_none(), TextKind::PickerProvider, &fonts)
                    .child(provider_label),
            )
        })
        // chat-fix-2: the star is the option's child, not the list's: the
        // list's items are its models alone (VoiceOver counts the list's
        // children: "1 of 4" for two models with their stars). It sits at
        // the row's right edge, 6 px after the text (`.model-choice`'s
        // right padding), as the page's; its own padding overlaps the
        // option's, so the row stays 34 px high.
        .child(div().flex_none().ml(px(6.)).my(px(-7.)).child(star_button));
        div()
            .id(("model-row", index))
            .group(group)
            // chat-fix-2: a block in the list (`.model-picker-row`), the
            // list's full width, the star at its right edge.
            .w_full()
            .flex()
            .items_center()
            .min_h(px(ROW))
            .rounded(px(radius::ROW))
            .when(chosen, |d| d.bg(colour::picker_selected()))
            .when(!chosen, |d| d.hover(|s| s.bg(colour::raised())))
            .child(option)
            .into_any_element()
    }

    fn render_list(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let fonts = fonts(cx);
        let entity = cx.entity();
        let note = |id: &'static str, text: String, error: bool| {
            theme::text(
                div()
                    .id(id)
                    .role(Role::Label)
                    .aria_label(text.clone())
                    .aria_value(text.clone())
                    .w_full()
                    .px(px(18.))
                    .py(px(24.))
                    .text_center()
                    .whitespace_normal(),
                TextKind::PickerNote,
                &fonts,
            )
            .when(error, |d| d.text_color(colour::err()))
            .child(text)
            .into_any_element()
        };
        let mut notes: Vec<AnyElement> = Vec::new();
        if self.listing.loading {
            notes.push(note("model-loading", models::LOADING.to_string(), false));
        }
        if let Some(error) = self.listing.error.clone() {
            notes.push(self.error_note(error, cx));
        }
        if let Some(empty) = self.listing.empty {
            notes.push(note("model-empty", empty.to_string(), false));
        }
        let picker = cx.entity().downgrade();
        let rows = list(self.list.clone(), move |index, window, cx| {
            entity.update(cx, |this, cx| this.render_row(index, window, cx))
        })
        .flex_1();
        let (prev, next, first, last) = (
            picker.clone(),
            picker.clone(),
            picker.clone(),
            picker.clone(),
        );
        let typed = picker.clone();
        let listbox = div()
            .id("model-list")
            // It clips its rows: one scrolled out is not showing (W2).
            .a11y_synthetic_children(|builder| {
                builder.parent_node().set_clips_children();
            })
            .role(Role::ListBox)
            .aria_label(self.look.label)
            .aria_size_of_set(self.keys.len())
            .key_context("Menu")
            .on_action(move |_: &MovePrevious, w, cx| {
                let _ = prev.update(cx, |p, cx| p.step_rows(Step::Previous, w, cx));
            })
            .on_action(move |_: &MoveNext, w, cx| {
                let _ = next.update(cx, |p, cx| p.step_rows(Step::Next, w, cx));
            })
            .on_action(move |_: &MoveFirst, w, cx| {
                let _ = first.update(cx, |p, cx| p.step_rows(Step::First, w, cx));
            })
            .on_action(move |_: &MoveLast, w, cx| {
                let _ = last.update(cx, |p, cx| p.step_rows(Step::Last, w, cx));
            })
            .on_key_down(move |event, window, cx| {
                let k = &event.keystroke;
                let plain = !k.modifiers.control && !k.modifiers.platform && !k.modifiers.alt;
                if let Some(text) = k.key_char.clone().filter(|t| plain && !t.trim().is_empty()) {
                    let _ = typed.update(cx, |p, cx| p.type_on(&text, window, cx));
                    cx.stop_propagation();
                }
            })
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(rows);
        // The column takes the popover's width less the rail and no more
        // (`min_w_0`): a long error wraps inside it.
        div()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .flex()
            .flex_col()
            .p(px(4.))
            .children(notes)
            .child(listbox)
            .into_any_element()
    }

    /// The catalogue's error (`.model-picker-note.error`): wrapped inside the
    /// list column, centred, in the error colour, selectable
    /// ([`crate::view::controls::selectable`]).
    fn error_note(&mut self, text: String, cx: &mut Context<Self>) -> AnyElement {
        let fonts = fonts(cx);
        let note = crate::view::controls::selectable::selectable(
            "model-error",
            text,
            font(fonts.ui.clone()),
            colour::err(),
            |p: &mut Self| &mut p.note,
            self,
            cx,
        );
        theme::text(
            note.w_full().px(px(18.)).py(px(24.)).text_center(),
            TextKind::PickerNote,
            &fonts,
        )
        .into_any_element()
    }

    fn render_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let fonts = fonts(cx);
        let viewport = window.viewport_size();
        let width = (360f32).min(f32::from(viewport.width) - 24.);
        let max_h = f32::from(viewport.height) - 96.;
        let height = self.rail_one_line(cx).max(400.).min(max_h);
        let rail = self.render_rail(height - 2., cx);
        let any_usable = self.providers(cx).iter().any(models::usable);
        let main = if !any_usable {
            let open_settings = self.set_up_press(cx);
            let picker = cx.entity().downgrade();
            let connect = move |window: &mut Window, cx: &mut App| {
                let _ = picker.update(cx, |p, cx| p.close(window, cx));
                open_settings(window, cx);
            };
            let connect_key = connect.clone();
            div()
                .flex_1()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(12.))
                .px(px(18.))
                .py(px(24.))
                .child(
                    theme::text(
                        div()
                            .id("no-ai")
                            .role(Role::Label)
                            .aria_label(models::NO_AI)
                            .aria_value(models::NO_AI),
                        TextKind::PickerNoAi,
                        &fonts,
                    )
                    .child(models::NO_AI),
                )
                .child(
                    theme::text(
                        ring(
                            div()
                                .id("connect-ai")
                                .role(Role::Button)
                                .aria_label(models::CONNECT_AI)
                                .track_focus(&self.connect)
                                .key_context("Part")
                                .on_click(move |_, window, cx| connect(window, cx))
                                .on_action(move |_: &Activate, window, cx| connect_key(window, cx)),
                        )
                        .min_h(px(32.))
                        .px(px(14.))
                        .flex()
                        .items_center()
                        .rounded(px(radius::SM))
                        .bg(colour::bone())
                        .shadow(shadow::primary_face())
                        .cursor_pointer()
                        .hover(|s| s.bg(colour::primary_hover())),
                        TextKind::Button,
                        &fonts,
                    )
                    .text_color(colour::bedrock())
                    .text_size(px(12.5))
                    .child(models::CONNECT_AI),
                )
                .into_any_element()
        } else {
            let picker = cx.entity().downgrade();
            let search = div()
                .flex()
                .items_center()
                .gap(px(8.))
                .min_h(px(42.))
                .px(px(11.))
                .py(px(2.))
                .border_b_1()
                .border_color(colour::line())
                .text_color(colour::dim())
                .child(icons::icon(Icon::Search, 14.))
                .child(
                    theme::text(
                        div()
                            .id("model-search")
                            .flex_1()
                            .capture_action({
                                let picker = picker.clone();
                                move |_: &MoveDown, window, cx| {
                                    cx.stop_propagation();
                                    let _ = picker.update(cx, |p, cx| {
                                        if !p.keys.is_empty() {
                                            p.focus_row(0, window, cx)
                                        }
                                    });
                                }
                            })
                            .capture_action(|_: &MoveUp, _, cx| cx.stop_propagation()),
                        TextKind::PickerSearch,
                        &fonts,
                    )
                    .child(
                        Input::new(&self.search)
                            .aria_label(SEARCH_NAME)
                            .appearance(false),
                    ),
                );
            let footer = self.footer.then(|| {
                let catalog = self.catalog.clone();
                let picker = picker.clone();
                let press = move |window: &mut Window, cx: &mut App| {
                    let hooks = catalog.read(cx).hooks();
                    let _ = picker.update(cx, |p, cx| p.close(window, cx));
                    (hooks.compare_in_bench)(window, cx);
                };
                let press_key = press.clone();
                theme::text(
                    ring(
                        div()
                            .id("model-footer")
                            .role(Role::Button)
                            .aria_label(FOOTER)
                            .track_focus(&self.footer_focus)
                            .key_context("Part")
                            .on_click(move |_, window, cx| press(window, cx))
                            .on_action(move |_: &Activate, window, cx| press_key(window, cx)),
                    )
                    .flex()
                    .items_center()
                    .gap(px(9.))
                    .min_h(px(42.))
                    .pl(px(12.))
                    .pr(px(14.))
                    .border_t_1()
                    .border_color(colour::line())
                    .cursor_pointer()
                    .hover(|s| s.bg(colour::raised()).text_color(colour::bone())),
                    TextKind::PickerFooter,
                    &fonts,
                )
                .child(icons::icon(Icon::Bench, 15.))
                .child(div().flex_1().child(FOOTER))
                .child(icons::icon(Icon::Forward, 13.))
            });
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .child(search)
                .child(self.render_list(cx))
                .children(footer)
                .into_any_element()
        };
        let picker = cx.entity().downgrade();
        div()
            .id("model-popover")
            .role(Role::Dialog)
            .aria_label(self.look.label)
            .w(px(width))
            .h(px(height))
            .flex()
            .rounded(px(radius::MENU))
            .border_1()
            .border_color(colour::line())
            .bg(colour::well())
            .shadow(shadow::panel())
            .overflow_hidden()
            .child(rail)
            .child(main)
            .child(
                canvas(
                    |_, _, _| {},
                    move |_, _, _, cx| {
                        let _ = picker.update(cx, |p, _| p.painted());
                    },
                )
                .absolute()
                .size_0(),
            )
            .into_any_element()
    }
}

#[allow(clippy::too_many_arguments)]
fn rail_button(
    id: impl Into<ElementId>,
    name: SharedString,
    active: bool,
    disabled: bool,
    index: usize,
    count: usize,
    focus: &FocusHandle,
    press: crate::view::Press,
) -> Stateful<Div> {
    let (click, key) = (press.clone(), press);
    ring(
        div()
            .id(id.into())
            .role(Role::Tab)
            .aria_label(name)
            .aria_selected(active)
            .aria_position_in_set(index)
            .aria_size_of_set(count)
            .when(disabled, |d| {
                d.a11y_synthetic_children(|builder| builder.parent_node().set_disabled())
            })
            .track_focus(&focus.clone().tab_stop(!disabled))
            .key_context("Part")
            .when(!disabled, |d| {
                d.on_click(move |_, window, cx| click(window, cx))
                    .on_action(move |_: &Activate, window, cx| key(window, cx))
            }),
    )
    .relative()
    .w(px(31.))
    .min_h(px(ROW))
    .flex()
    .items_center()
    .justify_center()
    .rounded(px(7.))
    .text_color(colour::dim())
    .when(active, |d| {
        d.bg(colour::raised()).text_color(colour::bone())
    })
    .when(disabled, |d| d.opacity(0.28))
    .when(!disabled, |d| {
        d.cursor_pointer()
            .hover(|s| s.bg(colour::raised()).text_color(colour::bone()))
    })
}

impl Render for ModelPicker {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.look.asks && self.catalog.update(cx, |c, _| c.take_ask()) && self.value.is_none() {
            self.open(window, cx);
            self.opened_by_itself = true;
        }
        let trigger = self.render_trigger(cx);
        let shown = self.popover.read(cx).phase(cx.background_executor().now())
            != crate::state::popover::Phase::Closed;
        let content = shown.then(|| self.render_panel(window, cx));
        let panel = content.and_then(|content| {
            if self.look.below {
                // settings agent: under the trigger, right-aligned.
                crate::view::controls::popover::panel(
                    &self.popover,
                    crate::view::controls::popover::Placement::Below,
                    content,
                    cx,
                )
            } else {
                panel(&self.popover, Entry::Surface, content, cx)
            }
        });
        div()
            .relative()
            .flex_initial()
            .min_w_0()
            .child(trigger)
            .children(panel)
    }
}
