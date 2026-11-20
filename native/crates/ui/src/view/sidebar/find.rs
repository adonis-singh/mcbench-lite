//! The sidebar's search and filters (`.side-find`, `FindFilter`,
//! `FilterChips`; inventory `app.md` 4.3, 4.5, 4.8, `components.md` J,
//! `style-viewer-routes.md` 1.5). The field searches the mode's list in the
//! core's full-text index 90 ms after the last key (`Command::Search`);
//! the newest request wins and an answer to an older one is dropped. Escape
//! in it clears the search, leaves the field, and goes on to the window's
//! Escape. The filter button opens a menu of three groups whose values
//! toggle in place: focus goes to the first value on open; Up and Down
//! wrap, Home and End go to the ends; Escape closes and gives focus back to
//! the button; Tab out of it closes it and goes on from the button (to the
//! stop after it, Shift+Tab to the one before), as a menu button does; a
//! press outside closes it. The values in
//! effect show as chips under the field, each removed by its own button
//! (focus to the next chip's, else the previous one's, else the filter
//! button), and `Clear all`. Both the text and the filters are the mode's
//! own: a mode switch clears them.

use std::collections::HashMap;
use std::time::Duration;

use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::super::controls::popover::{Placement, PopoverState, panel};
use super::super::controls::{Button, fonts};
use super::super::icons::{self, Icon};
use super::super::theme::{self, TextKind, colour, radius, shadow};
use super::super::{
    Activate, FocusNext, FocusPrevious, MoveFirst, MoveLast, MoveNext, MovePrevious, ring,
};
use super::{FindEscape, Sidebar};
use crate::state::Step;
use crate::state::filters::{self, Filters, GROUPS, Group, Label, Offer, Value};
use crate::state::place::Mode;
use crate::{SearchHit, SearchScope};

/// The search's wait after the last key.
pub const DEBOUNCE: Duration = Duration::from_millis(90);

/// The search field and the filters.
#[derive(Default)]
pub struct Find {
    /// The field, made at the first frame.
    pub input: Option<Entity<InputState>>,
    /// The text searched (trimmed).
    pub query: String,
    /// The core's ranked matches for the query, none while nothing is
    /// searched.
    pub found: Option<Vec<SearchHit>>,
    /// Why the last search failed (chat-fix), shown where the rows would be.
    pub failed: Option<String>,
    /// The request whose answer may land.
    pub request: Option<u64>,
    typed: u64,
    /// The values chosen.
    pub filters: Filters,
    /// The filter menu.
    pub menu: Option<Entity<PopoverState>>,
    chips: HashMap<Value, FocusHandle>,
    clear: Option<FocusHandle>,
    _subscription: Option<Subscription>,
}

