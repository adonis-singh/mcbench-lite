//! The transcript: a virtual list of rows (`state::rows`) drawn with the
//! markdown of [`super::super::markdown`], its keys (Up and Down by
//! message, Right and Left by block, Home, End, Page Up, Page Down), its
//! selection and copying, folds, links, images and the error row's
//! dismiss. A new kind of transcript item is a variant in
//! `state::transcript`, its row in `state::rows`, and its drawing here in
//! `render_row`.

use std::rc::Rc;

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::super::markdown::{self, RowEnv};
use super::super::{
    Activate, CopySelection, MoveFirst, MoveLast, MoveNext, MovePageDown, MovePageUp, MovePrevious,
    NextBlock, PreviousBlock, SelectAll, colour, ring,
};
use super::{ChatPane, now_ms};
use crate::state::rows::RowKind;
use crate::state::selection::{self, Point as TextPoint, Selection};
use crate::state::transcript::{self, Item};
use crate::state::{Step, step};

impl ChatPane {
    /// Removes the error shown in row `key` (`Dismiss error`).
    /// Focus goes to the composer, so something has focus and the reader
    /// does not re-read the window.
    pub(crate) fn dismiss(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(index) = self.rows.iter().position(|r| r.key == key) else {
            return;
        };
        let entries = transcript::group_items(&self.items, self.busy(cx));
        let transcript::Entry::Item(Item::Error { text, message_id }) = &entries[index] else {
            panic!("row {key} is not an error");
        };
        let position = self
            .items
            .iter()
            .position(|i| matches!(i, Item::Error { text: t, message_id: m } if t == text && m == message_id))
            .expect("the error is an item");
        self.log("dismissed error", serde_json::json!({ "text": text }));
        // A stored error is dismissed in the store too, or it comes back
        // with the next read (`POST .../dismiss-error`).
        if let (Some(chat), Some(message)) = (self.open.clone(), message_id.clone()) {
            self.options.core.send(crate::Command::DismissError {
                chat,
                message: crate::MessageId(message),
            });
        }
        self.items.remove(position);
        self.rebuild(cx);
        self.composer.update(cx, |c, cx| c.focus(window, cx));
        cx.notify();
    }

    pub(crate) fn toggle(&mut self, key: String, cx: &mut Context<Self>) {
        let open = !self.open_folds.remove(&key);
        if open {
            self.open_folds.insert(key.clone());
        }
        self.log("toggled", serde_json::json!({ "key": key, "open": open }));
        self.rebuild(cx);
        cx.notify();
    }

    /// Moves focus within the current message by top-level block: Right to
    /// the next (from the message itself, its first), Left to the previous
    /// (from the first, the message).
    pub(crate) fn move_block(
        &mut self,
        forward: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self.current_row else {
            return;
        };
        let key = self.rows[index].key.clone();
        let count = markdown::block_count(&self.rows[index].pieces);
        let at = (0..count).find(|&i| {
            self.parts
                .get(&markdown::block_focus_name(&key, i))
                .is_some_and(|handle| handle.is_focused(window))
        });
        let target = match (at, forward) {
            (None, true) if count > 0 => Some(0),
            (Some(i), true) if i + 1 < count => Some(i + 1),
            (Some(0), false) => None,
            (Some(i), false) => Some(i - 1),
            _ => return,
        };
        let handle = match target {
            Some(i) => self.parts[&markdown::block_focus_name(&key, i)].clone(),
            None => self.row_focus[&key].clone(),
        };
        window.focus(&handle, cx);
        cx.notify();
    }

    pub(crate) fn move_row(&mut self, to: Step, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(index) = step(self.current_row, self.rows.len(), to) {
            self.reveal_row(index, window, cx);
        }
    }

    /// Makes row `index` the current message: scrolled into view, focused.
    pub fn reveal_row(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.current_row = Some(index);
        self.list.scroll_to_reveal_item(index);
        if let Some(handle) = self.row_focus.get(&self.rows[index].key) {
            window.focus(handle, cx);
        }
        cx.notify();
    }

