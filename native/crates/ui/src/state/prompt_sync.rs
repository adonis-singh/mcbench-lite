//! The prompt library in the window (`web/prompt-sync.mjs`, tests ported
//! from `test/prompt-sync.test.js`, same cases and names; inventory
//! `app.md` 8.7). It lives in the core, and the builders can change it too
//! (the `prompt_library` tool). Local edits show at once and reach the core
//! shortly after. Each edit is a function of the list, kept in `unsent`
//! until a save that carried it succeeds: a reload (a builder changed the
//! library) lands the core's list with those edits made again on it, so
//! neither side's change is lost, and a save that failed goes again with
//! the next one.
//!
//! | Phase | Goes to | What |
//! |---|---|---|
//! | unloaded | loading | nothing read yet |
//! | loading | ready, failed | the first read on its way: there is no list to edit |
//! | failed | loading | that read failed (`error` says why); [`PromptSync::load`] reads again |
//! | ready | saving, reloading | the list is the core's with the unsent edits on it |
//! | saving | ready | the list on its way to the core |
//! | reloading | ready | the core's list being read again |
//!
//! One request at a time: what is asked for meanwhile happens when it ends.
//! The page's promises become [`Effect`]s the window carries out
//! ([`PromptSync::take_effects`]): a read is `Command::ReadPrompts` and its
//! `Update::Prompts`, a write `Command::SavePrompts` and its
//! `Update::PromptsSaved`, a wait the 300 ms timer before a save.

use std::rc::Rc;
use std::time::Duration;

/// How long edits gather before they are saved (`delay`).
pub const DELAY: Duration = Duration::from_millis(300);

/// The sentence of an edit made before the library loaded.
pub const NOT_LOADED: &str = "The prompt library has not loaded, so it cannot be changed yet";

/// A change to the library: a function of the list.
pub type Change<T> = Rc<dyn Fn(&[T]) -> Vec<T>>;

/// The phase.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// `unloaded`.
    Unloaded,
    /// `loading`.
    Loading,
    /// `failed`.
    Failed,
    /// `ready`.
    Ready,
    /// `saving`.
    Saving,
    /// `reloading`.
    Reloading,
}

impl Phase {
    /// The page's name for it.
    pub fn name(self) -> &'static str {
        match self {
            Phase::Unloaded => "unloaded",
            Phase::Loading => "loading",
            Phase::Failed => "failed",
            Phase::Ready => "ready",
            Phase::Saving => "saving",
            Phase::Reloading => "reloading",
        }
    }

    fn next(self) -> &'static [Phase] {
        match self {
            Phase::Unloaded => &[Phase::Loading],
            Phase::Loading => &[Phase::Ready, Phase::Failed],
            Phase::Failed => &[Phase::Loading],
            Phase::Ready => &[Phase::Saving, Phase::Reloading],
            Phase::Saving | Phase::Reloading => &[Phase::Ready],
        }
    }
}

/// The phases with a list read from the core: only then can it be edited,
/// and so saved (`promptsLoaded`).
pub fn prompts_loaded(phase: Phase) -> bool {
    matches!(phase, Phase::Ready | Phase::Saving | Phase::Reloading)
}

/// What the window does for the sync.
#[derive(Clone, Debug, PartialEq)]
pub enum Effect<T> {
    /// The phase or the list changed (`onChange`).
    Changed,
    /// Read the core's list (`read`).
    Read,
    /// Save this list (`write`).
    Write(Vec<T>),
    /// Call [`PromptSync::timer_fired`] with this number after [`DELAY`].
    Wait(u64),
    /// A load, a save or a reload failed (`onError`): a notice.
    Error(String),
}

/// The library and its sync.
pub struct PromptSync<T> {
    phase: Phase,
    error: Option<String>,
    list: Vec<T>,
    unsent: Vec<Change<T>>,
    /// The core's list changed since it was read: it is read again before
    /// anything is saved over it.
    stale: bool,
    /// The save that waits for more edits, in phase ready: its number.
    timer: Option<u64>,
    timers: u64,
    /// How many edits the save on its way carries.
    sending: usize,
    effects: Vec<Effect<T>>,
}

