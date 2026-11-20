//! A core started at once on the data folder of one that was killed in the
//! middle of a `bash` call, as the error screen's `Restart` starts the app
//! again while what the ended process left is still ending.
//!
//! Core A runs in a child process of this test binary (the test re-run with
//! `MCBENCH_SECOND_CORE_DIR` set) and its turn's `bash` call keeps running in
//! the real Linux box. A is killed with SIGKILL, the way the crash path ends
//! a process (no `Core::shutdown`), and core B is started in this process at
//! once on the same folder, without waiting for A's children. B takes the
//! folder (the lock judged by A's dead pid), ends the turn A left as today's
//! server's start does, A's box ends (its systemd scope empties: the keeper
//! ends its box when A's end of the control socket closes), and a new turn
//! of B on the same chat runs `bash` in a fresh box.
//!
//! The `bash` tool here is the sandbox call of `mcbench_tools::bash` over
//! the app's own sandbox (`PlatformSandbox` of the data folder's `sandbox`,
//! the app's binary as its helper), made of the root filesystem that
//! `MCBENCH_SANDBOX_ROOTFS` and `MCBENCH_SANDBOX_ROOTFS_SHA256` name, as
//! setup's shell step makes it; the rest of the tools need the engines and
//! are not used. Ignored without those two variables.

#![cfg(target_os = "linux")]

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use serde_json::{Map, Value, json};
use tokio_util::sync::CancellationToken;

use mcbench_headless::shell::PlatformSandbox;
use mcbench_lite::core::backend::Backend;
use mcbench_lite::core::{Core, Parts};
use mcbench_providers::{
    AssistantMessage, CompletionRequest, Delta, ModelFacts, ModelInfo, Outcome, Provider,
    ProviderError, ProviderId, Telemetry, ToolCall,
};
use mcbench_sandbox::{BashCommand, Sandbox};
use mcbench_setup::shell::{Pinned, ShellFiles};
use mcbench_store::Message;
use mcbench_tools::{Chat, Progress, Tool, ToolError, ToolOutcome, Tools};
use mcbench_ui::{Command as UiCommand, Outgoing, Target, TurnEvent, Update, Window};

const WAIT: Duration = Duration::from_secs(120);
/// Names the scratch folder of the child that runs core A.
const CHILD: &str = "MCBENCH_SECOND_CORE_DIR";
/// A's command: its box's hostname kept in `/work`, a marker that the call
/// runs, then a command that outlives A.
const LONG: &str = "hostname > /work/a-host && touch /work/started && sleep 100";
/// B's command: its box's hostname, then A's.
const SHORT: &str = "hostname && cat /work/a-host";

fn answer(content: &str, tool_calls: Vec<ToolCall>) -> AssistantMessage {
    AssistantMessage {
        content: content.into(),
        reasoning: None,
        reasoning_details: None,
        finish_reason: None,
        telemetry: Telemetry {
            provider: ProviderId::OpenRouter,
            model: String::new(),
            effort: None,
            started_at: String::new(),
            wall_ms: 0,
            first_delta_ms: None,
            context_bytes: 0,
            message_count: 0,
            usage: None,
            outcome: Outcome::Completed {
                context_tokens: None,
            },
        },
        notices: None,
        plan: None,
        rate_limit: None,
        tool_calls,
    }
}

/// A model that runs the prompt's command (`long` or `short`) with `bash`,
/// then says it is done.
struct Model;

#[async_trait]
impl Provider for Model {
    fn id(&self) -> ProviderId {
        ProviderId::OpenRouter
    }
    async fn complete(
        &self,
        request: &CompletionRequest,
        _: &mut (dyn FnMut(Delta) + Send),
        _: &CancellationToken,
    ) -> Result<AssistantMessage, ProviderError> {
        if request.tools.is_empty() {
            return Ok(answer("A title", Vec::new()));
        }
        let last = &request.messages.last().expect("a message").0;
        if last["role"] == "tool" {
            return Ok(answer("Done.", Vec::new()));
        }
        let command = if Value::Object(last.clone())
            .to_string()
            .contains("Run the long one")
        {
            LONG
        } else {
            SHORT
        };
        Ok(answer(
            "",
            vec![ToolCall {
                id: "call-1".into(),
                kind: "function".into(),
                name: "bash".into(),
                arguments: json!({ "command": command, "timeout_ms": 120_000 }).to_string(),
                extra_content: None,
            }],
        ))
    }
    async fn models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        Ok(Vec::new())
    }
    async fn model_info(&self, _: &str) -> Result<ModelFacts, ProviderError> {
        Ok(ModelFacts::unknown())
    }
    async fn close_session(&self, _: &str) {}
}

