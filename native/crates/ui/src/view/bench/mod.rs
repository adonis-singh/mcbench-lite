//! Bench's places that are not a composer (bench agent; inventory `app.md`
//! 7, `components.md` L, `style-viewer-routes.md` 1.13): a bench's
//! comparison ([`compare`]: per round its prompt and one result row per
//! model, the votes, the round's versions) and the leaderboard
//! ([`leaderboard`]). The comparison is drawn in the chat column above the
//! composer the chat pane keeps for the bench (`view::chat::bench`); the
//! header's round switch, Reveal and Fork are [`header`]'s.
//!
//! What it holds: each bench's details as read (`state::bench_details`,
//! a read on opening a comparison it has none of, the core's pushes after),
//! the round each comparison shows, the leaderboard as last read, the
//! retries and the fork on their way.

pub mod compare;
pub mod header;
pub mod leaderboard;
pub mod library;
pub mod prompts;

use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;

use gpui_kit::*;

use super::shell::announce::Announcer;
use super::shell::lists::Lists;
use crate::state::bench::{self as rules, Halt, Progress, RoundState};
use crate::state::bench_details::{self as details, Details};
use crate::state::place::Place;
use crate::{ActionLog, BenchDetails, Command, Core, Leaderboard, ListedBench, ListedChat, Update};

/// Shows a notice.
pub type Notify = Rc<dyn Fn(String, &mut App)>;
/// Opens a place.
pub type Go = Rc<dyn Fn(Place, &mut Window, &mut App)>;
/// Puts a round into a bench's composer: the bench, the round, its text,
/// its files.
pub type EditRound = Rc<
    dyn Fn(String, String, String, Vec<crate::state::transcript::ShownFile>, &mut Window, &mut App),
>;
/// Opens a file of a round's prompt (a picture in the lightbox).
pub type OpenFile = Rc<dyn Fn(crate::state::transcript::ShownFile, &mut Window, &mut App)>;
/// Says something happened to an id (a bench made; a notice's text).
pub type Opened = Rc<dyn Fn(String, &mut Window, &mut App)>;
/// Keeps the new bench's line-up, Blind and resolution.
pub type KeepBench = Rc<dyn Fn(Vec<crate::BenchChoice>, bool, crate::Resolution, &mut App)>;
/// Keeps the editor's generation model and source.
pub type KeepEditor = Rc<dyn Fn(Option<crate::BenchChoice>, bool, &mut App)>;
/// What [`install`] makes.
pub type Installed = (
    Entity<BenchPane>,
    Entity<library::Library>,
    Entity<prompts::PromptEditor>,
);

/// What the pane needs from the shell.
pub struct BenchOptions {
    /// The core.
    pub core: Arc<dyn Core>,
    /// The lists.
    pub lists: Entity<Lists>,
    /// The live regions.
    pub announcer: Entity<Announcer>,
    /// The action log.
    pub log: Option<ActionLog>,
    /// Opens a place (a member's chat, the Build chat a fork made).
    pub go: crate::view::bench::Go,
    /// Puts a round into the bench's composer as an edit (`editRound`):
    /// the bench, the round's id, its text and files.
    pub edit_round: crate::view::bench::EditRound,
    /// Opens a file of a round's prompt (`renderFiles`).
    pub open_file: crate::view::bench::OpenFile,
    /// Shows a notice.
    pub notice: crate::view::bench::Opened,
}

/// One model on one round, as its row shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct Member {
    /// Its chat.
    pub id: String,
    /// `Build <letter>` while hidden, else its model's name.
    pub name: String,
    /// Its letter while hidden.
    pub letter: Option<char>,
    /// The provider and model (for its logo and title).
    pub provider: String,
    /// The model.
    pub model: String,
    /// The effort shown (the member's, else none).
    pub effort: Option<String>,
    /// What the list says of it.
    pub progress: Progress,
    /// Its turn's start, while running.
    pub running_since: Option<i64>,
}

