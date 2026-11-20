//! A CLI whose stdout is newline-delimited JSON (`harness.js`
//! `runJsonLines`).
//!
//! Each stdout line is parsed as `JSON.parse` parses it and given to
//! `on_line`; an error from it stops the process and becomes the run's
//! outcome. [`JsonLinesRun::stop`] ends the process early (SIGTERM, then
//! SIGKILL after 10 seconds) and does nothing once it is stopping or has
//! exited. The run's outcome is its exit status and the last 4000
//! characters of stderr, once the process has exited and its output has
//! been read. The child is killed when its task is dropped. On Windows the
//! kill ends the CLI's whole tree, its tool bridge included ([`Tree`]): the
//! job ends at the kill, and its handle closes when the task ends or is
//! dropped.

use std::ffi::OsString;
use std::path::Path;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{mpsc, oneshot};

use super::tree::Tree;
use crate::bridge::{LineSplitter, blank};

/// How the process ended.
#[derive(Clone, Debug)]
pub(crate) struct Exit {
    /// Its exit code, or `None` when a signal ended it.
    pub code: Option<i32>,
    /// The signal's name (`SIGTERM`), when one ended it.
    pub signal: Option<String>,
    /// The tail of its stderr.
    pub stderr: String,
}

impl Exit {
    /// `exit.code ?? exit.signal`.
    pub(crate) fn code_or_signal(&self) -> String {
        match (&self.code, &self.signal) {
            (Some(code), _) => code.to_string(),
            (None, Some(signal)) => signal.clone(),
            (None, None) => "null".to_owned(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RunState {
    Running,
    Stopping,
    Exited,
}

struct StopState {
    state: RunState,
    pid: Option<u32>,
    on_stop: Option<Box<dyn FnOnce() + Send>>,
    kill: Option<oneshot::Sender<()>>,
}

/// A running CLI: stop it, or write more input to it.
#[derive(Clone)]
pub(crate) struct JsonLinesRun {
    stop: Arc<Mutex<StopState>>,
    input: Option<mpsc::UnboundedSender<Vec<u8>>>,
}

impl JsonLinesRun {
    /// SIGTERM, then SIGKILL after 10 seconds; nothing once stopping or
    /// exited.
    pub(crate) fn stop(&self) {
        stop(&self.stop);
    }

    /// Writes to the process's stdin (`keepOpen`).
    pub(crate) fn write(&self, bytes: Vec<u8>) {
        if let Some(input) = &self.input {
            let _ = input.send(bytes);
        }
    }
}

fn stop(shared: &Arc<Mutex<StopState>>) {
    let mut s = shared.lock().unwrap();
    if s.state != RunState::Running {
        return;
    }
    s.state = RunState::Stopping;
    let on_stop = s.on_stop.take();
    let pid = s.pid;
    let kill = s.kill.take();
    drop(s);
    if let Some(on_stop) = on_stop {
        on_stop();
    }
    terminate(pid);
    if let Some(kill) = kill {
        if cfg!(unix) {
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(10_000)).await;
                let _ = kill.send(());
            });
        } else {
            let _ = kill.send(());
        }
    }
}

#[cfg(unix)]
fn terminate(pid: Option<u32>) {
    if let Some(pid) = pid {
        // SAFETY: kill(2) with a pid this process started and has not yet
        // reaped (the run is not exited) and a valid signal number; it
        // touches no memory of this process.
        unsafe {
            libc::kill(pid as libc::pid_t, libc::SIGTERM);
        }
    }
}

#[cfg(not(unix))]
fn terminate(_pid: Option<u32>) {
    // No SIGTERM on Windows: the kill below (at once) ends the process.
}

/// The kill: SIGKILL to the CLI on macOS and Linux, its whole job on
/// Windows.
fn kill(child: &mut tokio::process::Child, tree: &Tree) {
    #[cfg(windows)]
    {
        let _ = &child;
        tree.end();
    }
    #[cfg(not(windows))]
    {
        let _ = tree;
        let _ = child.start_kill();
    }
}

/// A signal number's name, as Node reports a child's.
#[cfg(unix)]
fn signal_name(signal: i32) -> String {
    let names: [(i32, &str); 15] = [
        (libc::SIGHUP, "SIGHUP"),
        (libc::SIGINT, "SIGINT"),
        (libc::SIGQUIT, "SIGQUIT"),
        (libc::SIGILL, "SIGILL"),
        (libc::SIGTRAP, "SIGTRAP"),
        (libc::SIGABRT, "SIGABRT"),
        (libc::SIGBUS, "SIGBUS"),
        (libc::SIGFPE, "SIGFPE"),
        (libc::SIGKILL, "SIGKILL"),
        (libc::SIGUSR1, "SIGUSR1"),
        (libc::SIGSEGV, "SIGSEGV"),
        (libc::SIGUSR2, "SIGUSR2"),
        (libc::SIGPIPE, "SIGPIPE"),
        (libc::SIGALRM, "SIGALRM"),
        (libc::SIGTERM, "SIGTERM"),
    ];
    names
        .iter()
        .find(|(n, _)| *n == signal)
        .map(|(_, name)| (*name).to_owned())
        .unwrap_or_else(|| signal.to_string())
}

/// What starts the process.
pub(crate) struct Launch<'a> {
    /// The CLI as a person names it ([`too_long`]).
    pub name: &'a str,
    pub command: &'a OsString,
    pub args: &'a [String],
    pub cwd: &'a Path,
    pub env: &'a [(OsString, OsString)],
}

