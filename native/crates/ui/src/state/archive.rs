//! The sidebar's archived items, shown for one mode at a time
//! (`web/archive.mjs`, tests ported from `test/archive.test.js`, same cases
//! and names; inventory `app.md` 4.6 and 4.9, `components.md` M).
//!
//! One value: closed; loading (being read; what an earlier read of the same
//! mode gave stays on show meanwhile, or none); loaded (the core's
//! archive); failed (the read failed, with why, and it can be read again).
//! Any read can be followed by another, which supersedes it. A move the
//! table does not allow is an error (`The archive cannot go from <a> to
//! <b>`).

use super::place::Mode;
use crate::Archive;

/// Where the section is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Section {
    /// `closed`.
    Closed,
    /// `loading`: being read for `mode`.
    Loading {
        /// The mode.
        mode: Mode,
        /// An earlier read of the same mode, still shown.
        data: Option<Archive>,
    },
    /// `loaded`.
    Loaded {
        /// The mode.
        mode: Mode,
        /// The core's archive.
        data: Archive,
    },
    /// `failed`.
    Failed {
        /// The mode.
        mode: Mode,
        /// What an earlier read gave, still shown.
        data: Option<Archive>,
        /// Why.
        error: String,
    },
}

/// The section's starting value (`ARCHIVE_CLOSED`).
pub const ARCHIVE_CLOSED: Section = Section::Closed;

impl Section {
    /// Its phase's name.
    pub fn phase(&self) -> &'static str {
        match self {
            Section::Closed => "closed",
            Section::Loading { .. } => "loading",
            Section::Loaded { .. } => "loaded",
            Section::Failed { .. } => "failed",
        }
    }

    /// The mode it is open for.
    pub fn mode(&self) -> Option<Mode> {
        match self {
            Section::Closed => None,
            Section::Loading { mode, .. }
            | Section::Loaded { mode, .. }
            | Section::Failed { mode, .. } => Some(*mode),
        }
    }

    fn data(&self) -> Option<&Archive> {
        match self {
            Section::Closed => None,
            Section::Loading { data, .. } | Section::Failed { data, .. } => data.as_ref(),
            Section::Loaded { data, .. } => Some(data),
        }
    }

    fn to(&self, next: Section) -> Section {
        let allowed: &[&str] = match self.phase() {
            "closed" => &["loading"],
            "loading" => &["loading", "loaded", "failed", "closed"],
            _ => &["loading", "closed"],
        };
        assert!(
            allowed.contains(&next.phase()),
            "The archive cannot go from {} to {}",
            self.phase(),
            next.phase()
        );
        next
    }

    /// A read for `mode` (`archiveReading`): what was read before stays on
    /// show only in the same mode.
    pub fn reading(&self, mode: Mode) -> Section {
        let data = (self.mode() == Some(mode))
            .then(|| self.data().cloned())
            .flatten();
        self.to(Section::Loading { mode, data })
    }

    /// The read landed (`archiveRead`).
    pub fn read(&self, data: Archive) -> Section {
        let mode = self.mode().unwrap_or(Mode::Build);
        self.to(Section::Loaded { mode, data })
    }

    /// The read failed (`archiveFailed`).
    pub fn failed(&self, error: &str) -> Section {
        let mode = self.mode().unwrap_or(Mode::Build);
        self.to(Section::Failed {
            mode,
            data: self.data().cloned(),
            error: error.to_string(),
        })
    }

    /// Closed (`archiveClosed`).
    pub fn closed(&self) -> Section {
        self.to(Section::Closed)
    }
}

/// An archived row as the section lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArchivedRow {
    /// The chat's or bench's id.
    pub id: String,
    /// Its title.
    pub title: String,
    /// When it is deleted, epoch ms.
    pub expires_at: i64,
    /// A bench's model count.
    pub models: Option<i64>,
}

/// The rows of the section's mode, none before the first read
/// (`archivedRows`); a closed section shows no rows (an error).
pub fn archived_rows(section: &Section) -> Option<Vec<ArchivedRow>> {
    assert!(
        *section != Section::Closed,
        "A closed archive shows no rows"
    );
    let data = section.data()?;
    Some(match section.mode() {
        Some(Mode::Bench) => data
            .benches
            .iter()
            .map(|b| ArchivedRow {
                id: b.id.clone(),
                title: b.title.clone(),
                expires_at: b.expires_at,
                models: Some(b.models),
            })
            .collect(),
        _ => data
            .conversations
            .iter()
            .map(|c| ArchivedRow {
                id: c.id.clone(),
                title: c.title.clone(),
                expires_at: c.expires_at,
                models: None,
            })
            .collect(),
    })
}

const DAY: i64 = 24 * 60 * 60 * 1000;

