//! What happens when the process fails: a crash log, and a quiet exit.
//!
//! [`install`] runs first thing in [`crate::run`] and in the dev binary. A
//! panic writes `crash-<seconds>-<pid>.log` (message, location, backtrace,
//! version) into the logs folder and leaves with `_exit(101)`. The hook runs
//! before any unwinding, which matters on macOS: a panic inside an AppKit
//! draw callback cannot unwind through Objective-C frames and would abort,
//! and `abort` raises the system crash reporter. The process never aborts.
//!
//! A fatal signal on Unix (SIGABRT, SIGSEGV, SIGBUS, SIGILL, SIGFPE) writes
//! a record formatted at install time to `crash-signals.log`, opened at
//! install time, and leaves with `_exit(128 + signal)`; the handler calls
//! only `write` and `_exit`, which are async-signal-safe, and runs on its own
//! stack so a stack overflow still reaches it. On macOS an uncaught
//! Objective-C exception is recorded the same way. On Windows an unhandled
//! exception is recorded and the process ends with `TerminateProcess`, and
//! the error mode is set so Windows Error Reporting shows no dialog; the C
//! runtime's `abort()` (from C code) raises SIGABRT first, whose handler
//! records it and ends the process with status 3, the abort message and the
//! fault report switched off. A Rust `std::process::abort` on Windows is
//! `__fastfail`, which ends the process at once (status 0xC0000409): no
//! filter or handler sees it, and it leaves no record.

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

/// The exit status after a panic.
pub const PANIC_STATUS: i32 = 101;

static LOGS: OnceLock<PathBuf> = OnceLock::new();

// pane agent: the process's start for the window's `first_frame` line.
static STARTED: OnceLock<std::time::Instant> = OnceLock::new();

/// The instant the process started, as the window counts it: taken first
/// thing in [`install`], which the binaries call first in `main` before
/// the core starts; in a process that never called [`install`], the first
/// call of this function (the window's start, later than the process's).
pub fn process_started() -> std::time::Instant {
    *STARTED.get_or_init(std::time::Instant::now)
}

/// Installs the panic hook and the fatal-signal and exception handlers.
/// `logs` is created if missing. Called once per process.
///
/// `handled` is asked first about every panic: when it returns true the
/// hook does nothing at all (no log, no exit, nothing printed) and the
/// unwind goes on to whoever catches it. The binary passes a function that
/// is true on the threads that catch panics on purpose (the engine's
/// refusals, the core's tasks); the dev binary passes `|_| false`.
pub fn install(
    logs: &Path,
    version: &'static str,
    handled: fn(&std::panic::PanicHookInfo) -> bool,
) {
    process_started();
    fs::create_dir_all(logs)
        .unwrap_or_else(|e| panic!("cannot create the logs folder {}: {e}", logs.display()));
    LOGS.set(logs.to_path_buf())
        .expect("crash::install is called once");
    std::panic::set_hook(Box::new(move |info| {
        if handled(info) {
            return;
        }
        let message = match info.payload().downcast_ref::<&str>() {
            Some(text) => (*text).to_string(),
            None => match info.payload().downcast_ref::<String>() {
                Some(text) => text.clone(),
                None => "a panic with a payload that is not text".to_string(),
            },
        };
        let location = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "an unknown location".to_string());
        let thread = std::thread::current();
        let record = format!(
            "mcbench-lite {version} panicked\nthread: {}\nmessage: {message}\nlocation: {location}\npid: {}\n\nbacktrace:\n{:?}\n",
            thread.name().unwrap_or("unnamed"),
            std::process::id(),
            backtrace::Backtrace::new(),
        );
        let path = crash_path();
        // A crash log that cannot be written still ends the process; stderr
        // carries the record then.
        let written = fs::write(&path, &record).is_ok();
        let _ = writeln!(
            std::io::stderr(),
            "{record}{}",
            if written {
                format!("crash log: {}", path.display())
            } else {
                format!("the crash log {} could not be written", path.display())
            }
        );
        // par-shell: a binary that asked for it shows the error screen in a
        // new process (see `relaunch_on_panic`); a failed start exits as
        // before.
        if RELAUNCH.load(std::sync::atomic::Ordering::SeqCst) {
            relaunch(&message);
        }
        quit_now(PANIC_STATUS);
    }));
    platform::install(&logs.join("crash-signals.log"), version);
}

