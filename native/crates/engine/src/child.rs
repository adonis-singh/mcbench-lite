//! Engine child processes, the port of `server/engine.js`'s `Engine` (the
//! process, its requests over stdin and stdout, its files, its stop and
//! the sentences it fails with): [`EngineProgram`] is how a child starts,
//! `<the app's own binary> headless engine <tables>` (the released
//! program's own protocol loop, `mcbench-headless`), and [`ChildEngine`]
//! one running child.
//!
//! A child runs in a folder of its own under the system's temporary folder
//! (its working directory, `mcbench-engine-child-<app pid>-<n>`), and every
//! request names its files relative to it: the protocol splits a request on
//! spaces, and the path of a folder may hold one (a Windows user's name, the
//! macOS data folder). The folder goes once the child has exited.
//!
//! A child is killed, never left running: on Unix it is the leader of a
//! process group of its own and every signal goes to the group; on Windows
//! it runs in a Job object of its own, created with kill-on-job-close, and
//! every stop ends the job (`TerminateJobObject`), so its whole tree goes.
//! It is killed when its handle is dropped (and by tokio's `kill_on_drop`
//! when the runtime ends first). When the app itself dies, however it dies,
//! the protocol loop of the binary exits on Unix because its parent process
//! is gone (`mcbench-headless`'s `engine.rs`), and on Windows the system
//! closes the app's handles to the jobs, which kills every child.

use crate::{BlockState, Blueprint, DATA_VERSION, EngineError, PlaceCommands};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::{mpsc, oneshot, watch};
use tokio_util::sync::CancellationToken;

/// The prefix of every child's folder under the system's temporary folder.
pub const CHILD_FOLDER: &str = "mcbench-engine-child-";

/// How long a stopped child has to exit after `SIGTERM` before it gets
/// `SIGKILL` (`engine.js` `end`).
const STOP_GRACE: Duration = Duration::from_secs(10);

static FOLDERS: AtomicU64 = AtomicU64::new(0);

/// How an engine child starts: `<binary> headless engine <tables>`. The app
/// gives its own binary (`std::env::current_exe()`); tests give the built
/// `mcbench-lite`.
#[derive(Clone, Debug)]
pub struct EngineProgram {
    binary: PathBuf,
    tables: PathBuf,
}

impl EngineProgram {
    /// The children of `binary`, reading Minecraft's tables from `tables`.
    pub fn new(binary: impl Into<PathBuf>, tables: impl Into<PathBuf>) -> EngineProgram {
        EngineProgram {
            binary: binary.into(),
            tables: tables.into(),
        }
    }

    /// The binary.
    pub fn binary(&self) -> &Path {
        &self.binary
    }

    /// The tables folder.
    pub fn tables(&self) -> &Path {
        &self.tables
    }

    fn command(&self, extra: &[&str]) -> Command {
        let mut command = Command::from(mcbench_spawn::command(&self.binary));
        command
            .arg("headless")
            .arg("engine")
            .arg(&self.tables)
            .args(extra)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        #[cfg(unix)]
        command.process_group(0);
        command
    }

