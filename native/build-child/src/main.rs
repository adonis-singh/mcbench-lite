//! `mcbench-build-child`: the V8 child of one `build` call, shipped beside
//! the app (`mcbench_build::child_program` names it) and started by
//! `mcbench_build::V8Runner` under the platform sandbox. It reads one
//! request from stdin, runs the program with today's files and answers with
//! today's frames on stdout (the protocol is `mcbench_build`'s seam, in its
//! crate documentation), then exits. It installs no panic filter and opens
//! no window: those are the app's (`mcbench-lite app`).
//!
//! Its own workspace (`Cargo.toml` says why). Tests: `tests/protocol.rs`
//! (recorded frames of today's `server/build-child.js`, `tests/frames/`);
//! the rest run against this program from the main workspace
//! (`mcbench-build`'s `tests/child`, with `MCBENCH_BUILD_CHILD` naming it).

mod child;

fn main() {
    child::run()
}
