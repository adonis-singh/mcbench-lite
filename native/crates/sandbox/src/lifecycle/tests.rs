//! The lifecycle with the boxes faked: the ports of
//! `sandbox-lifecycle.test.js`, `sandbox-storage.test.js`,
//! `sandbox-network.test.js` and `sandbox.test.js` (each test names its
//! Node test), against a [`Backend`] that records what it is asked.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

use super::{IDLE, Sandboxes};
use crate::backend::{OneShotRun, ProgramRun, SessionRun};
use crate::testing::TestBudget;
use crate::{Backend, BashCommand, Sandbox, SandboxError, Workspace};

const MIB: u64 = 1024 * 1024;

/// A predicate on calls and the release they wait for.
type Hold = (
    Box<dyn Fn(&str) -> bool + Send + Sync>,
    watch::Receiver<bool>,
);

type OnOneShot = Box<dyn Fn(&str) -> OneShotRun + Send + Sync>;

/// The fake boxes: which exist, which shells are open, every call as a
/// line, and calls held until released.
#[derive(Default)]
struct Fake {
    calls: Mutex<Vec<String>>,
    boxes: Mutex<HashSet<(String, bool)>>,
    sessions: Mutex<HashSet<String>>,
    hold: Mutex<Option<Hold>>,
    on_one_shot: Mutex<Option<OnOneShot>>,
    previews: Mutex<HashMap<String, Vec<u8>>>,
    unremovable: Mutex<HashSet<String>>,
}

#[derive(Clone)]
struct FakeBackend(Arc<Fake>);

impl FakeBackend {
    async fn call(&self, line: String) {
        self.0.calls.lock().unwrap().push(line.clone());
        let held = self
            .0
            .hold
            .lock()
            .unwrap()
            .as_ref()
            .and_then(|(matches, released)| matches(&line).then(|| released.clone()));
        if let Some(mut released) = held {
            let _ = released.wait_for(|done| *done).await;
        }
    }
    fn calls(&self) -> Vec<String> {
        self.0.calls.lock().unwrap().clone()
    }
    fn clear(&self) {
        self.0.calls.lock().unwrap().clear();
    }
    fn count(&self, prefix: &str) -> usize {
        self.calls()
            .iter()
            .filter(|line| line.starts_with(prefix))
            .count()
    }
    /// Holds the calls `matches` picks until the sender is dropped or sent.
    fn hold(&self, matches: impl Fn(&str) -> bool + Send + Sync + 'static) -> watch::Sender<bool> {
        let (tx, rx) = watch::channel(false);
        *self.0.hold.lock().unwrap() = Some((Box::new(matches), rx));
        tx
    }
}

fn net(network: bool) -> &'static str {
    if network { "net" } else { "box" }
}

