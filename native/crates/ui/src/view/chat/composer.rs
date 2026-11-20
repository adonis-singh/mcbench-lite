//! The composer (`web/app.js` 5399-5556, 6895-6999, inventory 6): the held
//! send's line, the editing line, then the box (`.composer-box`) with the
//! selections and attachments ([`super::attachments`]), the growing text
//! box, and the footer: `Attach file`, the pickers ([`super::controls`]),
//! and Send or Stop. Enter sends, Shift+Enter breaks the line. A send is
//! held while setup is unfinished; it does nothing while a turn runs or
//! files are read; a new chat without a model says so in a notice. Typing
//! anywhere outside a field goes into the composer ([`type_ahead`]).

use gpui_kit::component::input::Textarea;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::super::icons::{self, Icon};
use super::super::{Activate, colour, ring};
use super::{ChatPane, Pending};
use crate::state::draft;
use crate::state::transcript::{FileSource, Item, ShownFile};
use crate::state::turn::{self, Turn};
use crate::{Command, Outgoing, Target};

/// The composer's placeholder in a chat with no message yet, before an idea
/// is picked (the first of `IDEAS`).
pub const PLACEHOLDER: &str = "Add a cat";

/// `.text-link`: a button that reads as a link, `--ash`, underlined.
pub(crate) fn text_link(
    id: &'static str,
    label: &'static str,
    press: impl Fn(&mut Window, &mut App) + 'static,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let press = std::rc::Rc::new(press);
    let on_key = press.clone();
    let focus = super::super::controls::focus_for(id, window, cx).tab_stop(true);
    ring(
        div()
            .id(id)
            .role(Role::Button)
            .aria_label(label)
            .key_context("Part")
            .track_focus(&focus)
            .on_action(move |_: &Activate, window, cx| on_key(window, cx))
            .on_click(move |_, window, cx| press(window, cx)),
    )
    .rounded(px(3.))
    .text_color(colour::ash())
    .underline()
    .cursor_pointer()
    .hover(|s| s.text_color(colour::bone()))
    .child(label)
    .into_any_element()
}

/// Typing anywhere goes into the composer (`typeAhead`, inventory 11.3): a
/// key with one printable character other than space, without Cmd, Ctrl or
/// Alt (Shift allowed), pressed where no text field has focus, while the
/// chat is on screen and no modal layer is open, goes at the end of the
/// draft, and the composer takes the focus.
pub fn type_ahead(pane: WeakEntity<ChatPane>, cx: &mut App) -> Subscription {
    cx.intercept_keystrokes(move |event, window, cx| {
        let stroke = &event.keystroke;
        let modifiers = stroke.modifiers;
        if modifiers.platform || modifiers.control || modifiers.alt || modifiers.function {
            return;
        }
        let Some(text) = stroke.key_char.clone() else {
            return;
        };
        // pane agent: the 3D pane keeps its camera keys while it has focus,
        // every key in player mode, and type-ahead is off while it fills
        // the window (`typeAhead && !viewerFull`).
        if super::super::world::keeps_key(&stroke.key, window, cx) {
            return;
        }
        // pane agent: a key on the world closes its block label before the
        // focus moves (BlueMap's window keydown `removeHandler`); the key
        // still goes on to the composer.
        super::super::world::key_on_world(window, cx);
        if text.chars().count() != 1 || text == " " || text.chars().any(char::is_control) {
            return;
        }
        // Not while a modal layer is open (the palette), nor while focus is
        // in a field or in an open picker (its search and rows take keys).
        if super::super::keys::modal_open(cx)
            || window
                .context_stack()
                .iter()
                .any(|context| context.contains("Input") || context.contains("Popover"))
        {
            return;
        }
        // pane agent (first real look): in a comparison, a digit 1 to 9
        // frames that model's build in the 3D pane, the n-th in the bench's
        // order, ahead of type-ahead (`typeKeyRef`, web/app.js 6122-6124); a
        // digit past the members goes into the composer as any character.
        let member = text
            .parse::<usize>()
            .ok()
            .filter(|n| (1..=9).contains(n))
            .and_then(|n| {
                pane.update(cx, |pane, cx| {
                    let super::bench::Surface::Compare(bench) = &pane.bench.surface else {
                        return None;
                    };
                    let lists = pane.options.lists.read(cx);
                    let listed = lists.benches().iter().find(|b| &b.id == bench)?;
                    listed.order.get(n - 1).cloned()
                })
                .ok()
                .flatten()
            });
        if let Some(member) = member {
            crate::view::world::frame_member(member, cx);
            cx.stop_propagation();
            return;
        }
        let taken = pane
            .update(cx, |pane, cx| {
                if !pane.shown {
                    return false;
                }
                pane.caret_at_end(window, cx);
                pane.composer
                    .update(cx, |c, cx| c.insert(text.clone(), window, cx));
                pane.composer_empty = false;
                cx.notify();
                true
            })
            .unwrap_or(false);
        if taken {
            cx.stop_propagation();
        }
    })
}

