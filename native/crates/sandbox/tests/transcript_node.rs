//! The transcript against Node's own `formatResult` and `truncateMiddle`
//! (`contract/generate.js format`), on generated cases, and the generated
//! contract files against `server/` (`generate.js --check`). Needs
//! `MCBENCH_SANDBOX_NODE=1` (Node on `PATH`, this checkout's `server/`).
//! Run with `cargo test -p mcbench-sandbox --test transcript_node -- --ignored`.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use mcbench_sandbox::transcript::{
    RESTRICTED_NOTE, SESSION_RESET_NOTE, format_result, truncate_middle,
};
use serde_json::{Value, json};

fn generate() -> std::path::PathBuf {
    assert_eq!(
        std::env::var("MCBENCH_SANDBOX_NODE").as_deref(),
        Ok("1"),
        "MCBENCH_SANDBOX_NODE=1 is needed"
    );
    Path::new(env!("CARGO_MANIFEST_DIR")).join("contract/generate.js")
}

/// A small deterministic generator (xorshift).
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

fn body(rng: &mut Rng) -> String {
    let pieces = [
        "a",
        "line\n",
        "é",
        "😀",
        "\n",
        "中文",
        "\t",
        "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
        "\u{FFFD}",
        "\n\n",
    ];
    let target = [
        0, 10, 9_999, 10_000, 10_001, 19_999, 20_000, 20_001, 25_000, 60_000,
    ][rng.below(10) as usize];
    let mut out = String::new();
    while out.len() < target {
        out.push_str(pieces[rng.below(pieces.len() as u64) as usize]);
    }
    out
}

#[test]
#[ignore = "needs MCBENCH_SANDBOX_NODE=1"]
fn the_transcript_is_nodes_format_result_byte_for_byte() {
    let script = generate();
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let mut cases = Vec::new();
    for i in 0..600 {
        let text = body(&mut rng);
        if i % 3 == 0 {
            cases.push(json!({ "kind": "truncate", "text": text, "maxBytes": ([20_000, 100, 7, 1][rng.below(4) as usize]) }));
        } else {
            let durations = [0, 49, 50, 149, 150, 250, 1049, 1050, 9_999, 123_456];
            let ms = [1000.0, 10_000.0, 1500.5, 120_000.0, 2500.25][rng.below(5) as usize];
            let note = [
                Value::Null,
                json!(SESSION_RESET_NOTE),
                json!(RESTRICTED_NOTE),
            ][rng.below(3) as usize]
                .clone();
            cases.push(json!({
                "kind": "format", "body": text, "exitCode": ([0, 1, 124, -1, 137][rng.below(5) as usize]),
                "durationMs": durations[rng.below(durations.len() as u64) as usize], "timedOut": rng.below(2) == 1, "ms": ms, "note": note,
            }));
        }
    }
    let mut child = Command::new("node")
        .arg(&script)
        .arg("format")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("node");
    let mut input = String::new();
    for case in &cases {
        input.push_str(&case.to_string());
        input.push('\n');
    }
    let mut stdin = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()).unwrap());
    let out = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(out.status.success());
    let answers: Vec<Value> = String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(answers.len(), cases.len());
    for (case, answer) in cases.iter().zip(&answers) {
        if case["kind"] == "truncate" {
            let cut = truncate_middle(
                case["text"].as_str().unwrap(),
                case["maxBytes"].as_u64().unwrap() as usize,
            );
            assert_eq!(cut.text, answer["out"]["text"].as_str().unwrap());
            assert_eq!(
                cut.total_lines.map(|n| n as u64),
                answer["out"]["totalLines"].as_u64()
            );
        } else {
            let ours = format_result(
                case["body"].as_str().unwrap(),
                case["exitCode"].as_i64().unwrap() as i32,
                case["durationMs"].as_u64().unwrap(),
                case["timedOut"].as_bool().unwrap(),
                case["ms"].as_f64().unwrap(),
                case["note"].as_str(),
            );
            assert_eq!(ours, answer["out"].as_str().unwrap(), "{case}");
        }
    }
}

#[test]
#[ignore = "needs MCBENCH_SANDBOX_NODE=1"]
fn the_contract_files_are_what_server_gives() {
    let script = generate();
    let status = Command::new("node")
        .arg(&script)
        .arg("--check")
        .status()
        .expect("node");
    assert!(
        status.success(),
        "contract/ differs from server/: run node contract/generate.js"
    );
}
