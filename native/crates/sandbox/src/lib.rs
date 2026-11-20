//! # mcbench-sandbox
//!
//! The `bash` tool's sandbox: one pinned Linux root filesystem under
//! bubblewrap on every platform (on the host kernel on Linux, in the app's own
//! Virtualization.framework VM on macOS, in a private WSL2 distro on
//! Windows), the networked command's user-space network (pasta) with today's
//! address rules, and the allow-list CONNECT proxy for Codex on Linux. It
//! replaces `server/sandbox.js` (Docker) and `egress-proxy.js`;
//! `container-shim.py` goes with Docker.
//!
//! The crate has three layers.
//!
//! | Layer | What | Where |
//! |---|---|---|
//! | [`Sandbox`] | what the tools call: `exec`, `view_image`, `remove_work_files`, `reset`, `shutdown` | [`lifecycle::Sandboxes`], on every platform, on the host |
//! | [`Backend`] | the boxes: a chat's box made, a command run in its persistent shell or a one-shot shell, its processes ended, the box removed | [`linux::Bubblewrap`] on the Linux kernel; [`agent::GuestClient`] carries the same calls over a byte stream to [`agent`]'s program, which runs [`linux::Bubblewrap`] inside the macOS VM or the WSL2 distro |
//! | the keeper | one small single-threaded process per box, in the box's systemd user scope, that starts the box and enters every shell into it | [`linux`] (`sandbox-agent keep`) |
//!
//! What a model reads from `bash` is today's text, byte for byte. Because the
//! halt and storage notes are the sandbox's own state (`sandbox.js`
//! `takeHalt`, `restricted`), this crate formats the transcript itself
//! ([`transcript::format_result`], `sandbox.js` `formatResult`) and returns
//! what `sandbox.js` `exec` returns ([`BashRun`]); the agent's `bash` tool
//! only adds `executeBash`'s envelope.
//!
//! ## Owner
//!
//! The stage-three session.
//!
//! ## Seams
//!
//! | Seam | What crosses | Who calls whom | Thread or process |
//! |---|---|---|---|
//! | tools | [`BashCommand`] in, [`BashRun`] or [`SandboxError`] out; the chat's `/work` folder ([`Workspace`]); [`work`]'s host-side file bridge | `mcbench-tools`' `bash`, `view_image`, `build` and `test_build` call this crate | the async runtime; commands run in sandbox child processes |
//! | storage | [`Budget`]: a chat's refusal, a measurement, the disk's free bytes | this crate calls `mcbench-store`'s `Storage` through an adapter | the async runtime (the store's calls through `spawn_blocking`) |
//! | the root filesystem | a tar.zst release asset pinned by sha256, made by `rootfs/build.sh` from `sandbox/Dockerfile` | `mcbench-setup` fetches it; [`Sandbox::prepare`] unpacks and checks it ([`linux::rootfs`]) | a setup task |
//! | the guests (macOS, Windows) | [`agent`]'s frames (`GUEST.md`) over any byte stream; chat folders shared by virtiofs (macOS) or drvfs (Windows) | [`macos::VirtualMachine`] and [`windows::WslDistro`] are a [`lifecycle::Sandboxes`] over an [`agent::GuestClient`] | the guest program `mcbench-sandbox-agent` (static musl) |
//! | Codex's network on Linux | [`proxy::codex_command`]: [`proxy::CodexRun`] in (the fields of `mcbench-providers`' `codex_cli::SandboxedCodex`, and the rootfs, its environment and the static agent that relays), [`proxy::CodexCommand`] out (the command, the session's environment, the proxy's stop) | `codex_cli::sandbox_codex_command` calls it through an adapter the session connects | the proxy on a thread of its own with a current-thread runtime |
//!
//! ## Behaviour defined by
//!
//! Class 7's sandbox files (docs/native-app.md, Appendix A), 28 tests in 11
//! files, ported in `tests/` (each test names its Node test):
//! `sandbox-docker-missing.test.js` 3 (Docker missing becomes bubblewrap,
//! user namespaces or the user session missing), `sandbox-errors.test.js` 3,
//! `sandbox-lifecycle.test.js` 6, `sandbox-network.test.js` 2,
//! `sandbox-state.test.js` 1, `sandbox-storage.integration.test.js` 1,
//! `sandbox-storage.test.js` 4, `sandbox-symlink.test.js` 4,
//! `sandbox-workspace.integration.test.js` 1, `sandbox.integration.test.js` 2
//! and `sandbox.test.js` 1; the egress proxy's test in
//! the benchmark's batch tests and the Codex container's isolation
//! in `batch-isolation.integration.test.js`. Beside them, the refused-range
//! matrix and the rules-cannot-be-undone attempts of the netspike, and the
//! differential test against today's Docker sandbox (`tests/docker_diff.rs`).
//!
//! Tests that need a real box say so by environment variable and fail, not
//! skip, when it is set and something is missing:
//! `MCBENCH_SANDBOX_ROOTFS=<rootfs tar.zst>` with
//! `MCBENCH_SANDBOX_ROOTFS_SHA256=<its sha256>` (bubblewrap, user namespaces,
//! a user systemd session and the rootfs: the integration tests),
//! `MCBENCH_SANDBOX_NETWORK=1` (the refused-range matrix and the undo
//! attempts, beside the rootfs), `MCBENCH_SANDBOX_DOCKER_DIFF=<checkout>`
//! (the differential test: Docker with today's image and Node), and
//! `MCBENCH_SANDBOX_NODE=<checkout>` (`contract/generate.js --check` and the
//! transcript against Node's `formatResult`). `MCBENCH_SANDBOX_VIA_AGENT=1`
//! runs the integration tests a second time through [`agent::GuestClient`]
//! talking to the agent program over a pipe. On macOS the integration
//! tests (`linux_box`, `network`, `native_diff`, `macos_vm`) run against
//! the VM and need `MCBENCH_SANDBOX_GUEST=<guest base tar.zst>` with
//! `MCBENCH_SANDBOX_GUEST_SHA256` beside the aarch64 rootfs (the tests read
//! them and give them to [`Config`]'s `guest`), and the test binary signed
//! with the virtualization entitlement; from `native/`:
//!
//! ```text
//! CARGO_TARGET_AARCH64_APPLE_DARWIN_RUNNER=$PWD/crates/sandbox/macos/run-signed.sh \
//!   cargo test -p mcbench-sandbox --test macos_vm --test linux_box -- --ignored --test-threads 1
//! ```
//!
//! `MCBENCH_SANDBOX_SCRATCH` is then the folder that holds the tests'
//! scratch folders and the VM's own files (`.vm`, `.guest-<sha>`), kept
//! between runs so the root filesystem is unpacked once. On Windows the
//! same tests and `windows_wsl` run against the WSL2 distro, with WSL
//! installed, the x86_64 guest base and rootfs variables, and
//! `MCBENCH_WSL_DISTRO=<a name>` (the test's own distro, imported on first
//! use into `MCBENCH_SANDBOX_SCRATCH`'s `.wsl`).
//!
//! ## Open
//!
//! The macOS network device's stronger form, the app's own stack on
//! a file-handle attachment, stays an open risk after stage three (the
//! netspike's decision).
//!
//! The pinned x86_64 root filesystem has no `pasta.avx2` until it is rebuilt
//! with `rootfs/build.sh`, so pasta's "Can't run AVX2 build" line stays in a
//! networked box's failure until then.
//!
//! ## Decided
//!
//! The networked box's egress rules run in netinit's own namespaces
//! (`linux::keeper::run_rules`), not under `/usr/bin/bwrap`: an AppArmor
//! profile for bwrap (Ubuntu 26.04's `bwrap-userns-restrict`) takes
//! `CAP_NET_ADMIN` from what bubblewrap runs, and netinit, root of pasta's
//! user namespace, is not confined by it. A box ends when its owner goes by
//! its keeper's control socket reaching the end, not by bubblewrap's
//! `--die-with-parent`, whose signal that profile clears.
//!
//! [`Config`] names the guest base ([`GuestBase`], a field only macOS and
//! Windows builds have), because setup owns where the sandbox's files come
//! from (`mcbench_setup::shell::files`); the crate reads no variable for it.
//!
//! A box's open-file limit and core dumps are the box's own on every host,
//! set by the keeper beside the cgroup limits (`linux::keeper::RLIMITS`):
//! `ulimit -n` is 65536 (soft and hard) and a crash prints no "(core
//! dumped)", where they followed the machine before (65536 on adipc's
//! systemd, 1024 in the macOS VM, 10240 in WSL; adipc's systemd-coredump
//! made bash add "(core dumped)").
//!
//! [`Sandbox`]'s calls are `async fn` on the core's tokio runtime
//! (`native/README.md`, Decisions of stage three), cancelled by a
//! [`CancellationToken`].
//!
//! The user-space network on Linux is pasta (passt 2026_10_02.cba3570,
//! GPL-2.0-or-later, built static from the pinned source tarball and shipped
//! inside the root filesystem's tarball as a separate program whose source
//! each release offers), with today's `EGRESS_RULES` text run by the root
//! filesystem's own iptables in the box's network namespace (the netspike,
//! `$S/netspike/REPORT.md`): it reproduces today's reach and today's two
//! refusals (curl 7, UDP `EPERM`).
//!
//! Limits are one mechanism on every platform, in the keeper: in a cgroup
//! v2 folder delegated to it, the keeper makes a `box` child cgroup with
//! today's `--memory 512m --memory-swap 512m --pids-limit 256 --cpus 2`
//! (`memory.max`, `memory.swap.max`, `pids.max`, `cpu.max`), so only the
//! box's `sleep infinity` and the model's processes count, as in today's
//! container. Only who delegates that folder differs
//! ([`linux::Delegation`]): on a Linux host a systemd user scope with
//! `Delegate=yes` per box; in the macOS VM (and in the WSL2 distro) the
//! guest's init, which gives the agent's user a subtree with the `memory`,
//! `pids` and `cpu` controllers, in which the agent makes each keeper's
//! folder. A guest has no user systemd session, and a second mechanism
//! (rlimits) cannot give a box's memory and CPU limits; the Linux host's
//! path is unchanged (its tests and the 106 cases were rerun on adipc). A
//! process can only be moved into a cgroup by a writer that may write the
//! common ancestor's `cgroup.procs`, which the app (in a login session's
//! scope) may not, so the shells are started by the keeper, which lives in
//! its folder.
//!
//! On macOS the boxes run in the app's own VM ([`macos`]): Debian 13's
//! cloud kernel and a guest base of pinned Debian packages (`guest/`,
//! busybox, bubblewrap and its libraries, the static agent, an init),
//! booted by the Virtualization framework from Rust (objc2-virtualization),
//! the sandbox folder and the root filesystem's folder shared by virtiofs,
//! a sparse disk for the unpacked root filesystem, the NAT attachment, and
//! the agent reached over vsock. The same guest base's userland is what
//! the WSL2 distro imports (x86_64), built by the same recipe. The VM's
//! clock follows the host's through a virtiofs file (`sandbox-agent
//! clock`), because files a box writes get the host's times and the VM
//! has no clock of its own. The agent's user is 1000:1000: Apple's virtiofs
//! shows every file as owned by whoever looks, so the host's ids need not
//! reach the guest.
//!
//! On Windows the boxes run in the app's own WSL2 distro ([`windows`]):
//! imported once from the x86_64 guest base's `userland.tar` with
//! automount, `fstab` and interop off and a user of its own, started by
//! `guest/wsl-init` as root (drvfs mounts of the sandbox folder and the
//! root filesystem's folder, a delegated cgroup v2 subtree, a tun node of
//! the distro's own), and the agent (`guest -`) serving on the stdin and
//! stdout of one `wsl.exe`; [`windows::wsl_state`] tells the setup screen
//! whether an administrator must install WSL first.
//!
//! Shells are entered by the keeper's own code (`setns` into the box's
//! namespaces, `chroot` to its root, the bounding, ambient, inheritable,
//! permitted and effective capability sets cleared, `no_new_privs`, a fork
//! for the pid namespace, then `execve`), not by `nsenter` and `setpriv`:
//! the host's `nsenter` is not on every machine and the rootfs's binaries
//! are not built to run on the host, `setpriv` cannot clear the bounding set
//! after the exec has dropped `CAP_SETPCAP`, and the cgroup move, the
//! `SIGPIPE` and signal-mask reset and the exit status relay need the
//! keeper anyway. Nothing needs root or a setuid bit beyond bubblewrap.
//!
//! A command's stdout and stderr share one pipe, so the transcript has them
//! in the order the shell wrote them. Today `docker exec` gives them two
//! pipes read as they arrive, and the sentinel on stdout overtakes stderr
//! written just before it: today's transcript reorders stderr or carries it
//! into the next command's (the differential test shows it in 5 of 106
//! cases). This changes what a model reads, and stands as the owner's
//! decision to take (`tests/docker_diff.rs` lists it).
//!
//! Boxes do not outlive the app: a keeper whose control socket closes kills
//! its box. Today's containers survive a server crash and are adopted by the
//! next start (`adoptContainers`); a clean stop removes them in both.
//!
//! The root filesystem's licences: Debian 13's packages (their own, many
//! GPL; the image's `/usr/share/doc/*/copyright`), Python 3.12 (PSF),
//! NumPy and Pillow (BSD-style and HPND) as today's image ships them, and
//! pasta (GPL-2.0-or-later). The release offers the sources beside the
//! tarball.