    pub(crate) fn select_all(&mut self, cx: &mut Context<Self>) {
        let Some(last_row) = self.leaves.iter().rposition(|l| !l.is_empty()) else {
            return;
        };
        let last_leaf = self.leaves[last_row].len() - 1;
        self.selection = Some(Selection {
            anchor: TextPoint {
                row: 0,
                leaf: 0,
                offset: 0,
            },
            focus: TextPoint {
                row: last_row,
                leaf: last_leaf,
                offset: self.leaves[last_row][last_leaf].text.len(),
            },
        });
        self.log("selected all", serde_json::json!({}));
        cx.notify();
    }

    pub(crate) fn copy(&mut self, cx: &mut Context<Self>) {
        let Some(selection) = self.selection.filter(|s| !s.is_empty()) else {
            return;
        };
        let text = selection::copied_text(&selection, |row| self.leaves[row].as_slice());
        self.log("copied", serde_json::json!({ "text": text }));
        cx.write_to_clipboard(ClipboardItem::new_string(text));
    }

    pub(crate) fn pointer_down(&mut self, event: &MouseDownEvent, cx: &mut Context<Self>) {
        let Some(point) = self.hits.borrow().point_at(event.position) else {
            return;
        };
        let text = &self.leaves[point.row][point.leaf].text;
        self.selection = Some(match event.click_count {
            1 => Selection::caret(point),
            2 => {
                let word = selection::word_at(text, point.offset);
                Selection {
                    anchor: TextPoint {
                        offset: word.start,
                        ..point
                    },
                    focus: TextPoint {
                        offset: word.end,
                        ..point
                    },
                }
            }
            _ => Selection {
                anchor: TextPoint { offset: 0, ..point },
                focus: TextPoint {
                    offset: text.len(),
                    ..point
                },
            },
        });
        self.dragging = event.click_count == 1;
        cx.notify();
    }

    pub(crate) fn pointer_move(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        if !self.dragging || event.pressed_button != Some(MouseButton::Left) {
            return;
        }
        if let (Some(point), Some(selection)) = (
            self.hits.borrow().point_at(event.position),
            self.selection.as_mut(),
        ) && selection.focus != point
        {
            selection.focus = point;
            cx.notify();
        }
    }