#[async_trait]
impl Backend for FakeBackend {
    async fn prepare(&self) -> Result<(), SandboxError> {
        Ok(())
    }
    async fn ensure(&self, workspace: &Workspace, network: bool) -> Result<(), SandboxError> {
        let exists = self
            .0
            .boxes
            .lock()
            .unwrap()
            .contains(&(workspace.chat.clone(), network));
        if !exists {
            self.call(format!("make {} {}", net(network), workspace.chat))
                .await;
            self.0
                .boxes
                .lock()
                .unwrap()
                .insert((workspace.chat.clone(), network));
        }
        Ok(())
    }
    async fn has_session(&self, workspace: &Workspace) -> bool {
        self.0.sessions.lock().unwrap().contains(&workspace.chat)
    }
    async fn session(
        &self,
        workspace: &Workspace,
        command: &str,
        _: f64,
        _: crate::backend::Tap,
        _: &CancellationToken,
    ) -> Result<SessionRun, SandboxError> {
        if self
            .0
            .sessions
            .lock()
            .unwrap()
            .insert(workspace.chat.clone())
        {
            self.call(format!("shell {}", workspace.chat)).await;
        }
        self.call(format!("session {} {command}", workspace.chat))
            .await;
        Ok(SessionRun {
            text: format!("ran: {command}"),
            exit_code: 0,
            timed_out: false,
            reset: false,
        })
    }
    async fn kill_session(&self, chat: &str) {
        self.0.sessions.lock().unwrap().remove(chat);
    }
    async fn one_shot(
        &self,
        workspace: &Workspace,
        network: bool,
        command: &str,
        _: f64,
        limits: &str,
        _: crate::backend::Tap,
        _: &CancellationToken,
    ) -> Result<OneShotRun, SandboxError> {
        self.call(format!(
            "oneshot {} {} {limits}{command}",
            net(network),
            workspace.chat
        ))
        .await;
        let hook = self.0.on_one_shot.lock().unwrap();
        Ok(match hook.as_ref() {
            Some(hook) => hook(command),
            None => OneShotRun {
                output: format!("ran: {command}\n"),
                code: 0,
            },
        })
    }
    // Asked of both boxes whether or not they exist, as today's
    // `docker exec <name> bash -c 'kill -KILL -1'` is.
    async fn end_processes(&self, chat: &str, network: bool) {
        self.call(format!("kill {} {chat}", net(network))).await;
    }
    async fn program(
        &self,
        workspace: &Workspace,
        argv: &[String],
        _: Duration,
    ) -> Result<ProgramRun, SandboxError> {
        self.call(format!("program {} {}", workspace.chat, argv[0]))
            .await;
        let name = argv.last().unwrap();
        Ok(match self.0.previews.lock().unwrap().get(name) {
            Some(png) => ProgramRun {
                stdout: png.clone(),
                all: [&png[..3], b"Image conversion diagnostic\n", &png[3..]].concat(),
                code: 0,
            },
            None => ProgramRun {
                stdout: vec![],
                all: b"Pillow could not decode the image".to_vec(),
                code: 1,
            },
        })
    }
    async fn remove(&self, chat: &str) {
        for network in [false, true] {
            self.call(format!("remove {} {chat}", net(network))).await;
        }
        if !self.0.unremovable.lock().unwrap().contains(chat) {
            self.0.boxes.lock().unwrap().retain(|(id, _)| id != chat);
        }
    }
    async fn shutdown(&self) {}
}

struct Setup {
    _dir: tempdir::Dir,
    root: PathBuf,
    fake: FakeBackend,
    budget: Arc<TestBudget>,
    sandbox: Sandboxes<FakeBackend>,
}