use async_trait::async_trait;
use std::path::PathBuf;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

pub mod agent;
pub mod backend;
pub mod lifecycle;
pub mod linux;
pub mod macos;
pub mod proxy;
#[doc(hidden)]
pub mod testing;
pub mod transcript;
pub mod windows;
pub mod work;
pub mod wsl_codex;

pub use backend::Backend;

/// A command's default limit (`sandbox.js` `DEFAULT_TIMEOUT_MS`).
pub const DEFAULT_TIMEOUT: Duration = Duration::from_millis(10_000);
/// The longest limit a model may ask for (`sandbox.js` `MAX_TIMEOUT_MS`).
pub const MAX_TIMEOUT: Duration = Duration::from_millis(120_000);

/// A chat's `/work` folder on the host, the one folder its commands see.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Workspace {
    /// The chat's id.
    pub chat: String,
    /// The folder on the host (`sandbox.js` `workDir`: the data folder's
    /// `sandbox/<chat>`).
    pub host: PathBuf,
}

/// One `bash` call: the command as the model wrote it, the limit as the
/// model gave it (`timeout_ms`; clamped by [`BashCommand::clamped`] as
/// `exec` does) and whether it is the networked command (`network: true`).
#[derive(Clone, Debug)]
pub struct BashCommand {
    /// The shell text.
    pub command: String,
    /// `timeout_ms` as the model gave it, or `None`.
    pub timeout_ms: Option<f64>,
    /// The networked one-shot command, with today's reach.
    pub network: bool,
}

