//! What the sidebar's rows show, with no GPUI types (inventory `app.md` 4.4,
//! 4.5, 4.7, 4.8, 7.11; `components.md` K.5, L.2, L.4, L.6;
//! `web/app.js` 6147-6311): each row's status and its label, the arrival
//! of a finished dot, the other mode's mark on the mode switcher, a search
//! snippet trimmed for the narrow sidebar, the successor of a row that left
//! the list, and a bench model's name.

use std::collections::HashMap;

use super::place::Mode;
use crate::{ListedBench, ListedChat};

/// A row's status dot (`.conv-dot`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Dot {
    /// `running`: a turn runs; the dot pulses.
    Running,
    /// `unread`: finished and not looked at; solid blue.
    Unread,
    /// `halted`: a bench model stopped or failed; hollow.
    Halted,
    /// `idle`: hollow.
    Idle,
}

impl Dot {
    /// What a reader hears (`DOT_LABEL`).
    pub fn label(self) -> &'static str {
        match self {
            Dot::Running => "Working",
            Dot::Unread => "Finished, not opened yet",
            Dot::Halted => "Stopped or failed",
            Dot::Idle => "Idle",
        }
    }
}

/// A Build chat's dot: running while its turn runs, unread when finished
/// and not looked at (the open chat is always looked at), else idle.
pub fn chat_dot(row: &ListedChat, open: Option<&str>) -> Dot {
    if row.running_since.is_some() {
        Dot::Running
    } else if row.summary.unread && open != Some(row.summary.id.0.as_str()) {
        Dot::Unread
    } else {
        Dot::Idle
    }
}

/// A bench model's dot (`memberDot`): running, halted when its turn failed
/// or was stopped, unread when not the open chat, else idle.
pub fn member_dot(row: &ListedChat, open: Option<&str>) -> Dot {
    if row.running_since.is_some() {
        Dot::Running
    } else if row.progress.is_some_and(|(_, halted)| halted.is_some()) {
        Dot::Halted
    } else if row.summary.unread && open != Some(row.summary.id.0.as_str()) {
        Dot::Unread
    } else {
        Dot::Idle
    }
}

/// A bench row's dot: the first of running and unread among its models'.
pub fn bench_dot(members: &[Dot]) -> Option<Dot> {
    [Dot::Running, Dot::Unread]
        .into_iter()
        .find(|d| members.contains(d))
}

/// A bench's models, in its order (`order`).
pub fn members<'a>(bench: &ListedBench, rows: &'a [ListedChat]) -> Vec<&'a ListedChat> {
    let mut members: Vec<&ListedChat> = rows
        .iter()
        .filter(|r| r.summary.bench_id.as_deref() == Some(bench.id.as_str()))
        .collect();
    let at = |id: &str| bench.order.iter().position(|o| o == id);
    members.sort_by_key(|m| at(&m.summary.id.0));
    members
}

/// The other mode's mark on the switcher: how many of its items finished
/// unseen, and its dot (unread if any did, else running if any runs). A
/// bench is unread only once none of its models runs.
pub fn other_mode(
    mode: Mode,
    rows: &[ListedChat],
    benches: &[ListedBench],
) -> (usize, Option<Dot>) {
    let states: Vec<Option<Dot>> = match mode.other() {
        Mode::Bench => benches
            .iter()
            .map(|b| {
                let m = members(b, rows);
                if m.iter().any(|r| r.running_since.is_some()) {
                    Some(Dot::Running)
                } else if m.iter().any(|r| r.summary.unread) {
                    Some(Dot::Unread)
                } else {
                    None
                }
            })
            .collect(),
        Mode::Build => rows
            .iter()
            .filter(|r| r.summary.bench_id.is_none())
            .map(|r| {
                if r.running_since.is_some() {
                    Some(Dot::Running)
                } else if r.summary.unread {
                    Some(Dot::Unread)
                } else {
                    None
                }
            })
            .collect(),
    };
    let waiting = states.iter().filter(|s| **s == Some(Dot::Unread)).count();
    let dot = if waiting > 0 {
        Some(Dot::Unread)
    } else if states.contains(&Some(Dot::Running)) {
        Some(Dot::Running)
    } else {
        None
    };
    (waiting, dot)
}

