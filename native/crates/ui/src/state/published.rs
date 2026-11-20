//! Settings' list of published builds (`web/published.mjs`, tests ported
//! from `test/published.test.js`, same cases and names; inventory
//! `components.md` N.3, `app.md` 8.2 and 8.9), read each time Settings
//! open (`Command::ReadShares`).
//!
//! One value: unread (not read since Settings last opened); loading (an
//! earlier read's data still shown, or none); loaded; failed (why). Each
//! keeps the share ids whose removal is on its way and why a build's last
//! removal failed. Any read can be followed by another, which supersedes
//! it.

use std::collections::BTreeMap;

use crate::{Published as Data, PublishedBuild};

/// The list's phase.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// `unread`.
    Unread,
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
            Phase::Unread => "unread",
            Phase::Loading => "loading",
            Phase::Loaded => "loaded",
            Phase::Failed => "failed",
        }
    }
}

/// The list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Published {
    /// Its phase.
    pub phase: Phase,
    /// The last read's data, when one landed.
    pub data: Option<Data>,
    /// The builds whose removal is on its way.
    pub removing: Vec<String>,
    /// Why a build's last removal failed.
    pub errors: BTreeMap<String, String>,
    /// Why the read failed (`failed` only).
    pub error: Option<String>,
}

/// `PUBLISHED_UNREAD`.
pub const UNREAD: Published = Published {
    phase: Phase::Unread,
    data: None,
    removing: Vec::new(),
    errors: BTreeMap::new(),
    error: None,
};

fn to(from: &Published, next: Published) -> Result<Published, String> {
    let allowed: &[Phase] = match from.phase {
        Phase::Unread => &[Phase::Loading],
        Phase::Loading => &[Phase::Loading, Phase::Loaded, Phase::Failed, Phase::Unread],
        Phase::Loaded | Phase::Failed => &[Phase::Loading, Phase::Unread],
    };
    if !allowed.contains(&next.phase) {
        return Err(format!(
            "The published builds cannot go from {} to {}",
            from.phase.name(),
            next.phase.name()
        ));
    }
    Ok(next)
}

/// A read starts (`publishedReading`): from unread with nothing, else
/// keeping what is shown.
pub fn reading(published: &Published) -> Result<Published, String> {
    let next = if published.phase == Phase::Unread {
        Published {
            phase: Phase::Loading,
            ..UNREAD
        }
    } else {
        Published {
            phase: Phase::Loading,
            error: None,
            ..published.clone()
        }
    };
    to(published, next)
}

/// A read landed (`publishedRead`).
pub fn read(published: &Published, data: Data) -> Result<Published, String> {
    to(
        published,
        Published {
            phase: Phase::Loaded,
            data: Some(data),
            error: None,
            ..published.clone()
        },
    )
}

/// A read failed (`publishedFailed`).
pub fn failed(published: &Published, error: &str) -> Result<Published, String> {
    to(
        published,
        Published {
            phase: Phase::Failed,
            error: Some(error.to_string()),
            ..published.clone()
        },
    )
}

/// Settings closed (`publishedClosed`).
pub fn closed(published: &Published) -> Result<Published, String> {
    to(published, UNREAD)
}

/// What the list shows (`publishedView`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    /// Before the first read lands.
    Loading,
    /// The read failed.
    Failed,
    /// No repository set.
    NoRepo,
    /// No builds.
    Empty,
    /// The builds.
    List,
}

/// `publishedView`.
pub fn view(published: &Published) -> Result<View, String> {
    if published.phase == Phase::Unread {
        return Err("Unread published builds show nothing".into());
    }
    if published.phase == Phase::Failed {
        return Ok(View::Failed);
    }
    let Some(data) = &published.data else {
        return Ok(View::Loading);
    };
    if data.repo.is_none() {
        return Ok(View::NoRepo);
    }
    Ok(if data.builds.is_empty() {
        View::Empty
    } else {
        View::List
    })
}