/// `bash`'s sandbox call (`mcbench_tools::bash::Bash::run`) over the app's
/// sandbox.
struct BoxBash(Arc<PlatformSandbox>);

#[async_trait]
impl Tool for BoxBash {
    fn name(&self) -> &'static str {
        "bash"
    }
    async fn call(
        &self,
        chat: &mut Chat,
        arguments: &Map<String, Value>,
        _: &mut (dyn FnMut(Progress) + Send),
        cancel: &CancellationToken,
    ) -> Result<ToolOutcome, ToolError> {
        let request = BashCommand {
            command: arguments["command"].as_str().expect("a command").into(),
            timeout_ms: arguments["timeout_ms"].as_f64(),
            network: false,
        };
        Ok(ToolOutcome::result(
            match self.0.exec(&chat.workspace, &request, cancel).await {
                Ok(run) => json!({"ok": true, "text": run.formatted}),
                Err(error) => json!({"ok": false, "error": error.to_string()}),
            },
        ))
    }
}

/// The model, the `bash` tool and the app's sandbox, made once the core's
/// store is open (the sandbox's budget is the store's).
#[derive(Clone, Default)]
struct Fake(Arc<OnceLock<Arc<PlatformSandbox>>>);

impl Fake {
    fn sandbox(&self) -> &Arc<PlatformSandbox> {
        self.0.get().expect("the sandbox is made")
    }
}

#[async_trait]
impl Backend for Fake {
    fn provider(&self, _: &str) -> Result<Arc<dyn Provider>, String> {
        Ok(Arc::new(Model))
    }
    fn tools(&self) -> Tools {
        Tools::of(vec![Arc::new(BoxBash(Arc::clone(self.sandbox())))])
    }
    fn sync_attachments(&self, _: &str) -> Result<(), String> {
        Ok(())
    }
    async fn reset_sandbox(&self, chat: &str) -> Result<(), String> {
        self.sandbox().reset(chat).await;
        Ok(())
    }
    fn network_access(&self) -> bool {
        false
    }
    async fn shutdown(&self) {
        self.sandbox().shutdown().await
    }
}

#[derive(Default)]
struct Recording(Mutex<Vec<Update>>);

impl Window for Recording {
    fn publish(&self, update: Update) {
        self.0.lock().unwrap().push(update);
    }
}

fn until(what: &str, done: impl Fn() -> bool) {
    let started = Instant::now();
    while !done() {
        assert!(started.elapsed() < WAIT, "{what}: not within {WAIT:?}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Process `pid` has exited (a zombie, or reaped).
fn exited(pid: u32) -> bool {
    match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        Ok(stat) => stat.rsplit_once(") ").unwrap().1.starts_with('Z'),
        Err(_) => true,
    }
}

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("set {name}"))
}

/// A core on `data` with the app's sandbox made and prepared.
fn core(data: &Path) -> (Core, Arc<Recording>) {
    let fake = Fake::default();
    let core = Core::start(Parts {
        data: data.to_path_buf(),
        program: std::env::current_exe().unwrap(),
        host: Arc::new(mcbench_store::SystemHost),
        shipped: Box::new(|| Err("unused".into())),
        client: mcbench_setup::fetch::client(),
        backend: Some(Arc::new(fake.clone())),
        installer: None,
        share: None,
        live: Default::default(),
    })
    .unwrap();
    let sandbox = Arc::new(PlatformSandbox::new(
        data.join("sandbox"),
        Path::new(env!("CARGO_BIN_EXE_mcbench-lite")),
        Arc::new(mcbench_tools::budget::StoreBudget(Arc::clone(
            core.store().storage(),
        ))),
    ));
    let files = ShellFiles {
        rootfs: Pinned {
            file: PathBuf::from(env("MCBENCH_SANDBOX_ROOTFS")),
            sha256: env("MCBENCH_SANDBOX_ROOTFS_SHA256"),
        },
    };
    // Made and prepared on the core's runtime, as setup's shell step does.
    let handle = core.handle().clone();
    let prepared = Arc::clone(&sandbox);
    handle
        .block_on(handle.spawn(async move {
            prepared.make(Ok(files)).unwrap();
            prepared.prepare().await
        }))
        .unwrap()
        .unwrap();
    assert!(fake.0.set(sandbox).is_ok());
    let window = Arc::new(Recording::default());
    mcbench_ui::Core::attach(&core, window.clone());
    (core, window)
}

