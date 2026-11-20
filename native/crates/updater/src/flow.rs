//! The app's use of the updater: [`at_start`] before the core and any
//! window, [`Updater::check_in_background`] after the core started, and
//! [`state`] for Settings.

use std::cell::RefCell;
use std::io::Write as _;
use std::panic::AssertUnwindSafe;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, Once};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tokio_util::sync::CancellationToken;

use crate::hold::{self, InstallHold, NoHold, Presence};
use crate::latest::{
    Latest, Problem, compiled_in, download_checked, fetch_latest, hex, parse_version, release_of,
    update_file,
};
use crate::swap::{Failure, beside};
use crate::{
    Installed, Target, UpdateKey, UpdateState, current_exe, install, install_source_at,
    remove_leftover, restart, verify_waiting,
};
use sha2::{Digest, Sha256};

/// This build's version, the app's (the workspace's one version).
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The time between two checks.
const DAY: Duration = Duration::from_secs(24 * 60 * 60);

/// The name of the updater's thread.
pub const THREAD_NAME: &str = "mcbench-updater";

/// The file in the logs folder the updater writes its failures to.
pub const LOG_FILE: &str = "update.log";

static STATE: Mutex<UpdateState> = Mutex::new(UpdateState::Off);

thread_local! {
    /// The logs folder on the updater's threads (the thread [`guarded`]
    /// runs work on, and every thread of the runtime [`runtime`] builds);
    /// `None` on every other thread.
    static UPDATER_LOGS: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
}

/// Marks this thread as one of the updater's, its panics written to
/// `update.log` in `logs`.
fn mark(logs: Option<&Path>) {
    UPDATER_LOGS.with(|l| *l.borrow_mut() = logs.map(Path::to_path_buf));
}

/// Installs the updater's panic hook once, chained before the hook that is
/// set now: [`at_start`] calls it on the main thread, after the crash hook
/// and before the core and the engine chain their own hooks before it. A
/// panic on one of the updater's threads ([`mark`]) is written to
/// `update.log` and goes no further, so the crash hook does not end the
/// process; every other panic goes to the hook it wraps.
pub(crate) fn install_panic_hook() {
    static HOOK: Once = Once::new();
    HOOK.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            // try_with: a thread's locals may already be gone when it
            // panics while exiting, and a panic inside a hook aborts.
            let logs = UPDATER_LOGS
                .try_with(|l| l.try_borrow().ok().and_then(|l| l.clone()))
                .ok()
                .flatten();
            let Some(logs) = logs else {
                return previous(info);
            };
            let message = info
                .payload()
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| info.payload().downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "a panic with a payload that is not text".to_string());
            let location = info
                .location()
                .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
                .unwrap_or_else(|| "an unknown location".to_string());
            log(
                &logs,
                &format!("The updater panicked at {location}: {message}"),
            );
        }));
    });
}

/// The runtime of one check: current-thread, every thread it starts (its
/// blocking pool) named [`THREAD_NAME`] and marked as the updater's.
fn runtime(logs: &Path) -> std::io::Result<tokio::runtime::Runtime> {
    let logs = logs.to_path_buf();
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .thread_name(THREAD_NAME)
        .on_thread_start(move || mark(Some(&logs)))
        .build()
}

/// What Settings shows of updates now: [`UpdateState::Off`] until
/// [`at_start`] ran, then the route's line, the check's progress, or the
/// version that starts next time.
pub fn state() -> UpdateState {
    STATE.lock().expect("the update state's lock").clone()
}

fn set(state: UpdateState) {
    *STATE.lock().expect("the update state's lock") = state;
}

/// Where the updater reads `latest.json` and the key it trusts.
#[derive(Clone, Debug)]
pub struct Feed {
    /// The address of `latest.json`; the assets are beside it.
    pub url: String,
    /// The first key of the chain this build follows.
    pub key: UpdateKey,
}

impl Feed {
    /// The feed compiled into this build (`MCBENCH_UPDATE_FEED` and
    /// `MCBENCH_UPDATE_KEY_FILE` at build time); `None` for a build without
    /// an updater, which makes no request.
    pub fn compiled_in() -> Option<Feed> {
        compiled_in().map(|(key, url)| Feed {
            url: url.to_string(),
            key,
        })
    }
}

/// What the start does after [`start_with`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Start {
    /// Start the app that is running.
    AsIs,
    /// The waiting update of this version was swapped in: run the new
    /// program instead.
    Restart {
        /// The new version.
        version: String,
    },
    /// The waiting update could not be swapped in, or its version is held
    /// ([`Held`]); this process does not take that version.
    Held(Held),
    /// The installed payload is not this program's (its `version` differs,
    /// or this process was loaded from a payload a swap moved aside): the
    /// installed program runs instead.
    Changed,
    /// No hold is possible on this install: the app starts as it is and
    /// never updates itself from this start.
    Unable(NoHold),
}

