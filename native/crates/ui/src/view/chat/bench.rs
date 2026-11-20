//! The chat pane in Bench (bench agent; inventory `app.md` 6.6, 6.8, 7.2,
//! 7.9, `components.md` L.1): the same composer sends a bench's first
//! round (the new bench) and its next rounds (a comparison). On the new
//! bench the transcript's place holds the line-up: `What should they
//! build?`, one chip per chosen model (`Remove <name>`, its effort menu
//! with `Also run <name> at <effort>`), the `Bench models` picker in
//! multi-select (2 to 8 models), and the hints; the footer holds `Blind`
//! and `Run on <n> models`. On a comparison the transcript's place holds
//! the comparison (`view::bench`), the footer the bench's resolution (read
//! only above Standard) and `Send to every model`. Neither has Stop: a
//! model is stopped on its row. A bench's sends never carry selections or
//! an effort; a round carries its bench's resolution.

use std::rc::Rc;

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::ChatPane;
use crate::state::bench::{self as rules, MIN_MODELS};
use crate::state::draft;
use crate::state::place;
use crate::state::transcript::ShownFile;
use crate::view::controls::popover::{self as popover, Menu, PopoverState};
use crate::view::controls::{Button, Kind, fonts};
use crate::view::icons::{self, Icon};
use crate::view::pickers::model::{Look, ModelPicker};
use crate::view::theme::{self, TextKind, colour};
use crate::{BenchChoice, Command, Outgoing, Resolution, Target};

/// What the chat column shows under its header.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Surface {
    /// A Build chat, the new chat, or a bench model's chat.
    Chat,
    /// The new bench.
    NewBench,
    /// A bench's comparison.
    Compare(String),
}

/// The pane's Bench side.
pub struct BenchSide {
    /// What the column shows.
    pub surface: Surface,
    /// The comparison's view (`view::bench::BenchPane`), drawn for a
    /// comparison in the transcript's place.
    pub body: Option<AnyView>,
    /// The new bench's line-up (`benchModels`).
    pub line: Vec<BenchChoice>,
    /// `benchBlind`.
    pub blind: bool,
    /// `benchResolution`.
    pub resolution: Resolution,
    /// The `Bench models` picker.
    pub picker: Entity<ModelPicker>,
    /// The resolution slider's track.
    pub track: crate::view::pickers::Bounds,
    /// Each chip's effort menu.
    pub menus: Vec<Entity<PopoverState>>,
    /// The new bench's resolution picker.
    pub resolution_popover: Entity<PopoverState>,
    /// A bench send on its way (the draft goes back on a refusal).
    pub sending: Option<Target>,
    /// Saves the line-up, Blind and resolution (`mcb-bench-*`).
    pub keep: crate::view::bench::KeepBench,
    /// A new bench was made: its comparison opens.
    pub opened: crate::view::bench::Opened,
    /// The window's navigations so far (`navigationRef`), counted by
    /// [`BenchSide::navigated`].
    pub navigation: u64,
    /// The navigation a new bench's send was made at: its comparison opens
    /// only if the person has not moved since (`runBench`).
    pub sent_at: u64,
}

impl BenchSide {
    /// The Bench side, from the kept preferences.
    pub fn new(
        preferences: &crate::Preferences,
        pane: WeakEntity<ChatPane>,
        keep: crate::view::bench::KeepBench,
        opened: crate::view::bench::Opened,
        window: &mut Window,
        cx: &mut App,
    ) -> Self {
        let catalog = crate::view::pickers::catalog::Catalog::installed(cx);
        let picker = cx.new(|cx| {
            let mut picker = ModelPicker::new(
                catalog,
                Rc::new(move |provider, model, _, cx| {
                    let _ = pane.update(cx, |p, cx| p.toggle_member(&provider, &model, cx));
                }),
                window,
                cx,
            );
            picker.set_look(bench_look(preferences.bench_models.len()), cx);
            picker.set_selected(pairs(&preferences.bench_models), cx);
            picker
        });
        BenchSide {
            surface: Surface::Chat,
            body: None,
            line: preferences.bench_models.clone(),
            blind: preferences.bench_blind,
            resolution: preferences.bench_resolution,
            picker,
            track: crate::view::pickers::bounds(),
            menus: Vec::new(),
            resolution_popover: cx
                .new(|cx| PopoverState::new(crate::state::popover::RESOLUTION_EXIT, cx)),
            sending: None,
            keep,
            opened,
            navigation: 0,
            sent_at: 0,
        }
    }