    /// Starts a child (`warm`): it is starting until it says
    /// `MCBENCH_ENGINE_READY` ([`ChildEngine::ready`]). Needs the tokio
    /// runtime. Fails only when the program cannot be started at all.
    pub fn spawn(&self) -> Result<ChildEngine, EngineError> {
        let folder = std::env::temp_dir().join(format!(
            "{CHILD_FOLDER}{}-{}",
            std::process::id(),
            FOLDERS.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&folder)
            .map_err(|error| EngineError::Stopped(format!("{}: {error}", folder.display())))?;
        let mut command = self.command(&[]);
        command.current_dir(&folder);
        let child = command.spawn().map_err(|error| {
            let _ = std::fs::remove_dir_all(&folder);
            EngineError::Stopped(format!(
                "The engine stopped (code null): {}: {error}",
                self.binary.display()
            ))
        })?;
        let tree = Tree::holding(&child).map_err(|error| {
            let _ = std::fs::remove_dir_all(&folder);
            EngineError::Stopped(format!(
                "The engine stopped (code null): {}: {error}",
                self.binary.display()
            ))
        })?;
        Ok(ChildEngine::run(child, tree, folder))
    }

    /// The engine's port of Minecraft's data fixer on lines `<type>
    /// <version> <snbt>`, in a child of its own (`--datafix`, as
    /// `engine.js` `fixedPalette` runs the program): a line each, `ok
    /// <snbt>` or the refusal. Dropping the future kills the child.
    pub async fn datafix(&self, lines: &str) -> Result<String, String> {
        let mut child = self
            .command(&["--datafix"])
            .spawn()
            .map_err(|error| format!("{}: {error}", self.binary.display()))?;
        // Dropped with the future, it ends the child's tree (Windows).
        let _tree =
            Tree::holding(&child).map_err(|error| format!("{}: {error}", self.binary.display()))?;
        let mut stdin = child.stdin.take().expect("piped");
        let (mut stdout, mut stderr) = (
            child.stdout.take().expect("piped"),
            child.stderr.take().expect("piped"),
        );
        let lines = lines.to_string();
        let write = async move {
            // A child that exits before it read its lines fails below with its code.
            let _ = stdin.write_all(lines.as_bytes()).await;
        };
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let (_, read_out, read_err) = tokio::join!(
            write,
            stdout.read_to_end(&mut out),
            stderr.read_to_end(&mut err)
        );
        read_out.map_err(|error| format!("the data fixer's stdout: {error}"))?;
        read_err.map_err(|error| format!("the data fixer's stderr: {error}"))?;
        let status = child
            .wait()
            .await
            .map_err(|error| format!("the data fixer: {error}"))?;
        if status.success() {
            Ok(String::from_utf8_lossy(&out).into_owned())
        } else {
            Err(format!(
                "The engine's data fixer failed (code {}): {}",
                status
                    .code()
                    .map_or_else(|| "null".to_string(), |code| code.to_string()),
                String::from_utf8_lossy(&err).trim()
            ))
        }
    }

    /// `server/engine.js` `fixedPalette`: the block states of a palette
    /// (its entries after air, in order) as the engine places the blocks of
    /// a world written at Minecraft data version `data_version`, each
    /// upgraded by the engine's data fixer as `VERIFY` upgrades every
    /// command's block. A palette of the pinned version or a later one is
    /// answered as it is (the fixer leaves it, and `VERIFY` refuses a later
    /// one before it places).
    pub async fn fixed_palette(
        &self,
        entries: &[BlockState],
        data_version: i64,
    ) -> Result<Vec<BlockState>, String> {
        if data_version >= DATA_VERSION {
            return Ok(entries.to_vec());
        }
        let quote = |text: &str| serde_json::to_string(text).unwrap();
        let mut lines = String::new();
        for entry in entries {
            let properties: Vec<String> = entry
                .properties
                .iter()
                .map(|(key, value)| format!("{}:{}", quote(key), quote(value)))
                .collect();
            let properties = if properties.is_empty() {
                String::new()
            } else {
                format!(",Properties:{{{}}}", properties.join(","))
            };
            lines.push_str(&format!(
                "block_state {data_version} {{Name:{}{properties}}}\n",
                quote(&entry.id)
            ));
        }
        let answer = self.datafix(&lines).await?;
        let mut fixed = Vec::new();
        for line in answer.split('\n').filter(|line| !line.is_empty()) {
            fixed.push(
                fixed_state(line)
                    .ok_or_else(|| format!("The engine's data fixer answered {line}"))?,
            );
        }
        if fixed.len() != entries.len() {
            return Err(format!(
                "The engine's data fixer answered {} of {} block states",
                fixed.len(),
                entries.len()
            ));
        }
        Ok(fixed)
    }
}

/// A line of the data fixer's answer as `fixedPalette` reads it:
/// `^ok \{Name:"([^"]*)"(?:,Properties:\{(.*)\})?\}$`, the properties
/// `"?([a-z0-9_]+)"?:"([^"]*)"` each.
pub(crate) fn fixed_state(line: &str) -> Option<BlockState> {
    let rest = line.strip_prefix("ok {Name:\"")?.strip_suffix('}')?;
    let close = rest.find('"')?;
    let id = rest[..close].to_string();
    let after = &rest[close + 1..];
    let properties = if after.is_empty() {
        Vec::new()
    } else {
        let inner = after.strip_prefix(",Properties:{")?.strip_suffix('}')?;
        property_pairs(inner)
    };
    Some(BlockState { id, properties })
}

/// Every match of `"?([a-z0-9_]+)"?:"([^"]*)"` in `text`, left to right.
pub(crate) fn property_pairs(text: &str) -> Vec<(String, String)> {
    let bytes = text.as_bytes();
    let name = |b: u8| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_';
    let at = |start: usize| -> Option<(usize, String, String)> {
        let mut i = start;
        if bytes.get(i) == Some(&b'"') {
            i += 1;
        }
        let key_start = i;
        while bytes.get(i).is_some_and(|&b| name(b)) {
            i += 1;
        }
        if i == key_start {
            return None;
        }
        let key = text[key_start..i].to_string();
        if bytes.get(i) == Some(&b'"') {
            i += 1;
        }
        if bytes.get(i) != Some(&b':') || bytes.get(i + 1) != Some(&b'"') {
            return None;
        }
        i += 2;
        let value_start = i;
        while bytes.get(i).is_some_and(|&b| b != b'"') {
            i += 1;
        }
        bytes.get(i)?;
        Some((i + 1, key, text[value_start..i].to_string()))
    };
    let mut pairs = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        match at(i) {
            Some((end, key, value)) => {
                pairs.push((key, value));
                i = end;
            }
            None => i += 1,
        }
    }
    pairs
}

