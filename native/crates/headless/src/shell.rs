//! The model's shell of a data folder: the platform's `bash` sandbox
//! ([`PlatformSandbox`]), made of the files setup names
//! (`mcbench_setup::shell::files`) when it has them. Shared by the app's
//! core (made at its start, the files given by the shell step of setup) and
//! `headless turn` (`turn.rs`' `adapter::tools`, made for one request and
//! shut down at its end), so both start the same sandbox the same way.
//!
//! | Platform | The sandbox | Before setup has the files | Without the files |
//! |---|---|---|---|
//! | Linux | bubblewrap on the host kernel (`mcbench_sandbox::for_this_platform`) | calls wait | every call: the shell step's sentence |
//! | macOS | the app's VM, booted in the background as soon as the files are there | calls wait | every call: the VM's sentence with the shell step's as its reason |
//! | Windows | the app's WSL2 distro: WSL checked and the distro imported from the guest base by the shell step, then started in the background | calls wait | every call: the distro's sentence with the shell step's as its reason; while WSL is to be installed or Windows restarted, `WSL_MISSING` |
//!
//! On Windows the shell step needs the person while WSL is not installed or
//! waits for a restart (`mcbench_setup::Needs` on the step, the owner's
//! sentences in `mcbench_setup::wsl`); the app's core answers the two
//! commands that do it.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use async_trait::async_trait;
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

use mcbench_sandbox::{BashCommand, BashRun, Budget, Sandbox, SandboxError, Workspace};
use mcbench_setup::StepError;
use mcbench_setup::shell::ShellFiles;

use crate::providers::{RootfsSlot, ShellRootfs};

/// The platform's one arm: the sandbox made of setup's files, whether it
/// offers the networked command, and what a model reads when this build has
/// no files.
#[cfg(target_os = "linux")]
mod platform {
    use super::*;

    pub const NETWORK: bool = true;

    pub fn sandbox(at: &Parts, files: ShellFiles) -> Arc<dyn Sandbox> {
        Arc::from(mcbench_sandbox::for_this_platform(
            mcbench_sandbox::Config {
                root: at.root.clone(),
                rootfs: (files.rootfs.file, files.rootfs.sha256),
                helper: at.helper.clone(),
                budget: Arc::clone(&at.budget),
            },
        ))
    }

    pub fn missing(why: &str) -> String {
        why.to_string()
    }

    /// Nothing to check before the sandbox is made.
    pub async fn before(_: &Parts, _: &ShellFiles) -> Result<(), (StepError, String)> {
        Ok(())
    }

    /// Nothing to check before the files are needed.
    pub async fn check(_: &Parts) -> Result<(), (StepError, String)> {
        Ok(())
    }

    /// No other sandbox reads the refusal.
    pub fn refused(_: &str) {}
}

#[cfg(target_os = "macos")]
mod platform {
    use super::*;

    pub const NETWORK: bool = true;

    /// The VM, booted in the background on the current runtime.
    pub fn sandbox(at: &Parts, files: ShellFiles) -> Arc<dyn Sandbox> {
        Arc::from(mcbench_sandbox::for_this_platform(
            mcbench_sandbox::Config {
                root: at.root.clone(),
                rootfs: (files.rootfs.file, files.rootfs.sha256),
                helper: at.helper.clone(),
                budget: Arc::clone(&at.budget),
                guest: mcbench_sandbox::GuestBase {
                    asset: files.guest.file,
                    sha256: files.guest.sha256,
                },
            },
        ))
    }

    pub fn missing(why: &str) -> String {
        format!("{}: {why}", mcbench_sandbox::macos::VM_UNAVAILABLE)
    }

    /// Nothing to check before the VM is made.
    pub async fn before(_: &Parts, _: &ShellFiles) -> Result<(), (StepError, String)> {
        Ok(())
    }

    /// Nothing to check before the files are needed.
    pub async fn check(_: &Parts) -> Result<(), (StepError, String)> {
        Ok(())
    }

