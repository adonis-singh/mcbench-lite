//! The crash module, by crashing a child process on purpose: a panic, a
//! segmentation fault and an abort each leave a record and a quiet exit
//! status. Linux only (adipc, and later the VMs): never on the Mac host,
//! where a crash could reach the system's crash reporter.
#![cfg(target_os = "linux")]

use std::path::Path;
use std::process::Command;

const CHILD: &str = "MCBENCH_CRASH_CHILD";
/// par-shell: the file a relaunched child notes itself in.
const RELAUNCHED: &str = "relaunched.log";
/// par-shell: the file the relaunched and restarted children note their
/// arguments in.
const ARGS: &str = "args.log";
/// par-shell: an argument for one start only (as the updater's restart
/// marker): the child passes it on to nobody. A second test-name filter to
/// the harness, which matches no test.
const ONCE: &str = "mcbench-marker-for-one-start";
/// par-shell: set by the error screen's child before it restarts.
const RESTARTED: &str = "MCBENCH_CRASH_RESTARTED";
/// par-shell: the program the armed child names for `Restart` (else its
/// own path).
const RESTART_PROGRAM: &str = "MCBENCH_CRASH_RESTART_PROGRAM";

/// Appends `line` to `name` in `logs`.
fn note(logs: &str, name: &str, line: &str) {
    use std::io::Write as _;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(Path::new(logs).join(name))
        .expect("the note");
    writeln!(file, "{line}").expect("a line");
}

/// This process's arguments, joined.
fn own_args() -> String {
    std::env::args().skip(1).collect::<Vec<_>>().join(" ")
}

/// Runs this test binary's `child` test with `how` and the logs folder.
fn crash(how: &str, logs: &Path) -> std::process::ExitStatus {
    Command::new(std::env::current_exe().expect("the test binary"))
        .args(["--exact", "child", "--nocapture", "--test-threads", "1"])
        .env(CHILD, format!("{how}:{}", logs.display()))
        .status()
        .expect("the child runs")
}

/// The child: installs the crash module, then crashes as asked. Without the
/// variable it does nothing (it is a test only so the parent can start it).
#[test]
fn child() {
    let Ok(order) = std::env::var(CHILD) else {
        return;
    };
    let (how, logs) = order.split_once(':').expect("how:logs");
    // A panic on a thread named "caught" is the binary's to catch.
    mcbench_ui::crash::install(Path::new(logs), "test", |_| {
        std::thread::current().name() == Some("caught")
    });
    // par-shell: `armed` and `armed-screen` opt in to the relaunch; the
    // relaunched process (the error screen's) notes the message it was
    // given instead of opening a window, then ends (`armed`) or panics
    // itself (`armed-screen`). Every other order does not opt in.
    if how.starts_with("armed") {
        // The app that `Restart` started notes its arguments and ends.
        if std::env::var(RESTARTED).is_ok() {
            note(logs, ARGS, &format!("restart: {}", own_args()));
            let me = std::env::current_exe().expect("this program");
            note(logs, ARGS, &format!("program: {}", me.display()));
            std::process::exit(0);
        }
        // The arguments to pass on: this process's own, less the one for
        // this start only.
        let args: Vec<std::ffi::OsString> =
            std::env::args_os().skip(1).filter(|a| a != ONCE).collect();
        let program = std::env::var_os(RESTART_PROGRAM)
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::env::current_exe().expect("this program"));
        if let Some(screen) = mcbench_ui::crash::relaunch_on_panic(program, args) {
            note(logs, RELAUNCHED, &screen.message.replace('\n', " "));
            note(logs, ARGS, &format!("screen: {}", own_args()));
            if how == "armed-screen" {
                panic!("the error screen panics on purpose");
            }
            if how == "armed-restart" {
                // SAFETY: one thread runs here; the variable marks the
                // restarted child.
                unsafe { std::env::set_var(RESTARTED, "1") };
                mcbench_ui::crash::restart().expect("the restart starts");
            }
            std::process::exit(0);
        }
        panic!("a panic on purpose");
    }
    // A process that did not opt in notes it if it was ever relaunched.
    if std::env::var(mcbench_ui::crash::FATAL_VARIABLE).is_ok() {
        std::fs::write(
            Path::new(logs).join(RELAUNCHED),
            "relaunched without the opt-in\n",
        )
        .expect("the marker");
        std::process::exit(0);
    }
    match how {
        "caught" => {
            let thread = std::thread::Builder::new()
                .name("caught".into())
                .spawn(|| std::panic::catch_unwind(|| panic!("a refusal on purpose")).is_err())
                .expect("a thread");
            assert!(
                thread.join().expect("the thread ends"),
                "the panic was caught"
            );
            println!("still alive");
            std::process::exit(0);
        }
        "panic" => panic!("a panic on purpose"),
        "segv" => unsafe {
            libc::raise(libc::SIGSEGV);
        },
        "abort" => unsafe { libc::abort() },
        other => panic!("no crash {other}"),
    }
    unreachable!("the crash ended the process");
}

