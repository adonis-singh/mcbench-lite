//! `mcbench-lite headless engine <tables folder> [--probe | --datafix |
//! --movement <cases file>]`: the released engine program's own protocol
//! loop, from its own source.
//!
//! `core/src/main.rs` is compiled in here unchanged (`include!`), so the
//! requests, replies, files, the refusal rule and the flags are the released
//! program's. This module only does what that file's `main()` does with the
//! words after `engine`: hands the tables folder to `mcbench_core::tables`
//! and runs `serve()` on a thread with a 1 GiB stack. `serve()` reads its
//! flags from `std::env::args()` with `any` and `position`, so it finds them
//! wherever they stand on this binary's longer command line.
//!
//! Which build it is: this workspace's release profile (`native/Cargo.toml`,
//! the same settings as `core/Cargo.toml`'s: `lto = true`, `codegen-units =
//! 1`), without the released program's profile-guided optimisation
//! (`core/pgo/`'s profiles match none of this build's functions), so its
//! speed is not the released program's: on adipc it ticked 10,000 observer
//! clocks at 21.8 ticks a second against the released program's 28.0, and
//! summed core-bench's 82 builds at 571 / 1,503 / 2,284 ms (place / 1,000
//! ticks / VERIFY) against 477 / 1,226 / 1,909. Built with a profile
//! recorded on this workspace's build it ran at 28.3 ticks a second and 504
//! / 1,326 / 2,049 ms (medians of 5 alternating rounds, load 4 to 11; the
//! stage-three engine report has the table). Its replies and files are
//! the released program's: the parity scripts give 0 differences between
//! the two. The parity scripts and the private benchmark keep driving the
//! released program by default until the speed table's ticks and
//! core-bench rows show this binary and the in-process engine at least as
//! fast as it.

/// `core/src/main.rs`, as the released program compiles it. Its `main` is
/// the released program's entry and is not called here; the file is
/// `core/`'s, so this crate's lints do not judge it.
#[allow(dead_code, unused_imports, clippy::all, missing_docs)]
mod program {
    include!("../../../../core/src/main.rs");

    /// The released program's `serve()`.
    pub(super) fn run() {
        serve()
    }
}

/// Serves the engine's protocol on stdin and stdout until stdin ends, as the
/// released program does, then exits with status 0. `arguments` are the
/// words after `engine`.
pub(crate) fn run(arguments: &[String]) -> ! {
    let Some(folder) = arguments.first().filter(|a| !a.starts_with("--")) else {
        eprintln!(
            "usage: mcbench-lite headless engine <tables folder> [--probe | --datafix | --movement <cases file>]"
        );
        std::process::exit(2);
    };
    mcbench_core::tables::set(mcbench_core::tables::read(std::path::Path::new(folder)));
    watch_parent();
    // Block code recurses as deep as Minecraft's (512 nested block sets).
    std::thread::Builder::new()
        .stack_size(1 << 30)
        .spawn(program::run)
        .unwrap()
        .join()
        .unwrap();
    std::io::Write::flush(&mut std::io::stdout()).unwrap();
    // SAFETY: the C runtime's `exit` with a status; it does not return.
    unsafe { exit(0) }
}

// The C runtime's `exit`, which runs its exit handlers and then ends the
// process. `std::process::exit` is this call on Unix; on Windows it is
// `ExitProcess`, which skips those handlers, and a program built to record
// a profile (the release build's instrumented engine) writes the profile
// from one: the engine's recording on Windows wrote no file.
unsafe extern "C" {
    fn exit(status: std::ffi::c_int) -> !;
}

/// Ends the process when its parent is gone (on Unix: its parent process
/// id changed, the process was given to another). The app's engine children
/// (`mcbench-engine`'s `ChildEngine`) are killed by the app; this is for an
/// app that was itself killed hard mid-request, whose child would otherwise
/// run its request to its end before it read the end of its stdin. Windows
/// has no parent process id to watch; there every child runs in a Job
/// object the app holds with kill-on-job-close (`mcbench-engine`), and
/// the system kills it when the app's process goes.
fn watch_parent() {
    #[cfg(unix)]
    {
        let parent = std::os::unix::process::parent_id();
        std::thread::Builder::new()
            .name("parent-watch".into())
            .spawn(move || {
                loop {
                    std::thread::sleep(std::time::Duration::from_millis(250));
                    if std::os::unix::process::parent_id() != parent {
                        std::process::exit(1);
                    }
                }
            })
            .unwrap();
    }
}

#[cfg(test)]
mod tests {
    // `include!` takes a file of items: an inner attribute (`#![...]`) or an
    // inner doc comment (`//!`) in core/src/main.rs would stop this crate from
    // compiling, and `mcbench-lite headless engine` (what the parity scripts
    // and the benchmark can drive in place of the released program) would be
    // gone with it. This test says why before the build error does, and holds
    // the two names this module calls.
    #[test]
    fn core_main_still_compiles_through_include() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../core/src/main.rs");
        let source = std::fs::read_to_string(path).unwrap();
        for (number, line) in source.lines().enumerate() {
            let line = line.trim_start();
            assert!(
                !line.starts_with("#![") && !line.starts_with("//!"),
                "core/src/main.rs line {} is an inner attribute or inner doc comment ({line}): \
                 mcbench-headless compiles that file with include! inside a module, where inner \
                 attributes are not allowed, so `mcbench-lite headless engine` would stop \
                 building. Keep main.rs free of them or move the protocol loop into core's library.",
                number + 1
            );
        }
        assert!(
            source.contains("\nfn serve() {"),
            "core/src/main.rs has no `fn serve()`: mcbench-headless's engine family runs it"
        );
        assert!(
            source.contains("use mcbench_core::"),
            "core/src/main.rs no longer names the library as mcbench_core, which the include! relies on"
        );
    }
}
