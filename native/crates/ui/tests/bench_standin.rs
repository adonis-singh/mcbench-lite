//! The stand-in answers Bench's and the library's commands as
//! `ui-seam-bench-prompts.md` and stage three's answers say: each ends in
//! the update that carries what it changed, `Update::Error` first on a
//! failure, a failure on demand for each (bench agent).

mod common;

use std::sync::Arc;
use std::time::Duration;

use common::{Inbox, TempDir};
use mcbench_ui::dev::{
    fixtures,
    standin::{self, BenchPlay, StandIn},
};
use mcbench_ui::{
    BenchChoice, Command, Core, Generated, MessageId, Outgoing, Resolution, Target, Update,
    VotePick,
};

fn bench(dir: &TempDir, plays: &[BenchPlay]) -> (StandIn, Arc<Inbox>) {
    let picture = dir.0.join("lamp.png");
    std::fs::write(&picture, fixtures::PICTURE).expect("the picture");
    let texts = fixtures::Texts::new(&picture);
    let core = StandIn::new(
        fixtures::bench(&texts),
        texts.streamed.clone(),
        Duration::ZERO,
        None,
    );
    core.bench_fixture();
    for play in plays {
        core.bench_play(*play);
    }
    let inbox = Arc::new(Inbox::default());
    core.attach(inbox.clone());
    take(&inbox);
    (core, inbox)
}

fn take(inbox: &Inbox) -> Vec<Update> {
    std::mem::take(&mut *inbox.0.lock().expect("lock"))
}

fn kinds(updates: &[Update]) -> Vec<&'static str> {
    updates
        .iter()
        .map(|u| match u {
            Update::Conversations(_) => "list",
            Update::Bench { details: Ok(_), .. } => "bench",
            Update::Bench {
                details: Err(_), ..
            } => "bench failed",
            Update::BenchSent { .. } => "sent",
            Update::SendRefused { .. } => "refused",
            Update::Error(_) => "error",
            Update::Leaderboard(Ok(_)) => "leaderboard",
            Update::Leaderboard(Err(_)) => "leaderboard failed",
            Update::Forked { .. } => "forked",
            Update::PromptsSaved(Ok(_)) => "saved",
            Update::PromptsSaved(Err(_)) => "not saved",
            Update::Prompts(_) => "prompts",
            Update::Generated { .. } => "generated",
            _ => "other",
        })
        .collect()
}

fn details(updates: &[Update]) -> mcbench_ui::BenchDetails {
    updates
        .iter()
        .rev()
        .find_map(|u| match u {
            Update::Bench { details: Ok(d), .. } => Some(d.clone()),
            _ => None,
        })
        .expect("details")
}

fn to_bench(text: &str, bench: &str, resolution: Resolution) -> Outgoing {
    Outgoing {
        target: Target::Bench(bench.into()),
        text: text.into(),
        attachments: Vec::new(),
        selections: Vec::new(),
        prompts: Vec::new(),
        effort: None,
        resolution,
        edit_of: None,
    }
}

#[test]
fn a_bench_is_read_with_its_rounds_branches_votes_and_stats() {
    let dir = TempDir::new("bench-read");
    let (core, inbox) = bench(&dir, &[]);
    core.send(Command::ReadBench {
        bench: "bench-castle".into(),
    });
    let updates = take(&inbox);
    assert_eq!(kinds(&updates), ["bench"]);
    let d = details(&updates);
    assert_eq!(d.rounds.len(), 3);
    assert_eq!(d.order.len(), 8);
    assert_eq!(
        d.rounds[1].branch.count, 2,
        "the second round was edited once"
    );
    assert_eq!(d.votes.len(), 1);
    assert!(d.blind && !d.revealed);
    let running = d
        .stats
        .iter()
        .find(|(m, _)| m == "castle-3")
        .expect("stats")
        .1[2];
    assert_eq!(running.ms, None, "a running round has no time yet");
}

