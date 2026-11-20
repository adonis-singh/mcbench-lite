//! A bench's details (its rounds, stats and votes) as the window holds
//! them, one value per bench (`web/bench-details.mjs`, tests ported from
//! `test/bench-details.test.js`, same cases and names; inventory `app.md`
//! 7.5 and 8.12, `components.md` L.7):
//!
//! | Phase | What |
//! |---|---|
//! | loading | being read; `data` is what an earlier read gave, still shown meanwhile, or none |
//! | loaded | `data` is the core's |
//! | failed | the read failed (`error` says why): `data` is the earlier read's, or none, and it can be read again |
//!
//! Any read can be followed by another, which supersedes it. A moment the
//! page's rules do not allow is a defect of the window: it panics with the
//! page's sentence.

use crate::{BenchDetails, Vote};

/// Where a bench's read stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// `loading`.
    Loading,
    /// `loaded`.
    Loaded,
    /// `failed`.
    Failed,
}

impl Phase {
    fn name(self) -> &'static str {
        match self {
            Phase::Loading => "loading",
            Phase::Loaded => "loaded",
            Phase::Failed => "failed",
        }
    }

    fn next(self) -> &'static [Phase] {
        match self {
            Phase::Loading => &[Phase::Loading, Phase::Loaded, Phase::Failed],
            Phase::Loaded | Phase::Failed => &[Phase::Loading],
        }
    }
}

/// A bench's details as held.
#[derive(Clone, Debug, PartialEq)]
pub struct Details {
    /// The phase.
    pub phase: Phase,
    /// What was read, kept through a later read and a failed one.
    pub data: Option<BenchDetails>,
    /// Why the last read failed, in `Failed`.
    pub error: Option<String>,
}

fn details_to(details: Option<&Details>, next: Details) -> Details {
    if let Some(details) = details
        && !details.phase.next().contains(&next.phase)
    {
        panic!(
            "Bench details cannot go from {} to {}",
            details.phase.name(),
            next.phase.name()
        );
    }
    next
}

/// A read begins (`detailsReading`): `details` is the bench's value so far,
/// none before its first read.
pub fn details_reading(details: Option<&Details>) -> Details {
    details_to(
        details,
        Details {
            phase: Phase::Loading,
            data: details.and_then(|d| d.data.clone()),
            error: None,
        },
    )
}

/// A read landed (`detailsRead`).
pub fn details_read(details: Option<&Details>, data: BenchDetails) -> Details {
    let details = details.expect("Bench details land only for a read that was begun");
    details_to(
        Some(details),
        Details {
            phase: Phase::Loaded,
            data: Some(data),
            error: None,
        },
    )
}

/// A read failed (`detailsFailed`).
pub fn details_failed(details: Option<&Details>, error: String) -> Details {
    let details = details.expect("Bench details fail only for a read that was begun");
    details_to(
        Some(details),
        Details {
            phase: Phase::Failed,
            data: details.data.clone(),
            error: Some(error),
        },
    )
}

/// The votes changed here (a vote cast, every vote cleared): the data the
/// window holds takes them, whatever read is on its way (`detailsVotes`).
pub fn details_votes(details: &Details, votes: Vec<(String, Vote)>) -> Details {
    let data = details
        .data
        .as_ref()
        .expect("Bench details that were never read hold no votes");
    Details {
        data: Some(BenchDetails {
            votes,
            ..data.clone()
        }),
        ..details.clone()
    }
}

#[cfg(test)]
pub(crate) fn sample(rounds: &[&str]) -> BenchDetails {
    BenchDetails {
        id: "b".into(),
        title: "Bench".into(),
        created_at: "2026-11-17T10:00:00.000Z".into(),
        head: rounds.last().map(|r| r.to_string()),
        rounds: rounds
            .iter()
            .map(|id| crate::BenchRound {
                id: id.to_string(),
                parent_id: None,
                content: crate::Content::Text(String::new()),
                prompts: Vec::new(),
                created_at: "2026-11-17T10:00:00.000Z".into(),
                branch: crate::RoundBranch {
                    index: 0,
                    count: 1,
                    previous_id: None,
                    next_id: None,
                },
            })
            .collect(),
        blind: true,
        revealed: false,
        order: Vec::new(),
        votes: Vec::new(),
        stats: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn panics(f: impl FnOnce() + std::panic::UnwindSafe) -> String {
        let error = std::panic::catch_unwind(f).expect_err("it refuses");
        error
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| error.downcast_ref::<&str>().map(|s| s.to_string()))
            .expect("a message")
    }

    #[test]
    fn bench_details_are_loading_loaded_or_failed_and_a_failed_read_can_be_made_again() {
        let data = sample(&["r1"]);
        let details = details_reading(None);
        assert_eq!(
            details,
            Details {
                phase: Phase::Loading,
                data: None,
                error: None
            }
        );
        let details = details_failed(Some(&details), "HTTP 500".into());
        assert_eq!(
            details,
            Details {
                phase: Phase::Failed,
                data: None,
                error: Some("HTTP 500".into())
            }
        );
        // Nothing lands on a failed read: it is read again first.
        let failed = details.clone();
        let d = data.clone();
        assert!(
            panics(move || drop(details_read(Some(&failed), d)))
                .contains("cannot go from failed to loaded")
        );
        let details = details_reading(Some(&details));
        assert_eq!(details.phase, Phase::Loading);
        let details = details_read(Some(&details), data.clone());
        assert_eq!(
            details,
            Details {
                phase: Phase::Loaded,
                data: Some(data.clone()),
                error: None
            }
        );
        let loaded = details.clone();
        assert!(
            panics(move || drop(details_failed(Some(&loaded), "late".into())))
                .contains("cannot go from loaded to failed")
        );
        assert!(
            panics(move || drop(details_read(None, data)))
                .contains("only for a read that was begun")
        );
    }

    #[test]
    fn what_was_read_stays_on_show_through_a_later_read_and_one_that_fails() {
        let data = sample(&["r1"]);
        let details = details_read(Some(&details_reading(None)), data.clone());
        let details = details_reading(Some(&details));
        assert_eq!(
            details,
            Details {
                phase: Phase::Loading,
                data: Some(data.clone()),
                error: None
            }
        );
        // A vote cast meanwhile shows at once.
        let vote = vec![(
            "r1".to_string(),
            Vote {
                pick: Some("m1".into()),
                blind: true,
            },
        )];
        let details = details_votes(&details, vote.clone());
        assert_eq!(details.data.as_ref().expect("data").votes, vote);
        let details = details_failed(Some(&details), "offline".into());
        assert_eq!(details.phase, Phase::Failed);
        assert_eq!(details.error.as_deref(), Some("offline"));
        assert_eq!(details.data.expect("data").rounds, data.rounds);
        assert!(
            panics(|| drop(details_votes(&details_reading(None), Vec::new())))
                .contains("never read hold no votes")
        );
    }
}
