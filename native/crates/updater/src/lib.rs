//! # mcbench-updater
//!
//! How the app knows its install route and, for the routes it updates
//! itself (`script` on macOS and Linux, `appimage`, `direct` on Windows),
//! the in-app updater: `latest.json`
//! beside the release's assets (its format is `LATEST.md` beside this
//! crate's `Cargo.toml`), a minisign signature checked against a public key
//! compiled into the binary (`minisign-verify` 0.3.0), key rotation, the
//! background check and download, and the swap at the next start. The
//! program `mcbench-feed` (feature `feed`) writes what the updater reads: the
//! key pair, the deterministic archives, the signatures and `latest.json`.
//!
//! An install is one payload folder, and the app finds every shipped file
//! beside its own executable. An update swaps that payload folder:
//!
//! | Target | Payload | Swap |
//! |---|---|---|
//! | `macos-aarch64`, `macos-x86_64` | the `.app` bundle (one architecture) | unpack the archive (`mcbench-lite.app` at its root) beside the bundle, give it the installed `install-source`, sync it to disk, exchange it with the installed bundle in one step (`renamex_np` with `RENAME_SWAP`), rename the old one to `<name>.old`, sync the folder |
//! | `linux-x86_64`, `linux-aarch64` (route `script`) | the executable's folder, `~/.local/lib/mcbench-lite` | the same as the bundle, with `mcbench-lite/` at the archive's root and `renameat2` with `RENAME_EXCHANGE` |
//! | `linux-x86_64-appimage`, `linux-aarch64-appimage` | the AppImage file (`APPIMAGE`) | the verified file given the AppImage's mode, synced, exchanged with it, the old one renamed to `<name>.old` |
//! | `windows-x86_64` (route `direct`) | the executable's folder, `%LOCALAPPDATA%\Programs\mcbench-lite` | Windows refuses to rename a folder that holds a running executable, so every top-level entry of the archive is swapped in name order with `mcbench-lite.exe` last, each entry's two renames adjacent (the installed entry to `<name>.old`, the new one in); `install-source` and `uninstall.exe`, which the archive does not hold, are left alone |
//!
//! Where the file system refuses the exchange, a folder or the AppImage is
//! swapped by two renames. A failure at any step of a swap reverses every
//! step made so far, in reverse order, so the old install stays; when a
//! reversal fails too, the install is not whole and the start shows the
//! sentence as fatal. The `.old` entries are removed by the updater's
//! thread once the new program's core has started, and an `.old` entry
//! whose installed entry is missing is never removed.
//!
//! ## Owner
//!
//! The stage-five session. [`UpdateState`] and [`state`] are a seam with
//! the stage-four session (Settings shows where updates come from and what
//! the updater did); the wiring into the app's core is the stage-three
//! session's.
//!
//! ## Seams
//!
//! | Seam | What crosses | Who calls whom | Thread or process |
//! |---|---|---|---|
//! | the installer | the `install-source` file ([`install_source_path`]: beside the executable on Linux and Windows, `Contents/Resources` on macOS), one of [`InstallSource`]'s values; a missing or unknown file is an error; and the `version` file ([`VERSION_FILE`]) in the same place, written by the packaging step | the installer writes it; [`install_source`] reads it; a swap carries the installed one into the new payload | at start |
//! | the uninstallers | [`Installed::updater_files`]: every name the updater can make beside a payload (the waiting download, its `.json` and `.json.part`, the unpack folder, the held and refused releases, the presence and gate files, `<name>.old`; on Windows `<entry>.old` inside the payload folder) | the uninstall routes remove them with the payload | |
//! | the app's start | [`at_start`] after the crash hook and before the core and any window, with the logs folder and a closure taking the core's instance lock; then [`Updater::check_in_background`] with a clone of the core's HTTP client after the core started; the returned [`Updater`] holds the install shared and is kept for the life of the process | `mcbench-lite app`'s `main.rs` calls both | the main thread; then the thread [`THREAD_NAME`] (`mcbench-updater`) with a current-thread tokio runtime per check, whose threads have the same name |
//! | a headless run | [`hold_install`]: a shared hold on the install for the life of the process; it cannot fail (no hold outside an install or where none is possible); when the installed program changed while it waited it prints [`CHANGED_WHILE_STARTING`] and exits with status 2 | `mcbench-lite headless`'s `main.rs` arm, before `mcbench_headless::run` | the main thread |
//! | the error screen | [`hold_install_quietly`]: the same shared hold as [`hold_install`] for the life of the process; it cannot fail, never prints and never exits (no hold where the payload is not this program's, its `version` cannot be read, or this process was loaded from a moved payload). [`installed_program`]: the program the screen's `Restart` executes, [`Installed::program`] at the install's real path for a self-updating install (the AppImage file for the AppImage), `current_exe()` otherwise, the error being `current_exe()`'s | the app's error-screen process (started by the app once more after a panic on the window's side; it starts no core and no child from the payload) calls both | the main thread of that process |
//! | interface | [`state`] gives [`UpdateState`]; [`InstallSource::package_manager`] gives the line Settings shows for the package-manager routes | Settings reads it | any thread |
//! | network | `latest.json` and the release asset, through the caller's `reqwest::Client` (the core's, from `mcbench_setup::fetch::client()`) | [`check`] and [`download`] fetch, a check within 60 seconds in all and a download until it receives nothing for 60 seconds ([`STALL`]); both stop on the caller's `CancellationToken` | the updater's thread |
//! | the install | [`Installed`] (what is replaced and what is restarted), [`install`], [`remove_leftover`], [`restart`], [`reverse`]; the waiting download `.<name>.update` and the `latest.json` it was checked against, `.<name>.update.json`, beside the payload; the held version `.<name>.update.held`; the presence file `.<name>.lock` and the gate `.<name>.gate` ([`InstallHold`]) | [`at_start`], [`start_with`] and [`check_once`] | blocking file system work |
//! | the logs | `update.log` in the app's logs folder: one line per failure, `<unix seconds> <sentence>`, cut to its last 256 KiB at each start | the updater appends | at start and on the updater's threads |
//! | the build | `MCBENCH_UPDATE_KEY_FILE` names the minisign public key (`minisign.pub`) and `MCBENCH_UPDATE_FEED` the address of `latest.json`, both compiled in ([`Feed::compiled_in`]); neither set, the build has no updater; one alone fails the build | the release scripts set them | `build.rs` |
//! | the release | `mcbench-feed keygen`, `pack`, `sign`, `sign-key` (`src/bin/mcbench-feed.rs`), sharing [`Target`]'s asset names and `latest.json`'s types and signed comment with the reader | the release scripts run it | a program, feature `feed` |
//!
//! ## Behaviour defined by
//!
//! No test of today's app. The plan's checks: the Windows VM's
//! swap-then-restart (renaming the running executable aside worked,
//! overwriting or deleting it failed), a tampered `latest.json` or asset
//! refused, and key rotation (the current key signs the next public key).
//! This crate's tests: `tests/updater.rs` (a local file server, the app's
//! own client, key pairs made at test time with the `minisign` crate),
//! `tests/feed.rs` (keygen, pack, sign, serve, check, download, install and
//! the start's swap of a waiting download, end to end) and the
//! failure-injection tests of `src/swap.rs`, which fail every step of every
//! swap once, with the exchange and without it, and compare the install's
//! files before and after.
//!
//! ## Decided
//!
//! | Question | Decision | Why |
//! |---|---|---|
//! | `latest.json`'s binding to the asset | Each asset's signature carries a trusted comment naming version, target, file, size and sha256; `latest.json`'s entry must equal it, the file must be the target's fixed asset name, and the size at most 512 MiB ([`MAX_ASSET`], refused before the download) | minisign's global signature covers the trusted comment, so a changed version, size, sha256, target or file in `latest.json` is refused without signing `latest.json` itself; the size can only be proven over the downloaded bytes, so it is capped first |
//! | The key chain's trusted comment | written as `mcbench-lite update key` by `mcbench-feed sign-key`, not read by the updater | the signature over the next key's base64 line is what binds it |
//! | When the swap happens | at the next start, before the core and any window: a download finished in the background waits beside the install with the `latest.json` it was checked against; at start every check of [`check`] and [`download`] is made again from those two files without the network, then the swap, then the new program is executed with the same arguments (`exec` on Unix, so the pid stays; spawn and exit on Windows; `$APPIMAGE` for the AppImage) | the running app is never replaced under its own open windows, its engine threads or a running build child; the files could have changed since the download |
//! | A waiting download that fails a check | deleted, and the app starts as it is | it is not the released file, and the next check fetches it again |
//! | A refused release | when `latest.json` is refused (the key, the signed comment, the name, the size, a downgrade) or its asset is (its size, sha256 or signature), the sha256 of those `latest.json` bytes and the sentence go to `.<name>.update.refused` beside the payload and the sentence to `update.log` once; later checks still fetch `latest.json`, and while its bytes are the same they do nothing more ([`UpdateState::Refused`] with the first sentence, no download, no line); a changed `latest.json` removes the file and the check goes on. A failure that is not a refusal (no network, a timeout, a server's error, a download that ended early) is not remembered | a refused 42 MB asset was otherwise downloaded again at every start and logged each time |
//! | A swap that fails | when every step was reversed: the waiting download stays and is tried again at the next start, the sentence goes to `update.log` and [`state`] shows it, and this process does not take that version again; when a reversal failed too: fatal, naming the renames to make by hand | the old install stays whole, or the person is told it is not |
//! | A new program that cannot be started | the previous version is put back ([`reverse`]: one exchange back, the folder synced; what is left of the new version is logged, not fatal), the sentence goes to `update.log`, the app starts as it is, and the version is kept in `.<name>.update.held`, so no later start fetches or swaps it until the feed offers another | the previous version is kept until the new core has run, and a version that cannot start would otherwise cost a download, a swap and a reversal at every second start |
//! | Who may swap | the start swaps only while it holds the core's one-instance lock on the data folder (taken through the closure `main.rs` passes), the install's gate `.<name>.gate`, and the install's presence file `.<name>.lock` exclusively, taken without waiting. Every process of the install (the app, each headless run) holds the presence file shared for its whole life; it takes that hold while holding the gate, and only a gate holder ever tries the presence file exclusively, so no swap comes between a start's decision and its hold. Both files are opened read-only and made (0644) where missing. Where a file cannot be made or opened, or the file system refuses the lock, no hold is possible: no swap, no background check, [`state`] says why ([`UpdateState::Unable`]), and the app and every headless family run as they are | a swap must not replace the payload under anything running from it, a read-only folder or a file system without locks must not stop the app or its engine children, and the core's own second-instance behaviour stays as it is |
//! | A panic on the updater's threads | the updater's panic hook, installed once by [`at_start`] on the main thread after the crash hook and before the core's and the engine's, writes it to `update.log` and goes no further; the updater's thread catches it and the next check is in 24 hours; every thread of the check's runtime is marked when it starts | the crash hook ends the process on a panic on a thread it does not know (`mcbench_lite::core::threads::handled`), and two hooks installed from unordered threads can lose each other |
//! | Leftovers | `<name>.old`, the unpack folder and `.update.json.part` are removed by the updater's thread once the core started; a failure goes to `update.log` and the app goes on | the previous version is what a new program that cannot start goes back to; on Windows the previous process may still be exiting, so its `.old` executable is removed at a later start |
//! | Windows swap | `std::fs::rename` of each entry aside, not `self-replace` 1.5.1 | `self-replace` acts on `current_exe()` only (no paths to test on scratch folders), copies rather than renames, and deletes the old file by spawning a copy of itself with `FILE_FLAG_DELETE_ON_CLOSE`; the Windows VM proved the plain rename |
//! | `install-source` across a swap | the installed file is copied into the unpacked payload before it is swapped in (macOS, Linux); on Windows it is outside the archive's entries and stays; a Windows archive that names `install-source` or `uninstall.exe` is refused | the archive is the same for every install of a target, so the route is the install's, never the archive's |
//! | Windows' installed-apps list | once a program's core runs (on the updater's thread, where `.old` is removed), `DisplayVersion` under `HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Uninstall\mcbench-lite` is set to that program's version when the key's `InstallLocation` names its install folder and the value differs, through `windows-sys` (what is read and written is plain Rust tested everywhere); a missing key or value is left alone, and a failure goes to `update.log` | the installer wrote the old version there and nothing else updates it; written by the new program itself, a reversed swap leaves nothing to undo; a second copy must not rewrite the installed one's entry; a `reg.exe` child would flash a console window from a windowed app |
//! | A payload that is not this program's | every payload carries a `version` file beside `install-source` (`Contents/Resources/version` on macOS), one line `MAJOR.MINOR.PATCH`, written by the packaging step; on every start of a self-updating install, once the hold is taken, the process compares its own version with it, and a program loaded from a payload a swap moved aside (`<payload>.old`, or inside `.<payload>.update.d`; Linux's `current_exe()` follows the rename) belongs to the install's real path ([`moved_from`]). When the payload is not this program's, the app executes the installed program once ([`RESTART_MARKER`], an argument `main` strips with [`take_restart_marker`], makes a second time a fatal sentence) and a headless run prints [`CHANGED_WHILE_STARTING`] and exits with status 2. A missing `version` is fatal at the app's start and no hold for a headless run, as a missing `install-source` is; the swap refuses a download whose unpacked `version` is not the release's. The AppImage needs none of it: a process runs wholly from the image it mounted | a process that starts while another start swaps is the old program on a new payload, and its engine and build children would start from the new version; the running image's own version is exact on every system, and nothing of the updater goes into the environment of the processes the app starts |
//! | The error screen's hold | the screen's process holds the install shared, quietly ([`hold_install_quietly`]): where the payload is not its own, or its `version` cannot be read, it runs with no hold and goes on | it starts no core and no child from the payload, so a payload that is not its own cannot hurt it, and a process with no terminal that exited with status 2 would leave the person with no error screen; the hold keeps the rule that no swap happens under a living process of the install, so the app that `Restart` starts puts a waiting swap off to the following start |
//! | A file system without the exchange | the folder or the AppImage is swapped by two renames (installed to `<name>.old`, new in), kept as a second path with its own failure tests; the exchange is refused on `EINVAL`, `ENOSYS`, `ENOTSUP` and `EOPNOTSUPP`, all four on purpose (Linux answers `EINVAL` for a file system without `RENAME_EXCHANGE` and `ENOSYS` without `renameat2`; macOS answers `ENOTSUP` from `renamex_np` and some file systems give `EOPNOTSUPP`, which differs from `ENOTSUP` on macOS) | refusing the swap there would leave installs on HFS+, exFAT, NFS and many FUSE mounts without updates; the price is the gap between the two renames, where a power loss leaves nothing at the install path and running the installer again repairs it |
//! | Version form | `MAJOR.MINOR.PATCH`, three decimal numbers | the workspace's versions; anything else is refused |
//! | The check's timing | at start and 24 hours of wall-clock time after each, on one thread of the updater's own | one small request a day; a computer that slept checks when it wakes; nothing else of the app waits on it |
//!
//! ## Open
//!
//! Where Settings shows [`state`] (stage four). There is no automatic
//! rollback when a new version starts and then crashes: the previous
//! version is removed once the new core has started. The presence file can
//! be deleted while the app runs (an uninstall, a person); the next start
//! then makes a new one whose exclusive try succeeds, and the swap is no
//! longer kept from running processes (accepted, a corner case). The Windows paths
//! pass clippy for `x86_64-pc-windows-msvc` and were tested on scratch
//! folders on Linux and macOS only: that a running `mcbench-lite.exe` and
//! its folder's other entries rename aside and back, that spawn-and-exit
//! starts the new program, that the registry calls read and write the
//! installer's entry, and that the previous `.old` executable is removed
//! at a later start are shown only by a run on Windows. The AppImage's
//! `$APPIMAGE` re-execution is shown only by a real AppImage, and
//! re-executing a GUI process the system launched only by a real macOS
//! start. The desktop entry and icon `install.sh` places outside the Linux
//! payload are not updated by a swap.