/// Where a child is: `engine.js`'s `starting`, `ready`, and the end of its
/// process (`dead`, or `stopped` when [`ChildEngine::stop`] was called
/// first) with the sentence its requests fail with.
#[derive(Clone, Debug)]
enum Phase {
    Starting,
    Ready,
    Ended(String),
}

/// A signal a child is sent.
#[derive(Clone, Copy, Debug)]
enum Signal {
    Term,
    Kill,
}

type Answer = oneshot::Sender<Result<(), EngineError>>;

/// What a child's tasks and its handle share.
struct Shared {
    phase: watch::Sender<Phase>,
    /// The requests sent and not answered, by id.
    requests: Mutex<HashMap<u64, Answer>>,
    /// The tail of its stderr (`run.output`, the last 8,000 characters).
    output: Mutex<String>,
    /// [`ChildEngine::stop`] was called: it answers no more requests.
    stopped: AtomicBool,
}

/// One engine child process and the requests it answers (`engine.js`
/// `Engine` for one run of its process). Requests run one at a time in the
/// order they were sent, as the protocol loop reads them. Each takes a
/// [`CancellationToken`]: when it fires, the child is stopped (its work
/// cannot be stopped otherwise) and the request fails with
/// [`EngineError::Cancelled`] at once.
///
/// A child that exits fails every request that waits and every later one
/// with `The engine stopped (<signal> | code <code>): <the tail of its
/// stderr>`; a stopped one fails later requests with `The engine was
/// stopped`. Dropping the handle kills the child.
///
/// A child belongs to the tokio runtime it was started on: its reader and
/// exit tasks run there, and when that runtime ends the child is killed
/// (`kill_on_drop`) and its handle answers nothing more.
pub struct ChildEngine {
    pid: u32,
    folder: PathBuf,
    shared: Arc<Shared>,
    stdin: tokio::sync::Mutex<Option<ChildStdin>>,
    kills: mpsc::UnboundedSender<Signal>,
    exited: watch::Receiver<bool>,
    next: AtomicU64,
}

/// The files a `TICK` writes: the patch of what changed and the entities.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TickFiles {
    /// The patch (`McbenchEngine.tick`).
    pub changes: Vec<u8>,
    /// The entities, the engine's JSON list.
    pub entities: Vec<u8>,
}

/// A `test_build` experiment for the engine's `VERIFY`: the request JSON and
/// the live import's commands, as `server/minecraft-verify.js` writes them
/// for `McbenchEngine` today.
#[derive(Clone, Debug)]
pub struct VerifyRequest {
    /// `request.json`, byte for byte as today's verifier writes it.
    pub request: String,
    /// The build's placement commands.
    pub commands: PlaceCommands,
}

/// The report of a `VERIFY`: `result.json` and the files the engine wrote
/// beside it, read back whole.
#[derive(Clone, Debug)]
pub struct VerifyReport {
    /// `result.json` as the engine wrote it.
    pub result: String,
    /// Every other file of the output folder, by name, sorted.
    pub files: Vec<(String, Vec<u8>)>,
}

/// `${signal || `code ${code}`}` of Node's `close` event. Windows has no
/// signals: there libuv reports the signal its `kill` was given for a
/// process it ended, as here `sent`, the last one sent; a process that
/// ended otherwise (by itself, or killed from outside) reads its exit code.
fn exit_words(status: &ExitStatus, sent: Option<Signal>) -> String {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        let _ = sent;
        if let Some(signal) = status.signal() {
            return signal_name(signal);
        }
    }
    #[cfg(windows)]
    if let Some(signal) = sent {
        return match signal {
            Signal::Term => "SIGTERM",
            Signal::Kill => "SIGKILL",
        }
        .to_string();
    }
    match status.code() {
        Some(code) => format!("code {code}"),
        None => "code null".to_string(),
    }
}

/// The name Node gives a signal (`os.constants.signals`).
#[cfg(unix)]
fn signal_name(signal: i32) -> String {
    let names = [
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
        (libc::SIGXCPU, "SIGXCPU"),
        (libc::SIGXFSZ, "SIGXFSZ"),
        (libc::SIGSYS, "SIGSYS"),
    ];
    names
        .iter()
        .find(|(number, _)| *number == signal)
        .map_or_else(|| format!("signal {signal}"), |(_, name)| name.to_string())
}

