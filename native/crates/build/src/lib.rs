//! # mcbench-build
//!
//! Build programs: the JavaScript a model passes to `build`, run by today's
//! files, unchanged, in a single-use V8 child under the platform's sandbox.
//! The child is its own program, `mcbench-build-child`, in its own
//! workspace with its own `Cargo.lock` (`native/build-child`: V8 and the
//! child's code, nothing of the interface, the renderer or the store),
//! shipped beside the app. This crate holds the parent's side ([`V8Runner`]:
//! the request, the frames, the wall clock, the spare process, the
//! sandbox's launch), the lookup of the child's program
//! ([`child_program`]), and what both sides share: the heap cap and the
//! Linux seccomp filter (`confine`). It links no V8.
//!
//! The JavaScript that runs in the child is today's, unchanged. These five
//! files of `server/` are the child's sources, embedded with `include_str!`
//! from their place (`native/build-child/src/child/mod.rs`):
//! `build-child.js`, `mc-dsl.js`, `world.js`, `minecraft-dimension.js` and
//! `build-transfer.js`. A change to one of them is a change to what models'
//! programs run on. Beside them, `native/build-child/src/child/prelude.js`
//! gives them what they use from Node
//! (docs/native-app.md, JavaScript for build programs). Everything a model
//! reads from a program (its logs, its errors with `build.js:N`, the source
//! line and the caret, the timeout and out-of-memory wording) comes from
//! those files, the ported `vm` callbacks and `build-sandbox.js`'s messages
//! (ported in `src/runner.rs` and `src/format.rs`, held by the tests and
//! the differential harness).
//!
//! It replaces `server/build-sandbox.js` (the parent and the sandbox per
//! platform) and takes `mc-dsl.js`, `build-transfer.js` and `build-child.js`
//! into the child.
//!
//! ## Owner
//!
//! The stage-three session.
//!
//! ## Seams
//!
//! | Seam | What crosses | Who calls whom | Thread or process |
//! |---|---|---|---|
//! | tools | [`BuildRequest`] and a [`World`] in; [`BuildOutcome`] or [`BuildFailure`] out | `mcbench-tools`' `build` calls [`BuildRunner::run`] | the async runtime; the program runs in the child process |
//! | world | `build-transfer.js`'s encoding of the world to the child and of its change set back: [`V8Runner::run_encoded`] takes and returns those bytes; [`BuildRunner::run`] clones the world, encodes it and applies the change set with `mcbench_world::transfer`, each palette entry checked by [`normalize::Normalizer`] (`mc-dsl.js` `createBlockNormalizer`) | this crate calls `mcbench-world` | the async runtime |
//! | the child | the process protocol below (today's `server/build-child.js` frames) | the parent starts `mcbench-build-child` (the path [`child_program`] gives), writes the request and answers `READ` and `SETTLED` with [`BuildHost`] | one child process per call, over its stdin and stdout |
//! | the sandbox | the child's command line and limits (`sandbox-exec` with `build-sandbox.js`'s profile on macOS; bubblewrap and `prlimit` on Linux; the same Linux command inside the app's WSL2 distro through `wsl.exe` on Windows); the started child as a [`process::ChildProcess`] (its pipes, its pid, `wait`, `kill`) | the parent launches through [`ChildSandbox::spawn`] ([`sandbox::platform`]) and reads a child that ended unanswered through [`ChildSandbox::refusal`]; the sandbox crate takes the Linux reading and its sentence from [`sandbox::linux`] | the child process |
//! | shared with the child | [`HEAP_LIMIT_MB`] and, on Linux, `confine::install` (the seccomp filter) | `native/build-child` names this crate by path | the child process |
//!
//! ### The process protocol (the seam between the app and the child)
//!
//! The app and the child share no code that speaks it: the child runs
//! today's `server/build-child.js`, and this crate's [`runner`] is
//! `build-sandbox.js` `runProgram`. All integers are 32-bit little-endian.
//!
//! | Step | Direction | Bytes |
//! |---|---|---|
//! | request | parent to child, stdin | `[len u32][JSON]`, the JSON `{"source", "registry", "readFile", "settledTicks"}` in that order (`settledTicks` a number or `null`) |
//! | world | parent to child, stdin | `[len u32][build-transfer.js encodeWorld bytes]` |
//! | a frame | child to parent, stdout | `[kind u8][len u32][payload]` |
//! | `READ` 1 | child asks, only when `readFile` | payload: the path, UTF-8; the parent answers on stdin `[ok u8][len u32][text]`, `ok` 1 with the file's text or 0 with the error the program sees |
//! | `SETTLED` 4 | child asks, only when `settledTicks` is a number, at most once | empty payload; the parent answers on stdin `[len u32][encodeWorld bytes]` of the settled world |
//! | `DONE` 2 | the child's last frame | `[meta len u32][meta JSON {"logs", "files", "touched"}][encodeChanges bytes]` |
//! | `FAIL` 3 | the child's last frame | JSON `{"stack", "message", "logs"}`: `stack` the error's stack or `""`, `message` its message (or the thrown value's string form when there is no stack), `logs` an array or `null` |
//!
//! Who closes what: after `DONE` or `FAIL` the child exits with status 0;
//! the parent kills it once it has an answer, past the wall clock
//! ([`WALL_TIMEOUT`], 10 s) or when the run is cancelled. A child whose
//! stdin closes before a whole request exits with status 1 and the error on
//! stderr. Any other frame, a `READ` or `SETTLED` that was not offered, or a
//! reply over 256 MiB, is a malformed result; stdout closing without an
//! answer is a crash, reported with the last line of the first 16 KiB of
//! stderr (or as out of memory, [`HEAP_LIMIT_MB`]), unless that stderr is
//! the sandbox's own refusal to start the child ([`ChildSandbox::refusal`]:
//! on Linux, bubblewrap refused a user namespace), which is reported with
//! the sentence `bash` gives for it. The parent accepts at
//! most 65 log entries of at most 314 UTF-16 units each, 16 written files of
//! 32 Mi UTF-16 units in all, and a safe-integer `touched`.
//!
//! Held on both sides by recorded frames of today's `build-child.js`
//! (`native/build-child/tests/frames/`, made by its `record.js`): the child
//! must answer each recorded request with the recorded bytes
//! (`native/build-child/tests/protocol.rs`), and the parent must write the
//! recorded request and read the recorded answer as the real child's
//! (`tests/child/protocol.rs`), so either side changing alone fails.
//!
//! ## Behaviour defined by
//!
//! `build-sandbox.test.js` 19 and `mc-dsl.test.js` 18 (the same JavaScript
//! cases run in the V8 child): 37 tests in 2 files (docs/native-app.md,
//! Appendix A), ported in `tests/child/ported.rs` against golden files of
//! Node 26.13.1's results (`tests/golden/`, 672 synthetic cases, 688 KB).
//! Beside them, in the same test binary: the recorded and adversarial part
//! of the corpus against Node 26.13.1 (every result Node's, or the one
//! `harness/allow-list.json` pins with its cause), the `Math` hash test (the
//! plan's 15 hashes on every platform), the confinement probes under
//! `sandbox-exec` and bubblewrap, the protocol against recorded frames and
//! forged frames. The differential harness (`harness/`, `examples/corpus.rs`)
//! runs the whole 928-case corpus, real chats included, from scratch.
//!
//! The tests that run the child need it built and named:
//! `MCBENCH_BUILD_CHILD=<native/build-child's built mcbench-build-child>`
//! (`native/check.sh` builds it first and sets it); without it they are
//! reported ignored. Tests that need Node or the scratch corpus are
//! `#[ignore]`d without their variable and fail when it is set and something
//! is missing:
//! `MCBENCH_BUILD_NODE=<node>` (the profile and arguments against
//! `build-sandbox.js`'s own) and `MCBENCH_BUILD_GOLDEN_NODE=<node>`
//! (regenerates the golden files with that Node and compares).
//!
//! ## Open
//!
//! What a model reads when a turn is stopped during a build: today's
//! `runProgram` takes no signal, so the build runs to its end; here a
//! cancelled run is killed and fails with [`runner::CANCELLED`]. Settled by
//! the agent crate's stopped-turn handling.
//!
//! A lone surrogate in a log line or an error (`mc.log("a\udc00b")`):
//! today `agent.js` `toolMessageContent` turns the result into JSON with
//! `JSON.stringify`, which writes it as the six characters `\udc00`, and
//! from there (the stored message, the provider's request) it is plain
//! text; a Rust `String` cannot hold the surrogate, so the parent reads
//! U+FFFD and the tool result would say `�` (`harness/allow-list.json`,
//! adv-lone-surrogate). Written files are not affected (`Buffer.from`
//! writes U+FFFD today too). Settled by a JavaScript string type through
//! [`BuildOutcome`], [`BuildFailure`] and `mcbench-json`'s writer, a seam
//! change for the tools crate.
//!
//! The recursion depth (adv-recursion-depth-exact) depends on the stack the
//! binary's own code used before the program started, so it is not pinned.
//!
//! `Math`: macOS arm64 and Linux x64 give different values for `Math.pow`
//! and `**` (the C library's pow, for exponents other than 0.5 and 2) and
//! NaN results with a different sign; the same split is in Node. Windows
//! gives Linux's: its child is the Linux program on the Linux C library. A program
//! that compares such a value with a threshold builds a different world on
//! the Mac than on Linux, today as here. The owner's decision on the
//! embedded V8's `Math` was taken on the statement that the platforms give
//! the same bits, which does not hold for these.
//!
//! ## Decided
//!
//! On Windows the child is the Linux x86_64 `mcbench-build-child` under
//! the Linux sandbox inside the app's WSL2 distro, one `wsl.exe` per child
//! with the protocol on its pipes ([`sandbox::windows`]; the owner's
//! decision of 2026-11-17): no Windows-native confinement code. The sandbox
//! is chosen in one place, [`sandbox::platform`], behind [`ChildSandbox`],
//! so a Windows-native sandbox can replace it later without a change to any
//! caller; nothing about WSL is outside the sandbox types.
//!
//! The child is its own program, `mcbench-build-child`, on every platform
//! (`native/README.md`, Decisions of stage three): on Windows a process that
//! imports the window libraries cannot start under the policy the sandbox
//! uses, and everywhere the child then maps only V8 and the C runtime. Its
//! tests' extra modes (the confinement probe, the forged children) live in
//! this crate's test binary, selected by an environment variable, so the
//! shipped child holds no test code.
//!
//! The child is a workspace of its own (`native/build-child`, its own
//! `Cargo.lock`): V8 146.9.0's `temporal_rs` 0.1.2 compiles only against the
//! icu_* crates at 2.1, the app's window toolkit needs icu 2.3, and one lock
//! cannot hold two 2.x versions of a crate. The app and the child meet only
//! in the process protocol, so each keeps its lock.
//!
//! One lookup of the child's program, [`child_program`]: `MCBENCH_BUILD_CHILD`
//! when it is set (tests, the check and the replays name the built program),
//! otherwise `mcbench-build-child` beside the running executable (the
//! shipped layout); a missing program is the error [`CHILD_MISSING`].
//!
//! [`ChildSandbox::spawn`] returns the crate's [`process::ChildProcess`],
//! not a `std::process::Command`: the Windows launch (a restricted token, an
//! AppContainer and a Job object) cannot be written as one. macOS and Linux
//! start their command through tokio ([`process::ChildProcess::start`]).
//!
//! On Linux the child installs a seccomp filter (`src/confine.rs`, in this
//! crate so that the confinement probe installs the same one) on all its
//! threads after V8 is initialised and before any JavaScript runs: no
//! program starts and no socket opens, the layer Node's permission model is
//! today. The macOS profile already refuses both (`process-exec` only of the
//! binary, `deny network*`: EPERM in the probe); on Windows the child is
//! the Linux program and installs the same filter inside the distro.
//!
//! The child and `v8` build at `opt-level = 3` in the dev profile too
//! (`native/build-child/Cargo.toml`): the `vm` interceptors run on every
//! global lookup, and a debug build ran adv-math-bits 5 times slower (2.8 to
//! 3.3 s against 0.6 to 0.8 s), which put heavy programs near the 5 s limit
//! in debug test runs.
//!
//! [`BuildRunner::run`] and [`BuildHost`]'s calls are `async fn` on the
//! core's tokio runtime (`native/README.md`, Decisions of stage three): the
//! parent awaits the child's frames with a timeout and a
//! [`CancellationToken`], and killing the child is dropping it.
//!
//! The runner keeps one spare process as `build-sandbox.js` does (started at
//! once, then on Linux as soon as a build's request is written and on macOS
//! when a build ends): the speed table's `build` rows measure the protocol
//! on a process that is already running, and a cold start costs 12 to 15 ms
//! on the Mac.
//!
//! The array-buffer allocator is Node's (`calloc`, `malloc`, a null pointer
//! on failure) and counts live bytes against the heap cap plus 2 GiB on
//! every platform, so `new Uint8Array(2 ** 40)` is V8's "Array buffer
//! allocation failed" on macOS as under Linux's `RLIMIT_DATA` (rusty_v8's
//! default allocator aborts the process with a V8 check there).
//!
//! The wall clock is `build-sandbox.js`'s `WALL_TIMEOUT_MS`, 10 s (the 5 s
//! script timeout of `mc-dsl.js` plus 5 s), not 5 s: the model reads
//! "exceeded 10000 ms" today.
//!
//! The world crosses as bytes ([`V8Runner::run_encoded`]): the encoding is
//! `build-transfer.js`'s, owned by `mcbench_world`'s transfer codec, so this
//! crate never decodes a world and is tested with Node's encodings.

