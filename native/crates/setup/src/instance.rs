//! One process at a time per data folder (`server/pid-lock.js` pidLock): a
//! file holds the pid of the process that holds the folder. A lock whose
//! process is gone (killed, crashed) is taken over; one whose process lives
//! is refused, naming it.
//!
//! The file is today's localhost app's, `server.pid` (`server/index.js`),
//! taken by the same steps (the pid written whole to `<file>.<pid>` and hard
//! linked to the file; a dead holder's file removed under
//! `<file>.takeover`), so the native app and today's server refuse each
//! other a data folder and each takes over a lock the other left when it
//! died (`tests/instance.rs`). Both write one SQLite database and prune the
//! folder's worlds, so two different files would let both run on it.
//!
//! One rule is the native app's own: a file that holds this process's pid
//! and that this program did not take is nobody's, and is taken over as a
//! dead holder's is. Such a file is left by the program this process ran
//! before it replaced itself (the updater's restart on Unix keeps the pid,
//! and a lock whose removal failed would otherwise refuse the new program
//! its own data folder), or by a dead process whose pid this one was given.
//! Today's server refuses there, naming itself. What this program does hold
//! is kept in a set in this module, so a second take in one process is
//! refused as before.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

/// The lock files this program holds, each by its folder's canonical path
/// and its name. Making a lock and noting it here, and removing a lock and
/// its note, each happen under this mutex.
static HELD: Mutex<BTreeSet<PathBuf>> = Mutex::new(BTreeSet::new());

/// `file` as [`HELD`] names it.
fn held_name(file: &Path) -> Result<PathBuf, String> {
    let folder = match file.parent() {
        Some(folder) if !folder.as_os_str().is_empty() => folder,
        _ => Path::new("."),
    };
    let folder = folder
        .canonicalize()
        .map_err(|e| format!("{}: {e}", folder.display()))?;
    Ok(folder.join(file.file_name().expect("a lock file has a name")))
}

/// The lock's file in a data folder: today's server's (`server/index.js`).
pub const LOCK_FILE: &str = "server.pid";

/// The one-instance lock on a data folder: held until dropped.
#[derive(Debug)]
pub struct InstanceLock {
    file: PathBuf,
    name: PathBuf,
}

impl InstanceLock {
    /// Takes the lock on `folder` (its file [`LOCK_FILE`]), or refuses
    /// naming the live process that holds it.
    pub fn take(folder: &Path) -> Result<InstanceLock, String> {
        std::fs::create_dir_all(folder).map_err(|e| format!("{}: {e}", folder.display()))?;
        let file = folder.join(LOCK_FILE);
        let name = take(&file, 0)?;
        Ok(InstanceLock { file, name })
    }
}

impl Drop for InstanceLock {
    fn drop(&mut self) {
        // A removal that fails leaves a file holding this process's pid
        // that this program no longer holds: the next take, by this program
        // or the one that replaces it, takes it over.
        let _ = release(&self.file, &self.name);
    }
}

/// Lets go of `file`, which this program holds as `name`.
fn release(file: &Path, name: &Path) -> std::io::Result<()> {
    let mut held = HELD.lock().expect("the held locks");
    held.remove(name);
    std::fs::remove_file(file)
}

/// Whether process `pid` (as the file holds it) lives.
pub fn alive(pid: &str) -> bool {
    let Ok(pid) = pid.parse::<u32>() else {
        return false;
    };
    os::alive(pid)
}

/// Makes `file` holding this process's pid and notes it as held by this
/// program, or answers false if it exists.
fn create_held(file: &Path, name: &Path) -> Result<bool, String> {
    let mut held = HELD.lock().expect("the held locks");
    let made = create(file)?;
    if made {
        held.insert(name.to_path_buf());
    }
    Ok(made)
}

/// Whether the holder `pid` of the lock `name` lives: another process that
/// runs, or this program (the module's doc).
fn holds(pid: &str, name: &Path) -> bool {
    if pid == std::process::id().to_string() {
        HELD.lock().expect("the held locks").contains(name)
    } else {
        alive(pid)
    }
}

/// Makes `file` holding this process's pid, or answers false if it exists.
/// The file is a hard link to one written whole first, so no reader ever
/// sees it empty.
fn create(file: &Path) -> Result<bool, String> {
    let own = PathBuf::from(format!("{}.{}", file.display(), std::process::id()));
    std::fs::write(&own, std::process::id().to_string())
        .map_err(|e| format!("{}: {e}", own.display()))?;
    let linked = std::fs::hard_link(&own, file);
    let removed = std::fs::remove_file(&own).map_err(|e| format!("{}: {e}", own.display()));
    match linked {
        Ok(()) => removed.map(|()| true),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => removed.map(|()| false),
        Err(e) => Err(format!("{}: {e}", file.display())),
    }
}

