//! Places and history: what the page's addresses became in the app
//! (`web/place.mjs` and `web/address.mjs`, tests ported from
//! `test/place.test.js`, `test/address.test.js` and
//! `test/page-addresses.test.js`, same cases and names).
//!
//! Decided by the stage-four planning session: the native app has no
//! addresses. A place is what an address named; the window keeps a back and
//! forward history of places, pushed and replaced exactly as the page
//! pushes and replaces its history entries (`app.md` 10); the place open at
//! quit is restored at the next start (the core stores it as a preference);
//! and today's address syntax stays as the one text form of a place, which
//! `mcbench-ui-dev --open <address>` (and the app, later) takes, and which
//! deep links will use when stage five registers a URL scheme. No scheme is
//! registered now.
//!
//! The places are exactly `web/address.mjs`'s. Settings, the search panel
//! and the sidebar's archived section open over a place and are no place:
//! Settings is a layer of [`super::layers`] (Escape closes it, shortcuts
//! wait under it), with no history entry.

/// Build or Bench.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Mode {
    /// Build.
    Build,
    /// Bench.
    Bench,
}

impl Mode {
    /// `build` or `bench`.
    pub fn word(self) -> &'static str {
        match self {
            Mode::Build => "build",
            Mode::Bench => "bench",
        }
    }

    /// The other mode.
    pub fn other(self) -> Mode {
        match self {
            Mode::Build => Mode::Bench,
            Mode::Bench => Mode::Build,
        }
    }
}

/// A place (`place.mjs`), or the landing an address can name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Place {
    /// `/`: not a place; the newest chat, or the mode last used.
    Landing,
    /// A Build chat the list has.
    Chat {
        /// Its id.
        id: String,
    },
    /// Build's new chat; `id` is its row in the core, none until it has one.
    NewChat {
        /// Its row.
        id: Option<String>,
    },
    /// A bench about to be run.
    NewBench,
    /// A bench's comparison.
    Compare {
        /// The bench.
        bench: String,
    },
    /// One model's chat in a bench.
    Member {
        /// The bench.
        bench: String,
        /// The model's chat.
        member: String,
    },
    /// The leaderboard.
    Leaderboard,
    /// The prompt library.
    Prompt,
    /// A chat or bench an address names that the lists do not have.
    Missing {
        /// Its mode.
        mode: Mode,
        /// The chat's or the bench's id.
        id: String,
        /// The address it came from.
        address: String,
        /// Whether the archive has it: none while that is looked up.
        archived: Option<bool>,
    },
}

/// The draft key of the new chat, the same before and after its row.
pub const NEW_CHAT_DRAFT: &str = "chat:new";
/// The new bench's draft key.
pub const NEW_BENCH_DRAFT: &str = "bench:new";

/// A chat's draft key.
pub fn chat_draft(id: &str) -> String {
    format!("chat:{id}")
}

/// A bench's draft key.
pub fn bench_draft(bench: &str) -> String {
    format!("bench:{bench}")
}

/// What a place means for the window (`placeKeys`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Keys {
    /// Build or Bench.
    pub mode: Mode,
    /// The chat whose transcript it shows.
    pub chat: Option<String>,
    /// The world the viewer shows: a chat id, a bench round, or none.
    pub view: Option<String>,
    /// The draft its composer edits; none for a place with no composer.
    pub draft: Option<String>,
}

/// A place's keys; `round` is the round each comparison shows, by bench.
pub fn place_keys(place: &Place, round: impl Fn(&str) -> Option<u32>) -> Keys {
    let keys = |mode, chat: Option<&str>, view: Option<String>, draft: Option<String>| Keys {
        mode,
        chat: chat.map(str::to_string),
        view,
        draft,
    };
    match place {
        Place::Chat { id } => keys(
            Mode::Build,
            Some(id),
            Some(id.clone()),
            Some(chat_draft(id)),
        ),
        Place::NewChat { id } => keys(
            Mode::Build,
            id.as_deref(),
            id.clone(),
            Some(NEW_CHAT_DRAFT.into()),
        ),
        Place::NewBench => keys(Mode::Bench, None, None, Some(NEW_BENCH_DRAFT.into())),
        Place::Compare { bench } => {
            let round = round(bench)
                .unwrap_or_else(|| panic!("The comparison of bench {bench} has no round to show"));
            keys(
                Mode::Bench,
                None,
                Some(format!("bench:{bench}:{round}")),
                Some(bench_draft(bench)),
            )
        }
        Place::Member { member, .. } => keys(Mode::Bench, Some(member), Some(member.clone()), None),
        Place::Leaderboard | Place::Prompt => keys(Mode::Bench, None, None, None),
        Place::Missing { mode, .. } => keys(*mode, None, None, None),
        Place::Landing => panic!("The landing names no place of its own"),
    }
}