use async_trait::async_trait;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

pub use mcbench_world::World;

#[cfg(target_os = "linux")]
pub mod confine;
mod format;
pub mod normalize;
pub mod process;
pub mod runner;
pub mod sandbox;
mod transfer;

pub use runner::{EncodedHost, EncodedOutcome, V8Runner};

/// The wall-clock limit of one build process (`server/build-sandbox.js`
/// `WALL_TIMEOUT_MS`, the 5 s script timeout of `mc-dsl.js` plus 5 s): the
/// child is killed past it and the model reads today's message.
pub const WALL_TIMEOUT: Duration = Duration::from_millis(10_000);

/// The heap cap of the child, 2,048 MB (the owner's decision: the
/// out-of-memory message a model reads stays "Program ran out of memory
/// (limit 2048 MB)").
pub const HEAP_LIMIT_MB: u64 = 2_048;

/// One `build` call's program, as `build-sandbox.js` `runProgram` sends it to
/// the child (its request frame's JSON: `source`, `registry`, `readFile`,
/// `settledTicks`).
#[derive(Clone, Debug)]
pub struct BuildRequest {
    /// The program's JavaScript source, as the model wrote it.
    pub source: String,
    /// The block registry the program's `mc` checks names against, as the
    /// JSON today's server sends.
    pub registry: serde_json::Value,
    /// Whether `mc.readFile` is offered (the chat's `/work` folder).
    pub read_file: bool,
    /// The ticks Minecraft ran for `mc.settled`, when a settled world exists.
    pub settled_ticks: Option<u32>,
}

