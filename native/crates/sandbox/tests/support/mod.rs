//! The real box for the integration tests: `MCBENCH_SANDBOX_ROOTFS` (the
//! rootfs tar.zst) and `MCBENCH_SANDBOX_ROOTFS_SHA256`; with
//! `MCBENCH_SANDBOX_VIA_AGENT=1` every call goes through
//! [`mcbench_sandbox::agent::GuestClient`] to `mcbench-sandbox-agent serve`
//! over a pipe. A test run without the variables fails here.

#![allow(dead_code)]

pub mod diff;

use std::path::PathBuf;
use std::sync::Arc;

use mcbench_sandbox::agent::GuestClient;
use mcbench_sandbox::lifecycle::Sandboxes;
use mcbench_sandbox::linux::Bubblewrap;
use mcbench_sandbox::testing::TestBudget;
use mcbench_sandbox::{BashCommand, BashRun, Sandbox, SandboxError, Workspace};
use tokio_util::sync::CancellationToken;

pub const AGENT: &str = env!("CARGO_BIN_EXE_mcbench-sandbox-agent");

pub fn rootfs() -> (PathBuf, String) {
    let tarball = std::env::var_os("MCBENCH_SANDBOX_ROOTFS")
        .expect("MCBENCH_SANDBOX_ROOTFS=<the rootfs tar.zst> is needed (rootfs/build.sh makes it)");
    let sha = std::env::var("MCBENCH_SANDBOX_ROOTFS_SHA256")
        .expect("MCBENCH_SANDBOX_ROOTFS_SHA256=<its sha256> is needed");
    (PathBuf::from(tarball), sha)
}

/// A scratch data folder, removed when dropped.
pub struct Scratch(pub PathBuf);

