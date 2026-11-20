//! The swap: a verified download takes the installed payload's place, and a
//! failure at any step leaves the old payload in place.
//!
//! | Target | Swap |
//! |---|---|
//! | macOS bundle, Linux `script` folder | unpack the archive beside the payload (`.<name>.update.d`), check it holds one root folder (`mcbench-lite.app`, `mcbench-lite`) with the program in it, copy the installed `install-source` into it, sync every unpacked file and folder to disk, exchange the new folder and the installed one in one step (`renameat2` with `RENAME_EXCHANGE` on Linux, `renamex_np` with `RENAME_SWAP` on macOS), rename the old one, now in the unpack place, to `<name>.old`, and sync the parent folder |
//! | Linux AppImage | give the download the AppImage's permissions, sync it, exchange it with the AppImage, rename the old file, now at the download's place, to `<name>.old`, and sync the folder |
//! | Windows | unpack the archive beside the payload folder, check it holds the program and neither `install-source` nor `uninstall.exe`, sync the unpacked files, then for each top-level entry of the archive, in name order with the program `mcbench-lite.exe` last: rename the installed entry of that name (when there is one) to `<name>.old` inside the payload folder and at once rename the new one in. Windows lets a running executable be renamed, not overwritten or deleted, and refuses to rename a folder that holds one, so the folder itself is never renamed and the swap is not one step: a power loss between an entry's two renames leaves `<name>.old` and no `<name>`, which running the installer again repairs |
//!
//! On a file system that refuses the exchange (`EINVAL`, `ENOSYS`,
//! `ENOTSUP`) a folder or the AppImage is swapped with two renames
//! (installed to `<name>.old`, new in), which leave nothing at the
//! install's path between them. Every step of a swap is recorded; when one
//! fails, every recorded step is reversed in reverse order, so the old
//! payload is whole again and the new files are back where they were
//! unpacked. When a reversal itself fails, the swap answers
//! [`Failure::Broken`] and nothing more is removed. The `.old` entries are
//! removed after the new program's core has started ([`remove_leftover`]),
//! and only while the entry they belong to is in place.

use std::convert::Infallible;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::{Downloaded, Installed, Target};

/// The file system steps of a swap, so tests can fail any one of them.
trait Ops {
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()>;
    /// Exchanges `a` and `b` in one step; an error of kind `Unsupported`
    /// when the file system has no such step.
    fn exchange(&self, a: &Path, b: &Path) -> io::Result<()>;
    fn remove_all(&self, path: &Path) -> io::Result<()>;
    fn copy(&self, from: &Path, to: &Path) -> io::Result<()>;
    fn copy_permissions(&self, from: &Path, to: &Path) -> io::Result<()>;
    fn unpack(&self, archive: &Path, into: &Path) -> io::Result<()>;
    /// Syncs `path` and, for a folder, everything in it to disk.
    fn sync_tree(&self, path: &Path) -> io::Result<()>;
    /// Syncs the folder `dir`'s entries to disk (nothing on Windows, which
    /// cannot open a folder for that).
    fn sync_dir(&self, dir: &Path) -> io::Result<()>;
}

struct Real;

/// An exchange error that means the file system has no exchange: Linux's
/// `EINVAL` (no `RENAME_EXCHANGE` on this file system) and `ENOSYS` (no
/// `renameat2`), macOS's `ENOTSUP`, and `EOPNOTSUPP`, which some file
/// systems answer and which differs from `ENOTSUP` on macOS. All four on
/// purpose: such a file system gets the two renames (the crate's Decided
/// table says why).
#[cfg(unix)]
fn exchange_error() -> io::Error {
    let e = io::Error::last_os_error();
    let code = e.raw_os_error();
    let refused = [libc::EINVAL, libc::ENOSYS, libc::ENOTSUP, libc::EOPNOTSUPP];
    if code.is_some_and(|c| refused.contains(&c)) {
        io::Error::new(io::ErrorKind::Unsupported, e)
    } else {
        e
    }
}

#[cfg(unix)]
fn c_path(path: &Path) -> io::Result<std::ffi::CString> {
    use std::os::unix::ffi::OsStrExt;
    std::ffi::CString::new(path.as_os_str().as_bytes())
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))
}

impl Ops for Real {
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        fs::rename(from, to)
    }

    #[cfg(target_os = "linux")]
    fn exchange(&self, a: &Path, b: &Path) -> io::Result<()> {
        let (a, b) = (c_path(a)?, c_path(b)?);
        // SAFETY: both paths are NUL-terminated strings that outlive the
        // call; AT_FDCWD makes them relative to the working folder.
        let done = unsafe {
            libc::renameat2(
                libc::AT_FDCWD,
                a.as_ptr(),
                libc::AT_FDCWD,
                b.as_ptr(),
                libc::RENAME_EXCHANGE,
            )
        };
        if done == 0 {
            Ok(())
        } else {
            Err(exchange_error())
        }
    }

    #[cfg(target_os = "macos")]
    fn exchange(&self, a: &Path, b: &Path) -> io::Result<()> {
        let (a, b) = (c_path(a)?, c_path(b)?);
        // SAFETY: both paths are NUL-terminated strings that outlive the
        // call.
        let done = unsafe { libc::renamex_np(a.as_ptr(), b.as_ptr(), libc::RENAME_SWAP) };
        if done == 0 {
            Ok(())
        } else {
            Err(exchange_error())
        }
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    fn exchange(&self, _a: &Path, _b: &Path) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "this system has no exchange of two paths",
        ))
    }

    fn remove_all(&self, path: &Path) -> io::Result<()> {
        match fs::symlink_metadata(path) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
            Ok(m) if m.is_dir() => fs::remove_dir_all(path),
            Ok(_) => fs::remove_file(path),
        }
    }

    fn copy(&self, from: &Path, to: &Path) -> io::Result<()> {
        fs::copy(from, to).map(drop)
    }

    fn copy_permissions(&self, from: &Path, to: &Path) -> io::Result<()> {
        fs::set_permissions(to, fs::metadata(from)?.permissions())
    }

    fn unpack(&self, archive: &Path, into: &Path) -> io::Result<()> {
        fs::create_dir(into)?;
        let gz = flate2::read::GzDecoder::new(fs::File::open(archive)?);
        tar::Archive::new(gz).unpack(into)
    }

    fn sync_tree(&self, path: &Path) -> io::Result<()> {
        let meta = fs::symlink_metadata(path)?;
        if meta.is_dir() {
            for entry in fs::read_dir(path)? {
                self.sync_tree(&entry?.path())?;
            }
            self.sync_dir(path)
        } else if meta.is_file() {
            // Windows flushes a file only through a handle that may write.
            fs::OpenOptions::new()
                .read(true)
                .write(cfg!(windows))
                .open(path)?
                .sync_all()
        } else {
            Ok(())
        }
    }

    fn sync_dir(&self, dir: &Path) -> io::Result<()> {
        if cfg!(unix) {
            fs::File::open(dir)?.sync_all()
        } else {
            Ok(())
        }
    }
}