impl BashCommand {
    /// `Math.min(Math.max(Number(timeoutMs) || DEFAULT_TIMEOUT_MS, 1000),
    /// MAX_TIMEOUT_MS)`, in milliseconds. It stays a JavaScript number: a
    /// fractional limit is written as such in the timeout line and in
    /// `timeout`'s argument, as today.
    pub fn clamped(&self) -> f64 {
        let given = match self.timeout_ms {
            Some(ms) if ms != 0.0 && !ms.is_nan() => ms,
            _ => DEFAULT_TIMEOUT.as_millis() as f64,
        };
        given.clamp(1000.0, MAX_TIMEOUT.as_millis() as f64)
    }
}

/// What `sandbox.js` `exec` answers: the exit code, the transcript the model
/// reads (`formatResult`), and, when the chat was over its storage limits,
/// the storage message (`executeBash` then answers a failure with the
/// transcript as its output).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BashRun {
    /// The exit code (124 when the limit was reached; -1 when the output
    /// limit killed it).
    pub exit_code: i32,
    /// The transcript.
    pub formatted: String,
    /// The storage message, when the command ran with file writes disabled.
    pub storage: Option<String>,
}

/// Why a command could not run. The message is the one the model reads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SandboxError {
    /// The sandbox is not available on this machine (bubblewrap, user
    /// namespaces or the user systemd session missing, the VM unusable, WSL
    /// not installed), with what to do about it.
    Unavailable(String),
    /// The call was cancelled.
    Cancelled,
    /// Any other failure, with its message.
    Failed(String),
}

