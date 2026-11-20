//! The change notification of a sign-in ([`crate::login::SignIns`]) and of
//! a CLI install ([`crate::harness::install::CliInstaller`]): ONE mechanism
//! for both. The watcher given to `watch` is called with each new view, on
//! the thread that changed it, while the state it reports is still held, so
//! the calls come in the order of the changes and each change once. It must
//! return at once and must not read the sign-ins or the installs back (their
//! lock is held): the core hands each view to the window's channel.
//!
//! Without a watcher nothing is called; the state is read on request as
//! before (`state`).

use std::sync::{Arc, OnceLock};

/// What a watcher is: a function of the new view.
pub type Watcher<T> = Arc<dyn Fn(T) + Send + Sync>;

/// The one watcher of a set of sign-ins or installs, set once.
pub struct Changes<T>(OnceLock<Watcher<T>>);

impl<T> Default for Changes<T> {
    fn default() -> Self {
        Changes(OnceLock::new())
    }
}

impl<T> Changes<T> {
    /// Sets the watcher; a second one is a programming error.
    pub fn watch(&self, watcher: Watcher<T>) {
        if self.0.set(watcher).is_err() {
            panic!("a set of sign-ins or installs has one watcher");
        }
    }

    /// Tells the watcher, if any, of `view`.
    pub(crate) fn changed(&self, view: T) {
        if let Some(watcher) = self.0.get() {
            watcher(view);
        }
    }
}