/// Whether [`install`] ran.
pub fn installed() -> bool {
    LOGS.get().is_some()
}

fn crash_path() -> PathBuf {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    LOGS.get()
        .expect("crash::install ran")
        .join(format!("crash-{seconds}-{}.log", std::process::id()))
}

/// Ends the process at once with `status`, running no destructors and no
/// `atexit` handlers (`_exit`, `TerminateProcess` on Windows).
pub fn quit_now(status: i32) -> ! {
    platform::exit(status)
}

// par-shell (parity pass, `app.md` 0, 12.1, 12.2): every fatal ends in the
// error screen. A panic cannot be shown in the process it happened in: on
// macOS GPUI draws inside `extern "C"` AppKit callbacks, where an unwind
// aborts, and a panic in the middle of an update leaves GPUI's own state
// half-borrowed. So the hook (above) starts this same program again, with
// the message in `MCBENCH_FATAL`, and that process shows the error screen
// before it does anything else.
//
// No loop can follow, by construction: a process relaunches only after its
// `main` called `relaunch_on_panic`, which arms the relaunch only when
// `MCBENCH_FATAL` is unset; the relaunched process has it set, so it never
// arms, and a panic in the error screen logs and exits 101 like any process
// that did not ask (the crate's tests, the stand-in, the build child). The
// restart (`Restart`) starts the program with the variable removed.

/// The variable that carries the message to the error screen's process.
pub const FATAL_VARIABLE: &str = "MCBENCH_FATAL";
/// The variable that carries the ended window's place and theme
/// (`x,y,width,height,light`).
pub const WINDOW_VARIABLE: &str = "MCBENCH_FATAL_WINDOW";
/// The most of a panic's message the error screen is given, in bytes (cut
/// at a character's edge, then `…`); the whole record is in the crash log.
pub const MESSAGE_LIMIT: usize = 2000;

static RELAUNCH: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static ARGS: OnceLock<Vec<std::ffi::OsString>> = OnceLock::new();
static PROGRAM: OnceLock<std::path::PathBuf> = OnceLock::new();
static WINDOW: std::sync::Mutex<Option<[f32; 4]>> = std::sync::Mutex::new(None);

/// The error screen a relaunched process shows.
#[derive(Clone, Debug, PartialEq)]
pub struct ErrorScreen {
    /// The panic's message, at most [`MESSAGE_LIMIT`] bytes.
    pub message: String,
    /// The ended window's bounds (`x, y, width, height`), when it had drawn.
    pub bounds: Option<[f32; 4]>,
    /// The ended window was light.
    pub light: bool,
}

impl ErrorScreen {
    /// Puts the ended window's place and theme on `options`.
    pub fn apply(&self, options: &mut crate::Options) {
        if let Some([x, y, width, height]) = self.bounds {
            options.origin = Some((x, y));
            options.size = (width, height);
            options.size_given = true;
        }
        crate::view::theme::set_mode(if self.light {
            crate::view::theme::Mode::Light
        } else {
            crate::view::theme::Mode::Dark
        });
    }
}