/// Sends `signal` to the child's process group (Unix) or ends its job
/// (Windows: there is no `SIGTERM`, so both end it at once, as Node's `kill`
/// does there). Called only while the child is not reaped, so its pid and
/// group are still its own.
fn send(child: &mut Child, tree: &Tree, signal: Signal) {
    #[cfg(unix)]
    {
        let _ = tree;
        let Some(pid) = child.id() else { return };
        let number = match signal {
            Signal::Term => libc::SIGTERM,
            Signal::Kill => libc::SIGKILL,
        };
        // SAFETY: kill(2) takes two integers and touches no memory of ours.
        // The child was made the leader of a group of its own
        // (`process_group(0)`, so its group id is its pid) and is not reaped
        // yet (this runs only before `wait` returned), so the group is still
        // the child's and nothing else's.
        unsafe {
            libc::kill(-(pid as i32), number);
        }
    }
    #[cfg(windows)]
    {
        let _ = (child, signal);
        tree.end();
    }
}

/// What holds a child's whole tree: nothing more than its process group on
/// Unix.
#[cfg(unix)]
struct Tree;

#[cfg(unix)]
impl Tree {
    fn holding(_: &Child) -> std::io::Result<Tree> {
        Ok(Tree)
    }
}

/// A child's Job object on Windows: created with kill-on-job-close and
/// holding the child, so every process it starts is in it too. Ending it
/// ends them all; closing its handle (this value dropped, or the app's
/// process gone however it went) kills them all.
#[cfg(windows)]
struct Tree(windows_sys::Win32::Foundation::HANDLE);

// SAFETY: a job handle is a kernel handle, usable from any thread; it is
// closed once, by `drop`.
#[cfg(windows)]
unsafe impl Send for Tree {}
#[cfg(windows)]
unsafe impl Sync for Tree {}

#[cfg(windows)]
impl Tree {
    fn holding(child: &Child) -> std::io::Result<Tree> {
        use windows_sys::Win32::System::JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
            SetInformationJobObject,
        };
        let process = child
            .raw_handle()
            .ok_or_else(|| std::io::Error::other("the child was reaped before its job was made"))?;
        // SAFETY: both pointers may be null (no security attributes, no
        // name); the result is a new handle or null.
        let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if job.is_null() {
            return Err(std::io::Error::last_os_error());
        }
        let tree = Tree(job);
        // SAFETY: an all-zero JOBOBJECT_EXTENDED_LIMIT_INFORMATION is valid
        // (no limits); the call reads exactly its size from it.
        let ok = unsafe {
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            SetInformationJobObject(
                tree.0,
                JobObjectExtendedLimitInformation,
                (&raw const info).cast(),
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        };
        if ok == 0 {
            return Err(std::io::Error::last_os_error());
        }
        // SAFETY: `job` is the handle made above and `process` the child's
        // own handle, open while `child` is not reaped (it was just spawned).
        if unsafe { AssignProcessToJobObject(tree.0, process) } == 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(tree)
    }

    /// Ends every process of the job, with exit code 1 (as libuv's `kill`
    /// ends a process).
    fn end(&self) {
        // SAFETY: the handle is this value's, open until it is dropped.
        unsafe {
            windows_sys::Win32::System::JobObjects::TerminateJobObject(self.0, 1);
        }
    }
}

