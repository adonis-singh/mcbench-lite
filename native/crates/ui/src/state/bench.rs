//! Bench's rules with no GPUI types (inventory `components.md` L.1 to L.6,
//! `app.md` 7; `web/app.js` 2728-2791, 3004-3059, 5157-5309, 6313-6428,
//! 6812-6894): where a model stands on a round, its name and letter, when
//! a round can be voted on and what its vote shows, the round a fork takes,
//! the new bench's line-up (2 to 8 models, a model at another effort), and
//! the leaderboard's cells.

use crate::{BenchChoice, BenchDetails, RoundStats, Vote};

/// The fewest models a bench runs.
pub const MIN_MODELS: usize = 2;
/// The most models a bench runs.
pub const MAX_MODELS: usize = 8;

/// Where one model stands on one round (`roundState`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoundState {
    /// `done`.
    Done,
    /// `running`.
    Running,
    /// `failed`.
    Failed,
    /// `stopped`.
    Stopped,
    /// `queued`.
    Queued,
    /// `held`: a failed or stopped turn holds the rest.
    Held,
}

/// How a member's last turn stopped short (`halted`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Halt {
    /// `failed`.
    Failed,
    /// `stopped`.
    Stopped,
}

impl Halt {
    /// The list's word for it.
    pub fn of(word: &str) -> Halt {
        match word {
            "failed" => Halt::Failed,
            "stopped" => Halt::Stopped,
            other => panic!("a bench model halts failed or stopped, not {other}"),
        }
    }
}

/// A member as the list has it: rounds sent, whether a turn runs, its halt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Progress {
    /// How many rounds it was sent (`round`).
    pub round: usize,
    /// A turn runs (`runningSince`).
    pub running: bool,
    /// `halted`.
    pub halted: Option<Halt>,
}

/// Where `member` stands on round `round` (from 1): earlier rounds done; the
/// current one running, halted or done; later ones held when halted, else
/// queued.
pub fn round_state(member: Progress, round: usize) -> RoundState {
    use std::cmp::Ordering::*;
    match round.cmp(&member.round) {
        Less => RoundState::Done,
        Equal if member.running => RoundState::Running,
        Equal => match member.halted {
            Some(Halt::Failed) => RoundState::Failed,
            Some(Halt::Stopped) => RoundState::Stopped,
            None => RoundState::Done,
        },
        Greater if member.halted.is_some() => RoundState::Held,
        Greater => RoundState::Queued,
    }
}

/// A blind bench that is not revealed hides which model is which
/// (`hidesNames`).
pub fn hides_names(blind: bool, revealed: bool) -> bool {
    blind && !revealed
}

/// A member's letter (`benchLetter`): `A` plus its place in `order`.
pub fn letter(order: &[String], member: &str) -> char {
    let at = order
        .iter()
        .position(|m| m == member)
        .unwrap_or_else(|| panic!("the member {member} is in its bench's order"));
    char::from(b'A' + at as u8)
}

/// `Build <letter>`: a member's name while the names are hidden.
pub fn build_name(letter: char) -> String {
    format!("Build {letter}")
}

/// A member's name (`memberName`): its model's short name, and its effort
/// (`default` when none) when the bench runs the same model more than once.
pub fn member_name(model: &str, effort: Option<&str>, twin: bool) -> String {
    let label = super::models::model_label(model);
    if twin {
        format!(
            "{label} \u{b7} {}",
            effort.filter(|e| !e.is_empty()).unwrap_or("default")
        )
    } else {
        label.to_string()
    }
}

/// Whether a round can be voted on: names hidden, no member running or
/// queued on it, two or more done.
pub fn votable(hidden: bool, states: &[RoundState]) -> bool {
    hidden
        && !states
            .iter()
            .any(|s| matches!(s, RoundState::Running | RoundState::Queued))
        && states.iter().filter(|s| **s == RoundState::Done).count() >= 2
}

/// The vote on a round that shows: only a blind one.
pub fn shown_vote<'a>(details: &'a BenchDetails, round: &str) -> Option<&'a Vote> {
    details
        .votes
        .iter()
        .find(|(id, vote)| id == round && vote.blind)
        .map(|(_, vote)| vote)
}

/// A member's stats on round `index` (from 0).
pub fn stats(details: &BenchDetails, member: &str, index: usize) -> Option<RoundStats> {
    details
        .stats
        .iter()
        .find(|(id, _)| id == member)
        .and_then(|(_, rounds)| rounds.get(index).copied())
}

/// The round a fork continues from: the member's last finished round
/// (`round - 1` while it runs).
pub fn fork_round(member: Progress) -> usize {
    if member.running {
        member.round.saturating_sub(1)
    } else {
        member.round
    }
}

/// A row's token stat: input plus output, `<n> tok`, and its title
/// `<in> in · <out> out`; none with no tokens.
pub fn tokens(stats: &RoundStats) -> Option<(String, String)> {
    let input = stats.input_tokens.unwrap_or(0);
    let output = stats.output_tokens.unwrap_or(0);
    let total = input + output;
    (total > 0).then(|| {
        (
            format!("{} tok", super::meters::format_tokens(total as f64)),
            format!(
                "{} in \u{b7} {} out",
                super::meters::format_tokens(input as f64),
                super::meters::format_tokens(output as f64)
            ),
        )
    })
}

/// The line-up's model picker (`onToggle`): a chosen provider and model is
/// removed (every effort of it); another is added with the default effort
/// while fewer than 8 are chosen.
pub fn toggle(members: &[BenchChoice], provider: &str, model: &str) -> Vec<BenchChoice> {
    if members
        .iter()
        .any(|m| m.provider == provider && m.model == model)
    {
        return members
            .iter()
            .filter(|m| !(m.provider == provider && m.model == model))
            .cloned()
            .collect();
    }
    let mut members = members.to_vec();
    if members.len() < MAX_MODELS {
        members.push(BenchChoice {
            provider: provider.into(),
            model: model.into(),
            effort: None,
        });
    }
    members
}