fn send(core: &Core, chat: &str, text: &str) {
    mcbench_ui::Core::send(
        core,
        UiCommand::Send(Outgoing {
            target: Target::Chat {
                id: chat.into(),
                provider: "openrouter".into(),
                model: "own/model".into(),
            },
            text: text.into(),
            attachments: Vec::new(),
            selections: Vec::new(),
            prompts: Vec::new(),
            effort: None,
            resolution: mcbench_ui::Resolution::Standard,
            edit_of: None,
        }),
    );
}

/// The pids of `pid`'s children, from every thread's.
fn children(pid: u32) -> Vec<u32> {
    let Ok(tasks) = std::fs::read_dir(format!("/proc/{pid}/task")) else {
        return Vec::new();
    };
    tasks
        .flatten()
        .filter_map(|task| std::fs::read_to_string(task.path().join("children")).ok())
        .flat_map(|text| {
            text.split_whitespace()
                .map(|p| p.parse().unwrap())
                .collect::<Vec<u32>>()
        })
        .collect()
}

/// The systemd scope of `pid`'s box: the cgroup folder of a child of `pid`
/// that is a keeper (`<app> sandbox-agent keep`, in its scope's `keeper`).
fn box_scope(pid: u32) -> Option<PathBuf> {
    children(pid).into_iter().find_map(|child| {
        let cmdline = std::fs::read(format!("/proc/{child}/cmdline")).ok()?;
        let args: Vec<&[u8]> = cmdline
            .split(|b| *b == 0)
            .filter(|a| !a.is_empty())
            .collect();
        if !args.ends_with(&[b"sandbox-agent".as_slice(), b"keep".as_slice()]) {
            return None;
        }
        let cgroup = std::fs::read_to_string(format!("/proc/{child}/cgroup")).ok()?;
        let path = cgroup.trim().strip_prefix("0::")?.strip_suffix("/keeper")?;
        Some(PathBuf::from(format!("/sys/fs/cgroup{path}")))
    })
}

/// The pids in a cgroup folder.
fn procs(cgroup: &Path) -> Vec<u32> {
    std::fs::read_to_string(cgroup.join("cgroup.procs"))
        .unwrap_or_default()
        .split_whitespace()
        .map(|p| p.parse().unwrap())
        .collect()
}

/// A scope with no process left in it, or gone.
fn emptied(scope: &Path) -> bool {
    match std::fs::read_to_string(scope.join("cgroup.events")) {
        Ok(events) => events.lines().any(|line| line == "populated 0"),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => true,
        Err(e) => panic!("{}: {e}", scope.display()),
    }
}

/// Core A, killed by the test while its `bash` call runs.
struct Killed(Child);