impl<T: Clone> Default for PromptSync<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Clone> PromptSync<T> {
    /// Nothing read yet.
    pub fn new() -> Self {
        PromptSync {
            phase: Phase::Unloaded,
            error: None,
            list: Vec::new(),
            unsent: Vec::new(),
            stale: false,
            timer: None,
            timers: 0,
            sending: 0,
            effects: Vec::new(),
        }
    }

    /// The phase.
    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// Why the first read failed, in phase failed.
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// The list as shown: the core's with the unsent edits on it.
    pub fn list(&self) -> &[T] {
        &self.list
    }

    /// How many edits have not reached the core.
    pub fn unsent(&self) -> usize {
        self.unsent.len()
    }

    /// What the window is to do, in order.
    pub fn take_effects(&mut self) -> Vec<Effect<T>> {
        std::mem::take(&mut self.effects)
    }

    fn to(&mut self, phase: Phase, error: Option<String>) {
        if !self.phase.next().contains(&phase) {
            panic!(
                "The prompt library cannot go from {} to {}",
                self.phase.name(),
                phase.name()
            );
        }
        if (phase == Phase::Failed) != error.is_some() {
            panic!("A failed load, and only one, has an error");
        }
        self.phase = phase;
        self.error = error;
        self.effects.push(Effect::Changed);
    }

    /// The first read, or another after one that failed.
    pub fn load(&mut self) {
        self.to(Phase::Loading, None);
        self.stale = false;
        self.effects.push(Effect::Read);
    }

    /// A change to the library. Before the list is read there is nothing to
    /// make it on, and saving it would replace the core's library with it:
    /// `Err` with [`NOT_LOADED`].
    pub fn edit(&mut self, change: Change<T>) -> Result<(), String> {
        if !prompts_loaded(self.phase) {
            return Err(NOT_LOADED.into());
        }
        self.list = change(&self.list);
        self.unsent.push(change);
        self.effects.push(Effect::Changed);
        if self.phase == Phase::Ready {
            self.next();
        }
        Ok(())
    }

    /// The core's list changed.
    pub fn reload(&mut self) {
        self.stale = true;
        if self.phase == Phase::Ready {
            self.next();
        }
    }

    /// Ready, with nothing on its way: a stale list is read again, and
    /// otherwise the unsent edits are saved once no more come for a moment.
    fn next(&mut self) {
        self.timer = None;
        if self.stale {
            self.reread();
        } else if !self.unsent.is_empty() {
            self.timers += 1;
            self.timer = Some(self.timers);
            self.effects.push(Effect::Wait(self.timers));
        }
    }

    /// The wait numbered `timer` ended: the save goes, unless a later edit
    /// or a read took the wait's place.
    pub fn timer_fired(&mut self, timer: u64) {
        if self.timer == Some(timer) {
            self.save();
        }
    }

    /// A save that fails keeps its edits unsent and is not tried again by
    /// itself: the next edit, or reload, sends them.
    fn save(&mut self) {
        self.timer = None;
        self.sending = self.unsent.len();
        self.to(Phase::Saving, None);
        self.effects.push(Effect::Write(self.list.clone()));
    }

    /// The save on its way ended.
    pub fn write_landed(&mut self, result: Result<(), String>) {
        assert_eq!(
            self.phase,
            Phase::Saving,
            "a save's answer came with no save on its way"
        );
        match result {
            Ok(()) => {
                // Edits made since are still to go.
                self.unsent.drain(..self.sending);
                self.to(Phase::Ready, None);
                self.next();
            }
            Err(error) => {
                self.to(Phase::Ready, None);
                self.effects.push(Effect::Error(error));
                if self.stale {
                    self.reread();
                }
            }
        }
    }

    /// A read that fails leaves the list stale, so the next edit reads it
    /// again before saving.
    fn reread(&mut self) {
        self.to(Phase::Reloading, None);
        self.stale = false;
        self.effects.push(Effect::Read);
    }

