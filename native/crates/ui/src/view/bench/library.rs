//! The prompt library, held once for the window (bench agent; inventory
//! `app.md` 8.4 and 8.7): the sync with the core (`state::prompt_sync`: a
//! read is `Command::ReadPrompts`, a save `Command::SavePrompts` 300 ms
//! after the last edit, one at a time, a library the core publishes unasked
//! read again with the unsent edits made on it), the prompt open in the
//! editor and a new one not saved until something is written in it, and
//! the editor's improve and write requests. The sidebar's list, the
//! palette over the composer and the editor read and change it here.

use std::rc::Rc;
use std::sync::Arc;

use gpui_kit::*;

use crate::state::prompt_sync::{self, Change, Effect, Phase, PromptSync};
use crate::state::prompts;
use crate::{Command, Core, Generated, LibraryPrompt, PromptAttachment};

/// The sentence when the text changed while the model wrote.
pub const CHANGED_MEANWHILE: &str = "The prompt changed while the model was writing. Try again.";

/// An improve or write on its way: its number, the prompt, the text the
/// prompt held when it was sent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Asked {
    /// Its request number.
    pub request: u64,
    /// The prompt it rewrites.
    pub prompt: String,
    /// The prompt's text when it was sent.
    pub text: String,
}

struct Installed(Entity<Library>);

impl Global for Installed {}

/// The library.
pub struct Library {
    core: Arc<dyn Core>,
    notice: crate::view::bench::Notify,
    sync: PromptSync<LibraryPrompt>,
    /// `promptsActiveId`.
    active: Option<String>,
    /// `newPrompt`: made with New prompt, not in the library until written in.
    new_prompt: Option<LibraryPrompt>,
    /// The improve or write on its way.
    pub(crate) asked: Option<Asked>,
    /// The last one's failure, under the editor.
    pub(crate) ai_error: Option<String>,
    requests: u64,
    /// The prompt whose delete asks for its second press, and since when
    /// (`PromptList`'s 3 s arm).
    armed: Option<(String, std::time::Instant)>,
}

impl Library {
    /// The library, unread; `notice` shows a failure as a notice.
    pub fn new(core: Arc<dyn Core>, notice: crate::view::bench::Notify) -> Self {
        Library {
            core,
            notice,
            sync: PromptSync::new(),
            active: None,
            new_prompt: None,
            asked: None,
            ai_error: None,
            requests: 0,
            armed: None,
        }
    }

    /// Makes it the window's library.
    pub fn install(library: &Entity<Library>, cx: &mut App) {
        cx.set_global(Installed(library.clone()));
    }

    /// The window's library, once the shell made it.
    pub fn installed(cx: &App) -> Option<Entity<Library>> {
        cx.try_global::<Installed>().map(|i| i.0.clone())
    }

    /// The phase.
    pub fn phase(&self) -> Phase {
        self.sync.phase()
    }

    /// The library as shown.
    pub fn list(&self) -> &[LibraryPrompt] {
        self.sync.list()
    }

    /// Why the first read failed.
    pub fn error(&self) -> Option<&str> {
        self.sync.error()
    }

    /// Loaded: it can be edited.
    pub fn loaded(&self) -> bool {
        prompt_sync::prompts_loaded(self.sync.phase())
    }

    /// The first read, or one after a failure (`Try again`).
    pub fn load(&mut self, cx: &mut Context<Self>) {
        if matches!(self.sync.phase(), Phase::Unloaded | Phase::Failed) {
            self.sync.load();
            self.run(cx);
        }
    }

    /// The core published the library: a read's answer, or (none on its
    /// way) the library changed by a model's `prompt_library` call.
    pub fn read(&mut self, library: Result<Vec<LibraryPrompt>, String>, cx: &mut Context<Self>) {
        // The sidebar asks for the first read, and again after a failure
        // (`Try again`): its answer is the load's.
        if matches!(self.sync.phase(), Phase::Unloaded | Phase::Failed) {
            self.sync.load();
            self.sync
                .take_effects()
                .retain(|e| !matches!(e, Effect::Read));
        }
        if self.sync.reading() {
            self.sync.read_landed(library);
        } else {
            self.sync.reload();
        }
        self.run(cx);
    }

    /// A save's end.
    pub fn saved(&mut self, result: Result<Vec<LibraryPrompt>, String>, cx: &mut Context<Self>) {
        self.sync.write_landed(result.map(drop));
        self.run(cx);
    }