    /// No other sandbox reads the refusal.
    pub fn refused(_: &str) {}
}

#[cfg(target_os = "windows")]
mod platform {
    use super::*;
    use mcbench_sandbox::windows::{self, WslSetup};
    use mcbench_setup::Needs;

    pub const NETWORK: bool = true;

    /// The app's WSL2 distro (imported by [`before`]), started in the
    /// background on the current runtime.
    pub fn sandbox(at: &Parts, files: ShellFiles) -> Arc<dyn Sandbox> {
        Arc::from(mcbench_sandbox::for_this_platform(
            mcbench_sandbox::Config {
                root: at.root.clone(),
                rootfs: (files.rootfs.file, files.rootfs.sha256),
                helper: at.helper.clone(),
                budget: Arc::clone(&at.budget),
                guest: mcbench_sandbox::GuestBase {
                    asset: files.guest.file,
                    sha256: files.guest.sha256,
                },
            },
        ))
    }

    pub fn missing(why: &str) -> String {
        format!("{}: {why}", windows::WSL_UNAVAILABLE)
    }

    /// WSL's state, read before setup's files are needed (so the person is
    /// asked to install WSL whether or not this build names them): not
    /// installed, or installed in this boot of Windows and waiting for a
    /// restart, the step needs the person (sentence 1 or 3) and a model reads
    /// [`windows::WSL_MISSING`]. Blocking work on the blocking pool.
    pub async fn check(at: &Parts) -> Result<(), (StepError, String)> {
        let root = at.root.clone();
        tokio::task::spawn_blocking(move || {
            let (needs, sentence) = match windows::setup_state(&root) {
                Ok(WslSetup::Ready) => return Ok(()),
                Ok(WslSetup::Install) => (Needs::InstallWsl, mcbench_setup::wsl::NEEDS_WSL),
                Ok(WslSetup::Restart) => (Needs::Restart, mcbench_setup::wsl::RESTART),
                Err(why) => {
                    let said = missing(&why);
                    return Err((StepError::Failed(said.clone()), said));
                }
            };
            Err((
                StepError::Needs {
                    needs,
                    failed: false,
                    sentence: sentence.into(),
                },
                windows::WSL_MISSING.to_string(),
            ))
        })
        .await
        .expect("the WSL check does not panic")
    }

    /// The distro imported from the guest base (so build programs run before
    /// the root filesystem is used).
    pub async fn before(at: &Parts, files: &ShellFiles) -> Result<(), (StepError, String)> {
        let root = at.root.clone();
        let guest = mcbench_sandbox::GuestBase {
            asset: files.guest.file.clone(),
            sha256: files.guest.sha256.clone(),
        };
        tokio::task::spawn_blocking(move || {
            windows::import(&root, &guest)
                .map(drop)
                .map_err(|said| (StepError::Failed(said.clone()), said))
        })
        .await
        .expect("the import does not panic")
    }

    /// What the build child's sandbox reads when no sandbox is made: the
    /// same sentence `bash` answers.
    pub fn refused(said: &str) {
        mcbench_build::sandbox::wsl::unavailable(said.to_string());
    }
}

/// What every platform's sandbox is made with besides setup's files.
pub struct Parts {
    root: PathBuf,
    helper: Vec<std::ffi::OsString>,
    budget: Arc<dyn Budget>,
}

enum Made {
    /// Setup has not named the files yet.
    Waiting,
    /// The platform's sandbox.
    Ready(Arc<dyn Sandbox>),
    /// No sandbox can be made yet (this build has no files, or on Windows
    /// WSL is to be installed or Windows restarted): what a model reads.
    Missing(String),
    /// Shut down before it was made.
    Stopped,
}

/// The platform's sandbox of a data folder, made of setup's files once it
/// has them ([`PlatformSandbox::make`]): calls before then wait for it.
pub struct PlatformSandbox {
    parts: Parts,
    made: watch::Sender<Made>,
    network_disabled: AtomicBool,
}

