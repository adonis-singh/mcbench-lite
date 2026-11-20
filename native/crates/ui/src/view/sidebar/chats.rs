//! The Build chat list (`.conv-list`, inventory `app.md` 4.8): one row per
//! Build chat in the list's order (by search rank while searching),
//! narrowed by the filters, the chat on screen marked current. A row shows
//! its status dot, its title with any search snippet, its resolution tag
//! above Standard, and its archive button (on hover or its own focus;
//! disabled and hidden while the chat runs). Up, Down, Home and End move
//! among the rows without opening them; Enter, Space, a click or a reader's
//! press opens the focused row. A row's name is its title, `open` on the
//! open chat, its status when it is not idle (`Working`, `Finished, not
//! opened yet`), and its snippet.
//! When searching or filtering left nothing, `No matches`.
//!
//! The list is virtual (a [`ListState`] with its neighbours kept): only the
//! rows on screen, one above and one below, and the focused row are laid
//! out and in the tree; each row carries its position and the list's size.

use std::collections::HashMap;
use std::rc::Rc;

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::super::icons::Icon;
use super::super::{MoveFirst, MoveLast, MoveNext, MovePrevious, OpenChat, colour, ring};
use super::Sidebar;
use super::parts::{Shown, resolution_tag, row_icon, row_text};
use crate::state::place::{Mode, Place};
use crate::state::sidebar::{Dot, chat_dot, row_name};
use crate::state::{Step, filters, step};
use crate::{Command, Subject};

/// A listed Build row as drawn.
#[derive(Clone, Debug, PartialEq)]
pub struct BuildRow {
    /// Its chat.
    pub id: String,
    /// Its title.
    pub title: String,
    /// Its search snippet.
    pub snippet: Option<String>,
    /// Its dot.
    pub dot: Dot,
    /// Its dot's arrival.
    pub arrival: Option<u32>,
    /// Its resolution.
    pub resolution: i64,
}

/// The list's own state: the rows drawn, the current row, each row's
/// focus and its archive button's.
pub struct ChatList {
    current: usize,
    focus: HashMap<String, FocusHandle>,
    archive: HashMap<String, FocusHandle>,
    pub(crate) list: ListState,
    pub(crate) rows: Vec<BuildRow>,
}

impl Default for ChatList {
    fn default() -> Self {
        ChatList {
            current: 0,
            focus: HashMap::new(),
            archive: HashMap::new(),
            list: ListState::new(0, ListAlignment::Top, px(200.)).keep_neighbours(),
            rows: Vec::new(),
        }
    }
}

impl Sidebar {
    fn move_chat(&mut self, to: Step, window: &mut Window, cx: &mut Context<Self>) {
        let count = self.chats.rows.len();
        if let Some(index) = step(Some(self.chats.current), count, to) {
            self.chats.current = index;
            self.chats.list.scroll_to_reveal_item(index);
            if let Some(handle) = self.chats.focus.get(&self.chats.rows[index].id) {
                window.focus(handle, cx);
            }
            cx.notify();
        }
    }

    fn open_row(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        self.go(Place::Chat { id }, window, cx);
    }

    /// Archives a chat (`archiveConversation`): it leaves the list when the
    /// core archives it, and the view moves with it; the archived section
    /// is read again if open.
    pub(crate) fn archive_chat(&mut self, id: String, cx: &mut Context<Self>) {
        self.log("archive", serde_json::json!({ "chat": id }));
        self.send(Command::Archive(Subject::Chat(id)));
        self.read_archive_again(cx);
    }

    /// The rows the list shows now (`buildRows`).
    fn build_rows(
        &mut self,
        open: Option<&str>,
        shown: &filters::Filters,
        cx: &App,
    ) -> Vec<BuildRow> {
        let lists = self.lists.read(cx);
        let chats: Vec<&crate::ListedChat> = lists
            .all()
            .iter()
            .filter(|c| c.summary.bench_id.is_none())
            .collect();
        let found: Option<Vec<String>> = self
            .find
            .found
            .as_ref()
            .map(|f| f.iter().map(|h| h.id.clone()).collect());
        let listed = filters::listed(
            &chats,
            |c| c.summary.id.0.as_str(),
            found.as_deref(),
            shown,
            |c| filters::chat_facets(&c.summary),
        );
        let snippets: HashMap<&str, &str> = self
            .find
            .found
            .iter()
            .flatten()
            .filter_map(|h| h.snippet.as_deref().map(|s| (h.id.as_str(), s)))
            .filter(|(_, s)| !s.is_empty())
            .collect();
        listed
            .into_iter()
            .map(|c| {
                let id = c.summary.id.0.clone();
                let dot = chat_dot(c, open);
                BuildRow {
                    snippet: snippets.get(id.as_str()).map(|s| s.to_string()),
                    arrival: self.dots.arrival(&id, dot),
                    title: c.summary.title.clone(),
                    resolution: c.summary.resolution,
                    dot,
                    id,
                }
            })
            .collect()
    }