mod tempdir {
    /// A folder removed when dropped.
    pub struct Dir(pub std::path::PathBuf);
    impl Dir {
        pub fn new(label: &str) -> Dir {
            use std::sync::atomic::{AtomicU32, Ordering};
            static N: AtomicU32 = AtomicU32::new(0);
            let path = std::env::temp_dir().join(format!(
                "mcbench-sandbox-{label}-{}-{}",
                std::process::id(),
                N.fetch_add(1, Ordering::SeqCst)
            ));
            std::fs::create_dir_all(&path).unwrap();
            Dir(path)
        }
    }
    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

fn setup(label: &str, chat_max: u64) -> Setup {
    let dir = tempdir::Dir::new(label);
    let root = dir.0.join("sandbox");
    std::fs::create_dir_all(&root).unwrap();
    let fake = FakeBackend(Arc::new(Fake::default()));
    let budget = Arc::new(TestBudget::new(root.clone(), chat_max));
    let sandbox = Sandboxes::new(fake.clone(), budget.clone());
    Setup {
        _dir: dir,
        root,
        fake,
        budget,
        sandbox,
    }
}

impl Setup {
    fn ws(&self, chat: &str) -> Workspace {
        let ws = Workspace {
            chat: chat.to_string(),
            host: self.root.join(chat),
        };
        crate::work::work_dir(&ws).unwrap();
        ws
    }
    async fn exec(
        &self,
        chat: &str,
        command: &str,
        network: bool,
    ) -> Result<crate::BashRun, SandboxError> {
        let cmd = BashCommand {
            command: command.to_string(),
            timeout_ms: Some(10_000.0),
            network,
        };
        self.sandbox
            .exec(&self.ws(chat), &cmd, &CancellationToken::new())
            .await
    }
    fn disk_under_reserve(&self) {
        self.budget.free.store(1000 * 4096, Ordering::SeqCst);
    }
}

async fn pause(ms: u64) {
    tokio::time::sleep(Duration::from_millis(ms)).await;
}

// sandbox-lifecycle.test.js: "the watchdog does not end the processes of a
// chat while its write-disabled command runs"
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_watchdog_does_not_end_the_processes_of_a_chat_while_its_write_disabled_command_runs() {
    let s = setup("cleanup", MIB);
    let id = "cleanup";
    s.disk_under_reserve();
    s.ws(id);
    s.budget.measure_now(id).await;
    let release = s.fake.hold(|line| line.starts_with("oneshot"));
    let running = {
        let ws = s.ws(id);
        let sandbox = &s.sandbox;
        async move {
            sandbox
                .exec(
                    &ws,
                    &BashCommand {
                        command: "rm -rf scratch".into(),
                        timeout_ms: Some(10_000.0),
                        network: false,
                    },
                    &CancellationToken::new(),
                )
                .await
        }
    };
    tokio::pin!(running);
    tokio::select! {
        _ = &mut running => panic!("the held command finished"),
        () = async { while s.fake.count("oneshot") == 0 { pause(1).await } } => {}
    }
    s.fake.clear();
    let (watched, _) = tokio::join!(s.sandbox.watch_storage(), async {});
    watched.unwrap();
    assert_eq!(
        s.fake.count("kill"),
        0,
        "the command that frees space is left running"
    );
    release.send(true).unwrap();
    let result = running.await.unwrap();
    assert_eq!(result.exit_code, 0);
    assert!(
        result.formatted.starts_with("Exit code: 0")
            && result.formatted.contains("ran: rm -rf scratch"),
        "{}",
        result.formatted
    );
    assert!(
        result
            .storage
            .unwrap()
            .starts_with("The disk holding the data folder has 3.9 MiB free")
    );
    // With no such command running, the chat's processes are ended as before.
    s.fake.clear();
    s.sandbox.watch_storage().await.unwrap();
    assert_eq!(s.fake.count("kill"), 2, "both of its boxes");
}

// sandbox-lifecycle.test.js: "a halt under way ends before the chat's
// write-disabled command starts"
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_halt_under_way_ends_before_the_chats_write_disabled_command_starts() {
    let s = setup("halting", MIB);
    let id = "halting";
    s.disk_under_reserve();
    s.exec(id, "true", false).await.unwrap();
    s.exec(id, "true", true).await.unwrap();
    // The watchdog's kills of this chat stay open.
    let release = s.fake.hold(|line| line.starts_with("kill"));
    s.fake.clear();
    let sandbox = &s.sandbox;
    let watching = sandbox.watch_storage();
    tokio::pin!(watching);
    tokio::select! {
        _ = &mut watching => panic!("the held watchdog finished"),
        () = async { while s.fake.count("kill") < 2 { pause(1).await } } => {}
    }
    let running = s.exec(id, "rm -rf scratch", false);
    tokio::pin!(running);
    tokio::select! {
        _ = &mut running => panic!("the command did not wait"),
        _ = &mut watching => panic!("the held watchdog finished"),
        () = pause(30) => {}
    }
    assert_eq!(
        s.fake.count("oneshot"),
        0,
        "the command waits for the kills to land"
    );
    release.send(true).unwrap();
    let (result, watched) = tokio::join!(running, watching);
    let result = result.unwrap();
    assert!(
        result.formatted.starts_with("Exit code: 0")
            && result.formatted.contains("ran: rm -rf scratch")
    );
    watched.unwrap();
}

// sandbox-lifecycle.test.js: "a command that arrives while the idle reaper
// removes the containers waits, and runs in a fresh container"
#[cfg_attr(
    windows,
    ignore = "the WSL2 distro for bash is not built yet (the test box's free space is its disk's)"
)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_command_that_arrives_while_the_idle_reaper_removes_the_boxes_waits_and_runs_in_a_fresh_box()
 {
    let s = setup("reaped", 4 * 1024 * MIB);
    let id = "reaped";
    let first = s.exec(id, "echo one", false).await.unwrap();
    assert!(first.formatted.contains("ran: echo one"));
    assert_eq!(s.fake.count("make"), 1);
    let release = s.fake.hold(|line| line.starts_with("remove"));
    s.sandbox
        .reap_idle_at(Instant::now() + IDLE + Duration::from_secs(60));
    while s.fake.count("remove") < 1 {
        pause(1).await;
    }
    s.fake.clear();
    let second = s.exec(id, "echo two", false);
    tokio::pin!(second);
    tokio::select! {
        _ = &mut second => panic!("the command did not wait"),
        () = pause(30) => {}
    }
    assert_eq!(
        s.fake.calls(),
        Vec::<String>::new(),
        "nothing is asked of a box that is being removed"
    );
    release.send(true).unwrap();
    let result = second.await.unwrap();
    assert!(result.formatted.contains("ran: echo two"));
    let calls: Vec<String> = s
        .fake
        .calls()
        .into_iter()
        .filter(|line| !line.starts_with("remove"))
        .collect();
    assert_eq!(
        calls,
        ["make box reaped", "shell reaped", "session reaped echo two"],
        "a fresh box and shell"
    );
}