    /// One row of the transcript.
    pub(crate) fn render_row(
        &mut self,
        index: usize,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.rows_drawn += 1;
        if index == self.rows.len() {
            return self.working_row(cx);
        }
        let row = self.rows[index].clone();
        let count = self.rows.len();
        let current = self.current_row == Some(index);
        let focus = self.row_focus[&row.key].clone().tab_stop(current);
        let screen = cx.entity().downgrade();
        let on_link: crate::view::Handler = {
            let screen = screen.clone();
            Rc::new(move |url, _, cx| {
                let _ = screen.update(cx, |this, cx| {
                    this.log("link activated", serde_json::json!({ "url": url }));
                    (this.options.opener)(&url, cx);
                });
            })
        };
        let on_copy: crate::view::Handler = {
            let screen = screen.clone();
            Rc::new(move |text, _, cx| {
                let _ = screen.update(cx, |this, cx| {
                    this.log("copied", serde_json::json!({ "text": text }));
                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                });
            })
        };
        let on_image: crate::view::Handler = {
            let screen = screen.clone();
            Rc::new(move |url, window, cx| {
                let _ = screen.update(cx, |this, cx| {
                    this.log("image activated", serde_json::json!({ "url": url }));
                    let file = crate::state::files::picture(
                        std::path::Path::new(&url),
                        &*this.options.core,
                    );
                    if let Ok(file) = file {
                        this.open_lightbox(super::Picture::File(file), window, cx);
                    }
                });
            })
        };
        let on_toggle: crate::view::Handler = {
            let screen = screen.clone();
            Rc::new(move |key, _, cx| {
                let _ = screen.update(cx, |this, cx| this.toggle(key, cx));
            })
        };
        let mut env = RowEnv {
            row: index,
            key: row.key.clone(),
            leaf: 0,
            selection: self.selection,
            hits: self.hits.clone(),
            fonts: self.fonts.clone(),
            current,
            focus: &mut self.parts,
            parts: 0,
            blocks: 0,
            // A streaming row's last block is the one being written.
            unsettled_from: row
                .live
                .then(|| markdown::block_count(&row.pieces).saturating_sub(1)),
            on_link,
            on_copy,
            on_image,
            pictures: {
                let core = self.options.core.clone();
                Rc::new(move |path| crate::state::files::picture(path, &*core))
            },
            on_toggle,
            code_scrolls: &mut self.code_scrolls,
        };
        let register = markdown::prose();
        let content = markdown::pieces(&mut env, &row.pieces, register, cx);
        env.finish(self.leaves[index].len());
        assert_eq!(
            env.blocks,
            markdown::block_count(&row.pieces),
            "row {index} drew a block count its pieces do not have"
        );
        drop(env);
        let parts = if row.kind == RowKind::User {
            Some(self.user_parts(&row, current, cx))
        } else {
            None
        };
        // The row's name is who wrote it and then how it reads
        // (`markdown::spoken_pieces`): every screen reader speaks a focused
        // node's name at once, not all of them its description.
        let sender = match row.kind {
            RowKind::User => "You".to_string(),
            RowKind::Assistant => self.model(cx),
            RowKind::Work => format!("{}, work", self.model(cx)),
            RowKind::Error => "Error".to_string(),
            RowKind::Notice => "Notice".to_string(),
        };
        // A streaming message is named by its writer alone until it
        // settles (open item b): its name changes once, at the end, and a
        // reader on it then hears the whole answer once.
        let text = if row.live {
            String::new()
        } else {
            markdown::spoken_pieces(&row.pieces)
        };
        let name = if text.is_empty() {
            sender
        } else {
            format!("{sender}, {text}")
        };
        let body = match row.kind {
            RowKind::User => {
                let (files, selections, tools) = parts.expect("a user row's parts");
                div()
                    .flex()
                    .flex_col()
                    .items_end()
                    .gap(px(3.))
                    .child(
                        div()
                            .max_w(relative(0.92))
                            .px(px(13.))
                            .py(px(9.))
                            .rounded(px(crate::view::theme::radius::USER))
                            .border_1()
                            .border_color(if row.editing {
                                colour::bone_28()
                            } else {
                                colour::line()
                            })
                            .bg(colour::raised())
                            .children(content)
                            .children(files)
                            .children(selections),
                    )
                    .child(tools)
            }
            RowKind::Error => {
                // `.error-dismiss`: dismisses the error (today's page also
                // marks a stored one dismissed on the server; that command
                // joins the seam with the stage-three session).
                let dismiss = self
                    .parts
                    .entry(format!("{}/dismiss", row.key))
                    .or_insert_with(|| cx.focus_handle())
                    .clone()
                    .tab_stop(current);
                let key = row.key.clone();
                let key_for_keys = row.key.clone();
                let dismissable = row.dismissable;
                div().child(
                    div()
                        .flex()
                        .items_start()
                        .justify_between()
                        .gap(px(10.))
                        .px(px(12.))
                        .py(px(10.))
                        .rounded(px(9.))
                        .border_1()
                        .border_color(colour::error_border())
                        .bg(colour::error_fill())
                        .text_size(px(13.))
                        .text_color(colour::err())
                        .child(div().flex_1().min_w_0().children(content))
                        .when(dismissable, |d| {
                            d.child(
                                ring(
                                    div()
                                        .id("dismiss")
                                        .role(Role::Button)
                                        .aria_label("Dismiss error")
                                        .track_focus(&dismiss)
                                        .key_context("Part")
                                        .on_action(cx.listener(
                                            move |this, _: &Activate, window, cx| {
                                                this.dismiss(&key_for_keys, window, cx)
                                            },
                                        ))
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            this.dismiss(&key, window, cx)
                                        })),
                                )
                                .size(px(22.))
                                .flex_none()
                                .rounded(px(6.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .cursor_pointer()
                                .child("\u{00d7}"),
                            )
                        }),
                )
            }
            // `.activity > summary { width: fit-content }`: folds and rows
            // take their content's width.
            RowKind::Work | RowKind::Notice => div()
                .flex()
                .flex_col()
                .items_start()
                .gap(px(1.))
                .children(content),
            RowKind::Assistant => div().max_w(relative(0.94)).children(content),
        };
        let screen_focus = screen.clone();
        let screen_scroll = screen.clone();
        ring(
            div()
                // Keyed by the row's key, not its index, so a node keeps its
                // identity while rows around it change.
                .id(ElementId::Name(row.key.clone().into()))
                .role(Role::Article)
                .aria_label(name)
                .aria_position_in_set(index)
                .aria_size_of_set(count)
                .track_focus(&focus)
                // chat-fix-2: a reader's ScrollIntoView on the row or any
                // node in it brings the row into view; a Click or a Focus on
                // one scrolls it first (vendor gpui-pre, `Window::
                // handle_a11y_action`).
                .on_a11y_action(accesskit::Action::ScrollIntoView, move |_, _, cx| {
                    let _ = screen_scroll.update(cx, |this, cx| {
                        this.list.scroll_to_reveal_item(index);
                        cx.notify();
                    });
                })
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    let _ = screen_focus.update(cx, |this, cx| {
                        this.current_row = Some(index);
                        cx.notify();
                    });
                }),
        )
        .group(SharedString::from(format!("row-{}", row.key)))
        .rounded(px(6.))
        // narrow agent: `.messages` padding `22px 20px 16px`, at 980 px and
        // below `20px 16px 14px` (less the ring's 2 px).
        .px(px(if self.narrow { 14. } else { 18. }))
        .pb(px(16.))
        .text_size(register.size)
        .line_height(register.line)
        .text_color(register.colour)
        .child(body)
        .into_any_element()
    }

    pub(crate) fn transcript(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let entity = cx.entity();
        let rows = list(self.list.clone(), move |index, window, cx| {
            entity.update(cx, |this, cx| this.render_row(index, window, cx))
        })
        .size_full();
        let busy = self.busy(cx);
        let empty = self.rows.is_empty();
        // settings agent: while setup is not ready, the setup screen in the
        // empty hint's place.
        // s4-needs: also while a step needs the person, ready or not.
        let hint = (empty && !busy && self.read == crate::state::transcript::ReadState::Shown)
            .then(|| match self.screen_steps() {
                // s4-needs: a ready app keeps its empty chat, with only the
                // steps that need the person.
                Some(steps) if crate::state::setup::setup_status(&steps).ready => {
                    let card = self.needs_card(&steps, window, cx);
                    self.empty_hint(Some(card), window, cx)
                }
                Some(steps) => self.setup_screen(&steps, window, cx),
                None => self.empty_hint(None, window, cx),
            });
        let failed = self.read_failed(window, cx);
        let list_state = self.list.clone();
        let list_state_up = self.list.clone();
        let region =
            div()
                .id("transcript")
                .role(Role::Region)
                .aria_label("Conversation")
                .aria_size_of_set(self.rows.len())
                .key_context("Transcript")
                .track_focus(&self.transcript_focus.clone().tab_stop(empty))
                .a11y_synthetic_children(move |builder| {
                    if busy {
                        builder.parent_node().set_busy();
                    }
                    // It clips its rows: one scrolled out (kept for a reader's
                    // next move) is not showing and takes no pointer (W2).
                    builder.parent_node().set_clips_children();
                })
                .on_a11y_action(accesskit::Action::ScrollDown, move |_, _, _| {
                    let page = list_state.viewport_bounds().size.height;
                    list_state.scroll_by(page * 0.8);
                })
                .on_a11y_action(accesskit::Action::ScrollUp, move |_, _, _| {
                    let page = list_state_up.viewport_bounds().size.height;
                    list_state_up.scroll_by(-page * 0.8);
                })
                .on_action(
                    cx.listener(|this, _: &NextBlock, window, cx| {
                        this.move_block(true, window, cx)
                    }),
                )
                .on_action(cx.listener(|this, _: &PreviousBlock, window, cx| {
                    this.move_block(false, window, cx)
                }))
                .on_action(cx.listener(|this, _: &MovePrevious, window, cx| {
                    this.move_row(Step::Previous, window, cx)
                }))
                .on_action(cx.listener(|this, _: &MoveNext, window, cx| {
                    this.move_row(Step::Next, window, cx)
                }))
                .on_action(cx.listener(|this, _: &MoveFirst, window, cx| {
                    this.move_row(Step::First, window, cx)
                }))
                .on_action(cx.listener(|this, _: &MoveLast, window, cx| {
                    this.move_row(Step::Last, window, cx)
                }))
                .on_action(cx.listener(|this, _: &MovePageUp, window, cx| {
                    this.move_row(Step::PageUp(5), window, cx)
                }))
                .on_action(cx.listener(|this, _: &MovePageDown, window, cx| {
                    this.move_row(Step::PageDown(5), window, cx)
                }))
                .on_action(cx.listener(|this, _: &SelectAll, _, cx| this.select_all(cx)))
                .on_action(cx.listener(|this, _: &CopySelection, _, cx| this.copy(cx)))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, event: &MouseDownEvent, _, cx| this.pointer_down(event, cx)),
                )
                .on_mouse_move(
                    cx.listener(|this, event: &MouseMoveEvent, _, cx| this.pointer_move(event, cx)),
                )
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, _: &MouseUpEvent, _, _| {
                        this.dragging = false;
                        if let Some(selection) = this.selection.filter(|s| !s.is_empty()) {
                            let (start, end) = selection.ordered();
                            this.log(
                                "selected",
                                serde_json::json!({
                                    "from": [start.row, start.leaf, start.offset],
                                    "to": [end.row, end.leaf, end.offset],
                                }),
                            );
                        }
                    }),
                )
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .map(|d| match hint {
                    // The empty chat: the hint takes the list's place.
                    Some(hint) => d.child(hint),
                    None => d.child(div().flex_1().min_h_0().child(rows)),
                })
                .children(failed);
        // chat-fix-2: the 22 px above the list are outside the region, so the
        // region's bounds, which clip its rows for assistive technology, are
        // the list's visible part (a row in that band was drawn clipped but
        // reported showing).
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .pt(px(if self.narrow { 20. } else { 22. }))
            .child(region)
            .into_any_element()
    }
}

