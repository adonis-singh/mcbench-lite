//! The guest agent: [`Backend`]'s calls carried over a byte stream to a
//! Linux program of ours that runs [`crate::linux::Bubblewrap`], so macOS
//! (the app's VM) and Windows (the WSL2 distro) reuse the one Linux
//! implementation. The protocol is `GUEST.md`.
//!
//! [`GuestClient`] is the host's side, a [`Backend`] over any
//! `AsyncRead + AsyncWrite` pair; [`serve`] is the guest's; [`main`] is the
//! program (`mcbench-lite sandbox-agent`, and the small static
//! `mcbench-sandbox-agent` for the guests), which is also every box's keeper
//! (`keep`, `netinit`).
//!
//! The lifecycle ([`crate::lifecycle::Sandboxes`]: states, the storage
//! watchdog, the idle reaper) stays on the host, beside the chats' storage.

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;

use crate::backend::{OneShotRun, ProgramRun, SessionRun, Tap};
use crate::{Backend, SandboxError, Workspace};

/// The frame kinds (`GUEST.md`).
pub mod kind {
    /// host to guest: a call, JSON.
    pub const CALL: u8 = 1;
    /// guest to host: a call's answer, JSON.
    pub const ANSWER: u8 = 2;
    /// guest to host: output of a running call, the call's id (8 bytes,
    /// big-endian) and the bytes.
    pub const OUTPUT: u8 = 3;
    /// host to guest: cancel a call, JSON `{"id": n}`.
    pub const CANCEL: u8 = 4;
}

/// The largest frame.
pub const MAX_FRAME: usize = 16 * 1024 * 1024;

/// A call, as [`Backend`] has them.
#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "snake_case", tag = "call")]
pub enum Call {
    /// [`Backend::prepare`].
    Prepare,
    /// [`Backend::ensure`].
    Ensure {
        /// The chat.
        chat: String,
        /// Its `/work` in the guest.
        work: PathBuf,
        /// The networked box.
        network: bool,
    },
    /// [`Backend::has_session`].
    HasSession {
        /// The chat.
        chat: String,
        /// Its `/work` in the guest.
        work: PathBuf,
    },
    /// [`Backend::session`].
    Session {
        /// The chat.
        chat: String,
        /// Its `/work` in the guest.
        work: PathBuf,
        /// The command.
        command: String,
        /// The limit in ms.
        ms: f64,
    },
    /// [`Backend::kill_session`].
    KillSession {
        /// The chat.
        chat: String,
    },
    /// [`Backend::one_shot`].
    OneShot {
        /// The chat.
        chat: String,
        /// Its `/work` in the guest.
        work: PathBuf,
        /// The networked box.
        network: bool,
        /// The command.
        command: String,
        /// The limit in ms.
        ms: f64,
        /// Shell builtins run first.
        limits: String,
    },
    /// [`Backend::end_processes`].
    EndProcesses {
        /// The chat.
        chat: String,
        /// The networked box.
        network: bool,
    },
    /// [`Backend::program`].
    Program {
        /// The chat.
        chat: String,
        /// Its `/work` in the guest.
        work: PathBuf,
        /// argv.
        argv: Vec<String>,
        /// The limit in ms.
        timeout_ms: u64,
    },
    /// [`Backend::remove`].
    Remove {
        /// The chat.
        chat: String,
    },
    /// [`Backend::shutdown`].
    Shutdown,
}

/// A call's answer.
#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub enum Answer {
    /// Nothing to answer.
    Done,
    /// A yes or no.
    Bool(bool),
    /// [`SessionRun`].
    Session {
        /// The text.
        text: String,
        /// The code.
        exit_code: i32,
        /// The limit passed.
        timed_out: bool,
        /// The shell must be reset.
        reset: bool,
    },
    /// [`OneShotRun`].
    OneShot {
        /// The output.
        output: String,
        /// The code.
        code: i32,
    },
    /// [`ProgramRun`], the bytes as base64.
    Program {
        /// stdout.
        #[serde(with = "base64")]
        stdout: Vec<u8>,
        /// stdout and stderr.
        #[serde(with = "base64")]
        all: Vec<u8>,
        /// The code.
        code: i32,
    },
    /// [`SandboxError::Unavailable`].
    Unavailable(String),
    /// [`SandboxError::Cancelled`].
    Cancelled,
    /// [`SandboxError::Failed`].
    Failed(String),
}

#[derive(Serialize, Deserialize, Debug)]
struct Envelope<T> {
    id: u64,
    #[serde(flatten)]
    body: T,
}

#[derive(Serialize, Deserialize, Debug)]
struct Cancel {
    id: u64,
}

