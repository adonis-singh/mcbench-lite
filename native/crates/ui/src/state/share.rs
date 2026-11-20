//! The share of one subject, a chat's build or a bench's builds on one
//! public page (`web/share.mjs`, tests ported from `test/share-page.test.js`
//! case for case, same names). One value per subject:
//! `Unknown` (not read yet), `Loading` (being read), `None` (not shared),
//! `Shared` (live at `url`), `Exporting`, `Publishing`, `Waiting` (a publish
//! on its way, the step the core last said; `url` the one it had before),
//! `Failed` (`error` the core's text; `url` kept), `Removing`.
//!
//! The page's stream reader (`readShareStream`) has no port: the core
//! publishes each step as its own update ([`crate::PublishStep`]), and a
//! publish that stops early is the core's `Failed` with
//! [`STOPPED_EARLY`] (`ui-seam-share.md`, answer 3).

use crate::{PublishStep, Subject};

/// A share's phase.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Not read yet.
    Unknown,
    /// Being read.
    Loading,
    /// Not shared.
    None,
    /// Live at its url.
    Shared,
    /// A publish preparing the build.
    Exporting,
    /// A publish pushing.
    Publishing,
    /// A publish waiting for GitHub Pages.
    Waiting,
    /// The publish failed, with the core's text.
    Failed(String),
    /// Being taken down.
    Removing,
}

impl Phase {
    /// The page's name for it (in the refusal's text).
    pub fn name(&self) -> &'static str {
        match self {
            Phase::Unknown => "unknown",
            Phase::Loading => "loading",
            Phase::None => "none",
            Phase::Shared => "shared",
            Phase::Exporting => "exporting",
            Phase::Publishing => "publishing",
            Phase::Waiting => "waiting",
            Phase::Failed(_) => "failed",
            Phase::Removing => "removing",
        }
    }

    fn may_become(&self, next: &Phase) -> bool {
        use Phase as P;
        matches!(
            (self, next),
            (P::Unknown, P::Loading)
                | (P::Loading, P::None | P::Shared | P::Unknown)
                | (P::None, P::Exporting)
                | (P::Shared, P::Exporting | P::Removing)
                | (P::Exporting, P::Exporting | P::Publishing | P::Failed(_))
                | (P::Publishing, P::Waiting | P::Failed(_))
                | (P::Waiting, P::Shared | P::Failed(_))
                | (P::Failed(_), P::Exporting)
                | (P::Removing, P::None | P::Shared)
        )
    }
}

/// One subject's share.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Share {
    /// Its phase.
    pub phase: Phase,
    /// Its page's address; none when not shared or not read.
    pub url: Option<String>,
}

/// `SHARE_UNKNOWN`.
pub const UNKNOWN: Share = Share {
    phase: Phase::Unknown,
    url: None,
};

/// The core's text for a publish that ended before live or failed.
pub const STOPPED_EARLY: &str = "The publish stopped before the page was live";

/// The step texts (`STEP_TEXT`).
pub const PREPARING: &str = "Preparing the build…";
/// `publishing`.
pub const UPLOADING: &str = "Uploading…";
/// `waiting`.
pub const WAITING: &str = "Waiting for GitHub Pages…";

/// A move the page refuses (`shareTo` throws), with the page's text.
pub type Moved = Result<Share, String>;

fn to(share: &Share, next: Share) -> Moved {
    if share.phase.may_become(&next.phase) {
        Ok(next)
    } else {
        Err(format!(
            "The share cannot go from {} to {}",
            share.phase.name(),
            next.phase.name()
        ))
    }
}

/// The page's key for a subject's share (`shareKey`).
pub fn share_key(subject: &Subject) -> String {
    match subject {
        Subject::Chat(id) => format!("chat:{id}"),
        Subject::Bench(id) => format!("bench:{id}"),
    }
}

/// `shareReading`.
pub fn reading(share: &Share) -> Moved {
    to(
        share,
        Share {
            phase: Phase::Loading,
            url: None,
        },
    )
}

/// `shareRead`: the core's url.
pub fn read(share: &Share, url: Option<String>) -> Moved {
    let phase = if url.is_some() {
        Phase::Shared
    } else {
        Phase::None
    };
    to(share, Share { phase, url })
}

/// `shareUnread`: the read failed; unknown again, read on the next opening.
pub fn unread(share: &Share) -> Moved {
    to(share, UNKNOWN)
}

/// `sharePublishing`: a press of Publish.
pub fn publishing(share: &Share) -> Moved {
    to(
        share,
        Share {
            phase: Phase::Exporting,
            url: share.url.clone(),
        },
    )
}

/// `shareStep`: one move the core published.
pub fn step(share: &Share, step: &PublishStep) -> Moved {
    let (phase, url) = match step {
        PublishStep::Exporting => (Phase::Exporting, share.url.clone()),
        PublishStep::Publishing => (Phase::Publishing, share.url.clone()),
        PublishStep::Waiting => (Phase::Waiting, share.url.clone()),
        PublishStep::Live { url } => (Phase::Shared, Some(url.clone())),
        PublishStep::Failed { error } => (Phase::Failed(error.clone()), share.url.clone()),
    };
    to(share, Share { phase, url })
}