#[test]
fn a_new_bench_runs_on_every_model_and_says_its_round() {
    let dir = TempDir::new("bench-new");
    let (core, inbox) = bench(&dir, &[]);
    let members = vec![
        BenchChoice {
            provider: "anthropic".into(),
            model: "claude-opus-5-5".into(),
            effort: None,
        },
        BenchChoice {
            provider: "openai".into(),
            model: "gpt-5.5".into(),
            effort: Some("high".into()),
        },
    ];
    let target = Target::NewBench {
        members: members.clone(),
        blind: true,
    };
    core.send(Command::Send(Outgoing {
        target: target.clone(),
        ..to_bench("A lighthouse", "x", Resolution::High)
    }));
    let updates = take(&inbox);
    // The list with it running, its details, the round to show; then its
    // models finish (at once under test): the list and the details again.
    assert_eq!(kinds(&updates), ["list", "bench", "sent", "list", "bench"]);
    let Update::BenchSent { bench, round, .. } = &updates[2] else {
        panic!("sent");
    };
    assert_eq!(*round, 1);
    assert!(bench.starts_with("bench-new-"));
    // Too few models are refused with the route's sentence.
    core.send(Command::Send(Outgoing {
        target: Target::NewBench {
            members: members[..1].to_vec(),
            blind: true,
        },
        ..to_bench("A lighthouse", "x", Resolution::Standard)
    }));
    assert!(
        matches!(&take(&inbox)[..], [Update::SendRefused { error, .. }] if error == "A bench runs 2 to 8 models")
    );
}

#[test]
fn a_round_keeps_its_bench_s_resolution_and_takes_no_effort() {
    let dir = TempDir::new("bench-resolution");
    let (core, inbox) = bench(&dir, &[]);
    core.send(Command::Send(to_bench(
        "A tower",
        "bench-houses",
        Resolution::Maximum,
    )));
    let updates = take(&inbox);
    assert!(
        matches!(&updates[..], [Update::SendRefused { error, .. }] if error == "resolution 10 is not this bench's (1)"),
        "{updates:?}"
    );
    core.send(Command::Send(Outgoing {
        effort: Some("high".into()),
        ..to_bench("A tower", "bench-houses", Resolution::Standard)
    }));
    assert!(
        matches!(&take(&inbox)[..], [Update::SendRefused { error, .. }] if error == "effort is not sent with a bench's round: each model has its own")
    );
    core.send(Command::Send(to_bench(
        "A tower",
        "bench-houses",
        Resolution::Standard,
    )));
    let updates = take(&inbox);
    assert_eq!(kinds(&updates), ["list", "bench", "sent", "list", "bench"]);
    assert!(matches!(updates[2], Update::BenchSent { round: 3, .. }));
}

#[test]
fn a_round_that_cannot_start_goes_back() {
    let dir = TempDir::new("bench-round-refused");
    let (core, inbox) = bench(&dir, &[BenchPlay::RoundRefused, BenchPlay::CreateRefused]);
    core.send(Command::Send(to_bench(
        "Again",
        "bench-houses",
        Resolution::Standard,
    )));
    assert!(
        matches!(&take(&inbox)[..], [Update::SendRefused { error, .. }] if error == standin::ROUND_REFUSED)
    );
    // An edit while a model runs is refused too.
    let (core, inbox) = bench(&TempDir::new("bench-edit-running"), &[]);
    core.send(Command::Send(Outgoing {
        edit_of: Some(MessageId("bench-castle-r3".into())),
        ..to_bench("Lanterns", "bench-castle", Resolution::Standard)
    }));
    assert!(
        matches!(&take(&inbox)[..], [Update::SendRefused { error, .. }] if error.contains("Stop the running models"))
    );
}