// sandbox-lifecycle.test.js: "a reset that arrives during a removal waits
// for it, and removes nothing twice"
#[cfg_attr(
    windows,
    ignore = "the WSL2 distro for bash is not built yet (the test box's free space is its disk's)"
)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_reset_that_arrives_during_a_removal_waits_for_it_and_removes_nothing_twice() {
    let s = setup("twice", 4 * 1024 * MIB);
    let id = "twice";
    s.exec(id, "echo one", false).await.unwrap();
    s.fake.clear();
    let release = s.fake.hold(|line| line.starts_with("remove"));
    let both = async { tokio::join!(s.sandbox.reset(id), s.sandbox.reset(id)) };
    tokio::pin!(both);
    // Both resets are under way once the first one's removal is held (the
    // join has polled the second by then).
    tokio::select! {
        _ = &mut both => panic!("the resets did not wait"),
        () = async { while s.fake.count("remove") < 1 { pause(1).await } } => {}
    }
    release.send(true).unwrap();
    both.await;
    assert_eq!(
        s.fake.calls(),
        ["remove box twice", "remove net twice"],
        "one removal of each box"
    );
    assert!(
        !s.sandbox.tracked().contains(&id.to_string()),
        "a removed sandbox is forgotten"
    );
}

// sandbox-lifecycle.test.js: "shutdown removes every container this process
// tracks, and a Docker that cannot be reached does not fail it"
#[cfg_attr(
    windows,
    ignore = "the WSL2 distro for bash is not built yet (the test box's free space is its disk's)"
)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shutdown_removes_every_box_this_process_tracks_and_one_that_cannot_be_removed_does_not_fail_it()
 {
    let s = setup("shutdown", 4 * 1024 * MIB);
    s.exec("kept", "echo a", false).await.unwrap();
    s.exec("unreachable", "echo b", false).await.unwrap();
    s.fake
        .0
        .unremovable
        .lock()
        .unwrap()
        .insert("unreachable".into());
    s.fake.clear();
    s.sandbox.shutdown().await;
    let mut removed: Vec<String> = s
        .fake
        .calls()
        .into_iter()
        .filter(|line| line.starts_with("remove"))
        .collect();
    removed.sort();
    assert_eq!(
        removed,
        [
            "remove box kept",
            "remove box unreachable",
            "remove net kept",
            "remove net unreachable"
        ]
    );
    assert_eq!(
        s.sandbox.tracked(),
        Vec::<String>::new(),
        "nothing is tracked"
    );
    assert!(
        !s.fake
            .0
            .boxes
            .lock()
            .unwrap()
            .iter()
            .any(|(id, _)| id == "kept"),
        "the removable boxes are gone"
    );
}

