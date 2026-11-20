//! The stand-in's benches, votes, leaderboard, library saves and prompt
//! generation (bench agent): the commands of `ui-seam-bench-prompts.md`
//! answered from the fixture's benches, the seam's two rules kept and
//! stage three's answers followed (`Update::Bench` pushed for the benches
//! asked for since attach; a round refused for an effort, or a resolution
//! other than its bench's, naming the field). Every failure plays on
//! demand ([`BenchPlay`], the dev binary's `--bench`).
//!
//! A bench's models "build" on a clock: a member given a round runs for
//! [`StandIn`]'s rate times [`MEMBER_STEPS`] (at once under test), then
//! finishes it, takes its next queued round, and the bench's details are
//! published again.

use std::collections::HashSet;
use std::time::Duration;

use serde_json::json;

use std::sync::{Arc, Mutex};

use super::{BenchSeat, ChatData, ChatRow, Data, Halted, StandIn, lists_of, now_ms};
use crate::{
    BenchChoice, BenchDetails, BenchRound, Command, Content, Generated, LeaderboardEntry,
    ListedBench, Outgoing, Part, PromptAddedBy, PromptAttachment, PromptLink, Resolution,
    RoundBranch, RoundStats, Target, Update, Vote, VotePick, Window,
};

/// A member's turn lasts this many of the stand-in's rate (60 ms in the dev
/// binary: 1.5 s).
pub const MEMBER_STEPS: u32 = 25;

/// How a bench or library command plays (`--bench <name>`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BenchPlay {
    /// `create-refused`: a new bench is refused.
    CreateRefused,
    /// `round-refused`: a bench's next round is refused.
    RoundRefused,
    /// `details-fail`: every details read fails.
    DetailsFail,
    /// `vote-fails`: a vote is refused.
    VoteFails,
    /// `reveal-fails`.
    RevealFails,
    /// `branch-fails`.
    BranchFails,
    /// `retry-fails`.
    RetryFails,
    /// `fork-fails`.
    ForkFails,
    /// `leaderboard-empty`: no blind vote yet.
    LeaderboardEmpty,
    /// `leaderboard-fails`.
    LeaderboardFails,
    /// `prompts-fail`: the library's first read fails.
    PromptsFail,
    /// `save-fails`: every library save fails.
    SaveFails,
    /// `generate-fails`.
    GenerateFails,
    /// `generate-slow`: an improve takes 4 s (to cancel it).
    GenerateSlow,
    /// `model-edits-library`: 2 s after attach a model adds a prompt
    /// (`Update::Prompts` unasked).
    ModelEditsLibrary,
    /// `members-wait`: members never finish by themselves.
    MembersWait,
    // par-bench: a new bench's answer that waits, so a person can move
    // before it comes.
    /// `create-held`: a new bench is made only at
    /// [`StandIn::release_held_bench`].
    CreateHeld,
}

impl BenchPlay {
    /// The play of `--bench <name>`.
    pub fn named(name: &str) -> BenchPlay {
        match name {
            "create-refused" => BenchPlay::CreateRefused,
            "round-refused" => BenchPlay::RoundRefused,
            "details-fail" => BenchPlay::DetailsFail,
            "vote-fails" => BenchPlay::VoteFails,
            "reveal-fails" => BenchPlay::RevealFails,
            "branch-fails" => BenchPlay::BranchFails,
            "retry-fails" => BenchPlay::RetryFails,
            "fork-fails" => BenchPlay::ForkFails,
            "leaderboard-empty" => BenchPlay::LeaderboardEmpty,
            "leaderboard-fails" => BenchPlay::LeaderboardFails,
            "prompts-fail" => BenchPlay::PromptsFail,
            "save-fails" => BenchPlay::SaveFails,
            "generate-fails" => BenchPlay::GenerateFails,
            "generate-slow" => BenchPlay::GenerateSlow,
            "model-edits-library" => BenchPlay::ModelEditsLibrary,
            "members-wait" => BenchPlay::MembersWait,
            "create-held" => BenchPlay::CreateHeld,
            other => panic!(
                "--bench takes create-refused, round-refused, details-fail, vote-fails, reveal-fails, branch-fails, retry-fails, fork-fails, leaderboard-empty, leaderboard-fails, prompts-fail, save-fails, generate-fails, generate-slow, model-edits-library, members-wait or create-held, not {other}"
            ),
        }
    }
}

/// `create-refused`'s sentence.
pub const CREATE_REFUSED: &str = "The bench could not be made: the store is full (a fixture error)";
/// `round-refused`'s sentence.
pub const ROUND_REFUSED: &str = "Wait for the round edit to finish";
/// `details-fail`'s sentence.
pub const DETAILS_FAILED: &str = "The bench's rounds could not be read (a fixture error)";
/// `vote-fails`'s sentence.
pub const VOTE_REFUSED: &str = "Vote once every model has finished this round";
/// `reveal-fails`'s sentence.
pub const REVEAL_FAILED: &str = "The bench could not be revealed (a fixture error)";
/// `branch-fails`'s sentence.
pub const BRANCH_REFUSED: &str = "Stop the running models before switching branches";
/// The refusal of a retry (`retry-fails`, or a model that is not halted).
pub const RETRY_REFUSED: &str = "Only a failed or stopped model can be retried";
/// `fork-fails`'s sentence.
pub const FORK_FAILED: &str = "The build could not be copied (a fixture error)";
/// The refusal of a fork of a round still running.
pub const FORK_WAIT: &str = "Wait for this round to finish before forking it";
/// `leaderboard-fails`'s sentence.
pub const LEADERBOARD_FAILED: &str = "The leaderboard could not be read (a fixture error)";
/// `prompts-fail`'s sentence.
pub const PROMPTS_FAILED: &str = "The prompt library could not be read (a fixture error)";
/// `save-fails`'s sentence.
pub const SAVE_FAILED: &str =
    "The prompt library was not saved: the disk is full (a fixture error)";
/// `generate-fails`'s sentence.
pub const GENERATE_FAILED: &str = "The model returned an empty prompt";
/// The prompt a model adds (`model-edits-library`).
pub const ADDED_TITLE: &str = "Sandstone pyramid";