fn error_answer(error: SandboxError) -> Answer {
    match error {
        SandboxError::Unavailable(message) => Answer::Unavailable(message),
        SandboxError::Cancelled => Answer::Cancelled,
        SandboxError::Failed(message) => Answer::Failed(message),
    }
}

/// Writes one frame.
pub async fn write_frame<W: AsyncWrite + Unpin>(
    writer: &mut W,
    kind: u8,
    body: &[u8],
) -> std::io::Result<()> {
    let length = u32::try_from(body.len() + 1).expect("a frame under 4 GiB");
    let mut frame = Vec::with_capacity(5 + body.len());
    frame.extend_from_slice(&length.to_be_bytes());
    frame.push(kind);
    frame.extend_from_slice(body);
    writer.write_all(&frame).await?;
    writer.flush().await
}

/// Reads one frame; `None` at the stream's end.
pub async fn read_frame<R: AsyncRead + Unpin>(
    reader: &mut R,
) -> std::io::Result<Option<(u8, Vec<u8>)>> {
    let mut length = [0u8; 4];
    match reader.read_exact(&mut length).await {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(error) => return Err(error),
    }
    let length = u32::from_be_bytes(length) as usize;
    if length == 0 || length > MAX_FRAME {
        return Err(std::io::Error::other(format!("a frame of {length} bytes")));
    }
    let mut frame = vec![0u8; length];
    reader.read_exact(&mut frame).await?;
    let kind = frame.remove(0);
    Ok(Some((kind, frame)))
}

/// Serves calls read from `reader` on `backend`, answering on `writer`,
/// until the stream ends; then every box is removed.
pub async fn serve<R, W, B>(mut reader: R, writer: W, backend: Arc<B>)
where
    R: AsyncRead + Unpin + Send + 'static,
    W: AsyncWrite + Unpin + Send + 'static,
    B: Backend,
{
    let (frames, mut outgoing) = mpsc::unbounded_channel::<(u8, Vec<u8>)>();
    let writing = tokio::spawn(async move {
        let mut writer = writer;
        while let Some((kind, body)) = outgoing.recv().await {
            if write_frame(&mut writer, kind, &body).await.is_err() {
                break;
            }
        }
    });
    let cancels: Arc<Mutex<HashMap<u64, CancellationToken>>> = Arc::default();
    while let Ok(Some((kind, body))) = read_frame(&mut reader).await {
        match kind {
            kind::CALL => {
                let Ok(call) = serde_json::from_slice::<Envelope<Call>>(&body) else {
                    eprintln!("mcbench sandbox agent: a call that is not JSON");
                    break;
                };
                let token = CancellationToken::new();
                cancels
                    .lock()
                    .expect("cancels")
                    .insert(call.id, token.clone());
                let backend = backend.clone();
                let frames = frames.clone();
                let cancels = cancels.clone();
                tokio::spawn(async move {
                    let id = call.id;
                    let (tap_tx, mut tap_rx) = mpsc::unbounded_channel::<Vec<u8>>();
                    let output_frames = frames.clone();
                    let forward = tokio::spawn(async move {
                        while let Some(bytes) = tap_rx.recv().await {
                            let mut body = id.to_be_bytes().to_vec();
                            body.extend_from_slice(&bytes);
                            let _ = output_frames.send((kind::OUTPUT, body));
                        }
                    });
                    let answer = answer(&*backend, call.body, Some(tap_tx), &token).await;
                    let _ = forward.await;
                    cancels.lock().expect("cancels").remove(&id);
                    let body = serde_json::to_vec(&Envelope { id, body: answer })
                        .expect("an answer serialises");
                    let _ = frames.send((kind::ANSWER, body));
                });
            }
            kind::CANCEL => {
                if let Ok(cancel) = serde_json::from_slice::<Cancel>(&body)
                    && let Some(token) = cancels.lock().expect("cancels").get(&cancel.id)
                {
                    token.cancel();
                }
            }
            other => {
                eprintln!("mcbench sandbox agent: a frame of kind {other}");
                break;
            }
        }
    }
    backend.shutdown().await;
    drop(frames);
    let _ = writing.await;
}

fn ws(chat: String, work: PathBuf) -> Workspace {
    Workspace { chat, host: work }
}