/// The pid `file` holds, or `None` if it is gone (its holder released it).
fn holder(file: &Path) -> Result<Option<String>, String> {
    match std::fs::read_to_string(file) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{}: {e}", file.display())),
    }
}

/// Takes `file`, or fails naming the live process that holds it. `waits` is
/// how many times a live holder is waited 10 ms for before that.
pub fn pid_lock(file: &Path, waits: u32) -> Result<(), String> {
    take(file, waits).map(|_| ())
}

/// [`pid_lock`], answering the name the lock is held under ([`HELD`]).
fn take(file: &Path, mut waits: u32) -> Result<PathBuf, String> {
    let name = held_name(file)?;
    loop {
        if create_held(file, &name)? {
            return Ok(name);
        }
        let Some(pid) = holder(file)? else { continue };
        if holds(&pid, &name) {
            if waits > 0 {
                waits -= 1;
                std::thread::sleep(Duration::from_millis(10));
                continue;
            }
            return Err(format!(
                "{} is held by process {pid}, which is still running",
                file.display()
            ));
        }
        // A dead holder's lock is removed under a lock of its own: two
        // processes that both found it dead would otherwise both remove it,
        // the second one the lock the first had just made. That lock is a
        // pid lock too: one left by a process that died holding it is taken
        // over the same way, and a live process holds it only for this
        // removal, so it is waited for.
        let takeover = PathBuf::from(format!("{}.takeover", file.display()));
        let taken = take(&takeover, 100)?;
        let removed = match holder(file) {
            Ok(Some(now)) if now == pid => {
                std::fs::remove_file(file).map_err(|e| format!("{}: {e}", file.display()))
            }
            Ok(_) => Ok(()),
            Err(e) => Err(e),
        };
        release(&takeover, &taken).map_err(|e| format!("{}: {e}", takeover.display()))?;
        removed?;
    }
}

#[cfg(unix)]
mod os {
    pub fn alive(pid: u32) -> bool {
        let Ok(pid) = libc::pid_t::try_from(pid) else {
            return false;
        };
        // SAFETY: kill with signal 0 sends nothing; it only checks that the
        // process exists and may be signalled. It reads no memory of ours.
        if unsafe { libc::kill(pid, 0) } == 0 {
            return true;
        }
        std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    }
}