/// A file a program wrote with `mc.writeFile` or `mc.dump`, applied to
/// `/work` by the caller only if the build is applied.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WrittenFile {
    /// The path the program gave, relative to `/work`.
    pub path: String,
    /// The file's text.
    pub text: String,
}

/// What a program that finished leaves: the clone it changed and what it
/// printed and wrote (`runProgram`'s `{ logs, files, touched }`).
#[derive(Clone, Debug)]
pub struct BuildOutcome {
    /// The world after the program, a clone of the one given.
    pub world: World,
    /// What the program logged, in order.
    pub logs: Vec<String>,
    /// The files it wrote.
    pub files: Vec<WrittenFile>,
    /// The blocks its calls touched (`mc-dsl.js`'s count).
    pub touched: i64,
}

/// A program that failed: the message the model reads (from `mc-dsl.js`
/// `formatProgramError` of the program's error or of `build-sandbox.js`'s
/// timeout, crash, out-of-memory and malformed-result errors) and the logs
/// printed before it. `agent.js`'s cut at 8,000 characters is the tool's.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuildFailure {
    /// The message, as today's files word it.
    pub message: String,
    /// The program's logs up to the failure.
    pub logs: Vec<String>,
}

/// What the child asks its parent for while a program runs.
#[async_trait]
pub trait BuildHost: Send {
    /// `mc.readFile(path)`: the file's text from the chat's `/work`, read by
    /// the parent's symlink-safe reader, or the error the program sees.
    async fn read_file(&mut self, path: &str) -> Result<String, String>;
    /// `mc.settled`: the world Minecraft made of the pre-build world, sent to
    /// the child only when the program asks for it.
    async fn settled_world(&mut self) -> Result<World, String>;
}