/// How long an archived item has before it is deleted (`timeLeft`), in whole
/// days rounded up: one archived a moment ago has the 7 days its Archive
/// button says.
pub fn time_left(expires_at: i64, now: i64) -> String {
    let left = expires_at - now;
    if left < DAY {
        return "less than a day left".to_string();
    }
    let days = (left + DAY - 1) / DAY;
    if days == 1 {
        "1 day left".to_string()
    } else {
        format!("{days} days left")
    }
}

/// A row's meta text: for a bench `<n> model · ` or `<n> models · ` before
/// the time left; for a chat the time left.
pub fn meta(row: &ArchivedRow, now: i64) -> String {
    let left = time_left(row.expires_at, now);
    match row.models {
        Some(1) => format!("1 model \u{b7} {left}"),
        Some(n) => format!("{n} models \u{b7} {left}"),
        None => left,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ArchivedBench, ArchivedConversation};

    fn data() -> Archive {
        Archive {
            conversations: vec![ArchivedConversation {
                id: "c1".into(),
                title: "House".into(),
                provider: String::new(),
                model: String::new(),
                archived_at: 1,
                expires_at: 2,
            }],
            benches: vec![ArchivedBench {
                id: "b1".into(),
                title: "Tower".into(),
                archived_at: 1,
                expires_at: 2,
                models: 3,
            }],
        }
    }

    fn refused(f: impl FnOnce() -> Section + std::panic::UnwindSafe) -> String {
        let error = std::panic::catch_unwind(f).expect_err("refused");
        error
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| error.downcast_ref::<&str>().map(|s| s.to_string()))
            .expect("a message")
    }

    #[test]
    fn the_archive_opens_loading_lands_or_fails_can_be_read_again_and_closes() {
        let archive = ARCHIVE_CLOSED.reading(Mode::Build);
        assert_eq!(
            archive,
            Section::Loading {
                mode: Mode::Build,
                data: None
            }
        );
        assert_eq!(archived_rows(&archive), None);
        let archive = archive.failed("HTTP 500");
        assert_eq!(
            archive,
            Section::Failed {
                mode: Mode::Build,
                data: None,
                error: "HTTP 500".into()
            }
        );
        // Nothing lands on a failed read: "Try again" reads it first.
        let failed = archive.clone();
        assert!(refused(move || failed.read(data())).contains("cannot go from failed to loaded"));
        let archive = archive.reading(Mode::Build).read(data());
        assert_eq!(archive.phase(), "loaded");
        assert_eq!(archived_rows(&archive).expect("rows")[0].title, "House");
        let loaded = archive.clone();
        assert!(refused(move || loaded.failed("late")).contains("cannot go from loaded to failed"));
        let archive = archive.closed();
        assert_eq!(archive, ARCHIVE_CLOSED);
        let closed = archive.clone();
        assert!(refused(move || closed.read(data())).contains("cannot go from closed to loaded"));
        let closed = archive.clone();
        assert!(refused(move || closed.closed()).contains("cannot go from closed to closed"));
        let closed = archive.clone();
        let shown = std::panic::catch_unwind(move || archived_rows(&closed)).expect_err("refused");
        assert!(
            shown
                .downcast_ref::<&str>()
                .is_some_and(|m| m.contains("closed archive shows no rows"))
        );
    }

    #[test]
    fn a_read_again_keeps_the_rows_on_show_in_the_same_mode_not_in_the_other() {
        let loaded = ARCHIVE_CLOSED.reading(Mode::Bench).read(data());
        assert_eq!(archived_rows(&loaded).expect("rows")[0].title, "Tower");
        let again = loaded.reading(Mode::Bench);
        assert_eq!(archived_rows(&again).expect("rows")[0].title, "Tower");
        assert_eq!(
            archived_rows(&again.failed("offline")).expect("rows")[0].title,
            "Tower"
        );
        assert_eq!(archived_rows(&loaded.reading(Mode::Build)), None);
    }

    #[test]
    fn time_left_counts_whole_days_rounded_up_and_under_one_day_says_so() {
        let day = DAY;
        let now = 1_000_000;
        assert_eq!(
            time_left(now + 7 * day - 1, now),
            "7 days left",
            "archived a moment ago: the 7 days the Archive button says"
        );
        assert_eq!(time_left(now + 7 * day, now), "7 days left");
        assert_eq!(time_left(now + day + 5, now), "2 days left");
        assert_eq!(time_left(now + day, now), "1 day left");
        assert_eq!(time_left(now + day - 1, now), "less than a day left");
        assert_eq!(time_left(now - 5, now), "less than a day left");
    }

    #[test]
    fn a_bench_row_says_its_models_before_the_time_left() {
        let row = |models| ArchivedRow {
            id: "b".into(),
            title: "t".into(),
            expires_at: 2 * DAY,
            models,
        };
        assert_eq!(meta(&row(Some(1)), 0), "1 model \u{b7} 2 days left");
        assert_eq!(meta(&row(Some(3)), 0), "3 models \u{b7} 2 days left");
        assert_eq!(meta(&row(None), 0), "2 days left");
    }
}
