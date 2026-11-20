//! # mcbench-engine
//!
//! The engine (`core/`, the crate `mcbench-core`) for the app, two ways:
//! in the app's own process for the live world of the open chat
//! ([`EngineWorld`], on engine threads), and in engine CHILD processes of
//! the app's own binary for everything that may run long or be stopped:
//! the settles of builds ([`SettlePool`] of [`ChildEngine`]s, with
//! [`Settles`] keeping what each conversation settled), `test_build`'s
//! `VERIFY` ([`VerifyEngines`]) and the data fixer's palette
//! ([`EngineProgram::fixed_palette`]). It replaces `server/engine.js` (what
//! the server uses of it), `engine-pool.js` and the engine half of
//! `shot-world.js` (the settle's requests and what is kept of each
//! conversation's settles).
//!
//! `core/` is not a member of this workspace and nothing here changes a file
//! in it: its sources, `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`,
//! `.cargo/config.toml` and `pgo/` are hashed into the engine's released
//! version (`server/engine.js` `sourcesHash`, today `ad327be5bdf0dd1a`). The
//! engine's protocol and files (`core/src/main.rs`: `PLACE`, `TICK`, `INPUT`,
//! `VERIFY`, `PSPAWN`, `PTICK`, `PREMOVE`, `SURVIVE`, `STATE`) stay as they
//! are, because the private benchmark, the parity scripts and the children
//! drive them (`mcbench-lite headless engine`).
//!
//! ## Owner
//!
//! The stage-three session.
//!
//! ## Seams
//!
//! | Seam | What crosses | Who calls whom | Thread or process |
//! |---|---|---|---|
//! | `mcbench-core` | command text in, blueprint, patch, entity and input bytes out, `Result<_, String>` with the engine's own wording ("unported: ...") | this crate calls `mcbench_core::session::Session` | an engine thread with a 1 GiB stack, as `core/src/main.rs` runs block code |
//! | the binary's `headless engine` | `core/src/main.rs`'s protocol: request lines on the child's stdin, reply lines on its stdout, files in the child's folder (named relative to it), stderr's tail in the exit's sentence | [`ChildEngine`] starts `<binary> headless engine <tables>` and talks to it; `--datafix` for the fixer | a child process per member, killed with its process group |
//! | `mcbench-world` | [`PlaceCommands`] in, [`Blueprint`] out, [`BlockState`] for [`EngineProgram::fixed_palette`] | this crate reads and writes | the caller's |
//! | tools and agent | [`EngineWorld`] for the live world, [`SettlePool`] and [`Settles`] for settles, [`VerifyEngines`] for `test_build` | `mcbench-tools` and `mcbench-agent` call this crate | `async fn`s on the tokio runtime; the work runs on engine threads or in children |
//! | the app's panic hook | [`on_engine_thread`]: true on the live world's engine threads, whose refusals are panics they catch | the binary calls it | any |
//! | ui | [`Ticked`] patches and entities for the live view, through the core | the core forwards ticks to the renderer and the interface | engine thread to render thread |
//!
//! Errors are [`EngineError`]: its `Display` is the message today's
//! `engine.js` rejects with (`The engine failed: <reason>`, `The engine
//! stopped (<signal> | code <n>): <stderr's tail>`, `The engine was
//! stopped`), which the verifier and the settle wrap into text models read,
//! so it is kept byte for byte.
//!
//! ## Behaviour defined by
//!
//! Class 2 and the engine half of class 1 (docs/native-app.md, Appendix
//! A): `engine.test.js` 9, `engines.test.js` 1, `core-survive.test.js` 1,
//! `settle-ticks-minecraft.test.js` 1, `engine-verify.test.js` 8 (with
//! `mcbench-tools`), `import-data-version.test.js` 2, `settle-eviction.test.js`
//! 2, `settle-pool.test.js` 14, and with `mcbench-agent` `settle.test.js` 7,
//! `settle-flow.test.js` 7, `settle-flow.integration.test.js` 2 and
//! `settle-ticks.test.js` 7: 61 tests in 12 files. `core-protocol.test.js`
//! (1) stays with the protocol in `mcbench-headless`. The engine's own
//! behaviour is `core/`'s, checked by `core-parity`, `core-verify-parity`,
//! `movement-parity`, `datafix-parity` and `core-probe-parity`.
//!
//! | Today's file | Ported to | Not ported, and why |
//! |---|---|---|
//! | `engine.test.js` (9) | `tests/child.rs` 5 (a failed build and the next warm in the start case, the two stops while building in the stop while starting), `src/child.rs` 1 (the whitespace refusal), `tests/engine.rs` 1 (placements and experiments on the real program), `tests/start.rs` 1 (the in-process engine's start) | a shared build's aborted signal: the child is the app's own binary, nothing is built |
//! | `engines.test.js` (1) | `tests/engine.rs` 1 | |
//! | `core-survive.test.js` (1) | `tests/engine.rs` 1 | |
//! | `settle-ticks-minecraft.test.js` (1) | `tests/engine.rs` 1 | |
//! | `import-data-version.test.js` (2) | `tests/engine.rs` 2 | the import order of the commands is `live-placement.js`'s (`mcbench-world`): the engine half is the fixed palette and the verifier's import |
//! | `settle-eviction.test.js` (2) | `tests/pool.rs` 2 | |
//! | `settle-pool.test.js` (14) | `tests/pool.rs` 14 | |
//! | (what the children are for) | `tests/pool.rs` 3 (a child killed from outside mid-settle, one past the wall budget, one past a memory limit), `tests/leftover.rs` 1 (no child outlives a test process that panics or aborts mid-run), `mcbench-tools`' `tests/engine_children_test.rs` 2 (a dense test cancelled, and one past its wall budget: the lane free at once) | |
//! | `engine-verify.test.js` (8) | `tests/engine.rs` 6 | the run folders' sweep and the verifier pool's memory reading are `EngineVerifier`'s and `VerifierPool`'s (`mcbench-tools`) |
//! | `core-protocol.test.js` (2) | `mcbench-lite`'s `tests/engine_protocol.rs` 2 | |
//!
//! Tests that need Minecraft's tables run when `MCBENCH_TABLES` names their
//! folder (`core/minecraft/1.21.10`) at build time, and are ignored with the
//! reason otherwise; those that start children also need
//! `MCBENCH_ENGINE_PROGRAM` (a built `mcbench-lite`, the program the
//! children are) at build time; those that also generate their fixtures
//! with Node need `MCBENCH_DIFF_NODE` (the `node` program; run from a
//! checkout whose `node_modules` Node finds, or with `NODE_PATH`), and fail
//! when it is set and Node fails. The tests that fake a child with a shell
//! script are Unix only.
//!
//! ## Decided
//!
//! Where the protocol loop's rules live (`native/README.md`, Decisions of
//! stage three). `core/src/main.rs` holds two rules the in-process binding
//! needs as well: after a request that panicked, a world takes only fresh
//! placements ([`SPENT`]), and block code runs on a thread with a 1 GiB
//! stack. Nothing under `core/` changes in stage three: this crate repeats
//! the two rules ([`WorldSession`] exists only on an engine thread), and a
//! test (`tests/equivalence.rs` of `mcbench-lite`) drives the same request
//! sequences through this binding and through `core/src/main.rs`'s own loop
//! (which `mcbench-headless` compiles in with `include!`) and requires the
//! same replies and bytes.
//!
//! Where `test_build` and settles run (`native/README.md`, Decisions of
//! stage three): in children, because an in-process `VERIFY` or settle
//! cannot be stopped (nothing under `core/` may change to give it a stop
//! flag) and an engine out of memory would end the app. A child is today's
//! engine process: started warm, its requests as `engine.js` writes them
//! (files relative to its own folder, so no path holds a space), stopped
//! with `SIGTERM` and then `SIGKILL` after 10 s, killed at once on a cancel
//! and on the settle's wall budget, and replaced when it died. The data
//! fixer's palette runs in a child of its own (`--datafix`), as
//! `fixedPalette` runs the program today, so the verifier needs no
//! in-process engine.
//!
//! What replaces each of the in-process world's recovery paths. A request
//! that panics is caught and spends the world until a fresh placement, as
//! the released program does; work a caller gave a world that panics
//! outside the engine's requests ends that world's thread, which answers
//! [`EngineError::Stopped`] from then on. Out of memory in the live world
//! ends the whole app; in a child it ends that child alone.
//!
//! The engine's version and speed reports. The engine is compiled into the
//! app, so the app's own version names it; `sourcesHash` and the released
//! program's pin stay with the parity scripts and the benchmark, which drive
//! the released program. The replies' milliseconds (`MCBENCH_PLACED <id>
//! <ms>`) were never read by the server, and the settle's timings are the
//! caller's to take, so neither is part of this API.
//!
//! `server/native-runtime-version.js` and `server/native-speed.js` are the
//! Paper verifier's (the plugin's sources per Minecraft version and its
//! physics guard agent), not the engine's: nothing here replaces them.
//!
//! Threads. `mcbench_core::boot`'s registries are not `Sync` (their item
//! components hold `Rc`s) and `boot` leaks what it makes, so every engine
//! thread boots its own once and runs one task at a time (`threads.rs`): a
//! world holds a thread for its life, and a thread whose task ended waits
//! for the next. The registries kept number the most worlds that ever ran
//! at once.
//!
//! The in-process engine's build. A path dependency is built with this
//! workspace's profile, so `native/Cargo.toml` has `core/Cargo.toml`'s
//! release profile (`lto = true`, `codegen-units = 1`, `opt-level = 3`,
//! `panic = "unwind"`). `core/pgo/`'s profiles do not apply to it: cargo
//! gives `mcbench_core` another crate hash in this workspace, so none of the
//! profile's function names match (without a profile the engine ticks about
//! a quarter slower). A profile recorded on this workspace's build with
//! `scripts/core-pgo.js`'s workload run on `mcbench-lite headless engine`
//! (instrumented) applies to the headless binary and to the in-process
//! engine alike, and with it both are as fast as the released program
//! (measured on adipc; the stage-three engine report has the table).
//!
//! Children on Windows. Each child runs in a Job object of its own, made
//! with kill-on-job-close: a stop, a cancel, the wall budget and a dropped
//! handle end the job (`TerminateJobObject`, exit code 1), so the child's
//! whole tree goes at once (there is no `SIGTERM` to ignore, so a stop
//! never waits the ten seconds), and when the app dies however it dies the
//! system closes its job handles and kills every child (Unix children watch
//! their parent instead). A child this process ended reads the signal it
//! was sent, "SIGTERM" or "SIGKILL", as libuv reports a process its `kill`
//! ended, so the sentences are Unix's. The one difference: a child that
//! ended otherwise (by itself, or killed from outside) has no signal on
//! Windows and reads "code N", N its exit code (`taskkill /F` gives "code 1"
//! where Unix's `kill -KILL` gives "SIGKILL").
//!
//! ## Open
//!
//! Where the workspace's profile lives: recorded per target (6.1 MB for
//! x86_64 Linux) and given to the release build with `--config
//! build.rustflags=['-Cprofile-use=<path>']`, either committed beside the
//! workspace (as `core/pgo/` is) or recorded by the release pipeline of
//! stage five. Settled by the stage-three session; until the default of the
//! parity scripts and the benchmark moves, they drive the released program.

