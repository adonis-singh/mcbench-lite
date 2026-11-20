//! The build child's process as a [`crate::ChildSandbox`] starts it: its
//! three pipes and the process handle the runner waits on and kills.
//!
//! macOS and Linux start a `std::process::Command` through tokio
//! ([`ChildProcess::start`]); a launch that is not a command (Windows) gives
//! its own pipes and an implementation of [`Process`] to
//! [`ChildProcess::new`].

use std::ffi::OsStr;
use std::process::{ExitStatus, Stdio};

use async_trait::async_trait;
use tokio::io::{AsyncRead, AsyncWrite};

/// The child's stdin: the parent writes the request, the world and the
/// answers to `READ` and `SETTLED` frames.
pub type ChildStdin = Box<dyn AsyncWrite + Send + Unpin>;

/// The child's stdout (its frames) or stderr (the last line of a crash).
pub type ChildOutput = Box<dyn AsyncRead + Send + Unpin>;

/// A started process, killed when dropped.
#[async_trait]
pub trait Process: Send {
    /// The process id, until the process has been waited for.
    fn id(&self) -> Option<u32>;
    /// The exit status if the process has exited, without blocking.
    fn try_wait(&mut self) -> std::io::Result<Option<ExitStatus>>;
    /// Asks the process to stop at once, without waiting.
    fn start_kill(&mut self) -> std::io::Result<()>;
    /// Waits for the process to exit.
    async fn wait(&mut self) -> std::io::Result<ExitStatus>;
}

#[async_trait]
impl Process for tokio::process::Child {
    fn id(&self) -> Option<u32> {
        tokio::process::Child::id(self)
    }

    fn try_wait(&mut self) -> std::io::Result<Option<ExitStatus>> {
        tokio::process::Child::try_wait(self)
    }

    fn start_kill(&mut self) -> std::io::Result<()> {
        tokio::process::Child::start_kill(self)
    }

    async fn wait(&mut self) -> std::io::Result<ExitStatus> {
        tokio::process::Child::wait(self).await
    }
}

/// One build child: its pipes (each taken once by the runner) and its
/// process.
pub struct ChildProcess {
    /// The child's stdin.
    pub stdin: Option<ChildStdin>,
    /// The child's stdout.
    pub stdout: Option<ChildOutput>,
    /// The child's stderr.
    pub stderr: Option<ChildOutput>,
    process: Box<dyn Process>,
}

impl ChildProcess {
    /// A child from its pipes and its process.
    pub fn new(
        process: Box<dyn Process>,
        stdin: ChildStdin,
        stdout: ChildOutput,
        stderr: ChildOutput,
    ) -> Self {
        Self {
            stdin: Some(stdin),
            stdout: Some(stdout),
            stderr: Some(stderr),
            process,
        }
    }

    /// Starts `command` as the runner starts every child: an empty
    /// environment but `env`, `/` as the working folder, the three pipes,
    /// killed when dropped.
    pub fn start(command: std::process::Command, env: &[(&str, &OsStr)]) -> std::io::Result<Self> {
        let mut command = tokio::process::Command::from(command);
        command
            .env_clear()
            .current_dir("/")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        command.envs(env.iter().copied());
        let mut child = command.spawn()?;
        let stdin = child.stdin.take().expect("piped stdin");
        let stdout = child.stdout.take().expect("piped stdout");
        let stderr = child.stderr.take().expect("piped stderr");
        Ok(Self::new(
            Box::new(child),
            Box::new(stdin),
            Box::new(stdout),
            Box::new(stderr),
        ))
    }

    /// The process id, until the process has been waited for.
    pub fn id(&self) -> Option<u32> {
        self.process.id()
    }

    /// The exit status if the process has exited, without blocking.
    pub fn try_wait(&mut self) -> std::io::Result<Option<ExitStatus>> {
        self.process.try_wait()
    }

    /// Asks the process to stop at once, without waiting.
    pub fn start_kill(&mut self) -> std::io::Result<()> {
        self.process.start_kill()
    }

    /// Waits for the process to exit.
    pub async fn wait(&mut self) -> std::io::Result<ExitStatus> {
        self.process.wait().await
    }

    /// Stops the process and waits for it.
    pub async fn kill(&mut self) -> std::io::Result<()> {
        self.start_kill()?;
        self.wait().await.map(drop)
    }
}