/// `Also run <name> at <effort>` (`onCopy`): while fewer than 8, the chip at
/// `index` becomes two, the first at `shown` (its effort as shown), the
/// second at `value`.
pub fn copy(members: &[BenchChoice], index: usize, value: &str, shown: &str) -> Vec<BenchChoice> {
    if members.len() >= MAX_MODELS {
        return members.to_vec();
    }
    let entry = &members[index];
    let at = |effort: &str| BenchChoice {
        effort: Some(effort.to_string()),
        ..entry.clone()
    };
    let mut out = members[..index].to_vec();
    out.push(at(shown));
    out.push(at(value));
    out.extend_from_slice(&members[index + 1..]);
    out
}

/// The efforts already chosen for a provider and model (`taken`), `''` read
/// as the model's default.
pub fn taken(members: &[BenchChoice], provider: &str, model: &str, default: &str) -> Vec<String> {
    members
        .iter()
        .filter(|m| m.provider == provider && m.model == model)
        .map(|m| {
            m.effort
                .clone()
                .filter(|e| !e.is_empty())
                .unwrap_or_else(|| default.to_string())
        })
        .collect()
}

/// The new bench's send: `Run on <n> models`.
pub fn run_label(count: usize) -> String {
    format!("Run on {count} models")
}

/// A leaderboard row's `Picked` cell and its title.
pub fn picked(wins: i64, votes: i64) -> (String, String) {
    (
        format!("{wins} / {votes}"),
        format!("Picked as best in {wins} of the {votes} votes it was in"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member(round: usize, running: bool, halted: Option<Halt>) -> Progress {
        Progress {
            round,
            running,
            halted,
        }
    }

    #[test]
    fn a_model_is_done_on_earlier_rounds_running_or_halted_on_its_last_and_queued_or_held_after() {
        let running = member(2, true, None);
        assert_eq!(round_state(running, 1), RoundState::Done);
        assert_eq!(round_state(running, 2), RoundState::Running);
        assert_eq!(round_state(running, 3), RoundState::Queued);
        let failed = member(2, false, Some(Halt::Failed));
        assert_eq!(round_state(failed, 2), RoundState::Failed);
        assert_eq!(round_state(failed, 3), RoundState::Held);
        let stopped = member(1, false, Some(Halt::Stopped));
        assert_eq!(round_state(stopped, 1), RoundState::Stopped);
        assert_eq!(round_state(member(3, false, None), 3), RoundState::Done);
    }

    #[test]
    fn a_round_is_voted_on_while_names_are_hidden_nobody_works_on_it_and_two_finished() {
        use RoundState::*;
        assert!(votable(true, &[Done, Done, Failed]));
        assert!(!votable(false, &[Done, Done]));
        assert!(!votable(true, &[Done, Done, Running]));
        assert!(!votable(true, &[Done, Done, Queued]));
        assert!(!votable(true, &[Done, Stopped, Held]));
    }

    #[test]
    fn a_member_is_named_by_its_model_and_by_its_effort_beside_a_twin() {
        assert_eq!(member_name("openai/gpt-5.5", None, false), "gpt-5.5");
        assert_eq!(
            member_name("gpt-5.5", Some("high"), true),
            "gpt-5.5 \u{b7} high"
        );
        assert_eq!(member_name("gpt-5.5", None, true), "gpt-5.5 \u{b7} default");
        let order = vec!["b".to_string(), "a".to_string()];
        assert_eq!(letter(&order, "a"), 'B');
        assert_eq!(build_name('A'), "Build A");
        assert!(hides_names(true, false) && !hides_names(true, true));
    }

    #[test]
    fn the_line_up_holds_two_to_eight_models_and_a_model_again_at_another_effort() {
        let mut members = Vec::new();
        for i in 0..9 {
            members = toggle(&members, "openrouter", &format!("m{i}"));
        }
        assert_eq!(members.len(), MAX_MODELS);
        members = toggle(&members, "openrouter", "m0");
        assert_eq!(members.len(), 7);
        let copied = copy(&members, 0, "high", "low");
        assert_eq!(copied.len(), 8);
        assert_eq!(copied[0].effort.as_deref(), Some("low"));
        assert_eq!(copied[1].effort.as_deref(), Some("high"));
        assert_eq!(copy(&copied, 0, "medium", "low").len(), 8);
        assert_eq!(
            taken(&copied, "openrouter", "m1", "medium"),
            vec!["low".to_string(), "high".to_string()]
        );
        // Removing a model removes every effort of it.
        assert_eq!(toggle(&copied, "openrouter", "m1").len(), 6);
    }

    #[test]
    fn a_fork_takes_the_last_finished_round() {
        assert_eq!(fork_round(member(3, true, None)), 2);
        assert_eq!(fork_round(member(3, false, None)), 3);
    }

    #[test]
    fn the_leaderboard_and_a_row_say_their_numbers() {
        assert_eq!(
            picked(3, 7),
            (
                "3 / 7".to_string(),
                "Picked as best in 3 of the 7 votes it was in".to_string()
            )
        );
        let stats = RoundStats {
            ms: Some(1),
            input_tokens: Some(12_000),
            output_tokens: Some(400),
        };
        assert_eq!(
            tokens(&stats),
            Some(("12k tok".to_string(), "12k in \u{b7} 400 out".to_string()))
        );
        assert_eq!(tokens(&RoundStats::default()), None);
        assert_eq!(run_label(3), "Run on 3 models");
    }
}
