//! # mcbench-headless
//!
//! `mcbench-lite headless <family>`: the same binary without a window, for
//! the private benchmark, CI and the parity scripts. Its
//! command line and JSON-lines protocol on stdin and stdout replace the
//! benchmark's `require`s of `server/` modules (the inventory's list:
//! `agent`'s `runTurn`, `executeToolCall`, `toolMessageContent`, `toolsFor`
//! and `toolsForImageInput`, `prompt`'s `systemPrompt`,
//! `verification-contract`'s budgets, the verifiers, `shot` and
//! `shot-world`, `mcp`'s `serveMcp`, the engine, the harness internals and
//! smaller modules), whose JavaScript then becomes a thin client of this
//! protocol (docs/native-app.md, Benchmark and parity scripts).
//!
//! | Family | What it covers | Reads | Writes |
//! |---|---|---|---|
//! | [`Family::Engine`] | place, tick, inputs, verify, dump, as the parity scripts drive the engine today | `core/src/main.rs`'s request lines | its replies and files, byte for byte |
//! | [`Family::Grade`] | `test_build`'s verifier with the benchmark's budgets | [`Request`]s | [`Reply`]s |
//! | [`Family::Turn`] | the model loop: a turn with a provider and a tool set, its events as they stream, one tool call executed, a tool result formatted | [`Request`]s | [`Event`]s, then [`Reply`]s |
//! | [`Family::Contract`] | the tool schemas, system prompt and budgets of every form of this process's budget regime | nothing | one JSON object |
//! | [`Family::Shots`] | `see_build`'s pictures of a world | [`Request`]s | [`Reply`]s, the pictures as files |
//! | [`Family::Setup`] | setup's data and textures steps on a data folder, with no window: the folders `turn` and `shots` name | its data folder (an argument) | one JSON object, progress on stderr |
//!
//! The frame of every family but `engine`, `contract` and `setup`: one JSON object per line. A
//! request has an `id` (a number or a string, unique among those in flight)
//! and its family's fields; each reply repeats the `id` and has `ok: true`
//! with its fields or `error: "<sentence>"`; a `turn` also writes
//! `{ "id", "event" }` lines before its reply. Bytes (blueprints, entities,
//! pictures) cross as files named in the request, as the engine's protocol
//! does. A line that is not JSON, or a request missing a field or carrying
//! one its family does not take, ends the process with status 2 and a line
//! on stderr: the stream can no longer be trusted.
//!
//! ## Owner
//!
//! The stage-three session. The protocol is a seam with the benchmark and
//! the parity scripts: a change to it changes their client in the same
//! commit, and `contract`'s output is pinned by the tool-contract hash.
//!
//! ## Seams
//!
//! | Seam | What crosses | Who calls whom | Thread or process |
//! |---|---|---|---|
//! | the benchmark and parity scripts | this protocol | Node starts `mcbench-lite headless <family>` | a child process of the script |
//! | the core crates | `mcbench-agent` (`run_turn`, `contract`), `mcbench-tools` (`Tool`, the verifier), `mcbench-engine`, `mcbench-render` | [`run`] calls them | the headless process |
//!
//! ## Behaviour defined by
//!
//! `core-protocol.test.js` (1, the engine protocol stays for the parity
//! scripts), the benchmark's tool contract script's sha256 for all 8
//! forms (`efc4d688...`, `mcbench_agent::contract::TOOL_CONTRACT_SHA256`),
//! the benchmark's own tests (about 231; on the stage-one branch 130 failed
//! against 136 on `main`, all also failing on `main` for a missing Python
//! virtual environment and data), and the parity scripts that are ported to
//! this binary: `core-parity`, `render-parity`, `mesher-parity`,
//! `settle-parity`, `datafix-parity`, `movement-parity`,
//! `core-verify-parity`, `core-probe-parity` and `core-bench`
//! (docs/native-app.md, Parity scripts).
//!
//! ## Decided
//!
//! `engine`'s implementation (`native/README.md`, Decisions of stage
//! three): `core/src/main.rs` is compiled into this crate unchanged with
//! `include!` inside a module, and `engine` gives the tables to
//! `mcbench_core::tables` and runs that file's `serve()` on a thread with a
//! 1 GiB stack. The protocol is the released program's source, so the parity
//! scripts drive either with the same replies; nothing under `core/` changes.
//!
//! `contract`'s output (`PROTOCOL.md`): one object with the budgets and, for
//! each of the regime's four benchmark forms, the tools, the system prompt
//! and `attemptContract`'s parts, so the benchmark's client replaces its
//! `require`s of `server/agent`, `server/prompt` and
//! `server/verification-contract` with one call per regime. The tools and
//! budgets are embedded as the bytes the hash covers; `mcbench-lite`'s test
//! `headless_contract` runs the built binary under both regimes and gets
//! `efc4d688...`.
//!
//! `setup` (`PROTOCOL.md`): the data folder is its one argument, not
//! `MCBENCH_DATA_DIR`, because the headless families name their folders
//! themselves (`turn`'s `data`, `grade`'s folders) and a benchmark machine's
//! script must not prepare the platform's default folder by leaving a
//! variable out. The shipped data is the binary's (`shipped` in `main.rs`,
//! the app's own, given to [`run`]). It holds the data folder's
//! one-instance lock while it works: the two steps remove whatever else is
//! in `minecraft/` and `textures/`, which would be a running app's.
//!
//! ## Open
//!
//! The fields of each family's requests and replies: settled by the
//! stage-three session from what the benchmark's thin client needs, which
//! is what the benchmark reads of today's modules. They are listed in
//! `native/crates/headless/PROTOCOL.md` when the first family is built, and a
//! change to them is a change to that file and the client in one commit.