    /// The window went to a place (each of the shell's visits).
    pub fn navigated(&mut self) {
        self.navigation += 1;
    }
}

fn pairs(line: &[BenchChoice]) -> Vec<(String, String)> {
    line.iter()
        .map(|m| (m.provider.clone(), m.model.clone()))
        .collect()
}

/// The `Bench models` picker's look: `Add` once some are chosen, else
/// `Models`.
fn bench_look(chosen: usize) -> Look {
    Look {
        label: "Bench models",
        empty: if chosen > 0 { "Add" } else { "Models" },
        below: false,
        asks: false,
        set_up: None,
        providers: None,
    }
}

impl ChatPane {
    /// The column shows a Bench surface (the new bench or a comparison):
    /// the draft of the place left is parked and this one's taken.
    pub fn show_bench(&mut self, surface: Surface, window: &mut Window, cx: &mut Context<Self>) {
        let leaving = self.draft_key();
        let draft = self.current_draft(cx);
        if !draft::draft_empty(&draft) {
            self.drafts.park(&leaving, draft);
        }
        self.open = None;
        self.bench.surface = surface;
        let arriving = self.draft_key();
        let taken = self.drafts.take(&arriving);
        self.put_draft(taken, window, cx);
        self.items.clear();
        self.rows.clear();
        self.list.reset(0);
        self.list_count = 0;
        self.pending = None;
        self.phase = crate::state::turn::IDLE;
        self.sync_placeholder(window, cx);
        cx.notify();
    }

    /// Sets the comparison's view.
    pub fn set_bench_body(&mut self, body: AnyView) {
        self.bench.body = Some(body);
    }

    /// What the column shows.
    pub fn surface(&self) -> &Surface {
        &self.bench.surface
    }

    /// The new bench's line-up.
    pub fn line_up(&self) -> &[BenchChoice] {
        &self.bench.line
    }

    /// `Blind`.
    pub fn blind(&self) -> bool {
        self.bench.blind
    }

    fn keep_line(&mut self, cx: &mut Context<Self>) {
        let line = self.bench.line.clone();
        (self.bench.keep)(line.clone(), self.bench.blind, self.bench.resolution, cx);
        self.bench.picker.update(cx, |p, cx| {
            p.set_look(bench_look(line.len()), cx);
            p.set_selected(pairs(&line), cx);
        });
        cx.notify();
    }

    /// The picker's toggle (`onToggle`): a chosen model leaves, another
    /// joins while fewer than 8 are chosen.
    pub fn toggle_member(&mut self, provider: &str, model: &str, cx: &mut Context<Self>) {
        self.bench.line = rules::toggle(&self.bench.line, provider, model);
        self.log(
            "bench models",
            serde_json::json!({ "count": self.bench.line.len() }),
        );
        let (p, m) = (provider.to_string(), model.to_string());
        crate::view::pickers::catalog::Catalog::installed(cx)
            .update(cx, |c, _| c.ask_facts(&p, &m));
        self.keep_line(cx);
    }

    fn remove_member(&mut self, index: usize, cx: &mut Context<Self>) {
        self.bench.line.remove(index);
        self.keep_line(cx);
    }

    fn member_effort(&mut self, index: usize, effort: String, cx: &mut Context<Self>) {
        self.bench.line[index].effort = Some(effort);
        self.keep_line(cx);
    }

