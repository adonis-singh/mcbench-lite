//! The chat turn on screen, from the window's side (`web/transcript.mjs`'s
//! phases, `turnTo`, `turnRunning`, `turnOver`, `turnToJoin`). Whether the
//! core runs a turn is its list row's `running_since`; this is what the
//! window does about it:
//!
//! | Phase | Meaning |
//! |---|---|
//! | idle | no turn read; `ended` is a turn start the window saw end, so a list that still shows it running is stale |
//! | sending | a message handed to the core (`Command::Send`), not yet taken (`Update::TurnJoined`) or refused (`Update::SendRefused`) |
//! | reading | the window applies the turn's events; `status` is the harness's line, `since` the turn's start once known |
//! | detached | the core runs the turn (begun `started_at`) and the window does not read it; the chat stays busy until the list stops showing it running |
//!
//! The page's `waiting` phase (its stream broke mid-turn) is not here:
//! inside one process the turn's updates cannot break off (the core's death
//! is `Update::Fatal`). Nothing in the seam detaches a turn either (the page
//! detached on an event it could not apply, which here is a bug and panics,
//! or on a join the server refused); the phase and its rules are kept as
//! the page has them.

/// The turn's phase.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Turn {
    /// No turn read.
    Idle {
        /// The start of a turn the window saw end.
        ended: Option<i64>,
    },
    /// A message on its way to the core.
    Sending,
    /// The window reads the turn's events.
    Reading {
        /// The harness's status line.
        status: Option<String>,
        /// The turn's start (epoch ms), once the core said it.
        since: Option<i64>,
    },
    /// The core runs the turn and the window does not read it.
    Detached {
        /// The turn's start, when the list showed it.
        started_at: Option<i64>,
    },
}

/// No turn, none seen ending (`IDLE`).
pub const IDLE: Turn = Turn::Idle { ended: None };

impl Turn {
    fn phase(&self) -> &'static str {
        match self {
            Turn::Idle { .. } => "idle",
            Turn::Sending => "sending",
            Turn::Reading { .. } => "reading",
            Turn::Detached { .. } => "detached",
        }
    }
}

/// The turn after `turn`: `next`, if it can follow it (`turnTo`); any other
/// step is a bug. Leaving the chat goes to idle from any phase; a status
/// line is a reading turn going on reading.
pub fn turn_to(turn: &Turn, next: Turn) -> Turn {
    let allowed: &[&str] = match turn {
        Turn::Idle { .. } => &["idle", "sending", "reading"],
        Turn::Sending => &["reading", "idle"],
        Turn::Reading { .. } => &["reading", "detached", "idle"],
        Turn::Detached { .. } => &["idle"],
    };
    assert!(
        allowed.contains(&next.phase()),
        "The turn cannot go from {} to {}",
        turn.phase(),
        next.phase()
    );
    next
}

/// Whether the chat is busy (`turnRunning`): a turn is held, or the list
/// row (`running_since`) runs one the window did not see end.
pub fn turn_running(turn: &Turn, row_running_since: Option<i64>) -> bool {
    match turn {
        Turn::Idle { ended } => row_running_since.is_some_and(|since| Some(since) != *ended),
        _ => true,
    }
}

/// A detached turn is over once the list stops showing it running
/// (`turnOver`).
pub fn turn_over(turn: &Turn, row_running_since: Option<i64>) -> bool {
    match turn {
        Turn::Detached { started_at } => {
            !(row_running_since.is_some() && row_running_since == *started_at)
        }
        _ => false,
    }
}

/// The start of a turn the core runs that the window should join
/// (`turnToJoin`): idle, and the row runs a turn not seen ending.
pub fn turn_to_join(turn: &Turn, row_running_since: Option<i64>) -> Option<i64> {
    match turn {
        Turn::Idle { ended } => row_running_since.filter(|since| Some(*since) != *ended),
        _ => None,
    }
}

/// Elapsed time for the working line (`formatElapsed`): `42s` under a
/// minute, `3m 5s` from one on, no hours; a negative time is `0s`.
pub fn format_elapsed(ms: i64) -> String {
    let seconds = ms.max(0) / 1000;
    if seconds < 60 {
        return format!("{seconds}s");
    }
    format!("{}m {}s", seconds / 60, seconds % 60)
}

/// The working line (`Ticker`): `Working for <elapsed>`, or
/// `<status> · <elapsed>` with the harness's status.
pub fn working_line(status: Option<&str>, elapsed_ms: i64) -> String {
    match status {
        Some(status) => format!("{status} \u{b7} {}", format_elapsed(elapsed_ms)),
        None => format!("Working for {}", format_elapsed(elapsed_ms)),
    }
}

/// The working line's status while the turn is detached.
pub const DETACHED_STATUS: &str = "Still working, shown when it ends";