async fn answer<B: Backend>(
    backend: &B,
    call: Call,
    tap: Tap,
    cancel: &CancellationToken,
) -> Answer {
    let result = match call {
        Call::Prepare => backend.prepare().await.map(|()| Answer::Done),
        Call::Ensure {
            chat,
            work,
            network,
        } => backend
            .ensure(&ws(chat, work), network)
            .await
            .map(|()| Answer::Done),
        Call::HasSession { chat, work } => {
            Ok(Answer::Bool(backend.has_session(&ws(chat, work)).await))
        }
        Call::Session {
            chat,
            work,
            command,
            ms,
        } => backend
            .session(&ws(chat, work), &command, ms, tap, cancel)
            .await
            .map(|run| Answer::Session {
                text: run.text,
                exit_code: run.exit_code,
                timed_out: run.timed_out,
                reset: run.reset,
            }),
        Call::KillSession { chat } => {
            backend.kill_session(&chat).await;
            Ok(Answer::Done)
        }
        Call::OneShot {
            chat,
            work,
            network,
            command,
            ms,
            limits,
        } => backend
            .one_shot(&ws(chat, work), network, &command, ms, &limits, tap, cancel)
            .await
            .map(|run| Answer::OneShot {
                output: run.output,
                code: run.code,
            }),
        Call::EndProcesses { chat, network } => {
            backend.end_processes(&chat, network).await;
            Ok(Answer::Done)
        }
        Call::Program {
            chat,
            work,
            argv,
            timeout_ms,
        } => backend
            .program(&ws(chat, work), &argv, Duration::from_millis(timeout_ms))
            .await
            .map(|run| Answer::Program {
                stdout: run.stdout,
                all: run.all,
                code: run.code,
            }),
        Call::Remove { chat } => {
            backend.remove(&chat).await;
            Ok(Answer::Done)
        }
        Call::Shutdown => {
            backend.shutdown().await;
            Ok(Answer::Done)
        }
    };
    result.unwrap_or_else(error_answer)
}

/// Maps a chat's `/work` on the host to the same folder in the guest.
pub type WorkMap = Arc<dyn Fn(&Workspace) -> Result<PathBuf, SandboxError> + Send + Sync>;

struct Pending {
    answer: oneshot::Sender<Answer>,
    tap: Tap,
}

/// The host's side: a [`Backend`] whose calls run in the guest.
pub struct GuestClient {
    frames: mpsc::UnboundedSender<(u8, Vec<u8>)>,
    pending: Arc<Mutex<HashMap<u64, Pending>>>,
    next: AtomicU64,
    map: WorkMap,
}

impl GuestClient {
    /// A client over the guest agent's stream (`reader` from it, `writer`
    /// to it), with `map` giving each `/work`'s path in the guest. Runs two
    /// tasks on the current runtime until the stream ends.
    pub fn new<R, W>(mut reader: R, writer: W, map: WorkMap) -> Self
    where
        R: AsyncRead + Unpin + Send + 'static,
        W: AsyncWrite + Unpin + Send + 'static,
    {
        let (frames, mut outgoing) = mpsc::unbounded_channel::<(u8, Vec<u8>)>();
        tokio::spawn(async move {
            let mut writer = writer;
            while let Some((kind, body)) = outgoing.recv().await {
                if write_frame(&mut writer, kind, &body).await.is_err() {
                    break;
                }
            }
        });
        let pending: Arc<Mutex<HashMap<u64, Pending>>> = Arc::default();
        let reading = pending.clone();
        tokio::spawn(async move {
            while let Ok(Some((kind, body))) = read_frame(&mut reader).await {
                match kind {
                    kind::ANSWER => {
                        let Ok(answer) = serde_json::from_slice::<Envelope<Answer>>(&body) else {
                            break;
                        };
                        if let Some(pending) = reading.lock().expect("pending").remove(&answer.id) {
                            let _ = pending.answer.send(answer.body);
                        }
                    }
                    kind::OUTPUT if body.len() >= 8 => {
                        let id = u64::from_be_bytes(body[..8].try_into().expect("8 bytes"));
                        if let Some(Pending { tap: Some(tap), .. }) =
                            reading.lock().expect("pending").get(&id)
                        {
                            let _ = tap.send(body[8..].to_vec());
                        }
                    }
                    _ => break,
                }
            }
            // The guest is gone: every call waiting fails.
            reading.lock().expect("pending").clear();
        });
        GuestClient {
            frames,
            pending,
            next: AtomicU64::new(1),
            map,
        }
    }