fn logs(name: &str) -> std::path::PathBuf {
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("mcbench-ui-crash-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

#[test]
fn a_panic_writes_a_crash_log_and_exits_101() {
    let dir = logs("panic");
    let status = crash("panic", &dir);
    assert_eq!(status.code(), Some(101));
    let log = std::fs::read_dir(&dir)
        .expect("the logs folder")
        .map(|e| e.expect("an entry").path())
        .find(|p| {
            p.file_name().is_some_and(|n| {
                n.to_string_lossy().starts_with("crash-") && n != "crash-signals.log"
            })
        })
        .expect("a crash log");
    let text = std::fs::read_to_string(log).expect("the crash log");
    assert!(text.contains("message: a panic on purpose"), "{text}");
    assert!(
        text.contains("location: crates/ui/tests/crash.rs"),
        "{text}"
    );
    assert!(text.contains("backtrace:"), "{text}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_fatal_signal_is_recorded_and_exits_quietly() {
    for (how, signal, name) in [
        ("segv", libc::SIGSEGV, "SIGSEGV"),
        ("abort", libc::SIGABRT, "SIGABRT"),
    ] {
        let dir = logs(how);
        let status = crash(how, &dir);
        assert_eq!(
            status.code(),
            Some(128 + signal),
            "{how} exits with 128 + its signal"
        );
        let record =
            std::fs::read_to_string(dir.join("crash-signals.log")).expect("the signal record");
        assert!(record.contains(&format!("fatal {name}")), "{record}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[test]
fn a_panic_the_binary_catches_writes_nothing_and_the_process_lives() {
    let dir = logs("caught");
    let status = crash("caught", &dir);
    assert_eq!(status.code(), Some(0));
    let entries: Vec<_> = std::fs::read_dir(&dir)
        .expect("the logs folder")
        .map(|e| e.expect("an entry").file_name())
        .filter(|n| n != "crash-signals.log")
        .collect();
    assert!(entries.is_empty(), "no crash log: {entries:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

// par-shell (parity pass, finding 1): the relaunch into the error screen.

/// The marker's lines once no process can still be starting (3 s).
fn relaunched(dir: &Path) -> Vec<String> {
    std::thread::sleep(std::time::Duration::from_secs(3));
    std::fs::read_to_string(dir.join(RELAUNCHED))
        .map(|t| t.lines().map(str::to_string).collect())
        .unwrap_or_default()
}

#[test]
fn a_panic_in_a_process_that_did_not_opt_in_starts_nothing() {
    let dir = logs("unarmed");
    let status = crash("panic", &dir);
    assert_eq!(status.code(), Some(101));
    assert!(relaunched(&dir).is_empty(), "no process was started");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_armed_panic_starts_this_program_once_with_the_message() {
    let dir = logs("armed");
    let status = crash("armed", &dir);
    assert_eq!(
        status.code(),
        Some(101),
        "the panicking process exits as before"
    );
    assert_eq!(relaunched(&dir), ["a panic on purpose"]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_error_screens_own_panic_starts_nothing_more() {
    let dir = logs("armed-screen");
    let status = crash("armed-screen", &dir);
    assert_eq!(status.code(), Some(101));
    // One relaunch: the error screen's process panicked and started none.
    assert_eq!(relaunched(&dir), ["a panic on purpose"]);
    let logs: Vec<String> = std::fs::read_dir(&dir)
        .expect("the logs folder")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("crash-") && n != "crash-signals.log")
        .collect();
    assert!(
        !logs.is_empty(),
        "the screen's panic is logged too: {logs:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_screen_and_restart_get_the_given_arguments_not_the_panicking_processs() {
    let dir = logs("armed-restart");
    // `Restart`'s program: this test binary copied to another path (a
    // copy: a symbolic link needs a privilege on Windows), so the
    // restarted process can say which program it is.
    std::fs::create_dir_all(&dir).expect("the folder");
    let copy = dir.join("restart-program");
    std::fs::copy(std::env::current_exe().expect("the test binary"), &copy).expect("the copy");
    let status = Command::new(std::env::current_exe().expect("the test binary"))
        .args([
            "--exact",
            "child",
            "--nocapture",
            "--test-threads",
            "1",
            ONCE,
        ])
        .env(CHILD, format!("armed-restart:{}", dir.display()))
        .env(RESTART_PROGRAM, &copy)
        .status()
        .expect("the child runs");
    assert_eq!(status.code(), Some(101));
    assert_eq!(relaunched(&dir), ["a panic on purpose"]);
    let args = std::fs::read_to_string(dir.join(ARGS)).expect("the arguments noted");
    let lines: Vec<&str> = args.lines().collect();
    let given = "--exact child --nocapture --test-threads 1";
    assert_eq!(
        lines,
        [
            format!("screen: {given}"),
            format!("restart: {given}"),
            format!("program: {}", copy.display()),
        ],
        "neither got {ONCE}; Restart ran the given program"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