impl std::fmt::Display for SandboxError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SandboxError::Unavailable(message) | SandboxError::Failed(message) => {
                f.write_str(message)
            }
            // An AbortController's reason (`signal.reason`), as Node words it.
            SandboxError::Cancelled => f.write_str("This operation was aborted"),
        }
    }
}

impl std::error::Error for SandboxError {}

/// The sandbox of one platform.
#[async_trait]
pub trait Sandbox: Send + Sync {
    /// Makes the sandbox ready: the root filesystem unpacked and checked,
    /// what the platform needs found (or [`SandboxError::Unavailable`]), the
    /// VM or the distro started. Called by setup; `exec` before it fails.
    async fn prepare(&self) -> Result<(), SandboxError>;
    /// Runs one command in the chat's persistent shell (or the networked
    /// one-shot shell, or the write-disabled one when the chat is over its
    /// limits) and waits for it (`sandbox.js` `exec`).
    async fn exec(
        &self,
        workspace: &Workspace,
        command: &BashCommand,
        cancel: &CancellationToken,
    ) -> Result<BashRun, SandboxError>;
    /// A PNG of an image in `/work`, normalised and downscaled by Pillow in
    /// the box (`sandbox.js` `viewImage`).
    async fn view_image(&self, workspace: &Workspace, path: &str) -> Result<Vec<u8>, SandboxError>;
    /// Deletes files in `/work` from inside the box (`removeWorkFiles`).
    async fn remove_work_files(
        &self,
        workspace: &Workspace,
        paths: &[String],
    ) -> Result<(), SandboxError>;
    /// Removes the chat's boxes; its `/work` stays (`sandbox.js` `reset`).
    async fn reset(&self, chat: &str);
    /// Removes the chat's boxes and its `/work`, and forgets its storage
    /// (`sandbox.js` `destroy`).
    async fn destroy(&self, workspace: &Workspace) -> Result<(), SandboxError>;
    /// Ends every box (`sandbox.js` `shutdown`).
    async fn shutdown(&self);
    /// Whether `network: true` is offered (`networkAccess`).
    fn network_access(&self) -> bool;
    /// Benchmark workers turn the networked command off for the whole
    /// process (`disableNetwork`).
    fn disable_network(&self);
}