    fn run(&mut self, cx: &mut Context<Self>) {
        for effect in self.sync.take_effects() {
            match effect {
                Effect::Changed => cx.notify(),
                Effect::Read => self.core.send(Command::ReadPrompts),
                Effect::Write(list) => self.core.send(Command::SavePrompts(list)),
                Effect::Wait(timer) => {
                    cx.spawn(async move |this, cx| {
                        cx.background_executor().timer(prompt_sync::DELAY).await;
                        let _ = this.update(cx, |this, cx| {
                            this.sync.timer_fired(timer);
                            this.run(cx);
                        });
                    })
                    .detach();
                }
                Effect::Error(error) => (self.notice)(error, cx),
            }
        }
    }

    /// A change to the library; before it loaded, a notice says why not.
    pub fn edit(&mut self, change: Change<LibraryPrompt>, cx: &mut Context<Self>) -> bool {
        match self.sync.edit(change) {
            Ok(()) => {
                self.run(cx);
                true
            }
            Err(error) => {
                (self.notice)(error, cx);
                false
            }
        }
    }

    /// The prompts the editor offers: the library and the new one.
    pub fn editor_prompts(&self) -> Vec<LibraryPrompt> {
        let mut list = self.sync.list().to_vec();
        list.extend(self.new_prompt.clone());
        list
    }

    /// The prompt the editor shows (`shownPromptId`): the active one if it
    /// is there, else the first, else none.
    pub fn shown(&self) -> Option<LibraryPrompt> {
        let list = self.editor_prompts();
        self.active
            .as_ref()
            .and_then(|id| list.iter().find(|p| p.id == *id).cloned())
            .or_else(|| list.first().cloned())
    }

    /// The shown prompt's title for the breadcrumb.
    pub fn shown_title(&self) -> Option<String> {
        self.shown()
            .map(|p| prompts::title_or_untitled(&p.title).to_string())
    }

    /// The open prompt's id.
    pub fn active(&self) -> Option<&str> {
        self.active.as_deref()
    }

    /// Opens a prompt (`openPrompt`): a new one left empty is let go.
    pub fn open(&mut self, id: String, cx: &mut Context<Self>) {
        if self.new_prompt.as_ref().is_some_and(|p| p.id != id) {
            self.new_prompt = None;
        }
        if self.active.as_deref() != Some(&id) {
            self.cancel_ask(cx);
            self.ai_error = None;
        }
        self.active = Some(id);
        cx.notify();
    }

    /// A new prompt's id: `crypto.randomUUID()`, as the page stores it.
    fn next_id(&mut self) -> String {
        mcbench_store::Host::uuid(&mcbench_store::SystemHost)
    }

    /// New prompt (`createPrompt`): held, and not in the library until
    /// something is written in it.
    pub fn create(&mut self, cx: &mut Context<Self>) -> String {
        let id = self.next_id();
        self.new_prompt = Some(prompts::blank(id.clone()));
        self.open(id.clone(), cx);
        id
    }

    /// Changes a prompt (`updatePrompt`): the new one's first edit adds it
    /// to the library; a prompt gone from the list is left be.
    pub fn update(
        &mut self,
        id: &str,
        patch: impl Fn(&mut LibraryPrompt) + 'static,
        cx: &mut Context<Self>,
    ) {
        if let Some(mut fresh) = self.new_prompt.clone().filter(|p| p.id == id) {
            patch(&mut fresh);
            if self.edit(
                Rc::new(move |list: &[LibraryPrompt]| {
                    let mut list = list.to_vec();
                    list.push(fresh.clone());
                    list
                }),
                cx,
            ) {
                self.new_prompt = None;
            }
            return;
        }
        let id = id.to_string();
        self.edit(
            Rc::new(move |list: &[LibraryPrompt]| {
                list.iter()
                    .cloned()
                    .map(|mut p| {
                        if p.id == id {
                            patch(&mut p);
                        }
                        p
                    })
                    .collect()
            }),
            cx,
        );
    }

    /// Flips a favourite.
    pub fn toggle_favorite(&mut self, id: &str, cx: &mut Context<Self>) {
        self.update(id, |p| p.favorite = !p.favorite, cx);
    }