mod child;
mod keep;
mod pool;
mod threads;
mod verify;
mod world;

pub use child::{
    CHILD_FOLDER, ChildEngine, EngineProgram, TickFiles, VerifyReport, VerifyRequest,
    sweep_children,
};
pub use keep::{KEPT_CONVERSATIONS, KEPT_WORLDS, KeptSettle, Settles, SettlesKey, check_ticks};
pub use mcbench_world::{BlockState, Blueprint, PlaceCommands};
pub use pool::{
    Lease, MAX_MEMBERS, MEMBER_BYTES, Member, Pool, SettlePool, Settled, pool_size, settle_on,
};
pub use threads::on_engine_thread;
pub use verify::VerifyEngines;
pub use world::{EngineWorld, SPENT, Ticked, WorldSession};

use std::path::Path;
use std::sync::Mutex;

/// The stack of every thread that runs block code: 1 GiB, as
/// `core/src/main.rs` gives its loop (block code recurses as deep as
/// Minecraft's, 512 nested block sets).
pub const ENGINE_STACK: usize = 1 << 30;

/// Minecraft's data version the engine ports (`mcbench_core::verify::DATA_VERSION`,
/// `server/assets.js` `MC_DATA_VERSION`).
pub const DATA_VERSION: i64 = mcbench_core::verify::DATA_VERSION;