/// Runs build programs. One implementation, [`V8Runner`]: the V8 child under
/// the platform sandbox; tests of `mcbench-tools` and `mcbench-agent` write
/// fakes.
#[async_trait]
pub trait BuildRunner: Send + Sync {
    /// Runs `request` against a clone of `world`. On success the clone is
    /// changed as an in-process run would change it; on failure nothing is.
    /// A cancelled run kills the child and fails with
    /// [`runner::CANCELLED`].
    async fn run(
        &self,
        request: &BuildRequest,
        world: &World,
        host: &mut dyn BuildHost,
        cancel: &CancellationToken,
    ) -> Result<BuildOutcome, BuildFailure>;
}

/// The name of the build child's program, `mcbench-build-child`.
pub const CHILD_PROGRAM: &str = "mcbench-build-child";

/// The variable that names the built child's program for tests, the check
/// and the replays.
pub const CHILD_PROGRAM_VAR: &str = "MCBENCH_BUILD_CHILD";

/// The sentence a person reads when the build child's program is not
/// there, for every build (a named one too).
pub const CHILD_MISSING: &str = "The app's files are incomplete: mcbench-build-child is missing beside the app. Install the app again.";

/// The build child's program: the one `MCBENCH_BUILD_CHILD` names when it is
/// set, otherwise `mcbench-build-child` beside the running executable, where
/// the app ships it. On Windows that file is the Linux x86_64 program
/// without a suffix (the sandbox runs it inside the app's WSL2 distro:
/// [`sandbox::windows`]). [`CHILD_MISSING`] when that file is not there.
pub fn child_program() -> Result<std::path::PathBuf, String> {
    child_program_named(std::env::var_os(CHILD_PROGRAM_VAR))
}

