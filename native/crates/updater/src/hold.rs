//! The hold on an install: two files beside the payload. The presence file
//! `.<name>.lock` is held shared by every process of the install for its
//! whole life; the gate file `.<name>.gate` is held exclusively by a
//! process only while it takes its hold (and, for a starting app, while it
//! decides whether to swap and swaps). Only a holder of the gate ever tries
//! the presence file exclusively, so between a starting app's decision and
//! its shared hold no other process can swap.
//!
//! Both files are opened read-only (`flock` and `LockFileEx` lock a read
//! handle); a missing one is made (mode 0644) where the folder lets it.
//! When a file is missing and cannot be made, cannot be opened, or the file
//! system refuses the lock (any error but "would block"), no hold is
//! possible: no swap ever happens from that start and the process runs as
//! it is.

use std::fs::{File, OpenOptions, TryLockError};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::Installed;

/// A process's hold on its install, released when dropped or when the
/// process ends (and by `exec`, which closes the files).
pub struct InstallHold {
    presence: Option<File>,
    /// Kept only by a start that swapped, until the new program runs.
    gate: Option<File>,
}

impl InstallHold {
    pub(crate) fn none() -> InstallHold {
        InstallHold {
            presence: None,
            gate: None,
        }
    }

    /// The presence file and the gate, for a start whose new program could
    /// not be executed and which goes on holding.
    pub(crate) fn into_files(self) -> (File, Option<File>) {
        (
            self.presence
                .expect("a swapping start holds the presence file"),
            self.gate,
        )
    }

    /// Whether this holds the install (false in a build without an
    /// updater, for a package manager's route, outside an install, or where
    /// no hold is possible).
    pub fn holds(&self) -> bool {
        self.presence.is_some()
    }
}

/// Why no hold is possible on an install, as one clause ("its lock file …
/// cannot be made: …").
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NoHold(pub String);

impl NoHold {
    /// The sentence [`crate::state`] shows.
    pub fn sentence(&self) -> String {
        format!("This install cannot update itself, because {}.", self.0)
    }
}

/// `installed`'s presence file, `.<name>.lock`.
pub(crate) fn presence_file(installed: &Installed) -> PathBuf {
    crate::swap::beside(&installed.path, ".lock")
}

/// `installed`'s gate file, `.<name>.gate`.
pub(crate) fn gate_file(installed: &Installed) -> PathBuf {
    crate::swap::beside(&installed.path, ".gate")
}

/// `path` opened read-only, made first (mode 0644) when it is missing.
fn open_read(path: &Path) -> Result<File, NoHold> {
    match File::open(path) {
        Ok(file) => return Ok(file),
        Err(e) if e.kind() != io::ErrorKind::NotFound => {
            return Err(NoHold(format!(
                "its lock file {} cannot be opened: {e}",
                path.display()
            )));
        }
        Err(_) => {}
    }
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o644);
    }
    match options.open(path) {
        Ok(_) => {}
        // Another process made it first.
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
        Err(e) => {
            return Err(NoHold(format!(
                "its lock file {} cannot be made: {e}",
                path.display()
            )));
        }
    }
    File::open(path).map_err(|e| {
        NoHold(format!(
            "its lock file {} cannot be opened: {e}",
            path.display()
        ))
    })
}

fn refused(path: &Path, e: io::Error) -> NoHold {
    NoHold(format!(
        "the file system refuses to lock {}: {e}",
        path.display()
    ))
}

/// How long a wait for the gate goes before it is written to `update.log`.
const GATE_PATIENCE: Duration = Duration::from_secs(5);

/// The gate taken exclusively, waiting for another holder as long as it
/// takes; after [`GATE_PATIENCE`] one line goes to `update.log` in `logs`
/// (the app's) and the wait goes on.
pub(crate) fn take_gate(installed: &Installed, logs: Option<&Path>) -> Result<File, NoHold> {
    let path = gate_file(installed);
    let gate = open_read(&path)?;
    let since = Instant::now();
    loop {
        match gate.try_lock() {
            Ok(()) => return Ok(gate),
            Err(TryLockError::WouldBlock) if since.elapsed() < GATE_PATIENCE => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(TryLockError::WouldBlock) => break,
            Err(TryLockError::Error(e)) => return Err(refused(&path, e)),
        }
    }
    if let Some(logs) = logs {
        crate::flow::log(
            logs,
            &format!(
                "Another start of this install has held {} for {} seconds; this start waits for it.",
                path.display(),
                GATE_PATIENCE.as_secs()
            ),
        );
    }
    gate.lock().map_err(|e| refused(&path, e))?;
    Ok(gate)
}

/// The presence file, taken exclusively when no other process holds it,
/// shared otherwise. Called only while the gate is held.
pub(crate) enum Presence {
    /// No other process of the install runs.
    Alone(File),
    /// Other processes of the install run; this one now holds it shared.
    Shared(File),
}

pub(crate) fn take_presence(installed: &Installed) -> Result<Presence, NoHold> {
    let path = presence_file(installed);
    let presence = open_read(&path)?;
    match presence.try_lock() {
        Ok(()) => Ok(Presence::Alone(presence)),
        Err(TryLockError::WouldBlock) => {
            presence.lock_shared().map_err(|e| refused(&path, e))?;
            Ok(Presence::Shared(presence))
        }
        Err(TryLockError::Error(e)) => Err(refused(&path, e)),
    }
}

/// Turns the exclusive presence into a shared one. Called only while the
/// gate is held, so no other process can take it exclusively between the
/// two calls.
pub(crate) fn share(installed: &Installed, presence: File) -> Result<File, NoHold> {
    let path = presence_file(installed);
    presence.unlock().map_err(|e| refused(&path, e))?;
    presence.lock_shared().map_err(|e| refused(&path, e))?;
    Ok(presence)
}

/// The shared hold of a process that keeps running (`presence` shared),
/// the gate released.
pub(crate) fn held(presence: File) -> InstallHold {
    InstallHold {
        presence: Some(presence),
        gate: None,
    }
}

/// The hold of a start that swapped: both files exclusive, until the new
/// program replaces this one.
pub(crate) fn swapping(presence: File, gate: File) -> InstallHold {
    InstallHold {
        presence: Some(presence),
        gate: Some(gate),
    }
}

/// What a process that only holds (a `headless` run, or an app that is not
/// the data folder's one instance) does: the gate taken, the presence held
/// shared, the gate released.
pub(crate) fn hold_shared(
    installed: &Installed,
    logs: Option<&Path>,
) -> Result<InstallHold, NoHold> {
    let gate = take_gate(installed, logs)?;
    let presence = match take_presence(installed)? {
        Presence::Alone(presence) => share(installed, presence)?,
        Presence::Shared(presence) => presence,
    };
    drop(gate);
    Ok(held(presence))
}
