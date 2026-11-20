//! The archived section (inventory `app.md` 4.1, 4.6, 4.9; `components.md`
//! M; `web/archive.mjs`), below a mode's list and only while that mode has
//! archived items (the list's counts). `Show archived` / `Hide archived`
//! opens it for the mode (`Command::ReadArchive`) or closes it; open, it
//! shows `Loading…` during the first read (rows already read stay on show
//! during later reads), `Archived items failed to load.` with `Try again`
//! when the read failed, else a row per archived item: its title, the time
//! it has left (a bench's model count first), `Restore` and the two-press
//! `Delete permanently` (the first press arms it for 4 s and the row says
//! `Delete for good?`, which the polite region says once; a second press
//! within that deletes it). It is read
//! again after every archive, restore and delete, and closes when the
//! counts say the mode has none left. A restored item opens once the list
//! has it, unless the user went elsewhere meanwhile.

use std::collections::HashMap;
use std::rc::Rc;

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::super::controls::fonts;
use super::super::icons::Icon;
use super::super::theme::{self, TextKind, colour, radius};
use super::super::{Activate, ring};
use super::Sidebar;
use super::parts::{Shown, row_icon, side_empty, side_note, side_title};
use crate::state::archive::{self, Section, archived_rows};
use crate::state::confirm::{ARMED_FOR, Confirm, Press as Pressed};
use crate::state::place::{Mode, Place};
use crate::{Archive, Command, Subject};

/// The row's text while its delete is armed (the page's `Delete for good?`),
/// said once in the polite region when it arms.
const ARMED_TEXT: &str = "Delete for good?";

/// The section's state.
pub struct Archived {
    /// Where it is.
    pub section: Section,
    /// The armed delete.
    confirm: Confirm,
    /// A restored item to open once listed: its mode, id, and the
    /// navigation count when it was restored.
    restored: Option<(Mode, String, u64)>,
    toggle: Option<FocusHandle>,
    retry: Option<FocusHandle>,
    buttons: HashMap<String, FocusHandle>,
    /// The rows' ids as last drawn, in order (where focus goes when one
    /// goes: [`Sidebar::sidebar_focus`]).
    pub(crate) order: Vec<String>,
}

impl Default for Archived {
    fn default() -> Self {
        Archived {
            section: archive::ARCHIVE_CLOSED,
            confirm: Confirm::default(),
            restored: None,
            toggle: None,
            retry: None,
            buttons: HashMap::new(),
            order: Vec::new(),
        }
    }
}

/// Now, epoch ms (the time left is counted against the clock).
pub(crate) fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("after 1970")
        .as_millis() as i64
}

fn subject(mode: Mode, id: &str) -> Subject {
    match mode {
        Mode::Build => Subject::Chat(id.to_string()),
        Mode::Bench => Subject::Bench(id.to_string()),
    }
}

impl Sidebar {
    /// Reads the archive for `mode` (`loadArchive`).
    fn load_archive(&mut self, mode: Mode, cx: &mut Context<Self>) {
        self.archive.section = self.archive.section.reading(mode);
        self.send(Command::ReadArchive);
        cx.notify();
    }

    /// Opens the section for `mode`, or closes it when open for it
    /// (`toggleArchive`).
    fn toggle_archive(&mut self, mode: Mode, cx: &mut Context<Self>) {
        if self.archive.section.mode() != Some(mode) {
            self.log("archived", serde_json::json!({ "open": true }));
            return self.load_archive(mode, cx);
        }
        self.log("archived", serde_json::json!({ "open": false }));
        self.archive.section = self.archive.section.closed();
        cx.notify();
    }

    /// Reads it again when open (`readArchiveAgain`).
    pub(crate) fn read_archive_again(&mut self, cx: &mut Context<Self>) {
        if let Some(mode) = self.archive.section.mode() {
            self.load_archive(mode, cx);
        }
    }