/// A version this process does not take: its swap failed at this start, or
/// its program could not be started (kept in `.<name>.update.held` beside
/// the payload, so later starts do not take it either, until the feed
/// offers a newer one). [`state`] shows the sentence, and the background
/// check does not fetch that version again.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Held {
    /// The version.
    pub version: String,
    /// Why, as `update.log` has it.
    pub sentence: String,
}

/// The updater of a running app, from [`at_start`]: either nothing (no
/// updater in this build, or a package manager's route) or the install it
/// checks for.
pub struct Updater {
    active: Option<Active>,
    _hold: InstallHold,
}

#[derive(Clone)]
struct Active {
    feed: Feed,
    installed: Installed,
    logs: PathBuf,
    held: Option<Held>,
}

/// The `latest.json` a waiting download was checked against:
/// `.<name>.update.json` beside the payload `path`.
fn waiting_json(path: &Path) -> PathBuf {
    beside(path, ".update.json")
}

/// Appends `sentence` to `update.log` in `logs`, after the time in seconds
/// since 1970; when that cannot be written, stderr carries it.
pub(crate) fn log(logs: &Path, sentence: &str) {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let path = logs.join(LOG_FILE);
    let line = format!("{seconds} {sentence}\n");
    let written = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .and_then(|mut f| f.write_all(line.as_bytes()));
    if let Err(e) = written {
        eprintln!(
            "mcbench-lite: {} cannot be written ({e}): {line}",
            path.display()
        );
    }
}

/// The size `update.log` is cut to at start when it is larger.
const LOG_KEEP: u64 = 256 * 1024;

/// Cuts `update.log` in `logs` to its last [`LOG_KEEP`] bytes, from the
/// first whole line, when it is larger.
fn trim_log(logs: &Path) {
    let path = logs.join(LOG_FILE);
    let Ok(text) = std::fs::read(&path) else {
        return;
    };
    let len = text.len() as u64;
    if len <= LOG_KEEP {
        return;
    }
    let cut = (len - LOG_KEEP) as usize;
    let start = text[cut..]
        .iter()
        .position(|&b| b == b'\n')
        .map_or(text.len(), |i| cut + i + 1);
    let part = logs.join(format!("{LOG_FILE}.part"));
    if let Err(e) =
        std::fs::write(&part, &text[start..]).and_then(|()| std::fs::rename(&part, &path))
    {
        let _ = std::fs::remove_file(&part);
        eprintln!("mcbench-lite: {} cannot be cut: {e}", path.display());
    }
}

/// Removes `path`; a missing file is not an error.
fn remove(path: &Path) -> Result<(), String> {
    match std::fs::remove_file(path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
            Err(format!("{} cannot be removed: {e}", path.display()))
        }
        _ => Ok(()),
    }
}

/// The file that keeps a held version across starts: `.<name>.update.held`
/// beside the payload, the version on its first line and the sentence on
/// its second.
fn held_file(installed: &Installed) -> PathBuf {
    beside(&installed.path, ".update.held")
}

/// The version held by an earlier start, if any and if it is newer than
/// `current`; a held version no newer (installed since by other means, or
/// older) is forgotten and its file removed.
pub fn read_held(installed: &Installed, current: &str) -> Option<Held> {
    let path = held_file(installed);
    let text = std::fs::read_to_string(&path).ok()?;
    let (version, sentence) = text.split_once('\n')?;
    let newer = match (
        parse_version(version, "The held"),
        parse_version(current, "This app's"),
    ) {
        (Ok(held), Ok(running)) => held > running,
        _ => false,
    };
    if !newer {
        let _ = std::fs::remove_file(&path);
        return None;
    }
    Some(Held {
        version: version.to_string(),
        sentence: sentence.trim_end().to_string(),
    })
}

fn write_held(installed: &Installed, held: &Held, logs: &Path) {
    let path = held_file(installed);
    if let Err(e) = std::fs::write(&path, format!("{}\n{}\n", held.version, held.sentence)) {
        log(logs, &format!("{} cannot be written: {e}", path.display()));
    }
}

/// What this program's install is to the updater.
enum Route {
    /// A package manager's route, with what Settings shows.
    PackageManager(UpdateState),
    /// A route the updater serves; `moved` when this process was loaded
    /// from a payload a swap has moved aside ([`crate::moved_from`]).
    Updates { installed: Installed, moved: bool },
}

/// The route of the executable `exe` (`APPIMAGE` given as `appimage`),
/// found at the install's real path when a swap moved it. A missing or
/// unreadable `install-source` or `version` file is the error.
fn route_at(exe: &Path, appimage: Option<PathBuf>) -> Result<Route, String> {
    let route = locate_route(exe, appimage)?;
    if let Route::Updates { installed, .. } = &route {
        installed.installed_version()?;
    }
    Ok(route)
}

