//! The parent's half of the process protocol (the crate's seam), on the
//! frames recorded from today's `server/build-child.js`
//! (`native/build-child/tests/frames/`, `record.js`): for each case the
//! runner writes exactly the recorded request and answers (`<case>.in`) to a
//! child that replays the recorded reply (`<case>.out`), and what it makes
//! of that reply is what it makes of the real `mcbench-build-child`'s under
//! the platform sandbox. The child's half (the real child answers each
//! `.in` with exactly the `.out`) is `native/build-child/tests/protocol.rs`.

use std::ffi::OsStr;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use mcbench_build::process::ChildProcess;
use mcbench_build::{BuildRequest, ChildSandbox, V8Runner};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use crate::common::{EMPTY_WORLD, Host, Test, crate_dir, program, test, test_binary};

/// The variable that makes this test binary replay the recorded reply of
/// the case it names.
pub const REPLAY: &str = "MCBENCH_BUILD_REPLAY";

fn frames() -> PathBuf {
    crate_dir().join("../../build-child/tests/frames")
}

fn u32_at(bytes: &[u8], at: usize) -> usize {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as usize
}

/// The replaying child of `case`: reads stdin against `<case>.in` (the
/// request and the world, then the answer to each `READ` and `SETTLED` frame
/// it writes from `<case>.out`) and exits 1 with the byte where the parent
/// wrote something else.
pub fn replay(case: &str) -> ! {
    let input = std::fs::read(frames().join(format!("{case}.in"))).unwrap();
    let output = std::fs::read(frames().join(format!("{case}.out"))).unwrap();
    let mut stdin = std::io::stdin().lock();
    let mut stdout = std::io::stdout().lock();
    let mut at = 0;
    // Reads the next `n` bytes of stdin, which must be `input`'s at `at`.
    let mut expect = |at: &mut usize, n: usize| {
        let mut got = vec![0u8; n];
        let read = stdin.read_exact(&mut got);
        if read.is_err() || got[..] != input[*at..*at + n] {
            eprintln!("the parent wrote other bytes than {case}.in from byte {at}");
            std::process::exit(1);
        }
        *at += n;
    };
    for _ in 0..2 {
        let n = 4 + u32_at(&input, at);
        expect(&mut at, n);
    }
    let mut out = 0;
    while out < output.len() {
        let kind = output[out];
        let end = out + 5 + u32_at(&output, out + 1);
        stdout.write_all(&output[out..end]).unwrap();
        stdout.flush().unwrap();
        out = end;
        let answer = match kind {
            1 => 5 + u32_at(&input, at + 1),
            4 => 4 + u32_at(&input, at),
            _ => continue,
        };
        expect(&mut at, answer);
    }
    if at != input.len() {
        eprintln!("the parent left {case}.in unread from byte {at}");
        std::process::exit(1);
    }
    std::process::exit(0)
}

/// This test binary, unconfined, replaying a case.
struct Replay(String);

impl ChildSandbox for Replay {
    fn spawn(&self, program: &Path, env: &[(&str, &OsStr)]) -> std::io::Result<ChildProcess> {
        let mut env = env.to_vec();
        env.push((REPLAY, OsStr::new(&self.0)));
        ChildProcess::start(std::process::Command::new(program), &env)
    }

    fn spare_during_build(&self) -> bool {
        false
    }
}

/// The case's request and a host that answers as its recorded answers do.
fn case_of(case: &Value) -> (BuildRequest, Host) {
    let r = &case["request"];
    let request = BuildRequest {
        source: r["source"].as_str().unwrap().to_string(),
        registry: r["registry"].clone(),
        read_file: r["readFile"].as_bool().unwrap(),
        settled_ticks: r["settledTicks"].as_u64().map(|t| t as u32),
    };
    let reads: serde_json::Map<String, Value> = case["answers"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|a| {
            a.get("read").map(|path| {
                (
                    path.as_str().unwrap().to_string(),
                    json!({ "ok": a["ok"], "text": a["text"] }),
                )
            })
        })
        .collect();
    let host = Host {
        answers: request.read_file.then(|| Value::Object(reads)),
        settled: Some(EMPTY_WORLD.to_vec()),
        reads: Default::default(),
        settled_calls: Default::default(),
    };
    (request, host)
}

pub fn real_child_tests() -> Vec<Test> {
    vec![test(
        "protocol: the parent writes today's recorded frames and reads today's reply as the real child's",
        || async {
            let cases: Vec<Value> =
                serde_json::from_slice(&std::fs::read(frames().join("cases.json")).unwrap())
                    .unwrap();
            assert_eq!(cases.len(), 5);
            let real = V8Runner::new(program(), mcbench_build::sandbox::platform().unwrap());
            for case in &cases {
                let name = case["name"].as_str().unwrap();
                let (request, mut host) = case_of(case);
                let replayed = V8Runner::new(test_binary(), Box::new(Replay(name.to_string())))
                    .run_encoded(&request, &EMPTY_WORLD, &mut host, &CancellationToken::new())
                    .await;
                let (request, mut host) = case_of(case);
                let answered = real
                    .run_encoded(&request, &EMPTY_WORLD, &mut host, &CancellationToken::new())
                    .await;
                assert_eq!(replayed, answered, "{name}");
                if let Err(failure) = &replayed {
                    assert!(
                        !failure.message.contains("exited unexpectedly"),
                        "{name}: {}",
                        failure.message
                    );
                }
            }
        },
    )]
}
