//! The stand-in's Share panel answers (share agent, `ui-seam-share.md`):
//! `ReadShare`, `Publish` and `Unshare` by subject, answered from Settings'
//! published list (`settings::State`'s `shares`), so a build published here
//! is in that list and one taken down here leaves it. A publish moves one
//! step at each [`StandIn::share_tick`] (the dev binary's clock, every
//! `--share-step-ms`; by hand in tests): `Exporting` at once, then
//! `Publishing`, `Waiting`, `Live`. Every failure plays on demand
//! ([`SharePlay`], the dev binary's `--share`).

use std::hash::{Hash as _, Hasher as _};

use super::{SHARING_NOT_JOINED, StandIn, now_ms};
use crate::{Command, PublishStep, PublishedBuild, ShareRead, Subject, Update};

/// How the panel's commands play (`--share <name>`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SharePlay {
    /// `fails-exporting`: a publish fails while preparing the build.
    FailsExporting,
    /// `fails-publishing`: a publish fails at the push.
    FailsPublishing,
    /// `fails-waiting`: GitHub Pages does not serve it in time.
    FailsWaiting,
    /// `held`: a publish stays at its first step (ticks move nothing).
    Held,
    /// `empty`: the build is empty (the route's 400).
    Empty,
    /// `read-fails`: every read of a subject's share fails.
    ReadFails,
    /// `unshare-fails`: a removal fails and the page stays up.
    UnshareFails,
    /// `not-joined`: the real core's answers until sharing is ported.
    NotJoined,
}

impl SharePlay {
    /// The play of a `--share` value.
    pub fn named(name: &str) -> SharePlay {
        match name {
            "fails-exporting" => SharePlay::FailsExporting,
            "fails-publishing" => SharePlay::FailsPublishing,
            "fails-waiting" => SharePlay::FailsWaiting,
            "held" => SharePlay::Held,
            "empty" => SharePlay::Empty,
            "read-fails" => SharePlay::ReadFails,
            "unshare-fails" => SharePlay::UnshareFails,
            "not-joined" => SharePlay::NotJoined,
            other => panic!(
                "--share takes fails-exporting, fails-publishing, fails-waiting, held, empty, read-fails, unshare-fails or not-joined, not {other}"
            ),
        }
    }
}

/// `server/share.js`'s export failure.
pub const SHARE_EXPORT_FAILED: &str = "The viewer's assets are not baked yet";
/// A failed push, as `Shares.update` words a clone that failed.
pub const PUSH_FAILED: &str = "Could not get a copy of mcbench-builds/gallery from GitHub: fatal: unable to access 'https://github.com/mcbench-builds/gallery.git/': Could not resolve host: github.com (a fixture error)";
/// `served`'s time-out, before the page's address.
pub const PAGES_LATE: &str =
    "The files are pushed, but GitHub Pages has not served them yet after 5 minutes: ";
/// The routes' refusals.
pub const REPO_NEEDED: &str = "Set the GitHub repository for shared builds in Settings";
/// A chat with nothing built.
pub const CHAT_EMPTY: &str = "The build is empty: there is nothing to share";
/// A bench with nothing built.
pub const BENCH_EMPTY: &str = "No model in this bench has built anything yet";
/// A chat's running turn.
pub const CHAT_RUNNING: &str = "Wait for the running turn before sharing";
/// A bench's running turns.
pub const BENCH_RUNNING: &str = "Wait for the bench's running turns before sharing";
/// A second publish.
pub const PUBLISHING: &str = "This build is already being published";
/// A subject the core does not have.
pub const NOT_FOUND: &str = "Conversation not found";
/// A removal of a page that is not up.
pub const NOT_SHARED: &str = "This build is not shared";
/// A failed read (`unshare-fails` and `read-fails`).
pub const SITE_FAILED: &str = "Could not bring the site's copy up to date with mcbench-builds/gallery on GitHub: fatal: unable to access 'https://github.com/mcbench-builds/gallery.git/': Could not resolve host: github.com (a fixture error)";

/// The stand-in's share state.
#[derive(Default)]
pub(super) struct State {
    plays: Vec<SharePlay>,
    /// The publishes under way: the subject and the steps it has made.
    running: Vec<(Subject, u8)>,
}

impl State {
    fn plays(&self, play: SharePlay) -> bool {
        self.plays.contains(&play)
    }
}

/// The share id the stand-in gives a subject: 12 hex digits of a hash of
/// its id (today's is sha256's first 12; nothing here depends on which).
fn share_id(subject: &Subject) -> String {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    match subject {
        Subject::Chat(id) => ("chat", id).hash(&mut h),
        Subject::Bench(id) => ("bench", id).hash(&mut h),
    }
    format!("{:012x}", h.finish() & 0xffff_ffff_ffff)
}