/// [`route_at`] without reading the `version` file: a missing or
/// unreadable `install-source` is the error.
fn locate_route(exe: &Path, appimage: Option<PathBuf>) -> Result<Route, String> {
    let (exe, moved) = match crate::moved_from(exe) {
        Some(real) => (real, true),
        None => (exe.to_path_buf(), false),
    };
    let source = install_source_at(&exe)?;
    if let Some(line) = source.package_manager() {
        return Ok(Route::PackageManager(line));
    }
    let target = Target::of_this_build(source)?;
    let installed = Installed::locate(&exe, target, appimage)?;
    Ok(Route::Updates { installed, moved })
}

/// The program a restart of this process executes ([`installed_program_for`]
/// with `std::env::current_exe()`, symlinks resolved, and `APPIMAGE`). In a
/// build without an updater it is `current_exe()` itself. The error is
/// `current_exe()`'s, as one sentence: the path of the running executable is
/// unknown, so there is nothing to execute; the caller shows it.
pub fn installed_program() -> Result<PathBuf, String> {
    let exe = current_exe()?;
    if Feed::compiled_in().is_none() {
        return Ok(exe);
    }
    Ok(installed_program_for(&exe, appimage()))
}

/// The program a restart of the executable `exe` (`APPIMAGE` given as
/// `appimage`) executes, whatever the build. For a self-updating install it
/// is [`Installed::program`], the program [`restart`] runs: on macOS
/// `<bundle>/Contents/MacOS/mcbench-lite`, on Linux's `script` route
/// `~/.local/lib/mcbench-lite/mcbench-lite`, on Windows `mcbench-lite.exe`
/// in the payload folder, each at the install's real path when `exe` was
/// loaded from a payload a swap moved aside ([`crate::moved_from`]); for the
/// AppImage it is the AppImage file. For a package manager's route, for a
/// program outside an install (no readable `install-source`), and for an
/// install this build cannot locate (a route of another system, or
/// `APPIMAGE` not set for the AppImage) it is `exe`. The `version` file is
/// not read: a restarted app reports a broken install itself.
pub fn installed_program_for(exe: &Path, appimage: Option<PathBuf>) -> PathBuf {
    match locate_route(exe, appimage) {
        Ok(Route::Updates { installed, .. }) => installed.program,
        Ok(Route::PackageManager(_)) | Err(_) => exe.to_path_buf(),
    }
}

/// Whether the payload at `installed` is no longer the program at version
/// `current`: its `version` file says another version (the AppImage, which
/// runs wholly from its image, never). A file that cannot be read is its
/// own error, reported as itself.
fn replaced(installed: &Installed, current: &str) -> Result<bool, String> {
    Ok(installed
        .installed_version()?
        .is_some_and(|version| version != current))
}

fn appimage() -> Option<PathBuf> {
    std::env::var_os("APPIMAGE").map(PathBuf::from)
}

/// The one sentence a `headless` start prints before it exits with status
/// 2 when the installed payload is not this program's: it was replaced
/// while the process started, or since the process was loaded.
pub const CHANGED_WHILE_STARTING: &str = "mcbench-lite was updated on disk while this process was starting, so it stopped before doing anything; start it again.";

/// The hold a `headless` run keeps on its install for as long as it lives,
/// so no starting app swaps the payload under it: `mcbench-lite`'s
/// `main.rs` calls it before `mcbench_headless::run`. It holds nothing and
/// makes no file in a build without an updater, for a package manager's
/// route, for a program that is not in an install (no readable
/// `install-source` or `version`: a release binary run from an unpacked
/// archive), and where no hold is possible; it cannot fail. When the
/// installed payload is not this program's (its `version` differs once the
/// hold is taken, or this process was loaded from a payload a swap moved
/// aside), it prints [`CHANGED_WHILE_STARTING`] on stderr and exits with
/// status 2.
pub fn hold_install() -> InstallHold {
    if Feed::compiled_in().is_none() {
        return InstallHold::none();
    }
    match current_exe() {
        Ok(exe) => hold_install_for(&exe, appimage(), VERSION),
        Err(_) => InstallHold::none(),
    }
}

/// [`hold_install`] for the executable `exe` at version `current`,
/// whatever the build.
pub fn hold_install_for(exe: &Path, appimage: Option<PathBuf>, current: &str) -> InstallHold {
    match hold_of_program(exe, appimage, current) {
        Ok(hold) => hold,
        Err(NotThisProgram::Replaced) => {
            eprintln!("{CHANGED_WHILE_STARTING}");
            std::process::exit(2)
        }
        Err(NotThisProgram::Unreadable(sentence)) => {
            eprintln!("{sentence}");
            std::process::exit(2)
        }
    }
}

