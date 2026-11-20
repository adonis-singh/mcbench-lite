//! The starred models (`web/favorites.mjs`, tests ported from
//! `test/favorites.test.js` case for case; inventory `app.md` 8.6).
//!
//! The list lives in the core (`modelFavorites`). A star shows at once and
//! is saved after: each toggle goes on its own, in order, as the core's
//! list with that one toggle made, so one that fails is undone (its star
//! goes back) and takes no other with it.
//!
//! The page's promise becomes two calls: [`Favorites::toggle`] says which
//! list to save now (`Command::SetFavorites`), and [`Favorites::ended`]
//! takes the save's answer and says which list to save next. Through the
//! seam an answer is `Update::Favorites` with the list the core kept; a
//! save the core could not make answers its unchanged list (and a notice,
//! `Update::Error`), which is the same as the page's failed save: the
//! toggle is dropped and its star goes back.

/// A starred model: its provider and its id (`<provider>::<model>` in the
/// page).
pub type Key = (String, String);

/// The page's `favoriteKey`: `<provider>::<model>`.
pub fn favorite_key(provider: &str, model: &str) -> String {
    format!("{provider}::{model}")
}

/// Where the favourites are.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// The core's list is not known: nothing can be starred, since saving
    /// would put this window's list over the core's.
    Unloaded,
    /// The list is the core's with the toggles still to save on it.
    Idle,
    /// The first of those toggles is on its way to the core.
    Saving,
}

/// The starred models in the window.
#[derive(Clone, Debug)]
pub struct Favorites {
    phase: Phase,
    /// The core's list.
    saved: Vec<Key>,
    /// The keys toggled and not saved yet, oldest first.
    unsent: Vec<Key>,
}

impl Default for Favorites {
    fn default() -> Self {
        Favorites {
            phase: Phase::Unloaded,
            saved: Vec::new(),
            unsent: Vec::new(),
        }
    }
}

fn toggled(list: &[Key], key: &Key) -> Vec<Key> {
    if list.contains(key) {
        list.iter().filter(|k| *k != key).cloned().collect()
    } else {
        let mut out = list.to_vec();
        out.push(key.clone());
        out
    }
}

/// How a save ended.
#[derive(Clone, Debug, PartialEq)]
pub enum Saved {
    /// The core stored this list.
    Stored(Vec<Key>),
    /// The save failed, for this reason.
    Failed(String),
}

impl Favorites {
    /// Where it is.
    pub fn phase(&self) -> Phase {
        self.phase
    }

    fn to(&mut self, next: Phase) {
        let allowed = matches!(
            (self.phase, next),
            (Phase::Unloaded, Phase::Idle)
                | (Phase::Idle, Phase::Saving)
                | (Phase::Saving, Phase::Idle)
        );
        assert!(
            allowed,
            "The favorites cannot go from {:?} to {next:?}",
            self.phase
        );
        self.phase = next;
    }

    /// The list shown: the core's with the unsent toggles made on it.
    pub fn list(&self) -> Vec<Key> {
        self.unsent
            .iter()
            .fold(self.saved.clone(), |list, key| toggled(&list, key))
    }

    /// Whether `provider`'s `model` is starred in the list shown.
    pub fn contains(&self, provider: &str, model: &str) -> bool {
        self.list().iter().any(|(p, m)| p == provider && m == model)
    }

    /// The core's list, read once at attach; a second load is an error.
    pub fn load(&mut self, list: Vec<Key>) -> Result<(), String> {
        if self.phase != Phase::Unloaded {
            return Err("The favorites have loaded already".into());
        }
        self.to(Phase::Idle);
        self.saved = list;
        Ok(())
    }

    /// The core published its list while no save is on its way (another
    /// window's change, or the list it keeps): it is the list now.
    pub fn published(&mut self, list: Vec<Key>) {
        assert_eq!(
            self.phase,
            Phase::Idle,
            "a published list is applied while no save is on its way"
        );
        assert!(self.unsent.is_empty(), "no toggle waits while idle");
        self.saved = list;
    }

    /// Stars or unstars `key`: the list to save now, when no save is on its
    /// way; an error before the core's list is known.
    pub fn toggle(&mut self, key: Key) -> Result<Option<Vec<Key>>, String> {
        if self.phase == Phase::Unloaded {
            return Err("The favorites have not loaded, so they cannot be changed yet".into());
        }
        self.unsent.push(key);
        Ok(if self.phase == Phase::Idle {
            Some(self.save())
        } else {
            None
        })
    }

    fn save(&mut self) -> Vec<Key> {
        self.to(Phase::Saving);
        toggled(&self.saved, &self.unsent[0])
    }