/// Whether a published build is the subject's.
fn of(build: &PublishedBuild, subject: &Subject) -> bool {
    match subject {
        Subject::Chat(id) => build.kind == "chat" && build.conversation_id.as_deref() == Some(id),
        Subject::Bench(id) => build.kind == "bench" && build.bench_id.as_deref() == Some(id),
    }
}

impl StandIn {
    /// Plays a share failure or state.
    pub fn share_play(&self, play: SharePlay) {
        self.share.lock().expect("lock").plays.push(play);
    }

    /// The subject's page in Settings' list, if any.
    fn share_url(&self, subject: &Subject) -> Result<Option<String>, String> {
        let s = self.settings.lock().expect("lock");
        let published = s.published().clone()?;
        Ok(published
            .builds
            .iter()
            .find(|b| of(b, subject))
            .map(|b| b.url.clone()))
    }

    /// Whether the core has the subject, and why a publish is refused.
    fn refusal(&self, subject: &Subject) -> Option<String> {
        let data = self.data.lock().expect("lock");
        let (exists, running) = match subject {
            Subject::Chat(id) => match data.chats.iter().find(|c| c.row.id == *id) {
                Some(c) => (true, c.row.running_since.is_some()),
                None => (false, false),
            },
            Subject::Bench(id) => (
                data.benches.iter().any(|b| b.id == *id),
                data.chats.iter().any(|c| {
                    c.row.bench.as_ref().is_some_and(|seat| seat.bench == *id)
                        && c.row.running_since.is_some()
                }),
            ),
        };
        drop(data);
        if !exists {
            return Some(NOT_FOUND.into());
        }
        if self.settings.lock().expect("lock").repo().is_none() {
            return Some(REPO_NEEDED.into());
        }
        let bench = matches!(subject, Subject::Bench(_));
        if self.share.lock().expect("lock").plays(SharePlay::Empty) {
            return Some(if bench { BENCH_EMPTY } else { CHAT_EMPTY }.into());
        }
        if running {
            return Some(if bench { BENCH_RUNNING } else { CHAT_RUNNING }.into());
        }
        let s = self.share.lock().expect("lock");
        s.running
            .iter()
            .any(|(other, _)| other == subject)
            .then(|| PUBLISHING.into())
    }

    /// The share commands (`ui-seam-share.md`).
    pub(super) fn share_command(&self, command: Command) {
        let window = self.window();
        let not_joined = self.share.lock().expect("lock").plays(SharePlay::NotJoined);
        match command {
            Command::ReadShare { subject } => {
                let repo = self.settings.lock().expect("lock").repo();
                let read = if not_joined {
                    Ok(ShareRead { repo, url: None })
                } else if self.share.lock().expect("lock").plays(SharePlay::ReadFails) {
                    Err(SITE_FAILED.into())
                } else {
                    self.share_url(&subject).map(|url| ShareRead { repo, url })
                };
                window.publish(Update::ShareRead { subject, read });
            }
            Command::Publish { subject } => {
                if not_joined {
                    return window.publish(Update::PublishStep {
                        subject,
                        step: PublishStep::Failed {
                            error: SHARING_NOT_JOINED.into(),
                        },
                    });
                }
                if let Some(error) = self.refusal(&subject) {
                    return window.publish(Update::PublishStep {
                        subject,
                        step: PublishStep::Failed { error },
                    });
                }
                let fails = self
                    .share
                    .lock()
                    .expect("lock")
                    .plays(SharePlay::FailsExporting);
                if fails {
                    window.publish(Update::PublishStep {
                        subject: subject.clone(),
                        step: PublishStep::Exporting,
                    });
                    return self.publish_ended(subject, Err(SHARE_EXPORT_FAILED.into()));
                }
                self.share
                    .lock()
                    .expect("lock")
                    .running
                    .push((subject.clone(), 0));
                window.publish(Update::PublishStep {
                    subject,
                    step: PublishStep::Exporting,
                });
            }
            Command::Unshare { subject } => {
                if not_joined {
                    window.publish(Update::Error(SHARING_NOT_JOINED.into()));
                    return window.publish(Update::ShareChanged { subject, url: None });
                }
                if self
                    .share
                    .lock()
                    .expect("lock")
                    .running
                    .iter()
                    .any(|(s, _)| *s == subject)
                {
                    let url = self.share_url(&subject).ok().flatten();
                    window.publish(Update::Error(
                        "Wait for the build to finish publishing".into(),
                    ));
                    return window.publish(Update::ShareChanged { subject, url });
                }
                if self
                    .share
                    .lock()
                    .expect("lock")
                    .plays(SharePlay::UnshareFails)
                {
                    let url = self.share_url(&subject).ok().flatten();
                    window.publish(Update::Error(SITE_FAILED.into()));
                    return window.publish(Update::ShareChanged { subject, url });
                }
                let removed = {
                    let mut s = self.settings.lock().expect("lock");
                    match s.published_mut() {
                        Ok(p) => match p.builds.iter().position(|b| of(b, &subject)) {
                            Some(at) => {
                                p.builds.remove(at);
                                Ok(())
                            }
                            None => Err(NOT_SHARED.to_string()),
                        },
                        Err(e) => Err(e.clone()),
                    }
                };
                if let Err(error) = removed {
                    window.publish(Update::Error(error));
                }
                window.publish(Update::ShareChanged { subject, url: None });
            }
            other => unreachable!("not a share command: {other:?}"),
        }
    }