    fn copy_member(&mut self, index: usize, value: String, shown: String, cx: &mut Context<Self>) {
        self.bench.line = rules::copy(&self.bench.line, index, &value, &shown);
        self.keep_line(cx);
    }

    /// Flips `Blind`.
    pub fn flip_blind(&mut self, cx: &mut Context<Self>) {
        self.bench.blind = !self.bench.blind;
        self.log("blind", serde_json::json!({ "on": self.bench.blind }));
        self.keep_line(cx);
    }

    /// The draft key of a Bench surface (`bench:new`, `bench:<id>`).
    pub(crate) fn bench_draft_key(&self) -> Option<String> {
        match &self.bench.surface {
            Surface::Chat => None,
            Surface::NewBench => Some(place::NEW_BENCH_DRAFT.to_string()),
            Surface::Compare(bench) => Some(place::bench_draft(bench)),
        }
    }

    /// A bench's send (`runBench`, `sendRound`): what the composer holds
    /// (nothing while files are read, or with neither text nor files),
    /// cleared at once and put back on a refusal.
    pub(crate) fn bench_send(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.bench.sending.is_some() || !self.draft.reading.is_empty() {
            return;
        }
        let draft = self.current_draft(cx);
        let text = draft.text.trim().to_string();
        if text.is_empty() && draft.attachments.is_empty() {
            return;
        }
        if self.hold_for_field(window, cx) {
            return;
        }
        let prompts = self.live_prompts(cx);
        let (target, resolution) = match &self.bench.surface {
            Surface::NewBench => {
                if self.bench.line.len() < MIN_MODELS {
                    return;
                }
                (
                    Target::NewBench {
                        members: self.bench.line.clone(),
                        blind: self.bench.blind,
                    },
                    self.bench.resolution,
                )
            }
            Surface::Compare(bench) => {
                let own = self
                    .options
                    .lists
                    .read(cx)
                    .benches()
                    .iter()
                    .find(|b| b.id == *bench)
                    .and_then(|b| b.resolution)
                    .map_or(Resolution::Standard, |r| Resolution::of(r as u32));
                (Target::Bench(bench.clone()), own)
            }
            Surface::Chat => unreachable!("a Bench surface sends a bench's round"),
        };
        self.log(
            "sent",
            serde_json::json!({ "text": text, "files": draft.attachments.len(), "bench": format!("{target:?}") }),
        );
        self.pending = Some(super::Pending {
            target: target.clone(),
            before: self.items.clone(),
            draft: draft.clone(),
            key: self.draft_key(),
        });
        self.bench.sending = Some(target.clone());
        self.bench.sent_at = self.bench.navigation;
        self.put_draft(draft::after_send(&draft), window, cx);
        self.inserted.clear();
        self.held = None;
        self.options.core.send(Command::Send(Outgoing {
            target,
            text,
            attachments: draft.attachments.clone(),
            selections: Vec::new(),
            prompts,
            effort: None,
            resolution,
            edit_of: draft.edit.as_ref().map(|e| crate::MessageId(e.of.clone())),
        }));
        self.composer.update(cx, |c, cx| c.focus(window, cx));
        cx.notify();
    }

    /// A bench send the core took: a new bench's comparison opens.
    pub fn bench_sent(
        &mut self,
        target: &Target,
        bench: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.bench.sending.as_ref() != Some(target) {
            return;
        }
        self.bench.sending = None;
        self.pending = None;
        if matches!(target, Target::NewBench { .. }) && self.bench.navigation == self.bench.sent_at
        {
            (self.bench.opened)(bench.to_string(), window, cx);
        }
        cx.notify();
    }

    /// A bench send refused: the draft went back (`refused`).
    pub fn bench_refused(&mut self, target: &Target) {
        if self.bench.sending.as_ref() == Some(target) {
            self.bench.sending = None;
        }
    }

