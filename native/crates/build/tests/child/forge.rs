//! A compromised child: forged frames, crashes and hangs, and the failure
//! `runProgram` makes of each (the parent's half of build-sandbox.test.js's
//! "a forged result from a compromised build process is rejected before
//! touching the world"; the change-set half, `applyChanges`, is
//! `mcbench_world::transfer`'s). Also the wall clock and cancellation.

use std::ffi::OsStr;
use std::io::Write;
use std::path::Path;
use std::time::{Duration, Instant};

use mcbench_build::process::ChildProcess;
use mcbench_build::{BuildRequest, ChildSandbox, V8Runner, WALL_TIMEOUT, runner::CANCELLED};
use serde_json::json;
use tokio_util::sync::CancellationToken;

use crate::common::{EMPTY_WORLD, Host, Test, test, test_binary};

/// The variable that makes this test binary the forged child of its value.
pub const FORGE: &str = "MCBENCH_BUILD_FORGE";

/// This test binary, unconfined, as the forged child of its kind.
struct Forge(&'static str);

impl ChildSandbox for Forge {
    fn spawn(&self, program: &Path, env: &[(&str, &OsStr)]) -> std::io::Result<ChildProcess> {
        let mut env = env.to_vec();
        env.push((FORGE, OsStr::new(self.0)));
        ChildProcess::start(std::process::Command::new(program), &env)
    }

    fn spare_during_build(&self) -> bool {
        false
    }
}

fn frame(kind: u8, payload: &[u8]) -> Vec<u8> {
    let mut out = vec![kind];
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(payload);
    out
}

fn done(meta: &str, changes: &[u8]) -> Vec<u8> {
    let mut payload = (meta.len() as u32).to_le_bytes().to_vec();
    payload.extend_from_slice(meta.as_bytes());
    payload.extend_from_slice(changes);
    frame(2, &payload)
}

/// The forged child of `kind`.
pub fn child(kind: &str) -> ! {
    let mut out = std::io::stdout().lock();
    let mut write = |bytes: &[u8]| {
        out.write_all(bytes).unwrap();
        out.flush().unwrap();
    };
    match kind {
        "ok" => write(&done(
            r#"{"logs":["fine"],"files":[{"path":"a.txt","text":"é"}],"touched":2}"#,
            b"CHANGES",
        )),
        "invalid-logs" => write(&done(r#"{"logs":[1],"files":[],"touched":0}"#, b"")),
        "long-log" => write(&done(
            &format!(
                r#"{{"logs":["{}"],"files":[],"touched":0}}"#,
                "x".repeat(315)
            ),
            b"",
        )),
        "invalid-result" => write(&done(r#"{"logs":[],"files":[],"touched":"many"}"#, b"")),
        "too-many-files" => {
            let files: Vec<_> = (0..17)
                .map(|i| json!({ "path": format!("f{i}"), "text": "" }))
                .collect();
            write(&done(
                &json!({ "logs": [], "files": files, "touched": 0 }).to_string(),
                b"",
            ))
        }
        "invalid-failure" => write(&frame(3, br#"{"stack":1,"message":"m","logs":null}"#)),
        "failure" => write(&frame(
            3,
            br#"{"stack":"","message":"thrown 42","logs":["before"]}"#,
        )),
        "unexpected" => write(&frame(9, b"")),
        "read-unoffered" => write(&frame(1, b"x.txt")),
        "too-large" => {
            let mut head = vec![2u8];
            head.extend_from_slice(&(300u32 << 20).to_le_bytes());
            write(&head);
            let zeros = vec![0u8; 1 << 20];
            for _ in 0..257 {
                write(&zeros);
            }
        }
        "crash" => {
            eprintln!("something went wrong\nlast words");
            std::process::exit(3)
        }
        "oom" => {
            // As the build child ends on out-of-memory (its `on_oom`): the
            // message, then `_exit(134)`, not an abort the system's crash
            // reporter would record on every test run.
            eprintln!(
                "FATAL ERROR: Reached heap limit Allocation failed - JavaScript heap out of memory"
            );
            // SAFETY: `_exit` takes a status and ends the process; it never returns.
            unsafe { libc::_exit(134) }
        }
        "silent" => std::process::exit(0),
        "hang" => std::thread::sleep(Duration::from_secs(60)),
        other => panic!("unknown forge {other}"),
    }
    std::process::exit(0)
}

async fn forged(
    kind: &'static str,
    read_file: bool,
    cancel: &CancellationToken,
) -> Result<mcbench_build::EncodedOutcome, mcbench_build::BuildFailure> {
    let runner = V8Runner::new(test_binary(), Box::new(Forge(kind)));
    let request = BuildRequest {
        source: String::new(),
        registry: json!({}),
        read_file,
        settled_ticks: None,
    };
    let mut host = Host {
        answers: read_file.then(|| json!({})),
        settled: None,
        reads: Default::default(),
        settled_calls: Default::default(),
    };
    runner
        .run_encoded(&request, &EMPTY_WORLD, &mut host, cancel)
        .await
}

async fn message(kind: &'static str) -> String {
    forged(kind, false, &CancellationToken::new())
        .await
        .unwrap_err()
        .message
}

pub fn tests() -> Vec<Test> {
    let malformed =
        |reason: &str| format!("Error: Program process returned a malformed result: {reason}");
    vec![
        test(
            "forge: a well-formed answer passes through with its change set",
            || async {
                let outcome = forged("ok", false, &CancellationToken::new())
                    .await
                    .unwrap();
                assert_eq!(outcome.logs, ["fine"]);
                assert_eq!(outcome.files[0].text, "é");
                assert_eq!(outcome.touched, 2);
                assert_eq!(outcome.changes, b"CHANGES");
            },
        ),
        test(
            "forge: forged results are refused with runProgram's reasons",
            move || async move {
                assert_eq!(message("invalid-logs").await, malformed("invalid logs"));
                assert_eq!(message("long-log").await, malformed("invalid logs"));
                assert_eq!(message("invalid-result").await, malformed("invalid result"));
                assert_eq!(message("too-many-files").await, malformed("invalid result"));
                assert_eq!(
                    message("invalid-failure").await,
                    malformed("invalid failure")
                );
                assert_eq!(message("unexpected").await, malformed("unexpected frame 9"));
                assert_eq!(
                    message("read-unoffered").await,
                    malformed("unexpected frame 1")
                );
                assert_eq!(message("too-large").await, malformed("result too large"));
            },
        ),
        test(
            "forge: a failure without a stack is its message, with its logs",
            || async {
                let failure = forged("failure", false, &CancellationToken::new())
                    .await
                    .unwrap_err();
                assert_eq!(failure.message, "thrown 42");
                assert_eq!(failure.logs, ["before"]);
            },
        ),
        test(
            "forge: a process that dies is reported with its last stderr line or as out of memory",
            || async {
                assert_eq!(
                    message("crash").await,
                    "Error: Program process exited unexpectedly: last words"
                );
                assert_eq!(
                    message("silent").await,
                    "Error: Program process exited unexpectedly"
                );
                assert_eq!(
                    message("oom").await,
                    "Error: Program ran out of memory (limit 2048 MB) and was terminated"
                );
            },
        ),
        test(
            "forge: a process past the wall clock is killed with today's message",
            || async {
                let started = Instant::now();
                assert_eq!(
                    message("hang").await,
                    "Error: Program process exceeded 10000 ms and was terminated"
                );
                assert!(
                    started.elapsed() >= WALL_TIMEOUT
                        && started.elapsed() < WALL_TIMEOUT + crate::common::AT_ONCE
                );
            },
        ),
        test(
            "forge: a cancelled run kills the process at once",
            || async {
                let cancel = CancellationToken::new();
                let later = cancel.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_millis(200)).await;
                    later.cancel();
                });
                let started = Instant::now();
                let failure = forged("hang", false, &cancel).await.unwrap_err();
                assert_eq!(failure.message, CANCELLED);
                assert!(started.elapsed() < crate::common::AT_ONCE);
            },
        ),
    ]
}