use serde::{Deserialize, Serialize};

mod contract;
mod engine;
mod grade;
pub mod providers;
pub mod renderer;
pub mod setup;
pub mod shell;
mod shots;
mod turn;

/// A command family of `mcbench-lite headless`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Family {
    /// `engine`: the engine's own protocol.
    Engine,
    /// `grade`: `test_build`'s verifier.
    Grade,
    /// `turn`: the model loop.
    Turn,
    /// `contract`: the tool contract's parts.
    Contract,
    /// `shots`: `see_build`'s pictures.
    Shots,
    /// `setup`: the shipped data and the textures in a data folder.
    Setup,
}

impl Family {
    /// The family named on the command line.
    pub fn parse(word: &str) -> Result<Family, String> {
        match word {
            "engine" => Ok(Family::Engine),
            "grade" => Ok(Family::Grade),
            "turn" => Ok(Family::Turn),
            "contract" => Ok(Family::Contract),
            "shots" => Ok(Family::Shots),
            "setup" => Ok(Family::Setup),
            other => Err(format!(
                "unknown headless family {other} (engine, grade, turn, contract, shots or setup)"
            )),
        }
    }
}

/// A request line: its `id` and its family's fields (see Open).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Request {
    /// The request's id.
    pub id: serde_json::Value,
    /// The family's fields.
    #[serde(flatten)]
    pub fields: serde_json::Map<String, serde_json::Value>,
}

/// A reply line.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Reply {
    /// The request's id.
    pub id: serde_json::Value,
    /// `ok: true` with the fields, or `error` with its sentence.
    #[serde(flatten)]
    pub outcome: serde_json::Map<String, serde_json::Value>,
}

/// An event line of a `turn`: `mcbench_agent::TurnEvent` as JSON, with the
/// `type` names of `agent.js` `emit`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Event {
    /// The request's id.
    pub id: serde_json::Value,
    /// The event.
    pub event: serde_json::Value,
}

/// `mcbench-lite headless <family> [arguments]`: serves the family on stdin
/// and stdout until stdin ends, then exits with status 0. `arguments` are
/// the words after the family (`engine` takes the tables folder and the
/// released program's own flags). `setup` prints its one line instead and
/// unpacks the shipped data with `shipped`, the binary's. Each family lives
/// in a module of its own.
pub fn run(family: Family, arguments: &[String], shipped: setup::Shipped) -> ! {
    match family {
        Family::Contract => contract::run(arguments),
        Family::Engine => engine::run(arguments),
        Family::Grade => grade::run(arguments),
        Family::Turn => turn::run(arguments),
        Family::Shots => shots::run(arguments),
        Family::Setup => setup::run(arguments, shipped),
    }
}

#[cfg(test)]
mod seam {
    use super::*;

    #[test]
    fn families_are_named_as_the_command_line_names_them() {
        for (word, family) in [
            ("engine", Family::Engine),
            ("grade", Family::Grade),
            ("turn", Family::Turn),
            ("contract", Family::Contract),
            ("shots", Family::Shots),
            ("setup", Family::Setup),
        ] {
            assert_eq!(Family::parse(word).unwrap(), family);
        }
        assert!(Family::parse("render").is_err());
    }
}
