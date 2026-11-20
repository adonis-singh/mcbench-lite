//! `test/core-protocol.test.js` against `mcbench-lite headless engine`: the
//! released program's protocol loop (`core/src/main.rs`, compiled in by
//! `mcbench-headless`). Needs Minecraft's tables: `MCBENCH_TABLES` names
//! their folder at build time.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn binary() -> &'static str {
    env!("CARGO_BIN_EXE_mcbench-lite")
}

fn tables() -> PathBuf {
    PathBuf::from(
        std::env::var("MCBENCH_TABLES").expect(
            "MCBENCH_TABLES names the folder of Minecraft's tables (core/minecraft/1.21.10)",
        ),
    )
}

fn scratch(name: &str) -> PathBuf {
    let folder =
        std::env::temp_dir().join(format!("mcbench-protocol-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(&folder).unwrap();
    folder
}

// The program reads Minecraft's tables from the folder its first argument
// names, before anything else: without it, or with a table missing, it
// stops at once and says what it lacks. The usage line names this binary's
// command where the released program names itself.
#[test]
#[cfg_attr(
    not(mcbench_tables),
    ignore = "needs Minecraft's tables: set MCBENCH_TABLES at build time"
)]
fn the_program_needs_its_tables_folder_and_names_a_table_it_cannot_read() {
    let alone = Command::new(binary())
        .args(["headless", "engine"])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_eq!(alone.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&alone.stderr)
            .starts_with("usage: mcbench-lite headless engine <tables folder>"),
        "{}",
        String::from_utf8_lossy(&alone.stderr)
    );
    assert!(alone.stdout.is_empty());
    let folder = scratch("tables");
    for name in ["blocks.txt", "loot.json"] {
        std::fs::copy(tables().join(name), folder.join(name)).unwrap();
    }
    let missing = Command::new(binary())
        .args(["headless", "engine"])
        .arg(&folder)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_eq!(missing.status.code(), Some(2));
    assert!(missing.stdout.is_empty());
    let expected = format!(
        "mcbench-core: cannot read Minecraft's table {}: ",
        folder.join("datafix.txt").display()
    );
    assert!(
        String::from_utf8_lossy(&missing.stderr).starts_with(&expected),
        "{}",
        String::from_utf8_lossy(&missing.stderr)
    );
    std::fs::remove_dir_all(&folder).unwrap();
}

struct Engine {
    child: std::process::Child,
    lines: std::io::Lines<BufReader<std::process::ChildStdout>>,
}

impl Engine {
    fn start(folder: &Path) -> Engine {
        let mut child = Command::new(binary())
            .args(["headless", "engine"])
            .arg(tables())
            .current_dir(folder)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let lines = BufReader::new(child.stdout.take().unwrap()).lines();
        Engine { child, lines }
    }

    fn next(&mut self) -> String {
        self.lines.next().unwrap().unwrap()
    }

    fn ask(&mut self, line: &str) -> String {
        writeln!(self.child.stdin.as_mut().unwrap(), "{line}").unwrap();
        self.next()
    }
}

fn matches_done(reply: &str, verb: &str, id: u32) -> bool {
    let prefix = format!("{verb} {id} ");
    reply
        .strip_prefix(&prefix)
        .is_some_and(|ms| !ms.is_empty() && ms.bytes().all(|b| b.is_ascii_digit()))
}