    /// The core's archive: it lands in the open section (a read the core
    /// answered on its own, after a restore, is a read again).
    pub fn archive_read(&mut self, data: Archive, cx: &mut Context<Self>) {
        let section = &self.archive.section;
        self.archive.section = match section {
            Section::Closed => return,
            Section::Loading { .. } => section.read(data),
            _ => section
                .reading(section.mode().expect("an open section has a mode"))
                .read(data),
        };
        cx.notify();
    }

    /// The archive could not be read.
    pub fn archive_failed(&mut self, error: String, cx: &mut Context<Self>) {
        if let Section::Loading { .. } = self.archive.section {
            self.archive.section = self.archive.section.failed(&error);
            cx.notify();
        }
    }

    /// Restores an item (`restoreArchived`), from the section or the
    /// missing screen: it opens once the list has it.
    pub fn restore(&mut self, mode: Mode, id: String, cx: &mut Context<Self>) {
        self.log("restore", serde_json::json!({ "id": id }));
        self.send(Command::Restore(subject(mode, &id)));
        self.read_archive_again(cx);
        self.archive.restored = Some((mode, id, self.navigations));
    }

    /// The list changed: an open section whose mode has none left closes;
    /// a restored item opens once listed (`receiveArchivedCounts`,
    /// `openRestored`).
    pub(crate) fn archive_lists_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (chats, benches) = self.lists.read(cx).published().archived;
        if let Some(mode) = self.archive.section.mode() {
            let left = match mode {
                Mode::Build => chats,
                Mode::Bench => benches,
            };
            if left == 0 {
                self.archive.section = self.archive.section.closed();
            }
        }
        let Some((mode, id, navigations)) = self.archive.restored.clone() else {
            return;
        };
        if navigations != self.navigations {
            self.archive.restored = None;
            return;
        }
        let lists = self.lists.read(cx);
        let listed = match mode {
            Mode::Build => lists.chat(&id).is_some(),
            Mode::Bench => lists.benches().iter().any(|b| b.id == id),
        };
        if !listed {
            return;
        }
        self.archive.restored = None;
        let place = match mode {
            Mode::Build => Place::Chat { id },
            Mode::Bench => Place::Compare { bench: id },
        };
        self.go(place, window, cx);
    }

    fn delete_press(&mut self, mode: Mode, id: String, cx: &mut Context<Self>) {
        let now = cx.background_executor().now();
        match self.archive.confirm.press(&id, now) {
            Pressed::Armed => {
                self.log("delete armed", serde_json::json!({ "id": id }));
                // The armed text is a status: the polite region says it
                // once; disarming after 4 s says nothing.
                if let Some(announcer) = self.announcer.clone() {
                    cx.defer(move |cx| {
                        announcer.update(cx, |a, cx| a.polite(ARMED_TEXT.to_string(), cx))
                    });
                }
                cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(ARMED_FOR).await;
                    let _ = this.update(cx, |_, cx| cx.notify());
                })
                .detach();
            }
            Pressed::Confirmed => {
                self.log("delete", serde_json::json!({ "id": id }));
                self.send(Command::Delete(subject(mode, &id)));
                self.read_archive_again(cx);
            }
        }
        cx.notify();
    }

    /// The section for `mode`, when it has archived items.
    pub(crate) fn archived_section(
        &mut self,
        mode: Mode,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        // The rows drawn this frame (none unless the section draws them).
        self.archive.order.clear();
        let (chats, benches) = self.lists.read(cx).published().archived;
        let count = match mode {
            Mode::Build => chats,
            Mode::Bench => benches,
        };
        if count == 0 {
            return None;
        }
        let fonts = fonts(cx);
        let open = self.archive.section.mode() == Some(mode);
        let toggle = self
            .archive
            .toggle
            .get_or_insert_with(|| cx.focus_handle().tab_stop(true))
            .clone();
        let this = cx.entity().downgrade();
        let title = side_title(
            "archived-toggle",
            &toggle,
            if open {
                "Hide archived"
            } else {
                "Show archived"
            },
            open,
            Rc::new(move |_, cx| {
                let _ = this.update(cx, |s, cx| s.toggle_archive(mode, cx));
            }),
            cx,
        );
        let mut section = div()
            .flex_none()
            .flex()
            .flex_col()
            .pt(px(4.))
            .px(px(4.))
            .child(div().flex().child(title));
        if !open {
            return Some(section.into_any_element());
        }
        let rows = archived_rows(&self.archive.section);
        let mut list = div()
            .id("archived-list")
            // It clips its rows: one scrolled out is not showing (W2).
            .a11y_synthetic_children(|builder| {
                builder.parent_node().set_clips_children();
            })
            .role(Role::List)
            .aria_label("Archived")
            .aria_size_of_set(rows.as_ref().map_or(0, Vec::len))
            .flex()
            .flex_col()
            .gap(px(2.))
            .max_h(px(320.))
            .overflow_y_scroll()
            .pt(px(2.))
            .pb(px(4.));
        if matches!(self.archive.section, Section::Loading { .. }) && rows.is_none() {
            list = list.child(side_note("archived-loading", "Loading\u{2026}", cx));
        }
        if let Section::Failed { .. } = &self.archive.section {
            let retry = self
                .archive
                .retry
                .get_or_insert_with(|| cx.focus_handle().tab_stop(true))
                .clone();
            let again = cx.listener(move |this, _, _, cx| this.load_archive(mode, cx));
            let again_key =
                cx.listener(move |this, _: &Activate, _, cx| this.load_archive(mode, cx));
            list = list.child(
                side_empty("Archived items failed to load. ", cx)
                    .id("archived-failed")
                    .role(Role::Alert)
                    .aria_label("Archived items failed to load.")
                    .flex()
                    .flex_wrap()
                    .child(
                        ring(
                            div()
                                .id("archived-retry")
                                .role(Role::Button)
                                .aria_label("Try again")
                                .track_focus(&retry)
                                .key_context("Part")
                                .on_click(again)
                                .on_action(again_key),
                        )
                        .text_color(colour::ash())
                        .underline()
                        .hover(|s| s.text_color(colour::bone()))
                        .cursor_pointer()
                        .child("Try again"),
                    ),
            );
        }
        let total = rows.as_ref().map_or(0, Vec::len);
        let now = self.now.unwrap_or_else(now_ms);
        let instant = cx.background_executor().now();
        self.archive.order = rows
            .as_ref()
            .map(|rows| rows.iter().map(|r| r.id.clone()).collect())
            .unwrap_or_default();
        for (index, row) in rows.unwrap_or_default().into_iter().enumerate() {
            let armed = self.archive.confirm.armed(&row.id, instant);
            let restore_focus = self
                .archive
                .buttons
                .entry(format!("restore:{}", row.id))
                .or_insert_with(|| cx.focus_handle().tab_stop(true))
                .clone();
            let delete_focus = self
                .archive
                .buttons
                .entry(format!("delete:{}", row.id))
                .or_insert_with(|| cx.focus_handle().tab_stop(true))
                .clone();
            let meta = archive::meta(&row, now);
            let group = format!("archived-row-{index}");
            let restore_this = cx.entity().downgrade();
            let delete_this = cx.entity().downgrade();
            let restore_id = row.id.clone();
            let delete_id = row.id.clone();
            let shown_meta = if armed {
                ARMED_TEXT.to_string()
            } else {
                meta.clone()
            };
            list = list.child(
                theme::text(
                    div()
                        .id(("archived-row", index))
                        .group(group.clone())
                        .role(Role::ListItem)
                        .aria_label(format!("{}, {shown_meta}", row.title))
                        .aria_position_in_set(index)
                        .aria_size_of_set(total),
                    TextKind::ArchivedRow,
                    &fonts,
                )
                .min_h(px(34.))
                .flex()
                .items_center()
                .gap(px(2.))
                .pl(px(9.))
                .pr(px(4.))
                .py(px(5.))
                .rounded(px(radius::ROW))
                .hover(|s| s.bg(colour::hover()).text_color(colour::bone()))
                // par-shell (`app.md` 4.6): the full title on hover.
                .child(crate::view::controls::pointer_tip(
                    div()
                        .id(("archived-title", index))
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .child(row.title.clone()),
                    ("archived-title-tip", index),
                    row.title.clone(),
                ))
                .child(
                    theme::text(
                        div().id(("archived-meta", index)),
                        TextKind::ArchivedMeta,
                        &fonts,
                    )
                    .mx(px(4.))
                    .ml(px(8.))
                    .flex_none()
                    .when(armed, |d| d.text_color(colour::err()))
                    .child(shown_meta),
                )
                .child(row_icon(
                    ("archived-restore", index),
                    &group,
                    &restore_focus,
                    Icon::Restart,
                    "Restore",
                    "Restore to the list and open it",
                    None,
                    false,
                    Shown::Always,
                    Rc::new(move |_, cx| {
                        let id = restore_id.clone();
                        let _ = restore_this.update(cx, |s, cx| s.restore(mode, id, cx));
                    }),
                ))
                .child(row_icon(
                    ("archived-delete", index),
                    &group,
                    &delete_focus,
                    Icon::Trash,
                    if armed {
                        "Delete for good"
                    } else {
                        "Delete permanently"
                    },
                    if armed {
                        "Click again to delete it for good"
                    } else {
                        "Delete permanently: this cannot be undone"
                    },
                    None,
                    armed,
                    Shown::Always,
                    Rc::new(move |_, cx| {
                        let id = delete_id.clone();
                        let _ = delete_this.update(cx, |s, cx| s.delete_press(mode, id, cx));
                    }),
                )),
            );
        }
        section = section.child(list);
        Some(section.into_any_element())
    }
}

