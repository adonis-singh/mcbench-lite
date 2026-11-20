//! The parent's side of one build: `build-sandbox.js` `runProgram` over the
//! world's encoded bytes.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde_json::Value;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;

use crate::format::{format_failure, format_parent_error, js_trim, replace_lone_surrogates};
use crate::process::{ChildProcess, ChildStdin};
use crate::{BuildFailure, BuildRequest, ChildSandbox, Readiness, WALL_TIMEOUT, WrittenFile};

const READ: u8 = 1;
const DONE: u8 = 2;
const FAIL: u8 = 3;
const SETTLED: u8 = 4;
const MAX_RESULT_BYTES: usize = 256 * 1024 * 1024;
const MAX_STDERR_BYTES: usize = 16 * 1024;
/// 64 entries plus the limit notice.
const MAX_LOG_ENTRIES: usize = 65;
const MAX_LOG_ENTRY_CHARS: usize = 300 + "...(truncated)".len();
const MAX_WRITE_FILES: usize = 16;
const MAX_WRITE_TOTAL: usize = 32 * 1024 * 1024;
/// `Number.MAX_SAFE_INTEGER`.
const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

/// The message of a run stopped by its [`CancellationToken`]. Today's server
/// has no such ending (`runProgram` takes no signal and a stopped turn
/// waits for the build), so no model reads it: the agent drops a stopped
/// turn's tool result.
pub const CANCELLED: &str = "Program process was stopped with the turn";

/// What the child asks for while a program runs, in the encoded form the
/// child speaks: [`crate::BuildHost`] with the settled world as
/// `build-transfer.js` `encodeWorld` bytes.
#[async_trait]
pub trait EncodedHost: Send {
    /// `mc.readFile(path)`: the file's text, or the error message the program
    /// sees.
    async fn read_file(&mut self, path: &str) -> Result<String, String>;
    /// `mc.settled`'s world, `encodeWorld`'s bytes. An `Err` ends the run
    /// with that message.
    async fn settled_world(&mut self) -> Result<Vec<u8>, String>;
}

/// A finished program as the child answered it: `runProgram`'s result and
/// the change set `build-transfer.js` `encodeChanges` wrote, which the
/// caller applies with `applyChanges` (a failure there is a malformed
/// result, see [`malformed`]).
#[derive(Clone, Debug, PartialEq)]
pub struct EncodedOutcome {
    /// What the program logged.
    pub logs: Vec<String>,
    /// The files it wrote.
    pub files: Vec<WrittenFile>,
    /// The blocks it touched.
    pub touched: i64,
    /// The `DONE` frame's change set.
    pub changes: Vec<u8>,
}

/// The failure of an answer the parent refuses, as `runProgram` words it:
/// `Program process returned a malformed result: <reason>`. The caller uses
/// it when `applyChanges` refuses the change set.
pub fn malformed(reason: &str) -> BuildFailure {
    BuildFailure {
        message: format_parent_error(&format!(
            "Program process returned a malformed result: {reason}"
        )),
        logs: Vec::new(),
    }
}

/// The child under the platform sandbox: `build-sandbox.js` `runProgram`.
pub struct V8Runner {
    program: PathBuf,
    sandbox: Arc<dyn ChildSandbox>,
    spare: Arc<Mutex<Option<ChildProcess>>>,
}

/// Starts `program` under `sandbox`: what the sandbox must wait on first
/// ([`ChildSandbox::prepare`]: `wsl.exe` installing the child on Windows) on
/// the blocking pool, then the start itself here, on a runtime thread. The
/// start stays off the blocking pool because its threads end when idle, and
/// bubblewrap's `--die-with-parent` (the parent-death signal) follows the
/// thread that started it: a child started there was killed with its thread
/// (measured: the corpus's long cases ended "exited unexpectedly" on Linux).
async fn launch(program: &Path, sandbox: &Arc<dyn ChildSandbox>) -> std::io::Result<ChildProcess> {
    {
        let (program, sandbox) = (program.to_path_buf(), Arc::clone(sandbox));
        tokio::task::spawn_blocking(move || sandbox.prepare(&program))
            .await
            .expect("a sandbox's prepare does not panic")?;
    }
    // World-height validation runs in the child and must see the parent's
    // offset.
    let offset = std::env::var_os("MCBENCH_MC_Y_OFFSET");
    let env: Vec<(&str, &std::ffi::OsStr)> = offset
        .as_deref()
        .map(|offset| ("MCBENCH_MC_Y_OFFSET", offset))
        .into_iter()
        .collect();
    sandbox.spawn(program, &env)
}