/// The hold the error screen's process keeps on its install for as long as
/// it lives: the same shared hold as [`hold_install`] (the gate taken, the
/// presence file held shared, the gate released), for a process that uses
/// nothing of the payload, so no starting app swaps the payload under it.
/// It never prints and never exits: when the installed payload is not this
/// program's (its `version` differs once the hold is taken), when the
/// `version` file cannot be read, or when this process was loaded from a
/// payload a swap moved aside, it holds nothing. As [`hold_install`], it
/// holds nothing and makes no file in a build without an updater, for a
/// package manager's route, outside an install, and where no hold is
/// possible.
pub fn hold_install_quietly() -> InstallHold {
    if Feed::compiled_in().is_none() {
        return InstallHold::none();
    }
    match current_exe() {
        Ok(exe) => hold_install_quietly_for(&exe, appimage(), VERSION),
        Err(_) => InstallHold::none(),
    }
}

/// [`hold_install_quietly`] for the executable `exe` at version `current`,
/// whatever the build.
pub fn hold_install_quietly_for(
    exe: &Path,
    appimage: Option<PathBuf>,
    current: &str,
) -> InstallHold {
    hold_of_program(exe, appimage, current).unwrap_or_else(|_| InstallHold::none())
}

/// The shared hold of the program `exe` at version `current` ([`hold_install_at`]
/// at the install's real path): no hold outside a self-updating install or
/// where none is possible, and `Err` when the payload is not this program's
/// (moved aside, another version, or an unreadable `version`).
fn hold_of_program(
    exe: &Path,
    appimage: Option<PathBuf>,
    current: &str,
) -> Result<InstallHold, NotThisProgram> {
    let Ok(Route::Updates { installed, moved }) = route_at(exe, appimage) else {
        return Ok(InstallHold::none());
    };
    if moved {
        return Err(NotThisProgram::Replaced);
    }
    hold_install_at(&installed, current)
}

/// The shared hold of a process of `installed` at version `current` that
/// only holds (the gate taken, the presence file held shared, the gate
/// released): no hold where none is possible, and `Err` when, once held,
/// the installed payload's `version` is not `current` or cannot be read.
pub fn hold_install_at(
    installed: &Installed,
    current: &str,
) -> Result<InstallHold, NotThisProgram> {
    let hold = match hold::hold_shared(installed, None) {
        Ok(hold) => hold,
        Err(_) => return Ok(InstallHold::none()),
    };
    match replaced(installed, current) {
        Ok(false) => Ok(hold),
        Ok(true) => Err(NotThisProgram::Replaced),
        Err(sentence) => Err(NotThisProgram::Unreadable(sentence)),
    }
}

/// Why a held process does not go on.
#[derive(Debug, PartialEq, Eq)]
pub enum NotThisProgram {
    /// The installed payload is another version's ([`CHANGED_WHILE_STARTING`]).
    Replaced,
    /// The installed payload's `version` file cannot be read, with the
    /// sentence saying so.
    Unreadable(String),
}