/// What Codex's box reads of setup's `files`: the root filesystem, or that
/// this build has none; a failed download leaves it waiting for a retry.
pub fn write_rootfs(slot: &RootfsSlot, files: &Result<ShellFiles, StepError>) {
    let found = match files {
        Ok(files) => ShellRootfs::File(files.rootfs.file.clone(), files.rootfs.sha256.clone()),
        Err(StepError::Unavailable(_)) => ShellRootfs::Missing,
        Err(StepError::Failed(_) | StepError::Needs { .. }) => return,
    };
    *slot.lock().expect("the shell's slot") = found;
}

/// The chat's `/work` removed, when no box can hold it.
fn remove_work(workspace: &Workspace) -> Result<(), SandboxError> {
    match std::fs::remove_dir_all(&workspace.host) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
            Err(SandboxError::Failed(e.to_string()))
        }
        _ => Ok(()),
    }
}

impl PlatformSandbox {
    /// The sandbox over `root` (the data folder's `sandbox`), with `program
    /// sandbox-agent` as its helper and `budget` as the chats' limits; made
    /// when [`PlatformSandbox::make`] is given the files.
    pub fn new(root: PathBuf, program: &Path, budget: Arc<dyn Budget>) -> PlatformSandbox {
        PlatformSandbox {
            parts: Parts {
                root,
                helper: vec![program.as_os_str().to_owned(), "sandbox-agent".into()],
                budget,
            },
            made: watch::channel(Made::Waiting).0,
            network_disabled: AtomicBool::new(false),
        }
    }

    /// Makes the platform's sandbox of setup's `files` (on macOS the VM
    /// starts booting in the background, on the current runtime), or
    /// records that there are none, and answers the sandbox or `files`'
    /// error. Made once: a later call (setup's retry) answers the sandbox
    /// made, and one after the files were missing makes it then.
    pub fn make(
        &self,
        files: Result<ShellFiles, StepError>,
    ) -> Result<Arc<dyn Sandbox>, StepError> {
        let mut answer = None;
        self.made.send_modify(|made| {
            answer = Some(match made {
                Made::Ready(sandbox) => Ok(Arc::clone(sandbox)),
                Made::Stopped => Err(StepError::Failed("The app has stopped".into())),
                Made::Waiting | Made::Missing(_) => match files {
                    Ok(files) => {
                        let sandbox = platform::sandbox(&self.parts, files);
                        if self.network_disabled.load(Ordering::SeqCst) {
                            sandbox.disable_network();
                        }
                        *made = Made::Ready(Arc::clone(&sandbox));
                        Ok(sandbox)
                    }
                    Err(error) => {
                        let (StepError::Failed(why)
                        | StepError::Unavailable(why)
                        | StepError::Needs { sentence: why, .. }) = &error;
                        let said = platform::missing(why);
                        platform::refused(&said);
                        *made = Made::Missing(said);
                        Err(error)
                    }
                },
            });
        });
        answer.expect("send_modify runs its closure")
    }

    /// The setup step's work: `files` written to `rootfs` (Codex's box reads
    /// it), the platform's check before its sandbox (on Windows WSL's state
    /// and the distro's import; when it refuses, calls answer what a model
    /// reads then), the files given to [`PlatformSandbox::make`], then the
    /// sandbox prepared.
    pub fn shell_step(
        self: Arc<Self>,
        files: impl std::future::Future<Output = Result<ShellFiles, StepError>> + Send + 'static,
        rootfs: RootfsSlot,
    ) -> mcbench_setup::Shell {
        Box::pin(async move {
            if let Err((error, said)) = platform::check(&self.parts).await {
                self.refuse(said);
                return Err(error);
            }
            let files = files.await;
            write_rootfs(&rootfs, &files);
            if let Ok(found) = &files
                && let Err((error, said)) = platform::before(&self.parts, found).await
            {
                self.refuse(said);
                return Err(error);
            }
            let sandbox = self.make(files)?;
            sandbox.prepare().await.map_err(|e| match e {
                SandboxError::Unavailable(m) => StepError::Unavailable(m),
                other => StepError::Failed(other.to_string()),
            })
        })
    }