mod apps_entry;
mod flow;
mod hold;
mod latest;
mod swap;

#[cfg(feature = "feed")]
pub mod feed;

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub use flow::{
    CHANGED_WHILE_STARTING, Feed, Held, LOG_FILE, NotThisProgram, Start, THREAD_NAME, Updater,
    at_start, check_once, hold_install, hold_install_at, hold_install_for, hold_install_quietly,
    hold_install_quietly_for, installed_program, installed_program_for, read_held, restart_failed,
    start_with, state,
};
pub use hold::{InstallHold, NoHold};
pub use latest::{
    Downloaded, MAX_ASSET, Release, STALL, UpdateKey, check, download, verify_waiting,
};
pub use swap::{
    Failure, RESTART_MARKER, install, remove_leftover, restart, reverse, take_restart_marker,
};

/// The install route, from the `install-source` file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InstallSource {
    /// The apt repository: updates come with system updates.
    Apt,
    /// The dnf repository.
    Dnf,
    /// The AUR package `mcbench-lite-bin`.
    Aur,
    /// winget.
    Winget,
    /// The Microsoft Store (only if the owner asks for a listing).
    Store,
    /// The Windows installer (no `.dmg` is offered, so never on macOS).
    Direct,
    /// `install.sh`.
    Script,
    /// The AppImage.
    Appimage,
}