/// The start-time work of the updater, called by `mcbench-lite app` after
/// the crash hook is installed and before the core starts or any window
/// opens. `logs` is the data folder's `logs/`; `lock` takes the core's
/// one-instance lock on the data folder (`mcbench_setup::InstanceLock::take`)
/// and returns its guard.
///
/// In a build without an updater, or for a package manager's route, it
/// does nothing and returns an [`Updater`] that checks nothing. Otherwise
/// it reads the install route (an error is the caller's fatal sentence:
/// the install is broken), records what this process runs, installs the
/// updater's panic hook, and, only while `lock` holds the instance lock,
/// takes the install's hold ([`start_with`]): when no other process runs
/// from the install and a verified download waits, it is swapped in, the
/// locks are released and the new program is executed with the same
/// arguments in this process's place ([`restart`]), so this returns only
/// when no swap was made. When `lock` fails, the app only holds the install
/// shared. When the installed program changed while this process waited
/// for its hold, the installed program is executed instead, once. When no
/// hold is possible, the app starts as it is, [`state`] says why, and no
/// check runs. An error is returned only for a broken install, a swap that
/// could not be reversed, or a program that cannot be started and cannot
/// be reversed; every other failure goes to `update.log` and the app starts
/// as it is, holding its install shared for its whole life (the returned
/// [`Updater`] keeps the hold).
pub fn at_start<G>(
    logs: &Path,
    lock: impl FnOnce() -> Result<G, String>,
    restarted: bool,
) -> Result<Updater, String> {
    let nothing = || Updater {
        active: None,
        _hold: InstallHold::none(),
    };
    let Some(feed) = Feed::compiled_in() else {
        set(UpdateState::Off);
        return Ok(nothing());
    };
    let (installed, moved) = match route_at(&current_exe()?, appimage())? {
        Route::PackageManager(line) => {
            set(line);
            return Ok(nothing());
        }
        Route::Updates { installed, moved } => (installed, moved),
    };
    install_panic_hook();
    set(UpdateState::Checking);
    let mut held = read_held(&installed, VERSION);
    let (start, hold) = if moved {
        (Start::Changed, InstallHold::none())
    } else {
        match lock() {
            Ok(guard) => {
                trim_log(logs);
                let started = start_with(&feed, &installed, VERSION, logs)?;
                drop(guard);
                started
            }
            Err(_) => match hold::hold_shared(&installed, Some(logs)) {
                Ok(hold) if replaced(&installed, VERSION)? => {
                    drop(hold);
                    (Start::Changed, InstallHold::none())
                }
                Ok(hold) => (Start::AsIs, hold),
                Err(why) => (Start::Unable(why), InstallHold::none()),
            },
        }
    };
    let hold = match start {
        Start::AsIs => hold,
        Start::Unable(why) => {
            set(UpdateState::Unable(why.sentence()));
            return Ok(nothing());
        }
        Start::Changed => {
            drop(hold);
            if restarted {
                // On Windows the one way to get here twice is a swap cut
                // off with no download left to finish it.
                let then = if installed.target == Target::WindowsX86_64 {
                    "install mcbench-lite again"
                } else {
                    "start it again"
                };
                return Err(format!(
                    "mcbench-lite at {} changed again while it was starting; {then}.",
                    installed.program.display()
                ));
            }
            let Err(e) = restart(&installed, true);
            return Err(e);
        }
        Start::Held(h) => {
            held = Some(h);
            hold
        }
        Start::Restart { version } => {
            // Both files stay exclusive until exec replaces this program
            // (they close on exec) or this process exits on Windows; they
            // cover the reversal below.
            let Err(e) = restart(&installed, false);
            let h = restart_failed(&installed, version, &e, logs)?;
            held = Some(h);
            let (presence, _gate) = hold.into_files();
            hold::held(hold::share(&installed, presence).map_err(|why| why.sentence())?)
        }
    };
    if let Some(h) = &held {
        set(UpdateState::Failed(h.sentence.clone()));
    }
    Ok(Updater {
        active: Some(Active {
            feed,
            installed,
            logs: logs.to_path_buf(),
            held,
        }),
        _hold: hold,
    })
}

/// After a swap whose new program could not be started (`error`): puts the
/// previous version back ([`crate::reverse`]), logs it, and keeps `version`
/// held in `.<name>.update.held`, so neither this process nor a later start
/// takes it again until the feed offers a newer one. A reversal that fails
/// is the error, for the caller's fatal screen.
pub fn restart_failed(
    installed: &Installed,
    version: String,
    error: &str,
    logs: &Path,
) -> Result<Held, String> {
    match crate::reverse(installed) {
        Err(e) => {
            let sentence = format!("{error}, and {}.", lower_first(&e));
            log(logs, &sentence);
            return Err(sentence);
        }
        Ok(Some(note)) => log(logs, &note),
        Ok(None) => {}
    }
    let sentence = format!(
        "{error}; the previous version was put back, and version {version} is not taken again until a newer version is offered."
    );
    log(logs, &sentence);
    let held = Held { version, sentence };
    write_held(installed, &held, logs);
    Ok(held)
}

/// `text` with its first letter in lower case, to join it into a sentence.
fn lower_first(text: &str) -> String {
    let mut chars = text.chars();
    chars
        .next()
        .map(|c| c.to_lowercase().chain(chars).collect())
        .unwrap_or_default()
}

/// [`at_start`]'s work while it holds the instance lock, for the app at
/// version `current` installed as `installed`: takes the install's gate
/// (waiting for another starting app, one line in `update.log` after five
/// seconds), then tries the presence file exclusively. When the installed
/// payload's `version` is not `current` it answers [`Start::Changed`]. When
/// no other process holds the presence file, a waiting
/// download is swapped in ([`Start::Restart`], with both files kept
/// exclusive until the new program runs). Otherwise the presence file is
/// held shared before the gate is released, so no swap can come between
/// this decision and the hold. When another process holds the install a
/// waiting download stays, and one line goes to `update.log`. Blocking
/// (file system).
pub fn start_with(
    feed: &Feed,
    installed: &Installed,
    current: &str,
    logs: &Path,
) -> Result<(Start, InstallHold), String> {
    let unable = |why: NoHold| Ok((Start::Unable(why), InstallHold::none()));
    let gate = match hold::take_gate(installed, Some(logs)) {
        Ok(gate) => gate,
        Err(why) => return unable(why),
    };
    let presence = match hold::take_presence(installed) {
        Ok(presence) => presence,
        Err(why) => return unable(why),
    };
    let changed = replaced(installed, current)?;
    let presence = match presence {
        Presence::Shared(presence) => {
            drop(gate);
            if changed {
                return Ok((Start::Changed, hold::held(presence)));
            }
            if waiting_json(&installed.path).exists() {
                log(
                    logs,
                    &format!(
                        "Another process runs from {}, so the waiting update was not swapped in; it is tried again at the next start.",
                        installed.path.display()
                    ),
                );
            }
            return Ok((Start::AsIs, hold::held(presence)));
        }
        Presence::Alone(presence) => presence,
    };
    let start = if !changed {
        swap_waiting(feed, installed, current, logs)?
    } else if waiting_json(&installed.path).exists() {
        // The payload is not this program's and a download waits: a swap
        // cut off before it renamed the program (on Windows the program is
        // the last entry). Running it finishes that swap; the new program
        // then runs as after any swap.
        match swap_waiting(feed, installed, current, logs)? {
            Start::Restart { version } => Start::Restart { version },
            _ => Start::Changed,
        }
    } else {
        Start::Changed
    };
    if let Start::Restart { .. } = start {
        return Ok((start, hold::swapping(presence, gate)));
    }
    let presence = match hold::share(installed, presence) {
        Ok(presence) => presence,
        Err(why) => return unable(why),
    };
    drop(gate);
    Ok((start, hold::held(presence)))
}