#[cfg(windows)]
mod os {
    use windows_sys::Win32::Foundation::{
        CloseHandle, ERROR_ACCESS_DENIED, GetLastError, STILL_ACTIVE,
    };
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    pub fn alive(pid: u32) -> bool {
        // SAFETY: OpenProcess takes plain values and returns a handle or
        // null; GetLastError reads this thread's last error.
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if handle.is_null() {
            // SAFETY: as above.
            return unsafe { GetLastError() } == ERROR_ACCESS_DENIED;
        }
        let mut code = 0u32;
        // SAFETY: the handle is the one OpenProcess just returned and `code`
        // is a valid place for the exit code; the handle is closed once.
        let alive =
            unsafe { GetExitCodeProcess(handle, &mut code) } != 0 && code == STILL_ACTIVE as u32;
        // SAFETY: as above.
        unsafe { CloseHandle(handle) };
        alive
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scratch folder removed when dropped, also when the test fails.
    struct Scratch(PathBuf);

    impl std::ops::Deref for Scratch {
        type Target = Path;
        fn deref(&self) -> &Path {
            &self.0
        }
    }

    impl AsRef<Path> for Scratch {
        fn as_ref(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn scratch(name: &str) -> Scratch {
        let dir =
            std::env::temp_dir().join(format!("mcbench-setup-lock-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut list: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        list.sort();
        list
    }

    const DEAD: &str = "2147483646";

    #[test]
    fn a_pid_lock_is_held_whole_refused_while_its_holder_lives_taken_over_once_it_is_dead_and_never_during_a_takeover()
     {
        let dir = scratch("held");
        let lock = dir.join("run.lock");
        pid_lock(&lock, 0).unwrap();
        assert_eq!(
            std::fs::read_to_string(&lock).unwrap(),
            std::process::id().to_string()
        );
        assert_eq!(
            pid_lock(&lock, 0).unwrap_err(),
            format!(
                "{} is held by process {}, which is still running",
                lock.display(),
                std::process::id()
            )
        );
        std::fs::remove_file(&lock).unwrap();
        assert!(names(&dir).is_empty());
        // A pid no process has.
        std::fs::write(&lock, DEAD).unwrap();
        pid_lock(&lock, 0).unwrap();
        std::fs::remove_file(&lock).unwrap();
        assert!(names(&dir).is_empty());
        // A dead holder's lock is being taken over and the takeover does not
        // finish (here it is this program's own, held): this take waits,
        // then leaves both alone.
        std::fs::write(&lock, DEAD).unwrap();
        let takeover = dir.join("run.lock.takeover");
        pid_lock(&takeover, 0).unwrap();
        assert_eq!(
            pid_lock(&lock, 0).unwrap_err(),
            format!(
                "{} is held by process {}, which is still running",
                takeover.display(),
                std::process::id()
            )
        );
        assert_eq!(std::fs::read_to_string(&lock).unwrap(), DEAD);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_lock_holding_this_process_pid_that_this_program_did_not_take_is_taken_over() {
        let dir = scratch("own-pid");
        let own = std::process::id().to_string();
        // What the program this process ran before left, or a dead process
        // that had this pid.
        let lock = dir.join("run.lock");
        std::fs::write(&lock, &own).unwrap();
        pid_lock(&lock, 0).unwrap();
        assert_eq!(names(&dir), ["run.lock"]);
        assert_eq!(std::fs::read_to_string(&lock).unwrap(), own);
        // Now this program holds it.
        assert_eq!(
            pid_lock(&lock, 0).unwrap_err(),
            format!(
                "{} is held by process {own}, which is still running",
                lock.display()
            )
        );
        std::fs::remove_dir_all(&dir).unwrap();
        // The data folder's lock: a left one is taken, a held one refuses,
        // and a dropped one is free again.
        let dir = scratch("own-pid-folder");
        std::fs::write(dir.join(LOCK_FILE), &own).unwrap();
        let held = InstanceLock::take(&dir).unwrap();
        assert_eq!(
            InstanceLock::take(&dir).unwrap_err(),
            format!(
                "{} is held by process {own}, which is still running",
                dir.join(LOCK_FILE).display()
            )
        );
        drop(held);
        assert!(names(&dir).is_empty());
        drop(InstanceLock::take(&dir).unwrap());
        assert!(names(&dir).is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_takeover_left_by_a_process_that_died_in_it_is_taken_over_as_the_lock_is() {
        let dir = scratch("takeover");
        let lock = dir.join("run.lock");
        std::fs::write(&lock, DEAD).unwrap();
        std::fs::write(dir.join("run.lock.takeover"), "2147483645").unwrap();
        pid_lock(&lock, 0).unwrap();
        std::fs::remove_file(&lock).unwrap();
        assert!(names(&dir).is_empty());
        // And so is the takeover of that takeover.
        std::fs::write(&lock, DEAD).unwrap();
        std::fs::write(dir.join("run.lock.takeover"), "2147483645").unwrap();
        std::fs::write(dir.join("run.lock.takeover.takeover"), "2147483644").unwrap();
        pid_lock(&lock, 0).unwrap();
        assert_eq!(names(&dir), ["run.lock"]);
        assert_eq!(
            std::fs::read_to_string(&lock).unwrap(),
            std::process::id().to_string()
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_racer_waits_for_a_live_takeover_to_end_instead_of_failing() {
        let dir = scratch("racer");
        let lock = dir.join("run.lock");
        let takeover = dir.join("run.lock.takeover");
        std::fs::write(&lock, DEAD).unwrap();
        // Another process that holds the takeover for 300 ms: a child that
        // sleeps (far longer than the test needs; it is killed at the end),
        // its pid written into the takeover, which a thread removes.
        #[cfg(unix)]
        let mut other = std::process::Command::new("sleep")
            .arg("600")
            .spawn()
            .unwrap();
        #[cfg(windows)]
        let mut other = std::process::Command::new("cmd")
            .args(["/C", "ping -n 61 127.0.0.1 > NUL"])
            .spawn()
            .unwrap();
        std::fs::write(&takeover, other.id().to_string()).unwrap();
        let remover = {
            let takeover = takeover.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(300));
                std::fs::remove_file(takeover).unwrap();
            })
        };
        pid_lock(&lock, 0).unwrap();
        remover.join().unwrap();
        assert_eq!(
            std::fs::read_to_string(&lock).unwrap(),
            std::process::id().to_string()
        );
        std::fs::remove_file(&lock).unwrap();
        assert!(names(&dir).is_empty());
        other.kill().unwrap();
        other.wait().unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_second_app_on_the_same_data_folder_is_refused_until_the_first_lets_go() {
        let dir = scratch("instance");
        let first = InstanceLock::take(&dir).unwrap();
        assert!(
            InstanceLock::take(&dir)
                .unwrap_err()
                .ends_with("which is still running")
        );
        drop(first);
        let _second = InstanceLock::take(&dir).unwrap();
        assert_eq!(names(&dir), [LOCK_FILE]);
    }
}
