//! # mcbench-lite (library)
//!
//! The binary's library half: the core ([`core`]), so the binary's `main`
//! and the crate's tests build the same one.
//!
//! ## Owner
//!
//! The stage-three session.
//!
//! ## Seams
//!
//! | Seam | What crosses | Who calls whom | Thread or process |
//! |---|---|---|---|
//! | window | `mcbench_ui::Command` in, `mcbench_ui::Update` out; the route methods of [`core::Core`] | the interface calls `Core::send` and the methods; the core calls `Window::publish` | the interface's thread hands over; the core's runtime threads publish |
//! | crash hook | [`core::threads::handled`] (`crash::install`'s filter): true on engine threads and on the core's runtime threads | the interface's hook calls it | any |
//! | the crates under it | `mcbench-agent`'s `Turns`, `mcbench-store`, `mcbench-setup`, `mcbench-tools`, `mcbench-providers`, `mcbench-engine`, `mcbench-sandbox` | the core calls them | the core's runtime; SQLite and the files on its blocking pool |
//!
//! ## Behaviour defined by
//!
//! `server/index.js` (every route, `ROUTES.md`), `turn-stream.js` (through
//! `Turns`), `server-fatal.test.js` (`tests/core_fatal.rs`),
//! `server-archive.test.js`, `server-turns.test.js`, `resolution.test.js`,
//! `provider-login.test.js`, `attachment-serving.test.js`: the cases whose
//! subject is a route, ported in `tests/core_routes.rs` and `src/core/`'s
//! unit tests (each names its Node test); the turn route's events and rows
//! against Node's (`tests/core_replay.rs`), the reading routes' bodies
//! against Node's (`tests/core_reads.rs`), and the bench and prompt
//! library routes' answers and rows against Node's through the window's
//! commands (`tests/core_bench_diff.rs`).
//!
//! ## Decided
//!
//! The core owns its tokio runtime and nothing outside it starts one: the
//! window's executor and tokio never meet (`native/README.md`, Decisions of
//! stage three). A panic on the runtime is fatal for the core (today's
//! `exitOnFatal`), not an abort of the app: the window shows it and offers
//! to quit. A bench's details are pushed, not polled: the core publishes
//! `Update::Bench` for every bench whose details the window asked for
//! after each change of them (`core/bench.rs`), because in one process the
//! core knows each change as it happens.
//! The live view's renderer is the drawing crate, `mcbench_draw::live::Draw`
//! on the hardware adapter, started once on the blocking pool when setup
//! has the assets (`core/renderer.rs` `draw`, `core/live/`
//! `LiveParts::default`): one codepath, and a failed start is
//! `Update::LivePane(Err)` with the renderer's sentence. The tools draw
//! `see_build`'s pictures with the same `Draw` (`LiveParts::shared`, given
//! the live world's renderer when it starts): a call before the start
//! waits for it, one after a failed start answers today's
//! `Screenshot initialization: <the renderer's sentence>`.
//!
//! Share (`server/share.js`, `share-viewer.js`): the core's five share
//! commands over `mcbench_tools::share` and `share_viewer` (`core/share.rs`).

pub mod core;