const SOURCES: [(&str, InstallSource); 8] = [
    ("apt", InstallSource::Apt),
    ("dnf", InstallSource::Dnf),
    ("aur", InstallSource::Aur),
    ("winget", InstallSource::Winget),
    ("store", InstallSource::Store),
    ("direct", InstallSource::Direct),
    ("script", InstallSource::Script),
    ("appimage", InstallSource::Appimage),
];

impl InstallSource {
    /// Whether the in-app updater applies (`direct`, `script`, `appimage`);
    /// for the others it stands down and Settings says where updates come
    /// from.
    pub fn updates_itself(self) -> bool {
        self.package_manager().is_none()
    }

    /// What Settings shows for a route a package manager updates, with the
    /// command to run; `None` for the routes the in-app updater serves.
    pub fn package_manager(self) -> Option<UpdateState> {
        let line = match self {
            InstallSource::Apt => "Updates come from apt: sudo apt update && sudo apt upgrade",
            InstallSource::Dnf => "Updates come from dnf: sudo dnf upgrade",
            InstallSource::Aur => "Updates come from the AUR: yay -Syu (or paru -Syu)",
            InstallSource::Winget => "Updates come from winget: winget upgrade mcbench-lite",
            InstallSource::Store => {
                "Updates come from the Microsoft Store: Library, then Get updates"
            }
            InstallSource::Direct | InstallSource::Script | InstallSource::Appimage => return None,
        };
        Some(UpdateState::PackageManager(line.to_string()))
    }
}