/// How a target's payload is swapped.
enum Kind {
    /// The payload is one folder, swapped whole; the archive holds it under
    /// this root name.
    Folder(&'static str),
    /// The payload folder's entries are swapped together (Windows).
    Entries,
    /// The payload is one file (the AppImage), swapped whole.
    File,
}

fn kind(target: Target) -> Kind {
    match target {
        Target::MacosAarch64 | Target::MacosX86_64 => Kind::Folder("mcbench-lite.app"),
        Target::LinuxX86_64 | Target::LinuxAarch64 => Kind::Folder("mcbench-lite"),
        Target::WindowsX86_64 => Kind::Entries,
        Target::LinuxX86_64Appimage | Target::LinuxAarch64Appimage => Kind::File,
    }
}

/// The entries of a Windows install that are the install's own, never in
/// its archive.
const INSTALL_OWN: [&str; 2] = ["install-source", "uninstall.exe"];

/// `<path>.old`: where a swap puts what it replaces.
pub(crate) fn old_path(path: &Path) -> PathBuf {
    let mut name = path
        .file_name()
        .expect("an installed path has a file name")
        .to_os_string();
    name.push(".old");
    path.with_file_name(name)
}

/// `.<name><suffix>` beside `path`.
pub(crate) fn beside(path: &Path, suffix: &str) -> PathBuf {
    path.with_file_name(format!(
        ".{}{suffix}",
        path.file_name()
            .expect("an installed path has a file name")
            .to_string_lossy()
    ))
}

/// `.<name>.update.d` beside `path`: where an archive is unpacked.
pub(crate) fn update_dir(path: &Path) -> PathBuf {
    beside(path, ".update.d")
}

fn fail(step: &str, path: &Path, e: io::Error) -> String {
    format!(
        "The update could not {step} {}: {e}; the app was not changed.",
        path.display()
    )
}

fn names_of(entries: &[String]) -> String {
    if entries.is_empty() {
        "nothing".to_string()
    } else {
        entries.join(", ")
    }
}

/// Why a swap did not happen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Failure {
    /// The installed payload is as it was; the sentence says why.
    Unchanged(String),
    /// A step failed and so did reversing it: the payload is not whole, and
    /// the sentence names the renames to make by hand.
    Broken(String),
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Failure::Unchanged(s) | Failure::Broken(s) => f.write_str(s),
        }
    }
}

/// Puts the verified `downloaded` asset in `installed`'s place, or leaves
/// the installed payload as it was and says why. The download itself is
/// the caller's: it stays where it is after a failure, and after a swap of
/// an archive. Blocking (file system). Followed by [`restart`].
pub fn install(downloaded: &Downloaded, installed: &Installed) -> Result<(), Failure> {
    install_with(&Real, downloaded, installed)
}

fn install_with(
    ops: &dyn Ops,
    downloaded: &Downloaded,
    installed: &Installed,
) -> Result<(), Failure> {
    assert_eq!(
        downloaded.target, installed.target,
        "the download is for this install"
    );
    let unchanged = Failure::Unchanged;
    let program = installed
        .program
        .strip_prefix(&installed.path)
        .expect("the program is inside its payload");
    match kind(installed.target) {
        Kind::Folder(root) => {
            let dir = update_dir(&installed.path);
            ops.remove_all(&dir)
                .map_err(|e| unchanged(fail("remove", &dir, e)))?;
            let result = ops
                .unpack(&downloaded.path, &dir)
                .map_err(|e| unchanged(fail("unpack the download into", &dir, e)))
                .and_then(|()| single_root(&dir, root, program).map_err(unchanged))
                .and_then(|new| {
                    the_version(installed, &new, &downloaded.version).map_err(unchanged)?;
                    Ok(new)
                })
                .and_then(|new| {
                    let source = installed.install_source();
                    let to = new.join(
                        source
                            .strip_prefix(&installed.path)
                            .expect("install-source is inside the payload"),
                    );
                    ops.copy(&source, &to).map_err(|e| {
                        unchanged(fail("copy the installed install-source to", &to, e))
                    })?;
                    swap_whole(ops, &installed.path, &new)
                });
            // Empty after a swap, the new payload after a failure. After a
            // broken swap it may hold the old payload, so it stays.
            if !matches!(result, Err(Failure::Broken(_))) {
                let _ = ops.remove_all(&dir);
            }
            result
        }
        Kind::File => {
            ops.copy_permissions(&installed.path, &downloaded.path)
                .map_err(|e| {
                    unchanged(fail(
                        "give the download the permissions of",
                        &installed.path,
                        e,
                    ))
                })?;
            swap_whole(ops, &installed.path, &downloaded.path)
        }
        Kind::Entries => {
            let dir = update_dir(&installed.path);
            ops.remove_all(&dir)
                .map_err(|e| unchanged(fail("remove", &dir, e)))?;
            let result = ops
                .unpack(&downloaded.path, &dir)
                .map_err(|e| unchanged(fail("unpack the download into", &dir, e)))
                .and_then(|()| the_entries(&dir, program).map_err(unchanged))
                .and_then(|names| {
                    the_version(installed, &dir, &downloaded.version).map_err(unchanged)?;
                    Ok(names)
                })
                .and_then(|names| {
                    let pairs: Vec<_> = names
                        .iter()
                        .map(|n| (installed.path.join(n), dir.join(n)))
                        .collect();
                    swap_entries(ops, &installed.path, &dir, &pairs)
                });
            if !matches!(result, Err(Failure::Broken(_))) {
                let _ = ops.remove_all(&dir);
            }
            result
        }
    }
}

/// The names at the root of the unpacked archive, sorted.
fn root_names(dir: &Path) -> Result<Vec<String>, String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .and_then(|d| {
            d.map(|e| e.map(|e| e.file_name().to_string_lossy().into_owned()))
                .collect()
        })
        .map_err(|e| fail("read", dir, e))?;
    names.sort();
    Ok(names)
}

/// The one folder `root` at the root of the unpacked archive, holding the
/// program at `program` (relative to the payload).
fn single_root(dir: &Path, root: &str, program: &Path) -> Result<PathBuf, String> {
    let names = root_names(dir)?;
    let new = dir.join(root);
    if names != [root] || !new.is_dir() {
        return Err(format!(
            "The downloaded archive holds {} at its root, not the one folder {root}; the app was not changed.",
            names_of(&names)
        ));
    }
    if !new.join(program).is_file() {
        return Err(format!(
            "The downloaded archive has no {}; the app was not changed.",
            Path::new(root).join(program).display()
        ));
    }
    Ok(new)
}

/// Refuses an unpacked payload at `new` whose `version` file is not the
/// release's `version` (the file sits at `installed`'s place for it).
fn the_version(installed: &Installed, new: &Path, version: &str) -> Result<(), String> {
    let Some(file) = installed.version_file() else {
        return Ok(());
    };
    let rel = file
        .strip_prefix(&installed.path)
        .expect("the version file is inside the payload");
    let got = crate::read_version(&new.join(rel)).map_err(|_| {
        format!(
            "The downloaded archive has no {}; the app was not changed.",
            rel.display()
        )
    })?;
    if got != version {
        return Err(format!(
            "The downloaded archive's {} says {got:?}, not the release's {version}; the app was not changed.",
            rel.display()
        ));
    }
    Ok(())
}

/// The top-level entries of the unpacked Windows archive, in name order
/// with the program last: the program among them, none of the install's
/// own.
fn the_entries(dir: &Path, program: &Path) -> Result<Vec<String>, String> {
    let mut names = root_names(dir)?;
    let own: Vec<String> = names
        .iter()
        .filter(|n| INSTALL_OWN.contains(&n.as_str()))
        .cloned()
        .collect();
    if !own.is_empty() {
        return Err(format!(
            "The downloaded archive holds {}, which belongs to the install and not to a release; the app was not changed.",
            own.join(" and ")
        ));
    }
    if !dir.join(program).is_file() {
        return Err(format!(
            "The downloaded archive holds {} at its root and not {}; the app was not changed.",
            names_of(&names),
            program.display()
        ));
    }
    let program = program.to_string_lossy().into_owned();
    names.retain(|n| *n != program);
    names.push(program);
    Ok(names)
}