// chat (stage four's chat agent): the working line, the empty chat, the
// read that failed.
impl ChatPane {
    /// The working line (`.pending` with `Ticker`): three dots and `Working
    /// for <elapsed>`, or the harness's status with the elapsed time; the
    /// list's item after the rows while a turn runs. The dots step once a
    /// second with the clock (the page's 2 s, six-step pulse would redraw
    /// every frame).
    pub(crate) fn working_row(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let text = self.working_line(cx).unwrap_or_default();
        let fonts = crate::view::controls::fonts(cx);
        let second = (now_ms() / 1000) as usize;
        let mut dots = div().flex().items_center().gap(px(3.));
        for n in 0..3 {
            dots = dots.child(
                div()
                    .size(px(4.))
                    .rounded_full()
                    .bg(colour::dim_70())
                    // At rest (`view::motion`) each at its mid opacity.
                    .opacity(if !self.moving {
                        0.75
                    } else if (second + 3 - n).is_multiple_of(3) {
                        1.
                    } else {
                        0.5
                    }),
            );
        }
        crate::view::theme::text(
            div()
                .id("working")
                .role(Role::Group)
                .aria_label(text.clone())
                .px(px(18.))
                .pb(px(16.))
                .min_h(px(24.))
                .flex()
                .items_center()
                .gap(px(8.)),
            crate::view::theme::TextKind::Ticker,
            &fonts,
        )
        .child(div().w(px(20.)).flex().justify_center().child(dots))
        .child(text)
        .into_any_element()
    }