#[test]
fn votes_are_cast_changed_taken_back_and_refused() {
    let dir = TempDir::new("bench-vote");
    let (core, inbox) = bench(&dir, &[]);
    let vote = |round: &str, pick: VotePick| Command::Vote {
        bench: "bench-castle".into(),
        round: round.into(),
        pick,
    };
    core.send(vote("bench-castle-r2", VotePick::Member("castle-1".into())));
    let updates = take(&inbox);
    assert_eq!(kinds(&updates), ["bench", "leaderboard"]);
    assert_eq!(details(&updates).votes.len(), 2);
    core.send(vote("bench-castle-r2", VotePick::Tie));
    let d = details(&take(&inbox));
    assert_eq!(
        d.votes
            .iter()
            .find(|(r, _)| r == "bench-castle-r2")
            .expect("a vote")
            .1
            .pick,
        None
    );
    core.send(vote("bench-castle-r2", VotePick::TakeBack));
    assert_eq!(details(&take(&inbox)).votes.len(), 1);
    // The round a model still works on cannot be voted on.
    core.send(vote("bench-castle-r3", VotePick::Member("castle-1".into())));
    let updates = take(&inbox);
    assert_eq!(kinds(&updates), ["error", "bench"]);
    // A revealed bench takes no vote.
    core.send(Command::Vote {
        bench: "bench-houses".into(),
        round: "bench-houses-r2".into(),
        pick: VotePick::Tie,
    });
    assert!(
        matches!(&take(&inbox)[0], Update::Error(e) if e == "Votes are cast while the models' names are hidden")
    );
    let (core, inbox) = bench(&TempDir::new("bench-vote-fails"), &[BenchPlay::VoteFails]);
    core.send(vote("bench-castle-r2", VotePick::Tie));
    assert_eq!(kinds(&take(&inbox)), ["error", "bench"]);
}

#[test]
fn reveal_branch_retry_and_stop_end_in_the_list() {
    let dir = TempDir::new("bench-moves");
    let (core, inbox) = bench(&dir, &[]);
    core.send(Command::ReadBench {
        bench: "bench-castle".into(),
    });
    take(&inbox);
    // Stop the running model: the list, then its details (par-bench: a
    // stopped round has its time, `roundStats`).
    core.send(Command::Stop {
        chat: "castle-3".into(),
    });
    assert_eq!(kinds(&take(&inbox)), ["list", "bench"]);
    // Retry the failed one: the list, it runs and finishes.
    core.send(Command::RetryMember {
        bench: "bench-castle".into(),
        member: "castle-7".into(),
    });
    // (par-bench: the failed round's time goes as it runs again, so its
    // details change at the retry's first list too.)
    assert_eq!(kinds(&take(&inbox)), ["list", "bench", "list", "bench"]);
    // One that is not halted cannot be retried.
    core.send(Command::RetryMember {
        bench: "bench-castle".into(),
        member: "castle-1".into(),
    });
    assert_eq!(kinds(&take(&inbox)), ["error", "list"]);
    // The round's other version.
    core.send(Command::BenchBranch {
        bench: "bench-castle".into(),
        round: "bench-castle-r2-old".into(),
    });
    let updates = take(&inbox);
    assert_eq!(kinds(&updates), ["list", "bench"]);
    let d = details(&updates);
    assert_eq!(d.rounds.len(), 2);
    assert_eq!(d.rounds[1].id, "bench-castle-r2-old");
    core.send(Command::Reveal {
        bench: "bench-castle".into(),
    });
    let updates = take(&inbox);
    assert_eq!(kinds(&updates), ["list", "bench"]);
    assert!(details(&updates).revealed);
    let (core, inbox) = bench(
        &TempDir::new("bench-moves-fail"),
        &[
            BenchPlay::RevealFails,
            BenchPlay::BranchFails,
            BenchPlay::RetryFails,
        ],
    );
    core.send(Command::Reveal {
        bench: "bench-houses".into(),
    });
    assert_eq!(kinds(&take(&inbox)), ["error", "list", "bench"]);
    core.send(Command::BenchBranch {
        bench: "bench-houses".into(),
        round: "bench-houses-r1".into(),
    });
    assert_eq!(kinds(&take(&inbox)), ["error", "list", "bench"]);
    core.send(Command::RetryMember {
        bench: "bench-castle".into(),
        member: "castle-7".into(),
    });
    assert_eq!(kinds(&take(&inbox)), ["error", "list"]);
}