/// An effort as `.bench-effort` shows it (`text-transform: capitalize`).
pub(crate) fn effort_word(effort: &str) -> String {
    let mut chars = effort.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

/// `.bench-effort`: a model's effort beside its name, 8 px after it, 12 px
/// `--dim-text`.
pub(crate) fn effort_mark(effort: &str) -> Div {
    div()
        .flex_none()
        .ml(px(8.))
        .text_size(px(12.))
        .text_color(crate::view::theme::colour::dim())
        .child(effort_word(effort))
}

/// The bench pane.
pub struct BenchPane {
    pub(crate) options: Rc<BenchOptions>,
    pub(crate) place: Place,
    pub(crate) details: HashMap<String, Details>,
    pub(crate) rounds: HashMap<String, usize>,
    pub(crate) leaderboard: Option<Result<Leaderboard, String>>,
    pub(crate) leaderboard_rows: Vec<FocusHandle>,
    /// Retries on their way (their buttons wait).
    pub(crate) retrying: HashSet<String>,
    /// Stops on their way (their buttons wait, as the page's `pending`).
    pub(crate) stopping: HashSet<String>,
    /// A fork on its way (Fork to Build waits).
    pub(crate) forking: bool,
    /// The vote the window cast last, to say it once it is counted.
    pub(crate) voted: Option<(String, String)>,
    /// What the rows last said of each member (`<bench>/<member>/<round>`),
    /// to announce a change nobody pressed for.
    pub(crate) said: HashMap<String, RoundState>,
    pub(crate) branching: bool,
    pub(crate) scroll: ScrollHandle,
    pub(crate) leaderboard_scroll: ScrollHandle,
    /// The comparison rows' focus stops by (round, row, part): the row
    /// that holds focus is scrolled into view (a browser does so for a
    /// focused element).
    pub(crate) row_focus: HashMap<(usize, usize, u8), FocusHandle>,
    /// Each round's rows' bounds from the last frame.
    pub(crate) row_bounds: Rc<std::cell::RefCell<HashMap<usize, Vec<Bounds<Pixels>>>>>,
    /// The row last scrolled to for focus.
    pub(crate) revealed: Option<(usize, usize)>,
    /// The working dots' clock: a redraw a second while a member runs.
    pub(crate) ticker: Option<Task<()>>,
    pub(crate) renders: usize,
    /// The round the composer edits (`editedId`), marked `editing`.
    pub(crate) edited: Option<String>,
    /// The models whose facts (their default effort) the rows asked for.
    pub(crate) facts_asked: HashSet<(String, String)>,
}

impl BenchPane {
    /// The pane.
    pub fn new(options: BenchOptions, _: &mut Window, cx: &mut Context<Self>) -> Self {
        let observed = cx.observe(&options.lists, |this: &mut Self, _, cx| {
            this.lists_changed(cx);
        });
        observed.detach();
        // A model's facts (its default effort, shown on its rows).
        let catalog = crate::view::pickers::catalog::Catalog::installed(cx);
        cx.observe(&catalog, |_: &mut Self, _, cx| cx.notify())
            .detach();
        BenchPane {
            options: Rc::new(options),
            place: Place::NewBench,
            details: HashMap::new(),
            rounds: HashMap::new(),
            leaderboard: None,
            leaderboard_rows: Vec::new(),
            retrying: HashSet::new(),
            stopping: HashSet::new(),
            forking: false,
            voted: None,
            said: HashMap::new(),
            branching: false,
            scroll: ScrollHandle::new(),
            leaderboard_scroll: ScrollHandle::new(),
            row_focus: HashMap::new(),
            row_bounds: Rc::default(),
            revealed: None,
            ticker: None,
            renders: 0,
            edited: None,
            facts_asked: HashSet::new(),
        }
    }

    pub(crate) fn log(&self, action: &str, fields: serde_json::Value) {
        if let Some(log) = &self.options.log {
            log.record(action, fields);
        }
    }

    /// How many times it rendered (an idle comparison draws nothing).
    pub fn render_count(&self) -> usize {
        self.renders
    }

    /// The place on screen changed: a comparison it has no details of is
    /// read (or read again after a failure, `showBenchView`), the
    /// leaderboard is read on arriving.
    pub fn show(&mut self, place: Place, cx: &mut Context<Self>) {
        match &place {
            Place::Compare { bench } | Place::Member { bench, .. } => {
                let read = match self.details.get(bench) {
                    None => true,
                    Some(d) => d.phase == details::Phase::Failed,
                };
                if read {
                    self.read(bench, cx);
                }
                // start-place agent: a bench not yet listed (a start place
                // opened before the lists) takes its newest round when the
                // lists name it.
                if !self.rounds.contains_key(bench)
                    && let Some(listed) = self.listed(bench, cx)
                {
                    self.rounds.insert(bench.clone(), listed.rounds.max(1));
                }
                // A comparison opens at its newest round (the page's
                // autoscroll pinned to the end).
                if matches!(place, Place::Compare { .. }) && self.place != place {
                    self.scroll.scroll_to_bottom();
                }
            }
            Place::Leaderboard => self.options.core.send(Command::ReadLeaderboard),
            _ => {}
        }
        self.place = place;
        self.sync_ticker(cx);
        cx.notify();
    }

    /// Reads a bench's details (`Read again` too).
    pub fn read(&mut self, bench: &str, cx: &mut Context<Self>) {
        let reading = details::details_reading(self.details.get(bench));
        self.details.insert(bench.to_string(), reading);
        self.options.core.send(Command::ReadBench {
            bench: bench.to_string(),
        });
        cx.notify();
    }

    /// The bench's listed row.
    pub fn listed<'a>(&self, bench: &str, cx: &'a App) -> Option<&'a ListedBench> {
        self.options
            .lists
            .read(cx)
            .benches()
            .iter()
            .find(|b| b.id == bench)
    }

    /// The bench's details as held.
    pub fn details_of(&self, bench: &str) -> Option<&Details> {
        self.details.get(bench)
    }

    /// The round the comparison shows.
    pub fn round_of(&self, bench: &str) -> usize {
        self.rounds.get(bench).copied().unwrap_or(1)
    }

    /// Shows round `round` of `bench` (`showRound`: no history change).
    pub fn show_round(&mut self, bench: &str, round: usize, cx: &mut Context<Self>) {
        self.rounds.insert(bench.to_string(), round);
        self.log(
            "round",
            serde_json::json!({ "bench": bench, "round": round }),
        );
        // pane agent: the 3D pane shows that round's world.
        crate::view::world::round_shown(cx);
        cx.notify();
    }

    /// The bench's members in its order, as their rows show them.
    pub fn members(&self, bench: &ListedBench, cx: &App) -> Vec<Member> {
        let all = self.options.lists.read(cx).all();
        let hidden = rules::hides_names(bench.blind, bench.revealed);
        let mut members: Vec<&ListedChat> = all
            .iter()
            .filter(|c| c.summary.bench_id.as_deref() == Some(bench.id.as_str()))
            .collect();
        members.sort_by_key(|c| {
            bench
                .order
                .iter()
                .position(|m| *m == c.summary.id.0)
                .unwrap_or(usize::MAX)
        });
        members
            .iter()
            .map(|c| {
                let s = &c.summary;
                let (round, halted) = c.progress.unwrap_or((0, None));
                let letter = hidden.then(|| rules::letter(&bench.order, &s.id.0));
                Member {
                    id: s.id.0.clone(),
                    name: crate::state::sidebar::name_of(c, bench, all),
                    letter,
                    provider: s.provider.clone(),
                    model: s.model.clone(),
                    effort: s.effort.clone().filter(|e| !e.is_empty()),
                    progress: Progress {
                        round: round.max(0) as usize,
                        running: c.running_since.is_some(),
                        halted: halted.map(Halt::of),
                    },
                    running_since: c.running_since,
                }
            })
            .collect()
    }

    /// Takes Bench's updates: details, a send's round, a fork's end, the
    /// leaderboard.
    pub fn apply(&mut self, update: Update, window: &mut Window, cx: &mut Context<Self>) {
        match update {
            Update::Bench { bench, details } => {
                let held = self.details.get(&bench);
                // The core pushes a bench's details after any change; one
                // the window never read starts here as read.
                let begun = match held {
                    Some(d) if d.phase == details::Phase::Loading => d.clone(),
                    other => details::details_reading(other),
                };
                let next = match details {
                    Ok(data) => {
                        self.said_votes(&bench, &data, cx);
                        details::details_read(Some(&begun), data)
                    }
                    Err(error) => details::details_failed(Some(&begun), error),
                };
                self.branching = false;
                self.details.insert(bench, next);
            }
            Update::BenchSent { bench, round, .. } => {
                self.rounds.insert(bench, round);
            }
            Update::Forked { member, chat } => {
                self.forking = false;
                match chat {
                    Ok(chat) => {
                        self.log(
                            "forked",
                            serde_json::json!({ "member": member, "chat": chat }),
                        );
                        if matches!(&self.place, Place::Member { member: m, .. } if *m == member) {
                            (self.options.go)(Place::Chat { id: chat }, window, cx);
                        }
                    }
                    Err(error) => (self.options.notice)(error, window, cx),
                }
            }
            // A failed read leaves the entries read before (or none) with
            // a notice (`app.md` 7.7).
            Update::Leaderboard(board) => match board {
                Ok(board) => self.leaderboard = Some(Ok(board)),
                Err(error) => {
                    if self.place == Place::Leaderboard {
                        (self.options.notice)(error, window, cx);
                    }
                }
            },
            other => panic!("the bench pane takes Bench's updates, not {other:?}"),
        }
        self.sync_ticker(cx);
        cx.notify();
    }

    /// Says a vote once it is counted: the pick, the tie, or taken back.
    fn said_votes(&mut self, bench: &str, data: &BenchDetails, cx: &mut Context<Self>) {
        let Some((voted_bench, round)) = self.voted.clone() else {
            return;
        };
        if voted_bench != bench {
            return;
        }
        self.voted = None;
        let text = match rules::shown_vote(data, &round) {
            None => "Vote taken back".to_string(),
            Some(vote) => match &vote.pick {
                None => "Your call for this round: a tie".to_string(),
                Some(member) => {
                    let name = self
                        .listed(bench, cx)
                        .map(|b| crate::state::bench::build_name(rules::letter(&b.order, member)))
                        .unwrap_or_default();
                    format!("Your pick for this round: {name}")
                }
            },
        };
        self.options
            .announcer
            .update(cx, |a, cx| a.polite(text, cx));
    }

    /// The lists changed: a member that finished, failed or was stopped
    /// on the comparison on screen is announced (nobody pressed for it).
    fn lists_changed(&mut self, cx: &mut Context<Self>) {
        // A round on screen past the bench's last (reverted elsewhere) comes
        // back to it (`receiveBenches`).
        let past: Vec<(String, usize)> = self
            .options
            .lists
            .read(cx)
            .benches()
            .iter()
            .filter(|b| self.rounds.get(&b.id).is_some_and(|&r| r > b.rounds.max(1)))
            .map(|b| (b.id.clone(), b.rounds.max(1)))
            .collect();
        for (bench, last) in past {
            self.show_round(&bench, last, cx);
        }
        let Place::Compare { bench } = self.place.clone() else {
            return self.sync_ticker(cx);
        };
        let Some(listed) = self.listed(&bench, cx).cloned() else {
            return;
        };
        let round = self.round_of(&bench);
        let mut spoken = Vec::new();
        for member in self.members(&listed, cx) {
            let key = format!("{bench}/{}/{round}", member.id);
            let now = rules::round_state(member.progress, round);
            let before = self.said.insert(key, now);
            if let Some(before) = before
                && before != now
                && matches!(before, RoundState::Running | RoundState::Queued)
            {
                let what = match now {
                    RoundState::Done => "finished",
                    RoundState::Failed => "failed",
                    RoundState::Stopped => "stopped",
                    _ => continue,
                };
                spoken.push(format!("{} {what} round {round}", member.name));
            }
        }
        if !spoken.is_empty() {
            let text = spoken.join(". ");
            self.options
                .announcer
                .update(cx, |a, cx| a.polite(text, cx));
        }
        self.retrying.retain(|_| false);
        self.stopping.retain(|_| false);
        self.sync_ticker(cx);
        cx.notify();
    }

    /// The comparison's clock runs only while a member on it works.
    fn sync_ticker(&mut self, cx: &mut Context<Self>) {
        let running = match &self.place {
            Place::Compare { bench } => self
                .listed(bench, cx)
                .cloned()
                .is_some_and(|b| self.members(&b, cx).iter().any(|m| m.progress.running)),
            _ => false,
        };
        if !running {
            self.ticker = None;
            return;
        }
        if self.ticker.is_some() {
            return;
        }
        self.ticker = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_secs(1))
                    .await;
                let going = this
                    .update(cx, |this, cx| {
                        let going = this.ticker.is_some();
                        if going {
                            cx.notify();
                        }
                        going
                    })
                    .unwrap_or(false);
                if !going {
                    break;
                }
            }
        }));
    }

    /// Votes on a round (`voteRound`): a member, the tie, or the pick
    /// taken back.
    pub fn vote(
        &mut self,
        bench: &str,
        round: &str,
        pick: crate::VotePick,
        cx: &mut Context<Self>,
    ) {
        self.log(
            "vote",
            serde_json::json!({ "bench": bench, "round": round, "pick": format!("{pick:?}") }),
        );
        self.voted = Some((bench.to_string(), round.to_string()));
        self.options.core.send(Command::Vote {
            bench: bench.to_string(),
            round: round.to_string(),
            pick,
        });
        cx.notify();
    }

    /// Retries a failed or stopped model (`onRetry`).
    pub fn retry(&mut self, bench: &str, member: &str, cx: &mut Context<Self>) {
        self.log("retry", serde_json::json!({ "member": member }));
        self.retrying.insert(member.to_string());
        self.options.core.send(Command::RetryMember {
            bench: bench.to_string(),
            member: member.to_string(),
        });
        cx.notify();
    }

    /// Stops a model (`onStop`).
    pub fn stop(&mut self, member: &str, cx: &mut Context<Self>) {
        self.log("stop", serde_json::json!({ "chat": member }));
        self.stopping.insert(member.to_string());
        self.options.core.send(Command::Stop {
            chat: member.to_string(),
        });
        cx.notify();
    }

    /// Follows another version of a round.
    pub fn branch(&mut self, bench: &str, round: &str, cx: &mut Context<Self>) {
        self.log("round branch", serde_json::json!({ "round": round }));
        self.branching = true;
        self.options.core.send(Command::BenchBranch {
            bench: bench.to_string(),
            round: round.to_string(),
        });
        cx.notify();
    }

    /// Reveals the models (`Reveal for good`).
    pub fn reveal(&mut self, bench: &str) {
        self.log("reveal", serde_json::json!({ "bench": bench }));
        self.options.core.send(Command::Reveal {
            bench: bench.to_string(),
        });
    }

    /// Forks a member's chat to Build (`forkToBuild`): one at a time, from
    /// its last finished round.
    pub fn fork(&mut self, member: &Member, cx: &mut Context<Self>) {
        if self.forking {
            return;
        }
        self.forking = true;
        let round = rules::fork_round(member.progress);
        self.log(
            "fork",
            serde_json::json!({ "member": member.id, "round": round }),
        );
        self.options.core.send(Command::Fork {
            member: member.id.clone(),
            round,
        });
        cx.notify();
    }
}

