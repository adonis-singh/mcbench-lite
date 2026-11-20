//! # mcbench-spawn
//!
//! The one place a child process of the app is made. [`command`] is
//! `std::process::Command::new` everywhere; on Windows it also sets
//! `CREATE_NO_WINDOW`, so a console program the app starts (the engine's
//! child, `tar`, a provider's CLI) gets a console of its own that has no
//! window, and its own children inherit that console, so none of them shows
//! a window either. `mcbench-lite.exe` is a window program on Windows and
//! has no console to hand down; without the flag Windows opens a console
//! window for each console program it starts. A tokio command is made from
//! it, `tokio::process::Command::from(command(program))`, which keeps the
//! flag.
//!
//! ## Seams
//!
//! | Seam | What crosses | Who calls whom | Thread or process |
//! |---|---|---|---|
//! | a child process | a program in; a `std::process::Command` out | every crate of the app that starts a process calls [`command`] | the caller's |
//!
//! ## Decided
//!
//! One flag on every spawn, not one call at the app's start:
//! `AllocConsoleWithOptions` with `ALLOC_CONSOLE_MODE_NO_WINDOW`, which
//! gives a process a console without a window that its children inherit,
//! exists from Windows 11 24H2 (build 26100), and the app supports Windows 11
//! 22H2 and Windows 10 1903 (docs/native-app.md, Supported hardware).
//! `mcbench-lite`'s test `spawns_show_no_console_window`, a source scan run
//! on every platform, fails when a crate of the app starts a process
//! without this function.

use std::ffi::OsStr;
use std::process::Command;

/// `CreateProcessW`'s flag for a console program run without a console
/// window (`CREATE_NO_WINDOW`, `WinBase.h`).
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// A command for `program` that shows no console window on Windows.
pub fn command(program: impl AsRef<OsStr>) -> Command {
    #[cfg_attr(not(windows), allow(unused_mut))]
    let mut command = Command::new(program);
    #[cfg(windows)]
    std::os::windows::process::CommandExt::creation_flags(&mut command, CREATE_NO_WINDOW);
    command
}
