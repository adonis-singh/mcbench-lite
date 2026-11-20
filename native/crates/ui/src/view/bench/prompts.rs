//! The prompt library's place: the editor (`PromptEditor`, `web/app.js`
//! 2213-2509; inventory `components.md` I.3, I.4, I.8,
//! `style-viewer-routes.md` 1.12). With no prompt: `No prompts yet`, its
//! line and `New prompt`. Else the title (`Untitled prompt`), the `Added
//! by` mark when a model added it, the tags (each with `Remove tag <tag>`,
//! a field `Add tag`: Enter, a comma or leaving it adds one, Backspace on
//! an empty field removes the last), the body (`{{fill-in}}` marks), and
//! the foot: `Attach file` and the attachments (`Remove attachment`), the
//! improve controls: the `Generation model` picker, the split button
//! (`Improve with AI` or `Write with AI`, `Improving…`, its menu `Choose
//! what the AI works from` with `Improve the text` and `Write from a
//! note`), the note box and `Write`, and the last failure. Every keystroke
//! is a library edit (`updatePrompt`), saved by the sync.

use std::rc::Rc;
use std::sync::Arc;

use gpui_kit::component::Sizable as _;
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::library::Library;
use crate::state::prompts;
use crate::view::controls::popover::{self as popover, PopoverState};
use crate::view::controls::{Button, Kind, fonts};
use crate::view::icons::{self, Icon};
use crate::view::pickers::model::{Look, ModelPicker};
use crate::view::shell::Shell;
use crate::view::theme::{self, TextKind, colour};
use crate::{BenchChoice, Core, LibraryPrompt, PromptAttachment};

/// The editor.
pub struct PromptEditor {
    library: Entity<Library>,
    title: Entity<InputState>,
    tag: Entity<InputState>,
    body: Entity<TextareaState>,
    note: Entity<TextareaState>,
    model: Entity<ModelPicker>,
    menu: Entity<PopoverState>,
    /// The `Generation reasoning effort` list.
    effort: Entity<PopoverState>,
    /// The note box (`.pe-ai-note`), over the split button.
    note_box: Entity<PopoverState>,
    /// The tag field's highlighted suggestion.
    suggestion: Option<usize>,
    /// A write from the note is on its way: the note empties once it lands.
    asked_note: bool,
    /// The prompt the fields hold.
    bound: Option<String>,
    /// The generation model and effort (`mcb-prompt-ai-model`).
    pub(crate) ai: Option<BenchChoice>,
    /// Writes from a note (`mcb-improve-source` = `note`).
    pub(crate) from_note: bool,
    picker: crate::view::Picker,
    keep: crate::view::bench::KeepEditor,
    notice: crate::view::bench::Notify,
    pub(crate) log: Option<crate::ActionLog>,
    writing: bool,
    /// The tag field's placeholder now (`Add tag` only with no tags).
    tag_hint: &'static str,
    _subscriptions: Vec<Subscription>,
}