#[cfg(windows)]
impl Drop for Tree {
    fn drop(&mut self) {
        // SAFETY: the handle is this value's and is closed only here; with
        // kill-on-job-close, closing it ends what still runs in the job.
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}

/// The last `count` characters of `text`.
fn tail(text: &str, count: usize) -> &str {
    let chars = text.chars().count();
    if chars <= count {
        return text;
    }
    let skip = chars - count;
    let at = text
        .char_indices()
        .nth(skip)
        .map_or(text.len(), |(at, _)| at);
    &text[at..]
}

/// A reply line as `engine.js` reads it: `MCBENCH_(PLACED|VERIFIED|TICKED|DONE)
/// <id> <ms>` answers request `id`, `MCBENCH_(PLACE_FAILED|VERIFY_FAILED|
/// TICK_FAILED|FAILED) <id> <reason>` fails it with "The engine failed:
/// \<reason\>".
fn reply(line: &str) -> Option<(u64, Result<(), EngineError>)> {
    let digits = |word: &str| !word.is_empty() && word.bytes().all(|b| b.is_ascii_digit());
    let (verb, rest) = line.split_once(' ')?;
    let done = [
        "MCBENCH_PLACED",
        "MCBENCH_VERIFIED",
        "MCBENCH_TICKED",
        "MCBENCH_DONE",
    ];
    let failed = [
        "MCBENCH_PLACE_FAILED",
        "MCBENCH_VERIFY_FAILED",
        "MCBENCH_TICK_FAILED",
        "MCBENCH_FAILED",
    ];
    if done.contains(&verb) {
        let (id, ms) = rest.split_once(' ')?;
        if !(digits(id) && digits(ms)) {
            return None;
        }
        return Some((id.parse().ok()?, Ok(())));
    }
    if failed.contains(&verb) {
        let (id, reason) = rest.split_once(' ')?;
        if !digits(id) {
            return None;
        }
        return Some((
            id.parse().ok()?,
            Err(EngineError::Failed(reason.to_string())),
        ));
    }
    None
}

/// `engine.js` `request`'s check: every argument is a word of the protocol,
/// which splits a line on spaces (`/^\S+$/`).
fn check_words(args: &[&str]) -> Result<(), EngineError> {
    for arg in args {
        if arg.is_empty() || arg.chars().any(char::is_whitespace) {
            return Err(EngineError::Stopped(format!(
                "An engine request's arguments cannot be empty or hold whitespace: {}",
                serde_json::to_string(arg).unwrap()
            )));
        }
    }
    Ok(())
}

/// Stops a child (`engine.js` `end`): `SIGTERM`, and `SIGKILL` if it has not
/// exited ten seconds later; resolves once it has exited.
async fn end(kills: mpsc::UnboundedSender<Signal>, mut exited: watch::Receiver<bool>) {
    let _ = kills.send(Signal::Term);
    if tokio::time::timeout(STOP_GRACE, exited.wait_for(|exited| *exited))
        .await
        .is_err()
    {
        let _ = kills.send(Signal::Kill);
        let _ = exited.wait_for(|exited| *exited).await;
    }
}

impl ChildEngine {
    /// The child's tasks: its stdout's replies, its stderr's tail, and its
    /// exit, which fails what waits and removes its folder.
    fn run(mut child: Child, tree: Tree, folder: PathBuf) -> ChildEngine {
        let pid = child.id().expect("a child just spawned has its pid");
        let stdin = child.stdin.take().expect("piped");
        let stdout = child.stdout.take().expect("piped");
        let mut stderr = child.stderr.take().expect("piped");
        let (phase, _) = watch::channel(Phase::Starting);
        let shared = Arc::new(Shared {
            phase,
            requests: Mutex::new(HashMap::new()),
            output: Mutex::new(String::new()),
            stopped: AtomicBool::new(false),
        });
        let replies = {
            let shared = shared.clone();
            tokio::spawn(async move {
                let mut lines = BufReader::new(stdout).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    // A child stopped meanwhile fails with its exit instead.
                    if line == "MCBENCH_ENGINE_READY" && !shared.stopped.load(Ordering::SeqCst) {
                        shared.phase.send_if_modified(|phase| {
                            let starting = matches!(phase, Phase::Starting);
                            if starting {
                                *phase = Phase::Ready;
                            }
                            starting
                        });
                    }
                    if let Some((id, answer)) = reply(&line) {
                        let waiting = shared.requests.lock().unwrap().remove(&id);
                        if let Some(waiting) = waiting {
                            let _ = waiting.send(answer);
                        }
                    }
                }
            })
        };
        let errors = {
            let shared = shared.clone();
            tokio::spawn(async move {
                let mut chunk = vec![0u8; 8192];
                while let Ok(read) = stderr.read(&mut chunk).await {
                    if read == 0 {
                        break;
                    }
                    let mut output = shared.output.lock().unwrap();
                    output.push_str(&String::from_utf8_lossy(&chunk[..read]));
                    let kept = tail(&output, 8000).len();
                    let cut = output.len() - kept;
                    output.drain(..cut);
                }
            })
        };
        let (kills, mut signals) = mpsc::unbounded_channel();
        let (exit, exited) = watch::channel(false);
        {
            let (shared, folder) = (shared.clone(), folder.clone());
            tokio::spawn(async move {
                let mut open = true;
                let mut sent = None;
                let status = loop {
                    tokio::select! {
                        status = child.wait() => break status,
                        signal = signals.recv(), if open => match signal {
                            Some(signal) => {
                                send(&mut child, &tree, signal);
                                sent = Some(signal);
                            }
                            None => open = false,
                        },
                    }
                };
                // The child exited: what it started still in its job goes.
                #[cfg(windows)]
                drop(tree);
                let _ = replies.await;
                let _ = errors.await;
                let words = match &status {
                    Ok(status) => exit_words(status, sent),
                    Err(error) => format!("code null, {error}"),
                };
                let message = format!(
                    "The engine stopped ({words}): {}",
                    tail(&shared.output.lock().unwrap(), 2000)
                );
                shared.phase.send_replace(Phase::Ended(message.clone()));
                let waiting: Vec<Answer> = shared
                    .requests
                    .lock()
                    .unwrap()
                    .drain()
                    .map(|(_, answer)| answer)
                    .collect();
                for answer in waiting {
                    let _ = answer.send(Err(EngineError::Stopped(message.clone())));
                }
                let _ = tokio::fs::remove_dir_all(&folder).await;
                let _ = exit.send(true);
            });
        }
        ChildEngine {
            pid,
            folder,
            shared,
            stdin: tokio::sync::Mutex::new(Some(stdin)),
            kills,
            exited,
            next: AtomicU64::new(0),
        }
    }