impl ChatPane {
    /// The composer's placeholder: the chat view's idea in a chat with no
    /// message yet, none after (`prompted`).
    pub(crate) fn sync_placeholder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let prompted = self.items.iter().any(|i| matches!(i, Item::User { .. }));
        let placeholder = if prompted { "" } else { self.idea };
        // bench agent: a comparison's composer.
        let placeholder = match self.bench.surface {
            super::bench::Surface::Compare(_) => "Follow up with every model",
            _ => placeholder,
        };
        self.composer
            .update(cx, |c, cx| c.set_placeholder(placeholder, window, cx));
    }

    /// Whether setup is unfinished (`preparing`): the steps arrived and a
    /// needed one is not done.
    fn preparing(&self) -> Option<crate::state::setup::SetupStatus> {
        let status = crate::state::setup::setup_status(self.setup.as_ref()?);
        (!status.ready).then_some(status)
    }

    /// Every send (`submit`: Enter, Send): held while setup is unfinished
    /// (when there is something to send), unless it is a new chat with no
    /// model, which goes on to say so.
    pub fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // bench agent: a bench's send, held as a chat's while setup runs.
        if self.bench.surface != super::bench::Surface::Chat {
            if self.preparing().is_some() {
                if !self.composer_empty || !self.draft.attachments.is_empty() {
                    self.held = Some(self.draft_key());
                    cx.notify();
                }
                return;
            }
            return self.bench_send(window, cx);
        }
        let no_model = !self.pickers.read(cx).choice().chosen();
        if self.preparing().is_some() && !no_model {
            if !self.composer_empty || !self.draft.attachments.is_empty() {
                self.held = Some(self.draft_key());
                self.log("held", serde_json::json!({ "key": self.draft_key() }));
                cx.notify();
            }
            return;
        }
        self.send(window, cx);
    }

    /// Sends the composer's message (`send`): nothing while a turn runs or
    /// files are read, or with neither text nor files; a new chat without a
    /// model says so. The turn is `sending` at once, the composer is cleared
    /// (after an edit it holds what it held before), the message shows at
    /// once (an edit's in place of the message it edits) and the view goes
    /// to the newest content.
    pub fn send(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy(cx) || self.creating || !self.draft.reading.is_empty() {
            return;
        }
        let draft = self.current_draft(cx);
        let text = draft.text.trim().to_string();
        if text.is_empty() && draft.attachments.is_empty() {
            return;
        }
        // bench agent: a fill-in left holds the send.
        if self.hold_for_field(window, cx) {
            return;
        }
        let sent_prompts = self.live_prompts(cx);
        let choice = self.pickers.read(cx).choice().clone();
        if !choice.chosen() {
            let text = if self.connected == Some(false) {
                "Connect an AI in Settings first"
            } else {
                "Choose a model before sending"
            };
            return self.notice(text.to_string(), window, cx);
        }
        let effort = self.pickers.read(cx).effort_to_send(cx);
        // par-chat: the picker's pair goes with every chat message; the new
        // chat's message goes to its row once made (`createdChat`), waits
        // while it is being made, and makes its own with no row asked for.
        let row = match &self.open {
            Some(id) => Some(id.clone()),
            None => self.new_row_id(),
        };
        let wait = self.open.is_none() && self.new_row == super::NewRow::Asked;
        let target = match row {
            Some(id) => Target::Chat {
                id,
                provider: choice.provider.clone(),
                model: choice.model.clone(),
            },
            None => Target::NewChat {
                provider: choice.provider.clone(),
                model: choice.model.clone(),
            },
        };
        self.creating = self.open.is_none();
        self.log(
            "sent",
            serde_json::json!({ "text": text, "files": draft.attachments.len() }),
        );
        let before = self.items.clone();
        if let Some(edit) = &draft.edit {
            let at = self
                .items
                .iter()
                .position(
                    |i| matches!(i, Item::User { message_id: Some(id), .. } if *id == edit.of),
                )
                .unwrap_or_else(|| panic!("the edited message {} is on screen", edit.of));
            self.items.truncate(at);
        }
        // A turn starts: both live regions lose their text (no value, not
        // an empty one), so the same answer or error again is announced.
        self.turn = Some((self.items.len(), false));
        self.options.announcer.update(cx, |a, cx| a.clear(cx));
        self.items.push(Item::User {
            text: text.clone(),
            message_id: None,
            files: draft
                .attachments
                .iter()
                .map(|f| ShownFile {
                    name: f.name.clone(),
                    content_type: f.content_type.clone(),
                    size: f.size,
                    source: FileSource::Sent(f.data.clone()),
                })
                .collect(),
            selections: draft.selections.clone(),
        });
        self.phase = turn::turn_to(&self.phase, Turn::Sending);
        self.pending = Some(Pending {
            target: target.clone(),
            before,
            draft: draft.clone(),
            key: self.draft_key(),
        });
        self.put_draft(draft::after_send(&draft), window, cx);
        self.inserted.clear();
        self.held = None;
        self.pin_next = true;
        self.rebuild(cx);
        self.sync_placeholder(window, cx);
        let outgoing = Outgoing {
            target,
            text,
            attachments: draft.attachments.clone(),
            selections: draft.selections.clone(),
            prompts: sent_prompts,
            effort,
            resolution: choice.resolution,
            edit_of: draft.edit.as_ref().map(|e| crate::MessageId(e.of.clone())),
        };
        if wait {
            self.waiting_send = Some(outgoing);
        } else {
            self.options.core.send(Command::Send(outgoing));
        }
        // After Send by any route (Enter, a click, a screen reader's press),
        // the composer has focus, ready for the next message.
        self.composer.update(cx, |c, cx| c.focus(window, cx));
        cx.notify();
    }

    /// Stop (`stop`): while the send is on its way it is kept, and sent once
    /// the core takes the turn; otherwise the chat's turn is stopped.
    pub(crate) fn stop(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.log("stop", serde_json::json!({ "chat": self.open }));
        if self.phase == Turn::Sending {
            self.stop_asked = true;
        } else if let Some(chat) = self.open.clone() {
            self.options.core.send(Command::Stop { chat });
        }
        // Focus stays in the composer's group once Stop goes away.
        self.composer.update(cx, |c, cx| c.focus(window, cx));
        cx.notify();
    }

    // par-chat (stage four's parity pass)
    /// Whether `position` is in the composer's box as last painted (the
    /// box holds the text box, its footer and the palette): a press there
    /// leaves the prompt palette open (`web/app.js` 5607-5612).
    pub fn in_composer_box(&self, position: Point<Pixels>) -> bool {
        self.composer_box
            .get()
            .is_some_and(|bounds| bounds.contains(&position))
    }

    /// The composer block: the held line, the editing line, the box.
    pub(crate) fn composer_block(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let busy = self.busy(cx);
        let has_files = !self.draft.attachments.is_empty();
        let no_model = !self.pickers.read(cx).choice().chosen();
        let disabled =
            (self.composer_empty && !has_files) || no_model || !self.draft.reading.is_empty();
        let control = if busy {
            ring(
                div()
                    .id("stop")
                    .role(Role::Button)
                    .aria_label("Stop generating")
                    .track_focus(&self.stop_focus)
                    .key_context("Part")
                    .on_action(cx.listener(|this, _: &Activate, window, cx| this.stop(window, cx)))
                    .on_click(cx.listener(|this, _, window, cx| this.stop(window, cx))),
            )
            .size(px(32.))
            .rounded_full()
            .flex()
            .items_center()
            .justify_center()
            // par-chat: `.stop`'s border and fills (S:1869-1878).
            .border_1()
            .border_color(colour::stop_border(false))
            .bg(colour::stop_face(false))
            .text_color(colour::err())
            .cursor_pointer()
            .hover(|s| {
                s.text_color(colour::bone())
                    .bg(colour::stop_face(true))
                    .border_color(colour::stop_border(true))
            })
            .child(icons::icon(Icon::Stop, 13.))
        } else {
            ring(
                div()
                    .id("send")
                    .role(Role::Button)
                    .aria_label("Send message")
                    .track_focus(&self.send_focus.clone().tab_stop(!disabled))
                    .key_context("Part")
                    .a11y_synthetic_children(move |builder| {
                        if disabled {
                            builder.parent_node().set_disabled();
                        }
                    })
                    .on_action(
                        cx.listener(|this, _: &Activate, window, cx| this.submit(window, cx)),
                    )
                    .on_click(cx.listener(|this, _, window, cx| this.submit(window, cx))),
            )
            .size(px(32.))
            .flex_none()
            .rounded_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(if disabled {
                colour::raised()
            } else {
                colour::bone()
            })
            .text_color(if disabled {
                colour::dim()
            } else {
                colour::bedrock()
            })
            .when(!disabled, |d| {
                d.cursor_pointer()
                    // par-chat: `.send:hover:not(:disabled)`.
                    .hover(|s| s.bg(colour::send_hover()))
            })
            .child(icons::icon(Icon::Send, 15.))
        };
        let attach = super::super::controls::Button::icon("attach", Icon::Plus, "Attach file")
            .tip("Attach file")
            .on_press({
                let this = cx.entity().downgrade();
                move |window, cx| {
                    let _ = this.update(cx, |this, cx| this.pick_files(window, cx));
                }
            });
        let bench_controls =
            (self.bench.surface != super::bench::Surface::Chat).then(|| self.bench_controls(cx));
        let pickers = super::controls::footer_pickers(&self.pickers);
        // bench agent: the prompts button.
        let palette_open = self.palette.as_ref().is_some_and(|p| p.search.is_some());
        let prompts_button = super::super::controls::Button::icon(
            "prompts-button",
            Icon::Prompts,
            "Insert a saved prompt",
        )
        .tip("Prompts (type / in the message)")
        .toggled(palette_open)
        .pressed(palette_open)
        .on_press({
            let this = cx.entity().downgrade();
            move |window, cx| {
                let _ = this.update(cx, |this, cx| this.toggle_palette(window, cx));
            }
        });
        let footer = div()
            .min_h(px(34.))
            .flex()
            .items_center()
            .gap(px(6.))
            .child(div().flex_none().child(attach))
            .child(div().flex_none().child(prompts_button))
            // pane agent (first real look): `.composer-controls`
            // (min-width: 0) gives way to a narrow column; the send button
            // keeps its size.
            .child(
                div()
                    .ml_auto()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap(px(3.))
                    .map(|d| match bench_controls {
                        // bench agent: Bench's controls: no model picker,
                        // no Stop.
                        Some(controls) => d.children(controls),
                        None => d.child(pickers).child(control),
                    }),
            );
        let fonts = super::super::controls::fonts(cx);
        let line = |id: &'static str| {
            super::super::theme::text(
                div()
                    .id(id)
                    .role(Role::Status)
                    .px(px(4.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .text_size(px(12.5))
                    .text_color(colour::ash()),
                super::super::theme::TextKind::Composer,
                &fonts,
            )
            .text_size(px(12.5))
            .text_color(colour::ash())
        };
        let held = match (&self.held, self.preparing()) {
            (Some(key), Some(status)) if *key == self.draft_key() => {
                // settings agent: the steps are above it on an empty chat.
                let text = crate::state::setup::held_line(&status, self.rows.is_empty());
                Some(
                    line("held")
                        .aria_label(text.clone())
                        .when(!status.failed, |d| {
                            d.child(div().flex().gap(px(3.)).children(
                                (0..3).map(|_| div().size(px(4.)).rounded_full().bg(colour::ash())),
                            ))
                        })
                        .child(div().flex_1().child(text))
                        .child(text_link(
                            "dont-send",
                            "Don't send",
                            {
                                let this = cx.entity().downgrade();
                                move |_, cx| {
                                    let _ = this.update(cx, |this, cx| {
                                        this.held = None;
                                        this.log("dont send", serde_json::json!({}));
                                        cx.notify();
                                    });
                                }
                            },
                            window,
                            cx,
                        )),
                )
            }
            _ => None,
        };
        // settings agent (s4-needs): a step that needs the person, with its
        // row's one button, wherever the steps are not on screen. The line
        // and its button share one focus, so focus stays as the button goes
        // while the installer runs.
        let needs = self
            .needs_line()
            .filter(|_| !self.shows_setup_screen(cx))
            .map(|need| {
                let focus = super::super::controls::focus_for("composer-needs", window, cx);
                let button = need.needs.map(|kind| {
                    let (label, press) = match kind {
                        crate::state::setup::StepNeeds::InstallWsl => {
                            (crate::state::setup::INSTALL_WSL, self.install_wsl_press())
                        }
                        crate::state::setup::StepNeeds::Restart => {
                            (crate::state::setup::RESTART_NOW, self.restart_press())
                        }
                    };
                    super::super::controls::Button::new("composer-need", label)
                        .kind(super::super::controls::Kind::Primary)
                        .focus(focus.clone())
                        .on_press(move |window, cx| press(window, cx))
                });
                line("needs")
                    .aria_label(need.text.clone())
                    .when(button.is_none(), |d| {
                        d.track_focus(&focus.clone().tab_stop(false)).child(
                            div().flex().gap(px(3.)).children(
                                (0..3).map(|_| div().size(px(4.)).rounded_full().bg(colour::ash())),
                            ),
                        )
                    })
                    .child(div().flex_1().min_w_0().child(need.text))
                    .children(button.map(|b| div().flex_none().child(b)))
            });
        let cancel_focus =
            super::super::controls::focus_for("cancel-edit", window, cx).tab_stop(true);
        let in_bench = self.bench.surface != super::bench::Surface::Chat;
        let editing = self.draft.edit.as_ref().map(|_| {
            let text = if in_bench {
                "Editing a round: sending it runs the new version on every model"
            } else {
                "Editing a message: sending it starts a new branch"
            };
            let this = cx.entity().downgrade();
            line("editing")
                .aria_label(text)
                .child(icons::icon(Icon::Compose, 13.))
                .child(div().flex_1().child(text))
                // chat-fix-2: the tip is the button's own (its description),
                // not a box's around it.
                .child(super::super::controls::tip(
                    ring(
                        div()
                            .id("cancel-edit")
                            .role(Role::Button)
                            .aria_label("Cancel edit")
                            .track_focus(&cancel_focus)
                            .key_context("Part")
                            .on_action({
                                let this = this.clone();
                                move |_: &Activate, window, cx| {
                                    let _ =
                                        this.update(cx, |this, cx| this.cancel_edit(window, cx));
                                }
                            })
                            .on_click(move |_, window, cx| {
                                let _ = this.update(cx, |this, cx| this.cancel_edit(window, cx));
                            }),
                    )
                    .rounded(px(3.))
                    .underline()
                    .cursor_pointer()
                    .hover(|s| s.text_color(colour::bone()))
                    .child("Cancel"),
                    "cancel-edit-tip",
                    "Cancel edit (Esc)",
                ))
        });
        let parts = self.composer_parts(cx);
        // bench agent: the prompt palette, and the prompt boxes' look
        // (`.ce-field-input`).
        let palette = self.palette_element(window, cx);
        let field_style = gpui_kit::component::input::InlineFieldStyle {
            background: colour::bedrock(),
            border: colour::input_line(),
            focus_border: colour::bone().opacity(0.4),
            placeholder: colour::dim().opacity(0.7),
            radius: px(5.),
            inset: px(6.),
        };
        if self.composer.read(cx).field_style() != field_style {
            self.composer
                .update(cx, |c, cx| c.set_field_style(field_style, cx));
        }
        // par-chat: `:focus-within`: the text box or the box's own Send or
        // Stop has focus.
        let focused = self.composer.read(cx).focus_handle(cx).is_focused(window)
            || self.send_focus.is_focused(window)
            || self.stop_focus.is_focused(window);
        // narrow agent: `.composer` padding `8px 14px 14px`, at 980 px and
        // below `8px 12px 12px`.
        let side = if self.narrow { 12. } else { 14. };
        div()
            .px(px(side))
            .pt(px(8.))
            .pb(px(side))
            .flex()
            .flex_col()
            .gap(px(8.))
            .children(needs)
            .children(held)
            .children(editing)
            .child(
                div()
                    .id("composer-box")
                    // par-chat: the box's bounds, painted (`in_composer_box`).
                    .relative()
                    .child({
                        let at = self.composer_box.clone();
                        canvas(move |bounds, _, _| at.set(Some(bounds)), |_, _, _, _| {})
                            .absolute()
                            .size_full()
                    })
                    .min_h(px(104.))
                    .p(px(8.))
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .rounded(px(super::super::theme::radius::COMPOSER))
                    .border_1()
                    .border_color(if focused {
                        colour::bone_24()
                    } else {
                        colour::line()
                    })
                    .bg(colour::well())
                    // par-chat: the box's rest and focus shadows.
                    .shadow(super::super::theme::shadow::composer(focused))
                    // A paste of files, pictures or a copied message is the
                    // composer's; plain text goes on to the text box.
                    .capture_action(cx.listener(
                        |this, _: &gpui_kit::component::input::Paste, window, cx| {
                            if this.paste(window, cx) {
                                cx.stop_propagation();
                            }
                        },
                    ))
                    // The text box sends on Enter (`PressEnter`) and passes
                    // the key on; it ends here, or the platform would type
                    // the key's newline into the cleared box.
                    .on_action(cx.listener(|_, _: &gpui_kit::component::input::Enter, _, _| {}))
                    // bench agent: the palette's keys come first (Up, Down,
                    // Enter, Tab, Escape); then Tab and Shift+Tab walk a
                    // prompt's fill-ins while there are any.
                    .capture_action(cx.listener(
                        |this, _: &gpui_kit::component::input::MoveDown, window, cx| {
                            if this.palette_key("down", false, window, cx) {
                                cx.stop_propagation();
                            }
                        },
                    ))
                    .capture_action(cx.listener(
                        |this, _: &gpui_kit::component::input::MoveUp, window, cx| {
                            if this.palette_key("up", false, window, cx) {
                                cx.stop_propagation();
                            }
                        },
                    ))
                    .capture_action(cx.listener(
                        |this, e: &gpui_kit::component::input::Enter, window, cx| {
                            if this.palette_key("enter", e.shift, window, cx) {
                                cx.stop_propagation();
                            }
                        },
                    ))
                    .capture_action(cx.listener(
                        |this, _: &gpui_kit::component::input::Escape, window, cx| {
                            if this.palette_key("escape", false, window, cx) {
                                cx.stop_propagation();
                            }
                        },
                    ))
                    .capture_action(cx.listener(
                        |this, _: &gpui_kit::component::input::IndentInline, window, cx| {
                            if this.palette_key("tab", false, window, cx)
                                || this.step_field(false, cx)
                            {
                                cx.stop_propagation();
                            }
                        },
                    ))
                    .capture_action(cx.listener(
                        |this, _: &gpui_kit::component::input::OutdentInline, _, cx| {
                            if this.step_field(true, cx) {
                                cx.stop_propagation();
                            }
                        },
                    ))
                    // bench agent: the palette sits 8 px over the box,
                    // across its width (`.prompt-palette`).
                    .children(palette.map(|p| {
                        div()
                            .absolute()
                            .left_0()
                            .right_0()
                            .bottom_full()
                            .pb(px(8.))
                            .child(p)
                    }))
                    .children(parts)
                    .child(
                        Textarea::new(&self.composer)
                            .aria_label("Message")
                            .appearance(false)
                            .text_size(px(14.)),
                    )
                    .child(footer),
            )
            .into_any_element()
    }
}