/// The mode switcher's title (`ModeSwitcher`): `Switch to Bench`, then
/// ` · <n> finished` when the other mode has finished items, else
/// ` · working` when it has a dot, then the shortcut when it has one.
pub fn switch_title(mode: Mode, waiting: usize, dot: Option<Dot>, shortcut: &str) -> String {
    let other = match mode.other() {
        Mode::Build => "Build",
        Mode::Bench => "Bench",
    };
    let mark = if waiting > 0 {
        format!(" \u{b7} {waiting} finished")
    } else if dot.is_some() {
        " \u{b7} working".to_string()
    } else {
        String::new()
    };
    let bind = if shortcut.is_empty() {
        String::new()
    } else {
        format!(" ({shortcut})")
    };
    format!("Switch to {other}{mark}{bind}")
}

/// The dots' history (`dotClass`'s `dotHistory`): a dot that was running
/// and is now unread has arrived (it eases from grey into blue), and stays
/// arrived while it stays unread; one already unread when it first appears
/// simply is blue. Each arrival has its own number, so the easing plays
/// once per arrival.
#[derive(Default)]
pub struct DotHistory(HashMap<String, (Dot, bool, u32)>);

impl DotHistory {
    /// Records `key`'s dot as `dot`; whether it arrived, and the arrival's
    /// number.
    pub fn arrival(&mut self, key: &str, dot: Dot) -> Option<u32> {
        let seen = self.0.get(key).copied();
        let from_running = matches!(seen, Some((Dot::Running, ..)));
        let arrived =
            dot == Dot::Unread && (from_running || matches!(seen, Some((Dot::Unread, true, _))));
        let number = seen.map_or(0, |(.., n)| n) + u32::from(dot == Dot::Unread && from_running);
        self.0.insert(key.to_string(), (dot, arrived, number));
        arrived.then_some(number)
    }

    /// Records `key`'s dot as `dot`; whether it arrived.
    pub fn arrived(&mut self, key: &str, dot: Dot) -> bool {
        self.arrival(key, dot).is_some()
    }
}

/// A search snippet as the narrow sidebar shows it (`marked`): when the
/// first match starts more than 18 characters in, it starts after the space
/// at least 12 characters before the match, with a leading `…`. Counted in
/// UTF-16 units, as the page's `indexOf` counts.
pub fn sidebar_snippet(snippet: &str) -> String {
    let units: Vec<u16> = snippet.encode_utf16().collect();
    let Some(first) = units.iter().position(|u| *u == 1) else {
        return snippet.to_string();
    };
    if first <= 18 {
        return snippet.to_string();
    }
    let from = units[..=first - 12]
        .iter()
        .rposition(|u| *u == u16::from(b' '));
    match from {
        Some(from) if from > 0 => format!(
            "\u{2026}{}",
            String::from_utf16(&units[from + 1..]).expect("cut at a space")
        ),
        _ => snippet.to_string(),
    }
}

/// What takes the place of a row that left the list (`successor`): the next
/// one below it in `before` still listed, or the nearest above.
pub fn successor<'a>(before: &'a [String], listed: &[String], id: &str) -> Option<&'a String> {
    let at = before.iter().position(|r| r == id)?;
    before[at + 1..]
        .iter()
        .find(|r| listed.contains(r))
        .or_else(|| before[..at].iter().rev().find(|r| listed.contains(r)))
}

/// A blind bench that is not revealed hides which model is which
/// (`hidesNames`).
pub fn hides_names(bench: &ListedBench) -> bool {
    bench.blind && !bench.revealed
}