    /// Whether a read is on its way (its answer is the next library the
    /// core publishes).
    pub fn reading(&self) -> bool {
        matches!(self.phase, Phase::Loading | Phase::Reloading)
    }

    /// The read on its way ended: the first load's, or a reread's.
    pub fn read_landed(&mut self, result: Result<Vec<T>, String>) {
        match (self.phase, result) {
            (Phase::Loading, Ok(list)) => {
                self.list = list;
                self.to(Phase::Ready, None);
                self.next();
            }
            (Phase::Loading, Err(error)) => {
                self.to(Phase::Failed, Some(error.clone()));
                self.effects.push(Effect::Error(error));
            }
            (Phase::Reloading, Ok(list)) => {
                self.list = self.unsent.iter().fold(list, |made, change| change(&made));
                self.to(Phase::Ready, None);
                self.next();
            }
            (Phase::Reloading, Err(error)) => {
                self.stale = true;
                self.to(Phase::Ready, None);
                self.effects.push(Effect::Error(error));
            }
            (phase, _) => panic!("a read's answer came in phase {}", phase.name()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Debug, PartialEq)]
    struct P(&'static str);

    /// A library whose reads and writes are settled by hand: `reads` counts
    /// each read asked for, `writes` holds each list as it was sent.
    struct Library {
        sync: PromptSync<P>,
        reads: usize,
        writes: Vec<Vec<P>>,
        phases: Vec<&'static str>,
        errors: Vec<String>,
        waits: Vec<u64>,
    }

    impl Library {
        fn new() -> Self {
            Library {
                sync: PromptSync::new(),
                reads: 0,
                writes: Vec::new(),
                phases: Vec::new(),
                errors: Vec::new(),
                waits: Vec::new(),
            }
        }

        fn drain(&mut self) {
            for effect in self.sync.take_effects() {
                match effect {
                    Effect::Changed => self.phases.push(self.sync.phase().name()),
                    Effect::Read => self.reads += 1,
                    Effect::Write(list) => self.writes.push(list),
                    Effect::Wait(timer) => self.waits.push(timer),
                    Effect::Error(error) => self.errors.push(error),
                }
            }
        }

        /// Past the save timer (delay 0 in the page's test) and every
        /// reaction.
        fn settled(&mut self) {
            self.drain();
            while !self.waits.is_empty() {
                for timer in std::mem::take(&mut self.waits) {
                    self.sync.timer_fired(timer);
                }
                self.drain();
            }
        }

        fn load(&mut self) {
            self.sync.load();
            self.drain();
        }

        fn edit(&mut self, change: Change<P>) -> Result<(), String> {
            let done = self.sync.edit(change);
            self.drain();
            done
        }

        fn reload(&mut self) {
            self.sync.reload();
            self.drain();
        }

        fn resolve_read(&mut self, list: Vec<P>) {
            self.sync.read_landed(Ok(list));
            self.drain();
        }

        fn reject_read(&mut self, error: &str) {
            self.sync.read_landed(Err(error.into()));
            self.drain();
        }

        fn resolve_write(&mut self) {
            self.sync.write_landed(Ok(()));
            self.drain();
        }

        fn reject_write(&mut self, error: &str) {
            self.sync.write_landed(Err(error.into()));
            self.drain();
        }
    }

    fn add(id: &'static str) -> Change<P> {
        Rc::new(move |list: &[P]| {
            let mut list = list.to_vec();
            list.push(P(id));
            list
        })
    }

    fn ids(list: &[P]) -> Vec<&'static str> {
        list.iter().map(|p| p.0).collect()
    }

    fn panics(f: impl FnOnce() + std::panic::UnwindSafe) -> String {
        let error = std::panic::catch_unwind(f).expect_err("it refuses");
        error
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| error.downcast_ref::<&str>().map(|s| s.to_string()))
            .expect("a message")
    }