/// The new chat's row now exists: the same place and draft, with a world
/// (`withRow`).
pub fn with_row(place: &Place, id: &str) -> Place {
    match place {
        Place::NewChat { id: None } => Place::NewChat {
            id: Some(id.to_string()),
        },
        other => panic!(
            "Only a new chat without a row takes one, not {}",
            kind(other)
        ),
    }
}

/// The new chat got its first message: a chat like any other (`asChat`).
pub fn as_chat(place: &Place) -> Place {
    match place {
        Place::NewChat { id: Some(id) } => Place::Chat { id: id.clone() },
        other => panic!(
            "Only a new chat with a row becomes a chat, not {}",
            kind(other)
        ),
    }
}

/// The page's name of a place's kind.
pub fn kind(place: &Place) -> &'static str {
    match place {
        Place::Landing => "landing",
        Place::Chat { .. } => "chat",
        Place::NewChat { .. } => "new-chat",
        Place::NewBench => "new-bench",
        Place::Compare { .. } => "compare",
        Place::Member { .. } => "member",
        Place::Leaderboard => "leaderboard",
        Place::Prompt => "prompt",
        Place::Missing { .. } => "missing",
    }
}

/// What the window's title needs to know of the lists.
pub trait Titles {
    /// A listed chat's title.
    fn chat(&self, id: &str) -> Option<String>;
    /// A listed bench's title.
    fn bench(&self, id: &str) -> Option<String>;
    /// A bench member's name as shown (its model, or its Build letter
    /// while the bench hides names).
    fn member(&self, bench: &str, member: &str) -> Option<String>;
}

/// The place's title (`app.md` 10.2), none when the page's is empty.
pub fn place_title(place: &Place, lists: &impl Titles) -> Option<String> {
    match place {
        Place::Missing { archived, .. } => Some(
            if *archived == Some(true) {
                "Archived"
            } else {
                "Not found"
            }
            .into(),
        ),
        Place::Chat { id } => lists.chat(id),
        Place::NewChat { .. } => Some("New build".into()),
        Place::NewBench => Some("New bench".into()),
        Place::Compare { bench } => lists.bench(bench),
        Place::Leaderboard => Some("Leaderboard".into()),
        Place::Member { bench, member } => Some(format!(
            "{} / {}",
            lists.bench(bench).unwrap_or_default(),
            lists.member(bench, member).unwrap_or_default()
        )),
        Place::Prompt => Some("Prompts".into()),
        Place::Landing => None,
    }
}

/// The window's title: `<place title> · mcbench-lite`, or `mcbench-lite`
/// when the place has none (the page's `document.title`).
pub fn window_title(place: &Place, lists: &impl Titles) -> String {
    match place_title(place, lists).filter(|t| !t.is_empty()) {
        Some(title) => format!("{title} \u{b7} mcbench-lite"),
        None => "mcbench-lite".into(),
    }
}

/// How a navigation changes the history (`setPlace` and
/// `replacingAddress`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Visit {
    /// A navigation the user makes: a new entry, unless the place's
    /// address is the current one.
    Push,
    /// The window renames the place on screen or stands in for one that is
    /// gone: the current entry is replaced.
    Replace,
}

/// The back and forward history of places.
#[derive(Clone, Debug, Default)]
pub struct History {
    entries: Vec<Place>,
    at: usize,
}

impl History {
    /// The place on screen.
    pub fn current(&self) -> Option<&Place> {
        self.entries.get(self.at)
    }