#[test]
fn a_fork_makes_a_build_chat_and_reveals_the_bench() {
    let dir = TempDir::new("bench-fork");
    let (core, inbox) = bench(&dir, &[]);
    core.send(Command::Fork {
        member: "castle-1".into(),
        round: 2,
    });
    let updates = take(&inbox);
    assert_eq!(kinds(&updates), ["list", "forked"]);
    let Update::Forked { chat: Ok(chat), .. } = &updates[1] else {
        panic!("forked");
    };
    let Update::Conversations(list) = &updates[0] else {
        panic!("a list");
    };
    assert!(
        list.conversations
            .iter()
            .any(|c| c.summary.id.0 == *chat && c.summary.bench_id.is_none())
    );
    assert!(
        list.benches
            .iter()
            .find(|b| b.id == "bench-castle")
            .expect("listed")
            .revealed
    );
    core.send(Command::Fork {
        member: "castle-3".into(),
        round: 3,
    });
    assert!(
        matches!(&take(&inbox)[..], [Update::Forked { chat: Err(e), .. }] if e == standin::FORK_WAIT)
    );
    let (core, inbox) = bench(&TempDir::new("bench-fork-fails"), &[BenchPlay::ForkFails]);
    core.send(Command::Fork {
        member: "castle-1".into(),
        round: 1,
    });
    assert!(
        matches!(&take(&inbox)[..], [Update::Forked { chat: Err(e), .. }] if e == standin::FORK_FAILED)
    );
}

#[test]
fn the_leaderboard_is_loaded_empty_failed_and_cleared() {
    let dir = TempDir::new("bench-board");
    let (core, inbox) = bench(&dir, &[]);
    core.send(Command::ReadLeaderboard);
    let updates = take(&inbox);
    let Update::Leaderboard(Ok(board)) = &updates[0] else {
        panic!("a leaderboard");
    };
    assert_eq!(board.entries.len(), 6);
    core.send(Command::ReadBench {
        bench: "bench-castle".into(),
    });
    take(&inbox);
    core.send(Command::ClearVotes);
    let updates = take(&inbox);
    assert_eq!(kinds(&updates), ["leaderboard", "bench"]);
    assert!(details(&updates).votes.is_empty());
    let Update::Leaderboard(Ok(board)) = &updates[0] else {
        panic!("a leaderboard");
    };
    assert!(board.entries.is_empty() && board.votes == 0);
    let (core, inbox) = bench(
        &TempDir::new("bench-board-empty"),
        &[BenchPlay::LeaderboardEmpty],
    );
    core.send(Command::ReadLeaderboard);
    assert!(matches!(&take(&inbox)[..], [Update::Leaderboard(Ok(b))] if b.entries.is_empty()));
    let (core, inbox) = bench(
        &TempDir::new("bench-board-fails"),
        &[BenchPlay::LeaderboardFails],
    );
    core.send(Command::ReadLeaderboard);
    assert_eq!(kinds(&take(&inbox)), ["leaderboard failed"]);
}

#[test]
fn the_library_loads_fails_saves_and_fails_to_save() {
    let dir = TempDir::new("bench-library");
    let (core, inbox) = bench(&dir, &[BenchPlay::PromptsFail]);
    core.send(Command::ReadPrompts);
    assert!(matches!(&take(&inbox)[..], [Update::Prompts(Err(e))] if e == standin::PROMPTS_FAILED));
    core.send(Command::ReadPrompts);
    let updates = take(&inbox);
    let [Update::Prompts(Ok(library))] = &updates[..] else {
        panic!("the library");
    };
    assert_eq!(library.len(), 6);
    let mut changed = library.clone();
    changed.pop();
    core.send(Command::SavePrompts(changed));
    assert!(matches!(&take(&inbox)[..], [Update::PromptsSaved(Ok(l))] if l.len() == 5));
    let (core, inbox) = bench(
        &TempDir::new("bench-library-fails"),
        &[BenchPlay::SaveFails],
    );
    core.send(Command::SavePrompts(Vec::new()));
    assert_eq!(kinds(&take(&inbox)), ["not saved"]);
}