/// One step of a swap, as it is reversed.
enum Done {
    Renamed(PathBuf, PathBuf),
    Exchanged(PathBuf, PathBuf),
}

/// Puts `new` (a folder or a file) at `current`, the old one at
/// `<current>.old`: synced, then one exchange and a rename where the file
/// system has the exchange, two renames where it has not, then the parent
/// folder synced.
fn swap_whole(ops: &dyn Ops, current: &Path, new: &Path) -> Result<(), Failure> {
    ops.sync_tree(new)
        .map_err(|e| Failure::Unchanged(fail("sync to disk", new, e)))?;
    let old = old_path(current);
    ops.remove_all(&old)
        .map_err(|e| Failure::Unchanged(fail("remove the leftover", &old, e)))?;
    let mut done = Vec::new();
    match ops.exchange(new, current) {
        Ok(()) => {
            done.push(Done::Exchanged(new.to_path_buf(), current.to_path_buf()));
            // `new` now holds the old payload.
            if let Err(e) = ops.rename(new, &old) {
                return Err(undo(
                    ops,
                    &done,
                    fail("rename the previous version to", &old, e),
                ));
            }
            done.push(Done::Renamed(new.to_path_buf(), old));
        }
        Err(e) if e.kind() == io::ErrorKind::Unsupported => {
            if let Err(e) = ops.rename(current, &old) {
                return Err(Failure::Unchanged(fail("rename aside", current, e)));
            }
            done.push(Done::Renamed(current.to_path_buf(), old));
            if let Err(e) = ops.rename(new, current) {
                return Err(undo(ops, &done, fail("rename the new app to", current, e)));
            }
            done.push(Done::Renamed(new.to_path_buf(), current.to_path_buf()));
        }
        Err(e) => {
            return Err(Failure::Unchanged(fail(
                "exchange the new app with",
                current,
                e,
            )));
        }
    }
    let parent = current.parent().expect("an installed path has a folder");
    ops.sync_dir(parent)
        .map_err(|e| undo(ops, &done, fail("sync to disk", parent, e)))
}

/// Swaps each pair (installed entry, new entry) of the payload folder
/// `payload` in order, the two renames of each entry adjacent; the entries
/// unpacked in `dir` are synced first and the payload folder after.
fn swap_entries(
    ops: &dyn Ops,
    payload: &Path,
    dir: &Path,
    pairs: &[(PathBuf, PathBuf)],
) -> Result<(), Failure> {
    // A swap that was cut off (a power loss between two renames) left some
    // entries already new, each with its `.old` beside it, and maybe one
    // entry with only its `.old`. An entry already equal to the new one is
    // done and keeps its `.old`; an entry whose installed copy is missing
    // keeps its `.old`, the one old copy; only other `.old` files are an
    // earlier update's leftovers and are removed.
    let pairs: Vec<&(PathBuf, PathBuf)> = pairs
        .iter()
        .filter(|(current, new)| !same_tree(current, new))
        .collect();
    for (current, _) in &pairs {
        if fs::symlink_metadata(current).is_err() {
            continue;
        }
        let old = old_path(current);
        ops.remove_all(&old)
            .map_err(|e| Failure::Unchanged(fail("remove the leftover", &old, e)))?;
    }
    ops.sync_tree(dir)
        .map_err(|e| Failure::Unchanged(fail("sync to disk", dir, e)))?;
    let mut done = Vec::new();
    for (current, new) in pairs {
        if fs::symlink_metadata(current).is_ok() {
            let old = old_path(current);
            if let Err(e) = ops.rename(current, &old) {
                return Err(undo(ops, &done, fail("rename aside", current, e)));
            }
            done.push(Done::Renamed(current.clone(), old));
        }
        if let Err(e) = ops.rename(new, current) {
            return Err(undo(ops, &done, fail("rename the new app to", current, e)));
        }
        done.push(Done::Renamed(new.clone(), current.clone()));
    }
    ops.sync_dir(payload)
        .map_err(|e| undo(ops, &done, fail("sync to disk", payload, e)))
}

/// Whether `a` and `b` hold the same: two files of the same bytes, or two
/// folders of the same names holding the same, all the way down.
fn same_tree(a: &Path, b: &Path) -> bool {
    let (Ok(ma), Ok(mb)) = (fs::symlink_metadata(a), fs::symlink_metadata(b)) else {
        return false;
    };
    if ma.is_file() && mb.is_file() {
        return ma.len() == mb.len() && fs::read(a).ok() == fs::read(b).ok();
    }
    if !(ma.is_dir() && mb.is_dir()) {
        return false;
    }
    let names = |dir: &Path| -> Option<std::collections::BTreeSet<std::ffi::OsString>> {
        fs::read_dir(dir)
            .ok()?
            .map(|e| e.ok().map(|e| e.file_name()))
            .collect()
    };
    match (names(a), names(b)) {
        (Some(na), Some(nb)) if na == nb => na.iter().all(|n| same_tree(&a.join(n), &b.join(n))),
        _ => false,
    }
}

/// Reverses each step in `done`, last first: [`Failure::Unchanged`] with
/// `failure` when that worked, otherwise [`Failure::Broken`] naming each
/// reversal that failed.
fn undo(ops: &dyn Ops, done: &[Done], failure: String) -> Failure {
    let failed: Vec<String> = done
        .iter()
        .rev()
        .filter_map(|step| match step {
            Done::Renamed(from, to) => ops.rename(to, from).err().map(|e| {
                format!(
                    "renaming {} back to {} failed ({e})",
                    to.display(),
                    from.display()
                )
            }),
            Done::Exchanged(a, b) => ops.exchange(a, b).err().map(|e| {
                format!(
                    "exchanging {} and {} back failed ({e})",
                    a.display(),
                    b.display()
                )
            }),
        })
        .collect();
    if failed.is_empty() {
        return Failure::Unchanged(failure);
    }
    let cause = failure
        .strip_suffix("; the app was not changed.")
        .expect("a swap step's failure says the app was not changed");
    Failure::Broken(format!(
        "{cause}, and {}: the old app is whole again only when those renames are made by hand.",
        failed.join(", and ")
    ))
}

/// Removes what the last swap left: `<payload>.old` beside a folder or the
/// AppImage, every `<entry>.old` in a Windows payload folder, the unpack
/// folder `.<name>.update.d` and a `.<name>.update.json.part` beside any
/// payload. An `.old` entry whose installed entry is missing is kept: it is
/// what a broken swap or a power loss left, and the error names it.
/// Blocking.
pub fn remove_leftover(installed: &Installed) -> Result<(), String> {
    let mut leftovers = vec![
        update_dir(&installed.path),
        beside(&installed.path, ".update.json.part"),
    ];
    let mut olds = Vec::new();
    match kind(installed.target) {
        Kind::Folder(_) | Kind::File => {
            olds.push((old_path(&installed.path), installed.path.clone()))
        }
        Kind::Entries => {
            let listed = |e: io::Error| {
                format!(
                    "The previous version's files in {} cannot be listed: {e}",
                    installed.path.display()
                )
            };
            for entry in fs::read_dir(&installed.path).map_err(listed)? {
                let path = entry.map_err(listed)?.path();
                if path.extension().is_some_and(|e| e == "old") {
                    olds.push((path.clone(), path.with_extension("")));
                }
            }
        }
    }
    let mut kept = Vec::new();
    for (old, entry) in olds {
        if fs::symlink_metadata(&entry).is_ok() {
            leftovers.push(old);
        } else if fs::symlink_metadata(&old).is_ok() {
            kept.push(old.display().to_string());
        }
    }
    for path in leftovers {
        Real.remove_all(&path).map_err(|e| {
            format!(
                "The previous version at {} cannot be removed: {e}",
                path.display()
            )
        })?;
    }
    if kept.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "{} {} kept, because what {} replaced is missing: rename {} back by hand or install mcbench-lite again.",
            kept.join(" and "),
            if kept.len() == 1 { "is" } else { "are" },
            if kept.len() == 1 { "it" } else { "they" },
            if kept.len() == 1 { "it" } else { "them" },
        ))
    }
}