// The native engine's protocol (core/src/main.rs) on requests in the wrong
// order or malformed: each fails alone, with a reply, and the world goes on.
#[test]
#[cfg_attr(
    not(mcbench_tables),
    ignore = "needs Minecraft's tables: set MCBENCH_TABLES at build time"
)]
fn requests_out_of_order_or_malformed_fail_with_a_reply_and_refuse_nothing() {
    let folder = scratch("requests");
    let file = |name: &str| folder.join(name).display().to_string();
    std::fs::write(
        folder.join("commands.txt"),
        "setblock 0 64 0 minecraft:stone\n",
    )
    .unwrap();
    let mut engine = Engine::start(&folder);
    assert_eq!(engine.next(), "MCBENCH_ENGINE_READY");
    let state = |engine: &mut Engine, id: u32| {
        assert!(matches_done(
            &engine.ask(&format!("STATE {id} {}", file("state.txt"))),
            "MCBENCH_DONE",
            id
        ));
        std::fs::read_to_string(folder.join("state.txt")).unwrap()
    };
    assert!(state(&mut engine, 1).lines().any(|line| line == "time 0"));
    // A tick before any base fails after its ticks ran, where McbenchEngine.tick
    // meets the null changes: the world's time went on.
    assert_eq!(
        engine.ask(&format!("TICK 2 5 {} {}", file("p"), file("e"))),
        "MCBENCH_TICK_FAILED 2 java.lang.NullPointerException: no base written"
    );
    assert!(state(&mut engine, 3).lines().any(|line| line == "time 5"));
    let ptick = engine.ask(&format!(
        "PTICK 4 0 0 0 {} {} {}",
        file("p"),
        file("e"),
        file("v")
    ));
    assert!(
        ptick.starts_with("MCBENCH_TICK_FAILED 4 ") && ptick.contains("PTICK before PSPAWN"),
        "{ptick}"
    );
    assert_eq!(
        engine.ask(&format!("DUMP 5 {}", file("d"))),
        "MCBENCH_FAILED 5 unknown request DUMP"
    );
    assert_eq!(
        engine.ask("NOPE 6"),
        "MCBENCH_FAILED 6 unknown request NOPE"
    );
    assert!(
        engine
            .ask("PLACE")
            .starts_with("MCBENCH_PLACE_FAILED ? malformed request")
    );
    assert!(
        engine
            .ask(&format!("TICK 7 many {} {}", file("p"), file("e")))
            .starts_with("MCBENCH_TICK_FAILED 7 malformed request")
    );
    let missing = engine.ask(&format!(
        "PLACE 8 {} {} fresh",
        file("missing.txt"),
        file("b")
    ));
    assert!(
        missing.starts_with("MCBENCH_PLACE_FAILED 8 ") && missing.contains("missing.txt"),
        "{missing}"
    );
    // None of these refused the world: a continuing placement and its ticks run.
    assert!(matches_done(
        &engine.ask(&format!(
            "PLACE 9 {} {} continue",
            file("commands.txt"),
            file("b")
        )),
        "MCBENCH_PLACED",
        9
    ));
    assert!(matches_done(
        &engine.ask(&format!("TICK 10 2 {} {}", file("p"), file("e"))),
        "MCBENCH_TICKED",
        10
    ));
    assert!(matches_done(
        &engine.ask("PSPAWN 11 0.5 65 0.5 0 0 1"),
        "MCBENCH_DONE",
        11
    ));
    assert!(matches_done(
        &engine.ask(&format!(
            "PTICK 12 0 0 0 {} {} {}",
            file("p"),
            file("e"),
            file("v")
        )),
        "MCBENCH_TICKED",
        12
    ));
    assert_eq!(std::fs::metadata(folder.join("v")).unwrap().len(), 28 * 8);
    // A fresh placement replaces the world before it reads its commands: one
    // whose file is missing leaves a world without a base.
    let missing = engine.ask(&format!(
        "PLACE 13 {} {} fresh",
        file("missing.txt"),
        file("b")
    ));
    assert!(
        missing.starts_with("MCBENCH_PLACE_FAILED 13 ") && missing.contains("missing.txt"),
        "{missing}"
    );
    assert_eq!(
        engine.ask(&format!("TICK 14 1 {} {}", file("p"), file("e"))),
        "MCBENCH_TICK_FAILED 14 java.lang.NullPointerException: no base written"
    );
    drop(engine.child.stdin.take());
    assert!(engine.child.wait().unwrap().success());
    std::fs::remove_dir_all(&folder).unwrap();
}