impl Scratch {
    pub fn new(label: &str) -> Scratch {
        use std::sync::atomic::{AtomicU32, Ordering};
        static N: AtomicU32 = AtomicU32::new(0);
        let base = std::env::var_os("MCBENCH_SANDBOX_SCRATCH")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        let path = base.join(format!(
            "mcbench-box-{label}-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(path.join("sandbox")).unwrap();
        Scratch(path)
    }
    pub fn sandbox_dir(&self) -> PathBuf {
        self.0.join("sandbox")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The unpacked rootfs is shared by every test of a run (unpacking takes
/// seconds): one folder beside the scratch folders.
pub fn shared_root() -> PathBuf {
    let base = std::env::var_os("MCBENCH_SANDBOX_SCRATCH")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let root = base.join("mcbench-box-rootfs");
    std::fs::create_dir_all(&root).unwrap();
    root
}

// On Windows the Linux types hold no state, which leaves the agent variant the large one.
#[allow(clippy::large_enum_variant)]
pub enum Any {
    Direct(Sandboxes<Bubblewrap>),
    Agent(Sandboxes<GuestClient>, tokio::process::Child),
    #[cfg(target_os = "macos")]
    Vm(mcbench_sandbox::macos::VirtualMachine),
    #[cfg(windows)]
    Wsl(Box<mcbench_sandbox::windows::WslDistro>),
}

impl Any {
    pub fn sandbox(&self) -> &dyn Sandbox {
        match self {
            Any::Direct(s) => s,
            Any::Agent(s, _) => s,
            #[cfg(target_os = "macos")]
            Any::Vm(vm) => vm,
            #[cfg(windows)]
            Any::Wsl(distro) => &**distro,
        }
    }
    pub async fn watch_storage(&self) {
        match self {
            Any::Direct(s) => s.watch_storage().await.unwrap(),
            Any::Agent(s, _) => s.watch_storage().await.unwrap(),
            #[cfg(target_os = "macos")]
            Any::Vm(vm) => vm.guest.watch_storage().await.unwrap(),
            #[cfg(windows)]
            Any::Wsl(distro) => distro.guest.watch_storage().await.unwrap(),
        }
    }
}

pub fn via_agent() -> bool {
    std::env::var("MCBENCH_SANDBOX_VIA_AGENT").as_deref() == Ok("1")
}

/// The guest base the macOS VM boots and the WSL2 distro imports: `MCBENCH_SANDBOX_GUEST` (the
/// `guest/build.sh` asset) and `MCBENCH_SANDBOX_GUEST_SHA256`.
#[cfg(any(target_os = "macos", windows))]
pub fn guest_base() -> mcbench_sandbox::GuestBase {
    mcbench_sandbox::GuestBase {
        asset: std::env::var_os("MCBENCH_SANDBOX_GUEST")
            .expect("MCBENCH_SANDBOX_GUEST=<the guest base tar.zst> is needed (guest/build.sh makes it)")
            .into(),
        sha256: std::env::var("MCBENCH_SANDBOX_GUEST_SHA256")
            .expect("MCBENCH_SANDBOX_GUEST_SHA256=<its sha256> is needed"),
    }
}

/// The macOS VM's sandbox folder: the scratch base, which holds every test's
/// scratch folder (the chats' `/work` the VM sees through its share) and the
/// VM's own files (`.vm`, `.guest-<sha>`).
#[cfg(any(target_os = "macos", windows))]
pub fn vm_root() -> PathBuf {
    let base = std::env::var_os("MCBENCH_SANDBOX_SCRATCH")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    std::fs::create_dir_all(&base).unwrap();
    base
}

/// The sandbox under test, its boxes' files under the shared root, its
/// chats' `/work` under `scratch`. On macOS: the VM.
pub async fn sandbox(budget: Arc<TestBudget>) -> Any {
    let (tarball, sha) = rootfs();
    #[cfg(target_os = "macos")]
    {
        let vm = mcbench_sandbox::macos::VirtualMachine::start(mcbench_sandbox::Config {
            root: vm_root(),
            rootfs: (tarball, sha),
            helper: Vec::new(),
            budget,
            guest: guest_base(),
        });
        vm.prepare().await.unwrap();
        return Any::Vm(vm);
    }
    // On Windows: the distro (MCBENCH_WSL_DISTRO names the test's).
    #[cfg(windows)]
    {
        let distro = mcbench_sandbox::windows::WslDistro::start(mcbench_sandbox::Config {
            root: vm_root(),
            rootfs: (tarball, sha),
            helper: Vec::new(),
            budget,
            guest: guest_base(),
        });
        distro.prepare().await.unwrap();
        return Any::Wsl(Box::new(distro));
    }
    #[allow(unreachable_code)]
    let root = shared_root();
    if via_agent() {
        let mut child = tokio::process::Command::new(AGENT)
            .arg("serve")
            .arg(&root)
            .arg(&tarball)
            .arg(&sha)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let client = GuestClient::new(
            stdout,
            stdin,
            Arc::new(|ws: &Workspace| Ok(ws.host.clone())),
        );
        let sandboxes = Sandboxes::new(client, budget);
        sandboxes.prepare().await.unwrap();
        Any::Agent(sandboxes, child)
    } else {
        let backend = Bubblewrap::new(
            root,
            (tarball, sha),
            vec![AGENT.into()],
            mcbench_sandbox::linux::Delegation::UserScope,
        );
        let sandboxes = Sandboxes::new(backend, budget);
        sandboxes.prepare().await.unwrap();
        Any::Direct(sandboxes)
    }
}

pub fn ws(scratch: &Scratch, chat: &str) -> Workspace {
    let ws = Workspace {
        chat: chat.to_string(),
        host: scratch.sandbox_dir().join(chat),
    };
    mcbench_sandbox::work::work_dir(&ws).unwrap();
    ws
}

/// The boxes' user: this process's on Linux, the VM's agent's on macOS.
pub fn box_uid() -> u32 {
    #[cfg(target_os = "macos")]
    return mcbench_sandbox::macos::GUEST_USER.0;
    #[cfg(windows)]
    return mcbench_sandbox::windows::GUEST_USER.0;
    // SAFETY: getuid cannot fail.
    #[cfg(unix)]
    #[allow(unreachable_code)]
    unsafe {
        libc::getuid()
    }
}

pub fn cmd(command: &str, ms: f64, network: bool) -> BashCommand {
    BashCommand {
        command: command.to_string(),
        timeout_ms: Some(ms),
        network,
    }
}

pub async fn run(
    sandbox: &dyn Sandbox,
    ws: &Workspace,
    command: &str,
    ms: f64,
    network: bool,
) -> Result<BashRun, SandboxError> {
    sandbox
        .exec(ws, &cmd(command, ms, network), &CancellationToken::new())
        .await
}

/// The transcript's output, after `Output:\n`.
pub fn output(run: &BashRun) -> &str {
    run.formatted
        .split_once("\nOutput:\n")
        .map(|(_, out)| out)
        .unwrap_or("")
}

pub fn budget(scratch: &Scratch, chat_max: u64) -> Arc<TestBudget> {
    Arc::new(TestBudget::new(scratch.sandbox_dir(), chat_max))
}