    /// Calls answer `said` until a sandbox is made (one made stays).
    fn refuse(&self, said: String) {
        platform::refused(&said);
        self.made.send_modify(|made| {
            if matches!(made, Made::Waiting | Made::Missing(_)) {
                *made = Made::Missing(said);
            }
        });
    }

    /// The sandbox, once made.
    async fn sandbox(&self) -> Result<Arc<dyn Sandbox>, SandboxError> {
        let mut made = self.made.subscribe();
        let made = made
            .wait_for(|m| !matches!(m, Made::Waiting))
            .await
            .expect("the sender is ours");
        match &*made {
            Made::Ready(sandbox) => Ok(Arc::clone(sandbox)),
            Made::Missing(said) => Err(SandboxError::Unavailable(said.clone())),
            Made::Stopped => Err(SandboxError::Failed("The app has stopped".into())),
            Made::Waiting => unreachable!("waited for it to be made"),
        }
    }

    /// The sandbox if it is made, without waiting.
    fn made(&self) -> Option<Arc<dyn Sandbox>> {
        match &*self.made.borrow() {
            Made::Ready(sandbox) => Some(Arc::clone(sandbox)),
            _ => None,
        }
    }
}

#[async_trait]
impl Sandbox for PlatformSandbox {
    async fn prepare(&self) -> Result<(), SandboxError> {
        self.sandbox().await?.prepare().await
    }
    async fn exec(
        &self,
        workspace: &Workspace,
        command: &BashCommand,
        cancel: &CancellationToken,
    ) -> Result<BashRun, SandboxError> {
        let sandbox = tokio::select! {
            sandbox = self.sandbox() => sandbox?,
            _ = cancel.cancelled() => return Err(SandboxError::Cancelled),
        };
        sandbox.exec(workspace, command, cancel).await
    }
    async fn view_image(&self, workspace: &Workspace, path: &str) -> Result<Vec<u8>, SandboxError> {
        self.sandbox().await?.view_image(workspace, path).await
    }
    async fn remove_work_files(
        &self,
        workspace: &Workspace,
        paths: &[String],
    ) -> Result<(), SandboxError> {
        self.sandbox()
            .await?
            .remove_work_files(workspace, paths)
            .await
    }
    /// No box exists before the sandbox is made.
    async fn reset(&self, chat: &str) {
        if let Some(sandbox) = self.made() {
            sandbox.reset(chat).await
        }
    }
    /// Before the sandbox is made no box holds the chat: its `/work` goes.
    async fn destroy(&self, workspace: &Workspace) -> Result<(), SandboxError> {
        match self.made() {
            Some(sandbox) => sandbox.destroy(workspace).await,
            None => remove_work(workspace),
        }
    }
    /// The sandbox shut down (on macOS the VM stopped); one not made yet
    /// never is.
    async fn shutdown(&self) {
        let mut ready = None;
        self.made.send_modify(|made| match made {
            Made::Ready(sandbox) => ready = Some(Arc::clone(sandbox)),
            _ => *made = Made::Stopped,
        });
        if let Some(sandbox) = ready {
            sandbox.shutdown().await
        }
    }
    fn network_access(&self) -> bool {
        match self.made() {
            Some(sandbox) => sandbox.network_access(),
            None => platform::NETWORK && !self.network_disabled.load(Ordering::SeqCst),
        }
    }
    fn disable_network(&self) {
        self.network_disabled.store(true, Ordering::SeqCst);
        if let Some(sandbox) = self.made() {
            sandbox.disable_network()
        }
    }
}