    /// The empty chat (`.empty-hint`): the grass block at 56 px, `What
    /// should we build?`, and the connect hint while no AI is connected,
    /// else `Or have several models build it in` with its `Bench` link.
    /// With a step needing the person (s4-needs), `card` holds it between
    /// the heading and the last line.
    pub(crate) fn empty_hint(
        &mut self,
        card: Option<AnyElement>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let fonts = crate::view::controls::fonts(cx);
        let sub = self.empty_sub(window, cx);
        let wide = card.is_some();
        div()
            .flex_1()
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .when(wide, |d| d.w(px(460.)).max_w_full())
                    .when(!wide, |d| d.max_w(px(340.)))
                    .p(px(30.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(13.))
                    .child(crate::view::icons::app_mark(56.))
                    .child(
                        crate::view::theme::text(
                            div()
                                .id("empty-heading")
                                .role(Role::Heading)
                                .aria_label("What should we build?"),
                            crate::view::theme::TextKind::EmptyHeading,
                            &fonts,
                        )
                        .child("What should we build?"),
                    )
                    .children(card)
                    .child(sub),
            )
            .into_any_element()
    }

    /// The empty chat's last line: the connect link while no AI is
    /// connected, else `Or have several models build it in` and `Bench`.
    pub(crate) fn empty_sub(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let fonts = crate::view::controls::fonts(cx);
        let mut link = |id: &'static str, label: &'static str, run: crate::view::Press| {
            super::composer::text_link(id, label, move |window, cx| run(window, cx), window, cx)
        };
        let sub = if self.connected == Some(false) {
            div().child(link(
                "connect",
                "Connect an AI to start building",
                self.options.connect.clone(),
            ))
        } else {
            crate::view::theme::text(
                div().flex().items_center().gap(px(4.)),
                crate::view::theme::TextKind::EmptySub,
                &fonts,
            )
            .child("Or have several models build it in")
            .child(link("to-bench", "Bench", self.options.to_bench.clone()))
        };
        sub.into_any_element()
    }