/// A round sent to a bench, on any branch.
#[derive(Clone, Debug)]
struct Node {
    id: String,
    parent: Option<String>,
    text: String,
    prompts: Vec<PromptLink>,
    // par-bench: the round's files, stored as a sent message's are.
    files: Vec<mcbench_store::Attachment>,
}

/// A bench's rounds and votes.
#[derive(Clone, Debug)]
struct Record {
    nodes: Vec<Node>,
    head: String,
    votes: Vec<(String, Vote)>,
    resolution: Resolution,
}

impl Record {
    /// The active line, first first.
    fn line(&self) -> Vec<&Node> {
        let mut line = Vec::new();
        let mut at = Some(self.head.clone());
        while let Some(id) = at {
            let node = self
                .nodes
                .iter()
                .find(|n| n.id == id)
                .unwrap_or_else(|| panic!("no round {id}"));
            at = node.parent.clone();
            line.push(node);
        }
        line.reverse();
        line
    }
}

/// The stand-in's bench state.
#[derive(Default)]
pub(super) struct State {
    records: Vec<(String, Record)>,
    built: bool,
    asked: HashSet<String>,
    plays: Vec<BenchPlay>,
    prompt_reads: usize,
    cancelled: HashSet<u64>,
    finished: HashSet<u64>,
    created: usize,
    votes_cast: u64,
    /// Every vote was cleared: the ranking holds only votes cast since.
    cleared: bool,
    /// The details last published of each watched bench.
    last: std::collections::HashMap<String, BenchDetails>,
    // par-bench: the new bench `create-held` holds.
    held: Option<Outgoing>,
}

/// What the bench commands work on, cloneable into the threads that play
/// a member's turn and a slow generation.
#[derive(Clone)]
pub(super) struct Parts {
    data: Arc<Mutex<Data>>,
    bench: Arc<Mutex<State>>,
    window: Arc<dyn Window>,
    rate: Duration,
    /// Settings' vote count, read when the parts were taken.
    settings_votes: Result<u64, String>,
}

impl StandIn {
    fn parts(&self) -> Parts {
        Parts {
            data: self.data.clone(),
            bench: self.bench.clone(),
            window: self.window(),
            rate: self.rate,
            settings_votes: self.settings.lock().expect("lock").votes.clone(),
        }
    }

    /// Plays `play` from now on (before attach too).
    pub fn bench_play(&self, play: BenchPlay) {
        self.bench.lock().expect("lock").plays.push(play);
    }

    /// `--fixture bench`'s library, votes and leaderboard (`Parts::fixture`).
    pub fn bench_fixture(&self) {
        Parts::fixture(&self.data, &self.bench);
    }

    /// Many prompts for the speed runs (`Parts::many_prompts`).
    pub fn many_prompts(&self, count: usize, chars: usize) {
        Parts::many_prompts(&self.data, count, chars);
    }

    /// A model's edit of the library, published unasked.
    pub fn model_edits_library(&self, prompt: crate::LibraryPrompt) {
        self.parts().model_edits_library(prompt);
    }

    /// The library as the stand-in keeps it (what the window saved).
    pub fn library_for_test(&self) -> Vec<crate::LibraryPrompt> {
        self.data.lock().expect("lock").library.clone()
    }

    // par-bench: the held new bench (`create-held`).
    /// Makes the new bench held by `create-held`, and answers its send.
    pub fn release_held_bench(&self) {
        let held = {
            let mut state = self.bench.lock().expect("lock");
            state.plays.retain(|p| *p != BenchPlay::CreateHeld);
            state.held.take().expect("a new bench was held")
        };
        self.parts().send(held);
    }

    /// Every running member of `bench` finishes its round now.
    pub fn finish_members(&self, bench: &str) {
        self.parts().finish_members(bench);
    }

    pub(super) fn prompts_read_fails(&self) -> bool {
        let mut state = self.bench.lock().expect("lock");
        state.prompt_reads += 1;
        state.plays.contains(&BenchPlay::PromptsFail) && state.prompt_reads == 1
    }

    pub(super) fn bench_attached(&self) {
        self.parts().attached();
    }

    pub(super) fn bench_stop(&self, chat: &str) -> bool {
        self.parts().stop(chat)
    }

    pub(super) fn bench_send(&self, outgoing: Outgoing) {
        self.parts().send(outgoing);
    }

    pub(super) fn bench_command(&self, command: Command) {
        self.parts().command(command);
    }

    /// The leaderboard now, for Settings' read too.
    pub(super) fn leaderboard_now(&self) -> Result<crate::Leaderboard, String> {
        self.parts().leaderboard_now()
    }

    /// After the stand-in's own chat list: every watched bench whose details
    /// changed (the core's push after each list).
    pub(super) fn bench_lists_published(&self) {
        if self.bench.lock().expect("lock").built {
            self.parts().push_changed();
        }
    }

    /// A bench deleted for good leaves the watched set.
    pub(super) fn bench_deleted(&self, bench: &str) {
        let mut state = self.bench.lock().expect("lock");
        state.asked.remove(bench);
        state.last.remove(bench);
    }

    /// Every vote was cleared: the benches' votes are emptied.
    pub(super) fn votes_cleared(&self) {
        let mut state = self.bench.lock().expect("lock");
        for (_, record) in state.records.iter_mut() {
            record.votes.clear();
        }
        state.votes_cast = 0;
        state.cleared = true;
    }

    /// After the cleared leaderboard: `Update::Bench` for every asked bench.
    pub(super) fn push_cleared(&self) {
        self.parts().votes_cleared();
    }
}

impl Parts {
    fn window(&self) -> Arc<dyn Window> {
        self.window.clone()
    }

    fn publish_lists(&self) {
        let lists = lists_of(&self.data.lock().expect("lock"));
        self.window.publish(Update::Conversations(lists));
        self.push_changed();
    }

    fn plays(&self, play: BenchPlay) -> bool {
        self.bench.lock().expect("lock").plays.contains(&play)
    }