    /// Tells the virtual list which rows changed: the rows kept at both
    /// ends keep their measured heights and the scroll position.
    fn splice_rows(&mut self, rows: Vec<BuildRow>, cx: &mut Context<Self>) {
        let key = |r: &BuildRow| (r.id.clone(), r.snippet.is_some());
        let old: Vec<_> = self.chats.rows.iter().map(key).collect();
        let new: Vec<_> = rows.iter().map(key).collect();
        if old != new {
            let head = old.iter().zip(&new).take_while(|(a, b)| a == b).count();
            let tail = old[head..]
                .iter()
                .rev()
                .zip(new[head..].iter().rev())
                .take_while(|(a, b)| a == b)
                .count();
            let handles: Vec<Option<FocusHandle>> = rows[head..rows.len() - tail]
                .iter()
                .map(|r| {
                    Some(
                        self.chats
                            .focus
                            .entry(r.id.clone())
                            .or_insert_with(|| cx.focus_handle())
                            .clone(),
                    )
                })
                .collect();
            self.chats
                .list
                .splice_focusable(head..old.len() - tail, handles);
        }
        self.chats.rows = rows;
    }

    /// The section.
    pub(crate) fn chat_list(
        &mut self,
        shown: &filters::Filters,
        narrowed: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let open = self.open_chat();
        let rows = self.build_rows(open.as_deref(), shown, cx);
        self.splice_rows(rows, cx);
        if let Some(index) = open
            .as_ref()
            .and_then(|id| self.chats.rows.iter().position(|r| &r.id == id))
        {
            // The open chat is the list's current row, unless focus is in
            // the list (walking it moves the current row).
            let walking = self.chats.focus.values().any(|h| h.is_focused(window));
            if !walking {
                self.chats.current = index;
            }
        }
        if let Some(index) = self.chats.rows.iter().position(|r| {
            self.chats
                .focus
                .get(&r.id)
                .is_some_and(|h| h.is_focused(window))
        }) {
            self.chats.current = index;
        }
        self.chats.current = self
            .chats
            .current
            .min(self.chats.rows.len().saturating_sub(1));
        let count = self.chats.rows.len();
        let no_rows = self.no_rows("chats-empty", cx);
        let entity = cx.entity();
        let rows = list(self.chats.list.clone(), move |index, window, cx| {
            entity.update(cx, |this, cx| this.chat_row(index, window, cx))
        })
        .size_full();
        div()
            .id("chats")
            // It clips its rows: one scrolled out is not showing (W2).
            .a11y_synthetic_children(|builder| {
                builder.parent_node().set_clips_children();
            })
            // A list, not a list box: AccessKit makes selecting an option a
            // click, so a screen reader walking a list box would open every
            // chat it passes. The open chat is marked current instead, and
            // says `open` in its name (macOS has no attribute for current).
            .role(Role::List)
            .aria_label("Chats")
            // The set's size on the container too: the Windows adapter reads
            // an item's set size from its container.
            .aria_size_of_set(count)
            .key_context("ChatList")
            .on_action(cx.listener(|this, _: &MovePrevious, window, cx| {
                this.move_chat(Step::Previous, window, cx)
            }))
            .on_action(
                cx.listener(|this, _: &MoveNext, window, cx| {
                    this.move_chat(Step::Next, window, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &MoveFirst, window, cx| {
                this.move_chat(Step::First, window, cx)
            }))
            .on_action(
                cx.listener(|this, _: &MoveLast, window, cx| {
                    this.move_chat(Step::Last, window, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &OpenChat, window, cx| {
                // The focused row, however focus came there (a screen
                // reader's Focus action does not move the current one).
                let index = this
                    .chats
                    .rows
                    .iter()
                    .position(|r| {
                        this.chats
                            .focus
                            .get(&r.id)
                            .is_some_and(|handle| handle.is_focused(window))
                    })
                    .unwrap_or(this.chats.current);
                if let Some(id) = this.chats.rows.get(index).map(|r| r.id.clone()) {
                    this.open_row(id, window, cx)
                }
            }))
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .py(px(3.))
            .when(narrowed && count == 0, |d| d.child(no_rows))
            .when(count > 0, |d| d.child(rows))
            .into_any_element()
    }

    /// Row `index` of the list.
    fn chat_row(
        &mut self,
        index: usize,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(row) = self.chats.rows.get(index).cloned() else {
            return div().into_any_element();
        };
        let count = self.chats.rows.len();
        let current = index == self.chats.current;
        let focus = self
            .chats
            .focus
            .entry(row.id.clone())
            .or_insert_with(|| cx.focus_handle())
            .clone()
            .tab_stop(current);
        let archive = self
            .chats
            .archive
            .entry(row.id.clone())
            .or_insert_with(|| cx.focus_handle())
            .clone()
            .tab_stop(current);
        let open = self.open_chat();
        let active = open.as_deref() == Some(row.id.as_str());
        let running = row.dot == Dot::Running;
        let mut name = row_name(
            &row.title,
            active,
            (row.dot != Dot::Idle).then_some(row.dot),
        );
        if let Some((_, title)) = crate::state::resolution::tag(row.resolution) {
            name = format!("{name}, {title}");
        }
        if let Some(snippet) = &row.snippet {
            name = format!("{name}, {}", super::parts::plain(snippet));
        }
        let click_id = row.id.clone();
        let archive_id = row.id.clone();
        let this = cx.entity().downgrade();
        let group = format!("chat-row-{index}");
        let dot = super::dot::dot(
            &row.id,
            row.dot,
            row.arrival,
            7.,
            self.pulses.clone(),
            self.moving,
        );
        div()
            .pb(px(1.))
            .child(
                ring(
                    div()
                        .id(ElementId::Name(format!("chat-{}", row.id).into()))
                        .group(group.clone())
                        .role(Role::ListItem)
                        .aria_label(name)
                        .a11y_synthetic_children(move |builder| {
                            if active {
                                builder
                                    .parent_node()
                                    .set_aria_current(accesskit::AriaCurrent::Page);
                            }
                        })
                        .aria_position_in_set(index)
                        .aria_size_of_set(count)
                        .track_focus(&focus)
                        // chat-fix-2: a reader's ScrollIntoView on the row or
                        // a node in it brings the row into view (and a Click
                        // or Focus on one scrolls it first).
                        .on_a11y_action(accesskit::Action::ScrollIntoView, {
                            let sidebar = cx.entity().downgrade();
                            move |_, _, cx| {
                                let _ = sidebar.update(cx, |this, cx| {
                                    this.chats.list.scroll_to_reveal_item(index);
                                    cx.notify();
                                });
                            }
                        })
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.open_row(click_id.clone(), window, cx)
                        })),
                )
                .min_h(px(34.))
                .flex()
                .items_center()
                .gap(px(8.))
                .px(px(9.))
                .py(px(5.))
                .rounded(px(super::super::theme::radius::ROW))
                .text_size(px(13.))
                .font_weight(FontWeight(430.))
                .text_color(if active {
                    colour::bone()
                } else {
                    colour::ash()
                })
                .when(active, |d| d.bg(colour::raised()))
                .hover(|s| s.bg(colour::hover()).text_color(colour::bone()))
                .cursor_pointer()
                .child(dot)
                .child(row_text(&row.title, row.snippet.as_deref(), cx))
                .children(resolution_tag(row.resolution, cx))
                .child(div().flex_none().my(px(-5.)).mr(px(-7.)).child(row_icon(
                    ("archive-chat", index),
                    &group,
                    &archive,
                    Icon::Archive,
                    "Archive build",
                    "Archive build (it can be restored for 7 days)",
                    running.then_some("Stop the running turn before archiving"),
                    false,
                    Shown::OnHover,
                    Rc::new(move |_, cx| {
                        let id = archive_id.clone();
                        let _ = this.update(cx, |s, cx| s.archive_chat(id, cx));
                    }),
                ))),
            )
            .into_any_element()
    }
}

// chat-fix: where focus goes when the focused element goes (W1).
impl Sidebar {
    /// A Build row or its archive button has focus.
    pub(crate) fn chats_hold_focus(&self, window: &Window) -> bool {
        self.chats
            .focus
            .values()
            .chain(self.chats.archive.values())
            .any(|h| h.is_focused(window))
    }

    /// Focus to the list's current row: the Build list's, or Bench's
    /// current line (the leaderboard row when it has none); the search
    /// field when the list has no row.
    pub fn focus_current(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let handle = match self.mode {
            Mode::Build => self
                .chats
                .rows
                .get(self.chats.current)
                .and_then(|row| self.chats.focus.get(&row.id))
                .cloned(),
            Mode::Bench => self.bench_current_focus(),
        };
        match handle {
            Some(handle) => window.focus(&handle, cx),
            None => {
                let input = self.find.input.clone().expect("made at the first frame");
                let handle = input.read(cx).focus_handle(cx);
                window.focus(&handle, cx);
            }
        }
    }
}