/// What Settings shows of updates ([`state`]).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum UpdateState {
    /// This build has no updater (it was built without an update key and
    /// feed).
    Off,
    /// Updates come from a package manager, with the command to run
    /// ("Updates come from apt: sudo apt update && sudo apt upgrade").
    PackageManager(String),
    /// The first check since the start has not finished.
    Checking,
    /// The app is up to date.
    Current,
    /// A newer version, with its version, is downloading.
    Downloading(String),
    /// A newer version, with its version, is downloaded and verified and
    /// starts the next time the app starts ("Version 1.2.0 starts next
    /// time").
    Ready(String),
    /// The last check or download failed, with its sentence (also in
    /// `update.log`); the next is in 24 hours or at the next start.
    Failed(String),
    /// The release on offer was refused (its `latest.json` or its asset is
    /// not what the key, the signed comment and the version allow), with
    /// the sentence of the first refusal; while `latest.json` stays the
    /// same, it is not fetched or logged again.
    Refused(String),
    /// This install cannot update itself, with the sentence saying why (no
    /// hold on the install is possible here); the app runs as it is.
    Unable(String),
}

/// The install route, read from `install-source` beside the running
/// executable ([`install_source_at`] with `std::env::current_exe()`).
pub fn install_source() -> Result<InstallSource, String> {
    install_source_at(&current_exe()?)
}

/// `std::env::current_exe()`, with symlinks resolved (Linux's
/// `bin/mcbench-lite` is a symlink into `lib/mcbench-lite/`, and
/// `current_exe()` already resolves it there; macOS's may not).
pub(crate) fn current_exe() -> Result<PathBuf, String> {
    std::env::current_exe()
        .and_then(std::fs::canonicalize)
        .map_err(|e| format!("The path of the running executable is unknown: {e}"))
}

/// Where the installer writes `install-source` for the executable `exe`:
/// `<bundle>/Contents/Resources/install-source` on macOS (`exe` is
/// `<bundle>/Contents/MacOS/<name>`), `<exe dir>/install-source` on Linux
/// and Windows.
pub fn install_source_path(exe: &Path) -> PathBuf {
    payload_file(exe, cfg!(target_os = "macos"), "install-source")
}

/// The payload's file `name` for the executable `exe`, in a bundle's
/// `Contents/Resources` when `bundle`, beside `exe` otherwise.
fn payload_file(exe: &Path, bundle: bool, name: &str) -> PathBuf {
    let dir = exe.parent().expect("an executable's path has a directory");
    if bundle {
        dir.with_file_name("Resources").join(name)
    } else {
        dir.join(name)
    }
}

/// Where the executable `exe` belongs when a swap moved what it was loaded
/// from (Linux's `current_exe()` follows a rename). Only the payload's own
/// position is looked at: its root (`<bundle>` of `<bundle>/Contents/MacOS/
/// <exe>` on macOS, the executable's folder elsewhere) named `<root>.old`
/// beside a `<root>`, or lying inside an unpack folder `.<root>.update.d`,
/// maps to the same path in `<root>`; the executable itself named
/// `<name>.old` beside a `<name>` (a Windows entry) maps to `<name>`.
/// `None` when `exe` is in its place, including inside an ancestor folder
/// that merely ends in `.old`.
pub fn moved_from(exe: &Path) -> Option<PathBuf> {
    moved_from_at(exe, if cfg!(target_os = "macos") { 3 } else { 1 })
}

/// [`moved_from`] with the payload root `depth` levels above `exe`.
fn moved_from_at(exe: &Path, depth: usize) -> Option<PathBuf> {
    let beside_without_old = |path: &Path| {
        let name = path.file_name()?.to_str()?;
        let real = path.with_file_name(name.strip_suffix(".old")?);
        std::fs::symlink_metadata(&real).is_ok().then_some(real)
    };
    if let Some(real) = beside_without_old(exe) {
        return Some(real);
    }
    let root = exe.ancestors().nth(depth)?;
    let rest = exe.strip_prefix(root).expect("an ancestor is a prefix");
    if let Some(real) = beside_without_old(root) {
        return Some(real.join(rest));
    }
    let unpack = root.parent()?;
    let payload = unpack
        .file_name()?
        .to_str()?
        .strip_prefix('.')?
        .strip_suffix(".update.d")?;
    Some(unpack.with_file_name(payload).join(rest))
}

/// The payload's `version` file, written by the packaging step: one line,
/// `MAJOR.MINOR.PATCH`, in the same place as `install-source`.
pub const VERSION_FILE: &str = "version";

/// The version in the `version` file at `path`, without its line ending.
pub(crate) fn read_version(path: &Path) -> Result<String, String> {
    let text = std::fs::read_to_string(path).map_err(|e| {
        format!(
            "The version file at {} cannot be read ({e}); every release carries it, so reinstall mcbench-lite.",
            path.display()
        )
    })?;
    Ok(text.trim_end_matches(['\n', '\r']).to_string())
}