/// `shareFailed`: a refusal of the publish (the core's `Failed` before any
/// step is this move too, through [`step`]).
pub fn failed(share: &Share, error: &str) -> Moved {
    to(
        share,
        Share {
            phase: Phase::Failed(error.to_string()),
            url: share.url.clone(),
        },
    )
}

/// `shareStoppedEarly`: a publish that ended while still publishing fails.
pub fn stopped_early(share: &Share) -> Moved {
    if !publishing_now(share) {
        return Ok(share.clone());
    }
    failed(share, STOPPED_EARLY)
}

/// `shareRemoving`.
pub fn removing(share: &Share) -> Moved {
    to(
        share,
        Share {
            phase: Phase::Removing,
            url: share.url.clone(),
        },
    )
}

/// `shareRemoved`.
pub fn removed(share: &Share) -> Moved {
    to(
        share,
        Share {
            phase: Phase::None,
            url: None,
        },
    )
}

/// `shareKept`: the removal failed; the page is still up.
pub fn kept(share: &Share) -> Moved {
    to(
        share,
        Share {
            phase: Phase::Shared,
            url: share.url.clone(),
        },
    )
}

/// `sharePublishingNow`.
pub fn publishing_now(share: &Share) -> bool {
    matches!(
        share.phase,
        Phase::Exporting | Phase::Publishing | Phase::Waiting
    )
}

/// `shareStepText`.
pub fn step_text(share: &Share) -> Result<&'static str, String> {
    match share.phase {
        Phase::Exporting => Ok(PREPARING),
        Phase::Publishing => Ok(UPLOADING),
        Phase::Waiting => Ok(WAITING),
        _ => Err(format!(
            "A share that is {} is not publishing",
            share.phase.name()
        )),
    }
}

