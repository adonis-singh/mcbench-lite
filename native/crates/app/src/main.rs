//! # mcbench-lite
//!
//! The binary. One program for every role the native app has:
//!
//! | Command | Role | Calls |
//! |---|---|---|
//! | `mcbench-lite` or `mcbench-lite app` | the app: the core and its window | [`mcbench_lite::core::Core::start`], then `mcbench_ui::run` |
//! | `mcbench-lite headless <family> [arguments]` | the headless mode for the benchmark, CI and the parity scripts | `mcbench_headless::run` |
//! | `mcbench-lite tool-bridge <address>` | a provider CLI's stdio MCP server, relaying to the app over a Unix socket or named pipe | `mcbench_providers::bridge::relay_main` |
//! | `mcbench-lite sandbox-agent <arguments>` | the `bash` sandbox's agent: a box's keeper on Linux, and the guest agent (`mcbench_sandbox::agent`) | `mcbench_sandbox::agent::main` |
//!
//! Anything else prints the usage and exits with status 2. The V8 child of a
//! `build` call is not one of its commands: it is its own program,
//! `mcbench-build-child` (`mcbench_build::child_program`), shipped beside it.
//!
//! ## Owner
//!
//! The stage-three session. The `app` command's body is the seam between
//! the core and the interface: it builds the core (an implementation of
//! `mcbench_ui::Core`) and hands it to `mcbench_ui::run`, whose signature is
//! the stage-four session's.
//!
//! ## Behaviour defined by
//!
//! Nothing of today's suite yet; `start-script.test.js` (20) goes with
//! `./start` at stage five and the install routes are tested in CI.
//!
//! ## Decided
//!
//! What `app` does before its window opens, in this order (agreed with the
//! stage-four session, `native-stage3-session.md`, Owed to stage four):
//! the data folder and its `logs/` are named and made
//! (`mcbench_setup::logs_folder`); the interface's crash hook is installed
//! (`mcbench_ui::crash::install(logs, version, handled)`, `handled` being
//! [`mcbench_lite::core::threads::handled`]: engine threads and the core's
//! runtime threads); the core is started (the instance lock, the store, the
//! lifecycle, setup in the background); then `mcbench_ui::run` opens the
//! window (`mcbench_ui::run(core, Options::app())`).
//!
//! The updater (`mcbench_updater`, stage five) is two calls of `app` and
//! one of `headless`. `headless` first takes `mcbench_updater::hold_install()`,
//! a shared hold on the install it runs from, kept until the process ends,
//! so no starting app swaps the payload under a benchmark run; it cannot
//! fail (outside an install, or where no hold is possible, it holds
//! nothing), and when the installed payload is not this program's (its
//! `version` file differs, or this process was loaded from a payload a
//! swap moved aside) it prints one sentence and exits with status 2.
//! `main` first strips the updater's restart marker from its arguments
//! (`mcbench_updater::take_restart_marker`), whose answer reaches
//! `at_start`; nothing of the updater is in any process's environment.
//! `mcbench_updater::at_start(&logs, lock, restarted)` runs after the crash hook and
//! before the core: in a build without an update feed or for a package
//! manager's route it does nothing; otherwise it installs the updater's
//! panic hook and, only while `lock` holds the data folder's one-instance
//! lock (`mcbench_setup::InstanceLock::take`) and no other process runs
//! from the install, swaps in a verified download that waits beside the
//! install, releases the locks and executes the new program with the same
//! arguments in this process's place. A broken install route, a swap that
//! could not be reversed, or a new program that cannot start and cannot be
//! reversed is shown with `fatal`; every other failure goes to
//! `update.log` in `logs/` and the app starts as it is, holding its install
//! shared for its whole life (the `updater` value, which `app` keeps).
//! After the core started, `updater.check_in_background(client)`, with a
//! clone of the core's one client, starts the thread `mcbench-updater`,
//! which removes the previous version once this core runs, then checks the
//! feed now and every 24 hours and leaves a newer release downloaded and
//! verified for the next start.
//!
//! A panic on a thread neither the core nor the engine claims (the window's,
//! the renderer's, the updater's) ends the process without
//! `Core::shutdown`, as it always did. Since `relaunch_on_panic` (called
//! right after the hook is installed, on the window's path only) the hook
//! first starts this program again with the message in its environment, and
//! that process shows the error screen with `Restart` at the same call and
//! goes no further: no `at_start`, no data-folder lock, no core. It holds
//! the install (`mcbench_updater::hold_install_quietly`), so no swap happens
//! under it, and `Restart` starts `mcbench_updater::installed_program()`
//! with the arguments `main` was given less the updater's restart marker,
//! which only the updater's own restart may pass. What the ended process
//! leaves is what a killed one leaves, which the next start is built for:
//! the data folder's lock is judged by its pid, a turn in flight is ended at
//! start, a box of the same chat is replaced.
//!
//! When the data folder cannot be named or made, `crash::install` takes
//! `mcbench-lite-logs` under the system's temporary folder and `mcbench_ui::fatal(message,
//! Options::app())` shows why; when the core cannot start, `fatal` shows it
//! after the hook was installed in the data folder's `logs/`. The
//! subcommands without a window (`headless`, `tool-bridge`,
//! `sandbox-agent`) install no hook, open no window and keep stderr.
//!
//! On Windows the program is a window program (subsystem `windows`), so
//! Windows opens no console window for it, from a shortcut, from Explorer
//! or as a child of the app. A window program is given no console: what
//! its parent passes as standard input, output and error is all it has.
//! So `main` calls `attach_to_parent_console` for every command but the
//! app's window (`headless`, `tool-bridge`, `sandbox-agent`, the usage):
//! when a standard handle is missing and the parent has a console (a
//! person's `cmd.exe` or PowerShell), it attaches to that console, and the
//! missing handles write there; every handle the parent gave (a pipe, a
//! file) stays as it was, which `AttachConsole` alone does not ensure
//! (started from `cmd.exe` with `> out.txt` or `| more`, it replaced the
//! file and the pipe with the console). A shell does not wait for a window
//! program, which `PROTOCOL.md` tells a person who starts `headless` by
//! hand.
//!
//! The app's window (`mcbench-lite` or `mcbench-lite app`) does not attach,
//! from a terminal either: a window program on Windows outlives the
//! terminal that started it, as any other does. Attached, it belonged to
//! that console, and closing the terminal's window ended it at once
//! (measured in the Windows 11 guest: no `mcbench-lite.exe` left 0.0 s after
//! the terminal closed, `server.pid` left behind as by a killed process).
//! Its log lines go where they go from a shortcut.