impl Sidebar {
    /// The field and the menu's state, made at the first frame.
    pub(crate) fn ensure_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.find.input.is_some() {
            return;
        }
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Search"));
        let subscription = cx.subscribe_in(
            &input,
            window,
            |this: &mut Self, input, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::Change) {
                    let text = input.read(cx).value().to_string();
                    this.find_typed(text, cx);
                }
            },
        );
        self.find.input = Some(input);
        self.find._subscription = Some(subscription);
        self.find.menu = Some(cx.new(|cx| PopoverState::new(crate::state::popover::MENU_EXIT, cx)));
        self.find.clear = Some(cx.focus_handle().tab_stop(true));
    }

    /// The field's text changed: searched after 90 ms without another key;
    /// an empty query lists every row at once.
    fn find_typed(&mut self, text: String, cx: &mut Context<Self>) {
        let query = text.trim().to_string();
        // The search's text is trimmed: a space typed changes nothing.
        if query == self.find.query {
            return;
        }
        self.find.typed += 1;
        self.find.query = query.clone();
        if query.is_empty() {
            self.find.found = None;
            self.find.failed = None;
            self.find.request = None;
            cx.notify();
            return;
        }
        let typed = self.find.typed;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(DEBOUNCE).await;
            let _ = this.update(cx, |this, cx| {
                if this.find.typed != typed {
                    return;
                }
                let scope = match this.mode {
                    Mode::Build => SearchScope::Build,
                    Mode::Bench => SearchScope::Bench,
                };
                let request = this.search(scope, query, cx);
                this.find.request = Some(request);
            });
        })
        .detach();
    }

    /// An answer for the field; one for an older request is dropped.
    /// A failed search narrows the list to nothing and says why there.
    pub(crate) fn find_found(
        &mut self,
        request: u64,
        results: Result<Vec<SearchHit>, String>,
    ) -> bool {
        if self.find.request != Some(request) {
            return false;
        }
        match results {
            Ok(hits) => {
                self.find.found = Some(hits);
                self.find.failed = None;
            }
            Err(error) => {
                self.find.found = Some(Vec::new());
                self.find.failed = Some(error);
                self.find_failed_text.clear();
            }
        }
        true
    }

    /// Where the list's rows would be when the search failed: its sentence,
    /// selectable; else `No matches`.
    pub(crate) fn no_rows(&mut self, id: &'static str, cx: &mut Context<Self>) -> AnyElement {
        let Some(error) = self.find.failed.clone() else {
            return super::parts::side_note(id, "No matches", cx).into_any_element();
        };
        let fonts = fonts(cx);
        let note = crate::view::controls::selectable::selectable(
            "find-failed",
            error,
            font(fonts.ui.clone()),
            colour::err(),
            |s: &mut Self| &mut s.find_failed_text,
            self,
            cx,
        );
        theme::text(note, TextKind::BenchMember, &fonts)
            .px(px(10.))
            .py(px(8.))
            .into_any_element()
    }

    /// Clears the text and the filters (a mode switch, Escape in the field
    /// clears the text only).
    pub(crate) fn clear_find(
        &mut self,
        filters: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.find.typed += 1;
        self.find.query.clear();
        self.find.found = None;
        self.find.failed = None;
        self.find.request = None;
        if filters {
            self.find.filters = Filters::default();
        }
        if let Some(input) = self.find.input.clone() {
            input.update(cx, |i, cx| i.set_value("", window, cx));
        }
        cx.notify();
    }

    fn find_escape(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.clear_find(false, window, cx);
        self.log("search cleared", serde_json::json!({}));
        // The field is left: focus goes to the list's current row (never
        // to nothing: W1).
        self.focus_current(window, cx);
        // And Escape goes on to the window (`closeTopLayer`) from a field:
        // a layer closes, an edit in the composer stays (par-chat).
        super::super::keys::close_top_from_field(window, cx);
    }

    /// The facets of every row the mode lists, the offer, and the labels.
    pub(crate) fn offer(&self, cx: &App) -> (Offer, Vec<(Value, Label)>) {
        let lists = self.lists.read(cx);
        let titles: HashMap<String, String> = lists
            .published()
            .prompt_titles
            .iter()
            .map(|(k, v)| (k.clone(), v.as_str().unwrap_or_default().to_string()))
            .collect();
        let library: Vec<(String, String)> = self
            .prompts
            .library()
            .iter()
            .map(|p| (p.id.clone(), p.title.clone()))
            .collect();
        let facets: Vec<filters::Facets> = match self.mode {
            Mode::Build => lists
                .chats()
                .into_iter()
                .map(filters::chat_facets)
                .collect(),
            Mode::Bench => lists
                .benches()
                .iter()
                .map(|b| {
                    let members: Vec<&mcbench_store::ConversationSummary> =
                        crate::state::sidebar::members(b, lists.all())
                            .into_iter()
                            .map(|m| &m.summary)
                            .collect();
                    filters::bench_facets(b, &members)
                })
                .collect(),
        };
        let offer = filters::offered(&facets, &library, &titles);
        let providers = &self.providers;
        let labels = filters::labels(&offer, |key| {
            providers
                .iter()
                .find(|p| p.id == key)
                .map(|p| p.label.clone())
        });
        (offer, labels)
    }

    /// The filters in effect (`shownFilters`).
    pub(crate) fn shown_filters(&self, offer: &Offer) -> Filters {
        if self.find.filters.filtering() {
            filters::in_effect(&self.find.filters, offer)
        } else {
            self.find.filters.clone()
        }
    }

    fn toggle_filter(&mut self, value: Value, cx: &mut Context<Self>) {
        self.find.filters = self.find.filters.toggled(&value);
        self.log("filter", serde_json::json!({ "value": format!("{value:?}"), "on": self.find.filters.has(&value) }));
        cx.notify();
    }

    /// The search row: the field and the filter button with its menu.
    pub(crate) fn find_row(
        &mut self,
        offer: &Offer,
        labels: &[(Value, Label)],
        shown: &Filters,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let fonts = fonts(cx);
        let input = self.find.input.clone().expect("made at the first frame");
        let menu = self.find.menu.clone().expect("made at the first frame");
        let focused = input.read(cx).focus_handle(cx).is_focused(window);
        let label = match self.mode {
            Mode::Build => "Search chats",
            Mode::Bench => "Search benches",
        };
        let chosen = shown.count();
        let button_label = if chosen > 0 {
            format!("Filter, {chosen} chosen")
        } else {
            "Filter".to_string()
        };
        let open = menu.read(cx).is_open();
        let trigger = super::super::controls::popover::trigger(
            &menu,
            Button::icon("find-filter", Icon::Filter, button_label).tip("Filter"),
            cx,
        );
        let menu_panel = panel(
            &menu,
            Placement::Below,
            self.filter_menu(&menu, offer, labels, shown, cx),
            cx,
        );
        // Focus leaving the menu any other way (a reader moving it, a
        // press outside) closes it, as a press outside does.
        if open {
            let inside = (0..menu_count(offer))
                .any(|i| menu.update(cx, |m, cx| m.item(i, cx)).is_focused(window))
                || menu.read(cx).trigger().is_focused(window);
            if !inside {
                let menu = menu.clone();
                cx.defer_in(window, move |_, window, cx| {
                    menu.update(cx, |m, cx| m.close_keeping_focus(window, cx));
                });
            }
        }
        let field = theme::text(
            div()
                .key_context("SideFind")
                .on_action(
                    cx.listener(|this, _: &FindEscape, window, cx| this.find_escape(window, cx)),
                )
                .flex_1()
                .min_w_0(),
            TextKind::SideSearch,
            &fonts,
        )
        .child(
            Input::new(&input)
                .aria_label(label)
                .appearance(false)
                .text_size(px(13.)),
        );
        div()
            .relative()
            .h(px(32.))
            .mx(px(2.))
            .pl(px(10.))
            .pr(px(3.))
            .flex()
            .items_center()
            .gap(px(7.))
            .rounded(px(radius::SM))
            .bg(colour::raised())
            .shadow(if focused {
                theme::sidebar::find_focus()
            } else {
                shadow::line_ring()
            })
            .child(
                div()
                    .flex_none()
                    .text_color(colour::dim())
                    .child(icons::icon(Icon::Search, 14.)),
            )
            .child(field)
            .child(
                div()
                    .relative()
                    .flex_none()
                    .size(px(26.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(if chosen > 0 || open {
                        colour::bone()
                    } else {
                        colour::dim()
                    })
                    .child(trigger)
                    .children(menu_panel),
            )
            .into_any_element()
    }

    /// The menu (`.tag-menu.find-menu`): three groups, each value a
    /// checkbox item; under Prompt, when no row came from a library prompt,
    /// the mode's sentence.
    fn filter_menu(
        &mut self,
        menu: &Entity<PopoverState>,
        offer: &Offer,
        labels: &[(Value, Label)],
        shown: &Filters,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let fonts = fonts(cx);
        let count = menu_count(offer);
        let mut list = div()
            .id("find-menu")
            // It clips its rows: one scrolled out is not showing (W2).
            .a11y_synthetic_children(|builder| {
                builder.parent_node().set_clips_children();
            })
            .role(Role::Menu)
            .aria_label("Filter")
            .key_context("Menu")
            .w(px(self.width - 22.))
            .max_h(px(440.))
            .overflow_y_scroll()
            .pt(px(3.))
            .px(px(5.))
            .pb(px(5.))
            .rounded(px(radius::MENU))
            .border_1()
            .border_color(colour::line())
            .bg(colour::well())
            .shadow(shadow::menu())
            .flex()
            .flex_col();
        // Tab out of the menu closes it and goes on from the Filter button,
        // as from a menu button: to the stop after it (Shift+Tab: before it).
        let tab = menu.clone();
        let back = menu.clone();
        list = list
            .on_action(move |_: &FocusNext, window, cx| {
                tab.update(cx, |m, cx| m.close(window, cx));
                window.focus_next(cx);
            })
            .on_action(move |_: &FocusPrevious, window, cx| {
                back.update(cx, |m, cx| m.close(window, cx));
                window.focus_prev(cx);
            });
        for (step, which) in [
            (Step::Previous, 0),
            (Step::Next, 1),
            (Step::First, 2),
            (Step::Last, 3),
        ] {
            let m = menu.clone();
            let mover = move |window: &mut Window, cx: &mut App| {
                m.update(cx, |p, cx| p.step(count, step, window, cx))
            };
            list = match which {
                0 => list.on_action(move |_: &MovePrevious, w, cx| mover(w, cx)),
                1 => list.on_action(move |_: &MoveNext, w, cx| mover(w, cx)),
                2 => list.on_action(move |_: &MoveFirst, w, cx| mover(w, cx)),
                _ => list.on_action(move |_: &MoveLast, w, cx| mover(w, cx)),
            };
        }
        let empty_prompts = match self.mode {
            Mode::Build => "No chat was sent from a library prompt yet",
            Mode::Bench => "No bench was sent from a library prompt yet",
        };
        let mut index = 0;
        for (g, group) in GROUPS.iter().enumerate() {
            let values: Vec<Value> = match group {
                Group::Size => offer.size.iter().map(|v| Value::Size(*v)).collect(),
                Group::Prompt => offer
                    .prompt
                    .iter()
                    .map(|p| Value::Prompt(p.id.clone()))
                    .collect(),
                Group::Model => offer
                    .model
                    .iter()
                    .map(|v| Value::Model(v.clone()))
                    .collect(),
            };
            // chat-fix-2: the group holds the set's size (the AT-SPI and
            // Windows adapters read an item's from its container).
            let group_size = values.len();
            let mut items = div()
                .id(("find-group", g))
                .role(Role::Group)
                .aria_label(group.heading())
                .aria_size_of_set(group_size)
                .flex()
                .flex_col()
                .when(g > 0, |d| {
                    d.mt(px(3.))
                        .pt(px(3.))
                        .border_t_1()
                        .border_color(colour::line())
                })
                .child(
                    theme::text(
                        div().px(px(8.)).pt(px(6.)).pb(px(4.)),
                        TextKind::SettingsNavLabel,
                        &fonts,
                    )
                    .child(group.heading()),
                );
            if *group == Group::Prompt && values.is_empty() {
                items = items.child(
                    theme::text(div().px(px(8.)).py(px(5.)), TextKind::ArchivedMeta, &fonts)
                        .child(empty_prompts),
                );
            }
            // chat-fix-2: an item's position and the set's size are its
            // group's (a reader says "2 of 4" within Size), not the menu's.
            for (in_group, value) in values.into_iter().enumerate() {
                let label = labels
                    .iter()
                    .find(|(v, _)| *v == value)
                    .map(|(_, l)| l.clone())
                    .expect("every offered value is labelled");
                let on = shown.has(&value);
                let focus = menu.update(cx, |m, cx| m.item(index, cx));
                let toggle = value.clone();
                let toggle_key = value.clone();
                let row =
                    ring(
                        div()
                            .id(("find-value", index))
                            .role(Role::MenuItemCheckBox)
                            .aria_label(label.text.clone())
                            .aria_description(label.full.clone())
                            .aria_toggled(if on {
                                accesskit::Toggled::True
                            } else {
                                accesskit::Toggled::False
                            })
                            .aria_position_in_set(in_group)
                            .aria_size_of_set(group_size)
                            .track_focus(&focus)
                            .key_context("Part")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.toggle_filter(toggle.clone(), cx)
                            }))
                            .on_action(cx.listener(move |this, _: &Activate, _, cx| {
                                this.toggle_filter(toggle_key.clone(), cx)
                            })),
                    )
                    .min_h(px(30.))
                    .px(px(8.))
                    .py(px(5.))
                    .rounded(px(radius::ROW))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .cursor_pointer()
                    .hover(|s| s.bg(colour::raised()).text_color(colour::bone()))
                    .focus_visible(|s| s.bg(colour::raised()).text_color(colour::bone()))
                    .when(on, |d| d.text_color(colour::bone()))
                    .when_some(label.logo, |d, logo| {
                        d.child(
                            div()
                                .text_color(colour::ash())
                                .child(icons::logo(logo, 14.)),
                        )
                    })
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .child(label.text.clone()),
                    )
                    .when_some(label.note.clone(), |d, note| {
                        d.child(theme::text(div(), TextKind::ArchivedMeta, &fonts).child(note))
                    })
                    .when(on, |d| d.child(icons::icon(Icon::Check, 13.)));
                items = items.child(
                    theme::text(row, TextKind::MenuItem, &fonts)
                        .when(on, |d| d.text_color(colour::bone())),
                );
                index += 1;
            }
            list = list.child(items);
        }
        list.into_any_element()
    }

    /// The chips of the values in effect (`FilterChips`), when any.
    pub(crate) fn chips(
        &mut self,
        labels: &[(Value, Label)],
        shown: &Filters,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !shown.filtering() {
            return None;
        }
        let fonts = fonts(cx);
        let values = shown.values();
        let mut row = div()
            .id("find-chips")
            .role(Role::Group)
            .aria_label("Filters")
            .flex()
            .flex_wrap()
            .gap(px(4.))
            .mt(px(-2.))
            .mx(px(2.));
        for (index, value) in values.iter().enumerate() {
            let label = labels
                .iter()
                .find(|(v, _)| v == value)
                .map(|(_, l)| l.clone())
                .expect("a value in effect is offered");
            let focus = self
                .find
                .chips
                .entry(value.clone())
                .or_insert_with(|| cx.focus_handle().tab_stop(true))
                .clone();
            let remove = value.clone();
            let remove_key = value.clone();
            let all = values.clone();
            let all_key = values.clone();
            let chip = div()
                .id(("find-chip", index))
                .h(px(22.))
                .pl(px(7.))
                .pr(px(2.))
                .flex()
                .items_center()
                .gap(px(4.))
                .rounded(px(radius::ICON))
                .bg(colour::raised())
                .shadow(shadow::line_ring())
                .when_some(label.logo, |d, logo| d.child(icons::logo(logo, 12.)))
                .child(
                    theme::text(div(), TextKind::Button, &fonts)
                        .text_size(px(12.))
                        .child(label.chip.clone()),
                )
                .child(
                    ring(
                        div()
                            .id(("find-chip-remove", index))
                            .role(Role::Button)
                            .aria_label(format!("Remove filter {}", label.full))
                            .track_focus(&focus)
                            .key_context("Part")
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.remove_chip(&remove, &all, window, cx)
                            }))
                            .on_action(cx.listener(move |this, _: &Activate, window, cx| {
                                this.remove_chip(&remove_key, &all_key, window, cx)
                            })),
                    )
                    .size(px(16. + 4.))
                    .rounded(px(4.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(colour::dim())
                    .hover(|s| s.bg(colour::hover()).text_color(colour::bone()))
                    .cursor_pointer()
                    .child(icons::icon(Icon::X, 12.)),
                );
            // chat-fix-2: the chip has no node of its own; its button's name
            // says the full name, so the tip is the pointer's alone.
            row = row.child(super::super::controls::pointer_tip(
                chip,
                ("find-chip-tip", index),
                label.full.clone(),
            ));
        }
        let clear = self.find.clear.clone().expect("made at the first frame");
        row = row.child(
            theme::text(
                ring(
                    div()
                        .id("find-clear")
                        .role(Role::Button)
                        .aria_label("Clear all")
                        .track_focus(&clear)
                        .key_context("Part")
                        .on_click(cx.listener(|this, _, window, cx| this.clear_filters(window, cx)))
                        .on_action(cx.listener(|this, _: &Activate, window, cx| {
                            this.clear_filters(window, cx)
                        })),
                ),
                TextKind::Button,
                &fonts,
            )
            .h(px(22. + 4.))
            .px(px(7.))
            .text_size(px(12.))
            .flex()
            .items_center()
            .rounded(px(radius::ICON))
            .text_color(colour::ash())
            .hover(|s| s.text_color(colour::bone()).bg(colour::hover()))
            .cursor_pointer()
            .child("Clear all"),
        );
        Some(row.into_any_element())
    }

    /// Removes a chip's value; focus moves to the next chip, else the
    /// previous, else the filter button.
    fn remove_chip(
        &mut self,
        value: &Value,
        all: &[Value],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let at = all.iter().position(|v| v == value).expect("a chip's value");
        let next = all
            .get(at + 1)
            .or(if at > 0 { all.get(at - 1) } else { None });
        self.toggle_filter(value.clone(), cx);
        match next.and_then(|v| self.find.chips.get(v)) {
            Some(handle) => window.focus(handle, cx),
            None => self.focus_filter_button(window, cx),
        }
    }

    fn clear_filters(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.find.filters = Filters::default();
        self.log("filters cleared", serde_json::json!({}));
        self.focus_filter_button(window, cx);
        cx.notify();
    }

    fn focus_filter_button(&self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(menu) = &self.find.menu {
            let trigger = menu.read(cx).trigger();
            window.focus(&trigger, cx);
        }
    }
}

/// How many values the menu lists.
fn menu_count(offer: &Offer) -> usize {
    offer.size.len() + offer.prompt.len() + offer.model.len()
}
