//! The CLI harnesses end to end: today's tests of `claude-code.js`,
//! `codex-cli.js`, `harness.js` and `mcp.js` that run a CLI, against the
//! scripted fake CLI (`tests/fakes/fake_cli.py`, which needs `python3`), and
//! the Rust side of the Node/Rust differential (`MCBENCH_DIFF_HARNESS`). On
//! Windows, Claude Code's harness against a stand-in written in Rust
//! (`tests/windows`).
//!
//! The binary has its own `main` (`harness = false`): started as
//! `<this test> tool-bridge <socket>` it is the bridge's relay, which the
//! fake CLI starts as a real CLI starts `mcbench-lite tool-bridge`; libtest
//! would print to the relay's stdout. Otherwise it runs the tests whose
//! names contain the first argument that is not a flag.
//!
//! `MCBENCH_DIFF_HARNESS=<dir>`: runs every case of `<dir>/cases.json`
//! (written by `tests/diff/cases.py`) and writes `<dir>/rust/<case>/out.json`;
//! `<dir>/codex-login` is the fake Codex login. A missing file fails.

#[cfg(any(target_os = "linux", windows))]
mod codex_proc;
mod dump;
mod scratch;
#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

fn main() {
    #[cfg(unix)]
    unix::main();
    #[cfg(windows)]
    windows::main();
}
