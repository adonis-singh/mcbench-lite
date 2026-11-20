//! An anchored picker's open, closing and closed states (`usePopover(exitMs)`,
//! `web/app.js:1254-1301`, inventory `components.md` G.3), and a menu's
//! highlighted item.
//!
//! Opening is at once and cancels a close still animating. Closing marks
//! the popover closing (it plays its exit and takes no pointer input) and
//! closes it after its exit time; a second close while closing does nothing
//! more. Each picker has its own exit time: [`RESOLUTION_EXIT`],
//! [`MODEL_EXIT`], [`EFFORT_EXIT`] and [`MENU_EXIT`].

use std::time::{Duration, Instant};

/// The resolution picker's exit (110 ms).
pub const RESOLUTION_EXIT: Duration = Duration::from_millis(110);
/// The model picker's exit (120 ms, `surface-out`).
pub const MODEL_EXIT: Duration = Duration::from_millis(120);
/// The effort picker's exit (110 ms).
pub const EFFORT_EXIT: Duration = Duration::from_millis(110);
/// The tag, find-filter and bench effort menus' exit (110 ms,
/// `effort-menu-out`).
pub const MENU_EXIT: Duration = Duration::from_millis(110);

/// Where a popover is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Not shown.
    Closed,
    /// Shown, taking input.
    Open,
    /// Playing its exit since this instant; closed at `since + exit`.
    Closing(Instant),
}

/// A popover with its exit time.
#[derive(Clone, Copy, Debug)]
pub struct Popover {
    phase: Phase,
    exit: Duration,
}

impl Popover {
    /// A closed popover whose exit lasts `exit`.
    pub fn new(exit: Duration) -> Self {
        Popover {
            phase: Phase::Closed,
            exit,
        }
    }

    /// Where it is at `now` (a close whose exit has run is closed).
    pub fn phase(&self, now: Instant) -> Phase {
        match self.phase {
            Phase::Closing(since) if now >= since + self.exit => Phase::Closed,
            phase => phase,
        }
    }

    /// Shown (open or closing) at `now`.
    pub fn shown(&self, now: Instant) -> bool {
        self.phase(now) != Phase::Closed
    }

    /// Open: takes input.
    pub fn is_open(&self) -> bool {
        self.phase == Phase::Open
    }

    /// The exit time.
    pub fn exit(&self) -> Duration {
        self.exit
    }

    /// Opens at once, cancelling a close still animating (`setOpen(true)`).
    pub fn open(&mut self) {
        self.phase = Phase::Open;
    }

    /// Closes at once, with no exit (`setOpen(false)`).
    pub fn close_now(&mut self) {
        self.phase = Phase::Closed;
    }

    /// Starts the exit (`closePicker()`); returns whether it started (a
    /// popover already closing or closed does nothing more).
    pub fn close(&mut self, now: Instant) -> bool {
        match self.phase(now) {
            Phase::Open => {
                self.phase = Phase::Closing(now);
                true
            }
            _ => false,
        }
    }

    /// Opens a closed popover, starts closing an open one (a trigger's
    /// click).
    pub fn toggle(&mut self, now: Instant) {
        match self.phase(now) {
            Phase::Open => {
                self.close(now);
            }
            _ => self.open(),
        }
    }
}

/// The item a menu's arrow keys move to from `current` among `count`
/// items: Up and Down wrap round, Home and End go to the ends.
pub fn menu_step(current: Option<usize>, count: usize, step: super::Step) -> Option<usize> {
    use super::Step;
    if count == 0 {
        return None;
    }
    let last = count - 1;
    Some(match (current, step) {
        (_, Step::First) => 0,
        (_, Step::Last) => last,
        (None, Step::Next | Step::PageDown(_)) => 0,
        (None, Step::Previous | Step::PageUp(_)) => last,
        (Some(i), Step::Next | Step::PageDown(_)) => (i + 1) % count,
        (Some(i), Step::Previous | Step::PageUp(_)) => (i + last) % count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::Step;

    #[test]
    fn a_close_plays_its_exit_and_a_second_close_does_nothing_more() {
        let start = Instant::now();
        let mut popover = Popover::new(MENU_EXIT);
        assert_eq!(popover.phase(start), Phase::Closed);
        popover.open();
        assert!(popover.close(start));
        assert_eq!(popover.phase(start), Phase::Closing(start));
        assert!(!popover.close(start + Duration::from_millis(50)));
        assert_eq!(
            popover.phase(start + Duration::from_millis(50)),
            Phase::Closing(start)
        );
        assert_eq!(popover.phase(start + MENU_EXIT), Phase::Closed);
    }

    #[test]
    fn opening_cancels_a_close_still_animating() {
        let start = Instant::now();
        let mut popover = Popover::new(MODEL_EXIT);
        popover.open();
        popover.close(start);
        popover.toggle(start + Duration::from_millis(30));
        assert_eq!(popover.phase(start + Duration::from_secs(1)), Phase::Open);
        popover.toggle(start + Duration::from_secs(1));
        assert!(popover.shown(start + Duration::from_secs(1)));
        assert!(!popover.shown(start + Duration::from_secs(2)));
    }

    #[test]
    fn each_picker_has_the_exit_the_page_gives_it() {
        assert_eq!(RESOLUTION_EXIT.as_millis(), 110);
        assert_eq!(MODEL_EXIT.as_millis(), 120);
        assert_eq!(EFFORT_EXIT.as_millis(), 110);
        assert_eq!(MENU_EXIT.as_millis(), 110);
    }

    #[test]
    fn a_menus_arrows_wrap_round() {
        assert_eq!(menu_step(Some(2), 3, Step::Next), Some(0));
        assert_eq!(menu_step(Some(0), 3, Step::Previous), Some(2));
        assert_eq!(menu_step(None, 3, Step::Next), Some(0));
        assert_eq!(menu_step(Some(1), 3, Step::Last), Some(2));
        assert_eq!(menu_step(Some(1), 0, Step::Next), None);
    }
}