    /// `--fixture bench`'s library, votes and leaderboard: six prompts
    /// (fill-ins, tags, a favourite, two used, one a model added, one with
    /// a picture attached), a vote on the castle's first round and the
    /// houses' tie.
    fn fixture(data: &Mutex<Data>, bench: &Mutex<State>) {
        let picture = format!(
            "data:image/png;base64,{}",
            base64_of(super::super::fixtures::PICTURE)
        );
        let prompt = |id: &str, title: &str, text: &str, tags: &[&str]| crate::LibraryPrompt {
            id: id.into(),
            title: title.into(),
            text: text.into(),
            tags: tags.iter().map(|t| t.to_string()).collect(),
            favorite: false,
            attachments: Vec::new(),
            used_at: None,
            added_by: None,
        };
        let mut library = vec![
            crate::LibraryPrompt {
                favorite: true,
                used_at: Some(super::super::fixtures::TODAY - 3_600_000),
                ..prompt(
                    "prompt-castle",
                    "Castle with a moat",
                    "A castle with four corner towers and a {{width}} moat.",
                    &["castle", "stone"],
                )
            },
            crate::LibraryPrompt {
                used_at: Some(super::super::fixtures::TODAY - 60_000),
                ..prompt(
                    "prompt-farm",
                    "Wheat field",
                    "A small wheat farm with water in the middle.",
                    &["farm"],
                )
            },
            prompt(
                "prompt-tower",
                "Two towers",
                "Two stone towers joined by a bridge.",
                &["castle"],
            ),
            crate::LibraryPrompt {
                attachments: vec![PromptAttachment {
                    name: "lamp.png".into(),
                    content_type: "image/png".into(),
                    size: super::super::fixtures::PICTURE.len() as u64,
                    data: picture,
                }],
                ..prompt(
                    "prompt-lamp",
                    "Lamp like this one",
                    "A redstone lamp on a post, like the picture, lit by a {{switch}}.",
                    &["redstone", "light"],
                )
            },
            crate::LibraryPrompt {
                added_by: Some(PromptAddedBy {
                    chat: "chat-lamp".into(),
                    model: "anthropic/claude-opus-5-5".into(),
                }),
                ..prompt(
                    "prompt-bridge",
                    "Rope bridge",
                    "A rope bridge over a ravine, {{length}} blocks long.",
                    &["terrain"],
                )
            },
            prompt("prompt-blank", "", "A hut.", &[]),
        ];
        let mut locked = data.lock().expect("lock");
        locked.library.clear();
        locked.library.append(&mut library);
        drop(locked);
        build_records(data, bench);
        let mut state = bench.lock().expect("lock");
        let castle = record(&mut state.records, "bench-castle");
        castle.votes.push((
            "bench-castle-r1".into(),
            Vote {
                pick: Some("castle-4".into()),
                blind: true,
            },
        ));
        // The castle's second round was edited once: two versions.
        castle.nodes.push(Node {
            id: "bench-castle-r2-old".into(),
            parent: Some("bench-castle-r1".into()),
            text: "Add a gate on the south side.".into(),
            prompts: Vec::new(),
            files: Vec::new(),
        });
        let houses = record(&mut state.records, "bench-houses");
        houses.votes.push((
            "bench-houses-r1".into(),
            Vote {
                pick: None,
                blind: true,
            },
        ));
    }

    /// `prompt_count` prompts of `text_length` characters each, for the
    /// speed runs (`--prompts N`, `--prompt-chars N`).
    fn many_prompts(data: &Mutex<Data>, count: usize, chars: usize) {
        let mut data = data.lock().expect("lock");
        for n in 0..count {
            let mut text = format!("Prompt {n}: a build with {{{{style}}}} walls. ");
            while text.len() < chars {
                text.push_str("Stone bricks, oak logs and glass panes in rows. ");
            }
            data.library.push(crate::LibraryPrompt {
                id: format!("prompt-many-{n}"),
                title: format!("Generated prompt {n}"),
                text,
                tags: vec![["castle", "farm", "terrain", "light"][n % 4].to_string()],
                favorite: n % 17 == 0,
                attachments: Vec::new(),
                used_at: (n % 11 == 0).then_some(n as i64),
                added_by: None,
            });
        }
    }

    fn build_records(&self) {
        build_records(&self.data, &self.bench);
    }