/// The install route of the executable `exe`, from its `install-source`
/// file: one of the eight values, with one optional line ending.
pub fn install_source_at(exe: &Path) -> Result<InstallSource, String> {
    let path = install_source_path(exe);
    let text = std::fs::read_to_string(&path).map_err(|e| {
        format!(
            "The install-source file at {} cannot be read ({e}); the installer writes it, so reinstall mcbench-lite.",
            path.display()
        )
    })?;
    let value = text.strip_suffix('\n').unwrap_or(&text);
    let value = value.strip_suffix('\r').unwrap_or(value);
    SOURCES
        .iter()
        .find(|(name, _)| *name == value)
        .map(|(_, source)| *source)
        .ok_or_else(|| {
            format!(
                "The install-source file at {} holds {value:?}, which is none of apt, dnf, aur, winget, store, direct, script, appimage.",
                path.display()
            )
        })
}

/// The release asset an install takes: its key in `latest.json`'s `assets`
/// and its fixed file name in the release.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// The macOS arm64 bundle (`script`, the only macOS route).
    MacosAarch64,
    /// The macOS x86_64 bundle (`script`).
    MacosX86_64,
    /// The Linux x86_64 payload folder (`script`).
    LinuxX86_64,
    /// The Linux aarch64 payload folder (`script`).
    LinuxAarch64,
    /// The Linux x86_64 AppImage.
    LinuxX86_64Appimage,
    /// The Linux aarch64 AppImage.
    LinuxAarch64Appimage,
    /// The Windows x64 payload (`direct`).
    WindowsX86_64,
}

impl Target {
    /// Every target, in `latest.json`'s order of its `assets` keys.
    pub const ALL: [Target; 7] = [
        Target::LinuxAarch64,
        Target::LinuxAarch64Appimage,
        Target::LinuxX86_64,
        Target::LinuxX86_64Appimage,
        Target::MacosAarch64,
        Target::MacosX86_64,
        Target::WindowsX86_64,
    ];

    /// The key of this target in `latest.json`'s `assets` and in the
    /// signature's trusted comment.
    pub fn name(self) -> &'static str {
        match self {
            Target::MacosAarch64 => "macos-aarch64",
            Target::MacosX86_64 => "macos-x86_64",
            Target::LinuxX86_64 => "linux-x86_64",
            Target::LinuxAarch64 => "linux-aarch64",
            Target::LinuxX86_64Appimage => "linux-x86_64-appimage",
            Target::LinuxAarch64Appimage => "linux-aarch64-appimage",
            Target::WindowsX86_64 => "windows-x86_64",
        }
    }

    /// The release asset's fixed file name, so `releases/latest/download/`
    /// links work.
    pub fn asset(self) -> &'static str {
        match self {
            Target::MacosAarch64 => "mcbench-lite-macos-aarch64.tar.gz",
            Target::MacosX86_64 => "mcbench-lite-macos-x86_64.tar.gz",
            Target::LinuxX86_64 => "mcbench-lite-linux-x86_64.tar.gz",
            Target::LinuxAarch64 => "mcbench-lite-linux-aarch64.tar.gz",
            Target::LinuxX86_64Appimage => "mcbench-lite-x86_64.AppImage",
            Target::LinuxAarch64Appimage => "mcbench-lite-aarch64.AppImage",
            Target::WindowsX86_64 => "mcbench-lite-windows-x86_64.tar.gz",
        }
    }

    /// The target named `name` ([`Target::name`]).
    pub fn from_name(name: &str) -> Result<Target, String> {
        Target::ALL
            .into_iter()
            .find(|t| t.name() == name)
            .ok_or_else(|| {
                format!(
                    "{name:?} is not a target; the targets are {}.",
                    Target::ALL.map(Target::name).join(", ")
                )
            })
    }

    /// The target of this build installed by `source`; an error for the
    /// package-manager routes and for a route this platform has not.
    pub fn of_this_build(source: InstallSource) -> Result<Target, String> {
        let (os, arch) = (std::env::consts::OS, std::env::consts::ARCH);
        Target::of(source, os, arch).ok_or_else(|| {
            format!(
                "The install route {source:?} is not one this build updates itself on {os} {arch}."
            )
        })
    }

    /// The target installed by `source` on the system `os` and the
    /// architecture `arch` (`std::env::consts`' names): each pair named
    /// exactly, `None` for any other.
    fn of(source: InstallSource, os: &str, arch: &str) -> Option<Target> {
        use InstallSource::{Appimage, Direct, Script};
        Some(match (source, os, arch) {
            (Script, "macos", "aarch64") => Target::MacosAarch64,
            (Script, "macos", "x86_64") => Target::MacosX86_64,
            (Script, "linux", "aarch64") => Target::LinuxAarch64,
            (Script, "linux", "x86_64") => Target::LinuxX86_64,
            (Appimage, "linux", "aarch64") => Target::LinuxAarch64Appimage,
            (Appimage, "linux", "x86_64") => Target::LinuxX86_64Appimage,
            (Direct, "windows", "x86_64") => Target::WindowsX86_64,
            _ => return None,
        })
    }
}

/// What an update replaces and what [`restart`] runs afterwards.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Installed {
    /// The release asset this install takes.
    pub target: Target,
    /// The payload the swap replaces: the `.app` bundle, the executable's
    /// folder (Linux `script`, Windows), or the AppImage file. The waiting
    /// download and the unpack folder are beside it.
    pub path: PathBuf,
    /// What [`restart`] runs: the executable, or the AppImage file.
    pub program: PathBuf,
}

impl Installed {
    /// The install's presence file, `.<name>.lock` beside the payload,
    /// held shared by every process of the install ([`InstallHold`]).
    pub fn lock_file(&self) -> PathBuf {
        hold::presence_file(self)
    }