    /// Deletes a prompt; the open one moves to the library's first.
    pub fn delete(&mut self, id: &str, cx: &mut Context<Self>) {
        let gone = id.to_string();
        if self.new_prompt.as_ref().is_some_and(|p| p.id == gone) {
            self.new_prompt = None;
        } else {
            self.edit(
                Rc::new(move |list: &[LibraryPrompt]| {
                    list.iter().filter(|p| p.id != gone).cloned().collect()
                }),
                cx,
            );
        }
        if self.active.as_deref() == Some(id) {
            self.active = self.sync.list().first().map(|p| p.id.clone());
        }
        cx.notify();
    }

    /// Whether `id`'s delete is armed (pressed once in the last 3 s).
    pub fn delete_armed(&self, id: &str) -> bool {
        self.armed
            .as_ref()
            .is_some_and(|(a, at)| a == id && at.elapsed() < std::time::Duration::from_secs(3))
    }

    /// A press on a row's delete: the first arms it (one prompt at a time),
    /// a second within 3 s deletes; it disarms by itself after 3 s.
    pub fn press_delete(&mut self, id: &str, cx: &mut Context<Self>) {
        if self.delete_armed(id) {
            self.armed = None;
            return self.delete(id, cx);
        }
        self.armed = Some((id.to_string(), std::time::Instant::now()));
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_secs(3))
                .await;
            let _ = this.update(cx, |_, cx| cx.notify());
        })
        .detach();
    }

    /// Moves a prompt (`reorderPrompts`).
    pub fn reorder(&mut self, from: usize, to: usize, cx: &mut Context<Self>) {
        self.edit(
            Rc::new(move |list: &[LibraryPrompt]| prompts::reorder(list, from, to)),
            cx,
        );
    }

    /// A prompt was inserted or benched: its `usedAt` is now.
    pub fn used(&mut self, id: &str, cx: &mut Context<Self>) {
        let now = crate::view::chat::now_ms();
        self.update(id, move |p| p.used_at = Some(now), cx);
    }

    /// `Save message as prompt`: a new prompt from the composer, titled by
    /// its first line, opened.
    pub fn save_draft(
        &mut self,
        text: &str,
        attachments: Vec<PromptAttachment>,
        cx: &mut Context<Self>,
    ) -> Option<String> {
        let id = self.next_id();
        let entry = LibraryPrompt {
            title: prompts::draft_title(text),
            text: text.trim().to_string(),
            attachments,
            ..prompts::blank(id.clone())
        };
        if !self.edit(
            Rc::new(move |list: &[LibraryPrompt]| {
                let mut list = list.to_vec();
                list.push(entry.clone());
                list
            }),
            cx,
        ) {
            return None;
        }
        self.open(id.clone(), cx);
        Some(id)
    }

    /// Improves the shown prompt's text, or writes it from `text` (the
    /// note): one request at a time, a new one cancelling the old.
    pub fn ask(
        &mut self,
        provider: String,
        model: String,
        effort: Option<String>,
        text: String,
        cx: &mut Context<Self>,
    ) {
        let Some(prompt) = self.shown() else {
            return;
        };
        self.cancel_ask(cx);
        self.requests += 1;
        let request = self.requests;
        self.asked = Some(Asked {
            request,
            prompt: prompt.id.clone(),
            text: prompt.text.clone(),
        });
        self.ai_error = None;
        self.core.send(Command::GeneratePrompt {
            request,
            provider,
            model,
            effort,
            text,
        });
        cx.notify();
    }

    /// Cancels the request on its way (a prompt switch, a new request).
    pub fn cancel_ask(&mut self, cx: &mut Context<Self>) {
        if let Some(asked) = self.asked.take() {
            self.core.send(Command::CancelGenerate {
                request: asked.request,
            });
            cx.notify();
        }
    }

    /// A request's end: the text replaces the body if the prompt and its
    /// text are as they were when it was sent; a failure stays under the
    /// editor; a cancel ends it quietly. Answers whether the text landed.
    pub fn generated(&mut self, request: u64, outcome: Generated, cx: &mut Context<Self>) -> bool {
        let Some(asked) = self.asked.clone().filter(|a| a.request == request) else {
            return false;
        };
        self.asked = None;
        let mut landed = false;
        match outcome {
            Generated::Text(text) => {
                let same = self
                    .shown()
                    .is_some_and(|p| p.id == asked.prompt && p.text == asked.text);
                if same {
                    self.update(&asked.prompt, move |p| p.text = text.clone(), cx);
                    landed = true;
                } else {
                    self.ai_error = Some(CHANGED_MEANWHILE.into());
                }
            }
            Generated::Failed(error) => self.ai_error = Some(error),
            Generated::Cancelled => {}
        }
        cx.notify();
        landed
    }
}