/// Opt-in, first thing in a binary's `main` after [`install`]: when this
/// process was started to show an error screen, returns it (the caller
/// shows it with [`crate::fatal`] before doing anything else); otherwise an
/// unhandled panic from now starts this program again, with `args`, into
/// the error screen. Never relaunches without this call.
///
/// `args` are the arguments both the error screen's process and the app
/// that `Restart` starts are given; the library reads none itself. The
/// caller leaves out any that a launcher added for one start only (the
/// updater's restart marker): repeated, it would tell the next start
/// something that is not true of it.
///
/// `restart_program` is what `Restart` starts. The relaunch into the error
/// screen starts `current_exe()` (the panicking app holds its install, so
/// its own path is good then); `Restart` comes later, from the screen's
/// process, after which an update may have swapped the payload (on Linux
/// `current_exe()` then names the renamed old one): the caller names the
/// program that stays.
pub fn relaunch_on_panic(
    restart_program: std::path::PathBuf,
    args: Vec<std::ffi::OsString>,
) -> Option<ErrorScreen> {
    assert!(installed(), "crash::install runs before relaunch_on_panic");
    ARGS.set(args).expect("relaunch_on_panic is called once");
    PROGRAM
        .set(restart_program)
        .expect("relaunch_on_panic is called once");
    let Ok(message) = std::env::var(FATAL_VARIABLE) else {
        RELAUNCH.store(true, std::sync::atomic::Ordering::SeqCst);
        return None;
    };
    let window = std::env::var(WINDOW_VARIABLE).unwrap_or_default();
    let parts: Vec<&str> = window.split(',').collect();
    let number = |i: usize| -> f32 {
        parts[i]
            .parse()
            .unwrap_or_else(|_| panic!("{WINDOW_VARIABLE} has {window:?}"))
    };
    let bounds =
        (parts.len() == 5 && parts[0] != "-").then(|| [number(0), number(1), number(2), number(3)]);
    Some(ErrorScreen {
        message,
        bounds,
        light: parts.last() == Some(&"light"),
    })
}

/// The window's bounds for a relaunch (the shell calls it as the window
/// opens, moves and resizes).
pub fn remember_window(bounds: [f32; 4]) {
    if let Ok(mut window) = WINDOW.lock() {
        *window = Some(bounds);
    }
}

/// `message` cut to [`MESSAGE_LIMIT`] bytes at a character's edge, with
/// `…` when cut; NUL (which no environment can hold) as U+FFFD.
pub fn bounded(message: &str) -> String {
    let message = message.replace('\0', "\u{fffd}");
    if message.len() <= MESSAGE_LIMIT {
        return message;
    }
    let mut end = MESSAGE_LIMIT;
    while !message.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\u{2026}", &message[..end])
}

/// Starts this program again into the error screen; a start that fails is
/// left (the process exits 101 as it would have).
fn relaunch(message: &str) {
    let window = WINDOW.try_lock().ok().and_then(|w| *w);
    let light = crate::view::theme::mode() == crate::view::theme::Mode::Light;
    let window = match window {
        Some([x, y, w, h]) => format!("{x},{y},{w},{h},{}", if light { "light" } else { "dark" }),
        None => format!("-,-,-,-,{}", if light { "light" } else { "dark" }),
    };
    let Ok(program) = std::env::current_exe() else {
        return;
    };
    let _ = std::process::Command::new(program)
        .args(ARGS.get().expect("armed by relaunch_on_panic"))
        .env(FATAL_VARIABLE, bounded(message))
        .env(WINDOW_VARIABLE, window)
        .spawn();
}

/// Starts the program `relaunch_on_panic` named, with its arguments and no
/// error screen (the error screen's `Restart`). The caller ends this
/// process when it started.
pub fn restart() -> std::io::Result<()> {
    let mut command = std::process::Command::new(
        PROGRAM
            .get()
            .expect("relaunch_on_panic ran in this program's main"),
    );
    command
        .args(
            ARGS.get()
                .expect("relaunch_on_panic ran in this program's main"),
        )
        .env_remove(FATAL_VARIABLE)
        .env_remove(WINDOW_VARIABLE);
    // A staging build's forced panic is for the process it was asked of:
    // the app `Restart` starts must not inherit it and panic again.
    #[cfg(feature = "proof-hooks")]
    command.env_remove(crate::proof::PANIC_AFTER);
    command.spawn().map(|_| ())
}