    /// Reads the open chat again (`readAgain`): `Read again`, and a press on
    /// the row of the chat on screen after its read failed.
    pub(crate) fn read_again(&mut self, cx: &mut Context<Self>) {
        if let Some(chat) = self.open.clone() {
            self.read = crate::state::transcript::ReadState::Loading;
            self.log("read again", serde_json::json!({ "chat": chat }));
            self.options.core.send(crate::Command::Open { chat });
            cx.notify();
        }
    }

    // par-chat (stage four's parity pass)
    /// The chat on screen chosen again (`openConversation` with its own
    /// id): its read is tried again if it failed, else nothing.
    pub fn chosen_again(&mut self, cx: &mut Context<Self>) {
        if matches!(self.read, crate::state::transcript::ReadState::Failed(_)) {
            self.read_again(cx);
        }
    }

    /// The read that failed (`.msg.error`, `role="alert"`): `This chat could
    /// not be read: <error>` and `Read again`.
    pub(crate) fn read_failed(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let crate::state::transcript::ReadState::Failed(error) = &self.read else {
            return None;
        };
        let text = format!("This chat could not be read: {error}");
        let fonts = crate::view::controls::fonts(cx);
        let this = cx.entity().downgrade();
        Some(
            crate::view::theme::text(
                div()
                    .id("read-failed")
                    .role(Role::Alert)
                    .aria_label(text.clone())
                    .mx(px(18.))
                    .mb(px(16.))
                    .px(px(12.))
                    .py(px(10.))
                    .rounded(px(crate::view::theme::radius::ERROR))
                    .border_1()
                    .border_color(colour::error_border())
                    .bg(colour::error_fill())
                    .flex()
                    .items_start()
                    .justify_between()
                    .gap(px(10.)),
                crate::view::theme::TextKind::Error,
                &fonts,
            )
            .child(div().flex_1().min_w_0().child(text))
            .child(super::composer::text_link(
                "read-again",
                "Read again",
                move |_, cx| {
                    let _ = this.update(cx, |this, cx| this.read_again(cx));
                },
                window,
                cx,
            ))
            .into_any_element(),
        )
    }
}