fn shown(published: &Published, share_id: &str) -> Result<(), String> {
    if published
        .data
        .as_ref()
        .is_some_and(|d| d.builds.iter().any(|b| b.share_id == share_id))
    {
        Ok(())
    } else {
        Err(format!("{share_id} is not a published build on show"))
    }
}

/// A removal starts, letting go of its last error (`publishedRemoving`).
pub fn removing(published: &Published, share_id: &str) -> Result<Published, String> {
    shown(published, share_id)?;
    if published.removing.iter().any(|r| r == share_id) {
        return Err(format!("{share_id} is already being removed"));
    }
    let mut next = published.clone();
    next.errors.remove(share_id);
    next.removing.push(share_id.to_string());
    Ok(next)
}

fn being_removed(published: &Published, share_id: &str) -> Result<(), String> {
    if published.removing.iter().any(|r| r == share_id) {
        Ok(())
    } else {
        Err(format!("{share_id} is not being removed"))
    }
}

/// The removal landed: the build leaves the list before the read again
/// (`publishedRemoved`).
pub fn removed(published: &Published, share_id: &str) -> Result<Published, String> {
    being_removed(published, share_id)?;
    let mut next = published.clone();
    if let Some(data) = next.data.as_mut() {
        data.builds.retain(|b| b.share_id != share_id);
    }
    next.removing.retain(|r| r != share_id);
    Ok(next)
}

/// The removal failed: the build stays, with why (`publishedKept`).
pub fn kept(published: &Published, share_id: &str, error: &str) -> Result<Published, String> {
    being_removed(published, share_id)?;
    let mut next = published.clone();
    next.removing.retain(|r| r != share_id);
    next.errors.insert(share_id.to_string(), error.to_string());
    Ok(next)
}

/// The chat or bench a build was made from (`publishedSubject`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Subject {
    /// A chat.
    Chat(String),
    /// A bench.
    Bench(String),
}

/// `publishedSubject`: the subject, or none once it was deleted; another
/// kind is an error.
pub fn subject(build: &PublishedBuild) -> Result<Option<Subject>, String> {
    match build.kind.as_str() {
        "chat" => Ok(build.conversation_id.clone().map(Subject::Chat)),
        "bench" => Ok(build.bench_id.clone().map(Subject::Bench)),
        other => Err(format!(
            "{other:?} is not a kind of published build (chat, bench)"
        )),
    }
}

/// How long ago `at` (ms) was, in its largest whole unit (`publishedAgo`).
pub fn ago(at: i64, now: i64) -> String {
    const UNITS: [(&str, i64); 5] = [
        ("year", 365 * 24 * 60 * 60),
        ("month", 30 * 24 * 60 * 60),
        ("day", 24 * 60 * 60),
        ("hour", 60 * 60),
        ("minute", 60),
    ];
    let seconds = ((now - at) / 1000).max(0);
    for (unit, size) in UNITS {
        let count = seconds / size;
        if count >= 1 {
            return format!("{count} {unit}{} ago", if count == 1 { "" } else { "s" });
        }
    }
    "just now".into()
}

/// The count line: `<n> build(s) at` (the repository follows as a link).
pub fn count_line(builds: usize) -> String {
    format!(
        "{builds} {} at",
        if builds == 1 { "build" } else { "builds" }
    )
}

/// A build's meta line (`<n> models` or the model, ` · published <ago>`,
/// then ` · <kind> deleted` or ` · archived`).
pub fn meta(build: &PublishedBuild, now: i64) -> String {
    let parts = meta_parts(build, now);
    format!(
        "{} \u{b7} published {}{}",
        parts.first, parts.ago, parts.tail
    )
}

// par-shell (N.3): the meta line's parts, for its hovers.
/// The meta line in its parts: the first part (a bench's model count, a
/// chat's model) with its hover (a bench's labels, comma-separated), the
/// `ago`, and the tail (`· <kind> deleted`, `· archived`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MetaParts {
    /// `3 models`, or the chat's model.
    pub first: String,
    /// A bench's labels (`title=${build.labels.join(', ')}`).
    pub first_tip: Option<String>,
    /// `2h ago`.
    pub ago: String,
    /// What follows the `ago`.
    pub tail: String,
}