    async fn call(
        &self,
        call: Call,
        tap: Tap,
        cancel: Option<&CancellationToken>,
    ) -> Result<Answer, SandboxError> {
        let id = self.next.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();
        self.pending
            .lock()
            .expect("pending")
            .insert(id, Pending { answer: tx, tap });
        let body = serde_json::to_vec(&Envelope { id, body: call }).expect("a call serialises");
        let gone = || SandboxError::Failed("The bash sandbox's guest agent is gone".to_string());
        self.frames.send((kind::CALL, body)).map_err(|_| gone())?;
        let answer = match cancel {
            Some(cancel) => {
                tokio::pin!(rx);
                tokio::select! {
                    answer = &mut rx => answer,
                    () = cancel.cancelled() => {
                        let _ = self.frames.send((kind::CANCEL, serde_json::to_vec(&Cancel { id }).expect("a cancel serialises")));
                        rx.await
                    }
                }
            }
            None => rx.await,
        };
        match answer.map_err(|_| gone())? {
            Answer::Unavailable(message) => Err(SandboxError::Unavailable(message)),
            Answer::Cancelled => Err(SandboxError::Cancelled),
            Answer::Failed(message) => Err(SandboxError::Failed(message)),
            other => Ok(other),
        }
    }

    fn unexpected(answer: Answer) -> SandboxError {
        SandboxError::Failed(format!(
            "The bash sandbox's guest agent answered {answer:?}"
        ))
    }
}

#[async_trait]
impl Backend for GuestClient {
    async fn prepare(&self) -> Result<(), SandboxError> {
        self.call(Call::Prepare, None, None).await.map(|_| ())
    }
    async fn ensure(&self, workspace: &Workspace, network: bool) -> Result<(), SandboxError> {
        self.call(
            Call::Ensure {
                chat: workspace.chat.clone(),
                work: (self.map)(workspace)?,
                network,
            },
            None,
            None,
        )
        .await
        .map(|_| ())
    }
    async fn has_session(&self, workspace: &Workspace) -> bool {
        let Ok(work) = (self.map)(workspace) else {
            return false;
        };
        let call = Call::HasSession {
            chat: workspace.chat.clone(),
            work,
        };
        matches!(self.call(call, None, None).await, Ok(Answer::Bool(true)))
    }
    async fn session(
        &self,
        workspace: &Workspace,
        command: &str,
        ms: f64,
        tap: Tap,
        cancel: &CancellationToken,
    ) -> Result<SessionRun, SandboxError> {
        let call = Call::Session {
            chat: workspace.chat.clone(),
            work: (self.map)(workspace)?,
            command: command.to_string(),
            ms,
        };
        match self.call(call, tap, Some(cancel)).await? {
            Answer::Session {
                text,
                exit_code,
                timed_out,
                reset,
            } => Ok(SessionRun {
                text,
                exit_code,
                timed_out,
                reset,
            }),
            other => Err(Self::unexpected(other)),
        }
    }
    async fn kill_session(&self, chat: &str) {
        let _ = self
            .call(
                Call::KillSession {
                    chat: chat.to_string(),
                },
                None,
                None,
            )
            .await;
    }
    async fn one_shot(
        &self,
        workspace: &Workspace,
        network: bool,
        command: &str,
        ms: f64,
        limits: &str,
        tap: Tap,
        cancel: &CancellationToken,
    ) -> Result<OneShotRun, SandboxError> {
        let call = Call::OneShot {
            chat: workspace.chat.clone(),
            work: (self.map)(workspace)?,
            network,
            command: command.to_string(),
            ms,
            limits: limits.to_string(),
        };
        match self.call(call, tap, Some(cancel)).await? {
            Answer::OneShot { output, code } => Ok(OneShotRun { output, code }),
            other => Err(Self::unexpected(other)),
        }
    }
    async fn end_processes(&self, chat: &str, network: bool) {
        let _ = self
            .call(
                Call::EndProcesses {
                    chat: chat.to_string(),
                    network,
                },
                None,
                None,
            )
            .await;
    }
    async fn program(
        &self,
        workspace: &Workspace,
        argv: &[String],
        timeout: Duration,
    ) -> Result<ProgramRun, SandboxError> {
        let call = Call::Program {
            chat: workspace.chat.clone(),
            work: (self.map)(workspace)?,
            argv: argv.to_vec(),
            timeout_ms: timeout.as_millis() as u64,
        };
        match self.call(call, None, None).await? {
            Answer::Program { stdout, all, code } => Ok(ProgramRun { stdout, all, code }),
            other => Err(Self::unexpected(other)),
        }
    }
    async fn remove(&self, chat: &str) {
        let _ = self
            .call(
                Call::Remove {
                    chat: chat.to_string(),
                },
                None,
                None,
            )
            .await;
    }
    async fn shutdown(&self) {
        let _ = self.call(Call::Shutdown, None, None).await;
    }
}