/// What [`start_with`] does while it holds the install exclusively, for
/// the app at version `current` installed as `installed`: when a download
/// waits beside the install, checks it again without the network
/// ([`verify_waiting`]) and swaps it in ([`install`]). A waiting download that fails a check is deleted; a swap
/// that fails leaves the download waiting for the next start. Both write
/// their sentence to `update.log` in `logs`. A swap whose reversal failed
/// too ([`Failure::Broken`]) is the error: the install is not whole, and
/// the caller shows the sentence as fatal. Blocking (file system).
fn swap_waiting(
    feed: &Feed,
    installed: &Installed,
    current: &str,
    logs: &Path,
) -> Result<Start, String> {
    let json = waiting_json(&installed.path);
    let file = update_file(&installed.path);
    if let Some(held) = read_held(installed, current)
        && waiting_version(installed).as_deref() == Some(held.version.as_str())
    {
        // A version whose program could not start: not swapped in again.
        for path in [&file, &json] {
            if let Err(e) = remove(path) {
                log(logs, &e);
            }
        }
        return Ok(Start::Held(held));
    }
    let latest = match std::fs::read(&json) {
        Ok(latest) => latest,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // A download stopped before it was verified.
            if let Err(e) = remove(&file) {
                log(logs, &e);
            }
            return Ok(Start::AsIs);
        }
        Err(e) => {
            let sentence = format!("{} cannot be read: {e}", json.display());
            discard(logs, &json, &file, &sentence);
            return Ok(Start::AsIs);
        }
    };
    let downloaded = match verify_waiting(
        &feed.url,
        &latest,
        &file,
        current,
        installed.target,
        &feed.key,
    ) {
        Ok(downloaded) => downloaded,
        Err(e) => {
            discard(logs, &json, &file, &e);
            return Ok(Start::AsIs);
        }
    };
    match install(&downloaded, installed) {
        Ok(()) => {
            let downloaded_version = downloaded.version().to_string();
            drop(downloaded);
            if let Err(e) = remove(&json) {
                log(logs, &e);
            }
            Ok(Start::Restart {
                version: downloaded_version,
            })
        }
        Err(Failure::Unchanged(e)) => {
            let version = downloaded.version().to_string();
            downloaded.keep();
            let sentence = format!("{e} The update is tried again at the next start.");
            log(logs, &sentence);
            Ok(Start::Held(Held { version, sentence }))
        }
        Err(Failure::Broken(e)) => {
            downloaded.keep();
            log(logs, &e);
            Err(e)
        }
    }
}

/// Deletes a waiting download that failed a check, and logs why.
fn discard(logs: &Path, json: &Path, file: &Path, sentence: &str) {
    log(
        logs,
        &format!("The waiting update was deleted and the app started as it is: {sentence}"),
    );
    for path in [file, json] {
        if let Err(e) = remove(path) {
            log(logs, &e);
        }
    }
}

/// The version of the download waiting beside `installed`, when both of
/// its files are there.
fn waiting_version(installed: &Installed) -> Option<String> {
    if !update_file(&installed.path).is_file() {
        return None;
    }
    let bytes = std::fs::read(waiting_json(&installed.path)).ok()?;
    serde_json::from_slice::<Latest>(&bytes)
        .ok()
        .map(|l| l.version)
}