/// `spare ||= launch()`, once the sandbox is ready. A spare that failed to
/// start is no spare; the next build starts its own and reports why it
/// cannot. No lock is held while the child starts.
async fn refill(
    program: &Path,
    sandbox: &Arc<dyn ChildSandbox>,
    spare: &Mutex<Option<ChildProcess>>,
) {
    if spare.lock().unwrap().is_some() || *sandbox.readiness().borrow() != Readiness::Ready {
        return;
    }
    let child = launch(program, sandbox).await.ok();
    let mut spare = spare.lock().unwrap();
    if spare.is_none() {
        *spare = child;
    }
}

impl V8Runner {
    /// A runner that starts `program` (`mcbench-build-child`, see
    /// [`crate::child_program`]) under `sandbox`. Like
    /// `build-sandbox.js` when it loads, it starts the first process at once,
    /// so it must be called inside the core's tokio runtime.
    pub fn new(program: PathBuf, sandbox: Box<dyn ChildSandbox>) -> Self {
        let runner = Self {
            program,
            sandbox: Arc::from(sandbox),
            spare: Arc::default(),
        };
        let (program, sandbox, spare) = (
            runner.program.clone(),
            Arc::clone(&runner.sandbox),
            Arc::clone(&runner.spare),
        );
        tokio::spawn(async move { refill(&program, &sandbox, &spare).await });
        runner
    }

    /// The program this runner starts.
    pub fn program(&self) -> &Path {
        &self.program
    }

    async fn refill(&self) {
        refill(&self.program, &self.sandbox, &self.spare).await
    }

    /// The spare when it is still alive, otherwise a new process.
    async fn take_process(&self) -> std::io::Result<ChildProcess> {
        let spare = self.spare.lock().unwrap().take();
        match spare {
            Some(mut child) => match child.try_wait() {
                Ok(None) => Ok(child),
                _ => launch(&self.program, &self.sandbox).await,
            },
            None => launch(&self.program, &self.sandbox).await,
        }
    }

    /// Runs `request` against `world` (`build-transfer.js` `encodeWorld`'s
    /// bytes of a clone of the pre-build world), as `runProgram` does with
    /// the bytes it sends, and returns the child's answer or the failure a
    /// model reads (before `agent.js` cuts it at 8,000 characters).
    pub async fn run_encoded(
        &self,
        request: &BuildRequest,
        world: &[u8],
        host: &mut dyn EncodedHost,
        cancel: &CancellationToken,
    ) -> Result<EncodedOutcome, BuildFailure> {
        let request_json = serde_json::to_vec(&serde_json::json!({
            "source": request.source,
            "registry": request.registry,
            "readFile": request.read_file,
            "settledTicks": request.settled_ticks,
        }))
        .expect("the request serialises");
        // No child starts before the sandbox can start one; while it cannot,
        // the build fails with its sentence (WSL's, the same a model reads
        // from `bash`).
        let mut readiness = self.sandbox.readiness();
        let ready = tokio::select! {
            ready = readiness.wait_for(|r| *r != Readiness::Waiting) => {
                ready.expect("the readiness is never dropped").clone()
            }
            () = cancel.cancelled() => {
                return Err(BuildFailure { message: CANCELLED.to_string(), logs: Vec::new() });
            }
        };
        if let Readiness::Unavailable(sentence) = ready {
            return Err(BuildFailure {
                message: sentence,
                logs: Vec::new(),
            });
        }
        let mut child = match self.take_process().await {
            Ok(child) => child,
            Err(error) => {
                self.refill().await;
                return Err(failure(&format!(
                    "spawn {}: {error}",
                    self.program.display()
                )));
            }
        };
        let stdin = child.stdin.take().expect("piped stdin");
        let stdout = child.stdout.take().expect("piped stdout");
        let stderr = child.stderr.take().expect("piped stderr");
        let stderr = tokio::spawn(collect_stderr(stderr));
        let deadline = tokio::time::Instant::now() + WALL_TIMEOUT;
        let exchange = Exchange {
            request,
            host,
            stdin,
        };
        let result = tokio::select! {
            result = exchange.run(self, &request_json, world, stdout) => match result {
                Ended::Answer(result) => result,
                Ended::Closed => {
                    let _ = child.wait().await;
                    let text = stderr.await.expect("the stderr reader does not panic");
                    match self.sandbox.refusal(&String::from_utf8_lossy(&text)) {
                        Some(sentence) => Err(BuildFailure { message: sentence.to_string(), logs: Vec::new() }),
                        None => Err(crash_error(&text)),
                    }
                }
            },
            () = tokio::time::sleep_until(deadline) => Err(failure(&format!(
                "Program process exceeded {} ms and was terminated",
                WALL_TIMEOUT.as_millis()
            ))),
            () = cancel.cancelled() => Err(BuildFailure { message: CANCELLED.to_string(), logs: Vec::new() }),
        };
        let _ = child.start_kill();
        let _ = child.wait().await;
        self.refill().await;
        result
    }
}