/// [`crate::Sandbox`] for a platform type whose `guest` field is a
/// [`crate::lifecycle::Sandboxes`] over a [`GuestClient`].
#[macro_export]
#[doc(hidden)]
macro_rules! guest_sandbox {
    ($platform:ty) => {
        #[async_trait::async_trait]
        impl $crate::Sandbox for $platform {
            async fn prepare(&self) -> Result<(), $crate::SandboxError> {
                $crate::Sandbox::prepare(&self.guest).await
            }
            async fn exec(
                &self,
                workspace: &$crate::Workspace,
                command: &$crate::BashCommand,
                cancel: &tokio_util::sync::CancellationToken,
            ) -> Result<$crate::BashRun, $crate::SandboxError> {
                $crate::Sandbox::exec(&self.guest, workspace, command, cancel).await
            }
            async fn view_image(
                &self,
                workspace: &$crate::Workspace,
                path: &str,
            ) -> Result<Vec<u8>, $crate::SandboxError> {
                $crate::Sandbox::view_image(&self.guest, workspace, path).await
            }
            async fn remove_work_files(
                &self,
                workspace: &$crate::Workspace,
                paths: &[String],
            ) -> Result<(), $crate::SandboxError> {
                $crate::Sandbox::remove_work_files(&self.guest, workspace, paths).await
            }
            async fn reset(&self, chat: &str) {
                $crate::Sandbox::reset(&self.guest, chat).await
            }
            async fn destroy(
                &self,
                workspace: &$crate::Workspace,
            ) -> Result<(), $crate::SandboxError> {
                $crate::Sandbox::destroy(&self.guest, workspace).await
            }
            async fn shutdown(&self) {
                $crate::Sandbox::shutdown(&self.guest).await
            }
            fn network_access(&self) -> bool {
                $crate::Sandbox::network_access(&self.guest)
            }
            fn disable_network(&self) {
                $crate::Sandbox::disable_network(&self.guest)
            }
        }
    };
}
pub use guest_sandbox;

/// Bytes in JSON as base64 (an image's PNG crosses as one answer).
mod base64 {
    use serde::{Deserialize, Deserializer, Serializer};

    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    pub fn serialize<S: Serializer>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
        let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
        for chunk in bytes.chunks(3) {
            let n = (u32::from(chunk[0]) << 16)
                | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
                | u32::from(*chunk.get(2).unwrap_or(&0));
            for i in 0..4 {
                if i <= chunk.len() {
                    out.push(TABLE[(n >> (18 - 6 * i) & 63) as usize] as char);
                } else {
                    out.push('=');
                }
            }
        }
        serializer.serialize_str(&out)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
        let text = String::deserialize(deserializer)?;
        let mut out = Vec::with_capacity(text.len() / 4 * 3);
        let mut acc = 0u32;
        let mut bits = 0;
        for byte in text.bytes().filter(|b| *b != b'=') {
            let value = TABLE
                .iter()
                .position(|t| *t == byte)
                .ok_or_else(|| serde::de::Error::custom("not base64"))?
                as u32;
            acc = (acc << 6) | value;
            bits += 6;
            if bits >= 8 {
                bits -= 8;
                out.push((acc >> bits) as u8);
                acc &= (1 << bits) - 1;
            }
        }
        Ok(out)
    }
}

/// A byte stream over a socket descriptor (the VM's vsock connection, on
/// the host and in the guest), for [`serve`] and [`GuestClient`].
#[cfg(unix)]
pub struct FdStream(tokio::io::unix::AsyncFd<std::os::fd::OwnedFd>);

#[cfg(unix)]
impl FdStream {
    /// The stream over `fd`, made non-blocking; on the current runtime.
    pub fn new(fd: std::os::fd::OwnedFd) -> std::io::Result<Self> {
        use std::os::fd::AsRawFd;
        // SAFETY: fcntl on a descriptor we own.
        unsafe {
            let flags = libc::fcntl(fd.as_raw_fd(), libc::F_GETFL);
            if flags < 0 || libc::fcntl(fd.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) < 0
            {
                return Err(std::io::Error::last_os_error());
            }
        }
        // SAFETY: the OwnedFd owns the descriptor and keeps it open until the
        // AsyncFd drops it.
        Ok(FdStream(unsafe { tokio::io::unix::AsyncFd::register(fd) }?))
    }
}