// sandbox-storage.test.js: "over its budget a chat's bash command runs with
// file writes disabled, and says so"
#[cfg_attr(
    windows,
    ignore = "the WSL2 distro for bash is not built yet (the test box's free space is its disk's)"
)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn over_its_budget_a_chats_bash_command_runs_with_file_writes_disabled_and_says_so() {
    let s = setup("storage", MIB);
    let id = "storage-chat";
    let big = s.ws(id).host.join("big.bin");
    std::fs::write(&big, vec![0u8; 2 * MIB as usize]).unwrap();
    s.budget.measure_now(id).await;
    let removed = big.clone();
    *s.fake.0.on_one_shot.lock().unwrap() = Some(Box::new(move |command| {
        assert_eq!(command, "rm big.bin");
        std::fs::remove_file(&removed).unwrap();
        OneShotRun {
            output: "removed\n".into(),
            code: 0,
        }
    }));
    s.fake.clear();
    let result = s.exec(id, "rm big.bin", false).await.unwrap();
    let calls = s.fake.calls();
    let run = calls
        .iter()
        .find(|line| line.ends_with("rm big.bin"))
        .unwrap();
    assert!(
        run.starts_with("oneshot box storage-chat ulimit -f 0; trap \"\" XFSZ; rm big.bin"),
        "{run}"
    );
    let kill = calls
        .iter()
        .position(|line| line == "kill box storage-chat")
        .expect("the session and its background jobs are ended first");
    assert!(kill < calls.iter().position(|line| line == run).unwrap());
    let storage = result.storage.unwrap();
    assert!(storage.starts_with("This chat's storage is full: its attachments, images, world and /work hold 2.0 MiB of its 1.0 MiB budget (MCBENCH_CHAT_MAX_BYTES). While that lasts, bash runs each command in a fresh shell with file writes disabled (ulimit -f 0)"), "{storage}");
    assert!(storage.ends_with(
        "After this command the chat is back within its limits; the next command runs normally."
    ));
    assert!(result.formatted.starts_with("Exit code: 0") && result.formatted.contains("removed"));
    assert_eq!(crate::Budget::refusal(&*s.budget, id).await.unwrap(), None);
}

// sandbox-storage.test.js: "the agent returns that command as a failed bash
// call with its output" (the envelope, `{ ok: false, error, output }`, is
// the tools crate's; here the run carries both parts)
#[cfg_attr(
    windows,
    ignore = "the WSL2 distro for bash is not built yet (the test box's free space is its disk's)"
)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_write_disabled_command_answers_its_storage_message_and_its_output() {
    let s = setup("storage2", MIB);
    let id = "storage-chat";
    std::fs::write(s.ws(id).host.join("big.bin"), vec![0u8; 2 * MIB as usize]).unwrap();
    s.budget.measure_now(id).await;
    *s.fake.0.on_one_shot.lock().unwrap() = Some(Box::new(|_| OneShotRun {
        output: "bash: line 1: echo: write error: File too large\n".into(),
        code: 1,
    }));
    let result = s.exec(id, "echo more > more.txt", false).await.unwrap();
    let storage = result.storage.unwrap();
    assert!(
        storage.contains("storage is full")
            && storage.contains("After this command: This chat's storage is full"),
        "{storage}"
    );
    assert!(result.formatted.contains("File too large"));
}