/// How the exchange with one child ended.
enum Ended {
    /// A `DONE` or `FAIL` frame, or an answer the parent refused.
    Answer(Result<EncodedOutcome, BuildFailure>),
    /// stdout closed first: the process died.
    Closed,
}

struct Exchange<'a> {
    request: &'a BuildRequest,
    host: &'a mut dyn EncodedHost,
    stdin: ChildStdin,
}

impl Exchange<'_> {
    async fn run(
        mut self,
        runner: &V8Runner,
        request_json: &[u8],
        world: &[u8],
        stdout: impl AsyncRead + Unpin,
    ) -> Ended {
        // A process that died early fails these writes; its stdout closing
        // then reports it, as Node ignores the stdin errors and waits for
        // 'close'.
        let mut message = Vec::with_capacity(8 + request_json.len());
        message.extend_from_slice(&(request_json.len() as u32).to_le_bytes());
        message.extend_from_slice(request_json);
        message.extend_from_slice(&(world.len() as u32).to_le_bytes());
        let written = self.write(&message).await && self.write(world).await;
        if written && runner.sandbox.spare_during_build() {
            runner.refill().await;
        }
        let mut stdout = tokio::io::BufReader::with_capacity(1 << 16, stdout);
        loop {
            let mut head = [0u8; 5];
            if stdout.read_exact(&mut head).await.is_err() {
                return Ended::Closed;
            }
            let length = u32::from_le_bytes(head[1..5].try_into().unwrap()) as usize;
            if 5 + length > MAX_RESULT_BYTES {
                // Node fails once more than the limit has arrived unconsumed.
                let mut sink = (&mut stdout).take((MAX_RESULT_BYTES - 5 + 1) as u64);
                let mut received = 0usize;
                let mut buf = vec![0u8; 1 << 16];
                loop {
                    match sink.read(&mut buf).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => received += n,
                    }
                }
                if 5 + received > MAX_RESULT_BYTES {
                    return Ended::Answer(Err(malformed("result too large")));
                }
                return Ended::Closed;
            }
            let mut payload = vec![0u8; length];
            if stdout.read_exact(&mut payload).await.is_err() {
                return Ended::Closed;
            }
            match self.handle(head[0], payload).await {
                Some(result) => return Ended::Answer(result),
                None => continue,
            }
        }
    }

    async fn write(&mut self, bytes: &[u8]) -> bool {
        self.stdin.write_all(bytes).await.is_ok() && self.stdin.flush().await.is_ok()
    }

    /// `runProgram`'s `handle`: `None` while the program runs on.
    async fn handle(
        &mut self,
        kind: u8,
        payload: Vec<u8>,
    ) -> Option<Result<EncodedOutcome, BuildFailure>> {
        if kind == READ && self.request.read_file {
            let path = String::from_utf8_lossy(&payload);
            let (ok, text) = match self.host.read_file(&path).await {
                Ok(text) => (1u8, text),
                Err(message) => (0u8, message),
            };
            let mut reply = Vec::with_capacity(5 + text.len());
            reply.push(ok);
            reply.extend_from_slice(&(text.len() as u32).to_le_bytes());
            reply.extend_from_slice(text.as_bytes());
            self.write(&reply).await;
            return None;
        }
        if kind == SETTLED && self.request.settled_ticks.is_some() {
            let world = match self.host.settled_world().await {
                Ok(world) => world,
                Err(message) => {
                    return Some(Err(BuildFailure {
                        message,
                        logs: Vec::new(),
                    }));
                }
            };
            let ok = self.write(&(world.len() as u32).to_le_bytes()).await;
            if ok {
                self.write(&world).await;
            }
            return None;
        }
        if kind == FAIL {
            return Some(read_failure(&payload));
        }
        if kind != DONE {
            return Some(Err(malformed(&format!("unexpected frame {kind}"))));
        }
        Some(read_done(payload))
    }
}