/// One check of `feed` for the app at version `current` installed as
/// `installed`: up to date, a newer version already waiting, or a newer
/// version downloaded, verified and left waiting beside the install with
/// the `latest.json` it was checked against (it is swapped in at the next
/// start). A waiting download of the same version is not fetched again.
/// The state while it downloads is [`UpdateState::Downloading`]; the
/// returned state is the outcome. A release of the `held` version is not
/// fetched: the state is that version's failure.
pub async fn check_once(
    client: &reqwest::Client,
    feed: &Feed,
    installed: &Installed,
    current: &str,
    held: Option<&Held>,
    logs: &Path,
) -> UpdateState {
    let failed = |sentence: String| {
        log(logs, &sentence);
        UpdateState::Failed(sentence)
    };
    let cancel = CancellationToken::new();
    let (url, latest) = match fetch_latest(client, &feed.url, &cancel).await {
        Ok(fetched) => fetched,
        Err(e) => return failed(e),
    };
    let refused_file = refused_file(installed);
    let digest = hex(&Sha256::digest(&latest));
    match read_refused(installed) {
        // The same latest.json as the refused one: nothing more, no line.
        Some((same, sentence)) if same == digest => return UpdateState::Refused(sentence),
        Some(_) => {
            if let Err(e) = remove(&refused_file) {
                return failed(e);
            }
        }
        None => {}
    }
    let refuse = |sentence: String| {
        log(logs, &sentence);
        if let Err(e) = std::fs::write(&refused_file, format!("{digest}\n{sentence}\n")) {
            log(
                logs,
                &format!("{} cannot be written: {e}", refused_file.display()),
            );
        }
        UpdateState::Refused(sentence)
    };
    let release = match release_of(&url, &latest, current, installed.target, &feed.key) {
        Err(e) => return refuse(e),
        Ok(None) => return UpdateState::Current,
        Ok(Some(release)) => release,
    };
    if let Some(held) = held.filter(|h| h.version == release.version) {
        return UpdateState::Failed(held.sentence.clone());
    }
    // Another version than the held one is offered: the held one no longer
    // matters.
    if held.is_some()
        && let Err(e) = remove(&held_file(installed))
    {
        return failed(e);
    }
    if waiting_version(installed).as_deref() == Some(release.version.as_str()) {
        return UpdateState::Ready(release.version);
    }
    let json = waiting_json(&installed.path);
    if let Err(e) = remove(&json) {
        return failed(e);
    }
    set(UpdateState::Downloading(release.version.clone()));
    let downloaded = match download_checked(client, &release, &installed.path, &cancel).await {
        Ok(downloaded) => downloaded,
        Err(Problem::Refused(e)) => return refuse(e),
        Err(Problem::Failed(e)) => return failed(e),
    };
    let part = beside(&installed.path, ".update.json.part");
    if let Err(e) = std::fs::write(&part, &latest).and_then(|()| std::fs::rename(&part, &json)) {
        let _ = std::fs::remove_file(&part);
        return failed(format!("{} cannot be written: {e}", json.display()));
    }
    downloaded.keep();
    UpdateState::Ready(release.version)
}

/// Where a refused release is remembered: `.<name>.update.refused` beside
/// the payload, the sha256 of the `latest.json` bytes that led to the
/// refusal on its first line and the refusal's sentence on its second.
fn refused_file(installed: &Installed) -> PathBuf {
    beside(&installed.path, ".update.refused")
}

fn read_refused(installed: &Installed) -> Option<(String, String)> {
    let text = std::fs::read_to_string(refused_file(installed)).ok()?;
    let (digest, sentence) = text.split_once('\n')?;
    Some((digest.to_string(), sentence.trim_end().to_string()))
}

/// Runs `work` on the updater's thread, marked as the updater's: a panic
/// in it is written to `update.log` by the hook [`install_panic_hook`]
/// installed and is caught here as a [`UpdateState::Failed`]; the app goes
/// on.
fn guarded(logs: &Path, work: impl FnOnce() -> UpdateState) -> UpdateState {
    mark(Some(logs));
    let result = std::panic::catch_unwind(AssertUnwindSafe(work));
    mark(None);
    result.unwrap_or_else(|_| {
        UpdateState::Failed(format!(
            "The updater stopped on an internal error ({LOG_FILE} in the logs folder has it); it checks again in 24 hours."
        ))
    })
}