    #[test]
    fn no_edit_is_taken_and_nothing_is_saved_before_the_library_has_loaded() {
        let mut l = Library::new();
        assert_eq!(l.sync.phase(), Phase::Unloaded);
        assert!(
            l.edit(add("early"))
                .expect_err("refused")
                .contains("has not loaded")
        );
        l.load();
        assert_eq!(l.sync.phase(), Phase::Loading);
        assert!(
            l.edit(add("early"))
                .expect_err("refused")
                .contains("has not loaded")
        );
        l.reject_read("HTTP 500");
        l.settled();
        assert_eq!(l.sync.phase(), Phase::Failed);
        assert_eq!(l.sync.error(), Some("HTTP 500"));
        assert_eq!(l.errors, vec!["HTTP 500"]);
        assert!(
            l.edit(add("early"))
                .expect_err("refused")
                .contains("has not loaded")
        );
        l.settled();
        // The refused edits are not queued against the empty list.
        assert!(l.sync.list().is_empty() && l.sync.unsent() == 0 && l.writes.is_empty());

        // The failed load is read again, and then edits are taken and saved.
        l.load();
        l.resolve_read(vec![P("a")]);
        l.settled();
        assert_eq!(l.sync.phase(), Phase::Ready);
        assert_eq!(l.sync.error(), None);
        l.edit(add("b")).expect("taken");
        l.settled();
        assert_eq!(l.sync.phase(), Phase::Saving);
        assert_eq!(ids(&l.writes[0]), ["a", "b"]);
        l.resolve_write();
        l.settled();
        assert_eq!(
            (l.sync.phase(), l.sync.unsent(), l.writes.len()),
            (Phase::Ready, 0, 1)
        );
        assert_eq!(
            l.phases,
            [
                "loading", "failed", "loading", "ready", "ready", "saving", "ready"
            ]
        );
    }

    #[test]
    fn a_load_cannot_start_while_one_is_on_its_way_or_once_it_has_landed() {
        let message = panics(|| {
            let mut l = Library::new();
            l.load();
            l.load();
        });
        assert!(message.contains("cannot go from loading to loading"));
        let message = panics(|| {
            let mut l = Library::new();
            l.load();
            l.resolve_read(Vec::new());
            l.settled();
            l.load();
        });
        assert!(message.contains("cannot go from ready to loading"));
    }

    #[test]
    fn a_failed_save_keeps_its_edits_which_go_again_with_the_next_one() {
        let mut l = Library::new();
        l.load();
        l.resolve_read(vec![P("a")]);
        l.settled();
        l.edit(add("b")).expect("taken");
        l.settled();
        l.reject_write("disk full");
        l.settled();
        assert_eq!(
            (l.sync.phase(), l.sync.unsent(), l.errors.clone()),
            (Phase::Ready, 1, vec!["disk full".to_string()])
        );
        // It is not sent again by itself.
        l.settled();
        assert_eq!(l.writes.len(), 1);

        // A reload meanwhile lands the core's list with the unsent edit on it.
        l.reload();
        assert_eq!(l.sync.phase(), Phase::Reloading);
        l.resolve_read(vec![P("a"), P("builder")]);
        l.settled();
        assert_eq!(ids(l.sync.list()), ["a", "builder", "b"]);
        // And sends what the failed save held.
        assert_eq!(l.sync.phase(), Phase::Saving);
        assert_eq!(ids(&l.writes[1]), ["a", "builder", "b"]);
        l.reject_write("disk full");
        l.settled();
        assert_eq!(l.sync.unsent(), 1);

        // The next edit's save carries both.
        l.edit(add("c")).expect("taken");
        l.settled();
        assert_eq!(ids(&l.writes[2]), ["a", "builder", "b", "c"]);
        l.resolve_write();
        l.settled();
        assert_eq!((l.sync.phase(), l.sync.unsent()), (Phase::Ready, 0));
        l.settled();
        assert_eq!(l.writes.len(), 3);
    }