    /// The save on its way ended: the toggle is the core's now, or dropped;
    /// either way no longer unsent. The list to save next, if any toggle
    /// waits.
    pub fn ended(&mut self, saved: Saved) -> Option<Vec<Key>> {
        if let Saved::Stored(list) = saved {
            self.saved = list;
        }
        self.unsent.remove(0);
        self.to(Phase::Idle);
        if self.unsent.is_empty() {
            None
        } else {
            Some(self.save())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn k(key: &str) -> Key {
        let (p, m) = key.split_once("::").expect("a key");
        (p.to_string(), m.to_string())
    }

    fn ks(keys: &[&str]) -> Vec<Key> {
        keys.iter().map(|key| k(key)).collect()
    }

    #[test]
    fn nothing_is_starred_and_nothing_saved_before_the_servers_list_is_known() {
        let mut sync = Favorites::default();
        let refused = sync.toggle(k("p::m")).expect_err("not loaded");
        assert!(refused.contains("have not loaded"), "{refused}");
        assert_eq!(sync.list(), Vec::<Key>::new());
        sync.load(ks(&["p::a", "p::b"])).expect("the first load");
        assert_eq!(sync.list(), ks(&["p::a", "p::b"]));
        let write = sync.toggle(k("p::m")).expect("loaded");
        // The save is the server's list with the star on it, not this
        // window's alone.
        assert_eq!(write, Some(ks(&["p::a", "p::b", "p::m"])));
        let again = sync.load(Vec::new()).expect_err("a second load");
        assert!(again.contains("have loaded already"), "{again}");
    }

    #[test]
    fn a_star_that_could_not_be_saved_goes_back_and_says_so() {
        let mut sync = Favorites::default();
        sync.load(ks(&["p::a"])).expect("load");
        assert!(sync.toggle(k("p::m")).expect("loaded").is_some());
        assert_eq!(sync.list(), ks(&["p::a", "p::m"]));
        assert_eq!(sync.ended(Saved::Failed("disk full".into())), None);
        assert_eq!(sync.list(), ks(&["p::a"]));
        assert_eq!(sync.phase(), Phase::Idle);
        // Taking a star off that could not be saved puts it back on.
        assert!(sync.toggle(k("p::a")).expect("loaded").is_some());
        assert_eq!(sync.list(), Vec::<Key>::new());
        sync.ended(Saved::Failed("disk full".into()));
        assert_eq!(sync.list(), ks(&["p::a"]));
        // Through the seam a failed save answers the core's unchanged list,
        // which ends the same way.
        sync.toggle(k("p::z")).expect("loaded");
        sync.ended(Saved::Stored(ks(&["p::a"])));
        assert_eq!(sync.list(), ks(&["p::a"]));
    }

    #[test]
    fn stars_are_saved_one_at_a_time_in_order_and_a_failed_one_takes_no_other_with_it() {
        let mut sync = Favorites::default();
        sync.load(Vec::new()).expect("load");
        let mut writes = vec![sync.toggle(k("p::a")).expect("loaded").expect("sent")];
        assert_eq!(sync.toggle(k("p::b")).expect("loaded"), None);
        assert_eq!(sync.toggle(k("p::c")).expect("loaded"), None);
        assert_eq!(sync.list(), ks(&["p::a", "p::b", "p::c"]));
        assert_eq!(writes.len(), 1);
        writes.push(
            sync.ended(Saved::Stored(ks(&["p::a"])))
                .expect("the next save"),
        );
        // The second save fails: its star goes back, and the third is sent
        // without it.
        assert_eq!(writes[1], ks(&["p::a", "p::b"]));
        writes.push(
            sync.ended(Saved::Failed("refused".into()))
                .expect("the third save"),
        );
        assert_eq!(sync.list(), ks(&["p::a", "p::c"]));
        assert_eq!(writes[2], ks(&["p::a", "p::c"]));
        assert_eq!(sync.ended(Saved::Stored(ks(&["p::a", "p::c"]))), None);
        assert_eq!(sync.list(), ks(&["p::a", "p::c"]));
        assert_eq!(sync.phase(), Phase::Idle);
        assert_eq!(writes.len(), 3);
    }

    #[test]
    fn a_list_published_while_no_save_is_on_its_way_is_applied() {
        let mut sync = Favorites::default();
        sync.load(ks(&["a::1"])).expect("loads");
        sync.published(ks(&["a::1", "b::2"]));
        assert_eq!(sync.list(), ks(&["a::1", "b::2"]));
        // A failed save answers the unchanged list: the star goes back.
        let save = sync.toggle(k("c::3")).expect("loaded").expect("a save");
        assert_eq!(save, ks(&["a::1", "b::2", "c::3"]));
        assert!(sync.contains("c", "3"));
        assert_eq!(sync.ended(Saved::Stored(ks(&["a::1", "b::2"]))), None);
        assert!(!sync.contains("c", "3"));
        assert_eq!(sync.phase(), Phase::Idle);
    }
}