impl Updater {
    /// Starts the updater's thread ([`THREAD_NAME`]), called by
    /// `mcbench-lite app` once the core started: it first removes the last
    /// swap's leftovers ([`remove_leftover`]: the previous version is no
    /// longer needed once this core runs), then runs [`check_once`] now and
    /// 24 hours of wall-clock time after each, each on a current-thread
    /// tokio runtime of its own with `client` (the app passes a clone of
    /// the core's). A failure is written to `update.log` and shown by
    /// [`state`]; a panic on any of its threads is written there too and
    /// the thread goes on. Does nothing for an [`Updater`] that checks
    /// nothing. The [`Updater`] itself holds the install and is kept by the
    /// caller for the life of the process.
    pub fn check_in_background(&self, client: reqwest::Client) {
        let Some(active) = self.active.clone() else {
            return;
        };
        std::thread::Builder::new()
            .name(THREAD_NAME.to_string())
            .spawn(move || {
                // The core has started: the previous version is no longer
                // needed to go back to.
                let leftover = guarded(&active.logs, || {
                    if active.installed.target == Target::WindowsX86_64 {
                        crate::apps_entry::record(VERSION, &active.installed.path, &active.logs);
                    }
                    match remove_leftover(&active.installed) {
                        Ok(()) => UpdateState::Checking,
                        Err(e) => UpdateState::Failed(e),
                    }
                });
                if let UpdateState::Failed(sentence) = &leftover {
                    log(&active.logs, sentence);
                }
                loop {
                    // check_once writes its own failures and first refusals
                    // to update.log; a panic is written by the hook.
                    let state = guarded(&active.logs, || {
                        runtime(&active.logs)
                            .map_err(|e| format!("The updater's runtime cannot start: {e}"))
                            .map_or_else(
                                |e| {
                                    log(&active.logs, &e);
                                    UpdateState::Failed(e)
                                },
                                |runtime| {
                                    runtime.block_on(check_once(
                                        &client,
                                        &active.feed,
                                        &active.installed,
                                        VERSION,
                                        active.held.as_ref(),
                                        &active.logs,
                                    ))
                                },
                            )
                    });
                    set(state);
                    // A day on the wall clock, in short sleeps: a computer
                    // that slept does not wait a day of awake time.
                    let next = SystemTime::now() + DAY;
                    while SystemTime::now() < next {
                        std::thread::sleep(Duration::from_secs(60));
                    }
                }
            })
            .expect("the updater's thread starts");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_panic_on_the_updaters_thread_is_logged_and_caught() {
        install_panic_hook();
        let dir = tempfile::tempdir().unwrap();
        let logs = dir.path().to_path_buf();
        let state = std::thread::spawn(move || guarded(&logs, || panic!("a test panic")))
            .join()
            .unwrap();
        assert_eq!(
            state,
            UpdateState::Failed(
                "The updater stopped on an internal error (update.log in the logs folder has it); it checks again in 24 hours."
                    .into()
            )
        );
        let text = std::fs::read_to_string(dir.path().join(LOG_FILE)).unwrap();
        let (seconds, sentence) = text.trim_end().split_once(' ').unwrap();
        assert!(seconds.parse::<u64>().unwrap() > 0);
        assert!(
            sentence.starts_with("The updater panicked at ")
                && sentence.ends_with(": a test panic"),
            "{sentence}"
        );
        assert_eq!(text.lines().count(), 1);
    }

    /// A panic on a blocking-pool thread of the check's runtime is written
    /// to `update.log` and does not end the process (the test goes on).
    #[test]
    fn a_panic_in_the_runtimes_blocking_pool_is_logged() {
        install_panic_hook();
        let dir = tempfile::tempdir().unwrap();
        let logs = dir.path().to_path_buf();
        let joined = std::thread::spawn(move || {
            let rt = runtime(&logs).unwrap();
            rt.block_on(async {
                tokio::task::spawn_blocking(|| {
                    assert_eq!(
                        std::thread::current().name(),
                        Some(THREAD_NAME),
                        "the blocking pool's threads are named"
                    );
                    panic!("a blocking panic")
                })
                .await
            })
        })
        .join()
        .unwrap();
        assert!(joined.unwrap_err().is_panic());
        let text = std::fs::read_to_string(dir.path().join(LOG_FILE)).unwrap();
        assert!(
            text.trim_end().ends_with(": a blocking panic") && text.lines().count() == 1,
            "{text}"
        );
    }

    #[test]
    fn the_log_is_cut_to_its_last_256_kib_from_a_whole_line() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(LOG_FILE);
        let line = format!("{} {}\n", 1_000_000_000u64, "x".repeat(88));
        std::fs::write(&path, line.repeat(4000)).unwrap();
        trim_log(dir.path());
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.len() as u64 <= LOG_KEEP);
        assert!(text.len() as u64 > LOG_KEEP - line.len() as u64);
        assert!(text.starts_with("1000000000 x"));
        let small = line.repeat(3);
        std::fs::write(&path, &small).unwrap();
        trim_log(dir.path());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), small);
    }

    #[test]
    fn a_build_without_a_feed_does_nothing_at_start() {
        let updater = at_start(
            Path::new("/nonexistent"),
            || -> Result<(), String> { panic!("no lock is taken without an updater") },
            false,
        )
        .unwrap();
        assert!(updater.active.is_none());
        assert!(!updater._hold.holds());
        assert_eq!(state(), UpdateState::Off);
        // A headless run of such a build holds nothing and makes no file.
        assert!(!hold_install().holds());
        // Nor does its error screen, whose restart runs the program itself.
        assert!(!hold_install_quietly().holds());
        assert_eq!(installed_program(), current_exe());
    }
}