#[cfg(unix)]
mod platform {
    use std::ffi::CString;
    use std::path::Path;
    use std::sync::OnceLock;
    use std::sync::atomic::{AtomicI32, Ordering};

    const SIGNALS: [(libc::c_int, &str); 5] = [
        (libc::SIGABRT, "SIGABRT"),
        (libc::SIGSEGV, "SIGSEGV"),
        (libc::SIGBUS, "SIGBUS"),
        (libc::SIGILL, "SIGILL"),
        (libc::SIGFPE, "SIGFPE"),
    ];
    const RECORD: usize = 160;
    static FD: AtomicI32 = AtomicI32::new(-1);
    // One record per signal, formatted before any handler is installed, plus
    // one for an uncaught Objective-C exception; the handler only reads them.
    static RECORDS: OnceLock<[([u8; RECORD], usize); 6]> = OnceLock::new();

    pub(super) fn exit(status: i32) -> ! {
        unsafe { libc::_exit(status) }
    }

    pub(super) fn install(path: &Path, version: &str) {
        let c_path = CString::new(path.as_os_str().as_encoded_bytes())
            .expect("the crash record's path has no NUL byte");
        let fd = unsafe {
            libc::open(
                c_path.as_ptr(),
                libc::O_WRONLY | libc::O_CREAT | libc::O_APPEND | libc::O_CLOEXEC,
                0o644,
            )
        };
        assert!(
            fd >= 0,
            "cannot open the crash record {}: {}",
            path.display(),
            std::io::Error::last_os_error()
        );
        FD.store(fd, Ordering::SeqCst);
        let pid = std::process::id();
        let mut names: Vec<&str> = SIGNALS.iter().map(|(_, name)| *name).collect();
        names.push("an uncaught Objective-C exception");
        let mut records = [([0u8; RECORD], 0usize); 6];
        for (index, name) in names.iter().enumerate() {
            let text = format!("mcbench-lite {version} pid {pid}: fatal {name}\n");
            let bytes = text.as_bytes();
            let length = bytes.len().min(RECORD);
            records[index].0[..length].copy_from_slice(&bytes[..length]);
            records[index].1 = length;
        }
        RECORDS
            .set(records)
            .expect("the crash records are written once");
        // The handlers run on their own stack, so a stack overflow reaches them.
        const ALT_STACK: usize = 64 * 1024;
        let stack = Box::leak(vec![0u8; ALT_STACK].into_boxed_slice());
        let alt = libc::stack_t {
            ss_sp: stack.as_mut_ptr().cast(),
            ss_flags: 0,
            ss_size: ALT_STACK,
        };
        assert_eq!(
            unsafe { libc::sigaltstack(&alt, std::ptr::null_mut()) },
            0,
            "sigaltstack failed: {}",
            std::io::Error::last_os_error()
        );
        for (signal, _) in SIGNALS {
            unsafe {
                let mut action: libc::sigaction = std::mem::zeroed();
                action.sa_sigaction = on_signal as *const () as libc::sighandler_t;
                action.sa_flags = libc::SA_SIGINFO | libc::SA_ONSTACK;
                libc::sigemptyset(&mut action.sa_mask);
                assert_eq!(
                    libc::sigaction(signal, &action, std::ptr::null_mut()),
                    0,
                    "sigaction failed: {}",
                    std::io::Error::last_os_error()
                );
            }
        }
        #[cfg(target_os = "macos")]
        unsafe {
            NSSetUncaughtExceptionHandler(on_objc_exception);
        }
    }

    fn write_record(index: usize) {
        let fd = FD.load(Ordering::SeqCst);
        if let Some(records) = RECORDS.get() {
            let (bytes, length) = &records[index];
            unsafe { libc::write(fd, bytes.as_ptr().cast(), *length) };
        }
    }