    /// The child's process id.
    pub fn pid(&self) -> u32 {
        self.pid
    }

    /// The child's own folder, its working directory.
    pub fn folder(&self) -> &Path {
        &self.folder
    }

    /// False once the child exited or [`ChildEngine::stop`] was called: it
    /// takes no more requests.
    pub fn alive(&self) -> bool {
        !self.shared.stopped.load(Ordering::SeqCst)
            && !matches!(*self.shared.phase.borrow(), Phase::Ended(_))
    }

    /// True once the child's process has exited and been reaped.
    pub fn exited(&self) -> bool {
        *self.exited.borrow()
    }

    /// Stops the child without waiting for it (a cancel).
    fn stop_now(&self) {
        self.shared.stopped.store(true, Ordering::SeqCst);
        tokio::spawn(end(self.kills.clone(), self.exited.clone()));
    }

    /// Ends the child (`engine.js` `stop`): `SIGTERM`, then `SIGKILL` after
    /// 10 s. Resolves once it has exited; a second stop waits for the same
    /// exit.
    pub async fn stop(&self) {
        self.shared.stopped.store(true, Ordering::SeqCst);
        end(self.kills.clone(), self.exited.clone()).await
    }

    /// Resolves once the child said it is ready, or fails with its exit.
    pub async fn ready(&self, cancel: &CancellationToken) -> Result<(), EngineError> {
        let mut phase = self.shared.phase.subscribe();
        let phase = tokio::select! {
            phase = phase.wait_for(|phase| !matches!(phase, Phase::Starting)) => {
                phase.expect("the phase's sender lives in the handle").clone()
            }
            _ = cancel.cancelled() => {
                self.stop_now();
                return Err(EngineError::Cancelled);
            }
        };
        match phase {
            Phase::Ended(message) => Err(EngineError::Stopped(message)),
            _ => Ok(()),
        }
    }

    /// Sends `verb` with `args` (words of the protocol, which splits a line
    /// on spaces) once the child is ready, and answers its reply.
    async fn request(
        &self,
        verb: &str,
        args: &[&str],
        cancel: &CancellationToken,
    ) -> Result<(), EngineError> {
        check_words(args)?;
        self.ready(cancel).await?;
        let id = self.next.fetch_add(1, Ordering::SeqCst) + 1;
        let (answer, answered) = oneshot::channel();
        {
            let mut requests = self.shared.requests.lock().unwrap();
            if let Some(error) = self.ended() {
                return Err(error);
            }
            requests.insert(id, answer);
        }
        let mut line = format!("{verb} {id}");
        for arg in args {
            line.push(' ');
            line.push_str(arg);
        }
        line.push('\n');
        {
            let mut stdin = self.stdin.lock().await;
            // A child that cannot be written to has exited or is exiting:
            // its exit fails the request (engine.js ignores stdin's errors).
            if let Some(pipe) = stdin.as_mut()
                && (pipe.write_all(line.as_bytes()).await.is_err() || pipe.flush().await.is_err())
            {
                stdin.take();
            }
        }
        tokio::select! {
            answer = answered => answer.unwrap_or_else(|_| Err(EngineError::Stopped("The engine was stopped".into()))),
            _ = cancel.cancelled() => {
                self.stop_now();
                Err(EngineError::Cancelled)
            }
        }
    }

    fn file(&self, name: &str) -> PathBuf {
        self.folder.join(name)
    }

    /// Why the child takes no request: it was stopped, or it exited.
    fn ended(&self) -> Option<EngineError> {
        if self.shared.stopped.load(Ordering::SeqCst) {
            return Some(EngineError::Stopped("The engine was stopped".into()));
        }
        match &*self.shared.phase.borrow() {
            Phase::Ended(message) => Some(EngineError::Stopped(message.clone())),
            _ => None,
        }
    }

    /// Writes a request's file into the child's folder. A child that ended
    /// fails as its requests do (its folder goes with it).
    async fn write(&self, name: &str, bytes: &[u8]) -> Result<(), EngineError> {
        if let Some(error) = self.ended() {
            return Err(error);
        }
        let path = self.file(name);
        tokio::fs::write(&path, bytes).await.map_err(|error| {
            self.ended()
                .unwrap_or_else(|| EngineError::Stopped(format!("{}: {error}", path.display())))
        })
    }

    async fn read(&self, name: &str) -> Result<Vec<u8>, EngineError> {
        let path = self.file(name);
        tokio::fs::read(&path)
            .await
            .map_err(|error| EngineError::Stopped(format!("{}: {error}", path.display())))
    }