    /// Every file and folder the updater can make beside the payload, for
    /// the uninstallers: the waiting download `.<name>.update`, the
    /// `latest.json` it was checked against `.<name>.update.json` and that
    /// file while it is written `.<name>.update.json.part`, the unpack
    /// folder `.<name>.update.d`, the held version `.<name>.update.held`,
    /// the refused release `.<name>.update.refused`,
    /// the presence file `.<name>.lock`, the gate file `.<name>.gate`, and
    /// the previous version `<name>.old` (beside a folder payload or the
    /// AppImage; on Windows it is `<entry>.old` inside the payload folder,
    /// for each entry an update replaced).
    pub fn updater_files(&self) -> Vec<PathBuf> {
        let mut files = [
            ".update",
            ".update.json",
            ".update.json.part",
            ".update.d",
            ".update.held",
            ".update.refused",
            ".lock",
            ".gate",
        ]
        .map(|suffix| swap::beside(&self.path, suffix))
        .to_vec();
        if self.target != Target::WindowsX86_64 {
            files.push(swap::old_path(&self.path));
        }
        files
    }

    /// This install's `install-source` file: [`install_source_path`] by
    /// the target's layout rather than the running system's, so a swap of
    /// any target finds it wherever its tests run.
    pub fn install_source(&self) -> PathBuf {
        payload_file(&self.program, self.is_bundle(), "install-source")
    }

    /// This install's `version` file ([`VERSION_FILE`]), by the target's
    /// layout. `None` for the AppImage: its payload is inside the image, and
    /// a process runs wholly from the image it mounted, so no version mix
    /// can arise there.
    pub fn version_file(&self) -> Option<PathBuf> {
        (!matches!(
            self.target,
            Target::LinuxX86_64Appimage | Target::LinuxAarch64Appimage
        ))
        .then(|| payload_file(&self.program, self.is_bundle(), VERSION_FILE))
    }

    /// The installed payload's version, from its `version` file; `None`
    /// for the AppImage.
    pub fn installed_version(&self) -> Result<Option<String>, String> {
        self.version_file().map(|f| read_version(&f)).transpose()
    }

    fn is_bundle(&self) -> bool {
        matches!(self.target, Target::MacosAarch64 | Target::MacosX86_64)
    }