/// A model's letter (`benchLetter`): `A` plus its index in the order.
pub fn letter(bench: &ListedBench, member: &str) -> char {
    let index = bench
        .order
        .iter()
        .position(|m| m == member)
        .unwrap_or_else(|| panic!("{member} is not in bench {}'s order", bench.id));
    (b'A' + index as u8) as char
}

/// A bench model's name (`nameOf`): `Build <letter>` while the bench hides
/// names, else its model's short name, with ` · <effort>` (or ` · default`)
/// when the bench runs the same provider and model more than once
/// (`memberName`).
pub fn name_of(row: &ListedChat, bench: &ListedBench, rows: &[ListedChat]) -> String {
    if hides_names(bench) {
        return format!("Build {}", letter(bench, &row.summary.id.0));
    }
    let s = &row.summary;
    let short = super::filters::model_label(&s.model);
    let twin = rows.iter().any(|o| {
        o.summary.id != s.id
            && o.summary.bench_id == s.bench_id
            && o.summary.provider == s.provider
            && o.summary.model == s.model
    });
    if twin {
        format!(
            "{short} \u{b7} {}",
            s.effort
                .as_deref()
                .filter(|e| !e.is_empty())
                .unwrap_or("default")
        )
    } else {
        short.to_string()
    }
}

/// A row's accessible name: what it shows, then `open` on the open chat,
/// bench or model (the page marks it only by its `active` class, so the
/// word is the app's), then its dot's label, as a reader hears the page's
/// row (the dot's label, the title). The open row is also AccessKit's
/// current one (`aria-current`); it is not the list's focused row, which
/// walking moves without opening anything.
pub fn row_name(title: &str, open: bool, dot: Option<Dot>) -> String {
    let mut name = title.to_string();
    if open {
        name.push_str(", open");
    }
    if let Some(dot) = dot {
        name = format!("{name}, {}", dot.label());
    }
    name
}

#[cfg(test)]
mod tests {
    use super::*;
    use mcbench_store::{ConversationId, ConversationSummary};

    fn row(id: &str, bench: Option<&str>) -> ListedChat {
        ListedChat {
            summary: ConversationSummary {
                id: ConversationId(id.into()),
                title: id.into(),
                provider: "anthropic".into(),
                model: "vendor/claude".into(),
                effort: None,
                resolution: 1,
                created_at: String::new(),
                bench_id: bench.map(str::to_string),
                bench_slot: None,
                unread: false,
                message_count: 1,
                prompts: Vec::new(),
            },
            progress: bench.map(|_| (1, None)),
            running_since: None,
        }
    }