#[cfg(test)]
mod tests {
    //! `test/transcript.test.js`, the cases of the turn's phases (the
    //! grouping and event cases are in `state::transcript`'s tests).
    use super::*;

    fn reading(status: Option<&str>) -> Turn {
        Turn::Reading {
            status: status.map(str::to_string),
            since: None,
        }
    }

    #[test]
    fn the_page_joins_a_running_turn_only_while_it_holds_no_stream_and_has_not_seen_it_end() {
        assert_eq!(turn_to_join(&IDLE, Some(100)), Some(100));
        assert!(turn_running(&IDLE, Some(100)));
        assert_eq!(turn_to_join(&reading(None), Some(100)), None);
        assert!(turn_running(&Turn::Sending, None));
        // The list still shows a turn the window saw end: stale, not joined.
        let ended = Turn::Idle { ended: Some(100) };
        assert_eq!(turn_to_join(&ended, Some(100)), None);
        assert!(!turn_running(&ended, Some(100)));
        // A later turn in the same chat is.
        assert_eq!(turn_to_join(&ended, Some(200)), Some(200));
        assert!(!turn_running(&IDLE, None));
    }

    #[test]
    fn the_turn_moves_only_along_its_phase_table() {
        let detached = Turn::Detached {
            started_at: Some(100),
        };
        // A send, read to its end; a status line is reading going on reading.
        assert_eq!(turn_to(&IDLE, Turn::Sending), Turn::Sending);
        assert_eq!(turn_to(&Turn::Sending, reading(None)), reading(None));
        assert_eq!(
            turn_to(&reading(None), reading(Some("Retrying"))),
            reading(Some("Retrying"))
        );
        assert_eq!(
            turn_to(&reading(None), Turn::Idle { ended: Some(100) }),
            Turn::Idle { ended: Some(100) }
        );
        // A joined turn, or one the window stops reading.
        assert_eq!(turn_to(&IDLE, reading(None)), reading(None));
        assert_eq!(turn_to(&reading(None), detached.clone()), detached);
        // Leaving the chat goes to idle from every phase.
        for turn in [IDLE, Turn::Sending, reading(None), detached.clone()] {
            assert_eq!(turn_to(&turn, IDLE), IDLE);
        }
        // Nothing is sent or joined while a turn is held, and only a turn
        // being read can be detached from.
        for (from, to, text) in [
            (
                detached.clone(),
                Turn::Sending,
                "cannot go from detached to sending",
            ),
            (
                detached.clone(),
                reading(None),
                "cannot go from detached to reading",
            ),
            (
                reading(None),
                Turn::Sending,
                "cannot go from reading to sending",
            ),
            (
                Turn::Sending,
                Turn::Sending,
                "cannot go from sending to sending",
            ),
            (IDLE, detached.clone(), "cannot go from idle to detached"),
            (
                Turn::Sending,
                detached.clone(),
                "cannot go from sending to detached",
            ),
        ] {
            let failed = std::panic::catch_unwind(|| turn_to(&from, to.clone()))
                .expect_err("a step off the table panics");
            let message = failed.downcast_ref::<String>().cloned().unwrap_or_default();
            assert!(message.contains(text), "{message}");
        }
    }

    #[test]
    fn a_detached_turn_keeps_the_chat_busy_until_the_list_stops_showing_it_running() {
        let detached = Turn::Detached {
            started_at: Some(100),
        };
        assert!(turn_running(&detached, Some(100)));
        assert_eq!(turn_to_join(&detached, Some(100)), None);
        assert!(!turn_over(&detached, Some(100)));
        // The list says it ended, or shows a later turn of the chat.
        assert!(turn_over(&detached, None));
        assert!(turn_over(&detached, Some(200)));
        // A sent turn the list never showed is not waited for.
        assert!(turn_over(&Turn::Detached { started_at: None }, None));
        // Idle again, with that turn seen to end, a list that still shows it
        // is stale.
        let ended = Turn::Idle { ended: Some(100) };
        assert_eq!(turn_to_join(&ended, Some(100)), None);
        assert!(!turn_running(&ended, Some(100)));
        assert_eq!(turn_to_join(&ended, Some(200)), Some(200));
        for turn in [IDLE, reading(None), Turn::Sending] {
            assert!(!turn_over(&turn, None));
        }
    }

    #[test]
    fn the_working_line_reads_as_the_ticker() {
        assert_eq!(format_elapsed(-5), "0s");
        assert_eq!(format_elapsed(42_900), "42s");
        assert_eq!(format_elapsed(185_000), "3m 5s");
        assert_eq!(format_elapsed(7_200_000), "120m 0s");
        assert_eq!(working_line(None, 3_000), "Working for 3s");
        assert_eq!(
            working_line(Some("Compacting context"), 61_000),
            "Compacting context \u{b7} 1m 1s"
        );
    }
}