    /// The install of the executable `exe` (`std::env::current_exe()`) for
    /// `target`. `appimage` is the `APPIMAGE` environment variable, which the
    /// AppImage runtime sets to the AppImage file's path (the executable
    /// itself runs from the image's mount).
    pub fn locate(
        exe: &Path,
        target: Target,
        appimage: Option<PathBuf>,
    ) -> Result<Installed, String> {
        match target {
            Target::MacosAarch64 | Target::MacosX86_64 => {
                let bundle = exe
                    .ancestors()
                    .nth(3)
                    .filter(|b| b.extension().is_some_and(|e| e == "app"))
                    .ok_or_else(|| {
                        format!(
                            "{} is not inside a .app bundle (<bundle>/Contents/MacOS/<name>).",
                            exe.display()
                        )
                    })?;
                Ok(Installed {
                    target,
                    path: bundle.to_path_buf(),
                    program: exe.to_path_buf(),
                })
            }
            Target::LinuxX86_64Appimage | Target::LinuxAarch64Appimage => {
                let file = appimage.ok_or_else(|| {
                    "APPIMAGE is not set, so the AppImage file to replace is unknown.".to_string()
                })?;
                Ok(Installed {
                    target,
                    path: file.clone(),
                    program: file,
                })
            }
            Target::LinuxX86_64 | Target::LinuxAarch64 | Target::WindowsX86_64 => Ok(Installed {
                target,
                path: exe
                    .parent()
                    .expect("an executable's path has a directory")
                    .to_path_buf(),
                program: exe.to_path_buf(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exe_in(root: &Path) -> PathBuf {
        if cfg!(target_os = "macos") {
            root.join("mcbench-lite.app/Contents/MacOS/mcbench-lite")
        } else if cfg!(windows) {
            root.join("mcbench-lite").join("mcbench-lite.exe")
        } else {
            root.join("lib/mcbench-lite/mcbench-lite")
        }
    }

    /// F2: a program loaded just before a swap finds itself in the moved
    /// payload; its place is the install's real path.
    #[test]
    fn a_moved_program_belongs_to_the_installs_real_path() {
        let dir = tempfile::tempdir().unwrap();
        let lib = dir.path().join("lib");
        std::fs::create_dir_all(lib.join("mcbench-lite")).unwrap();
        std::fs::create_dir_all(lib.join("mcbench-lite.old")).unwrap();
        assert_eq!(
            moved_from_at(&lib.join("mcbench-lite.old/mcbench-lite"), 1),
            Some(lib.join("mcbench-lite/mcbench-lite"))
        );
        assert_eq!(
            moved_from_at(
                &lib.join(".mcbench-lite.update.d/mcbench-lite/mcbench-lite"),
                1
            ),
            Some(lib.join("mcbench-lite/mcbench-lite"))
        );
        let a = dir.path().join("A");
        std::fs::create_dir_all(a.join("mcbench-lite.app")).unwrap();
        std::fs::create_dir_all(a.join("mcbench-lite.app.old")).unwrap();
        assert_eq!(
            moved_from_at(
                &a.join(".mcbench-lite.app.update.d/mcbench-lite.app/Contents/MacOS/mcbench-lite"),
                3
            ),
            Some(a.join("mcbench-lite.app/Contents/MacOS/mcbench-lite"))
        );
        assert_eq!(
            moved_from_at(
                &a.join("mcbench-lite.app.old/Contents/MacOS/mcbench-lite"),
                3
            ),
            Some(a.join("mcbench-lite.app/Contents/MacOS/mcbench-lite"))
        );
        // Windows: the program itself renamed aside; the real path is the
        // entry's own, with no separator after it.
        let programs = dir.path().join("Programs/mcbench-lite");
        std::fs::create_dir_all(&programs).unwrap();
        std::fs::write(programs.join("mcbench-lite.exe"), "").unwrap();
        let real = moved_from_at(&programs.join("mcbench-lite.exe.old"), 1).unwrap();
        assert_eq!(real, programs.join("mcbench-lite.exe"));
        assert!(!real.as_os_str().to_string_lossy().ends_with(['/', '\\']));
        assert_eq!(
            moved_from_at(&lib.join("mcbench-lite/mcbench-lite"), 1),
            None
        );
        // An .old with nothing of its name beside it is no swap's.
        assert_eq!(moved_from_at(&lib.join("x.old/mcbench-lite"), 1), None);
        // An ancestor further up that ends in .old, with its name beside it,
        // is not the payload's position.
        let apps = dir
            .path()
            .join("Applications.old/mcbench-lite.app/Contents/MacOS");
        std::fs::create_dir_all(&apps).unwrap();
        std::fs::create_dir_all(dir.path().join("Applications")).unwrap();
        assert_eq!(moved_from_at(&apps.join("mcbench-lite"), 3), None);
        let deep = dir.path().join("Applications.old/x/mcbench-lite");
        assert_eq!(moved_from_at(&deep.join("mcbench-lite"), 1), None);
    }

    #[test]
    fn install_source_path_per_platform() {
        let root = Path::new("/x");
        let want = if cfg!(target_os = "macos") {
            "/x/mcbench-lite.app/Contents/Resources/install-source"
        } else if cfg!(windows) {
            "/x/mcbench-lite\\install-source"
        } else {
            "/x/lib/mcbench-lite/install-source"
        };
        assert_eq!(install_source_path(&exe_in(root)), Path::new(want));
    }

    /// Linux's `bin/mcbench-lite` is a symlink into `lib/mcbench-lite/`;
    /// the resolved path finds `install-source` beside the real file.
    #[cfg(target_os = "linux")]
    #[test]
    fn the_linux_symlink_resolves_into_the_payload_folder() {
        let dir = tempfile::tempdir().unwrap();
        let exe = exe_in(dir.path());
        std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
        std::fs::write(&exe, "").unwrap();
        std::fs::write(install_source_path(&exe), "script\n").unwrap();
        std::fs::create_dir(dir.path().join("bin")).unwrap();
        let link = dir.path().join("bin/mcbench-lite");
        std::os::unix::fs::symlink("../lib/mcbench-lite/mcbench-lite", &link).unwrap();
        let resolved = std::fs::canonicalize(&link).unwrap();
        assert_eq!(install_source_at(&resolved), Ok(InstallSource::Script));
    }

    #[test]
    fn reads_each_of_the_eight_values() {
        let dir = tempfile::tempdir().unwrap();
        let exe = exe_in(dir.path());
        let file = install_source_path(&exe);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        for (name, source) in SOURCES {
            for ending in ["", "\n", "\r\n"] {
                std::fs::write(&file, format!("{name}{ending}")).unwrap();
                assert_eq!(install_source_at(&exe), Ok(source));
            }
            assert_eq!(
                source.updates_itself(),
                matches!(name, "direct" | "script" | "appimage")
            );
        }
    }

    #[test]
    fn missing_and_unknown_files_are_errors_naming_the_path() {
        let dir = tempfile::tempdir().unwrap();
        let exe = exe_in(dir.path());
        let file = install_source_path(&exe);
        let missing = install_source_at(&exe).unwrap_err();
        assert!(
            missing.starts_with(&format!(
                "The install-source file at {} cannot be read (",
                file.display()
            )),
            "{missing}"
        );
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "brew\n").unwrap();
        assert_eq!(
            install_source_at(&exe).unwrap_err(),
            format!(
                "The install-source file at {} holds \"brew\", which is none of apt, dnf, aur, winget, store, direct, script, appimage.",
                file.display()
            )
        );
        std::fs::write(&file, "apt\n\n").unwrap();
        assert!(
            install_source_at(&exe)
                .unwrap_err()
                .contains("holds \"apt\\n\"")
        );
    }

    #[test]
    fn settings_lines_of_the_package_manager_routes() {
        let lines: Vec<_> = SOURCES
            .iter()
            .filter_map(|(_, s)| s.package_manager())
            .collect();
        assert_eq!(
            lines,
            [
                "Updates come from apt: sudo apt update && sudo apt upgrade",
                "Updates come from dnf: sudo dnf upgrade",
                "Updates come from the AUR: yay -Syu (or paru -Syu)",
                "Updates come from winget: winget upgrade mcbench-lite",
                "Updates come from the Microsoft Store: Library, then Get updates",
            ]
            .map(|l| UpdateState::PackageManager(l.to_string()))
        );
    }

    #[test]
    fn the_asset_table() {
        let table: Vec<_> = Target::ALL.map(|t| (t.name(), t.asset())).to_vec();
        assert_eq!(
            table,
            [
                ("linux-aarch64", "mcbench-lite-linux-aarch64.tar.gz"),
                ("linux-aarch64-appimage", "mcbench-lite-aarch64.AppImage"),
                ("linux-x86_64", "mcbench-lite-linux-x86_64.tar.gz"),
                ("linux-x86_64-appimage", "mcbench-lite-x86_64.AppImage"),
                ("macos-aarch64", "mcbench-lite-macos-aarch64.tar.gz"),
                ("macos-x86_64", "mcbench-lite-macos-x86_64.tar.gz"),
                ("windows-x86_64", "mcbench-lite-windows-x86_64.tar.gz"),
            ]
        );
        for t in Target::ALL {
            assert_eq!(Target::from_name(t.name()), Ok(t));
        }
        assert_eq!(
            Target::from_name("linux").unwrap_err(),
            "\"linux\" is not a target; the targets are linux-aarch64, linux-aarch64-appimage, linux-x86_64, linux-x86_64-appimage, macos-aarch64, macos-x86_64, windows-x86_64."
        );
    }

    #[test]
    fn locate_per_target() {
        let bundle_exe = Path::new("/A/mcbench-lite.app/Contents/MacOS/mcbench-lite");
        let mac = Installed::locate(bundle_exe, Target::MacosAarch64, None).unwrap();
        assert_eq!(mac.path, Path::new("/A/mcbench-lite.app"));
        assert_eq!(mac.program, bundle_exe);
        let script = Installed::locate(
            Path::new("/h/.local/lib/mcbench-lite/mcbench-lite"),
            Target::LinuxX86_64,
            None,
        )
        .unwrap();
        assert_eq!(script.path, Path::new("/h/.local/lib/mcbench-lite"));
        assert_eq!(
            script.program,
            Path::new("/h/.local/lib/mcbench-lite/mcbench-lite")
        );
        let windows = Installed::locate(
            Path::new("/p/mcbench-lite/mcbench-lite.exe"),
            Target::WindowsX86_64,
            None,
        )
        .unwrap();
        assert_eq!(windows.path, Path::new("/p/mcbench-lite"));
        assert_eq!(
            Installed::locate(
                Path::new("/usr/bin/mcbench-lite"),
                Target::MacosX86_64,
                None
            )
            .unwrap_err(),
            "/usr/bin/mcbench-lite is not inside a .app bundle (<bundle>/Contents/MacOS/<name>)."
        );
        let image = Installed::locate(
            Path::new("/tmp/.mount_x/usr/bin/mcbench-lite"),
            Target::LinuxX86_64Appimage,
            Some(PathBuf::from("/h/mcbench-lite.AppImage")),
        )
        .unwrap();
        assert_eq!(image.path, Path::new("/h/mcbench-lite.AppImage"));
        assert_eq!(image.program, Path::new("/h/mcbench-lite.AppImage"));
        assert_eq!(
            Installed::locate(Path::new("/x"), Target::LinuxAarch64Appimage, None).unwrap_err(),
            "APPIMAGE is not set, so the AppImage file to replace is unknown."
        );
    }

    #[test]
    fn this_builds_routes() {
        let mac = cfg!(target_os = "macos");
        let windows = cfg!(windows);
        assert_eq!(
            Target::of_this_build(InstallSource::Direct).is_ok(),
            windows && cfg!(target_arch = "x86_64"),
            "direct is the Windows installer's route, on x86_64 only"
        );
        assert_eq!(
            Target::of_this_build(InstallSource::Script).is_ok(),
            !windows
        );
        assert_eq!(
            Target::of_this_build(InstallSource::Appimage).is_ok(),
            !mac && !windows
        );
        if mac {
            assert_eq!(
                Target::of_this_build(InstallSource::Script),
                Ok(if cfg!(target_arch = "aarch64") {
                    Target::MacosAarch64
                } else {
                    Target::MacosX86_64
                })
            );
        }
    }

    /// The names the uninstallers remove, the lock file among them.
    #[test]
    fn the_updaters_files_beside_each_payload() {
        let names = |exe: &Path, target, appimage: Option<PathBuf>| {
            Installed::locate(exe, target, appimage)
                .unwrap()
                .updater_files()
        };
        let joined = |dir: &Path, all: &[&str]| all.iter().map(|n| dir.join(n)).collect::<Vec<_>>();
        let lib = Path::new("/h").join(".local").join("lib");
        assert_eq!(
            names(
                &lib.join("mcbench-lite").join("mcbench-lite"),
                Target::LinuxX86_64,
                None
            ),
            joined(
                &lib,
                &[
                    ".mcbench-lite.update",
                    ".mcbench-lite.update.json",
                    ".mcbench-lite.update.json.part",
                    ".mcbench-lite.update.d",
                    ".mcbench-lite.update.held",
                    ".mcbench-lite.update.refused",
                    ".mcbench-lite.lock",
                    ".mcbench-lite.gate",
                    "mcbench-lite.old",
                ]
            )
        );
        let a = Path::new("/A");
        let bundle_exe = a
            .join("mcbench-lite.app")
            .join("Contents")
            .join("MacOS")
            .join("mcbench-lite");
        assert_eq!(
            names(&bundle_exe, Target::MacosAarch64, None)[6..8],
            joined(a, &[".mcbench-lite.app.lock", ".mcbench-lite.app.gate"])
        );
        let d = Path::new("/d");
        assert_eq!(
            names(
                Path::new("/m/usr/bin/mcbench-lite"),
                Target::LinuxAarch64Appimage,
                Some(d.join("mcbench-lite.AppImage"))
            )[6..],
            joined(
                d,
                &[
                    ".mcbench-lite.AppImage.lock",
                    ".mcbench-lite.AppImage.gate",
                    "mcbench-lite.AppImage.old"
                ]
            )
        );
        let programs = Path::new("/P");
        let windows = names(
            &programs.join("mcbench-lite").join("mcbench-lite.exe"),
            Target::WindowsX86_64,
            None,
        );
        assert_eq!(windows.len(), 8);
        assert_eq!(
            windows[6..],
            joined(programs, &[".mcbench-lite.lock", ".mcbench-lite.gate"])
        );
    }

    /// Each system and architecture is named exactly; any other is refused.
    #[test]
    fn the_target_of_each_system_and_architecture() {
        use InstallSource::{Appimage, Direct, Script};
        let cases = [
            (Script, "macos", "aarch64", Some(Target::MacosAarch64)),
            (Script, "macos", "x86_64", Some(Target::MacosX86_64)),
            (Script, "linux", "aarch64", Some(Target::LinuxAarch64)),
            (Script, "linux", "x86_64", Some(Target::LinuxX86_64)),
            (
                Appimage,
                "linux",
                "aarch64",
                Some(Target::LinuxAarch64Appimage),
            ),
            (
                Appimage,
                "linux",
                "x86_64",
                Some(Target::LinuxX86_64Appimage),
            ),
            (Direct, "windows", "x86_64", Some(Target::WindowsX86_64)),
            (Direct, "windows", "aarch64", None),
            (Script, "linux", "riscv64", None),
            (Appimage, "linux", "arm", None),
            (Script, "macos", "x86", None),
            (Direct, "macos", "aarch64", None),
            (Appimage, "macos", "aarch64", None),
            (Script, "windows", "x86_64", None),
            (Script, "freebsd", "x86_64", None),
        ];
        for (source, os, arch, want) in cases {
            assert_eq!(Target::of(source, os, arch), want, "{source:?} {os} {arch}");
        }
    }

    #[test]
    fn this_build_refuses_package_manager_routes() {
        for source in [
            InstallSource::Apt,
            InstallSource::Dnf,
            InstallSource::Aur,
            InstallSource::Winget,
            InstallSource::Store,
        ] {
            assert!(Target::of_this_build(source).is_err());
        }
    }
}