    /// Records a navigation to `place`. A pushed place whose address is the
    /// current one's changes nothing but the current entry (no new entry,
    /// as the page's address bar); a push drops the forward entries.
    pub fn visit(&mut self, place: Place, visit: Visit) {
        let same = self.current().is_some_and(|current| {
            super::address::address(current) == super::address::address(&place)
        });
        match (visit, self.entries.is_empty()) {
            (_, true) => {
                self.entries.push(place);
                self.at = 0;
            }
            (Visit::Replace, false) => self.entries[self.at] = place,
            (Visit::Push, false) if same => self.entries[self.at] = place,
            (Visit::Push, false) => {
                self.entries.truncate(self.at + 1);
                self.entries.push(place);
                self.at += 1;
            }
        }
    }

    /// Back: the previous entry, if any.
    pub fn back(&mut self) -> Option<&Place> {
        if self.at == 0 {
            return None;
        }
        self.at -= 1;
        self.current()
    }

    /// Forward: the next entry, if any.
    pub fn forward(&mut self) -> Option<&Place> {
        if self.at + 1 >= self.entries.len() {
            return None;
        }
        self.at += 1;
        self.current()
    }

    /// Whether Back has somewhere to go.
    pub fn can_back(&self) -> bool {
        self.at > 0
    }