    /// The details of `bench` now.
    fn details(&self, bench: &str) -> Result<BenchDetails, String> {
        if self.plays(BenchPlay::DetailsFail) {
            return Err(DETAILS_FAILED.into());
        }
        self.build_records();
        let data = self.data.lock().expect("lock");
        let state = self.bench.lock().expect("lock");
        let listed = data
            .benches
            .iter()
            .find(|b| b.id == bench)
            .ok_or_else(|| "Bench not found".to_string())?;
        let record = &state
            .records
            .iter()
            .find(|(id, _)| id == bench)
            .ok_or_else(|| "Bench not found".to_string())?
            .1;
        let line = record.line();
        let rounds = line
            .iter()
            .map(|node| {
                let siblings: Vec<&Node> = record
                    .nodes
                    .iter()
                    .filter(|n| n.parent == node.parent)
                    .collect();
                let index = siblings
                    .iter()
                    .position(|n| n.id == node.id)
                    .expect("among its siblings");
                BenchRound {
                    id: node.id.clone(),
                    parent_id: node.parent.clone(),
                    content: Content::Parts(
                        std::iter::once(Part::Text {
                            text: node.text.clone(),
                        })
                        .chain(node.files.iter().map(|file| Part::Attachment {
                            attachment: file.clone(),
                        }))
                        .collect(),
                    ),
                    prompts: node.prompts.clone(),
                    created_at: super::super::fixtures::CREATED_AT.into(),
                    branch: RoundBranch {
                        index,
                        count: siblings.len(),
                        previous_id: index.checked_sub(1).map(|i| siblings[i].id.clone()),
                        next_id: siblings.get(index + 1).map(|n| n.id.clone()),
                    },
                }
            })
            .collect::<Vec<_>>();
        let stats = listed
            .order
            .iter()
            .map(|member| {
                let chat = data
                    .chats
                    .iter()
                    .find(|c| c.row.id == *member)
                    .unwrap_or_else(|| panic!("the member {member} is a chat"));
                let seat = chat.row.bench.as_ref().expect("a member's seat");
                let finished = |i: usize| {
                    i + 1 < seat.round as usize
                        || (i + 1 == seat.round as usize
                            && chat.row.running_since.is_none()
                            && seat.halted.is_none())
                };
                let rounds = (0..line.len())
                    .map(|i| {
                        let n = (seat.slot as u64 + 1) * (i as u64 + 2);
                        if finished(i) {
                            RoundStats {
                                ms: Some(31_000 + n * 4_700),
                                input_tokens: Some(8_000 + n * 1_300),
                                output_tokens: Some(900 + n * 210),
                            }
                        } else if i + 1 == seat.round as usize
                            && chat.row.running_since.is_none()
                            && seat.halted.is_some()
                        {
                            // par-bench: a failed or stopped round has its
                            // wall time to its last message and no usage
                            // (`roundStats`, server/index.js 603-617).
                            RoundStats {
                                ms: Some(12_000 + n * 1_100),
                                input_tokens: None,
                                output_tokens: None,
                            }
                        } else {
                            RoundStats::default()
                        }
                    })
                    .collect();
                (member.clone(), rounds)
            })
            .collect();
        Ok(BenchDetails {
            id: listed.id.clone(),
            title: listed.title.clone(),
            created_at: listed.created_at.clone(),
            head: Some(record.head.clone()),
            rounds,
            blind: listed.blind,
            revealed: listed.revealed,
            order: listed.order.clone(),
            votes: record.votes.clone(),
            stats,
        })
    }

    fn publish_bench(&self, bench: &str) {
        let details = self.details(bench);
        if let Ok(d) = &details {
            self.bench
                .lock()
                .expect("lock")
                .last
                .insert(bench.to_string(), d.clone());
        }
        self.window().publish(Update::Bench {
            bench: bench.to_string(),
            details,
        });
    }

    /// `Update::Bench` for a watched bench whose details changed since
    /// they were last published (stage three's answer 2).
    fn push_asked(&self, bench: &str) {
        if !self.bench.lock().expect("lock").asked.contains(bench) {
            return;
        }
        let Ok(now) = self.details(bench) else {
            return;
        };
        let changed = self.bench.lock().expect("lock").last.get(bench) != Some(&now);
        if changed {
            self.publish_bench(bench);
        }
    }

    /// A command's ending in its bench's details after a chat list: the
    /// details, unless that list's push just published them as they are.
    fn end_in_bench(&self, bench: &str) {
        let now = self.details(bench);
        let same = match &now {
            Ok(d) => self.bench.lock().expect("lock").last.get(bench) == Some(d),
            Err(_) => false,
        };
        if !same {
            self.publish_bench(bench);
        }
    }

    /// After a chat list: every watched bench whose details changed.
    fn push_changed(&self) {
        let watched: Vec<String> = self
            .bench
            .lock()
            .expect("lock")
            .asked
            .iter()
            .cloned()
            .collect();
        for bench in watched {
            self.push_asked(&bench);
        }
    }

    /// The leaderboard's rows: the fixture's ranking, none with
    /// `leaderboard-empty`.
    fn leaderboard_entries(&self) -> Vec<LeaderboardEntry> {
        let (cast, cleared) = {
            let state = self.bench.lock().expect("lock");
            (state.votes_cast as i64, state.cleared)
        };
        if self.plays(BenchPlay::LeaderboardEmpty) || (cleared && cast == 0) {
            return Vec::new();
        }
        let entry =
            |provider: &str, model: &str, effort: Option<&str>, wins, votes, rating, benches| {
                LeaderboardEntry {
                    provider: provider.into(),
                    model: model.into(),
                    effort: effort.map(str::to_string),
                    wins,
                    votes,
                    rating,
                    benches,
                }
            };
        vec![
            entry(
                "google",
                "gemini-3-pro",
                None,
                5 + cast,
                9 + cast,
                1068.0 + 8.0 * cast as f64,
                4,
            ),
            entry(
                "anthropic",
                "claude-opus-5-5",
                None,
                4,
                10 + cast,
                1041.0,
                5,
            ),
            entry("openai", "gpt-5.5", Some("high"), 3, 8 + cast, 1012.0, 3),
            entry("openrouter", "x-ai/grok-5", None, 1, 6, 977.0, 2),
            entry("openai", "gpt-5.5", Some("low"), 1, 7, 968.0, 2),
            entry("groq", "llama-5-70b", None, 0, 5, 934.0, 2),
        ]
    }

    /// The leaderboard as published: its read's failure with
    /// `leaderboard-fails`.
    fn leaderboard_failure(&self) -> Option<String> {
        self.plays(BenchPlay::LeaderboardFails)
            .then(|| LEADERBOARD_FAILED.to_string())
    }