/// `postUrl`: the X post link (`encodeURIComponent` of the url).
pub fn post_url(url: &str) -> String {
    let mut out = String::from("https://x.com/intent/post?url=");
    for b in url.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&b) {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// What a `ShareChanged` the window did not wait for does: a share being
/// removed is removed (none) or kept (the url); a shared one whose page
/// went elsewhere (Settings' list) is removed, as `removePublished` moves
/// it; any other is left as it is (the page touches only a shared one).
pub fn changed(share: &Share, url: Option<&str>) -> Moved {
    match (&share.phase, url) {
        (Phase::Removing, None) => removed(share),
        (Phase::Removing, Some(_)) => kept(share),
        (Phase::Shared, None) => removed(&removing(share)?),
        _ => Ok(share.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const URL_A: &str = "https://owner.github.io/builds/a1/";

    fn s(phase: Phase, url: Option<&str>) -> Share {
        Share {
            phase,
            url: url.map(str::to_string),
        }
    }

    fn st(name: &str) -> PublishStep {
        match name {
            "exporting" => PublishStep::Exporting,
            "publishing" => PublishStep::Publishing,
            "waiting" => PublishStep::Waiting,
            _ => unreachable!(),
        }
    }

    fn refused(moved: Moved, text: &str) {
        let error = moved.expect_err("refused");
        assert!(error.contains(text), "{error}");
    }

    #[test]
    fn a_share_is_read_once_unknown_loading_then_none_or_shared() {
        let loading = reading(&UNKNOWN).unwrap();
        assert_eq!(loading, s(Phase::Loading, None));
        assert_eq!(read(&loading, None).unwrap(), s(Phase::None, None));
        assert_eq!(
            read(&loading, Some(URL_A.into())).unwrap(),
            s(Phase::Shared, Some(URL_A))
        );
        // A read that failed leaves the share unknown, to be read again.
        assert_eq!(unread(&loading).unwrap(), UNKNOWN);
        refused(read(&UNKNOWN, None), "cannot go from unknown to none");
        refused(reading(&loading), "cannot go from loading to loading");
        refused(
            reading(&read(&loading, None).unwrap()),
            "cannot go from none to loading",
        );
    }

    #[test]
    fn a_publish_steps_through_the_servers_states_to_shared_one_at_a_time() {
        let mut share = publishing(&s(Phase::None, None)).unwrap();
        assert_eq!(share, s(Phase::Exporting, None));
        // The server's first line says what the click already moved to.
        share = step(&share, &st("exporting")).unwrap();
        assert_eq!(share, s(Phase::Exporting, None));
        share = step(&share, &st("publishing")).unwrap();
        assert_eq!(share, s(Phase::Publishing, None));
        // No second publish while one runs.
        refused(publishing(&share), "cannot go from publishing to exporting");
        refused(
            step(&share, &PublishStep::Live { url: URL_A.into() }),
            "cannot go from publishing to shared",
        );
        share = step(&share, &st("waiting")).unwrap();
        assert_eq!(share, s(Phase::Waiting, None));
        share = step(&share, &PublishStep::Live { url: URL_A.into() }).unwrap();
        assert_eq!(share, s(Phase::Shared, Some(URL_A)));
        refused(
            step(&share, &st("publishing")),
            "cannot go from shared to publishing",
        );
        // The page's `paused` (a state the stream has not) cannot be sent:
        // `PublishStep` has only the five.
    }

    #[test]
    fn a_failed_publish_keeps_the_url_it_had_and_its_message_try_again_publishes_anew() {
        let again = publishing(&s(Phase::Shared, Some(URL_A))).unwrap();
        assert_eq!(again, s(Phase::Exporting, Some(URL_A)));
        let failed_one = step(
            &step(&again, &st("publishing")).unwrap(),
            &PublishStep::Failed {
                error: "git push failed".into(),
            },
        )
        .unwrap();
        assert_eq!(
            failed_one,
            s(Phase::Failed("git push failed".into()), Some(URL_A))
        );
        // A refusal (409 during a running turn) fails the publish the click began.
        assert_eq!(
            failed(&s(Phase::Exporting, None), "A turn is running").unwrap(),
            s(Phase::Failed("A turn is running".into()), None)
        );
        assert_eq!(
            publishing(&failed_one).unwrap(),
            s(Phase::Exporting, Some(URL_A))
        );
        refused(
            failed(&s(Phase::None, None), "x"),
            "cannot go from none to failed",
        );
        // A stream that ended before live or failed is a failed publish.
        assert_eq!(
            stopped_early(&s(Phase::Waiting, None)).unwrap(),
            s(Phase::Failed(STOPPED_EARLY.into()), None)
        );
        assert_eq!(stopped_early(&failed_one).unwrap(), failed_one);
        assert_eq!(
            stopped_early(&s(Phase::Shared, Some(URL_A))).unwrap(),
            s(Phase::Shared, Some(URL_A))
        );
    }

    #[test]
    fn a_shared_build_is_removed_and_a_removal_that_fails_leaves_it_shared() {
        let removing_one = removing(&s(Phase::Shared, Some(URL_A))).unwrap();
        assert_eq!(removing_one, s(Phase::Removing, Some(URL_A)));
        assert_eq!(removed(&removing_one).unwrap(), s(Phase::None, None));
        assert_eq!(kept(&removing_one).unwrap(), s(Phase::Shared, Some(URL_A)));
        refused(
            removing(&s(Phase::None, None)),
            "cannot go from none to removing",
        );
        refused(
            removing(&s(Phase::Failed("x".into()), Some(URL_A))),
            "cannot go from failed to removing",
        );
        refused(
            removed(&s(Phase::Shared, Some(URL_A))),
            "cannot go from shared to none",
        );
    }

    #[test]
    fn the_post_link_carries_the_encoded_url() {
        assert_eq!(
            post_url("https://o.github.io/b/a 1/?x=1&y"),
            "https://x.com/intent/post?url=https%3A%2F%2Fo.github.io%2Fb%2Fa%201%2F%3Fx%3D1%26y"
        );
    }

    #[test]
    fn the_step_shown_while_publishing_is_said_in_plain_words() {
        assert_eq!(
            step_text(&s(Phase::Exporting, None)).unwrap(),
            "Preparing the build…"
        );
        assert_eq!(
            step_text(&s(Phase::Publishing, None)).unwrap(),
            "Uploading…"
        );
        assert_eq!(
            step_text(&s(Phase::Waiting, None)).unwrap(),
            "Waiting for GitHub Pages…"
        );
        assert!(
            step_text(&s(Phase::Shared, Some(URL_A)))
                .unwrap_err()
                .contains("not publishing")
        );
    }

    #[test]
    fn a_share_belongs_to_a_subject_a_chat_or_a_bench_with_its_own_key_and_endpoint() {
        assert_eq!(share_key(&Subject::Chat("c1".into())), "chat:c1");
        assert_eq!(share_key(&Subject::Bench("c1".into())), "bench:c1");
        // The endpoint is the core's (`Command::ReadShare` by subject); a
        // subject of another kind cannot be written (`Subject` has two).
    }

    #[test]
    fn a_page_taken_down_elsewhere_moves_only_a_shared_or_removing_share() {
        let shared = s(Phase::Shared, Some(URL_A));
        assert_eq!(changed(&shared, None).unwrap(), s(Phase::None, None));
        let removing_one = removing(&shared).unwrap();
        assert_eq!(changed(&removing_one, None).unwrap(), s(Phase::None, None));
        assert_eq!(changed(&removing_one, Some(URL_A)).unwrap(), shared);
        for other in [
            UNKNOWN,
            s(Phase::Waiting, None),
            s(Phase::Failed("x".into()), Some(URL_A)),
        ] {
            assert_eq!(changed(&other, None).unwrap(), other);
        }
    }
}