#[cfg(unix)]
impl AsyncRead for FdStream {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        use std::os::fd::AsRawFd;
        loop {
            let mut guard = std::task::ready!(self.0.poll_read_ready(cx))?;
            let unfilled = buf.initialize_unfilled();
            let got = guard.try_io(|fd| {
                // SAFETY: read(2) into the unfilled part of the buffer.
                let n = unsafe {
                    libc::read(
                        fd.get_ref().as_raw_fd(),
                        unfilled.as_mut_ptr() as *mut _,
                        unfilled.len(),
                    )
                };
                if n < 0 {
                    Err(std::io::Error::last_os_error())
                } else {
                    Ok(n as usize)
                }
            });
            match got {
                Ok(Ok(n)) => {
                    buf.advance(n);
                    return std::task::Poll::Ready(Ok(()));
                }
                Ok(Err(error)) => return std::task::Poll::Ready(Err(error)),
                Err(_would_block) => continue,
            }
        }
    }
}

#[cfg(unix)]
impl AsyncWrite for FdStream {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        data: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        use std::os::fd::AsRawFd;
        loop {
            let mut guard = std::task::ready!(self.0.poll_write_ready(cx))?;
            let got = guard.try_io(|fd| {
                // SAFETY: write(2) of the caller's buffer.
                let n = unsafe {
                    libc::write(
                        fd.get_ref().as_raw_fd(),
                        data.as_ptr() as *const _,
                        data.len(),
                    )
                };
                if n < 0 {
                    Err(std::io::Error::last_os_error())
                } else {
                    Ok(n as usize)
                }
            });
            match got {
                Ok(result) => return std::task::Poll::Ready(result),
                Err(_would_block) => continue,
            }
        }
    }
    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::task::Poll::Ready(Ok(()))
    }
    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        use std::os::fd::AsRawFd;
        // SAFETY: shutdown of a socket we hold.
        unsafe { libc::shutdown(self.0.get_ref().as_raw_fd(), libc::SHUT_WR) };
        std::task::Poll::Ready(Ok(()))
    }
}

/// `clock <file>`: the macOS VM's clock, kept on the host's. The VM has no
/// clock of its own and stops while the Mac sleeps; a file on a share of the
/// host's file system gets the host's time when its times are set to now
/// (virtiofs passes "now" to the host), so every [`CLOCK_EVERY`] the file is
/// touched and the guest's clock set to its new modification time. Files a
/// box writes to `/work` get the host's times too, so the guest's clock must
/// not fall behind them. Run as root by the VM's init; it never returns.
#[cfg(target_os = "linux")]
fn clock(file: &std::path::Path) -> ! {
    use std::os::fd::AsRawFd;
    let handle = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(file)
        .unwrap_or_else(|error| {
            eprintln!("mcbench sandbox clock: {}: {error}", file.display());
            std::process::exit(1)
        });
    loop {
        // SAFETY: futimens with null times (both now) and fstat into a
        // zeroed struct, on a descriptor we hold; clock_settime of the
        // realtime clock with the time read.
        let set = unsafe {
            let mut stat: libc::stat = std::mem::zeroed();
            libc::futimens(handle.as_raw_fd(), std::ptr::null()) == 0
                && libc::fstat(handle.as_raw_fd(), &mut stat) == 0
                && libc::clock_settime(
                    libc::CLOCK_REALTIME,
                    &libc::timespec {
                        tv_sec: stat.st_mtime,
                        tv_nsec: stat.st_mtime_nsec,
                    },
                ) == 0
        };
        if !set {
            eprintln!(
                "mcbench sandbox clock: {}: {}",
                file.display(),
                std::io::Error::last_os_error()
            );
        }
        std::thread::sleep(CLOCK_EVERY);
    }
}

/// How often the macOS VM's clock is set to the host's.
pub const CLOCK_EVERY: std::time::Duration = std::time::Duration::from_secs(2);

/// Drops root for the user `uid`:`gid` (the guest's init starts the agent
/// as root): its group alone as supplementary groups, then the group and the
/// user, which clears every capability.
#[cfg(target_os = "linux")]
pub(crate) fn become_user(uid: u32, gid: u32) {
    // SAFETY: setgroups, setgid and setuid of this process, before any
    // thread is started.
    let done = unsafe {
        libc::setgroups(1, &gid) == 0 && libc::setgid(gid) == 0 && libc::setuid(uid) == 0
    };
    if !done {
        eprintln!(
            "mcbench sandbox agent: becoming {uid}:{gid}: {}",
            std::io::Error::last_os_error()
        );
        std::process::exit(1)
    }
}