    /// At attach: a model's edit of the library 2 s later
    /// (`model-edits-library`).
    fn attached(&self) {
        if !self.plays(BenchPlay::ModelEditsLibrary) {
            return;
        }
        let core = self.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(2));
            let library = {
                let mut data = core.data.lock().expect("lock");
                data.library.push(crate::LibraryPrompt {
                    id: "prompt-added".into(),
                    title: ADDED_TITLE.into(),
                    text: "A sandstone pyramid with a hidden chamber.".into(),
                    tags: vec!["desert".into()],
                    favorite: false,
                    attachments: Vec::new(),
                    used_at: None,
                    added_by: Some(PromptAddedBy {
                        chat: "chat-lamp".into(),
                        model: "anthropic/claude-opus-5-5".into(),
                    }),
                });
                data.library.clone()
            };
            core.window().publish(Update::Prompts(Ok(library)));
        });
    }

    fn model_edits_library(&self, prompt: crate::LibraryPrompt) {
        let library = {
            let mut data = self.data.lock().expect("lock");
            data.library.push(prompt);
            data.library.clone()
        };
        self.window().publish(Update::Prompts(Ok(library)));
    }

    /// A bench member stopped (`Command::Stop` on a member's chat): true
    /// when `chat` was a running member, which is now stopped.
    fn stop(&self, chat: &str) -> bool {
        let bench = {
            let mut data = self.data.lock().expect("lock");
            let Some(member) = data.chats.iter_mut().find(|c| c.row.id == chat) else {
                return false;
            };
            let Some(seat) = member.row.bench.clone() else {
                return false;
            };
            if member.row.running_since.is_none() {
                return true;
            }
            member.row.running_since = None;
            member.push(
                "s",
                json!({ "role": "assistant", "content": "(stopped by user)", "stopped": true }),
            );
            member.row.bench.as_mut().expect("a seat").halted = Some(Halted::Stopped);
            seat.bench
        };
        self.publish_lists();
        self.push_asked(&bench);
        true
    }

    /// A send to a new bench or a bench.
    fn send(&self, outgoing: Outgoing) {
        let refuse = |error: String| {
            self.window().publish(Update::SendRefused {
                target: outgoing.target.clone(),
                error,
            })
        };
        if !outgoing.selections.is_empty() {
            return refuse("Selections belong to one build, not a bench".into());
        }
        if outgoing.effort.is_some() {
            return refuse(
                "effort is not sent with a bench's round: each model has its own".into(),
            );
        }
        if outgoing.text.trim().is_empty() && outgoing.attachments.is_empty() {
            return refuse("Message text or an attachment is required".into());
        }
        self.build_records();
        match outgoing.target.clone() {
            Target::NewBench { members, blind } => {
                if !(2..=8).contains(&members.len()) {
                    return refuse("A bench runs 2 to 8 models".into());
                }
                if self.plays(BenchPlay::CreateRefused) {
                    return refuse(CREATE_REFUSED.into());
                }
                if self.plays(BenchPlay::CreateHeld) {
                    self.bench.lock().expect("lock").held = Some(outgoing.clone());
                    return;
                }
                let bench = self.create(&members, blind, &outgoing);
                self.publish_lists();
                self.bench.lock().expect("lock").asked.insert(bench.clone());
                self.publish_bench(&bench);
                self.window().publish(Update::BenchSent {
                    target: outgoing.target.clone(),
                    bench: bench.clone(),
                    round: 1,
                });
                self.run_members(&bench);
            }
            Target::Bench(bench) => {
                let own = self
                    .bench
                    .lock()
                    .expect("lock")
                    .records
                    .iter()
                    .find(|(id, _)| *id == bench)
                    .map(|(_, r)| r.resolution);
                let Some(own) = own else {
                    return refuse("Bench not found".into());
                };
                if outgoing.resolution != own {
                    return refuse(format!(
                        "resolution {} is not this bench's ({})",
                        outgoing.resolution.value(),
                        own.value()
                    ));
                }
                if self.plays(BenchPlay::RoundRefused) {
                    return refuse(ROUND_REFUSED.into());
                }
                if outgoing.edit_of.is_some() && self.any_running(&bench) {
                    return refuse("Stop the running models before editing a round".into());
                }
                let round = self.add_round(&bench, &outgoing);
                self.publish_lists();
                self.bench.lock().expect("lock").asked.insert(bench.clone());
                self.end_in_bench(&bench);
                self.window().publish(Update::BenchSent {
                    target: outgoing.target.clone(),
                    bench: bench.clone(),
                    round,
                });
                self.run_members(&bench);
            }
            _ => unreachable!("only a bench's targets come here"),
        }
    }

    // par-bench: a round's files are stored as a sent message's, under the
    // files folder (`StandIn::keep_files`), named by the bench.
    /// Stores a round's files; their parts.
    fn round_files(&self, bench: &str, outgoing: &Outgoing) -> Vec<mcbench_store::Attachment> {
        if outgoing.attachments.is_empty() {
            return Vec::new();
        }
        let folder = self
            .data
            .lock()
            .expect("lock")
            .files
            .clone()
            .expect("the stand-in stores a round's files only with keep_files");
        let tag = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("the clock is after 1970")
            .as_nanos();
        outgoing
            .attachments
            .iter()
            .enumerate()
            .map(|(n, file)| {
                let stored = format!("{tag}-{n}-{}", file.name);
                let path = folder.join("attachments").join(bench).join(&stored);
                std::fs::create_dir_all(path.parent().expect("a folder"))
                    .unwrap_or_else(|e| panic!("cannot create {}: {e}", path.display()));
                std::fs::write(&path, &file.data)
                    .unwrap_or_else(|e| panic!("cannot write {}: {e}", path.display()));
                mcbench_store::Attachment {
                    name: file.name.clone(),
                    mime: file.content_type.clone(),
                    size: file.size,
                    url: format!("{}{bench}/{stored}", crate::state::files::ATTACHMENTS),
                    path: format!("/work/attachments/{stored}"),
                }
            })
            .collect()
    }

    fn any_running(&self, bench: &str) -> bool {
        self.data.lock().expect("lock").chats.iter().any(|c| {
            c.row.bench.as_ref().is_some_and(|s| s.bench == bench) && c.row.running_since.is_some()
        })
    }

    /// Makes a bench: its members' chats, each given the first round.
    fn create(&self, members: &[BenchChoice], blind: bool, outgoing: &Outgoing) -> String {
        let created = self.bench.lock().expect("lock").created + 1;
        let files = self.round_files(&format!("bench-new-{created}"), outgoing);
        let mut state = self.bench.lock().expect("lock");
        state.created += 1;
        let bench = format!("bench-new-{}", state.created);
        let title: String = outgoing.text.chars().take(40).collect();
        let round = format!("{bench}-r1");
        state.records.push((
            bench.clone(),
            Record {
                nodes: vec![Node {
                    id: round.clone(),
                    parent: None,
                    text: outgoing.text.clone(),
                    prompts: outgoing.prompts.clone(),
                    files,
                }],
                head: round.clone(),
                votes: Vec::new(),
                resolution: outgoing.resolution,
            },
        ));
        drop(state);
        let mut data = self.data.lock().expect("lock");
        let ids: Vec<String> = (0..members.len())
            .map(|i| format!("{bench}-m{}", i + 1))
            .collect();
        let mut order = ids.clone();
        order.rotate_left(1);
        for (slot, (member, id)) in members.iter().zip(&ids).enumerate() {
            let mut chat = ChatData {
                row: ChatRow {
                    id: id.clone(),
                    title: title.clone(),
                    provider: member.provider.clone(),
                    model: member.model.clone(),
                    effort: member.effort.clone(),
                    resolution: outgoing.resolution,
                    created_at: "2026-11-17T12:00:00Z".into(),
                    bench: Some(BenchSeat {
                        bench: bench.clone(),
                        slot: slot as u32,
                        round: 1,
                        halted: None,
                    }),
                    unread: false,
                    message_count: 0,
                    prompts: outgoing.prompts.iter().map(|p| p.id.clone()).collect(),
                    running_since: Some(now_ms()),
                },
                rows: Vec::new(),
                head: None,
                archived_at: None,
            };
            chat.push("u", super::user_payload(&outgoing.text));
            data.chats.insert(0, chat);
        }
        data.benches.insert(
            0,
            ListedBench {
                id: bench.clone(),
                title,
                created_at: "2026-11-17T12:00:00Z".into(),
                head: Some(round),
                resolution: Some(i64::from(outgoing.resolution.value())),
                rounds: 1,
                prompts: outgoing.prompts.iter().map(|p| p.id.clone()).collect(),
                blind,
                revealed: false,
                order,
            },
        );
        bench
    }

    /// The bench's next round, or a round's new version: every member that
    /// is idle and not held takes it now; the others queue. Answers the
    /// round to show.
    fn add_round(&self, bench: &str, outgoing: &Outgoing) -> usize {
        let files = self.round_files(bench, outgoing);
        let mut state = self.bench.lock().expect("lock");
        let record = record(&mut state.records, bench);
        let parent = match &outgoing.edit_of {
            Some(of) => record
                .nodes
                .iter()
                .find(|n| n.id == of.0)
                .unwrap_or_else(|| panic!("no round {}", of.0))
                .parent
                .clone(),
            None => Some(record.head.clone()),
        };
        let id = format!("{bench}-r{}", record.nodes.len() + 1);
        record.nodes.push(Node {
            id: id.clone(),
            parent,
            text: outgoing.text.clone(),
            prompts: outgoing.prompts.clone(),
            files,
        });
        record.head = id.clone();
        let round = record.line().len();
        drop(state);
        let mut data = self.data.lock().expect("lock");
        let listed = data
            .benches
            .iter_mut()
            .find(|b| b.id == bench)
            .expect("listed");
        listed.rounds = round;
        listed.head = Some(id);
        for chat in data.chats.iter_mut() {
            let Some(seat) = chat.row.bench.clone() else {
                continue;
            };
            if seat.bench != bench {
                continue;
            }
            let edit = outgoing.edit_of.is_some();
            if edit || (chat.row.running_since.is_none() && seat.halted.is_none()) {
                chat.push("u", super::user_payload(&outgoing.text));
                let s = chat.row.bench.as_mut().expect("a seat");
                s.round = round as u32;
                s.halted = None;
                chat.row.running_since = Some(now_ms());
            }
        }
        round
    }

    /// The members running on `bench` finish after their time (at once
    /// under test), unless `members-wait`.
    fn run_members(&self, bench: &str) {
        if self.plays(BenchPlay::MembersWait) {
            return;
        }
        let after = self.rate * MEMBER_STEPS;
        if after.is_zero() {
            return self.finish_members(bench);
        }
        let core = self.clone();
        let bench = bench.to_string();
        std::thread::spawn(move || {
            std::thread::sleep(after);
            core.finish_members(&bench);
        });
    }

    /// Every running member of `bench` finishes its round; one queued for a
    /// later round takes it. The list, then the details.
    fn finish_members(&self, bench: &str) {
        let rounds = self
            .data
            .lock()
            .expect("lock")
            .benches
            .iter()
            .find(|b| b.id == bench)
            .map(|b| b.rounds);
        let Some(rounds) = rounds else {
            return;
        };
        let texts: Vec<String> = {
            let state = self.bench.lock().expect("lock");
            state
                .records
                .iter()
                .find(|(id, _)| id == bench)
                .map(|(_, r)| r.line().iter().map(|n| n.text.clone()).collect())
                .unwrap_or_default()
        };
        let mut again = false;
        {
            let mut data = self.data.lock().expect("lock");
            for chat in data.chats.iter_mut() {
                let Some(seat) = chat.row.bench.clone() else {
                    continue;
                };
                if seat.bench != bench {
                    continue;
                }
                if chat.row.running_since.is_some() {
                    chat.row.running_since = None;
                    chat.row.unread = true;
                    chat.push(
                        "a",
                        json!({ "role": "assistant", "content": format!("Built round {}.", seat.round) }),
                    );
                }
                let seat = chat.row.bench.clone().expect("a seat");
                if seat.halted.is_none() && (seat.round as usize) < rounds {
                    let next = texts[seat.round as usize].clone();
                    chat.push("u", super::user_payload(&next));
                    chat.row.bench.as_mut().expect("a seat").round += 1;
                    chat.row.running_since = Some(now_ms());
                    again = true;
                }
            }
        }
        self.publish_lists();
        self.push_asked(bench);
        if again {
            self.run_members(bench);
        }
    }

    /// Bench's and the library's commands.
    fn command(&self, command: Command) {
        self.build_records();
        match command {
            Command::ReadBench { bench } => {
                self.bench.lock().expect("lock").asked.insert(bench.clone());
                self.publish_bench(&bench);
            }
            Command::Vote { bench, round, pick } => {
                let refusal = if self.plays(BenchPlay::VoteFails) {
                    Some(VOTE_REFUSED.to_string())
                } else {
                    self.vote_refusal(&bench, &round, &pick)
                };
                if let Some(error) = refusal {
                    self.window().publish(Update::Error(error));
                    return self.publish_bench(&bench);
                }
                {
                    let mut state = self.bench.lock().expect("lock");
                    let record = record(&mut state.records, &bench);
                    let before = record.votes.iter().any(|(id, _)| *id == round);
                    record.votes.retain(|(id, _)| *id != round);
                    let cast = match pick {
                        VotePick::Member(member) => Some(Some(member)),
                        VotePick::Tie => Some(None),
                        VotePick::TakeBack => None,
                    };
                    if let Some(pick) = cast {
                        record
                            .votes
                            .push((round.clone(), Vote { pick, blind: true }));
                        if !before {
                            state.votes_cast += 1;
                        }
                    } else if before {
                        state.votes_cast = state.votes_cast.saturating_sub(1);
                    }
                }
                self.publish_bench(&bench);
                self.publish_leaderboard();
            }
            Command::Reveal { bench } => {
                if self.plays(BenchPlay::RevealFails) {
                    self.window().publish(Update::Error(REVEAL_FAILED.into()));
                    self.publish_lists();
                    return self.publish_bench(&bench);
                }
                if let Some(b) = self
                    .data
                    .lock()
                    .expect("lock")
                    .benches
                    .iter_mut()
                    .find(|b| b.id == bench)
                {
                    b.revealed = true;
                }
                self.publish_lists();
                self.end_in_bench(&bench);
            }
            Command::BenchBranch { bench, round } => {
                if self.plays(BenchPlay::BranchFails) || self.any_running(&bench) {
                    self.window().publish(Update::Error(BRANCH_REFUSED.into()));
                    self.publish_lists();
                    return self.publish_bench(&bench);
                }
                let length = {
                    let mut state = self.bench.lock().expect("lock");
                    let record = record(&mut state.records, &bench);
                    // The round's newest descendant is the new head.
                    let mut head = round.clone();
                    while let Some(child) = record
                        .nodes
                        .iter()
                        .rev()
                        .find(|n| n.parent.as_deref() == Some(head.as_str()))
                    {
                        head = child.id.clone();
                    }
                    record.head = head;
                    record.line().len()
                };
                if let Some(b) = self
                    .data
                    .lock()
                    .expect("lock")
                    .benches
                    .iter_mut()
                    .find(|b| b.id == bench)
                {
                    b.rounds = length;
                }
                self.publish_lists();
                self.end_in_bench(&bench);
            }
            Command::RetryMember { bench, member } => {
                let retried = !self.plays(BenchPlay::RetryFails) && {
                    let mut data = self.data.lock().expect("lock");
                    let chat = data.chats.iter_mut().find(|c| c.row.id == member);
                    match chat {
                        Some(chat)
                            if chat.row.bench.as_ref().is_some_and(|s| s.halted.is_some()) =>
                        {
                            chat.row.bench.as_mut().expect("a seat").halted = None;
                            chat.row.running_since = Some(now_ms());
                            true
                        }
                        _ => false,
                    }
                };
                if !retried {
                    self.window().publish(Update::Error(RETRY_REFUSED.into()));
                }
                self.publish_lists();
                if retried {
                    self.run_members(&bench);
                }
            }
            Command::Fork { member, round } => {
                let made = if self.plays(BenchPlay::ForkFails) {
                    Err(FORK_FAILED.to_string())
                } else {
                    self.fork(&member, round)
                };
                if made.is_ok() {
                    self.publish_lists();
                }
                self.window().publish(Update::Forked { member, chat: made });
            }
            Command::SavePrompts(prompts) => {
                if self.plays(BenchPlay::SaveFails) {
                    return self
                        .window()
                        .publish(Update::PromptsSaved(Err(SAVE_FAILED.into())));
                }
                self.data.lock().expect("lock").library = prompts.clone();
                self.window().publish(Update::PromptsSaved(Ok(prompts)));
            }
            Command::GeneratePrompt { request, text, .. } => {
                let fails = self.plays(BenchPlay::GenerateFails);
                let after = if self.plays(BenchPlay::GenerateSlow) {
                    Duration::from_secs(4)
                } else {
                    self.rate * MEMBER_STEPS
                };
                let outcome = move || {
                    if fails {
                        Generated::Failed(GENERATE_FAILED.into())
                    } else {
                        Generated::Text(format!(
                            "{}\n\nUse stone bricks for the walls, dark oak for the roof, and light it with lanterns.",
                            text.trim()
                        ))
                    }
                };
                if after.is_zero() {
                    return self.window().publish(Update::Generated {
                        request,
                        outcome: outcome(),
                    });
                }
                let core = self.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(after);
                    let mut state = core.bench.lock().expect("lock");
                    if state.cancelled.contains(&request) {
                        return;
                    }
                    state.finished.insert(request);
                    drop(state);
                    core.window().publish(Update::Generated {
                        request,
                        outcome: outcome(),
                    });
                });
            }
            Command::CancelGenerate { request } => {
                let mut state = self.bench.lock().expect("lock");
                if state.finished.contains(&request) || !state.cancelled.insert(request) {
                    return;
                }
                drop(state);
                self.window().publish(Update::Generated {
                    request,
                    outcome: Generated::Cancelled,
                });
            }
            other => panic!("not a bench command: {other:?}"),
        }
    }

    /// The vote route's refusals (`Core::vote`).
    fn vote_refusal(&self, bench: &str, round: &str, pick: &VotePick) -> Option<String> {
        let details = self.details(bench).ok()?;
        if !crate::state::bench::hides_names(details.blind, details.revealed) {
            return Some("Votes are cast while the models' names are hidden".into());
        }
        let Some(index) = details.rounds.iter().position(|r| r.id == round) else {
            return Some("Unknown round".into());
        };
        let data = self.data.lock().expect("lock");
        let states: Vec<(String, crate::state::bench::RoundState)> = details
            .order
            .iter()
            .map(|m| {
                let chat = data
                    .chats
                    .iter()
                    .find(|c| c.row.id == *m)
                    .expect("a member");
                let seat = chat.row.bench.as_ref().expect("a seat");
                let progress = crate::state::bench::Progress {
                    round: seat.round as usize,
                    running: chat.row.running_since.is_some(),
                    halted: seat.halted.map(|h| match h {
                        Halted::Failed => crate::state::bench::Halt::Failed,
                        Halted::Stopped => crate::state::bench::Halt::Stopped,
                    }),
                };
                (
                    m.clone(),
                    crate::state::bench::round_state(progress, index + 1),
                )
            })
            .collect();
        use crate::state::bench::RoundState;
        if states
            .iter()
            .any(|(_, s)| matches!(s, RoundState::Running | RoundState::Queued))
        {
            return Some("Vote once every model has finished this round".into());
        }
        let done: Vec<&String> = states
            .iter()
            .filter(|(_, s)| *s == RoundState::Done)
            .map(|(m, _)| m)
            .collect();
        if done.len() < 2 {
            return Some("A vote needs at least two finished builds".into());
        }
        match pick {
            VotePick::Member(m) if !done.contains(&m) => {
                Some("Pick a model that finished this round".into())
            }
            _ => None,
        }
    }

    /// A member's chat continued in Build from round `round`.
    fn fork(&self, member: &str, round: usize) -> Result<String, String> {
        let mut data = self.data.lock().expect("lock");
        let source = data
            .chats
            .iter()
            .find(|c| c.row.id == member && c.row.bench.is_some())
            .ok_or_else(|| "Bench chat not found".to_string())?
            .clone();
        let seat = source.row.bench.clone().expect("a seat");
        if round == 0 || round > seat.round as usize {
            return Err("Unknown round".into());
        }
        if round == seat.round as usize && source.row.running_since.is_some() {
            return Err(FORK_WAIT.into());
        }
        data.created += 1;
        let id = format!("chat-fork-{}", data.created);
        let mut chat = ChatData {
            row: ChatRow {
                id: id.clone(),
                bench: None,
                unread: false,
                running_since: None,
                message_count: 0,
                ..source.row.clone()
            },
            rows: Vec::new(),
            head: None,
            archived_at: None,
        };
        let mut prompts = 0;
        for row in source.branch() {
            let prompt = row.payload["role"] == "user" && row.payload["seeBuild"] != true;
            if prompt {
                prompts += 1;
                if prompts > round {
                    break;
                }
            }
            let mut payload = row.payload.clone();
            payload
                .as_object_mut()
                .expect("a payload object")
                .remove("createdAt");
            chat.push("f", payload);
        }
        data.chats.insert(0, chat);
        // A fork shows the model: a blind bench is revealed.
        if let Some(b) = data.benches.iter_mut().find(|b| b.id == seat.bench) {
            b.revealed = true;
        }
        Ok(id)
    }

    fn publish_leaderboard(&self) {
        let board = self.leaderboard_now();
        self.window().publish(Update::Leaderboard(board));
    }

    /// The leaderboard now: the vote count is Settings' (plus the votes cast
    /// here), the entries the fixture's.
    fn leaderboard_now(&self) -> Result<crate::Leaderboard, String> {
        if let Some(error) = self.leaderboard_failure() {
            return Err(error);
        }
        let cast = self.bench.lock().expect("lock").votes_cast;
        let votes = self.settings_votes.clone()?;
        Ok(crate::Leaderboard {
            votes: if self.plays(BenchPlay::LeaderboardEmpty) {
                0
            } else {
                votes + cast
            },
            entries: self.leaderboard_entries(),
        })
    }

    /// After every vote was cleared: every asked bench's votes are empty,
    /// and each is published (stage three's answer).
    fn votes_cleared(&self) {
        let asked: Vec<String> = {
            let state = self.bench.lock().expect("lock");
            state.asked.iter().cloned().collect()
        };
        for bench in asked {
            self.publish_bench(&bench);
        }
    }
}