    fn bench(id: &str, order: &[&str], blind: bool) -> ListedBench {
        ListedBench {
            id: id.into(),
            title: id.into(),
            created_at: String::new(),
            head: None,
            resolution: Some(1),
            rounds: 1,
            prompts: Vec::new(),
            blind,
            revealed: false,
            order: order.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn a_chat_is_running_unread_unless_open_or_idle() {
        let mut r = row("a", None);
        assert_eq!(chat_dot(&r, None), Dot::Idle);
        r.summary.unread = true;
        assert_eq!(chat_dot(&r, None), Dot::Unread);
        assert_eq!(
            chat_dot(&r, Some("a")),
            Dot::Idle,
            "the open chat is looked at"
        );
        r.running_since = Some(1);
        assert_eq!(chat_dot(&r, Some("a")), Dot::Running);
        assert_eq!(Dot::Unread.label(), "Finished, not opened yet");
    }

    #[test]
    fn a_bench_model_that_stopped_or_failed_is_halted_and_a_bench_shows_running_before_unread() {
        let mut m = row("m", Some("b"));
        m.progress = Some((1, Some("failed")));
        assert_eq!(member_dot(&m, None), Dot::Halted);
        m.running_since = Some(5);
        assert_eq!(member_dot(&m, None), Dot::Running);
        assert_eq!(bench_dot(&[Dot::Unread, Dot::Running]), Some(Dot::Running));
        assert_eq!(bench_dot(&[Dot::Idle, Dot::Halted]), None);
    }

    #[test]
    fn the_switch_counts_the_other_modes_finished_items_and_says_working() {
        let mut a = row("a", None);
        a.summary.unread = true;
        let mut b = row("b", None);
        b.running_since = Some(1);
        let rows = vec![a, b];
        assert_eq!(other_mode(Mode::Bench, &rows, &[]), (1, Some(Dot::Unread)));
        assert_eq!(
            switch_title(Mode::Bench, 1, Some(Dot::Unread), "\u{2318}1"),
            "Switch to Build \u{b7} 1 finished (\u{2318}1)"
        );
        let mut m = row("m", Some("t"));
        m.running_since = Some(1);
        let mut n = row("n", Some("t"));
        n.summary.unread = true;
        let rows = vec![m, n];
        let benches = vec![bench("t", &["m", "n"], false)];
        assert_eq!(
            other_mode(Mode::Build, &rows, &benches),
            (0, Some(Dot::Running)),
            "a bench is unread only once none of its models runs"
        );
        assert_eq!(
            switch_title(Mode::Build, 0, Some(Dot::Running), ""),
            "Switch to Bench \u{b7} working"
        );
    }

    #[test]
    fn a_dot_that_was_working_arrives_and_one_first_seen_unread_does_not() {
        let mut history = DotHistory::default();
        assert!(!history.arrived("a", Dot::Unread));
        assert!(!history.arrived("b", Dot::Running));
        assert!(history.arrived("b", Dot::Unread));
        assert!(
            history.arrived("b", Dot::Unread),
            "it stays arrived while unread"
        );
        assert!(!history.arrived("b", Dot::Idle));
        assert!(!history.arrived("b", Dot::Unread));
        assert!(!history.arrived("c", Dot::Running));
        let first = history.arrival("c", Dot::Unread);
        assert_eq!(history.arrival("c", Dot::Unread), first, "the same arrival");
        history.arrival("c", Dot::Running);
        assert_ne!(
            history.arrival("c", Dot::Unread),
            first,
            "a new arrival plays again"
        );
    }

    #[test]
    fn a_snippet_starts_a_few_words_before_a_late_first_match() {
        assert_eq!(sidebar_snippet("the \u{1}gate\u{2}"), "the \u{1}gate\u{2}");
        let long = "two towers with an iron bar \u{1}portcullis\u{2} between";
        assert_eq!(
            sidebar_snippet(long),
            "\u{2026}an iron bar \u{1}portcullis\u{2} between"
        );
    }

    #[test]
    fn the_next_row_below_takes_the_place_of_one_that_left_else_the_nearest_above() {
        let s = |l: &[&str]| l.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        let before = s(&["a", "b", "c", "d"]);
        assert_eq!(
            successor(&before, &s(&["a", "c", "d"]), "b").map(String::as_str),
            Some("c")
        );
        assert_eq!(
            successor(&before, &s(&["a", "d"]), "b").map(String::as_str),
            Some("d")
        );
        assert_eq!(
            successor(&before, &s(&["a", "b"]), "d").map(String::as_str),
            Some("b")
        );
        assert_eq!(successor(&before, &s(&[]), "d"), None);
    }

    #[test]
    fn a_bench_model_goes_by_its_letter_while_names_are_hidden_and_by_its_effort_beside_a_twin() {
        let blind = bench("t", &["m2", "m1"], true);
        let rows = vec![row("m1", Some("t")), row("m2", Some("t"))];
        assert_eq!(name_of(&rows[0], &blind, &rows), "Build B");
        let open = bench("t", &["m2", "m1"], false);
        let mut rows = rows;
        rows[1].summary.effort = Some("high".into());
        assert_eq!(name_of(&rows[0], &open, &rows), "claude \u{b7} default");
        assert_eq!(name_of(&rows[1], &open, &rows), "claude \u{b7} high");
        rows[1].summary.model = "other".into();
        assert_eq!(name_of(&rows[0], &open, &rows), "claude");
    }
}