/// Puts the previous version back after a swap whose new program could not
/// be started: the `.old` payload back in place (one exchange where the
/// file system has it), the folder synced, and the new one removed (on
/// Windows each `<entry>.old` back over its entry; entries the release
/// added stay). The error is a previous version that could not be put
/// back; a sync or a removal that failed after it is back is the returned
/// note, for `update.log`. Blocking.
pub fn reverse(installed: &Installed) -> Result<Option<String>, String> {
    let cannot = |what: &Path, e: io::Error| {
        format!(
            "The previous version could not be put back at {}: {e}",
            what.display()
        )
    };
    // Once the previous version is in place, what is left of the new one
    // and the folder's sync are logged, not fatal.
    let tidy = |parent: &Path, leftover: &Path| {
        let mut notes = Vec::new();
        if let Err(e) = Real.sync_dir(parent) {
            notes.push(format!(
                "{} could not be synced to disk ({e})",
                parent.display()
            ));
        }
        if let Err(e) = Real.remove_all(leftover) {
            notes.push(format!("{} could not be removed ({e})", leftover.display()));
        }
        (!notes.is_empty()).then(|| {
            format!(
                "The previous version is back in place, but {}.",
                notes.join(", and ")
            )
        })
    };
    match kind(installed.target) {
        Kind::Folder(_) | Kind::File => {
            let current = &installed.path;
            let parent = current.parent().expect("an installed path has a folder");
            let old = old_path(current);
            match Real.exchange(current, &old) {
                Ok(()) => Ok(tidy(parent, &old)),
                Err(e) if e.kind() == io::ErrorKind::Unsupported => {
                    let aside = update_dir(current);
                    Real.remove_all(&aside).map_err(|e| cannot(&aside, e))?;
                    Real.rename(current, &aside)
                        .map_err(|e| cannot(current, e))?;
                    Real.rename(&old, current).map_err(|e| cannot(current, e))?;
                    Ok(tidy(parent, &aside))
                }
                Err(e) => Err(cannot(current, e)),
            }
        }
        Kind::Entries => {
            let listed = |e: io::Error| cannot(&installed.path, e);
            for entry in fs::read_dir(&installed.path).map_err(listed)? {
                let old = entry.map_err(listed)?.path();
                if old.extension().is_some_and(|e| e == "old") {
                    let current = old.with_extension("");
                    Real.remove_all(&current).map_err(|e| cannot(&current, e))?;
                    Real.rename(&old, &current)
                        .map_err(|e| cannot(&current, e))?;
                }
            }
            Ok(None)
        }
    }
}

/// The argument [`restart`] puts first for a program executed because its
/// payload changed while the process started; `main` strips it with
/// [`take_restart_marker`] before it reads its arguments. Nothing of the
/// updater goes into a process's environment.
pub const RESTART_MARKER: &str = "--mcbench-updater-restarted";

/// Removes [`RESTART_MARKER`] from the front of `args` (the program's
/// arguments without its name), answering whether it was there; `main`
/// calls it first and hands the answer to [`crate::at_start`].
pub fn take_restart_marker(args: &mut Vec<String>) -> bool {
    let marked = args.first().is_some_and(|a| a == RESTART_MARKER);
    if marked {
        args.remove(0);
    }
    marked
}

/// The command [`restart`] runs: `installed`'s program with this process's
/// arguments (without a marker of its own), [`RESTART_MARKER`] first when
/// `again`; the environment as it is.
fn restart_command(
    installed: &Installed,
    again: bool,
    args: impl Iterator<Item = std::ffi::OsString>,
) -> std::process::Command {
    let mut command = std::process::Command::new(&installed.program);
    if again {
        command.arg(RESTART_MARKER);
    }
    command.args(args.filter(|a| a != RESTART_MARKER));
    command
}

/// Runs `installed`'s program in place of this process with this process's
/// arguments: `exec` on Unix, so the pid stays; on Windows the program is
/// started and this process exits. `again` marks the new process as one
/// started because the payload changed while this one started
/// ([`RESTART_MARKER`]). Returns only with an error.
pub fn restart(installed: &Installed, again: bool) -> Result<Infallible, String> {
    let command = restart_command(installed, again, std::env::args_os().skip(1));
    run_instead(command).map_err(|e| {
        format!(
            "The updated app at {} cannot be started: {e}",
            installed.program.display()
        )
    })
}

/// `command` in this process's place (`exec`).
#[cfg(unix)]
fn run_instead(mut command: std::process::Command) -> io::Result<Infallible> {
    use std::os::unix::process::CommandExt;
    Err(command.exec())
}