/// Makes Bench's pane, the library and the prompt editor, and ties them to
/// the shell: the pane opens places and puts a round in the chat pane's
/// composer; the library's list reaches the sidebar whenever it changes.
#[allow(clippy::too_many_arguments)]
pub fn install(
    options: &super::shell::ShellOptions,
    lists: &Entity<Lists>,
    announcer: &Entity<Announcer>,
    chat: &Entity<super::chat::ChatPane>,
    sidebar: &Entity<super::sidebar::Sidebar>,
    shell: WeakEntity<super::shell::Shell>,
    window: &mut Window,
    cx: &mut App,
) -> Installed {
    let notice_shell = shell.clone();
    let notice: crate::view::bench::Notify = Rc::new(move |text, cx| {
        let shell = notice_shell.clone();
        cx.defer(move |cx| {
            let _ = shell.update(cx, |s, cx| s.notice(text, cx));
        });
    });
    let go_shell = shell.clone();
    let chat_for_edit = chat.downgrade();
    let chat_for_file = chat.downgrade();
    let notice_for_pane = notice.clone();
    let bench = cx.new(|cx| {
        BenchPane::new(
            BenchOptions {
                core: options.core.clone(),
                lists: lists.clone(),
                announcer: announcer.clone(),
                log: options.log.clone(),
                go: Rc::new(move |place, window, cx| {
                    let shell = go_shell.clone();
                    window.defer(cx, move |window, cx| {
                        let _ = shell.update(cx, |s, cx| s.go(place, window, cx));
                    });
                }),
                edit_round: Rc::new(move |_, round, text, files, window, cx| {
                    let _ = chat_for_edit
                        .update(cx, |c, cx| c.edit_round(round, text, files, window, cx));
                }),
                open_file: Rc::new(move |file, window, cx| {
                    let _ = chat_for_file.update(cx, |c, cx| c.open_round_file(&file, window, cx));
                }),
                notice: Rc::new(move |text, _, cx| notice_for_pane(text, cx)),
            },
            window,
            cx,
        )
    });
    // The round the composer edits is marked (`editedId`): the pane is
    // drawn again only when that changes.
    let marked = bench.downgrade();
    cx.observe(chat, move |chat, cx| {
        let now = chat.read(cx).draft.edit.as_ref().map(|e| e.of.clone());
        let _ = marked.update(cx, |b, cx| {
            if b.edited != now {
                b.edited = now;
                cx.notify();
            }
        });
    })
    .detach();
    let library = cx.new(|_| library::Library::new(options.core.clone(), notice.clone()));
    library::Library::install(&library, cx);
    let sidebar = sidebar.downgrade();
    cx.observe(&library, move |library, cx| {
        let l = library.read(cx);
        let shown = match l.phase() {
            crate::state::prompt_sync::Phase::Failed => {
                Some(Err(l.error().unwrap_or_default().to_string()))
            }
            _ if l.loaded() => Some(Ok(l.list().to_vec())),
            _ => None,
        };
        if let Some(shown) = shown {
            let _ = sidebar.update(cx, |s, cx| s.prompts_read(shown, cx));
        }
    })
    .detach();
    let keep_shell = shell.clone();
    let editor = cx.new(|cx| {
        prompts::PromptEditor::new(
            library.clone(),
            options.core.clone(),
            &options.preferences,
            options.picker.clone(),
            Rc::new(move |ai, note, cx| {
                let shell = keep_shell.clone();
                cx.defer(move |cx| {
                    let _ = shell.update(cx, |s, _| s.keep_editor(ai, note));
                });
            }),
            notice,
            options.log.clone(),
            window,
            cx,
        )
    });
    (bench, library, editor)
}

impl Render for BenchPane {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.renders += 1;
        match self.place.clone() {
            Place::Compare { bench } => self.render_compare(&bench, window, cx),
            Place::Leaderboard => self.render_leaderboard(window, cx),
            _ => div().into_any_element(),
        }
    }
}
