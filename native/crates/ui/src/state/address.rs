//! A place's text form, in today's address syntax (`web/address.mjs`; tests
//! ported from `test/address.test.js` and `test/page-addresses.test.js`):
//!
//! ```text
//! /                         the landing: the newest chat, or the mode last used
//! /new                      Build's new chat, before and after it has a row
//! /chat/<id>                a Build chat
//! /bench                    a new bench
//! /bench/<id>               a bench's comparison
//! /bench/<id>/<memberId>    one model's chat in a bench
//! /leaderboard
//! /prompts                  the prompt library
//! ```
//!
//! An id is one percent-encoded path segment.
//! `mcbench-ui-dev --open <address>` takes these; so will deep links (stage
//! five).

use super::place::{Mode, Place};

/// `encodeURIComponent`: everything but `A-Z a-z 0-9 - _ . ! ~ * ' ( )`
/// percent-encoded as UTF-8.
fn segment(id: &str) -> String {
    let mut out = String::new();
    for byte in id.bytes() {
        match byte {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'_'
            | b'.'
            | b'!'
            | b'~'
            | b'*'
            | b'\''
            | b'('
            | b')' => out.push(byte as char),
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

/// `decodeURIComponent`, none for a malformed escape.
fn decode(part: &str) -> Option<String> {
    let bytes = part.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = std::str::from_utf8(bytes.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// A place's address (`placeAddress`).
pub fn address(place: &Place) -> String {
    match place {
        Place::Chat { id } => format!("/chat/{}", segment(id)),
        Place::NewChat { .. } => "/new".into(),
        Place::NewBench => "/bench".into(),
        Place::Compare { bench } => format!("/bench/{}", segment(bench)),
        Place::Member { bench, member } => {
            format!("/bench/{}/{}", segment(bench), segment(member))
        }
        Place::Leaderboard => "/leaderboard".into(),
        Place::Prompt => "/prompts".into(),
        // A chat or bench the address names that is not there keeps it.
        Place::Missing { address, .. } => address.clone(),
        Place::Landing => "/".into(),
    }
}

/// What an address names: the landing, or a place (a chat's or a bench's
/// not yet looked up in the lists); none when it is not an address
/// (`addressPlace`).
pub fn address_place(path: &str) -> Option<Place> {
    match path {
        "/" => return Some(Place::Landing),
        "/new" => return Some(Place::NewChat { id: None }),
        "/bench" => return Some(Place::NewBench),
        "/leaderboard" => return Some(Place::Leaderboard),
        "/prompts" => return Some(Place::Prompt),
        _ => {}
    }
    let parts: Vec<&str> = path.strip_prefix('/')?.split('/').collect();
    if parts.iter().any(|p| p.is_empty()) {
        return None;
    }
    let ids = parts[1..]
        .iter()
        .map(|p| decode(p))
        .collect::<Option<Vec<String>>>()?;
    match (parts[0], ids.as_slice()) {
        ("chat", [id]) => Some(Place::Chat { id: id.clone() }),
        ("bench", [bench]) => Some(Place::Compare {
            bench: bench.clone(),
        }),
        ("bench", [bench, member]) => Some(Place::Member {
            bench: bench.clone(),
            member: member.clone(),
        }),
        _ => None,
    }
}

/// Whether `path` is an address (`isPageAddress`).
pub fn is_address(path: &str) -> bool {
    address_place(path).is_some()
}

/// The place shown while the lists load (`startPlace`): what the address
/// names when it needs no list; Build's new chat or a new bench while the
/// chat or bench it names is looked up; on the landing, the mode last used.
pub fn start_place(target: &Place, mode: Mode) -> Place {
    match target {
        Place::Landing => match mode {
            Mode::Bench => Place::NewBench,
            Mode::Build => Place::NewChat { id: None },
        },
        Place::Chat { .. } => Place::NewChat { id: None },
        Place::Compare { .. } | Place::Member { .. } => Place::NewBench,
        other => other.clone(),
    }
}

/// A chat row as the lists give it: its id and its bench, if a bench
/// model's.
pub struct ListedChat<'a> {
    /// The chat's id.
    pub id: &'a str,
    /// The bench it belongs to.
    pub bench: Option<&'a str>,
}

/// The place an address names, from the lists (`listedPlace`): a chat or
/// bench they do not have is missing, never another one in its place.
pub fn listed_place(
    target: &Place,
    address: &str,
    chats: &[ListedChat],
    benches: &[&str],
) -> Place {
    let missing = |mode, id: &str| Place::Missing {
        mode,
        id: id.to_string(),
        address: address.to_string(),
        archived: None,
    };
    match target {
        Place::Chat { id } => {
            if chats.iter().any(|c| c.id == id && c.bench.is_none()) {
                target.clone()
            } else {
                missing(Mode::Build, id)
            }
        }
        Place::Compare { bench } => {
            if benches.contains(&bench.as_str()) {
                target.clone()
            } else {
                missing(Mode::Bench, bench)
            }
        }
        Place::Member { bench, member } => {
            if benches.contains(&bench.as_str())
                && chats
                    .iter()
                    .any(|c| c.id == member && c.bench == Some(bench.as_str()))
            {
                target.clone()
            } else {
                missing(Mode::Bench, bench)
            }
        }
        Place::Landing => panic!("The landing names no place of its own"),
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn places() -> Vec<(Place, &'static str)> {
        vec![
            (Place::Chat { id: "c1".into() }, "/chat/c1"),
            (Place::NewChat { id: None }, "/new"),
            (Place::NewBench, "/bench"),
            (Place::Compare { bench: "b1".into() }, "/bench/b1"),
            (
                Place::Member {
                    bench: "b1".into(),
                    member: "m1".into(),
                },
                "/bench/b1/m1",
            ),
            (Place::Leaderboard, "/leaderboard"),
            (Place::Prompt, "/prompts"),
        ]
    }

    #[test]
    fn every_place_has_an_address_that_names_it_again() {
        for (place, path) in places() {
            assert_eq!(address(&place), path);
            assert_eq!(address_place(path), Some(place));
            assert!(is_address(path));
        }
        // The new chat has one address before and after it has a row.
        assert_eq!(
            address(&Place::NewChat {
                id: Some("c9".into())
            }),
            "/new"
        );
        // An id is one path segment, whatever it holds.
        assert_eq!(
            address(&Place::Chat { id: "a/b?c".into() }),
            "/chat/a%2Fb%3Fc"
        );
        assert_eq!(
            address_place("/chat/a%2Fb%3Fc"),
            Some(Place::Chat { id: "a/b?c".into() })
        );
        assert_eq!(
            address(&Place::Missing {
                mode: Mode::Build,
                id: "gone".into(),
                address: "/chat/gone".into(),
                archived: None
            }),
            "/chat/gone"
        );
        assert_eq!(address_place("/"), Some(Place::Landing));
    }

    #[test]
    fn the_apps_files_and_anything_else_are_not_page_addresses() {
        for path in [
            "/app.js",
            "/style.css",
            "/icons/block.svg",
            "/share/bridge.js",
            "/shot.html",
            "/bluemap/x",
            "/api/config",
            "/chat",
            "/chat/",
            "/chat/c1/x",
            "/bench/b1/m1/x",
            "/bench//m1",
            "/new/x",
            "/chat/%E0%A4%A",
        ] {
            assert_eq!(address_place(path), None, "{path}");
            assert!(!is_address(path), "{path}");
        }
    }

    #[test]
    fn while_the_lists_load_the_page_shows_what_needs_none_and_the_landing_shows_the_mode_last_used()
     {
        assert_eq!(
            start_place(&Place::Landing, Mode::Build),
            Place::NewChat { id: None }
        );
        assert_eq!(start_place(&Place::Landing, Mode::Bench), Place::NewBench);
        assert_eq!(
            start_place(&Place::Chat { id: "c1".into() }, Mode::Bench),
            Place::NewChat { id: None }
        );
        assert_eq!(
            start_place(&Place::Compare { bench: "b1".into() }, Mode::Build),
            Place::NewBench
        );
        assert_eq!(
            start_place(
                &Place::Member {
                    bench: "b1".into(),
                    member: "m1".into()
                },
                Mode::Build
            ),
            Place::NewBench
        );
        for place in [
            Place::Leaderboard,
            Place::Prompt,
            Place::NewBench,
            Place::NewChat { id: None },
        ] {
            assert_eq!(start_place(&place, Mode::Build), place);
        }
    }

    #[test]
    fn a_chat_or_bench_the_lists_do_not_have_is_missing_never_another_one() {
        let chats = [
            ListedChat {
                id: "c1",
                bench: None,
            },
            ListedChat {
                id: "m1",
                bench: Some("b1"),
            },
            ListedChat {
                id: "m2",
                bench: Some("b2"),
            },
        ];
        let benches = ["b1"];
        let at = |target: Place, path: &str| listed_place(&target, path, &chats, &benches);
        let missing = |mode, id: &str, path: &str| Place::Missing {
            mode,
            id: id.into(),
            address: path.into(),
            archived: None,
        };
        assert_eq!(
            at(Place::Chat { id: "c1".into() }, "/chat/c1"),
            Place::Chat { id: "c1".into() }
        );
        assert_eq!(
            at(Place::Chat { id: "gone".into() }, "/chat/gone"),
            missing(Mode::Build, "gone", "/chat/gone")
        );
        // A bench model's chat is not a Build chat.
        assert_eq!(
            at(Place::Chat { id: "m1".into() }, "/chat/m1"),
            missing(Mode::Build, "m1", "/chat/m1")
        );
        assert_eq!(
            at(Place::Compare { bench: "b1".into() }, "/bench/b1"),
            Place::Compare { bench: "b1".into() }
        );
        assert_eq!(
            at(Place::Compare { bench: "b2".into() }, "/bench/b2"),
            missing(Mode::Bench, "b2", "/bench/b2")
        );
        let member = |m: &str| Place::Member {
            bench: "b1".into(),
            member: m.into(),
        };
        assert_eq!(at(member("m1"), "/bench/b1/m1"), member("m1"));
        // A model of another bench, under this bench's address, is not shown.
        assert_eq!(
            at(member("m2"), "/bench/b1/m2"),
            missing(Mode::Bench, "b1", "/bench/b1/m2")
        );
        assert_eq!(at(Place::Leaderboard, "/leaderboard"), Place::Leaderboard);
        let landing = std::panic::catch_unwind(|| at(Place::Landing, "/")).unwrap_err();
        assert!(
            landing
                .downcast_ref::<&str>()
                .is_some_and(|m| m.contains("names no place"))
        );
    }

    #[test]
    fn a_missing_place_has_a_mode_and_nothing_to_show_or_write() {
        let keys = super::super::place::place_keys(
            &Place::Missing {
                mode: Mode::Bench,
                id: "b2".into(),
                address: "/bench/b2".into(),
                archived: None,
            },
            |_| None,
        );
        assert_eq!(
            keys,
            super::super::place::Keys {
                mode: Mode::Bench,
                chat: None,
                view: None,
                draft: None
            }
        );
    }

    // test/page-addresses.test.js: the server served the page at every
    // address and nothing else; in the app every address names a place and
    // nothing else does. Its sign-in case (`?token=` kept on the address) has
    // no counterpart: the app has no token and no query.
    #[test]
    fn every_address_of_the_page_is_the_page_its_files_are_themselves_and_anything_else_is_not_found()
     {
        for path in [
            "/",
            "/new",
            "/chat/0521fc61-aa65-48dd-b473-cced680b0b49",
            "/bench",
            "/bench/b1",
            "/bench/b1/m1",
            "/leaderboard",
            "/prompts",
        ] {
            assert!(is_address(path), "{path}");
        }
        for path in [
            "/chat",
            "/chat/c1/x",
            "/nowhere",
            "/chat/%E0%A4%A",
            "/app.js",
        ] {
            assert!(!is_address(path), "{path}");
        }
    }
}