// settings agent: the setup screen in the empty chat.
impl ChatPane {
    /// The setup steps the empty Build chat shows: while setup is not
    /// ready, or while a step needs the person (s4-needs).
    pub(crate) fn screen_steps(&self) -> Option<Vec<mcbench_setup::Step>> {
        let steps = self.setup.as_ref()?;
        (!crate::state::setup::setup_status(steps).ready || self.needs_line().is_some())
            .then(|| steps.clone())
    }

    /// The step needing the person, or the one whose work they started
    /// (s4-needs).
    pub(crate) fn needs_line(&self) -> Option<crate::state::setup::NeedsLine> {
        crate::state::setup::needs_line(self.setup.as_ref()?, self.needs_step.as_deref())
    }

    /// The transcript shows the steps (the empty Build chat), so the line
    /// over the composer would say the row again.
    pub(crate) fn shows_setup_screen(&self, cx: &App) -> bool {
        self.bench.surface == super::bench::Surface::Chat
            && self.rows.is_empty()
            && !self.busy(cx)
            && self.read == crate::state::transcript::ReadState::Shown
            && self.screen_steps().is_some()
    }

    /// `Install WSL` (`Command::InstallWsl`), logged.
    pub(crate) fn install_wsl_press(&self) -> crate::view::Press {
        let (core, log) = (self.options.core.clone(), self.options.log.clone());
        std::rc::Rc::new(move |_, _| {
            if let Some(log) = &log {
                log.record("install wsl", serde_json::json!({}));
            }
            core.send(crate::Command::InstallWsl)
        })
    }

    /// `Restart now` (`Command::RestartComputer`), logged.
    pub(crate) fn restart_press(&self) -> crate::view::Press {
        let (core, log) = (self.options.core.clone(), self.options.log.clone());
        std::rc::Rc::new(move |_, _| {
            if let Some(log) = &log {
                log.record("restart computer", serde_json::json!({}));
            }
            core.send(crate::Command::RestartComputer)
        })
    }

    fn setup_screen(
        &mut self,
        steps: &[mcbench_setup::Step],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        crate::view::setup::setup_screen(
            steps,
            self.connected == Some(false),
            self.setup_actions(),
            window,
            cx,
        )
    }

    /// The card of a ready app's empty chat (s4-needs): the steps that need
    /// the person, or whose work they started and that still runs.
    fn needs_card(
        &mut self,
        steps: &[mcbench_setup::Step],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let shown = crate::state::setup::needs_steps(steps, self.needs_step.as_deref());
        crate::view::setup::steps_card(&shown, &self.setup_actions(), window, cx)
            .w_full()
            .into_any_element()
    }

    fn setup_actions(&self) -> crate::view::setup::SetupActions {
        let core = self.options.core.clone();
        let log = self.options.log.clone();
        let retry_log = log.clone();
        crate::view::setup::SetupActions {
            retry: std::rc::Rc::new(move |_, _| {
                if let Some(log) = &retry_log {
                    log.record("retry setup", serde_json::json!({}));
                }
                core.send(crate::Command::RetrySetup);
            }),
            connect: self.options.connect.clone(),
            opener: self.options.opener.clone(),
            log,
            install_wsl: self.install_wsl_press(),
            restart: self.restart_press(),
        }
    }
}