/// The chats' storage limits as the sandbox uses them (`storage.js`
/// `refusal`, `measure`, `freeBytes`, `DISK_RESERVE_BYTES`, `forget`);
/// `mcbench-store`'s `Storage` implements it through an adapter.
#[async_trait]
pub trait Budget: Send + Sync + 'static {
    /// Why nothing more may be written for the chat, or `None`
    /// (`storage.refusal`); an error is a failed measurement.
    async fn refusal(&self, chat: &str) -> Result<Option<String>, String>;
    /// Measures the chat again (`storage.measure`); its result postdates the
    /// call.
    async fn measure(&self, chat: &str) -> Result<(), String>;
    /// The disk's free bytes (`storage.freeBytes`).
    fn free_bytes(&self) -> Result<u64, String>;
    /// The disk's reserve (`storage.DISK_RESERVE_BYTES`).
    fn disk_reserve_bytes(&self) -> u64;
    /// Forgets the chat's measurement (`storage.forget`).
    fn forget(&self, chat: &str);
}

/// The guest base the macOS VM boots and the WSL2 distro imports: its
/// tar.zst (`guest/build.sh`) and its sha256, from setup.
#[derive(Clone, Debug)]
pub struct GuestBase {
    /// `mcbench-sandbox-guest-<arch>.tar.zst`.
    pub asset: PathBuf,
    /// Its sha256 (hex).
    pub sha256: String,
}