#![cfg_attr(windows, windows_subsystem = "windows")]

use std::sync::Arc;

use mcbench_headless::Family;
use mcbench_lite::core::{Core, Parts};
use mcbench_providers::bridge::BridgeAddress;

const USAGE: &str = "usage: mcbench-lite [app | headless <engine|grade|turn|contract|shots|setup> [arguments] | tool-bridge <address> | sandbox-agent <arguments>]";

/// The app's version, which the crash log records.
const VERSION: &str = env!("CARGO_PKG_VERSION");

fn usage() -> ! {
    eprintln!("{USAGE}");
    std::process::exit(2)
}

/// The window's failure screen: `message` on the fatal screen, and exit
/// status 2 when the user quits. `crash::install` has run before it.
fn fatal(message: String) -> ! {
    mcbench_ui::fatal(message, mcbench_ui::Options::app())
}

/// The crash log's folder when the data folder cannot be named or made.
fn temporary_logs() -> std::path::PathBuf {
    std::env::temp_dir().join("mcbench-lite-logs")
}

/// The shipped data: embedded in a release build (feature `shipped-data`).
/// The app's setup and `headless setup` unpack it with this.
#[cfg(feature = "shipped-data")]
fn shipped(data: std::path::PathBuf) -> mcbench_lite::core::backend::DataSource {
    Box::new(move || mcbench_setup::Data::shipped(&data))
}

/// A build without the shipped data (feature `shipped-data` off) cannot
/// unpack it: its data step fails with how to make one that can.
#[cfg(not(feature = "shipped-data"))]
fn shipped(_data: std::path::PathBuf) -> mcbench_lite::core::backend::DataSource {
    Box::new(|| {
        Err("This build has no shipped Minecraft data: build it with --features shipped-data and MCBENCH_DATA_BUNDLE naming the bundle".into())
    })
}