/// [`meta`] in its parts.
pub fn meta_parts(build: &PublishedBuild, now: i64) -> MetaParts {
    let bench = build.kind == "bench";
    let tail = match subject(build) {
        Ok(None) => format!(" \u{b7} {} deleted", build.kind),
        _ if build.archived => " \u{b7} archived".into(),
        _ => String::new(),
    };
    MetaParts {
        first: if bench {
            let n = build.labels.len();
            format!("{n} {}", if n == 1 { "model" } else { "models" })
        } else {
            build.labels.first().cloned().unwrap_or_default()
        },
        first_tip: bench.then(|| build.labels.join(", ")),
        ago: ago(build.published_at, now),
        tail,
    }
}

/// The publish time's hover (`new Date(publishedAt).toLocaleString()` in
/// the page's usual locale, en-US): `11/1/2026, 3:40:05 PM`, local time.
pub fn published_at_tip(at: i64) -> String {
    use chrono::TimeZone as _;
    chrono::Local
        .timestamp_millis_opt(at)
        .single()
        .unwrap_or_else(|| panic!("{at} is not a time"))
        .format("%-m/%-d/%Y, %-I:%M:%S %p")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    const AT: i64 = 1_793_491_200_000; // 2026-11-01T00:00:00Z

    fn build(share_id: &str) -> PublishedBuild {
        PublishedBuild {
            share_id: share_id.into(),
            title: format!("Build {share_id}"),
            kind: "chat".into(),
            labels: vec!["m".into()],
            url: format!("https://o.github.io/r/{share_id}/"),
            published_at: AT,
            conversation_id: Some(format!("c-{share_id}")),
            bench_id: None,
            archived: false,
        }
    }

    fn data() -> Data {
        Data {
            repo: Some("o/r".into()),
            builds: vec![build("a"), build("b")],
        }
    }

    #[test]
    fn the_list_reads_lands_or_fails_can_be_read_again_and_goes_unread_when_settings_close() {
        assert!(view(&UNREAD).unwrap_err().contains("show nothing"));
        let mut published = reading(&UNREAD).unwrap();
        assert_eq!(
            published,
            Published {
                phase: Phase::Loading,
                ..UNREAD
            }
        );
        assert_eq!(view(&published), Ok(View::Loading));
        published = failed(&published, "gh is not signed in").unwrap();
        assert_eq!(view(&published), Ok(View::Failed));
        assert_eq!(published.error.as_deref(), Some("gh is not signed in"));
        assert!(
            read(&published, data())
                .unwrap_err()
                .contains("cannot go from failed to loaded")
        );
        published = read(&reading(&published).unwrap(), data()).unwrap();
        assert_eq!(published.phase, Phase::Loaded);
        assert_eq!(published.error, None);
        assert_eq!(view(&published), Ok(View::List));
        // A read again keeps the rows on show until it lands.
        let again = reading(&published).unwrap();
        assert_eq!(again.data, Some(data()));
        assert_eq!(view(&again), Ok(View::List));
        assert!(
            failed(&published, "late")
                .unwrap_err()
                .contains("cannot go from loaded to failed")
        );
        assert_eq!(closed(&again).unwrap(), UNREAD);
        assert!(
            closed(&UNREAD)
                .unwrap_err()
                .contains("cannot go from unread to unread")
        );
        // Settings opened again read from nothing.
        assert_eq!(reading(&closed(&published).unwrap()).unwrap().data, None);
    }

    #[test]
    fn no_repository_and_no_builds_are_their_own_views() {
        let read_as = |body| read(&reading(&UNREAD).unwrap(), body).unwrap();
        assert_eq!(
            view(&read_as(Data {
                repo: None,
                builds: Vec::new()
            })),
            Ok(View::NoRepo)
        );
        assert_eq!(
            view(&read_as(Data {
                repo: Some("o/r".into()),
                builds: Vec::new()
            })),
            Ok(View::Empty)
        );
    }

    #[test]
    fn a_removal_marks_its_row_then_drops_it_or_keeps_it_with_why() {
        let loaded = read(&reading(&UNREAD).unwrap(), data()).unwrap();
        assert!(
            removing(&loaded, "zz")
                .unwrap_err()
                .contains("not a published build on show")
        );
        let mut published = removing(&loaded, "a").unwrap();
        assert_eq!(published.removing, ["a"]);
        assert!(
            removing(&published, "a")
                .unwrap_err()
                .contains("already being removed")
        );
        published = kept(&published, "a", "This build is not shared").unwrap();
        assert!(published.removing.is_empty());
        assert_eq!(
            published.errors,
            BTreeMap::from([("a".to_string(), "This build is not shared".to_string())])
        );
        assert_eq!(published.data.as_ref().unwrap().builds.len(), 2);
        assert!(
            kept(&published, "a", "again")
                .unwrap_err()
                .contains("not being removed")
        );
        // Trying again lets go of the error; a read meanwhile keeps the
        // removal.
        published = reading(&removing(&published, "a").unwrap()).unwrap();
        assert!(published.errors.is_empty());
        assert_eq!(published.removing, ["a"]);
        published = removed(&published, "a").unwrap();
        assert_eq!(
            published
                .data
                .as_ref()
                .unwrap()
                .builds
                .iter()
                .map(|b| b.share_id.as_str())
                .collect::<Vec<_>>(),
            ["b"]
        );
        assert!(published.removing.is_empty());
        assert!(
            removed(&published, "a")
                .unwrap_err()
                .contains("not being removed")
        );
    }

    #[test]
    fn published_times_read_in_their_largest_whole_unit() {
        assert_eq!(ago(AT, AT + 30_000), "just now");
        assert_eq!(ago(AT, AT - 5_000), "just now");
        assert_eq!(ago(AT, AT + 60_000), "1 minute ago");
        assert_eq!(ago(AT, AT + 2 * 3_600_000 + 59_000), "2 hours ago");
        assert_eq!(ago(AT, AT + 86_400_000), "1 day ago");
        assert_eq!(ago(AT, AT + 45 * 86_400_000), "1 month ago");
        assert_eq!(ago(AT, AT + 800 * 86_400_000), "2 years ago");
    }

    #[test]
    fn a_published_build_names_the_chat_or_bench_it_came_from_or_null_once_that_is_deleted() {
        assert_eq!(subject(&build("a")), Ok(Some(Subject::Chat("c-a".into()))));
        let mut gone = build("a");
        gone.conversation_id = None;
        assert_eq!(subject(&gone), Ok(None));
        let mut bench = build("b");
        bench.kind = "bench".into();
        bench.labels = vec!["A".into(), "B".into()];
        bench.conversation_id = None;
        bench.bench_id = Some("bench-1".into());
        assert_eq!(subject(&bench), Ok(Some(Subject::Bench("bench-1".into()))));
        bench.bench_id = None;
        assert_eq!(subject(&bench), Ok(None));
        let mut prompt = build("c");
        prompt.kind = "prompt".into();
        assert!(
            subject(&prompt)
                .unwrap_err()
                .contains("not a kind of published build")
        );
    }

    // par-shell (N.3)
    #[test]
    fn the_meta_line_parts_carry_the_pages_hovers() {
        let mut b = build("s1");
        b.kind = "bench".into();
        b.labels = vec!["Claude".into(), "GPT".into()];
        let parts = meta_parts(&b, b.published_at);
        assert_eq!(parts.first, "2 models");
        assert_eq!(parts.first_tip.as_deref(), Some("Claude, GPT"));
        let chat = meta_parts(&build("s2"), 0);
        assert_eq!((chat.first.as_str(), chat.first_tip), ("m", None));
        let tip = published_at_tip(AT);
        let re =
            regex::Regex::new(r"^\d{1,2}/\d{1,2}/\d{4}, \d{1,2}:\d{2}:\d{2} (AM|PM)$").expect("re");
        assert!(re.is_match(&tip), "{tip}");
    }
}