    #[test]
    fn an_edit_made_while_a_save_is_on_its_way_stays_unsent_until_its_own_save() {
        let mut l = Library::new();
        l.load();
        l.resolve_read(Vec::new());
        l.settled();
        l.edit(add("a")).expect("taken");
        l.settled();
        assert_eq!(l.sync.phase(), Phase::Saving);
        l.edit(add("b")).expect("taken");
        assert_eq!(ids(&l.writes[0]), ["a"]);
        l.resolve_write();
        l.settled();
        // Only the edit the save carried is cleared; the other goes next.
        assert_eq!(l.sync.phase(), Phase::Saving);
        assert_eq!(l.sync.unsent(), 1);
        assert_eq!(ids(&l.writes[1]), ["a", "b"]);
        l.resolve_write();
        l.settled();
        assert_eq!((l.sync.phase(), l.sync.unsent()), (Phase::Ready, 0));
    }

    #[test]
    fn edits_made_close_together_are_saved_once() {
        let mut l = Library::new();
        l.load();
        l.resolve_read(Vec::new());
        l.settled();
        l.edit(add("a")).expect("taken");
        l.edit(add("b")).expect("taken");
        l.edit(add("c")).expect("taken");
        l.settled();
        assert_eq!(l.writes.len(), 1);
        assert_eq!(ids(&l.writes[0]), ["a", "b", "c"]);
    }

    #[test]
    fn a_reload_waits_for_the_save_on_its_way_and_an_edit_made_during_it_is_kept() {
        let mut l = Library::new();
        l.load();
        l.resolve_read(vec![P("a")]);
        l.settled();
        l.edit(add("b")).expect("taken");
        l.settled();
        l.reload();
        assert_eq!(l.sync.phase(), Phase::Saving);
        assert_eq!(l.reads, 1);
        l.resolve_write();
        l.settled();
        assert_eq!(l.sync.phase(), Phase::Reloading);
        l.edit(add("c")).expect("taken");
        assert_eq!(ids(l.sync.list()), ["a", "b", "c"]);
        // Another change in the core while this read is out: it is read
        // again.
        l.reload();
        l.resolve_read(vec![P("a"), P("b"), P("x")]);
        l.settled();
        assert_eq!(l.sync.phase(), Phase::Reloading);
        assert_eq!(l.writes.len(), 1);
        l.resolve_read(vec![P("a"), P("b"), P("x"), P("y")]);
        l.settled();
        assert_eq!(ids(l.sync.list()), ["a", "b", "x", "y", "c"]);
        assert_eq!(l.sync.phase(), Phase::Saving);
        assert_eq!(ids(&l.writes[1]), ["a", "b", "x", "y", "c"]);
    }

    #[test]
    fn a_reload_asked_for_while_the_first_read_is_out_follows_it() {
        let mut l = Library::new();
        l.load();
        l.reload();
        l.resolve_read(vec![P("old")]);
        l.settled();
        assert_eq!(l.sync.phase(), Phase::Reloading);
        l.resolve_read(vec![P("new")]);
        l.settled();
        assert_eq!(
            (l.sync.phase(), ids(l.sync.list())),
            (Phase::Ready, vec!["new"])
        );
    }

    #[test]
    fn a_failed_reload_leaves_the_list_stale_it_is_read_again_before_anything_is_saved_over_the_server()
     {
        let mut l = Library::new();
        l.load();
        l.resolve_read(vec![P("a")]);
        l.settled();
        l.reload();
        l.reject_read("offline");
        l.settled();
        assert_eq!(
            (l.sync.phase(), l.errors.clone(), ids(l.sync.list())),
            (Phase::Ready, vec!["offline".to_string()], vec!["a"])
        );
        l.edit(add("b")).expect("taken");
        assert_eq!(l.sync.phase(), Phase::Reloading);
        l.settled();
        assert!(l.writes.is_empty());
        l.resolve_read(vec![P("a"), P("builder")]);
        l.settled();
        assert_eq!(l.sync.phase(), Phase::Saving);
        assert_eq!(ids(&l.writes[0]), ["a", "builder", "b"]);
    }
}