fn app(restarted: bool, args: &[String]) -> ! {
    let handled = mcbench_lite::core::threads::handled as fn(&std::panic::PanicHookInfo) -> bool;
    let logs = match mcbench_setup::logs_folder() {
        Ok(logs) => logs,
        Err(message) => {
            mcbench_ui::crash::install(&temporary_logs(), VERSION, handled);
            fatal(message)
        }
    };
    mcbench_ui::crash::install(&logs, VERSION, handled);
    // The window's path only (the commands without a window keep a panic as
    // a log and an exit status for their caller): a panic the core and the
    // engine did not claim starts this program once more into the error
    // screen, and a process started that way shows it here, before the
    // updater's start, the data folder's lock and the core. `Restart` runs
    // the installed program (not this process's own path, which a swap may
    // have moved aside) with this process's arguments, the updater's marker
    // taken out of them by `main`. The screen's process holds the install
    // like every process of it, quietly, until it ends.
    let installed = mcbench_updater::installed_program().unwrap_or_else(|message| fatal(message));
    let arguments = args.iter().map(std::ffi::OsString::from).collect();
    if let Some(screen) = mcbench_ui::crash::relaunch_on_panic(installed, arguments) {
        let _hold = mcbench_updater::hold_install_quietly();
        let mut options = mcbench_ui::Options::app();
        screen.apply(&mut options);
        mcbench_ui::fatal(screen.message, options);
    }
    let data = logs
        .parent()
        .expect("logs/ is in the data folder")
        .to_path_buf();
    let updater = mcbench_updater::at_start(
        &logs,
        || mcbench_setup::InstanceLock::take(&data),
        restarted,
    )
    .unwrap_or_else(|message| fatal(message));
    let program = std::env::current_exe().unwrap_or_else(|e| fatal(format!("this program: {e}")));
    let client = mcbench_setup::fetch::client();
    let core = Core::start(Parts {
        shipped: shipped(data.clone()),
        data,
        program,
        host: Arc::new(mcbench_store::SystemHost),
        client: client.clone(),
        backend: None,
        installer: None,
        share: None,
        live: Default::default(),
    })
    .unwrap_or_else(|message| fatal(message));
    updater.check_in_background(client);
    mcbench_ui::run(Arc::new(core), mcbench_ui::Options::app())
}

/// Windows: the standard handles a window program writes with. Each handle
/// the parent gave is kept; when one is missing and the parent has a
/// console, the process attaches to it and the missing ones write there.
/// Without a console in the parent (Explorer, a shortcut, the app starting
/// its engine child with pipes) nothing changes.
#[cfg(windows)]
fn attach_to_parent_console() {
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Console::{
        ATTACH_PARENT_PROCESS, AttachConsole, GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE,
        STD_OUTPUT_HANDLE, SetStdHandle,
    };
    let kinds = [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE];
    let missing = |handle: HANDLE| handle.is_null() || handle == INVALID_HANDLE_VALUE;
    // SAFETY: GetStdHandle reads this process's standard handle of a valid
    // kind; it opens nothing.
    let given = kinds.map(|kind| unsafe { GetStdHandle(kind) });
    if !given.into_iter().any(missing) {
        return;
    }
    // SAFETY: AttachConsole takes no pointer; it fails, changing nothing,
    // when the parent has no console or this process has one already, and
    // otherwise attaches to the parent's console, opening handles of it as
    // standard handles.
    if unsafe { AttachConsole(ATTACH_PARENT_PROCESS) } == 0 {
        return;
    }
    for (kind, handle) in kinds.into_iter().zip(given) {
        // SAFETY: as above.
        let now = unsafe { GetStdHandle(kind) };
        if !missing(handle) && now != handle {
            // SAFETY: `now` is the console handle AttachConsole opened for
            // `kind`, this process's own, and nothing else holds it, so it
            // is closed once; `handle`, the one the parent gave, is still
            // open (AttachConsole replaced it without closing it) and becomes
            // the standard handle again.
            let closed = unsafe { CloseHandle(now) };
            assert_ne!(closed, 0, "closing the console's handle {kind:#x}");
            // SAFETY: `handle` is an open handle of this process (above).
            let set = unsafe { SetStdHandle(kind, handle) };
            assert_ne!(set, 0, "putting back the parent's handle {kind:#x}");
        }
    }
}

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let restarted = mcbench_updater::take_restart_marker(&mut args);
    // Every command but the app's window writes to the console it was
    // started from; the window is tied to no console (the header).
    #[cfg(windows)]
    {
        let window = args.is_empty() || args == ["app"];
        if !window {
            attach_to_parent_console();
        }
    }
    match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        [] | ["app"] => app(restarted, &args),
        ["headless", family, ..] => match Family::parse(family) {
            Ok(family) => {
                let _hold = mcbench_updater::hold_install();
                mcbench_headless::run(family, &args[2..], shipped)
            }
            Err(message) => {
                eprintln!("mcbench-lite: {message}");
                usage()
            }
        },
        ["tool-bridge", address] => {
            #[cfg(unix)]
            let address = BridgeAddress::UnixSocket(address.into());
            #[cfg(windows)]
            let address = BridgeAddress::NamedPipe(address.to_string());
            mcbench_providers::bridge::relay_main(&address)
        }
        ["sandbox-agent", ..] => {
            let exe = std::env::current_exe().expect("the program's own path");
            mcbench_sandbox::agent::main(
                vec![exe.into_os_string(), "sandbox-agent".into()],
                &args[1..],
            )
        }
        _ => usage(),
    }
}