/// The records of the listed benches, made once: a bench's rounds are
/// its members' prompts (the most any member was sent).
fn build_records(data: &Mutex<Data>, bench_state: &Mutex<State>) {
    let mut data = data.lock().expect("lock");
    let mut state = bench_state.lock().expect("lock");
    if state.built {
        return;
    }
    state.built = true;
    let benches: Vec<ListedBench> = data.benches.clone();
    for bench in benches {
        let members: Vec<&ChatData> = data
            .chats
            .iter()
            .filter(|c| c.row.bench.as_ref().is_some_and(|s| s.bench == bench.id))
            .collect();
        let longest = members
            .iter()
            .max_by_key(|m| m.row.bench.as_ref().map_or(0, |s| s.round))
            .unwrap_or_else(|| panic!("the bench {} has models", bench.id));
        let prompts: Vec<String> = longest
            .branch()
            .iter()
            .filter(|r| r.payload["role"] == "user" && r.payload["seeBuild"] != true)
            .map(|r| super::message_text(&r.payload))
            .collect();
        let mut nodes = Vec::new();
        for (i, text) in prompts.iter().enumerate() {
            nodes.push(Node {
                id: format!("{}-r{}", bench.id, i + 1),
                parent: (i > 0).then(|| format!("{}-r{i}", bench.id)),
                text: text.clone(),
                prompts: Vec::new(),
                files: Vec::new(),
            });
        }
        let head = nodes.last().expect("a bench has a round").id.clone();
        let resolution = Resolution::of(bench.resolution.unwrap_or(1) as u32);
        state.records.push((
            bench.id.clone(),
            Record {
                nodes,
                head: head.clone(),
                votes: Vec::new(),
                resolution,
            },
        ));
        let listed = data
            .benches
            .iter_mut()
            .find(|b| b.id == bench.id)
            .expect("listed");
        listed.rounds = prompts.len();
        listed.head = Some(head);
    }
}

fn record<'a>(records: &'a mut [(String, Record)], bench: &str) -> &'a mut Record {
    &mut records
        .iter_mut()
        .find(|(id, _)| id == bench)
        .unwrap_or_else(|| panic!("no bench {bench}"))
        .1
}

fn base64_of(bytes: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}