impl Drop for Killed {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// The scratch folder's read-only folders (the unpacked root filesystem's)
/// made writable, then the folder removed.
fn remove(dir: &Path) {
    use std::os::unix::fs::PermissionsExt;
    fn writable(dir: &Path) {
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let kind = entry.file_type().unwrap();
            if kind.is_dir() {
                std::fs::set_permissions(entry.path(), std::fs::Permissions::from_mode(0o755))
                    .unwrap();
                writable(&entry.path());
            }
        }
    }
    writable(dir);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
#[ignore = "needs MCBENCH_SANDBOX_ROOTFS and MCBENCH_SANDBOX_ROOTFS_SHA256 (the box's root filesystem)"]
fn a_core_started_at_once_after_one_killed_in_a_bash_call_takes_the_folder_and_runs_a_fresh_box() {
    let dir = std::env::temp_dir().join(format!("mcbench-second-core-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let data = dir.join("data");
    std::fs::create_dir_all(&data).unwrap();

    // A: its turn's bash call runs in the box.
    let a = Killed(
        Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "core_a", "--ignored", "--nocapture"])
            .env(CHILD, &dir)
            .stdin(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let a_pid = a.0.id();
    until("A names its chat", || {
        assert!(!exited(a_pid), "A ended");
        dir.join("chat").exists()
    });
    let chat = std::fs::read_to_string(dir.join("chat")).unwrap();
    let work = data.join("sandbox").join(&chat);
    until("A's bash call runs", || {
        assert!(!exited(a_pid), "A ended");
        work.join("started").exists()
    });
    let scope = box_scope(a_pid).expect("A's keeper");
    let boxed = procs(&scope.join("box"));
    assert!(!boxed.is_empty(), "A's box runs");
    assert_eq!(
        std::fs::read_to_string(data.join("server.pid")).unwrap(),
        a_pid.to_string()
    );

    // A killed, and B started at once.
    let mut a = a;
    a.0.kill().unwrap();
    a.0.wait().unwrap();
    let started = Instant::now();
    let (core, window) = core(&data);
    assert_eq!(
        std::fs::read_to_string(data.join("server.pid")).unwrap(),
        std::process::id().to_string(),
        "B holds the folder"
    );
    let handle = core.handle().clone();
    handle.block_on(core.started());
    let head = || {
        let live = core.store().conversation(&chat).unwrap().unwrap();
        let conversation = live.lock().unwrap();
        conversation.messages.last().unwrap().message.clone()
    };
    match head() {
        Message::Error(error) => {
            assert_eq!(error.content, "The server stopped during this turn")
        }
        other => panic!("the turn A left is ended: {other:?}"),
    }

    // B's turn on the same chat runs bash in a fresh box.
    send(&core, &chat, "Run the short one");
    let result = || {
        window.0.lock().unwrap().iter().find_map(|u| match u {
            Update::Turn {
                chat: of,
                event: TurnEvent::ToolResult { name, result, .. },
            } if *of == chat && name == "bash" => Some(result.clone()),
            _ => None,
        })
    };
    until("B's bash result", || result().is_some());
    until(
        "B's answer",
        || matches!(head(), Message::Assistant(said) if said.content == "Done."),
    );
    let answered = started.elapsed();
    let result = result().unwrap();
    assert_eq!(result["ok"], true, "{result}");
    let text = result["text"].as_str().unwrap();
    let a_host = std::fs::read_to_string(work.join("a-host")).unwrap();
    let b_host = text.lines().next().unwrap();
    assert!(
        text.contains(a_host.trim()) && b_host != a_host.trim(),
        "B's box is not A's: {text}"
    );
    eprintln!("B took {answered:?} from its start to its first answer");

    // A's box ended: no process is left in its scope.
    until("A's box ends", || emptied(&scope));
    eprintln!(
        "A's box was empty {:?} after A was killed",
        started.elapsed()
    );
    for pid in boxed {
        assert!(
            !Path::new(&format!("/proc/{pid}")).exists(),
            "{pid} is left"
        );
    }

    core.shutdown();
    remove(&dir);
}

/// Core A: a chat whose turn runs the long command, its id written for the
/// test; it runs until it is killed (or the test's end of stdin closes).
#[test]
#[ignore = "run by a_core_started_at_once_after_one_killed_in_a_bash_call_takes_the_folder_and_runs_a_fresh_box"]
fn core_a() {
    let dir = PathBuf::from(std::env::var_os(CHILD).expect("run by the test"));
    let (core, _window) = core(&dir.join("data"));
    let handle = core.handle().clone();
    handle.block_on(core.started());
    let made = core.clone();
    let chat = handle
        .block_on(handle.spawn(async move {
            made.create_conversation("openrouter", "own/model")
                .await
                .unwrap()
                .id
        }))
        .unwrap();
    send(&core, &chat, "Run the long one");
    let named = dir.join("chat.partial");
    std::fs::write(&named, &chat).unwrap();
    std::fs::rename(&named, dir.join("chat")).unwrap();
    let mut rest = Vec::new();
    std::io::stdin().read_to_end(&mut rest).unwrap();
    panic!("the test ended without killing core A");
}
