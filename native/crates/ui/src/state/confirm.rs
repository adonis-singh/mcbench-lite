//! `ConfirmButton`'s arming (`web/app.js:2825-2842`, inventory
//! `components.md` N.1): an action that cannot be undone asks first. The
//! first press arms it for the subject it was pressed on; a second press
//! within [`ARMED_FOR`] acts and disarms. The same button shown for another
//! subject asks again.

use std::time::{Duration, Instant};

/// How long a first press stays armed.
pub const ARMED_FOR: Duration = Duration::from_millis(4000);

/// What a press does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Press {
    /// It armed the button; nothing is done yet.
    Armed,
    /// It confirmed: the caller acts.
    Confirmed,
}

/// The armed subject, if any, and since when.
#[derive(Clone, Debug, Default)]
pub struct Confirm {
    armed: Option<(String, Instant)>,
}

impl Confirm {
    /// Whether the button shown for `subject` is armed at `now`.
    pub fn armed(&self, subject: &str, now: Instant) -> bool {
        matches!(&self.armed, Some((s, since)) if s == subject && now < *since + ARMED_FOR)
    }

    /// When the armed state ends (the caller redraws then), if armed.
    pub fn disarms_at(&self) -> Option<Instant> {
        self.armed.as_ref().map(|(_, since)| *since + ARMED_FOR)
    }

    /// A press on the button shown for `subject`.
    pub fn press(&mut self, subject: &str, now: Instant) -> Press {
        if self.armed(subject, now) {
            self.armed = None;
            Press::Confirmed
        } else {
            self.armed = Some((subject.to_string(), now));
            Press::Armed
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_press_arms_and_the_second_within_four_seconds_acts() {
        let t = Instant::now();
        let mut confirm = Confirm::default();
        assert_eq!(confirm.press("published:a", t), Press::Armed);
        assert!(confirm.armed("published:a", t + Duration::from_millis(3999)));
        assert_eq!(
            confirm.press("published:a", t + Duration::from_secs(3)),
            Press::Confirmed
        );
        assert!(!confirm.armed("published:a", t + Duration::from_secs(3)));
    }

    #[test]
    fn it_disarms_after_four_seconds_and_asks_again() {
        let t = Instant::now();
        let mut confirm = Confirm::default();
        confirm.press("s", t);
        assert!(!confirm.armed("s", t + ARMED_FOR));
        assert_eq!(confirm.press("s", t + ARMED_FOR), Press::Armed);
    }

    #[test]
    fn it_is_armed_per_subject() {
        let t = Instant::now();
        let mut confirm = Confirm::default();
        confirm.press("unshare:chat:1", t);
        assert!(!confirm.armed("unshare:chat:2", t));
        assert_eq!(confirm.press("unshare:chat:2", t), Press::Armed);
        assert!(!confirm.armed("unshare:chat:1", t));
    }
}