/// [`child_program`] with the variable's value given.
fn child_program_named(named: Option<std::ffi::OsString>) -> Result<std::path::PathBuf, String> {
    let program = match named {
        Some(named) => std::path::PathBuf::from(named),
        None => std::env::current_exe()
            .map_err(|e| format!("this program's own path: {e}"))?
            .with_file_name(CHILD_PROGRAM),
    };
    if program.is_file() {
        Ok(program)
    } else {
        Err(CHILD_MISSING.to_string())
    }
}

/// How the child is confined, one implementation per platform, each
/// compiling on every platform.
pub trait ChildSandbox: Send + Sync {
    /// Starts `program` (`mcbench-build-child`) confined, as
    /// `build-sandbox.js` confines its child on macOS and Linux, with an
    /// empty environment but `env`, `/` as the working folder and its three
    /// pipes, killed when dropped.
    fn spawn(
        &self,
        program: &std::path::Path,
        env: &[(&str, &std::ffi::OsStr)],
    ) -> std::io::Result<process::ChildProcess>;
    /// What must be done, possibly waiting on other programs, before
    /// `program` can be started quickly; the runner calls it on the blocking
    /// pool before each [`ChildSandbox::spawn`]. Nothing on macOS and Linux;
    /// on Windows the child is installed into the distro (once per import).
    fn prepare(&self, _program: &std::path::Path) -> std::io::Result<()> {
        Ok(())
    }
    /// Whether the next spare process starts as soon as a build's request is
    /// written (Linux) rather than when the build ends (macOS), as
    /// `build-sandbox.js` `spareDuringBuild`.
    fn spare_during_build(&self) -> bool;
    /// The sentence a build fails with when the child ended before it
    /// answered and its stderr says the sandbox itself could not start it
    /// (on Linux, bubblewrap refused a user namespace:
    /// [`sandbox::linux::classify_bwrap_failure`]); `None` where such an
    /// ending is the child's crash, which the build reports with the
    /// child's last line.
    fn refusal(&self, _stderr: &str) -> Option<&'static str> {
        None
    }
    /// Whether a child can be started now: [`Readiness::Ready`] where the
    /// sandbox needs nothing first (macOS, Linux); on Windows what the app
    /// knows of its WSL distro ([`sandbox::wsl::state`]). No child is
    /// started before it is ready, and while it is unavailable a build
    /// fails with its sentence.
    fn readiness(&self) -> tokio::sync::watch::Receiver<Readiness> {
        static READY: std::sync::OnceLock<tokio::sync::watch::Sender<Readiness>> =
            std::sync::OnceLock::new();
        READY
            .get_or_init(|| tokio::sync::watch::channel(Readiness::Ready).0)
            .subscribe()
    }
}

