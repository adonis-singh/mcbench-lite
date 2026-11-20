//! When the window marks a chat seen (inventory `app.md` 2.7 and 7.7,
//! `web/app.js` 4043-4055, 4605-4609 and 5341-5344), with no GPUI types.
//! The page marks seen in three places, and so does the window, with
//! `Command::MarkSeen`:
//!
//! 1. The chat on screen is unread and the window is visible: checked
//!    again whenever the chat on screen, its `unread` or the window's
//!    visibility changes (the page's effect on `activeRow.id`,
//!    `activeRow.unread`, `pageVisible`): on opening it, when it turns
//!    unread while open, and when the window becomes visible again.
//! 2. A turn of the chat on screen ended (`turnEnded`) while the window is
//!    visible, whatever the list says yet.
//! 3. A bench's comparison is on screen and the window is visible: every
//!    unread model, checked again whenever the place, a model's `unread` or
//!    the visibility changes.
//!
//! Each case marks once per change. A chat marked is "asked" until a list
//! shows it read and not running (the mark's answer, or a list as good as
//! it), until it leaves the list, or until the list that follows an
//! `Update::Error` (the core answers a failed mark with the error, then the
//! unchanged list): no second mark goes out for it meanwhile, though a
//! list read before the mark landed still shows it unread. A failed mark is
//! not tried again until something changes, as the page's effect is not.

use std::collections::HashSet;

/// A chat's id and `unread`.
pub type Unread = (String, bool);

/// A bench's id and its models' (in the list's order).
pub type Comparison = (String, Vec<Unread>);

/// What is on screen, as the three cases read it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Look {
    /// Whether the window is visible (GPUI's `Window::is_visible`).
    pub visible: bool,
    /// The chat on screen, when the list has it: its id and `unread`.
    pub open: Option<Unread>,
    /// The bench whose comparison is on screen, with each of its models'
    /// ids and `unread` in the list's order.
    pub compare: Option<Comparison>,
}

/// A chat as a list shows it, for ending marks on their way.
#[derive(Clone, Copy, Debug)]
pub struct Listed<'a> {
    /// Its id.
    pub id: &'a str,
    /// `unread`.
    pub unread: bool,
    /// Whether a turn runs (`runningSince`).
    pub running: bool,
}

/// The marks the window has sent and what it last looked at.
#[derive(Debug, Default)]
pub struct Seen {
    asked: HashSet<String>,
    /// What cases 1 and 3 last saw, with the visibility.
    open: Option<(Option<Unread>, bool)>,
    compare: Option<(Option<Comparison>, bool)>,
    failed: bool,
}

impl Seen {
    /// The core published an error (`Update::Error`): the list that
    /// follows ends every mark on its way.
    pub fn error(&mut self) {
        self.failed = true;
    }