// sandbox-storage.test.js: "the watchdog ends the processes of a chat whose
// /work passed its budget, once the disk loses space"
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_watchdog_ends_the_processes_of_a_chat_whose_work_passed_its_budget_once_the_disk_loses_space()
 {
    let s = setup("watchdog", MIB);
    let id = "storage-chat";
    let gib = 1024 * MIB;
    s.budget.free.store(50 * gib, Ordering::SeqCst);
    s.exec(id, "true", false).await.unwrap();
    s.exec(id, "true", true).await.unwrap();
    s.budget.measure_now(id).await;
    s.sandbox.watch_storage().await.unwrap(); // the baseline
    std::fs::write(s.ws(id).host.join("big.bin"), vec![0u8; 2 * MIB as usize]).unwrap();
    s.fake.clear();
    s.budget.free.fetch_sub(30 * MIB, Ordering::SeqCst);
    s.sandbox.watch_storage().await.unwrap();
    assert_eq!(s.fake.count("kill"), 0, "a small drop measures nothing");
    s.budget.free.fetch_sub(300 * MIB, Ordering::SeqCst);
    s.sandbox.watch_storage().await.unwrap();
    assert_eq!(s.fake.count("kill"), 2, "both of its boxes");
    s.fake.clear();
    s.sandbox.watch_storage().await.unwrap();
    s.budget.free.fetch_sub(300 * MIB, Ordering::SeqCst);
    s.sandbox.watch_storage().await.unwrap();
    assert_eq!(
        s.fake.count("kill"),
        0,
        "ended once until a command learns why"
    );
    // The next command learns why, once.
    let error = s.exec(id, "true", false).await;
    assert!(
        error.as_ref().unwrap().storage.is_some(),
        "over its budget the command runs write-disabled"
    );
}

// sandbox-storage.test.js: "under the disk's reserve every chat's command
// runs with writes disabled"
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn under_the_disks_reserve_every_chats_command_runs_with_writes_disabled() {
    let s = setup("reserve", MIB);
    s.disk_under_reserve();
    let result = s.exec("storage-chat", "ls", false).await.unwrap();
    assert!(result.storage.unwrap().starts_with("The disk holding the data folder has 3.9 MiB free; mcbench-lite keeps 2.00 GiB free (MCBENCH_DISK_RESERVE_BYTES)"));
}

// sandbox-network.test.js: "the chat app keeps the one-shot networked bash
// mode" (the schema and the prompt are the agent's)
#[tokio::test]
async fn the_chat_app_keeps_the_one_shot_networked_bash_mode() {
    let s = setup("network", MIB);
    assert!(s.sandbox.network_access());
}

// sandbox-network.test.js: "benchmark mode removes network from the schema
// and refuses it in execution"
#[tokio::test]
async fn benchmark_mode_refuses_the_networked_command_without_starting_anything() {
    let s = setup("offline", MIB);
    s.sandbox.disable_network();
    assert!(!s.sandbox.network_access());
    let error = s
        .exec("offline", "curl https://example.com", true)
        .await
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "Network access is disabled in this workspace; the bash tool has no network mode here"
    );
    assert_eq!(s.fake.calls(), Vec::<String>::new());
}

// sandbox.test.js: "image previews preserve binary bytes, exclude stderr,
// and never share an output file"
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn image_previews_preserve_binary_bytes_exclude_stderr_and_never_share_an_output_file() {
    let s = setup("preview", MIB);
    let ws = s.ws("preview-test");
    let previews = [
        ("first.png", vec![0x89, 0x50, 0x4e, 0x47, 0, 0xff, 0xfe, 1]),
        ("second.png", vec![0x89, 0x50, 0x4e, 0x47, 0, 0x80, 0xfe, 2]),
    ];
    for (name, png) in &previews {
        s.fake
            .0
            .previews
            .lock()
            .unwrap()
            .insert(name.to_string(), png.clone());
    }
    for name in ["first.png", "second.png", "invalid.png"] {
        std::fs::write(ws.host.join(name), "source").unwrap();
    }
    let listing = || {
        let mut names: Vec<String> = std::fs::read_dir(&ws.host)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    };
    let before = listing();
    let (a, b) = tokio::join!(
        s.sandbox.view_image(&ws, "first.png"),
        s.sandbox.view_image(&ws, "second.png")
    );
    assert_eq!(
        [a.unwrap(), b.unwrap()],
        [previews[0].1.clone(), previews[1].1.clone()]
    );
    assert_eq!(
        s.sandbox.view_image(&ws, "first.png").await.unwrap(),
        previews[0].1
    );
    let error = s
        .sandbox
        .view_image(&ws, "invalid.png")
        .await
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("Pillow could not decode the image"),
        "{error}"
    );
    assert_eq!(listing(), before);
}

impl TestBudget {
    async fn measure_now(&self, chat: &str) {
        crate::Budget::measure(self, chat).await.unwrap();
    }
}