    /// `PLACE`: places the commands on an empty world (`fresh`) or on the
    /// world the last placement left, and returns the blueprint.
    pub async fn place(
        &self,
        commands: &PlaceCommands,
        fresh: bool,
        cancel: &CancellationToken,
    ) -> Result<Blueprint, EngineError> {
        self.write("place.txt", commands.0.as_bytes()).await?;
        let mode = if fresh { "fresh" } else { "continue" };
        self.request("PLACE", &["place.txt", "blueprint.bin", mode], cancel)
            .await?;
        Ok(Blueprint(self.read("blueprint.bin").await?))
    }

    /// `TICK`: runs `ticks` game ticks, lighting what they changed when
    /// `light` (the stock request), or not (`nolight`), and returns the
    /// patch and the entities.
    pub async fn tick(
        &self,
        ticks: i32,
        light: bool,
        cancel: &CancellationToken,
    ) -> Result<TickFiles, EngineError> {
        let ticks = ticks.to_string();
        let mut args = vec![ticks.as_str(), "patch.bin", "entities.json"];
        if !light {
            args.push("nolight");
        }
        self.request("TICK", &args, cancel).await?;
        Ok(TickFiles {
            changes: self.read("patch.bin").await?,
            entities: self.read("entities.json").await?,
        })
    }

    /// `SURVIVE`: the blocks that cannot survive where they are, a line `x y
    /// z` each.
    pub async fn unsupported(&self, cancel: &CancellationToken) -> Result<String, EngineError> {
        self.request("SURVIVE", &["survive.txt"], cancel).await?;
        Ok(String::from_utf8_lossy(&self.read("survive.txt").await?).into_owned())
    }

    /// `STATE`: what the world holds besides its blocks.
    pub async fn state(&self, cancel: &CancellationToken) -> Result<String, EngineError> {
        self.request("STATE", &["state.txt"], cancel).await?;
        Ok(String::from_utf8_lossy(&self.read("state.txt").await?).into_owned())
    }

    /// Runs a `test_build` experiment (`VERIFY`) on a level of its own: a
    /// refusal or a failure leaves the child's world as it was. `progress`
    /// is given the experiment's `progress.json` each time it changes
    /// (polled every 100 ms, as `server/minecraft-verify.js` reads it).
    /// `Err` is [`EngineError::Failed`] with the reason the engine replies
    /// `MCBENCH_VERIFY_FAILED` with, the child's exit, or
    /// [`EngineError::Cancelled`] at once when `cancel` fires (the child is
    /// stopped).
    pub async fn verify(
        &self,
        request: &VerifyRequest,
        mut progress: impl FnMut(&str) + Send,
        cancel: &CancellationToken,
    ) -> Result<VerifyReport, EngineError> {
        if cancel.is_cancelled() {
            return Err(EngineError::Cancelled);
        }
        if let Some(error) = self.ended() {
            return Err(error);
        }
        let run = format!("verify-{}", self.next.load(Ordering::SeqCst) + 1);
        tokio::fs::create_dir(self.file(&run))
            .await
            .map_err(|error| {
                EngineError::Stopped(format!("{}: {error}", self.file(&run).display()))
            })?;
        let (request_file, commands_file, out) = (
            format!("{run}/request.json"),
            format!("{run}/commands.txt"),
            format!("{run}/out"),
        );
        self.write(&request_file, request.request.as_bytes())
            .await?;
        self.write(&commands_file, request.commands.0.as_bytes())
            .await?;
        let progress_file = self.file(&format!("{out}/progress.json"));
        let words = [request_file.as_str(), commands_file.as_str(), out.as_str()];
        let answer = self.request("VERIFY", &words, cancel);
        tokio::pin!(answer);
        let mut last = String::new();
        let mut poll = tokio::time::interval(Duration::from_millis(100));
        let answer = loop {
            tokio::select! {
                answer = &mut answer => break answer,
                _ = poll.tick() => {
                    if let Ok(text) = tokio::fs::read_to_string(&progress_file).await
                        && text != last
                    {
                        progress(&text);
                        last = text;
                    }
                }
            }
        };
        let report = match answer {
            Ok(()) => read_report(&self.file(&out)).await,
            Err(error) => Err(error),
        };
        if !matches!(report, Err(EngineError::Cancelled)) {
            let _ = tokio::fs::remove_dir_all(self.file(&run)).await;
        }
        report
    }
}

impl Drop for ChildEngine {
    fn drop(&mut self) {
        let _ = self.kills.send(Signal::Kill);
    }
}