    /// A new list: the marks it answers end.
    pub fn listed<'a>(&mut self, rows: impl IntoIterator<Item = Listed<'a>>) {
        if std::mem::take(&mut self.failed) {
            self.asked.clear();
            return;
        }
        let unseen: HashSet<&str> = rows
            .into_iter()
            .filter(|r| r.unread || r.running)
            .map(|r| r.id)
            .collect();
        self.asked.retain(|id| unseen.contains(id.as_str()));
    }

    /// What is on screen now: the chats to mark seen (cases 1 and 3).
    pub fn look(&mut self, look: Look) -> Vec<String> {
        let mut marks = Vec::new();
        let open = (look.open.clone(), look.visible);
        if self.open.as_ref() != Some(&open) {
            self.open = Some(open);
            if let (Some((id, true)), true) = (look.open, look.visible) {
                marks.push(id);
            }
        }
        let compare = (look.compare.clone(), look.visible);
        if self.compare.as_ref() != Some(&compare) {
            self.compare = Some(compare);
            if let (Some((_, models)), true) = (look.compare, look.visible) {
                marks.extend(models.into_iter().filter(|(_, u)| *u).map(|(id, _)| id));
            }
        }
        marks.retain(|id| self.asked.insert(id.clone()));
        marks
    }

    /// A turn of the chat on screen ended (case 2): whether to mark it.
    pub fn turn_ended(&mut self, chat: &str, visible: bool) -> bool {
        visible && self.asked.insert(chat.to_string())
    }

    /// Whether a mark of `chat` is on its way.
    pub fn asked(&self, chat: &str) -> bool {
        self.asked.contains(chat)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn look(visible: bool, open: Option<(&str, bool)>) -> Look {
        Look {
            visible,
            open: open.map(|(id, u)| (id.to_string(), u)),
            compare: None,
        }
    }

    fn row(id: &str, unread: bool, running: bool) -> Listed<'_> {
        Listed {
            id,
            unread,
            running,
        }
    }

    #[test]
    fn opening_an_unread_chat_marks_it_once_and_a_read_one_not_at_all() {
        let mut seen = Seen::default();
        assert_eq!(seen.look(look(true, Some(("a", true)))), ["a"]);
        // The same look again (a list read before the mark landed).
        seen.listed([row("a", true, false)]);
        assert!(seen.look(look(true, Some(("a", true)))).is_empty());
        // The answer.
        seen.listed([row("a", false, false)]);
        assert!(!seen.asked("a"));
        assert!(seen.look(look(true, Some(("a", false)))).is_empty());
        assert!(seen.look(look(true, Some(("b", false)))).is_empty());
    }

    #[test]
    fn the_open_chat_turning_unread_is_marked_again() {
        let mut seen = Seen::default();
        assert!(seen.look(look(true, Some(("a", false)))).is_empty());
        assert_eq!(seen.look(look(true, Some(("a", true)))), ["a"]);
        seen.listed([row("a", false, false)]);
        assert!(seen.look(look(true, Some(("a", false)))).is_empty());
        assert_eq!(seen.look(look(true, Some(("a", true)))), ["a"]);
    }

    #[test]
    fn a_hidden_window_marks_nothing_until_it_is_visible_again() {
        let mut seen = Seen::default();
        assert!(seen.look(look(false, Some(("a", true)))).is_empty());
        assert!(seen.look(look(false, Some(("a", true)))).is_empty());
        assert!(!seen.turn_ended("a", false));
        assert_eq!(seen.look(look(true, Some(("a", true)))), ["a"]);
    }

    #[test]
    fn a_turns_end_marks_once_and_the_list_after_it_does_not_mark_again() {
        let mut seen = Seen::default();
        assert!(seen.look(look(true, Some(("a", false)))).is_empty());
        assert!(seen.turn_ended("a", true));
        assert!(!seen.turn_ended("a", true));
        // A list read during the turn, then the turn's end before the mark
        // landed: no second mark.
        seen.listed([row("a", false, true)]);
        assert!(seen.look(look(true, Some(("a", false)))).is_empty());
        seen.listed([row("a", true, false)]);
        assert!(seen.look(look(true, Some(("a", true)))).is_empty());
        // The mark's answer.
        seen.listed([row("a", false, false)]);
        assert!(!seen.asked("a"));
    }

    #[test]
    fn a_failed_mark_is_tried_again_only_after_a_change() {
        let mut seen = Seen::default();
        assert_eq!(seen.look(look(true, Some(("a", true)))), ["a"]);
        seen.error();
        seen.listed([row("a", true, false)]);
        assert!(!seen.asked("a"));
        assert!(seen.look(look(true, Some(("a", true)))).is_empty());
        assert!(seen.look(look(false, Some(("a", true)))).is_empty());
        assert_eq!(seen.look(look(true, Some(("a", true)))), ["a"]);
    }

    #[test]
    fn a_chat_that_left_the_list_is_no_longer_asked() {
        let mut seen = Seen::default();
        assert_eq!(seen.look(look(true, Some(("a", true)))), ["a"]);
        seen.listed([row("b", true, false)]);
        assert!(!seen.asked("a"));
    }

    #[test]
    fn the_comparison_marks_every_unread_model_once() {
        let mut seen = Seen::default();
        let compare = |visible, models: &[(&str, bool)]| Look {
            visible,
            open: None,
            compare: Some((
                "bench".into(),
                models.iter().map(|(id, u)| (id.to_string(), *u)).collect(),
            )),
        };
        assert!(
            seen.look(compare(false, &[("m1", true), ("m2", true)]))
                .is_empty()
        );
        assert_eq!(
            seen.look(compare(true, &[("m1", true), ("m2", false), ("m3", true)])),
            ["m1", "m3"]
        );
        assert!(
            seen.look(compare(true, &[("m1", true), ("m2", false), ("m3", true)]))
                .is_empty()
        );
        // m2 finishes unseen while m1's mark is on its way.
        assert_eq!(
            seen.look(compare(true, &[("m1", true), ("m2", true), ("m3", true)])),
            ["m2"]
        );
        seen.listed([
            row("m1", false, false),
            row("m2", false, false),
            row("m3", false, false),
        ]);
        assert!(
            seen.look(compare(
                true,
                &[("m1", false), ("m2", false), ("m3", false)]
            ))
            .is_empty()
        );
    }
}