/// What a person reads when the CLI `name` wrote a line longer than
/// [`crate::bridge::MAX_LINE`] (64 MiB): the turn ends with it.
pub(crate) fn too_long(name: &str) -> String {
    format!(
        "{name} wrote a line of more than 64 MiB to mcbench, longer than any event it sends, and mcbench stopped the turn"
    )
}

/// Keeps the last `limit` UTF-16 code units of `text` (`.slice(-limit)`).
fn tail(text: String, limit: usize) -> String {
    let units: usize = text.chars().map(char::len_utf16).sum();
    if units <= limit {
        return text;
    }
    let mut drop = units - limit;
    let mut start = 0;
    for (i, c) in text.char_indices() {
        if drop == 0 {
            start = i;
            break;
        }
        drop = drop.saturating_sub(c.len_utf16());
        start = i + c.len_utf8();
    }
    text[start..].to_owned()
}

/// Starts the process. `on_line` gets each parsed stdout line; `done`
/// receives the outcome. A spawn that fails as Node's fails at once (an
/// argument with a NUL byte) is an `Err`; a program that is missing or not
/// executable fails the run as Node's `error` event does (`spawn <command>
/// ENOENT`).
pub(crate) fn run_json_lines(
    launch: Launch<'_>,
    input: Vec<u8>,
    keep_open: bool,
    mut on_line: Box<dyn FnMut(Value) -> Result<(), String> + Send>,
    on_stop: Box<dyn FnOnce() + Send>,
    done: Box<dyn FnOnce(Result<Exit, String>) + Send>,
) -> Result<JsonLinesRun, String> {
    if let Some(index) = launch.args.iter().position(|a| a.contains('\0')) {
        return Err(format!(
            "The argument 'args[{index}]' must be a string without null bytes"
        ));
    }
    let mut command = tokio::process::Command::from(mcbench_spawn::command(launch.command));
    command
        .args(launch.args)
        .current_dir(launch.cwd)
        .env_clear()
        .envs(launch.env.iter().map(|(k, v)| (k, v)))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let (kill_send, kill_recv) = oneshot::channel();
    let shared = Arc::new(Mutex::new(StopState {
        state: RunState::Running,
        pid: None,
        on_stop: Some(on_stop),
        kill: Some(kill_send),
    }));
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(e) => {
            let code = match e.kind() {
                std::io::ErrorKind::NotFound => Some("ENOENT"),
                std::io::ErrorKind::PermissionDenied => Some("EACCES"),
                _ => None,
            };
            let Some(code) = code else {
                return Err(format!("spawn {}: {e}", launch.command.to_string_lossy()));
            };
            let message = format!("spawn {} {code}", launch.command.to_string_lossy());
            shared.lock().unwrap().state = RunState::Exited;
            tokio::spawn(async move { done(Err(message)) });
            return Ok(JsonLinesRun {
                stop: shared,
                input: None,
            });
        }
    };
    let tree = match Tree::holding(&child) {
        Ok(tree) => tree,
        Err(e) => {
            let _ = child.start_kill();
            return Err(format!(
                "spawn {}: its process tree: {e}",
                launch.command.to_string_lossy()
            ));
        }
    };
    shared.lock().unwrap().pid = child.id();
    let mut stdin = child.stdin.take().expect("stdin is piped");
    let mut stdout = child.stdout.take().expect("stdout is piped");
    let mut stderr = child.stderr.take().expect("stderr is piped");
    let (input_send, mut input_recv) = mpsc::unbounded_channel::<Vec<u8>>();
    let _ = input_send.send(input);
    let writer_input = if keep_open {
        Some(input_send.clone())
    } else {
        None
    };
    drop(input_send);
    // stdin: written in order; write errors are ignored (`stdin.on('error')`).
    tokio::spawn(async move {
        while let Some(bytes) = input_recv.recv().await {
            if stdin.write_all(&bytes).await.is_err() {
                return;
            }
        }
        let _ = stdin.shutdown().await;
    });
    let errors = tokio::spawn(async move {
        let mut text = String::new();
        let mut buffer = vec![0u8; 65536];
        loop {
            match stderr.read(&mut buffer).await {
                Ok(0) | Err(_) => return text,
                Ok(n) => {
                    text.push_str(&String::from_utf8_lossy(&buffer[..n]));
                    text = tail(text, 4000);
                }
            }
        }
    });
    let stopper = shared.clone();
    let name = launch.name.to_owned();
    tokio::spawn(async move {
        let mut failure: Option<String> = None;
        let mut lines = LineSplitter::new();
        let mut buffer = vec![0u8; 65536];
        let mut handle = |line: String, failure: &mut Option<String>| {
            if failure.is_some() || blank(&line) {
                return;
            }
            let result = crate::api::v8json::parse(&line).and_then(&mut on_line);
            if let Err(error) = result {
                *failure = Some(error);
                stop(&stopper);
            }
        };
        let mut kill_recv = kill_recv;
        let mut killed = false;
        loop {
            let read = tokio::select! {
                read = stdout.read(&mut buffer) => read,
                fired = &mut kill_recv, if !killed => {
                    killed = true;
                    if fired.is_ok() {
                        kill(&mut child, &tree);
                    }
                    continue;
                }
            };
            match read {
                Ok(0) | Err(_) => {
                    if let Some(line) = lines.finish() {
                        handle(line, &mut failure);
                    }
                    break;
                }
                Ok(n) => {
                    for line in lines.feed(&buffer[..n]) {
                        handle(line, &mut failure);
                    }
                    if lines.too_long() {
                        if failure.is_none() {
                            failure = Some(too_long(&name));
                            stop(&stopper);
                        }
                        break;
                    }
                }
            }
        }
        let status = tokio::select! {
            status = child.wait() => status,
            fired = &mut kill_recv, if !killed => {
                if fired.is_ok() {
                    kill(&mut child, &tree);
                }
                child.wait().await
            }
        };
        // What the CLI left running ends with the job's handle.
        tree.close();
        let stderr = errors.await.unwrap_or_default();
        stopper.lock().unwrap().state = RunState::Exited;
        let outcome = match (failure, status) {
            (Some(failure), _) => Err(failure),
            (None, Err(e)) => Err(e.to_string()),
            (None, Ok(status)) => {
                #[cfg(unix)]
                let signal =
                    std::os::unix::process::ExitStatusExt::signal(&status).map(signal_name);
                #[cfg(not(unix))]
                let signal = None;
                Ok(Exit {
                    code: status.code(),
                    signal,
                    stderr,
                })
            }
        };
        done(outcome);
    });
    Ok(JsonLinesRun {
        stop: shared,
        input: writer_input,
    })
}