    /// Puts a round into the composer as its edit (`editRound` through
    /// `editPrompt`): its text, no selections, and its files read back
    /// (each a batch entry until read), as Build's `Edit and branch` does.
    pub fn edit_round(
        &mut self,
        round: String,
        text: String,
        files: Vec<ShownFile>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.draft.edit.is_some() {
            return;
        }
        let batch: Vec<draft::Picked> = files
            .iter()
            .map(|file| {
                self.picks += 1;
                draft::Picked {
                    id: self.picks,
                    name: file.name.clone(),
                    size: file.size,
                }
            })
            .collect();
        let now = self.current_draft(cx);
        let edit = draft::begin_edit(&now, &round, text, Vec::new(), batch.clone());
        self.log(
            "edit",
            serde_json::json!({ "of": round, "files": files.len() }),
        );
        self.put_draft(edit, window, cx);
        self.caret_at_end(window, cx);
        self.composer.update(cx, |c, cx| c.focus(window, cx));
        cx.notify();
        let core = self.options.core.clone();
        cx.spawn_in(window, async move |this, cx| {
            let read: Vec<Result<crate::OutgoingFile, String>> = cx
                .background_executor()
                .spawn(async move {
                    files
                        .iter()
                        .map(|file| {
                            super::message::read_back(file, &*core)
                                .map_err(|why| format!("Could not load \"{}\" ({why})", file.name))
                        })
                        .collect()
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                let mut kept = Vec::new();
                for result in read {
                    match result {
                        Ok(file) => kept.push(file),
                        Err(error) => this.notice(error, window, cx),
                    }
                }
                this.finish_read(&batch, kept, cx);
            });
        })
        .detach();
    }

    /// A file of a round's prompt pressed (`renderFiles`): a picture opens
    /// in the lightbox, any other file with the system, as a message's.
    pub fn open_round_file(
        &mut self,
        file: &ShownFile,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.log("file activated", serde_json::json!({ "name": file.name }));
        match super::message::file_picture(file, &*self.options.core) {
            Ok(picture) if file.is_image() => self.open_lightbox(picture, window, cx),
            Ok(super::Picture::File(path)) => {
                let url = format!("file://{}", path.display());
                (self.options.opener)(&url, cx);
            }
            Ok(super::Picture::Bytes(_)) => {}
            Err(sentence) => self.notice(sentence, window, cx),
        }
    }