/// Raises this process's open-file limit, soft and hard, to the boxes'
/// (`linux::keeper::RLIMITS`), as root: the VM's kernel starts every
/// process at a hard limit of 4096, which the keeper, as the agent's user,
/// could not raise.
#[cfg(target_os = "linux")]
fn raise_open_files() {
    let (_, open_files) = crate::linux::keeper::RLIMITS[0];
    let limit = libc::rlimit {
        rlim_cur: open_files as libc::rlim_t,
        rlim_max: open_files as libc::rlim_t,
    };
    // SAFETY: setrlimit of this process with a valid rlimit.
    if unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &limit) } != 0 {
        eprintln!(
            "mcbench sandbox agent: raising the open-file limit to {open_files}: {}",
            std::io::Error::last_os_error()
        );
        std::process::exit(1)
    }
}

/// Waits for the host's one connection on the vsock `port` (any CID).
#[cfg(target_os = "linux")]
fn vsock_accept(port: u32) -> std::io::Result<std::os::fd::OwnedFd> {
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    // SAFETY: socket, bind, listen and accept on descriptors made here, with
    // a zeroed sockaddr_vm of the right size.
    unsafe {
        let listener = libc::socket(libc::AF_VSOCK, libc::SOCK_STREAM | libc::SOCK_CLOEXEC, 0);
        if listener < 0 {
            return Err(std::io::Error::last_os_error());
        }
        let listener = OwnedFd::from_raw_fd(listener);
        let mut address: libc::sockaddr_vm = std::mem::zeroed();
        address.svm_family = libc::AF_VSOCK as libc::sa_family_t;
        address.svm_port = port;
        address.svm_cid = libc::VMADDR_CID_ANY;
        if libc::bind(
            listener.as_raw_fd(),
            &address as *const libc::sockaddr_vm as *const libc::sockaddr,
            std::mem::size_of::<libc::sockaddr_vm>() as libc::socklen_t,
        ) != 0
            || libc::listen(listener.as_raw_fd(), 1) != 0
        {
            return Err(std::io::Error::last_os_error());
        }
        let connection = libc::accept4(
            listener.as_raw_fd(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            libc::SOCK_CLOEXEC,
        );
        if connection < 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(OwnedFd::from_raw_fd(connection))
    }
}

const USAGE: &str = "usage: sandbox-agent serve <sandbox folder> <rootfs.tar.zst> <sha256> | guest <vsock port>|- <uid>:<gid> <cgroup folder> <sandbox folder> <rootfs.tar.zst> <sha256> | clock <file> | keep | netinit <spec> | binfmt <folder> <program...> | relay <port> <socket> -- <command...> | tool-bridge <socket> | codex <login> <tools> <harness> <uid>:<gid> cli|bridge|box ...";

/// The agent program. `helper` is how this program is started (the
/// program, and `sandbox-agent` before the arguments when it is
/// `mcbench-lite`): boxes' keepers are started that way.
///
/// | Arguments | Role |
/// |---|---|
/// | `serve <sandbox folder> <rootfs.tar.zst> <sha256>` | the guest agent on stdin and stdout, each box in a systemd user scope |
/// | `guest <vsock port>\|- <uid>:<gid> <cgroup folder> <sandbox folder> <rootfs.tar.zst> <sha256>` | the guest agent of the macOS VM and the WSL2 distro, started as root by the VM's init or the distro's `wsl-init`: it becomes the user (no supplementary group but its own), then serves the host's one vsock connection, or with `-` its stdin and stdout (the pipes of `wsl.exe`), each box in a child of the cgroup folder the init delegated |
/// | `keep` | a box's keeper (stdin is its control socket) |
/// | `netinit <spec>` | the networked box's set-up inside pasta's namespaces |
/// | `binfmt <folder> <program...>` | a guest's box start: the program (bubblewrap, or pasta) in a user and mount namespace of the box's own with an empty binfmt_misc mounted on the folder (`linux::keeper::own_binfmt`) |
/// | `codex <login> <tools> <harness> <uid>:<gid> <verb> ...` | the Codex CLI in the app's WSL distro, started as root by `wsl.exe` (`crate::wsl_codex`): its login, tools and harness folders mounted, then Codex outside the box (`cli`), the run's tool bridge (`bridge`) or the box (`box`) |
pub fn main(helper: Vec<OsString>, args: &[String]) -> ! {
    #[cfg(target_os = "linux")]
    match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["relay", port, socket, "--", command @ ..] if !command.is_empty() => {
            let port = port.parse().unwrap_or_else(|_| {
                eprintln!("{USAGE}");
                std::process::exit(2)
            });
            crate::proxy::relay(port, std::path::Path::new(socket), &args[4..])
        }
        // The tool bridge inside Codex's box (`mcbench-lite tool-bridge`'s
        // relay, which needs the app's whole binary outside it). Its stderr
        // is the app's: not dumpable, as the box's relay.
        ["tool-bridge", socket] => {
            crate::proxy::undumpable();
            mcbench_relay::relay_main(&mcbench_relay::BridgeAddress::UnixSocket(socket.into()))
        }
        // Codex in the app's WSL distro (`crate::wsl_codex`), as root.
        ["codex", login, tools, harness, ids, verb @ ..] if !verb.is_empty() => {
            crate::wsl_codex::main(login, tools, harness, ids, &args[5..])
        }
        ["keep"] => crate::linux::keeper::keep(),
        ["clock", file] => clock(std::path::Path::new(file)),
        ["netinit", spec] => crate::linux::keeper::netinit(std::path::Path::new(spec)),
        ["binfmt", argv @ ..] => crate::linux::keeper::own_binfmt(argv),
        ["serve", root, rootfs, sha256] => {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("a tokio runtime");
            runtime.block_on(async {
                let backend = Arc::new(crate::linux::Bubblewrap::new(
                    root.into(),
                    (rootfs.into(), sha256.to_string()),
                    helper,
                    crate::linux::Delegation::UserScope,
                ));
                serve(tokio::io::stdin(), tokio::io::stdout(), backend).await;
            });
            std::process::exit(0)
        }
        ["guest", port, user, cgroup, root, rootfs, sha256] => {
            // `-`: the host's stream is stdin and stdout (the WSL2 distro,
            // where they are the pipes of `wsl.exe`); a number: a vsock port.
            let port: Option<Option<u32>> = if *port == "-" {
                Some(None)
            } else {
                port.parse().ok().map(Some)
            };
            let parsed = port.zip(
                user.split_once(':')
                    .and_then(|(uid, gid)| uid.parse().ok().zip(gid.parse().ok())),
            );
            let Some((port, (uid, gid))) = parsed else {
                eprintln!("{USAGE}");
                std::process::exit(2)
            };
            raise_open_files();
            become_user(uid, gid);
            let connection = port.map(|port| {
                vsock_accept(port).unwrap_or_else(|error| {
                    eprintln!("mcbench sandbox agent: vsock port {port}: {error}");
                    std::process::exit(1)
                })
            });
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("a tokio runtime");
            runtime.block_on(async {
                let backend = Arc::new(crate::linux::Bubblewrap::new(
                    root.into(),
                    (rootfs.into(), sha256.to_string()),
                    helper,
                    crate::linux::Delegation::Subtree(cgroup.into()),
                ));
                match connection {
                    Some(connection) => {
                        let stream = FdStream::new(connection).expect("the vsock connection");
                        let (reader, writer) = tokio::io::split(stream);
                        serve(reader, writer, backend).await;
                    }
                    None => serve(tokio::io::stdin(), tokio::io::stdout(), backend).await,
                }
            });
            std::process::exit(0)
        }
        _ => {
            eprintln!("{USAGE}");
            std::process::exit(2)
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (helper, args);
        eprintln!("{USAGE}: the agent runs on Linux");
        std::process::exit(2)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The frames' JSON as GUEST.md documents it.
    #[test]
    fn the_wire_forms_are_guest_mds() {
        let call = |body: Call| serde_json::to_string(&Envelope { id: 7, body }).unwrap();
        let answer = |body: Answer| serde_json::to_string(&Envelope { id: 7, body }).unwrap();
        assert_eq!(call(Call::Prepare), r#"{"id":7,"call":"prepare"}"#);
        assert_eq!(
            call(Call::Ensure {
                chat: "c".into(),
                work: "/w".into(),
                network: true
            }),
            r#"{"id":7,"call":"ensure","chat":"c","work":"/w","network":true}"#
        );
        assert_eq!(answer(Answer::Done), r#"{"id":7,"done":null}"#);
        assert_eq!(answer(Answer::Bool(true)), r#"{"id":7,"bool":true}"#);
        assert_eq!(answer(Answer::Cancelled), r#"{"id":7,"cancelled":null}"#);
        assert_eq!(
            answer(Answer::Failed("x".into())),
            r#"{"id":7,"failed":"x"}"#
        );
        assert_eq!(
            answer(Answer::Program {
                stdout: b"\x89PNG".to_vec(),
                all: vec![],
                code: 0
            }),
            r#"{"id":7,"program":{"stdout":"iVBORw==","all":"","code":0}}"#
        );
        let back: Envelope<Answer> =
            serde_json::from_str(r#"{"id":7,"program":{"stdout":"iVBORw==","all":"","code":0}}"#)
                .unwrap();
        assert!(matches!(back.body, Answer::Program { ref stdout, .. } if stdout == b"\x89PNG"));
    }
}