impl PromptEditor {
    /// The editor over `library`.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        library: Entity<Library>,
        _core: Arc<dyn Core>,
        preferences: &crate::Preferences,
        picker: crate::view::Picker,
        keep: crate::view::bench::KeepEditor,
        notice: crate::view::bench::Notify,
        log: Option<crate::ActionLog>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let title = cx.new(|cx| InputState::new(window, cx).placeholder(prompts::UNTITLED));
        let tag = cx.new(|cx| InputState::new(window, cx).placeholder("Add tag"));
        let body = cx.new(|cx| {
            TextareaState::new(window, cx).placeholder(
                "Describe the build. Write {{like this}} for parts to fill in when you insert it.",
            )
        });
        let note = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(3, 3)
                .submit_on_enter(true)
                .placeholder("e.g. a lighthouse on a cliff, stormy")
        });
        let catalog = crate::view::pickers::catalog::Catalog::installed(cx);
        let catalog_for_facts = catalog.clone();
        let this = cx.entity().downgrade();
        let model = cx.new(|cx| {
            let mut picker = ModelPicker::new(
                catalog,
                Rc::new(move |provider, model, _, cx| {
                    let _ = this.update(cx, |e, cx| e.pick_model(provider, model, cx));
                }),
                window,
                cx,
            );
            picker.set_look(
                Look {
                    label: "Generation model",
                    empty: "Choose generation model",
                    below: false,
                    asks: false,
                    set_up: None,
                    providers: None,
                },
                cx,
            );
            picker
        });
        let ai = preferences.prompt_ai_model.clone().or_else(|| {
            preferences
                .last_model
                .clone()
                .map(|(provider, model)| BenchChoice {
                    provider,
                    model,
                    effort: None,
                })
        });
        if let Some(ai) = &ai {
            let value = Some((ai.provider.clone(), ai.model.clone()));
            model.update(cx, |m, cx| m.set_value(value, cx));
        }
        let subscriptions = vec![
            cx.subscribe_in(
                &title,
                window,
                |this: &mut Self, s, e: &InputEvent, _, cx| {
                    if matches!(e, InputEvent::Change) && !this.writing {
                        let value = s.read(cx).value().to_string();
                        this.patch(move |p| p.title = value.trim().to_string(), cx);
                    }
                },
            ),
            cx.subscribe_in(
                &body,
                window,
                |this: &mut Self, s, e: &InputEvent, _, cx| {
                    if matches!(e, InputEvent::Change) && !this.writing {
                        let value = s.read(cx).value().to_string();
                        this.patch(move |p| p.text = value.clone(), cx);
                    }
                },
            ),
            cx.subscribe_in(
                &tag,
                window,
                |this: &mut Self, s, e: &InputEvent, window, cx| match e {
                    InputEvent::Change => {
                        this.suggestion = None;
                        cx.notify();
                        let value = s.read(cx).value().to_string();
                        if let Some(tag) = prompts::comma_tag(&value) {
                            this.add_tag(&tag, window, cx);
                        }
                    }
                    InputEvent::PressEnter { .. } | InputEvent::Blur => {
                        let value = s.read(cx).value().to_string();
                        // Enter on a highlighted suggestion takes it.
                        let chosen = match (e, this.suggestion) {
                            (InputEvent::PressEnter { .. }, Some(i)) => {
                                let prompt = this.library.read(cx).shown();
                                prompt.and_then(|p| {
                                    prompts::suggested_tags(
                                        &prompts::known_tags(this.library.read(cx).list(), &p.tags),
                                        &value,
                                    )
                                    .get(i)
                                    .cloned()
                                })
                            }
                            _ => None,
                        };
                        this.suggestion = None;
                        this.add_tag(&chosen.unwrap_or(value), window, cx);
                    }
                    _ => {}
                },
            ),
            cx.subscribe_in(
                &note,
                window,
                |this: &mut Self, _, e: &InputEvent, window, cx| {
                    if let InputEvent::PressEnter { shift: false, .. } = e {
                        this.send_note(window, cx);
                    }
                },
            ),
            cx.observe_in(&library, window, |this: &mut Self, library, window, cx| {
                // The note empties once what it asked for lands (`setNote('')`).
                let (asking, failed) = {
                    let l = library.read(cx);
                    (l.asked.is_some(), l.ai_error.is_some())
                };
                if this.asked_note && !asking {
                    this.asked_note = false;
                    if !failed {
                        this.note.update(cx, |n, cx| n.set_value("", window, cx));
                    }
                }
                this.bind(window, cx);
                cx.notify();
            }),
            cx.observe(&catalog_for_facts, |_: &mut Self, _, cx| cx.notify()),
        ];
        let mut editor = PromptEditor {
            library,
            title,
            tag,
            body,
            note,
            model,
            menu: cx.new(|cx| PopoverState::new(crate::state::popover::MENU_EXIT, cx)),
            effort: cx.new(|cx| PopoverState::new(crate::state::popover::MENU_EXIT, cx)),
            note_box: cx.new(|cx| PopoverState::new(crate::state::popover::MENU_EXIT, cx)),
            suggestion: None,
            asked_note: false,
            bound: None,
            ai,
            from_note: preferences.improve_source_note,
            picker,
            keep,
            notice,
            log,
            writing: false,
            tag_hint: "Add tag",
            _subscriptions: subscriptions,
        };
        editor.bind(window, cx);
        editor.ask_facts(cx);
        editor
    }

    /// Asks for the generation model's facts (its efforts), and draws again
    /// when they come.
    fn ask_facts(&mut self, cx: &mut Context<Self>) {
        let Some(ai) = self.ai.clone() else {
            return;
        };
        let catalog = crate::view::pickers::catalog::Catalog::installed(cx);
        catalog.update(cx, |c, _| c.ask_facts(&ai.provider, &ai.model));
    }

    /// The generation model's facts, when they have arrived.
    fn facts(&self, cx: &App) -> Option<crate::ModelFacts> {
        let ai = self.ai.as_ref()?;
        crate::view::pickers::catalog::Catalog::installed(cx)
            .read(cx)
            .facts(&ai.provider, &ai.model)
            .cloned()
    }

    fn log(&self, action: &str, fields: serde_json::Value) {
        if let Some(log) = &self.log {
            log.record(action, fields);
        }
    }

    /// The fields hold the shown prompt: set again when another is shown,
    /// and when its text changed under them (an improve landed, a model
    /// changed the library) while the field does not hold it already.
    fn bind(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let shown = self.library.read(cx).shown();
        let id = shown.as_ref().map(|p| p.id.clone());
        self.writing = true;
        if let Some(prompt) = shown {
            let switched = self.bound != id;
            if switched {
                self.title
                    .update(cx, |t, cx| t.set_value(prompt.title.clone(), window, cx));
            }
            if switched || self.body.read(cx).value().as_ref() != prompt.text {
                self.body
                    .update(cx, |b, cx| b.set_value(prompt.text.clone(), window, cx));
            }
            if switched {
                self.tag.update(cx, |t, cx| t.set_value("", window, cx));
                self.suggestion = None;
                self.note_box
                    .update(cx, |n, cx| n.close_keeping_focus(window, cx));
            }
        }
        self.writing = false;
        self.bound = id;
    }

    fn patch(&mut self, change: impl Fn(&mut LibraryPrompt) + 'static, cx: &mut Context<Self>) {
        let Some(id) = self.bound.clone() else {
            return;
        };
        self.library.update(cx, |l, cx| l.update(&id, change, cx));
    }

    fn add_tag(&mut self, typed: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(prompt) = self.library.read(cx).shown() else {
            return;
        };
        self.writing = true;
        self.tag.update(cx, |t, cx| t.set_value("", window, cx));
        self.writing = false;
        if let Some(tag) = prompts::new_tag(&prompt.tags, typed) {
            self.log("tag", serde_json::json!({ "tag": tag }));
            self.patch(move |p| p.tags.push(tag.clone()), cx);
        }
    }

    fn remove_tag(&mut self, tag: String, cx: &mut Context<Self>) {
        self.patch(move |p| p.tags.retain(|t| *t != tag), cx);
    }

    fn pick_model(&mut self, provider: String, model: String, cx: &mut Context<Self>) {
        let value = Some((provider.clone(), model.clone()));
        self.model.update(cx, |m, cx| m.set_value(value, cx));
        self.ai = Some(BenchChoice {
            provider,
            model,
            effort: None,
        });
        (self.keep)(self.ai.clone(), self.from_note, cx);
        self.ask_facts(cx);
        cx.notify();
    }

    /// `Generation reasoning effort` chosen: kept with the model.
    fn pick_effort(&mut self, effort: String, cx: &mut Context<Self>) {
        let Some(ai) = self.ai.as_mut() else {
            return;
        };
        ai.effort = Some(effort);
        (self.keep)(self.ai.clone(), self.from_note, cx);
        cx.notify();
    }

    fn set_source(&mut self, note: bool, cx: &mut Context<Self>) {
        self.from_note = note;
        (self.keep)(self.ai.clone(), note, cx);
        cx.notify();
    }

    /// Ready: a model whose provider is connected, nothing on its way.
    fn ready(&self, cx: &App) -> bool {
        let Some(ai) = &self.ai else {
            return false;
        };
        let connected = crate::view::pickers::catalog::Catalog::installed(cx)
            .read(cx)
            .providers()
            .iter()
            .any(|p| p.id == ai.provider && p.connected && p.enabled);
        connected && self.library.read(cx).asked.is_none()
    }

    fn send_text(&mut self, text: String, cx: &mut Context<Self>) {
        let Some(ai) = self.ai.clone() else {
            return;
        };
        // `effortFor`: the chosen effort if the model lists it, else its
        // default; none for a model that does not reason.
        let facts = self.facts(cx);
        let effort = crate::state::models::effort_for(facts.as_ref(), ai.effort.as_deref());
        self.log(
            "improve",
            serde_json::json!({ "note": self.from_note, "effort": effort }),
        );
        self.library.update(cx, |l, cx| {
            l.ask(ai.provider.clone(), ai.model.clone(), effort, text, cx)
        });
    }

    fn send_note(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let note = self.note.read(cx).value().trim().to_string();
        if note.is_empty() || !self.ready(cx) {
            return;
        }
        self.note_box.update(cx, |n, cx| n.close(window, cx));
        self.asked_note = true;
        self.send_text(note, cx);
    }

    /// The tag field's suggestions (`datalist`): a list under the field,
    /// one row a tag; a press takes it, as Enter on the highlighted one.
    fn suggestion_list(
        &mut self,
        tags: &[String],
        fonts: &crate::view::theme::Fonts,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let count = tags.len();
        let mut list = div()
            .id("tag-suggestions")
            .role(Role::ListBox)
            .aria_label("Tag suggestions")
            .aria_size_of_set(count)
            .absolute()
            .top(px(24.))
            .left(px(4.))
            .w(px(190.))
            .max_h(px(260.))
            .overflow_y_scroll()
            .p(px(5.))
            .rounded(px(theme::radius::MENU))
            .border_1()
            .border_color(colour::line())
            .bg(colour::well())
            .shadow(theme::shadow::menu())
            .flex()
            .flex_col();
        for (index, tag) in tags.iter().enumerate() {
            let highlighted = self.suggestion == Some(index);
            let taken = tag.clone();
            list = list.child(
                theme::text(
                    div()
                        .id(("tag-suggestion", index))
                        .role(Role::ListBoxOption)
                        .aria_label(tag.clone())
                        .aria_selected(highlighted)
                        .aria_position_in_set(index)
                        .aria_size_of_set(count)
                        .min_h(px(30.))
                        .px(px(8.))
                        .py(px(5.))
                        .rounded(px(theme::radius::ROW))
                        .flex()
                        .items_center()
                        .cursor_pointer()
                        .when(highlighted, |d| {
                            d.bg(colour::raised()).text_color(colour::bone())
                        })
                        .hover(|s| s.bg(colour::raised()).text_color(colour::bone()))
                        // A press takes the tag; the field keeps its focus.
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _, window, cx| {
                                cx.stop_propagation();
                                this.suggestion = None;
                                this.add_tag(&taken, window, cx);
                            }),
                        ),
                    TextKind::MenuItem,
                    fonts,
                )
                .child(div().truncate().child(tag.clone())),
            );
        }
        deferred(list).priority(2).into_any_element()
    }

    /// The shown prompt's files as the limits count them.
    fn have(&self, cx: &App) -> Vec<(String, u64)> {
        self.library
            .read(cx)
            .shown()
            .map(|p| {
                p.attachments
                    .iter()
                    .map(|a| (a.name.clone(), a.size))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Files picked with `Attach file` (`addFiles`).
    fn pick_files(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let picked = (self.picker)(cx);
        let Some(id) = self.bound.clone() else {
            return;
        };
        let have = self.have(cx);
        cx.spawn_in(window, async move |this, cx| {
            let paths = picked.await.unwrap_or(Ok(Vec::new()));
            let read = cx
                .background_executor()
                .spawn(async move { read_paths(&have, paths?) })
                .await;
            let _ = this.update(cx, |this, cx| this.attach(&id, read, cx));
        })
        .detach();
    }

    /// The files read join the prompt they were added to, as it is once
    /// they are read (`addFiles`); a failure is a notice.
    fn attach(
        &mut self,
        id: &str,
        read: Result<Vec<crate::OutgoingFile>, String>,
        cx: &mut Context<Self>,
    ) {
        match read {
            Ok(files) => {
                let added = prompt_attachments(files);
                self.library.update(cx, |l, cx| {
                    l.update(id, move |p| p.attachments.extend(added.clone()), cx)
                });
            }
            Err(error) => (self.notice)(error, cx),
        }
    }

    /// A paste in the body (`onPaste`): a message copied from this app
    /// brings its text (in place of the selection) and its files, within
    /// the limits; files or pictures on the clipboard join the prompt;
    /// otherwise the body pastes the text. Returns whether it took the
    /// paste.
    fn paste(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(item) = cx.read_from_clipboard() else {
            return false;
        };
        let Some(id) = self.bound.clone() else {
            return false;
        };
        let have = self.have(cx);
        for entry in item.entries() {
            if let ClipboardEntry::String(string) = entry
                && let Some(metadata) = string.metadata_json::<serde_json::Value>()
            {
                match crate::state::clipboard::read_message(&metadata) {
                    Ok(Some((text, files))) => {
                        let added: Vec<(String, u64)> =
                            files.iter().map(|f| (f.name.clone(), f.size)).collect();
                        if let Err(error) =
                            crate::state::upload::check_attachment_limits(&have, &added)
                        {
                            (self.notice)(error, cx);
                            return true;
                        }
                        self.body.update(cx, |b, cx| b.replace(text, window, cx));
                        let value = self.body.read(cx).value().to_string();
                        let files = prompt_attachments(files);
                        self.log(
                            "pasted message",
                            serde_json::json!({ "files": added.len() }),
                        );
                        self.patch(
                            move |p| {
                                p.text = value.clone();
                                p.attachments.extend(files.clone());
                            },
                            cx,
                        );
                        return true;
                    }
                    Ok(None) => {}
                    Err(error) => {
                        (self.notice)(error, cx);
                        return true;
                    }
                }
            }
        }
        let mut paths = Vec::new();
        let mut pictures = Vec::new();
        for entry in item.entries() {
            match entry {
                ClipboardEntry::ExternalPaths(external) => {
                    paths.extend(external.paths().iter().cloned())
                }
                ClipboardEntry::Image(image) => pictures.push(image.clone()),
                ClipboardEntry::String(_) => {}
            }
        }
        if paths.is_empty() && pictures.is_empty() {
            return false;
        }
        self.log(
            "pasted files",
            serde_json::json!({ "files": paths.len() + pictures.len() }),
        );
        cx.spawn_in(window, async move |this, cx| {
            let read = cx
                .background_executor()
                .spawn(async move {
                    if !paths.is_empty() {
                        return read_paths(&have, paths);
                    }
                    let sized: Vec<(String, u64)> = pictures
                        .iter()
                        .map(|p| {
                            (
                                crate::state::upload::PASTED_NAME.to_string(),
                                p.bytes.len() as u64,
                            )
                        })
                        .collect();
                    crate::state::upload::check_attachment_limits(&have, &sized)?;
                    pictures
                        .into_iter()
                        .map(|picture| {
                            let (name, png) = crate::state::upload::pasted_picture(
                                picture.format.mime_type(),
                                picture.bytes,
                            )?;
                            crate::state::upload::read_upload(&name, Some("image/png"), png)
                        })
                        .collect()
                })
                .await;
            let _ = this.update(cx, |this, cx| this.attach(&id, read, cx));
        })
        .detach();
        true
    }
}

/// Files read from `paths` (`readUpload` each) once the limits allow them
/// beside `have`.
fn read_paths(
    have: &[(String, u64)],
    paths: Vec<std::path::PathBuf>,
) -> Result<Vec<crate::OutgoingFile>, String> {
    let sized: Vec<(String, u64)> = paths
        .iter()
        .map(|p| {
            let name = p
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let size = std::fs::metadata(p)
                .map_err(|e| format!("\"{name}\" could not be read: {e}"))?
                .len();
            Ok((name, size))
        })
        .collect::<Result<_, String>>()?;
    crate::state::upload::check_attachment_limits(have, &sized)?;
    paths
        .iter()
        .map(|p| crate::state::upload::read_path(p))
        .collect()
}

/// Read files as a prompt keeps them: each with its data URL.
fn prompt_attachments(files: Vec<crate::OutgoingFile>) -> Vec<PromptAttachment> {
    use base64::Engine as _;
    files
        .into_iter()
        .map(|f| PromptAttachment {
            data: format!(
                "data:{};base64,{}",
                f.content_type,
                base64::engine::general_purpose::STANDARD.encode(&f.data)
            ),
            name: f.name,
            content_type: f.content_type,
            size: f.size,
        })
        .collect()
}

/// The library's place in the chat column.
pub fn place(shell: &Shell, _window: &mut Window, _cx: &mut Context<Shell>) -> AnyElement {
    div()
        .id("prompts-place")
        .role(Role::Region)
        .aria_label("Prompts")
        .size_full()
        .child(shell.prompt_editor())
        .into_any_element()
}

impl Render for PromptEditor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let fonts = fonts(cx);
        let library = self.library.clone();
        let Some(prompt) = library.read(cx).shown() else {
            let lib = library.clone();
            return div()
                .id("prompt-editor")
                .role(Role::Region)
                .aria_label("No prompts yet")
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .max_w(px(360.))
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap(px(10.))
                        .child(icons::app_mark(44.))
                        // `.pe-empty-state h2`: 16 px, 600, `--bone`.
                        .child(
                            theme::text(div(), TextKind::Button, &fonts)
                                .text_size(px(16.))
                                .font_weight(FontWeight(600.))
                                .child("No prompts yet"),
                        )
                        // `.pe-empty-state p`: 13 px at 1.55, `--ash`, its
                        // `/` and `{{style}}` in `--bone` on `--raised`.
                        .child(
                            theme::text(div(), TextKind::EmptySub, &fonts)
                                .text_center()
                                .line_height(relative(1.55))
                                .text_color(colour::ash())
                                .mb(px(8.))
                                .child(empty_line()),
                        )
                        .child(
                            Button::new("prompt-new", "New prompt")
                                .kind(Kind::Primary)
                                .with_icon(Icon::Plus, 14.)
                                .on_press(move |_, cx| {
                                    lib.update(cx, |l, cx| {
                                        l.create(cx);
                                    })
                                }),
                        ),
                )
                .into_any_element();
        };
        if self.bound.as_deref() != Some(prompt.id.as_str()) {
            self.bind(window, cx);
        }
        // `Add tag` only while the prompt has no tags.
        let placeholder = if prompt.tags.is_empty() {
            "Add tag"
        } else {
            ""
        };
        if self.tag_hint != placeholder {
            self.tag_hint = placeholder;
            self.tag
                .update(cx, |t, cx| t.set_placeholder(placeholder, window, cx));
        }
        let this = cx.entity().downgrade();
        let mut tags = div()
            .id("prompt-tags")
            .role(Role::Group)
            .aria_label("Tags")
            .flex()
            .flex_wrap()
            .gap(px(5.))
            .min_h(px(26.))
            .px(px(26.))
            .pt(px(4.));
        for (i, tag) in prompt.tags.iter().enumerate() {
            let colour = colour::tag(prompts::tag_color(tag));
            let this = this.clone();
            let gone = tag.clone();
            tags = tags.child(
                theme::text(div(), TextKind::Button, &fonts)
                    .h(px(22.))
                    .pl(px(8.))
                    .pr(px(4.))
                    .flex()
                    .items_center()
                    .gap(px(3.))
                    .rounded(px(6.))
                    .bg(colour.opacity(0.14))
                    .text_color(colour)
                    .text_size(px(12.))
                    .child(tag.clone())
                    .child(tag_remove(i, tag, colour, gone, this, window, cx)),
            );
        }
        let tag_field = self.tag.clone();
        // The other prompts' tags that hold what is typed (the field's
        // `datalist`), under the field while it has focus.
        let suggestions = if self.tag.read(cx).focus_handle(cx).is_focused(window) {
            prompts::suggested_tags(
                &prompts::known_tags(library.read(cx).list(), &prompt.tags),
                &self.tag.read(cx).value(),
            )
        } else {
            Vec::new()
        };
        if self.suggestion.is_some_and(|i| i >= suggestions.len()) {
            self.suggestion = None;
        }
        let suggestion_list =
            (!suggestions.is_empty()).then(|| self.suggestion_list(&suggestions, &fonts, cx));
        let shown = suggestions.len();
        tags = tags.child(
            // The text box pads its text 10 px; the tag's text sits 5 px
            // after the last tag (or 26 px in), as `.pe-tags input`'s does.
            div()
                .relative()
                .flex_1()
                .min_w(px(80.))
                .ml(px(-10.))
                .on_key_down(cx.listener(move |this, e: &KeyDownEvent, _, cx| {
                    match e.keystroke.key.as_str() {
                        "backspace" if this.tag.read(cx).value().is_empty() => {
                            this.patch(
                                |p| {
                                    p.tags.pop();
                                },
                                cx,
                            );
                        }
                        "down" | "up" if shown > 0 => {
                            let down = e.keystroke.key == "down";
                            this.suggestion = Some(match (this.suggestion, down) {
                                (None, true) => 0,
                                (None, false) => shown - 1,
                                (Some(i), true) => (i + 1) % shown,
                                (Some(i), false) => (i + shown - 1) % shown,
                            });
                            cx.stop_propagation();
                            cx.notify();
                        }
                        _ => {}
                    }
                }))
                .child(
                    Input::new(&tag_field)
                        .aria_label("Add tag")
                        .appearance(false)
                        .text_size(px(12.)),
                )
                .children(suggestion_list),
        );
        let added = prompt.added_by.as_ref().map(|by| {
            let label = prompts::added_by_label(&by.model);
            theme::text(
                div()
                    .id("prompt-added-by")
                    .role(Role::Note)
                    .aria_label(label.clone())
                    .px(px(26.))
                    .pt(px(2.))
                    .flex()
                    .items_center()
                    .gap(px(5.)),
                TextKind::ToolMeta,
                &fonts,
            )
            .child(icons::icon(Icon::Sparkles, 13.))
            .child(label)
        });
        // The foot: attach, the files, the improve controls.
        let mut files = div().flex().flex_wrap().gap(px(7.));
        for (i, file) in prompt.attachments.iter().enumerate() {
            let this = this.clone();
            let kind = file
                .content_type
                .split_once('/')
                .map_or("file", |(_, k)| k)
                .to_string();
            files = files.child(
                theme::text(div(), TextKind::ToolMeta, &fonts)
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .px(px(8.))
                    .py(px(4.))
                    .rounded(px(8.))
                    .bg(colour::raised())
                    .child(icons::icon(Icon::File, 16.))
                    .child(div().font_weight(FontWeight(600.)).child(file.name.clone()))
                    .child(div().text_color(colour::dim()).child(kind))
                    .child(
                        Button::icon(("prompt-file-remove", i), Icon::X, "Remove attachment")
                            .tip("Remove attachment")
                            .on_press(move |_, cx| {
                                let _ = this.update(cx, |e, cx| {
                                    e.patch(
                                        move |p| {
                                            if i < p.attachments.len() {
                                                p.attachments.remove(i);
                                            }
                                        },
                                        cx,
                                    )
                                });
                            }),
                    ),
            );
        }
        let ready = self.ready(cx);
        let busy = library.read(cx).asked.is_some();
        let blank = prompt.text.trim().is_empty();
        let main_label = if busy {
            "Improving\u{2026}"
        } else if self.from_note {
            "Write with AI"
        } else {
            "Improve with AI"
        };
        let main_disabled = !ready || (!self.from_note && blank);
        let main_this = this.clone();
        let body_text = prompt.text.clone();
        let note_state = self.note_box.clone();
        let main = Button::new("prompt-ai", main_label)
            .with_icon(Icon::Sparkles, 14.)
            .tip(if self.from_note {
                "Writes the prompt from a short description"
            } else {
                "Rewrites the text above"
            })
            .focus(note_state.read(cx).trigger())
            .expanded(self.from_note && note_state.read(cx).is_open())
            .disabled(main_disabled)
            .on_press(move |window, cx| {
                let text = body_text.clone();
                let _ = main_this.update(cx, |e, cx| {
                    if e.from_note {
                        let opened = !e.note_box.read(cx).is_open();
                        e.note_box.update(cx, |n, cx| n.toggle(window, cx));
                        if opened {
                            e.note.update(cx, |n, cx| n.focus(window, cx));
                        }
                        cx.notify();
                    } else {
                        e.send_text(text, cx);
                    }
                });
            });
        let menu_state = self.menu.clone();
        let menu = ai_menu(&menu_state, self.from_note, this.clone(), cx);
        let toggler = menu_state.clone();
        let open = menu_state.read(cx).is_open();
        let chevron = popover::trigger(
            &menu_state,
            Button::icon(
                "prompt-ai-mode",
                Icon::ChevronDown,
                "Choose what the AI works from",
            )
            .expanded(open)
            .on_press(move |window, cx| toggler.update(cx, |s, cx| s.toggle(window, cx))),
            cx,
        );
        let note_panel = {
            let ready_note = ready && !self.note.read(cx).value().trim().is_empty();
            let this = this.clone();
            theme::text(
                div()
                    .w(px(320.))
                    .p(px(10.))
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .rounded(px(10.))
                    .bg(colour::popover())
                    .shadow(raised_panel()),
                TextKind::Button,
                &fonts,
            )
            .child(
                Textarea::new(&self.note)
                    .aria_label("Note for the AI")
                    .text_size(px(13.)),
            )
            .child(
                div().flex().justify_end().child(
                    Button::new("prompt-write", "Write")
                        .kind(Kind::Primary)
                        .disabled(!ready_note)
                        .on_press(move |window, cx| {
                            let _ = this.update(cx, |e, cx| e.send_note(window, cx));
                        }),
                ),
            )
            .into_any_element()
        };
        // `.pe-ai-split`: the main button with the note box over it, the
        // chevron with its menu; both open over the split's right edge
        // (`right: 0; bottom: calc(100% + 8px)`), and Escape or a press
        // outside closes them.
        let split = div()
            .relative()
            .flex()
            .items_center()
            .child(
                div()
                    .relative()
                    .child(main)
                    .child(popover::record_trigger(&note_state, cx)),
            )
            .child(chevron)
            .children(popover::panel(
                &menu_state,
                popover::Placement::AboveRight(popover::Entry::Surface),
                menu,
                cx,
            ))
            .children(popover::panel(
                &note_state,
                popover::Placement::AboveRight(popover::Entry::Surface),
                note_panel,
                cx,
            ));
        let error = library.read(cx).ai_error.clone().map(|e| {
            theme::text(
                div()
                    .id("prompt-ai-error")
                    .role(Role::Alert)
                    .aria_label(e.clone())
                    .max_w(px(420.))
                    .truncate(),
                TextKind::ToolError,
                &fonts,
            )
            .text_size(px(11.))
            .child(e)
        });
        // `Generation reasoning effort`, for a model that reasons and lists
        // efforts.
        let effort = self.facts(cx).and_then(|facts| {
            (facts.reasoning && !facts.efforts.is_empty()).then(|| {
                let this = this.clone();
                let on_change: crate::view::pickers::effort::OnEffort =
                    Rc::new(move |effort, _, cx| {
                        let _ = this.update(cx, |e, cx| e.pick_effort(effort, cx));
                    });
                let chosen = self.ai.as_ref().and_then(|a| a.effort.clone());
                crate::view::pickers::effort::effort_picker_as(
                    "prompt-effort",
                    "Generation reasoning effort",
                    false,
                    &self.effort,
                    &facts,
                    chosen.as_deref(),
                    on_change,
                    cx,
                )
            })
        });
        let attach_this = this.clone();
        let foot = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(9.))
            .px(px(12.))
            .py(px(10.))
            .child(
                Button::icon("prompt-attach", Icon::Plus, "Attach file")
                    .tip("Attach file")
                    .on_press(move |window, cx| {
                        let _ = attach_this.update(cx, |e, cx| e.pick_files(window, cx));
                    }),
            )
            .child(files)
            .child(
                div()
                    .ml_auto()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .justify_end()
                    .gap(px(6.))
                    .children(error)
                    .child(self.model.clone())
                    .children(effort)
                    .child(split),
            );
        div()
            .id("prompt-editor")
            .role(Role::Region)
            .aria_label(format!(
                "Prompt {}",
                prompts::title_or_untitled(&prompt.title)
            ))
            .size_full()
            .flex()
            .justify_center()
            .bg(colour::bedrock())
            .child(
                div()
                    .w(relative(1.))
                    .max_w(px(820.))
                    .pt(px(28.))
                    .flex()
                    .flex_col()
                    .child(
                        // The text box pads its text 12 px: the title starts
                        // 26 px in, as `.pe-title`'s does.
                        theme::text(div().px(px(14.)).pt(px(6.)), TextKind::EmptyHeading, &fonts)
                            .text_size(px(24.))
                            // `620 24px/1.2`: a 29 px line in a box that
                            // holds its descenders.
                            .child(
                                div().debug_selector(|| "prompt-editor-title".into()).child(
                                    Input::new(&self.title)
                                        .aria_label("Title")
                                        .appearance(false)
                                        .text_size(px(24.))
                                        .line_height(px(29.))
                                        // A 44 px box (`Sizable::large`).
                                        .large(),
                                ),
                            ),
                    )
                    .children(added)
                    .child(tags)
                    .child(
                        // The text box pads its text 10 px: 26 in all.
                        div()
                            .flex_1()
                            .min_h_0()
                            .px(px(16.))
                            .pt(px(10.))
                            .pb(px(16.))
                            // A copied message's files, or files and
                            // pictures, are the editor's paste; plain text
                            // goes on to the text box.
                            .capture_action(cx.listener(
                                |this, _: &gpui_kit::component::input::Paste, window, cx| {
                                    if this.paste(window, cx) {
                                        cx.stop_propagation();
                                    }
                                },
                            ))
                            .child(
                                Textarea::new(&self.body)
                                    .aria_label("Prompt text")
                                    .appearance(false)
                                    .text_size(px(14.))
                                    .h_full(),
                            ),
                    )
                    .child(foot),
            )
            .into_any_element()
    }
}