    /// The new bench's body: the mark, the question, the line-up and its
    /// hints.
    pub(crate) fn new_bench_body(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let fonts = fonts(cx);
        let catalog = crate::view::pickers::catalog::Catalog::installed(cx);
        while self.bench.menus.len() < self.bench.line.len() {
            self.bench
                .menus
                .push(cx.new(|cx| PopoverState::new(crate::state::popover::MENU_EXIT, cx)));
        }
        let mut chips = div()
            .id("bench-lineup")
            .role(Role::Group)
            .aria_label("Bench models")
            .flex()
            .flex_wrap()
            .justify_center()
            .items_center()
            .gap(px(6.))
            .max_w(px(760.))
            .px(px(16.))
            .mt(px(18.));
        for (index, entry) in self.bench.line.clone().iter().enumerate() {
            let name = crate::state::models::model_label(&entry.model).to_string();
            let facts = catalog
                .read(cx)
                .facts(&entry.provider, &entry.model)
                .cloned();
            let pane = cx.entity().downgrade();
            let remove = Button::new(("bench-chip", index), name.clone())
                .named(format!("Remove {name}"))
                .kind(Kind::Ghost)
                .tip("Remove")
                // par-bench: the name's own `title`, the model's id
                // (`.bench-chip-name`).
                .label_tip(entry.model.clone())
                .on_press(move |_, cx| {
                    let _ = pane.update(cx, |p, cx| p.remove_member(index, cx));
                });
            let mut chip = theme::text(div(), TextKind::PickerTrigger, &fonts)
                .relative()
                .flex()
                .items_center()
                .rounded(px(8.))
                .bg(colour::raised())
                // par-bench: the logo turns into an x while the name is
                // hovered (`.bench-chip-model:hover`).
                .child(
                    div()
                        .group(SharedString::from(format!("bench-chip-model-{index}")))
                        .flex()
                        .items_center()
                        .child(
                            div()
                                .id(("bench-chip-icon", index))
                                .cursor_pointer()
                                .on_click({
                                    let pane = cx.entity().downgrade();
                                    move |_, _, cx| {
                                        let _ = pane.update(cx, |p, cx| p.remove_member(index, cx));
                                    }
                                })
                                .pl(px(8.))
                                .relative()
                                .child(
                                    div()
                                        .group_hover(
                                            SharedString::from(format!("bench-chip-model-{index}")),
                                            |s| s.opacity(0.),
                                        )
                                        .child(icons::logo(
                                            crate::state::vendor::vendor_icon(
                                                &entry.provider,
                                                &entry.model,
                                            ),
                                            13.,
                                        )),
                                )
                                .child(
                                    div()
                                        .absolute()
                                        .top_0()
                                        .left(px(8.))
                                        .size(px(13.))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .opacity(0.)
                                        .group_hover(
                                            SharedString::from(format!("bench-chip-model-{index}")),
                                            |s| s.opacity(1.),
                                        )
                                        .child(icons::icon(Icon::X, 11.)),
                                ),
                        )
                        .child(remove),
                );
            if let Some(facts) = facts.filter(|f| f.reasoning && !f.efforts.is_empty()) {
                let default = facts.default_effort.clone().unwrap_or_default();
                let shown = entry
                    .effort
                    .clone()
                    .filter(|e| !e.is_empty())
                    .unwrap_or_else(|| default.clone());
                let taken = rules::taken(&self.bench.line, &entry.provider, &entry.model, &default);
                let state = self.bench.menus[index].clone();
                let open = state.read(cx).is_open();
                let mut menu = Menu::new(&state, format!("{name} reasoning effort"));
                for effort in &facts.efforts {
                    let pane = cx.entity().downgrade();
                    let value = effort.clone();
                    menu = menu.item(effort.clone(), *effort == shown, move |_, cx| {
                        let value = value.clone();
                        let _ = pane.update(cx, |p, cx| p.member_effort(index, value, cx));
                    });
                }
                // Offered at any count, as the page's are; a copy at eight
                // models changes nothing (`rules::copy`).
                for effort in facts.efforts.iter().filter(|e| !taken.contains(e)) {
                    let pane = cx.entity().downgrade();
                    let (value, current) = (effort.clone(), shown.clone());
                    menu = menu.item(
                        format!("Also run {name} at {effort}"),
                        false,
                        move |_, cx| {
                            let (value, current) = (value.clone(), current.clone());
                            let _ =
                                pane.update(cx, |p, cx| p.copy_member(index, value, current, cx));
                        },
                    );
                }
                let toggler = state.clone();
                let trigger = popover::trigger(
                    &state,
                    Button::new(("bench-chip-effort", index), shown.clone())
                        .named(format!("{name} reasoning effort: {shown}"))
                        .kind(Kind::Ghost)
                        .with_icon(Icon::ChevronDown, 10.)
                        .icon_after()
                        .expanded(open)
                        .on_press(move |window, cx| {
                            toggler.update(cx, |s, cx| s.toggle(window, cx))
                        }),
                    cx,
                );
                let menu = menu.render(cx);
                chip = chip.child(trigger).children(popover::panel(
                    &state,
                    popover::Placement::Above,
                    menu,
                    cx,
                ));
            }
            chips = chips.child(chip);
        }
        let picker = self.bench.picker.clone();
        chips = chips.child(picker);
        let hint: AnyElement = if self.connected == Some(false) {
            crate::view::chat::composer::text_link(
                "bench-connect",
                "Connect an AI to start building",
                {
                    let connect = self.options.connect.clone();
                    move |window, cx| connect(window, cx)
                },
                window,
                cx,
            )
        } else {
            let to_build = self.options.to_build.clone();
            div()
                .flex()
                .flex_col()
                .items_center()
                .gap(px(6.))
                .when(self.bench.line.len() < MIN_MODELS, |d| {
                    d.child(
                        // par-bench: read as the page's text is.
                        theme::text(
                            div()
                                .id("bench-two-models")
                                .role(Role::Label)
                                .aria_label("Choose at least two models"),
                            TextKind::EmptySub,
                            &fonts,
                        )
                        .text_size(px(12.))
                        .text_color(colour::dim())
                        .child("Choose at least two models"),
                    )
                })
                .child(
                    theme::text(div().flex().gap(px(4.)), TextKind::EmptySub, &fonts)
                        .child("Or have one model build it in")
                        .child(crate::view::chat::composer::text_link(
                            "bench-to-build",
                            "Build",
                            move |window, cx| to_build(window, cx),
                            window,
                            cx,
                        )),
                )
                .into_any_element()
        };
        div()
            .id("bench-new")
            .role(Role::Region)
            .aria_label("New bench")
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(13.))
            .child(icons::app_mark(56.))
            .child(
                theme::text(div(), TextKind::EmptyHeading, &fonts).child("What should they build?"),
            )
            .child(chips)
            .child(hint)
            .into_any_element()
    }

    /// The footer's controls in Bench: the resolution (the new bench's, or
    /// the bench's read only above Standard), `Blind` on the new bench, and
    /// the send.
    pub(crate) fn bench_controls(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut out = Vec::new();
        let new = self.bench.surface == Surface::NewBench;
        let has_files = !self.draft.attachments.is_empty();
        let empty = self.composer_empty && !has_files;
        if new {
            let pane = cx.entity().downgrade();
            let track = self.bench.track.clone();
            out.push(crate::view::pickers::resolution::resolution_picker(
                &self.bench.resolution_popover,
                &track,
                self.bench.resolution,
                false,
                Rc::new(move |resolution, _, cx| {
                    let _ = pane.update(cx, |p, cx| {
                        p.bench.resolution = resolution;
                        p.keep_line(cx);
                    });
                }),
                cx,
            ));
            let on = self.bench.blind;
            let pane = cx.entity().downgrade();
            out.push(
                Button::new("blind", "Blind")
                    .kind(Kind::Toggle)
                    .with_icon(if on { Icon::EyeOff } else { Icon::Eye }, 14.)
                    .toggled(on)
                    .pressed(on)
                    .tip(if on {
                        "Blind: models go by letters until you reveal them"
                    } else {
                        "Names shown: turn on to judge without knowing which model is which"
                    })
                    .on_press(move |_, cx| {
                        let _ = pane.update(cx, |p, cx| p.flip_blind(cx));
                    })
                    .into_any_element(),
            );
        } else if let Surface::Compare(bench) = &self.bench.surface {
            let own = self
                .options
                .lists
                .read(cx)
                .benches()
                .iter()
                .find(|b| b.id == *bench)
                .and_then(|b| b.resolution)
                .map_or(Resolution::Standard, |r| Resolution::of(r as u32));
            if own != Resolution::Standard {
                let track = self.bench.track.clone();
                out.push(crate::view::pickers::resolution::resolution_picker(
                    &self.bench.resolution_popover,
                    &track,
                    own,
                    true,
                    Rc::new(|_, _, _| {}),
                    cx,
                ));
            }
        }
        let count = self.bench.line.len();
        let label = if new {
            rules::run_label(count)
        } else {
            "Send to every model".to_string()
        };
        let disabled = empty
            || (new && count < MIN_MODELS)
            || !self.draft.reading.is_empty()
            || self.bench.sending.is_some();
        let pane = cx.entity().downgrade();
        out.push(
            Button::icon("send", Icon::Send, label.clone())
                .tip(label)
                .kind(Kind::Primary)
                .disabled(disabled)
                .on_press(move |window, cx| {
                    let _ = pane.update(cx, |p, cx| p.submit(window, cx));
                })
                .into_any_element(),
        );
        out
    }
}
