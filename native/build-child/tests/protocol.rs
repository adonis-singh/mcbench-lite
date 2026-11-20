//! The child's half of the process protocol (`mcbench_build`'s seam): given
//! each recorded request (`tests/frames/<case>.in`, what the parent writes:
//! the request, the world, the answers to the child's `READ` and `SETTLED`
//! frames), this program writes exactly what today's `server/build-child.js`
//! wrote (`<case>.out`, recorded by `tests/frames/record.js` with Node
//! 26.13.1) and exits 0. The parent's half is `mcbench-build`'s
//! `tests/child/protocol.rs`, on the same files.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

fn frames() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/frames")
}

#[test]
fn the_child_answers_each_recorded_request_with_today_s_bytes() {
    let cases: Vec<serde_json::Value> =
        serde_json::from_slice(&std::fs::read(frames().join("cases.json")).unwrap()).unwrap();
    assert_eq!(cases.len(), 5);
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let input = std::fs::read(frames().join(format!("{name}.in"))).unwrap();
        let expected = std::fs::read(frames().join(format!("{name}.out"))).unwrap();
        let mut child = Command::new(env!("CARGO_BIN_EXE_mcbench-build-child"))
            .env_clear()
            .current_dir("/")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(&input).unwrap();
        let out = child.wait_with_output().unwrap();
        assert!(
            out.status.success(),
            "{name}: {} {}",
            out.status,
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(
            out.stdout,
            expected,
            "{name}: the child wrote {:?}, today's build-child.js {:?}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&expected)
        );
    }
}