/// `command` started, then this process ends.
#[cfg(windows)]
fn run_instead(mut command: std::process::Command) -> io::Result<Infallible> {
    command.spawn()?;
    std::process::exit(0)
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::collections::BTreeMap;
    use std::io::Write;

    use super::*;

    /// [`Real`], failing the step numbered `at` (counted from 0), and the
    /// step after it too when `also_next`; with `exchange` false, the
    /// exchange answers as on a file system without one (not counted as a
    /// step).
    struct Failing {
        at: usize,
        also_next: bool,
        exchange: bool,
        seen: Cell<usize>,
    }

    impl Failing {
        fn step(&self) -> io::Result<()> {
            let n = self.seen.get();
            self.seen.set(n + 1);
            if n == self.at || (self.also_next && n == self.at + 1) {
                Err(io::Error::other(format!("injected at step {n}")))
            } else {
                Ok(())
            }
        }
    }

    impl Ops for Failing {
        fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
            self.step()?;
            Real.rename(from, to)
        }
        fn exchange(&self, a: &Path, b: &Path) -> io::Result<()> {
            if !self.exchange {
                return Err(io::Error::new(io::ErrorKind::Unsupported, "no exchange"));
            }
            self.step()?;
            Real.exchange(a, b)
        }
        fn remove_all(&self, path: &Path) -> io::Result<()> {
            self.step()?;
            Real.remove_all(path)
        }
        fn copy(&self, from: &Path, to: &Path) -> io::Result<()> {
            self.step()?;
            Real.copy(from, to)
        }
        fn copy_permissions(&self, from: &Path, to: &Path) -> io::Result<()> {
            self.step()?;
            Real.copy_permissions(from, to)
        }
        fn unpack(&self, archive: &Path, into: &Path) -> io::Result<()> {
            self.step()?;
            Real.unpack(archive, into)
        }
        fn sync_tree(&self, path: &Path) -> io::Result<()> {
            self.step()?;
            Real.sync_tree(path)
        }
        fn sync_dir(&self, dir: &Path) -> io::Result<()> {
            self.step()?;
            Real.sync_dir(dir)
        }
    }

    fn failing(at: usize, exchange: bool) -> Failing {
        Failing {
            at,
            also_next: false,
            exchange,
            seen: Cell::new(0),
        }
    }

    /// [`Real`] with the exchange or without it.
    fn ops(exchange: bool) -> Failing {
        failing(usize::MAX, exchange)
    }

    /// The ways a folder or file is swapped on this system: with the
    /// exchange where the system has one (Linux, macOS), and with the two
    /// renames everywhere.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    const EXCHANGE: &[bool] = &[true, false];
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    const EXCHANGE: &[bool] = &[false];

    /// Every file and folder under `root` with its text (folders as
    /// `<dir>`) and, on Unix, its mode.
    fn snapshot(root: &Path) -> BTreeMap<String, String> {
        let mut out = BTreeMap::new();
        let mut stack = vec![root.to_path_buf()];
        while let Some(dir) = stack.pop() {
            for entry in fs::read_dir(&dir).unwrap() {
                let path = entry.unwrap().path();
                // `/` between the parts on every system, so keys read alike.
                let rel = path
                    .strip_prefix(root)
                    .unwrap()
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
                    .join("/");
                let meta = fs::symlink_metadata(&path).unwrap();
                #[cfg(unix)]
                let mode = {
                    use std::os::unix::fs::PermissionsExt;
                    format!(" {:o}", meta.permissions().mode() & 0o777)
                };
                #[cfg(not(unix))]
                let mode = String::new();
                if meta.is_dir() {
                    out.insert(rel, format!("<dir>{mode}"));
                    stack.push(path);
                } else {
                    let bytes = fs::read(&path).unwrap();
                    out.insert(rel, format!("{}{mode}", String::from_utf8_lossy(&bytes)));
                }
            }
        }
        out
    }

    /// A `.tar.gz` of `entries` (name, text), each mode 0755.
    fn archive(path: &Path, entries: &[(&str, &str)]) {
        let gz = flate2::write::GzEncoder::new(
            fs::File::create(path).unwrap(),
            flate2::Compression::fast(),
        );
        let mut tar = tar::Builder::new(gz);
        for (name, text) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_size(text.len() as u64);
            header.set_mode(0o755);
            tar.append_data(&mut header, name, text.as_bytes()).unwrap();
        }
        tar.into_inner().unwrap().finish().unwrap().flush().unwrap();
    }

    /// `rel` (parts between `/`) under `root`, joined part by part, so the
    /// path is written as the product writes its own paths on every system.
    fn at(root: &Path, rel: &str) -> PathBuf {
        rel.split('/')
            .fold(root.to_path_buf(), |path, part| path.join(part))
    }

    fn write(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn downloaded(installed: &Installed) -> Downloaded {
        Downloaded {
            path: beside(&installed.path, ".update"),
            target: installed.target,
            version: "9.9.9".into(),
        }
    }

    /// An installed payload for `target` under `root`: the program and its
    /// install-source as "old" (the program mode 0700 on Unix), the other
    /// files of the payload, and the download holding the new release.
    fn install_of(root: &Path, target: Target) -> (Installed, Downloaded) {
        let (exe, others, new, route): (&str, &[&str], &[(&str, &str)], &str) = match kind(target) {
            Kind::Folder("mcbench-lite.app") => (
                "mcbench-lite.app/Contents/MacOS/mcbench-lite",
                &[
                    "mcbench-lite.app/Contents/MacOS/mcbench-build-child",
                    "mcbench-lite.app/Contents/Resources/icon.icns",
                ],
                &[
                    ("mcbench-lite.app/Contents/MacOS/mcbench-lite", "new app"),
                    (
                        "mcbench-lite.app/Contents/MacOS/mcbench-build-child",
                        "new child",
                    ),
                    ("mcbench-lite.app/Contents/Resources/icon.icns", "new icon"),
                    ("mcbench-lite.app/Contents/Resources/version", "9.9.9\n"),
                ],
                "script\n",
            ),
            Kind::Folder(_) => (
                "lib/mcbench-lite/mcbench-lite",
                &[
                    "lib/mcbench-lite/mcbench-build-child",
                    "lib/mcbench-lite/mcbench-sandbox-agent",
                ],
                &[
                    ("mcbench-lite/mcbench-lite", "new app"),
                    ("mcbench-lite/mcbench-build-child", "new child"),
                    ("mcbench-lite/mcbench-sandbox-agent", "new agent"),
                    (
                        "mcbench-lite/share/applications/mcbench-lite.desktop",
                        "new",
                    ),
                    ("mcbench-lite/version", "9.9.9\n"),
                ],
                "script\n",
            ),
            Kind::Entries => (
                "Programs/mcbench-lite/mcbench-lite.exe",
                &[
                    "Programs/mcbench-lite/mcbench-build-child.exe",
                    "Programs/mcbench-lite/resources/guest.img",
                    "Programs/mcbench-lite/uninstall.exe",
                ],
                &[
                    ("added.dll", "new dll"),
                    ("mcbench-build-child.exe", "new child"),
                    ("mcbench-lite.exe", "new app"),
                    ("resources/guest.img", "new image"),
                    ("version", "9.9.9\n"),
                ],
                "direct\n",
            ),
            Kind::File => unreachable!("the AppImage has its own tests"),
        };
        let exe = at(root, exe);
        let installed = Installed::locate(&exe, target, None).unwrap();
        for path in std::iter::once(exe.clone()).chain(others.iter().map(|o| at(root, o))) {
            write(&path, "old");
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&exe, fs::Permissions::from_mode(0o700)).unwrap();
        }
        write(&installed.install_source(), route);
        write(&installed.version_file().unwrap(), "1.0.0\n");
        let download = downloaded(&installed);
        archive(&download.path, new);
        (installed, download)
    }

    fn read(path: &Path) -> String {
        fs::read_to_string(path).unwrap()
    }

    const FOLDER_TARGETS: [Target; 4] = [
        Target::MacosAarch64,
        Target::MacosX86_64,
        Target::LinuxX86_64,
        Target::LinuxAarch64,
    ];

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn the_exchange_is_one_step_where_the_file_system_has_it() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (dir.path().join("a"), dir.path().join("b"));
        write(&a.join("x"), "a");
        write(&b.join("x"), "b");
        Real.exchange(&a, &b).unwrap();
        assert_eq!(
            (read(&a.join("x")), read(&b.join("x"))),
            ("b".into(), "a".into())
        );
    }

    #[test]
    fn folder_swap_replaces_every_file_and_keeps_install_source() {
        for &exchange in EXCHANGE {
            for target in FOLDER_TARGETS {
                let dir = tempfile::tempdir().unwrap();
                let (installed, download) = install_of(dir.path(), target);
                write(&old_path(&installed.path).join("stale"), "older");
                install_with(&ops(exchange), &download, &installed).unwrap();
                drop(download);
                let child = installed.program.with_file_name("mcbench-build-child");
                assert_eq!(read(&installed.program), "new app", "{target:?}");
                assert_eq!(read(&child), "new child", "{target:?}");
                assert_eq!(read(&installed.install_source()), "script\n", "{target:?}");
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    assert_eq!(
                        fs::metadata(&installed.program)
                            .unwrap()
                            .permissions()
                            .mode()
                            & 0o777,
                        0o755
                    );
                }
                let old = old_path(&installed.path);
                let rel = installed.program.strip_prefix(&installed.path).unwrap();
                assert_eq!(read(&old.join(rel)), "old", "{target:?}");
                assert!(!update_dir(&installed.path).exists());
                remove_leftover(&installed).unwrap();
                assert!(!old.exists());
                let left: Vec<_> = fs::read_dir(installed.path.parent().unwrap())
                    .unwrap()
                    .map(|e| e.unwrap().file_name())
                    .collect();
                assert_eq!(left, [installed.path.file_name().unwrap()], "{target:?}");
                remove_leftover(&installed).unwrap();
            }
        }
    }

    /// The Linux `script` route's payload folder holds a third program, the
    /// sandbox agent, which the pair swap used to leave old.
    #[test]
    fn the_linux_folder_swap_replaces_the_sandbox_agent() {
        let dir = tempfile::tempdir().unwrap();
        let (installed, download) = install_of(dir.path(), Target::LinuxX86_64);
        install(&download, &installed).unwrap();
        assert_eq!(
            read(&installed.path.join("mcbench-sandbox-agent")),
            "new agent"
        );
        assert_eq!(
            read(
                &installed
                    .path
                    .join("share/applications/mcbench-lite.desktop")
            ),
            "new"
        );
    }

    /// Steps of the folder swap: 0 remove the unpack folder, 1 unpack,
    /// 2 copy install-source, 3 sync the new folder, 4 remove the leftover,
    /// then with the exchange 5 exchange, 6 rename the old one to `.old`,
    /// and without it 5 rename aside, 6 rename in; 7 sync the parent
    /// folder; then the reversals.
    #[test]
    fn folder_swap_failure_at_each_step_keeps_the_old_payload() {
        for &exchange in EXCHANGE {
            for target in FOLDER_TARGETS {
                for at in 0..8 {
                    let dir = tempfile::tempdir().unwrap();
                    let (installed, download) = install_of(dir.path(), target);
                    let before = snapshot(dir.path());
                    let err =
                        install_with(&failing(at, exchange), &download, &installed).unwrap_err();
                    let Failure::Unchanged(err) = err else {
                        panic!("{target:?} step {at}: {err}")
                    };
                    assert!(
                        err.ends_with("the app was not changed."),
                        "{target:?} step {at}: {err}"
                    );
                    assert!(
                        err.contains(&format!("injected at step {at}")),
                        "{target:?} step {at}: {err}"
                    );
                    assert_eq!(
                        snapshot(dir.path()),
                        before,
                        "{target:?} exchange {exchange} step {at}"
                    );
                }
            }
        }
    }

    /// A broken swap is [`Failure::Broken`], keeps the unpack folder (it
    /// may hold the old payload), and the next start's leftover removal
    /// keeps the `.old` that the sentence names.
    #[test]
    fn folder_swap_whose_reversal_fails_is_broken_and_keeps_the_old_app() {
        let dir = tempfile::tempdir().unwrap();
        let (installed, download) = install_of(dir.path(), Target::LinuxAarch64);
        // Without the exchange: step 6 (the new folder in) and step 7 (the
        // old one back).
        let ops = Failing {
            at: 6,
            also_next: true,
            exchange: false,
            seen: Cell::new(0),
        };
        let err = install_with(&ops, &download, &installed).unwrap_err();
        let old = old_path(&installed.path);
        assert_eq!(
            err,
            Failure::Broken(format!(
                "The update could not rename the new app to {}: injected at step 6, and renaming {} back to {} failed (injected at step 7): the old app is whole again only when those renames are made by hand.",
                installed.path.display(),
                old.display(),
                installed.path.display()
            ))
        );
        assert_eq!(read(&old.join("mcbench-lite")), "old");
        assert!(update_dir(&installed.path).join("mcbench-lite").is_dir());
        assert_eq!(
            remove_leftover(&installed).unwrap_err(),
            format!(
                "{} is kept, because what it replaced is missing: rename it back by hand or install mcbench-lite again.",
                old.display()
            )
        );
        assert_eq!(read(&old.join("mcbench-lite")), "old");
    }

    /// The same with the exchange (Linux and macOS): step 6 (the old one to
    /// .old) and step 7 (the exchange back) fail; the old payload is in the
    /// unpack folder, kept.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn folder_exchange_whose_reversal_fails_keeps_the_old_app_in_the_unpack_folder() {
        let dir = tempfile::tempdir().unwrap();
        let (installed, download) = install_of(dir.path(), Target::LinuxAarch64);
        let ops = Failing {
            at: 6,
            also_next: true,
            exchange: true,
            seen: Cell::new(0),
        };
        let Failure::Broken(err) = install_with(&ops, &download, &installed).unwrap_err() else {
            panic!("broken")
        };
        assert!(err.contains("exchanging "), "{err}");
        let unpacked = update_dir(&installed.path).join("mcbench-lite");
        assert_eq!(read(&unpacked.join("mcbench-lite")), "old");
    }

    #[test]
    fn folder_archives_without_the_root_or_the_program_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let (installed, download) = install_of(dir.path(), Target::MacosX86_64);
        let before = snapshot(dir.path());
        archive(&download.path, &[("README", "x")]);
        assert_eq!(
            install(&download, &installed).unwrap_err().to_string(),
            "The downloaded archive holds README at its root, not the one folder mcbench-lite.app; the app was not changed."
        );
        archive(
            &download.path,
            &[("mcbench-lite.app/Contents/Resources/icon.icns", "x")],
        );
        assert_eq!(
            install(&download, &installed).unwrap_err().to_string(),
            format!(
                "The downloaded archive has no {}; the app was not changed.",
                Path::new("mcbench-lite.app")
                    .join(installed.program.strip_prefix(&installed.path).unwrap())
                    .display()
            )
        );
        let mut after = snapshot(dir.path());
        after.insert(
            ".mcbench-lite.app.update".into(),
            before[".mcbench-lite.app.update"].clone(),
        );
        assert_eq!(after, before);
    }

    #[test]
    fn entry_swap_replaces_the_archives_entries_and_leaves_the_installs_own() {
        let dir = tempfile::tempdir().unwrap();
        let (installed, download) = install_of(dir.path(), Target::WindowsX86_64);
        let p = &installed.path;
        write(&p.join("mcbench-lite.exe.old"), "older");
        install(&download, &installed).unwrap();
        assert_eq!(read(&p.join("mcbench-lite.exe")), "new app");
        assert_eq!(read(&p.join("mcbench-build-child.exe")), "new child");
        assert_eq!(read(&p.join("added.dll")), "new dll");
        assert_eq!(read(&p.join("resources/guest.img")), "new image");
        assert_eq!(read(&p.join("install-source")), "direct\n");
        assert_eq!(read(&p.join("uninstall.exe")), "old");
        assert_eq!(read(&p.join("mcbench-lite.exe.old")), "old");
        assert_eq!(read(&p.join("resources.old/guest.img")), "old");
        assert!(!update_dir(p).exists());
        remove_leftover(&installed).unwrap();
        let mut left: Vec<_> = fs::read_dir(p)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        left.sort();
        assert_eq!(
            left,
            [
                "added.dll",
                "install-source",
                "mcbench-build-child.exe",
                "mcbench-lite.exe",
                "resources",
                "uninstall.exe",
                "version"
            ]
        );
        remove_leftover(&installed).unwrap();
    }

    /// F1: the unpacked payload's version file must be the release's.
    #[test]
    fn a_download_of_another_version_is_refused() {
        for target in [Target::LinuxX86_64, Target::WindowsX86_64] {
            let dir = tempfile::tempdir().unwrap();
            let (installed, download) = install_of(dir.path(), target);
            let (root, exe) = if target == Target::WindowsX86_64 {
                ("", "mcbench-lite.exe")
            } else {
                ("mcbench-lite/", "mcbench-lite/mcbench-lite")
            };
            let version = format!("{root}version");
            archive(&download.path, &[(exe, "new"), (&version, "1.2.3\n")]);
            assert_eq!(
                install(&download, &installed).unwrap_err().to_string(),
                "The downloaded archive's version says \"1.2.3\", not the release's 9.9.9; the app was not changed.",
                "{target:?}"
            );
            archive(&download.path, &[(exe, "new")]);
            assert_eq!(
                install(&download, &installed).unwrap_err().to_string(),
                "The downloaded archive has no version; the app was not changed.",
                "{target:?}"
            );
            assert_eq!(read(&installed.program), "old");
        }
    }

    /// F3: the restart marker is an argument, first, and nothing goes into
    /// the environment; `main` strips the marker, so neither the app's
    /// arguments nor any child's carry it.
    #[test]
    fn the_restart_marker_is_an_argument_and_the_environment_is_untouched() {
        let installed = Installed {
            target: Target::LinuxX86_64,
            path: PathBuf::from("/p"),
            program: PathBuf::from("/p/mcbench-lite"),
        };
        let command = restart_command(
            &installed,
            true,
            ["app", RESTART_MARKER]
                .map(std::ffi::OsString::from)
                .into_iter(),
        );
        assert_eq!(command.get_envs().count(), 0);
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            [RESTART_MARKER, "app"]
        );
        let plain = restart_command(&installed, false, [RESTART_MARKER.into()].into_iter());
        assert_eq!(plain.get_args().count(), 0);
        let mut args = vec![RESTART_MARKER.to_string(), "app".to_string()];
        assert!(take_restart_marker(&mut args));
        assert_eq!(args, ["app"]);
        assert!(!take_restart_marker(&mut args));
        assert_eq!(args, ["app"]);
    }

    /// F3, run: a program started by `restart` gets the marker as its first
    /// argument and exactly this process's environment.
    #[cfg(unix)]
    #[test]
    fn a_restarted_childs_environment_carries_nothing_of_the_updaters() {
        use std::collections::BTreeSet;
        use std::os::unix::ffi::OsStrExt;
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let program = dir.path().join("mcbench-lite");
        // Its arguments, then its whole environment, each ended by a NUL.
        fs::write(
            &program,
            "#!/bin/sh\nfor a in \"$@\"; do printf '%s\\0' \"$a\"; done\nprintf 'ENV\\0'\nexec env -0\n",
        )
        .unwrap();
        fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
        let installed = Installed {
            target: Target::LinuxX86_64,
            path: dir.path().to_path_buf(),
            program,
        };
        let out = restart_command(&installed, true, ["app".into()].into_iter())
            .output()
            .unwrap();
        assert!(out.status.success(), "{out:?}");
        let fields: Vec<&[u8]> = out.stdout.split(|&b| b == 0).collect();
        let env_at = fields.iter().position(|f| *f == b"ENV").unwrap();
        assert_eq!(
            fields[..env_at],
            [RESTART_MARKER.as_bytes(), b"app"],
            "the arguments"
        );
        // What the shell sets itself, and macOS's removal of DYLD_ variables
        // for a protected program such as /bin/sh, are not the updater's.
        let own = |entry: &[u8]| {
            let name = entry.split(|&b| b == b'=').next().unwrap_or_default();
            [&b"_"[..], b"SHLVL", b"PWD", b"OLDPWD"].contains(&name) || name.starts_with(b"DYLD_")
        };
        let child: BTreeSet<Vec<u8>> = fields[env_at + 1..]
            .iter()
            .filter(|f| !f.is_empty() && !own(f))
            .map(|f| f.to_vec())
            .collect();
        let parent: BTreeSet<Vec<u8>> = std::env::vars_os()
            .map(|(name, value)| [name.as_bytes(), b"=", value.as_bytes()].concat())
            .filter(|f| !own(f))
            .collect();
        assert_eq!(
            child, parent,
            "the child's environment is exactly the parent's"
        );
    }

    #[test]
    fn the_entries_are_in_name_order_with_the_program_last() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["a.dll", "mcbench-lite.exe", "z.dll"] {
            write(&dir.path().join(name), "x");
        }
        assert_eq!(
            the_entries(dir.path(), Path::new("mcbench-lite.exe")).unwrap(),
            ["a.dll", "z.dll", "mcbench-lite.exe"]
        );
    }

    /// Steps of the entry swap, the archive's entries in name order with
    /// the program last (`added.dll`, `mcbench-build-child.exe`,
    /// `resources`, `version`, `mcbench-lite.exe`): 0 remove the unpack
    /// folder, 1 unpack, 2 to 5 remove the four leftovers (`added.dll` has
    /// no installed copy, so none), 6 sync the unpacked entries, then each
    /// entry's two renames together: 7 `added.dll` in (it is new), 8 and 9
    /// the build child aside and in, 10 and 11 `resources`, 12 and 13
    /// `version`, 14 and 15 `mcbench-lite.exe`; 16 sync the payload folder;
    /// then the reversals.
    #[test]
    fn entry_swap_failure_at_each_step_keeps_the_old_set() {
        for at in 0..17 {
            let dir = tempfile::tempdir().unwrap();
            let (installed, download) = install_of(dir.path(), Target::WindowsX86_64);
            let before = snapshot(dir.path());
            let err = install_with(&failing(at, false), &download, &installed).unwrap_err();
            let Failure::Unchanged(err) = err else {
                panic!("step {at}: {err}")
            };
            assert!(
                err.ends_with("the app was not changed."),
                "step {at}: {err}"
            );
            assert!(
                err.contains(&format!("injected at step {at}")),
                "step {at}: {err}"
            );
            assert_eq!(snapshot(dir.path()), before, "step {at}");
        }
    }

    /// [`Failing`] that keeps failing from step `at` on, so a swap stops
    /// there with nothing reversed, as a power loss leaves it.
    fn halting(at: usize) -> Halting {
        Halting(Cell::new(0), at)
    }

    struct Halting(Cell<usize>, usize);

    impl Halting {
        fn step(&self) -> io::Result<()> {
            let n = self.0.get();
            self.0.set(n + 1);
            if n >= self.1 {
                Err(io::Error::other("the power went"))
            } else {
                Ok(())
            }
        }
    }

    impl Ops for Halting {
        fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
            self.step()?;
            Real.rename(from, to)
        }
        fn exchange(&self, _a: &Path, _b: &Path) -> io::Result<()> {
            Err(io::Error::new(io::ErrorKind::Unsupported, "no exchange"))
        }
        fn remove_all(&self, path: &Path) -> io::Result<()> {
            self.step()?;
            Real.remove_all(path)
        }
        fn copy(&self, from: &Path, to: &Path) -> io::Result<()> {
            self.step()?;
            Real.copy(from, to)
        }
        fn copy_permissions(&self, from: &Path, to: &Path) -> io::Result<()> {
            self.step()?;
            Real.copy_permissions(from, to)
        }
        fn unpack(&self, archive: &Path, into: &Path) -> io::Result<()> {
            self.step()?;
            Real.unpack(archive, into)
        }
        fn sync_tree(&self, path: &Path) -> io::Result<()> {
            self.step()?;
            Real.sync_tree(path)
        }
        fn sync_dir(&self, dir: &Path) -> io::Result<()> {
            self.step()?;
            Real.sync_dir(dir)
        }
    }

    /// W2: a Windows swap cut off at each of its renames (steps 7 to 15)
    /// and at the folder's sync, then run again from the same download,
    /// ends with every entry new and the old copy of each replaced entry
    /// as its `.old`, so a reversal still puts the old program back.
    #[test]
    fn an_entry_swap_cut_off_at_each_rename_is_finished_by_the_next() {
        for at in 7..17 {
            let dir = tempfile::tempdir().unwrap();
            let (installed, download) = install_of(dir.path(), Target::WindowsX86_64);
            let _ = install_with(&halting(at), &download, &installed);
            install(&download, &installed).unwrap();
            let p = &installed.path;
            for (name, new) in [
                ("added.dll", "new dll"),
                ("mcbench-build-child.exe", "new child"),
                ("resources/guest.img", "new image"),
                ("version", "9.9.9\n"),
                ("mcbench-lite.exe", "new app"),
            ] {
                assert_eq!(read(&p.join(name)), new, "cut at {at}: {name}");
            }
            for (old, text) in [
                ("mcbench-build-child.exe.old", "old"),
                ("resources.old/guest.img", "old"),
                ("version.old", "1.0.0\n"),
                ("mcbench-lite.exe.old", "old"),
            ] {
                assert_eq!(read(&p.join(old)), text, "cut at {at}: {old}");
            }
            assert!(!p.join("added.dll.old").exists(), "cut at {at}");
            reverse(&installed).unwrap();
            assert_eq!(read(&installed.program), "old", "cut at {at}");
            assert_eq!(read(&p.join("version")), "1.0.0\n", "cut at {at}");
        }
    }

    /// The Windows case of a running executable: renaming the new
    /// `mcbench-lite.exe` in fails (step 15, after every other entry is
    /// in). Every rename is reversed, so the old `mcbench-lite.exe`, which
    /// the running program is, is back at its path with every entry beside
    /// it as it was, and the old program starts normally.
    #[test]
    fn a_failed_rename_of_the_new_executable_rolls_back_to_the_old_program() {
        let dir = tempfile::tempdir().unwrap();
        let (installed, download) = install_of(dir.path(), Target::WindowsX86_64);
        let before = snapshot(dir.path());
        let err = install_with(&failing(15, false), &download, &installed).unwrap_err();
        assert_eq!(
            err,
            Failure::Unchanged(format!(
                "The update could not rename the new app to {}: injected at step 15; the app was not changed.",
                installed.program.display()
            ))
        );
        assert_eq!(snapshot(dir.path()), before);
        assert_eq!(read(&installed.program), "old");
        assert!(!installed.path.join("added.dll").exists());
    }

    /// The reversal of the program's own rename aside fails too: broken,
    /// and the leftover removal keeps `mcbench-lite.exe.old`, the one copy
    /// of the old program.
    #[test]
    fn entry_swap_whose_reversal_fails_is_broken_and_keeps_the_old_program() {
        let dir = tempfile::tempdir().unwrap();
        let (installed, download) = install_of(dir.path(), Target::WindowsX86_64);
        // Step 15 (the new program in) and step 16 (the first reversal: the
        // old program back).
        let ops = Failing {
            at: 15,
            also_next: true,
            exchange: false,
            seen: Cell::new(0),
        };
        let err = install_with(&ops, &download, &installed).unwrap_err();
        let p = &installed.path;
        assert_eq!(
            err,
            Failure::Broken(format!(
                "The update could not rename the new app to {}: injected at step 15, and renaming {} back to {} failed (injected at step 16): the old app is whole again only when those renames are made by hand.",
                p.join("mcbench-lite.exe").display(),
                p.join("mcbench-lite.exe.old").display(),
                p.join("mcbench-lite.exe").display()
            ))
        );
        assert!(!p.join("mcbench-lite.exe").exists());
        let err = remove_leftover(&installed).unwrap_err();
        assert!(err.contains("mcbench-lite.exe.old is kept"), "{err}");
        assert_eq!(read(&p.join("mcbench-lite.exe.old")), "old");
    }

    #[test]
    fn entry_archives_without_the_program_or_with_the_installs_own_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let (installed, download) = install_of(dir.path(), Target::WindowsX86_64);
        let before = snapshot(dir.path());
        archive(
            &download.path,
            &[("mcbench-lite.exe", "new"), ("install-source", "direct\n")],
        );
        assert_eq!(
            install(&download, &installed).unwrap_err().to_string(),
            "The downloaded archive holds install-source, which belongs to the install and not to a release; the app was not changed."
        );
        archive(&download.path, &[("mcbench-build-child.exe", "new")]);
        assert_eq!(
            install(&download, &installed).unwrap_err().to_string(),
            "The downloaded archive holds mcbench-build-child.exe at its root and not mcbench-lite.exe; the app was not changed."
        );
        let mut after = snapshot(dir.path());
        after.insert(
            "Programs/.mcbench-lite.update".into(),
            before["Programs/.mcbench-lite.update"].clone(),
        );
        assert_eq!(after, before);
    }

    /// After a swap whose new program cannot start, the previous version
    /// goes back in place and the new one is removed.
    #[test]
    fn reverse_puts_the_previous_version_back() {
        for target in FOLDER_TARGETS.into_iter().chain([Target::WindowsX86_64]) {
            let dir = tempfile::tempdir().unwrap();
            let (installed, download) = install_of(dir.path(), target);
            let before = snapshot(dir.path());
            install(&download, &installed).unwrap();
            reverse(&installed).unwrap();
            let mut after = snapshot(dir.path());
            if target == Target::WindowsX86_64 {
                // An entry the release added stays.
                assert_eq!(
                    after.remove("Programs/mcbench-lite/added.dll").as_deref(),
                    Some(if cfg!(unix) { "new dll 755" } else { "new dll" })
                );
            }
            assert_eq!(after, before, "{target:?}");
        }
        let dir = tempfile::tempdir().unwrap();
        let (installed, download) = appimage_install(dir.path(), 0o755);
        let before = snapshot(dir.path());
        install(&download, &installed).unwrap();
        reverse(&installed).unwrap();
        let mut after = snapshot(dir.path());
        after.insert(
            ".mcbench-lite.AppImage.update".into(),
            before[".mcbench-lite.AppImage.update"].clone(),
        );
        assert_eq!(after, before);
    }

    fn appimage_install(root: &Path, download_mode: u32) -> (Installed, Downloaded) {
        let file = root.join("mcbench-lite.AppImage");
        let installed = Installed::locate(
            Path::new("/tmp/.mount/usr/bin/mcbench-lite"),
            Target::LinuxX86_64Appimage,
            Some(file.clone()),
        )
        .unwrap();
        fs::write(&file, "old").unwrap();
        let download = downloaded(&installed);
        fs::write(&download.path, "new").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&file, fs::Permissions::from_mode(0o755)).unwrap();
            fs::set_permissions(&download.path, fs::Permissions::from_mode(download_mode)).unwrap();
        }
        #[cfg(not(unix))]
        let _ = download_mode;
        (installed, download)
    }

    #[test]
    fn appimage_swap_keeps_the_mode_and_the_previous_file() {
        for &exchange in EXCHANGE {
            let dir = tempfile::tempdir().unwrap();
            let (installed, download) = appimage_install(dir.path(), 0o600);
            install_with(&ops(exchange), &download, &installed).unwrap();
            assert_eq!(read(&installed.program), "new");
            assert!(!download.path.exists());
            assert_eq!(read(&old_path(&installed.path)), "old");
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                assert_eq!(
                    fs::metadata(&installed.path).unwrap().permissions().mode() & 0o777,
                    0o755
                );
            }
            remove_leftover(&installed).unwrap();
            assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
        }
    }

    /// Steps of the AppImage swap: 0 copy the mode, 1 sync the download,
    /// 2 remove the leftover, then 3 exchange and 4 the old file to `.old`
    /// (or 3 rename aside and 4 rename in), 5 sync the folder. The download
    /// stays for the next start.
    #[test]
    fn appimage_failure_at_each_step_keeps_the_old_app() {
        for &exchange in EXCHANGE {
            for at in 0..6 {
                let dir = tempfile::tempdir().unwrap();
                let (installed, download) = appimage_install(dir.path(), 0o755);
                let before = snapshot(dir.path());
                let err = install_with(&failing(at, exchange), &download, &installed).unwrap_err();
                let Failure::Unchanged(err) = err else {
                    panic!("step {at}: {err}")
                };
                assert!(
                    err.ends_with("the app was not changed."),
                    "step {at}: {err}"
                );
                assert_eq!(
                    snapshot(dir.path()),
                    before,
                    "exchange {exchange} step {at}"
                );
            }
        }
    }
}
