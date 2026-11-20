//! `mcbench-lite headless grade`, the built binary, as the benchmark's
//! thin client drives it: every grader experiment of the verifier's
//! differential (`mcbench-tools`' `tests/node/verify-cases.js`) sent as a
//! request line, its reply and report compared with what today's Node
//! `EngineVerifier` answered (`verify-node.js`) by `verify-compare.js`.
//!
//! The differential's out folder `MCBENCH_VERIFY_DIFF` (with `cases.jsonl`
//! and `node/`, made by `mcbench-tools`' `tests/differential.rs`),
//! `MCBENCH_TABLES` and `node` on the PATH turn the comparison on; it is
//! ignored without them and fails when they are set and something is
//! missing. The protocol's refusals need nothing.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn grade(args: &[&str]) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_mcbench-lite"));
    command.args(["headless", "grade"]).args(args);
    command
}

#[test]
fn grade_refuses_a_command_line_without_its_folders() {
    let output = grade(&[]).output().unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "mcbench-lite headless grade: usage: mcbench-lite headless grade <tables folder> <verifier root>\n"
    );
}

#[test]
#[ignore = "needs MCBENCH_VERIFY_DIFF (the verifier differential's out folder), MCBENCH_TABLES and node"]
fn every_grader_experiment_answers_what_node_answered() {
    let folder = |name: &str| {
        let folder = PathBuf::from(std::env::var_os(name).unwrap_or_else(|| panic!("set {name}")));
        assert!(
            folder.is_dir(),
            "{name} names {}, which is not a folder",
            folder.display()
        );
        folder
    };
    let out = folder("MCBENCH_VERIFY_DIFF");
    let tables = folder("MCBENCH_TABLES");
    assert!(
        out.join("node").is_dir(),
        "{} has no node/: run mcbench-tools' differential first",
        out.display()
    );
    let side = out.join("rust-bin");
    let root = out.join("rust-bin-root");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&side).unwrap();
    let mut child = grade(&[tables.to_str().unwrap(), root.to_str().unwrap()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let cases = std::fs::read_to_string(out.join("cases.jsonl")).unwrap();
    let mut sent = 0;
    {
        let mut stdin = child.stdin.take().unwrap();
        for (n, line) in cases.lines().enumerate() {
            let case = mcbench_json::parse(line).unwrap();
            if case["kind"] != "grade" {
                continue;
            }
            let files = side.join(n.to_string());
            let _ = std::fs::remove_dir_all(&files);
            std::fs::create_dir_all(&files).unwrap();
            let mut request = serde_json::Map::new();
            request.insert("id".into(), serde_json::Value::from(n));
            for key in [
                "world",
                "test",
                "placementMode",
                "reportEncoding",
                "budgets",
            ] {
                if let Some(value) = case.get(key) {
                    request.insert(key.into(), value.clone());
                }
            }
            request.insert(
                "reportPath".into(),
                files.join("0.json").to_string_lossy().into_owned().into(),
            );
            writeln!(
                stdin,
                "{}",
                mcbench_json::stringify(&serde_json::Value::Object(request))
            )
            .unwrap();
            sent += 1;
        }
    }
    let mut replies = 0;
    for line in BufReader::new(child.stdout.take().unwrap()).lines() {
        let mut reply = mcbench_json::parse(&line.unwrap()).unwrap();
        let n = reply["id"].as_u64().unwrap();
        reply.as_object_mut().unwrap().shift_remove("id");
        std::fs::write(
            side.join(format!("{n}.json")),
            mcbench_json::stringify(&reply),
        )
        .unwrap();
        replies += 1;
    }
    assert!(child.wait().unwrap().success());
    assert_eq!(replies, sent);
    let _ = std::fs::remove_dir_all(&root);
    let compare =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../tools/tests/node/verify-compare.js");
    let status = Command::new("node")
        .arg(compare)
        .arg(&out)
        .args(["rust-bin", "--kind", "grade", "--json"])
        .arg(out.join("compare-rust-bin.json"))
        .status()
        .unwrap();
    assert!(
        status.success(),
        "headless grade answered differently from Node: see the table above"
    );
}