#[test]
fn an_improve_succeeds_fails_and_is_cancelled() {
    let dir = TempDir::new("bench-generate");
    let (core, inbox) = bench(&dir, &[]);
    let ask = |request| Command::GeneratePrompt {
        request,
        provider: "anthropic".into(),
        model: "claude-opus-5-5".into(),
        effort: None,
        text: "A hut".into(),
    };
    core.send(ask(1));
    assert!(
        matches!(&take(&inbox)[..], [Update::Generated { request: 1, outcome: Generated::Text(t) }] if t.starts_with("A hut"))
    );
    let (core, inbox) = bench(
        &TempDir::new("bench-generate-fails"),
        &[BenchPlay::GenerateFails],
    );
    core.send(ask(2));
    assert!(matches!(
        &take(&inbox)[..],
        [Update::Generated {
            outcome: Generated::Failed(_),
            ..
        }]
    ));
    let (core, inbox) = bench(
        &TempDir::new("bench-generate-slow"),
        &[BenchPlay::GenerateSlow],
    );
    core.send(ask(3));
    core.send(Command::CancelGenerate { request: 3 });
    assert!(matches!(
        &take(&inbox)[..],
        [Update::Generated {
            request: 3,
            outcome: Generated::Cancelled
        }]
    ));
    // A second cancel, or one for a request that ended, publishes nothing.
    core.send(Command::CancelGenerate { request: 3 });
    assert!(take(&inbox).is_empty());
}

#[test]
fn a_model_s_edit_of_the_library_is_published_unasked() {
    let dir = TempDir::new("bench-model-edit");
    let (core, inbox) = bench(&dir, &[]);
    core.model_edits_library(mcbench_ui::LibraryPrompt {
        id: "prompt-added".into(),
        title: standin::ADDED_TITLE.into(),
        text: "A pyramid".into(),
        tags: Vec::new(),
        favorite: false,
        attachments: Vec::new(),
        used_at: None,
        added_by: None,
    });
    assert!(matches!(&take(&inbox)[..], [Update::Prompts(Ok(l))] if l.len() == 7));
}

#[test]
fn a_watched_bench_is_pushed_after_a_list_that_changed_it_until_it_is_deleted() {
    let dir = TempDir::new("bench-watched");
    let (core, inbox) = bench(&dir, &[]);
    // Not watched: a change publishes the list alone.
    core.send(Command::Stop {
        chat: "castle-3".into(),
    });
    assert_eq!(kinds(&take(&inbox)), ["list"]);
    core.send(Command::ReadBench {
        bench: "bench-castle".into(),
    });
    take(&inbox);
    // Watched: the list, then its changed details once.
    core.send(Command::RetryMember {
        bench: "bench-castle".into(),
        member: "castle-3".into(),
    });
    // (par-bench: the halted round's time goes with its retry, so the
    // details change at the retry's first list too.)
    assert_eq!(kinds(&take(&inbox)), ["list", "bench", "list", "bench"]);
    // A list that changes nothing of it publishes no details.
    core.send(Command::MarkSeen {
        chat: "chat-lamp".into(),
    });
    assert!(!kinds(&take(&inbox)).contains(&"bench"));
    // Deleted: no longer watched.
    core.send(Command::Archive(mcbench_ui::Subject::Bench(
        "bench-castle".into(),
    )));
    core.send(Command::Delete(mcbench_ui::Subject::Bench(
        "bench-castle".into(),
    )));
    take(&inbox);
    core.send(Command::MarkSeen {
        chat: "chat-lamp".into(),
    });
    assert!(!kinds(&take(&inbox)).contains(&"bench"));
}