/// The child's `FAIL` frame: `{ stack, message, logs }`.
fn read_failure(payload: &[u8]) -> Result<EncodedOutcome, BuildFailure> {
    let failure = parse_json(payload)?;
    let (Some(stack), Some(message)) = (
        failure.get("stack").and_then(Value::as_str),
        failure.get("message").and_then(Value::as_str),
    ) else {
        return Err(malformed("invalid failure"));
    };
    let logs = match failure.get("logs") {
        Some(Value::Null) => Vec::new(),
        logs => check_logs(logs).ok_or_else(|| malformed("invalid logs"))?,
    };
    Err(BuildFailure {
        message: format_failure(stack, message),
        logs,
    })
}

/// The child's `DONE` frame: `[meta length u32][meta JSON][change set]`.
fn read_done(mut payload: Vec<u8>) -> Result<EncodedOutcome, BuildFailure> {
    if payload.len() < 4 {
        return Err(malformed("Attempt to access memory outside buffer bounds"));
    }
    let meta_length = u32::from_le_bytes(payload[..4].try_into().unwrap()) as usize;
    let meta_end = (4 + meta_length).min(payload.len());
    let meta = parse_json(&payload[4..meta_end])?;
    let (logs, files, touched) = check_result(&meta).ok_or_else(|| malformed("invalid result"))?;
    let logs = check_logs(Some(logs)).ok_or_else(|| malformed("invalid logs"))?;
    let changes = payload.split_off(meta_end);
    Ok(EncodedOutcome {
        logs,
        files,
        touched,
        changes,
    })
}

fn parse_json(bytes: &[u8]) -> Result<Value, BuildFailure> {
    let text = String::from_utf8_lossy(bytes);
    serde_json::from_str(&replace_lone_surrogates(&text))
        .map_err(|error| malformed(&error.to_string()))
}

/// The length of a JavaScript string.
fn js_length(text: &str) -> usize {
    text.encode_utf16().count()
}

/// `checkLogs`.
fn check_logs(logs: Option<&Value>) -> Option<Vec<String>> {
    let logs = logs?.as_array()?;
    if logs.len() > MAX_LOG_ENTRIES {
        return None;
    }
    logs.iter()
        .map(|entry| {
            let entry = entry.as_str()?;
            (js_length(entry) <= MAX_LOG_ENTRY_CHARS).then(|| entry.to_string())
        })
        .collect()
}

/// `checkResult` but the logs: accept only what the mc API could produce.
fn check_result(meta: &Value) -> Option<(&Value, Vec<WrittenFile>, i64)> {
    let files = meta.get("files")?.as_array()?;
    if files.len() > MAX_WRITE_FILES {
        return None;
    }
    let files = files
        .iter()
        .map(|file| {
            let path = file.get("path")?.as_str()?;
            let text = file.get("text")?.as_str()?;
            (!js_trim(path).is_empty()).then(|| WrittenFile {
                path: path.to_string(),
                text: text.to_string(),
            })
        })
        .collect::<Option<Vec<_>>>()?;
    if files
        .iter()
        .map(|file| js_length(&file.text))
        .sum::<usize>()
        > MAX_WRITE_TOTAL
    {
        return None;
    }
    let touched = meta.get("touched")?.as_f64()?;
    if touched.fract() != 0.0 || touched.abs() > MAX_SAFE_INTEGER {
        return None;
    }
    // A missing `logs` fails checkLogs.
    Some((
        meta.get("logs").unwrap_or(&Value::Null),
        files,
        touched as i64,
    ))
}

/// `new Error(message)` from `build-sandbox.js`, as `formatProgramError`
/// shows it.
fn failure(message: &str) -> BuildFailure {
    BuildFailure {
        message: format_parent_error(message),
        logs: Vec::new(),
    }
}