/// The empty library's line, its `/` and `{{style}}` marked as the page's
/// `kbd` and `code`.
fn empty_line() -> StyledText {
    const LINE: &str = "Save build requests you reuse. Type / in a message to insert one, and write {{style}} for parts you fill in each time.";
    let mark = HighlightStyle {
        color: Some(colour::bone()),
        background_color: Some(colour::raised()),
        ..Default::default()
    };
    let at = |part: &str| {
        let start = LINE.find(part).expect("the line holds it");
        start..start + part.len()
    };
    StyledText::new(LINE).with_highlights([
        (at(" / ").start + 1..at(" / ").end - 1, mark),
        (at("{{style}}"), mark),
    ])
}

/// `var(--highlight), var(--shadow-panel)`.
fn raised_panel() -> Vec<BoxShadow> {
    let mut shadows = theme::shadow::highlight();
    shadows.extend(theme::shadow::panel());
    shadows
}

/// `.pe-ai-menu`: `Choose what the AI works from`, two items each with its
/// line under it (`Improve the text`, `Rewrites what is in the editor`;
/// `Write from a note`, `Opens a box for a short description`), the one in
/// use checked.
fn ai_menu(
    state: &Entity<PopoverState>,
    from_note: bool,
    editor: WeakEntity<PromptEditor>,
    cx: &mut App,
) -> AnyElement {
    use crate::view::{Activate, MoveFirst, MoveLast, MoveNext, MovePrevious};
    const ITEMS: [(&str, &str, bool); 2] = [
        ("Improve the text", "Rewrites what is in the editor", false),
        (
            "Write from a note",
            "Opens a box for a short description",
            true,
        ),
    ];
    let fonts = fonts(cx);
    let count = ITEMS.len();
    let mut list = div()
        .id("ai-menu")
        .role(Role::Menu)
        .aria_label("Choose what the AI works from")
        .aria_size_of_set(count)
        .key_context("Menu")
        .w(px(260.))
        .p(px(4.))
        .rounded(px(10.))
        .bg(colour::popover())
        .shadow(raised_panel())
        .flex()
        .flex_col()
        .gap(px(1.));
    for step in [
        crate::state::Step::Previous,
        crate::state::Step::Next,
        crate::state::Step::First,
        crate::state::Step::Last,
    ] {
        let s = state.clone();
        let mover = move |window: &mut Window, cx: &mut App| {
            s.update(cx, |p, cx| p.step(count, step, window, cx))
        };
        list = match step {
            crate::state::Step::Previous => {
                list.on_action(move |_: &MovePrevious, w, cx| mover(w, cx))
            }
            crate::state::Step::Next => list.on_action(move |_: &MoveNext, w, cx| mover(w, cx)),
            crate::state::Step::First => list.on_action(move |_: &MoveFirst, w, cx| mover(w, cx)),
            _ => list.on_action(move |_: &MoveLast, w, cx| mover(w, cx)),
        };
    }
    for (index, (label, hint, note)) in ITEMS.into_iter().enumerate() {
        let selected = note == from_note;
        let focus = state.update(cx, |s, cx| s.item(index, cx));
        let choose: crate::view::Press = {
            let (editor, state) = (editor.clone(), state.clone());
            Rc::new(move |window, cx| {
                let _ = editor.update(cx, |e, cx| e.set_source(note, cx));
                state.update(cx, |s, cx| s.close(window, cx));
            })
        };
        let choose_key = choose.clone();
        list = list.child(
            crate::view::ring(
                div()
                    .id(("ai-menu-item", index))
                    .role(Role::MenuItemRadio)
                    .aria_label(format!("{label} {hint}"))
                    .aria_toggled(if selected {
                        accesskit::Toggled::True
                    } else {
                        accesskit::Toggled::False
                    })
                    .aria_position_in_set(index)
                    .aria_size_of_set(count)
                    .track_focus(&focus)
                    .key_context("Part")
                    .on_click(move |_, window, cx| choose(window, cx))
                    .on_action(move |_: &Activate, window, cx| choose_key(window, cx)),
            )
            .flex()
            .items_center()
            .justify_between()
            .gap(px(10.))
            .px(px(9.))
            .py(px(7.))
            .rounded(px(7.))
            .text_color(colour::bone())
            .cursor_pointer()
            .hover(|s| s.bg(colour::hover()))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(1.))
                    .child(
                        theme::text(div(), TextKind::Button, &fonts)
                            .text_size(px(13.))
                            .font_weight(FontWeight(500.))
                            .child(label),
                    )
                    .child(
                        theme::text(div(), TextKind::Button, &fonts)
                            .text_size(px(11.5))
                            .font_weight(FontWeight(400.))
                            .text_color(colour::dim())
                            .child(hint),
                    ),
            )
            .when(selected, |d| d.child(icons::icon(Icon::Check, 14.))),
        );
    }
    list.into_any_element()
}