// chat-fix: where focus goes when the focused element goes (W1).
/// Where the sidebar held focus in the last frame drawn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SidebarFocus {
    /// A Build list row or its archive button.
    ChatRows,
    /// A Bench list line or one of its controls.
    BenchRows,
    /// An archived row's button: its position in the rows as drawn.
    Archived(usize),
}

impl Sidebar {
    /// Where focus is in the sidebar's lists now, if it is there.
    pub fn sidebar_focus(&self, window: &Window) -> Option<SidebarFocus> {
        let focused = |h: &FocusHandle| h.is_focused(window);
        if self.chats_hold_focus(window) {
            return Some(SidebarFocus::ChatRows);
        }
        if self.benches_hold_focus(window) {
            return Some(SidebarFocus::BenchRows);
        }
        if self.archive.toggle.as_ref().is_some_and(focused) {
            return Some(SidebarFocus::Archived(0));
        }
        self.archive
            .buttons
            .iter()
            .find(|(_, h)| focused(h))
            .and_then(|(key, _)| {
                let id = key.split_once(':').map(|(_, id)| id).unwrap_or(key);
                self.archive.order.iter().position(|o| o == id)
            })
            .map(SidebarFocus::Archived)
    }

    /// The focused element of `held` went: focus goes to its neighbour.
    /// An archived row's: the row now at its place, else the one before,
    /// else the section's toggle, else the list's current row. A list row's:
    /// the list's current row (the next one, else the one before), else the
    /// search field.
    pub fn restore_focus(
        &mut self,
        held: SidebarFocus,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let SidebarFocus::Archived(at) = held {
            let (chats, benches) = self.lists.read(cx).published().archived;
            let left = match self.mode {
                Mode::Build => chats,
                Mode::Bench => benches,
            };
            let order = &self.archive.order;
            if left > 0
                && self.archive.section.mode().is_some()
                && !order.is_empty()
                && let Some(handle) = self
                    .archive
                    .buttons
                    .get(&format!("restore:{}", order[at.min(order.len() - 1)]))
            {
                return window.focus(&handle.clone(), cx);
            }
            if left > 0
                && let Some(toggle) = self.archive.toggle.clone()
            {
                return window.focus(&toggle, cx);
            }
        }
        self.focus_current(window, cx);
    }
}