/// `crashError`.
fn crash_error(stderr: &[u8]) -> BuildFailure {
    let text = String::from_utf8_lossy(stderr);
    if text.contains("heap out of memory") || text.contains("Allocation failed") {
        return failure(&format!(
            "Program ran out of memory (limit {} MB) and was terminated",
            crate::HEAP_LIMIT_MB
        ));
    }
    let detail = js_trim(&text).split('\n').next_back().unwrap_or("");
    failure(&if detail.is_empty() {
        "Program process exited unexpectedly".to_string()
    } else {
        format!("Program process exited unexpectedly: {detail}")
    })
}

/// stderr as `runProgram` keeps it: chunks while fewer than 16 KiB arrived.
async fn collect_stderr(mut stderr: impl AsyncRead + Unpin) -> Vec<u8> {
    let mut kept = Vec::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        match stderr.read(&mut buf).await {
            Ok(0) | Err(_) => return kept,
            Ok(n) => {
                if kept.len() < MAX_STDERR_BYTES {
                    kept.extend_from_slice(&buf[..n]);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A sandbox whose readiness the test sets and that counts its starts
    /// (each fails: no child program exists here).
    struct Gated {
        readiness: tokio::sync::watch::Sender<Readiness>,
        starts: std::sync::atomic::AtomicUsize,
    }

    impl ChildSandbox for Gated {
        fn spawn(&self, _: &Path, _: &[(&str, &std::ffi::OsStr)]) -> std::io::Result<ChildProcess> {
            self.starts
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Err(std::io::Error::other("no child here"))
        }
        fn spare_during_build(&self) -> bool {
            true
        }
        fn readiness(&self) -> tokio::sync::watch::Receiver<Readiness> {
            self.readiness.subscribe()
        }
    }

    struct NoHost;

    #[async_trait]
    impl EncodedHost for NoHost {
        async fn read_file(&mut self, _: &str) -> Result<String, String> {
            Err("no file".into())
        }
        async fn settled_world(&mut self) -> Result<Vec<u8>, String> {
            Err("no world".into())
        }
    }

    // The review's C3 and C4: no child is started while the sandbox's state
    // is not known (on Windows: no wsl.exe before setup's shell step has
    // looked); a build then waits; an unavailable sandbox fails it with its
    // sentence, unchanged; a ready one starts children.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn no_child_starts_before_the_sandbox_is_ready_and_an_unavailable_one_answers_its_sentence()
     {
        let sandbox = Arc::new(Gated {
            readiness: tokio::sync::watch::channel(Readiness::Waiting).0,
            starts: Default::default(),
        });
        struct Shared(Arc<Gated>);
        impl ChildSandbox for Shared {
            fn spawn(
                &self,
                program: &Path,
                env: &[(&str, &std::ffi::OsStr)],
            ) -> std::io::Result<ChildProcess> {
                self.0.spawn(program, env)
            }
            fn spare_during_build(&self) -> bool {
                true
            }
            fn readiness(&self) -> tokio::sync::watch::Receiver<Readiness> {
                self.0.readiness()
            }
        }
        let runner = V8Runner::new(
            PathBuf::from("no-child"),
            Box::new(Shared(Arc::clone(&sandbox))),
        );
        let request = BuildRequest {
            source: "module.exports = mc => {}".into(),
            registry: serde_json::json!({}),
            read_file: false,
            settled_ticks: None,
        };
        let cancel = CancellationToken::new();
        let starts = || sandbox.starts.load(std::sync::atomic::Ordering::SeqCst);
        let waiting = tokio::time::timeout(
            std::time::Duration::from_millis(300),
            runner.run_encoded(&request, &[], &mut NoHost, &cancel),
        )
        .await;
        assert!(
            waiting.is_err(),
            "a build waits while the state is not known"
        );
        assert_eq!(starts(), 0, "no child starts before the sandbox is ready");
        sandbox
            .readiness
            .send_replace(Readiness::Unavailable("the WSL sentence".into()));
        let refused = runner
            .run_encoded(&request, &[], &mut NoHost, &cancel)
            .await
            .unwrap_err();
        assert_eq!(refused.message, "the WSL sentence");
        assert_eq!(starts(), 0);
        sandbox.readiness.send_replace(Readiness::Ready);
        let failed = runner
            .run_encoded(&request, &[], &mut NoHost, &cancel)
            .await
            .unwrap_err();
        assert!(
            failed.message.contains("spawn no-child: no child here"),
            "{}",
            failed.message
        );
        assert!(starts() >= 1);
    }

    /// A child that writes `stderr` and exits before answering, under the
    /// Linux sandbox's reading of such an ending.
    #[cfg(unix)]
    struct Says(&'static str);

    #[cfg(unix)]
    impl ChildSandbox for Says {
        fn spawn(
            &self,
            _: &Path,
            env: &[(&str, &std::ffi::OsStr)],
        ) -> std::io::Result<ChildProcess> {
            let mut command = mcbench_spawn::command("/bin/sh");
            command.args(["-c", "printf '%s\\n' \"$0\" >&2; exit 1", self.0]);
            ChildProcess::start(command, env)
        }
        fn spare_during_build(&self) -> bool {
            true
        }
        fn refusal(&self, stderr: &str) -> Option<&'static str> {
            crate::sandbox::linux::Bubblewrap.refusal(stderr)
        }
    }

    // Ubuntu 24.04 with no profile for bwrap: the build child's bubblewrap
    // is refused its loopback address (its words, verbatim from the VM), and
    // the build answers the sentence `bash` answers, not bubblewrap's line.
    // The child's own crash keeps its last line, whatever it says.
    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_build_whose_bubblewrap_is_refused_a_user_namespace_answers_userns_missing() {
        let request = BuildRequest {
            source: "module.exports = mc => {}".into(),
            registry: serde_json::json!({}),
            read_file: false,
            settled_ticks: None,
        };
        let cancel = CancellationToken::new();
        for (stderr, message) in [
            (
                "bwrap: loopback: Failed RTM_NEWADDR: Operation not permitted",
                crate::sandbox::linux::USERNS_MISSING.to_string(),
            ),
            (
                "Error: EPERM: Operation not permitted",
                "Error: Program process exited unexpectedly: Error: EPERM: Operation not permitted"
                    .to_string(),
            ),
        ] {
            let runner = V8Runner::new(PathBuf::from("/bin/sh"), Box::new(Says(stderr)));
            let failed = runner
                .run_encoded(&request, &[], &mut NoHost, &cancel)
                .await
                .unwrap_err();
            assert_eq!(failed.message, message);
            assert!(failed.logs.is_empty());
        }
    }

    #[test]
    fn a_crash_reads_the_last_line_of_stderr() {
        assert_eq!(
            crash_error(b"Uncaught Error: x\n    at y\n\nNode.js v26.13.1\n").message,
            "Error: Program process exited unexpectedly: Node.js v26.13.1"
        );
        assert_eq!(
            crash_error(b"").message,
            "Error: Program process exited unexpectedly"
        );
        assert_eq!(
            crash_error(b"FATAL ERROR: Reached heap limit Allocation failed - JavaScript heap out of memory").message,
            "Error: Program ran out of memory (limit 2048 MB) and was terminated"
        );
    }

    #[test]
    fn forged_answers_are_refused_with_today_s_reasons() {
        let reason = |payload: &[u8]| read_done(payload.to_vec()).unwrap_err().message;
        let done = |meta: &str| {
            let mut payload = (meta.len() as u32).to_le_bytes().to_vec();
            payload.extend_from_slice(meta.as_bytes());
            payload
        };
        assert_eq!(
            reason(&done(r#"{"logs":[],"files":[],"touched":1.5}"#)),
            "Error: Program process returned a malformed result: invalid result"
        );
        assert_eq!(
            reason(&done(r#"{"logs":[1],"files":[],"touched":1}"#)),
            "Error: Program process returned a malformed result: invalid logs"
        );
        assert_eq!(
            reason(&done(
                r#"{"logs":[],"files":[{"path":" ","text":""}],"touched":1}"#
            )),
            "Error: Program process returned a malformed result: invalid result"
        );
        let ok = read_done(done(
            r#"{"logs":["a"],"files":[{"path":"a.txt","text":"x"}],"touched":3}"#,
        ))
        .unwrap();
        assert_eq!(ok.touched, 3);
        assert_eq!(
            ok.files,
            vec![WrittenFile {
                path: "a.txt".into(),
                text: "x".into()
            }]
        );
        assert_eq!(
            read_failure(br#"{"stack":7,"message":"m","logs":null}"#)
                .unwrap_err()
                .message,
            "Error: Program process returned a malformed result: invalid failure"
        );
        assert_eq!(
            read_failure(br#"{"stack":"","message":"42","logs":["a"]}"#).unwrap_err(),
            BuildFailure {
                message: "42".into(),
                logs: vec!["a".into()]
            }
        );
    }
}