async fn read_report(folder: &Path) -> Result<VerifyReport, EngineError> {
    let failed =
        |error: std::io::Error| EngineError::Stopped(format!("{}: {error}", folder.display()));
    let mut result = None;
    let mut files = Vec::new();
    let mut entries = tokio::fs::read_dir(folder).await.map_err(failed)?;
    while let Some(entry) = entries.next_entry().await.map_err(failed)? {
        let name = entry.file_name().to_string_lossy().into_owned();
        let bytes = tokio::fs::read(entry.path()).await.map_err(failed)?;
        if name == "result.json" {
            result = Some(
                String::from_utf8(bytes)
                    .map_err(|_| EngineError::Stopped("result.json is not UTF-8".into()))?,
            );
        } else {
            files.push((name, bytes));
        }
    }
    files.sort();
    let result = result.ok_or_else(|| {
        EngineError::Stopped(format!(
            "VERIFY wrote no result.json in {}",
            folder.display()
        ))
    })?;
    Ok(VerifyReport { result, files })
}

/// Removes the engine children's folders under `folder` (the system's
/// temporary folder) that a process which ended mid-run left
/// (`minecraft-verify.js` `sweepRuns`): those last changed over `age_ms`
/// ago and not of a process that is running (on Unix by `kill(pid, 0)`; on
/// Windows a running child's working directory cannot be removed). This
/// process's own are kept.
pub fn sweep_children(folder: &Path, age_ms: f64) {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return;
    };
    let now = std::time::SystemTime::now();
    let own = format!("{CHILD_FOLDER}{}-", std::process::id());
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(rest) = name.strip_prefix(CHILD_FOLDER) else {
            continue;
        };
        if name.starts_with(&own) {
            continue;
        }
        let path = entry.path();
        let age = std::fs::metadata(&path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|modified| now.duration_since(modified).ok())
            .map_or(0.0, |d| d.as_secs_f64() * 1000.0);
        if age <= age_ms {
            continue;
        }
        #[cfg(unix)]
        {
            let pid: Option<i32> = rest.split('-').next().and_then(|pid| pid.parse().ok());
            let running = pid.is_some_and(|pid| {
                // SAFETY: kill(2) with signal 0 sends nothing and touches no
                // memory; it says whether a process with that pid exists
                // (EPERM: it exists and is another user's).
                pid > 0
                    && (unsafe { libc::kill(pid, 0) } == 0
                        || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM))
            });
            if running {
                continue;
            }
        }
        #[cfg(windows)]
        let _ = rest;
        let _ = std::fs::remove_dir_all(&path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reply_reads_as_engine_js_reads_it() {
        assert_eq!(reply("MCBENCH_PLACED 1 3"), Some((1, Ok(()))));
        assert_eq!(reply("MCBENCH_DONE 12 0"), Some((12, Ok(()))));
        assert_eq!(reply("MCBENCH_PLACED 1 x"), None);
        assert_eq!(reply("MCBENCH_PLACED 1"), None);
        assert_eq!(
            reply(
                "MCBENCH_VERIFY_FAILED 3 java.lang.UnsupportedOperationException: Light is not modelled"
            ),
            Some((
                3,
                Err(EngineError::Failed(
                    "java.lang.UnsupportedOperationException: Light is not modelled".into()
                ))
            ))
        );
        assert_eq!(reply("MCBENCH_FAILED ? unknown request X"), None);
        assert_eq!(reply("MCBENCH_ENGINE_READY"), None);
        assert_eq!(tail("abcdef", 2), "ef");
        assert_eq!(tail("ab", 5), "ab");
    }

    // engine.test.js: "a request argument holding whitespace is refused
    // before it reaches the engine". The words of every request are this
    // module's file names, relative to the child's folder; the check holds
    // them to the protocol all the same.
    #[test]
    fn a_request_argument_holding_whitespace_is_refused() {
        assert_eq!(
            check_words(&["my commands.txt", "b.bin"])
                .unwrap_err()
                .to_string(),
            "An engine request's arguments cannot be empty or hold whitespace: \"my commands.txt\""
        );
        assert!(check_words(&[""]).is_err());
        assert!(check_words(&["place.txt", "verify-1/out"]).is_ok());
    }

    #[test]
    fn a_fixed_line_reads_as_fixed_palette_reads_it() {
        let chain = fixed_state(
            "ok {Name:\"minecraft:iron_chain\",Properties:{axis:\"x\",waterlogged:\"false\"}}",
        )
        .unwrap();
        assert_eq!(chain.id, "minecraft:iron_chain");
        assert_eq!(
            chain.properties,
            vec![
                ("axis".to_string(), "x".to_string()),
                ("waterlogged".to_string(), "false".to_string())
            ]
        );
        assert_eq!(
            fixed_state("ok {Name:\"minecraft:stone\"}").unwrap().id,
            "minecraft:stone"
        );
        assert!(fixed_state("unported: something").is_none());
        assert_eq!(
            property_pairs("\"a\":\"1\",b:\"2\""),
            vec![("a".into(), "1".into()), ("b".into(), "2".into())]
        );
    }
}