    /// Moves every publish under way one step (`Publishing`, `Waiting`,
    /// then `Live` into Settings' list), unless held; a `fails-*` play ends
    /// it at that step.
    pub fn share_tick(&self) {
        let moves: Vec<(Subject, u8)> = {
            let mut s = self.share.lock().expect("lock");
            if s.plays(SharePlay::Held) {
                return;
            }
            for (_, step) in &mut s.running {
                *step += 1;
            }
            s.running.clone()
        };
        if moves.is_empty() {
            return;
        }
        let window = self.window();
        for (subject, step) in moves {
            let plays = |p| self.share.lock().expect("lock").plays(p);
            match step {
                1 => window.publish(Update::PublishStep {
                    subject,
                    step: PublishStep::Publishing,
                }),
                2 if plays(SharePlay::FailsPublishing) => {
                    self.publish_ended(subject, Err(PUSH_FAILED.into()))
                }
                2 => window.publish(Update::PublishStep {
                    subject,
                    step: PublishStep::Waiting,
                }),
                _ => {
                    let url = format!(
                        "https://mcbench-builds.github.io/gallery/b/{}/",
                        share_id(&subject)
                    );
                    if plays(SharePlay::FailsWaiting) {
                        self.publish_ended(subject, Err(format!("{PAGES_LATE}{url}")));
                    } else {
                        self.publish_ended(subject, Ok(url));
                    }
                }
            }
        }
    }

    /// A publish's end: live in Settings' list at `url`, or failed.
    fn publish_ended(&self, subject: Subject, end: Result<String, String>) {
        self.share
            .lock()
            .expect("lock")
            .running
            .retain(|(s, _)| *s != subject);
        let step = match end {
            Ok(url) => {
                let build = self.published_build(&subject, &url);
                let mut s = self.settings.lock().expect("lock");
                if let Ok(p) = s.published_mut() {
                    p.builds.retain(|b| !of(b, &subject));
                    p.builds.insert(0, build);
                }
                PublishStep::Live { url }
            }
            Err(error) => PublishStep::Failed { error },
        };
        self.window().publish(Update::PublishStep { subject, step });
    }

    /// The list's row for a build published now.
    fn published_build(&self, subject: &Subject, url: &str) -> PublishedBuild {
        let data = self.data.lock().expect("lock");
        let (title, labels) = match subject {
            Subject::Chat(id) => {
                let row = &data
                    .chats
                    .iter()
                    .find(|c| c.row.id == *id)
                    .expect("a published chat")
                    .row;
                (row.title.clone(), vec![row.model.clone()])
            }
            Subject::Bench(id) => {
                let bench = data
                    .benches
                    .iter()
                    .find(|b| b.id == *id)
                    .expect("a published bench");
                // `benchLabel`: the app's letter while names are hidden.
                let hidden = crate::state::bench::hides_names(bench.blind, bench.revealed);
                let labels = bench
                    .order
                    .iter()
                    .enumerate()
                    .filter_map(|(i, m)| {
                        let chat = data.chats.iter().find(|c| c.row.id == *m)?;
                        Some(if hidden {
                            char::from(b'A' + i as u8).to_string()
                        } else {
                            chat.row.model.clone()
                        })
                    })
                    .collect();
                (bench.title.clone(), labels)
            }
        };
        PublishedBuild {
            share_id: share_id(subject),
            title,
            kind: match subject {
                Subject::Chat(_) => "chat",
                Subject::Bench(_) => "bench",
            }
            .into(),
            labels,
            url: url.into(),
            published_at: now_ms(),
            conversation_id: match subject {
                Subject::Chat(id) => Some(id.clone()),
                Subject::Bench(_) => None,
            },
            bench_id: match subject {
                Subject::Bench(id) => Some(id.clone()),
                Subject::Chat(_) => None,
            },
            archived: false,
        }
    }

    /// The subject of a published build that exists here (`RemoveShare`'s
    /// `ShareChanged`).
    pub(super) fn subject_of(&self, build: &PublishedBuild) -> Option<Subject> {
        let data = self.data.lock().expect("lock");
        match build.kind.as_str() {
            "chat" => build
                .conversation_id
                .clone()
                .filter(|id| data.chats.iter().any(|c| c.row.id == *id))
                .map(Subject::Chat),
            _ => build
                .bench_id
                .clone()
                .filter(|id| data.benches.iter().any(|b| b.id == *id))
                .map(Subject::Bench),
        }
    }
}
