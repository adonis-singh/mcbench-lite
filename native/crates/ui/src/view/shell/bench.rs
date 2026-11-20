//! What Bench's places ask of the shell (bench agent): arriving at the new
//! bench, a comparison, a member's chat, the leaderboard and the prompt
//! library; Bench's updates; the new bench's line-up kept with the
//! preferences; `Build` from the new bench and `Run bench` from a prompt.

use gpui_kit::*;

use super::Shell;
use crate::state::place::{Mode, Place};
use crate::view::bench::BenchPane;
use crate::view::bench::library::Library;
use crate::view::chat::bench::Surface;
use crate::{BenchChoice, Resolution, Update};

impl Shell {
    /// Bench's pane.
    pub fn bench_pane(&self) -> Entity<BenchPane> {
        self.bench_pane.clone()
    }

    /// The prompt library.
    pub fn library(&self) -> Entity<Library> {
        self.library.clone()
    }

    /// The prompt editor.
    pub fn prompt_editor(&self) -> Entity<crate::view::bench::prompts::PromptEditor> {
        self.prompt_editor.clone()
    }

    /// Keeps the editor's generation model and source with the
    /// preferences (`mcb-prompt-ai-model`, `mcb-improve-source`).
    pub(crate) fn keep_editor(&mut self, ai: Option<BenchChoice>, note: bool) {
        let p = &mut self.preferences;
        if p.prompt_ai_model == ai && p.improve_source_note == note {
            return;
        }
        p.prompt_ai_model = ai;
        p.improve_source_note = note;
        self.save();
    }

    /// The lists.
    pub fn lists(&self) -> &Entity<super::lists::Lists> {
        &self.lists
    }

    /// Arrives at a Bench place: the chat pane shows the new bench, the
    /// comparison or a member's chat; the bench pane reads what it needs.
    pub(super) fn arrive_bench(
        &mut self,
        place: Place,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.bench_pane
            .update(cx, |b, cx| b.show(place.clone(), cx));
        match &place {
            Place::NewBench => {
                self.chat.update(cx, |c, cx| {
                    c.show_bench(Surface::NewBench, window, cx);
                    c.focus_composer(window, cx);
                });
            }
            Place::Compare { bench } => {
                let surface = Surface::Compare(bench.clone());
                let body = self.bench_pane.clone().into();
                self.chat.update(cx, |c, cx| {
                    c.set_bench_body(body);
                    if *c.surface() != surface {
                        c.show_bench(surface, window, cx);
                    }
                });
            }
            Place::Member { member, .. } => {
                let member = member.clone();
                if self.chat.read(cx).open_chat() != Some(member.as_str())
                    || *self.chat.read(cx).surface() != Surface::Chat
                {
                    self.chat
                        .update(cx, |c, cx| c.show_chat(Some(member), window, cx));
                }
            }
            // The sidebar may be the one navigating: its row's prompt is
            // read after this update.
            Place::Prompt => {
                let (sidebar, library) = (self.sidebar.clone(), self.library.clone());
                cx.defer_in(window, move |_, _, cx| {
                    let active = sidebar.read(cx).prompts.active.clone();
                    if let Some(id) = active {
                        library.update(cx, |l, cx| l.open(id, cx));
                    }
                });
            }
            _ => {}
        }
    }

    /// Bench's and the library's updates.
    pub(super) fn apply_bench(
        &mut self,
        update: Update,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match update {
            Update::BenchSent {
                target,
                bench,
                round,
            } => {
                self.bench_pane.update(cx, |b, cx| {
                    b.apply(
                        Update::BenchSent {
                            target: target.clone(),
                            bench: bench.clone(),
                            round,
                        },
                        window,
                        cx,
                    )
                });
                self.chat
                    .update(cx, |c, cx| c.bench_sent(&target, &bench, window, cx));
            }
            Update::PromptsSaved(result) => {
                self.library.update(cx, |l, cx| l.saved(result, cx));
            }
            Update::Generated { request, outcome } => {
                self.library
                    .update(cx, |l, cx| l.generated(request, outcome, cx));
            }
            other => self
                .bench_pane
                .update(cx, |b, cx| b.apply(other, window, cx)),
        }
    }

    /// Keeps the new bench's line-up, Blind and resolution with the
    /// preferences (`mcb-bench-models`, `mcb-bench-blind`,
    /// `mcb-bench-resolution`).
    pub(crate) fn keep_bench(
        &mut self,
        line: Vec<BenchChoice>,
        blind: bool,
        resolution: Resolution,
    ) {
        let p = &mut self.preferences;
        if p.bench_models == line && p.bench_blind == blind && p.bench_resolution == resolution {
            return;
        }
        p.bench_models = line;
        p.bench_blind = blind;
        p.bench_resolution = resolution;
        self.save();
    }

    /// `Compare several in Bench` and the empty chat's `Bench` link
    /// (`benchFromBuild`): the new bench, with Build's draft (text and
    /// files) when the new bench's is empty; the composer takes focus.
    pub(crate) fn bench_from_build(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.chat.update(cx, |c, cx| {
            c.move_draft_to(crate::state::place::NEW_BENCH_DRAFT, window, cx)
        });
        self.go(Place::NewBench, window, cx);
        self.chat.update(cx, |c, cx| c.focus_composer(window, cx));
    }

    /// `Or have one model build it in`'s `Build` (`buildFromBench`): the new
    /// chat, with the new bench's draft when the new chat's is empty.
    pub(crate) fn build_from_bench(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.chat.update(cx, |c, cx| {
            c.move_draft_to(crate::state::place::NEW_CHAT_DRAFT, window, cx)
        });
        let _ = Mode::Build;
        self.go(Place::NewChat { id: None }, window, cx);
    }

    /// Opens a prompt in the library (`openPrompt`): the sidebar's row and
    /// the editor follow.
    pub(crate) fn open_prompt(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        self.sidebar.update(cx, |s, cx| {
            s.prompts.active = Some(id.clone());
            cx.notify();
        });
        self.library.update(cx, |l, cx| l.open(id, cx));
        self.go(Place::Prompt, window, cx);
    }

    /// `Run bench` (`benchPrompt`): the new bench with the prompt in its
    /// composer, its `usedAt` now.
    pub fn bench_prompt(
        &mut self,
        prompt: crate::LibraryPrompt,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.library.update(cx, |l, cx| l.used(&prompt.id, cx));
        self.go(Place::NewBench, window, cx);
        self.chat
            .update(cx, |c, cx| c.put_prompt(&prompt, true, window, cx));
    }
}