/// Why a request of the engine did not give its answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EngineError {
    /// The engine answered the request with a failure: its reason, worded as
    /// `core/src/main.rs` replies (`unported: ...`, `mcbench-core failed:
    /// ...`, or Minecraft's exception). Shown as `engine.js` rejects: "The
    /// engine failed: \<reason\>".
    Failed(String),
    /// The work never reached an answer: the child exited ("The engine
    /// stopped (SIGKILL): ..."), the world, the child or the pool was
    /// stopped, a job ran past its budget, or work given to a world
    /// panicked. The message is whole.
    Stopped(String),
    /// The caller cancelled the request (a child is stopped by it).
    Cancelled,
}

impl std::fmt::Display for EngineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EngineError::Failed(reason) => write!(f, "The engine failed: {reason}"),
            EngineError::Stopped(message) => f.write_str(message),
            EngineError::Cancelled => f.write_str("The engine stopped (cancelled)"),
        }
    }
}

impl std::error::Error for EngineError {}

/// The in-process engine of the process, for the live world: Minecraft's
/// tables read once (`mcbench_core::tables::set`, which may be called only
/// once per process) and the registries booted (`mcbench_core::boot`), once
/// on each engine thread (see `threads.rs`: the registries are not `Sync`).
/// It holds nothing itself: it is the proof the tables were given, copied
/// freely.
#[derive(Clone, Copy)]
pub struct Engine {
    _started: (),
}