/// A tag's remove button (`.pe-tag button`): 16 px, its 10 px x in the tag's
/// colour at 60%, full on hover over a 20% tint of it.
#[allow(clippy::too_many_arguments)]
fn tag_remove(
    index: usize,
    tag: &str,
    colour: Hsla,
    gone: String,
    editor: WeakEntity<PromptEditor>,
    window: &mut Window,
    cx: &mut Context<PromptEditor>,
) -> impl IntoElement {
    let id = ("tag-remove", index);
    let focus = crate::view::controls::focus_for(id, window, cx);
    let press: crate::view::Press = Rc::new(move |_, cx| {
        let gone = gone.clone();
        let _ = editor.update(cx, |e, cx| e.remove_tag(gone, cx));
    });
    let key = press.clone();
    crate::view::ring(
        div()
            .id(id)
            .role(Role::Button)
            .aria_label(format!("Remove tag {tag}"))
            .track_focus(&focus)
            .key_context("Part")
            .on_action(move |_: &crate::view::Activate, window, cx| key(window, cx))
            .on_click(move |_, window, cx| press(window, cx)),
    )
    .flex_none()
    .size(px(16.))
    .rounded(px(4.))
    .flex()
    .items_center()
    .justify_center()
    .cursor_pointer()
    .text_color(colour.opacity(0.6))
    .hover(move |s| s.text_color(colour).bg(colour.opacity(0.2)))
    .child(icons::icon(Icon::X, 10.))
}