    extern "C" fn on_signal(signal: libc::c_int, _: *mut libc::siginfo_t, _: *mut libc::c_void) {
        let index = SIGNALS.iter().position(|(s, _)| *s == signal).unwrap_or(0);
        write_record(index);
        unsafe { libc::_exit(128 + signal) }
    }

    #[cfg(target_os = "macos")]
    unsafe extern "C" {
        fn NSSetUncaughtExceptionHandler(handler: extern "C" fn(*mut libc::c_void));
    }

    #[cfg(target_os = "macos")]
    extern "C" fn on_objc_exception(_: *mut libc::c_void) {
        write_record(SIGNALS.len());
        unsafe { libc::_exit(128 + libc::SIGABRT) }
    }
}

#[cfg(windows)]
mod platform {
    use std::fs::OpenOptions;
    use std::io::Write as _;
    use std::path::Path;
    use std::sync::Mutex;
    use windows_sys::Win32::System::Diagnostics::Debug::{
        EXCEPTION_POINTERS, SEM_FAILCRITICALERRORS, SEM_NOGPFAULTERRORBOX, SEM_NOOPENFILEERRORBOX,
        SetErrorMode, SetUnhandledExceptionFilter,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, TerminateProcess};

    static RECORD: Mutex<Option<(std::fs::File, String)>> = Mutex::new(None);

    pub(super) fn exit(status: i32) -> ! {
        unsafe { TerminateProcess(GetCurrentProcess(), status as u32) };
        unreachable!("TerminateProcess returned")
    }

    pub(super) fn install(path: &Path, version: &str) {
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .unwrap_or_else(|e| panic!("cannot open the crash record {}: {e}", path.display()));
        *RECORD.lock().expect("the crash record lock") = Some((
            file,
            format!("mcbench-lite {version} pid {}", std::process::id()),
        ));
        unsafe {
            SetErrorMode(SEM_FAILCRITICALERRORS | SEM_NOGPFAULTERRORBOX | SEM_NOOPENFILEERRORBOX);
            SetUnhandledExceptionFilter(Some(on_exception));
            _set_abort_behavior(0, WRITE_ABORT_MSG | CALL_REPORTFAULT);
            signal(SIGABRT, on_abort as *const () as usize);
        }
    }

    // The C runtime's signal and abort settings (ucrt).
    const SIGABRT: i32 = 22;
    const WRITE_ABORT_MSG: u32 = 0x1;
    const CALL_REPORTFAULT: u32 = 0x2;
    unsafe extern "C" {
        fn signal(signal: i32, handler: usize) -> usize;
        fn _set_abort_behavior(flags: u32, mask: u32) -> u32;
    }

    /// The C runtime's `abort()` raises SIGABRT before it fails fast.
    extern "C" fn on_abort(_: i32) {
        if let Ok(mut record) = RECORD.try_lock()
            && let Some((file, prefix)) = record.as_mut()
        {
            let _ = writeln!(file, "{prefix}: abort (SIGABRT)");
        }
        exit(3)
    }

    unsafe extern "system" fn on_exception(info: *const EXCEPTION_POINTERS) -> i32 {
        let code = unsafe { (*(*info).ExceptionRecord).ExceptionCode };
        if let Ok(mut record) = RECORD.try_lock()
            && let Some((file, prefix)) = record.as_mut()
        {
            let _ = writeln!(file, "{prefix}: unhandled exception 0x{code:08x}");
        }
        exit(code)
    }
}

// par-shell
#[cfg(test)]
mod relaunch {
    use super::*;

    #[test]
    fn a_long_message_is_cut_at_a_characters_edge_and_a_nul_is_replaced() {
        let long = "é".repeat(MESSAGE_LIMIT);
        let cut = bounded(&long);
        assert!(cut.ends_with('\u{2026}'));
        assert!(cut.len() <= MESSAGE_LIMIT + '\u{2026}'.len_utf8());
        assert_eq!(bounded("a\0b"), "a\u{fffd}b");
        assert_eq!(bounded("short"), "short");
    }
}