static STARTED: Mutex<bool> = Mutex::new(false);

impl Engine {
    /// Reads Minecraft's tables from `tables` (`core/minecraft/<version>/`
    /// or the shipped data) and boots the registries. A table that cannot be
    /// read fails with the released program's sentence ("mcbench-core:
    /// cannot read Minecraft's table \<path\>: \<error\>") and leaves nothing
    /// set, so a later call may try again; a second call after one that
    /// worked fails, as the tables can be given only once.
    pub fn start(tables: &Path) -> Result<Engine, String> {
        let mut started = STARTED.lock().unwrap();
        if *started {
            return Err(
                "The engine was started already: Minecraft's tables are given once per process"
                    .into(),
            );
        }
        let file = |name: &str| {
            let path = tables.join(name);
            std::fs::read_to_string(&path).map_err(|error| {
                format!(
                    "mcbench-core: cannot read Minecraft's table {}: {error}",
                    path.display()
                )
            })
        };
        let text = mcbench_core::tables::Text {
            blocks: file("blocks.txt")?,
            loot: file("loot.json")?,
            datafix: file("datafix.txt")?,
        };
        mcbench_core::tables::set(text);
        // A refusal is an answer, not a crash: the released program's hook
        // (core/src/main.rs) prints every other panic and no refusal.
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if !mcbench_core::is_refusal(info.payload()) {
                previous(info);
            }
        }));
        *started = true;
        // The first engine thread boots now, so a table the registries cannot
        // read fails here; it waits for the next task after.
        let (answer, answered) = std::sync::mpsc::channel();
        threads::run(Box::new(move |_booted| {
            let _ = answer.send(());
        }))
        .map_err(|error| error.to_string())?;
        answered
            .recv()
            .map_err(|_| "The engine's registries could not be read from Minecraft's tables (see the panic above)".to_string())?;
        Ok(Engine { _started: () })
    }

    /// A new, empty live world on an engine thread of its own.
    pub fn world(&self) -> Result<EngineWorld, EngineError> {
        EngineWorld::new(*self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_error_reads_as_engine_js_rejects() {
        assert_eq!(
            EngineError::Failed("unported: a block".into()).to_string(),
            "The engine failed: unported: a block"
        );
        assert_eq!(
            EngineError::Stopped("The engine pool was stopped".into()).to_string(),
            "The engine pool was stopped"
        );
    }
}