/// Whether a [`ChildSandbox`] can start a child.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Readiness {
    /// It can.
    Ready,
    /// Not known yet: wait.
    Waiting,
    /// It cannot: the sentence a build fails with.
    Unavailable(String),
}

#[async_trait]
impl BuildRunner for V8Runner {
    async fn run(
        &self,
        request: &BuildRequest,
        world: &World,
        host: &mut dyn BuildHost,
        cancel: &CancellationToken,
    ) -> Result<BuildOutcome, BuildFailure> {
        let mut clone = world
            .clone_world()
            .expect("the world given to a build is finalized");
        let palette_start = clone.palette().len();
        let encoded = transfer::encode_world(&clone);
        let mut host = transfer::Host(host);
        let outcome = self
            .run_encoded(request, &encoded, &mut host, cancel)
            .await?;
        transfer::apply_changes(
            &mut clone,
            &outcome.changes,
            palette_start,
            &request.registry,
        )
        .map_err(|reason| runner::malformed(&reason))?;
        Ok(BuildOutcome {
            world: clone,
            logs: outcome.logs,
            files: outcome.files,
            touched: outcome.touched,
        })
    }
}

#[cfg(test)]
mod seam {
    use super::*;

    struct FakeRunner;
    #[async_trait]
    impl BuildRunner for FakeRunner {
        async fn run(
            &self,
            _: &BuildRequest,
            _: &World,
            _: &mut dyn BuildHost,
            _: &CancellationToken,
        ) -> Result<BuildOutcome, BuildFailure> {
            unimplemented!("a fake for the seam test")
        }
    }

    struct FakeHost;
    #[async_trait]
    impl BuildHost for FakeHost {
        async fn read_file(&mut self, _: &str) -> Result<String, String> {
            unimplemented!("a fake for the seam test")
        }
        async fn settled_world(&mut self) -> Result<World, String> {
            unimplemented!("a fake for the seam test")
        }
    }

    struct FakeSandbox;
    impl ChildSandbox for FakeSandbox {
        fn spawn(
            &self,
            _: &std::path::Path,
            _: &[(&str, &std::ffi::OsStr)],
        ) -> std::io::Result<process::ChildProcess> {
            unimplemented!("a fake for the seam test")
        }
        fn spare_during_build(&self) -> bool {
            unimplemented!("a fake for the seam test")
        }
    }

    #[test]
    fn a_missing_child_program_says_the_app_is_incomplete() {
        let refusal = |_: &std::path::Path| {
            "The app's files are incomplete: mcbench-build-child is missing beside the app. Install the app again.".to_string()
        };
        // Unnamed: beside this test binary, whose folder holds no build
        // child (native/build-child builds into its own target folder).
        let exe = std::env::current_exe().unwrap();
        let beside = exe.with_file_name("mcbench-build-child");
        assert_eq!(child_program_named(None).unwrap_err(), refusal(&beside));
        // Named: that file and no other.
        let named = exe.with_file_name("no-such-build-child");
        assert_eq!(
            child_program_named(Some(named.clone().into_os_string())).unwrap_err(),
            refusal(&named)
        );
        assert_eq!(
            child_program_named(Some(exe.clone().into_os_string())).unwrap(),
            exe
        );
    }

    #[test]
    fn a_fake_can_be_written_against_each_trait() {
        let _: Box<dyn BuildRunner> = Box::new(FakeRunner);
        let _: Box<dyn BuildHost> = Box::new(FakeHost);
        let _: Box<dyn ChildSandbox> = Box::new(FakeSandbox);
    }
}