    /// Whether Forward has somewhere to go.
    pub fn can_forward(&self) -> bool {
        self.at + 1 < self.entries.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rounds(bench: &str) -> Option<u32> {
        (bench == "b1").then_some(2)
    }

    #[test]
    fn a_place_gives_its_mode_chat_world_and_draft_and_only_a_place_with_a_composer_has_a_draft() {
        let k = |p: &Place| place_keys(p, rounds);
        assert_eq!(
            k(&Place::Chat { id: "c1".into() }),
            Keys {
                mode: Mode::Build,
                chat: Some("c1".into()),
                view: Some("c1".into()),
                draft: Some("chat:c1".into())
            }
        );
        assert_eq!(
            k(&Place::NewBench),
            Keys {
                mode: Mode::Bench,
                chat: None,
                view: None,
                draft: Some("bench:new".into())
            }
        );
        assert_eq!(
            k(&Place::Compare { bench: "b1".into() }),
            Keys {
                mode: Mode::Bench,
                chat: None,
                view: Some("bench:b1:2".into()),
                draft: Some("bench:b1".into())
            }
        );
        // A bench model's chat is read, never written to: no composer, no
        // draft.
        assert_eq!(
            k(&Place::Member {
                bench: "b1".into(),
                member: "m1".into()
            }),
            Keys {
                mode: Mode::Bench,
                chat: Some("m1".into()),
                view: Some("m1".into()),
                draft: None
            }
        );
        for place in [Place::Leaderboard, Place::Prompt] {
            assert_eq!(
                k(&place),
                Keys {
                    mode: Mode::Bench,
                    chat: None,
                    view: None,
                    draft: None
                }
            );
        }
        // No Bench place shares a draft with Build's new chat.
        let new_chat = k(&Place::NewChat { id: None }).draft;
        for place in [Place::NewBench, Place::Leaderboard, Place::Prompt] {
            assert_ne!(k(&place).draft, new_chat);
        }
        let missing_round =
            std::panic::catch_unwind(|| k(&Place::Compare { bench: "b2".into() })).unwrap_err();
        assert!(
            missing_round
                .downcast_ref::<String>()
                .is_some_and(|m| m.contains("no round to show"))
        );
    }

    #[test]
    fn the_new_chat_keeps_one_draft_key_before_and_after_its_row_exists_and_gives_it_up_as_a_chat()
    {
        let k = |p: &Place| place_keys(p, |_| None);
        let fresh = Place::NewChat { id: None };
        assert_eq!(
            k(&fresh),
            Keys {
                mode: Mode::Build,
                chat: None,
                view: None,
                draft: Some(NEW_CHAT_DRAFT.into())
            }
        );
        let made = with_row(&fresh, "c9");
        // The row gives the new chat a world to show; the draft is the same.
        assert_eq!(
            k(&made),
            Keys {
                mode: Mode::Build,
                chat: Some("c9".into()),
                view: Some("c9".into()),
                draft: Some(NEW_CHAT_DRAFT.into())
            }
        );
        let chat = as_chat(&made);
        assert_eq!(chat, Place::Chat { id: "c9".into() });
        assert_eq!(k(&chat).draft, Some(chat_draft("c9")));
        assert_ne!(chat_draft("c9"), NEW_CHAT_DRAFT);
        let message = |f: Box<dyn FnOnce() -> Place + std::panic::UnwindSafe>| {
            std::panic::catch_unwind(f)
                .unwrap_err()
                .downcast_ref::<String>()
                .cloned()
                .expect("a message")
        };
        let made2 = made.clone();
        assert!(
            message(Box::new(move || with_row(&made2, "c10")))
                .contains("Only a new chat without a row takes one")
        );
        let chat2 = chat.clone();
        assert!(message(Box::new(move || with_row(&chat2, "c10"))).contains("not chat"));
        assert!(
            message(Box::new(move || as_chat(&Place::NewChat { id: None })))
                .contains("Only a new chat with a row becomes a chat")
        );
        assert!(message(Box::new(|| as_chat(&Place::NewBench))).contains("not new-bench"));
    }

    struct Lists;
    impl Titles for Lists {
        fn chat(&self, id: &str) -> Option<String> {
            (id == "c1").then(|| "Castle gate".into())
        }
        fn bench(&self, id: &str) -> Option<String> {
            (id == "b1").then(|| "Towers".into())
        }
        fn member(&self, _: &str, member: &str) -> Option<String> {
            (member == "m1").then(|| "Build A".into())
        }
    }

    #[test]
    fn the_window_title_follows_the_pages_document_title() {
        let t = |p: Place| window_title(&p, &Lists);
        assert_eq!(
            t(Place::Chat { id: "c1".into() }),
            "Castle gate · mcbench-lite"
        );
        assert_eq!(t(Place::NewChat { id: None }), "New build · mcbench-lite");
        assert_eq!(t(Place::NewBench), "New bench · mcbench-lite");
        assert_eq!(
            t(Place::Compare { bench: "b1".into() }),
            "Towers · mcbench-lite"
        );
        // A comparison whose bench is not known yet: the bare name.
        assert_eq!(t(Place::Compare { bench: "b9".into() }), "mcbench-lite");
        assert_eq!(
            t(Place::Member {
                bench: "b1".into(),
                member: "m1".into()
            }),
            "Towers / Build A · mcbench-lite"
        );
        assert_eq!(t(Place::Leaderboard), "Leaderboard · mcbench-lite");
        assert_eq!(t(Place::Prompt), "Prompts · mcbench-lite");
        let missing = |archived| Place::Missing {
            mode: Mode::Build,
            id: "gone".into(),
            address: "/chat/gone".into(),
            archived,
        };
        assert_eq!(t(missing(None)), "Not found · mcbench-lite");
        assert_eq!(t(missing(Some(false))), "Not found · mcbench-lite");
        assert_eq!(t(missing(Some(true))), "Archived · mcbench-lite");
    }

    #[test]
    fn history_pushes_replaces_and_moves_back_and_forward() {
        let chat = |id: &str| Place::Chat { id: id.into() };
        let mut history = History::default();
        history.visit(chat("a"), Visit::Replace);
        history.visit(chat("b"), Visit::Push);
        history.visit(chat("c"), Visit::Push);
        assert_eq!(history.back(), Some(&chat("b")));
        assert_eq!(history.back(), Some(&chat("a")));
        assert_eq!(history.back(), None);
        assert_eq!(history.forward(), Some(&chat("b")));
        // A push drops the forward entries.
        history.visit(chat("d"), Visit::Push);
        assert!(!history.can_forward());
        assert_eq!(history.back(), Some(&chat("b")));
        // A replace changes the entry in place.
        history.visit(chat("e"), Visit::Replace);
        assert_eq!(history.current(), Some(&chat("e")));
        assert_eq!(history.forward(), Some(&chat("d")));
        // The new chat taking its row keeps one entry: the address is /new.
        history.visit(Place::NewChat { id: None }, Visit::Push);
        history.visit(
            Place::NewChat {
                id: Some("c9".into()),
            },
            Visit::Push,
        );
        assert_eq!(history.back(), Some(&chat("d")));
    }
}