/// What the sandbox of this platform is made from.
pub struct Config {
    /// The data folder's `sandbox` folder: the root filesystem is unpacked
    /// under it, beside the chats' `/work` folders.
    pub root: PathBuf,
    /// The root filesystem's tar.zst and its sha256 (hex), from setup.
    pub rootfs: (PathBuf, String),
    /// The program and the arguments before `keep` that run
    /// [`agent::main`] (`mcbench-lite sandbox-agent`, or the small
    /// `mcbench-sandbox-agent`).
    pub helper: Vec<std::ffi::OsString>,
    /// The chats' storage limits.
    pub budget: std::sync::Arc<dyn Budget>,
    /// The guest base, from setup. The field exists only where a guest runs
    /// the boxes (macOS, Windows): a Linux build cannot be given one, and a
    /// macOS or Windows build cannot be made without one.
    #[cfg(not(target_os = "linux"))]
    pub guest: GuestBase,
}

/// The sandbox of the platform the app was built for: bubblewrap on the
/// Linux kernel, the VM on macOS (booted in the background:
/// `macos::VirtualMachine::start`), the WSL2 distro on Windows (imported
/// and started in the background: `windows::WslDistro::start`). On macOS
/// and Windows it is made inside a tokio runtime.
pub fn for_this_platform(config: Config) -> Box<dyn Sandbox> {
    #[cfg(target_os = "linux")]
    let sandbox: Box<dyn Sandbox> = Box::new(lifecycle::Sandboxes::new(
        linux::Bubblewrap::new(
            config.root,
            config.rootfs,
            config.helper,
            linux::Delegation::UserScope,
        ),
        config.budget,
    ));
    #[cfg(target_os = "macos")]
    let sandbox: Box<dyn Sandbox> = Box::new(macos::VirtualMachine::start(config));
    #[cfg(target_os = "windows")]
    let sandbox: Box<dyn Sandbox> = Box::new(windows::WslDistro::start(config));
    sandbox
}

#[cfg(test)]
mod seam {
    use super::*;

    struct FakeSandbox;
    #[async_trait]
    impl Sandbox for FakeSandbox {
        async fn prepare(&self) -> Result<(), SandboxError> {
            unimplemented!("a fake for the seam test")
        }
        async fn exec(
            &self,
            _: &Workspace,
            _: &BashCommand,
            _: &CancellationToken,
        ) -> Result<BashRun, SandboxError> {
            unimplemented!("a fake for the seam test")
        }
        async fn view_image(&self, _: &Workspace, _: &str) -> Result<Vec<u8>, SandboxError> {
            unimplemented!("a fake for the seam test")
        }
        async fn remove_work_files(&self, _: &Workspace, _: &[String]) -> Result<(), SandboxError> {
            unimplemented!("a fake for the seam test")
        }
        async fn reset(&self, _: &str) {
            unimplemented!("a fake for the seam test")
        }
        async fn destroy(&self, _: &Workspace) -> Result<(), SandboxError> {
            unimplemented!("a fake for the seam test")
        }
        async fn shutdown(&self) {
            unimplemented!("a fake for the seam test")
        }
        fn network_access(&self) -> bool {
            unimplemented!("a fake for the seam test")
        }
        fn disable_network(&self) {
            unimplemented!("a fake for the seam test")
        }
    }

    #[test]
    fn a_fake_can_be_written_against_the_trait() {
        let _: Box<dyn Sandbox> = Box::new(FakeSandbox);
        let _ = for_this_platform;
    }

    #[test]
    fn the_limit_is_clamped_as_exec_clamps_it() {
        let at = |ms| {
            BashCommand {
                command: String::new(),
                timeout_ms: ms,
                network: false,
            }
            .clamped()
        };
        assert_eq!(at(None), 10_000.0);
        assert_eq!(at(Some(0.0)), 10_000.0);
        assert_eq!(at(Some(f64::NAN)), 10_000.0);
        assert_eq!(at(Some(5.0)), 1000.0);
        assert_eq!(at(Some(-5.0)), 1000.0);
        assert_eq!(at(Some(2500.7)), 2500.7);
        assert_eq!(at(Some(f64::INFINITY)), 120_000.0);
    }
}
