//! The boxes behind [`crate::lifecycle::Sandboxes`]: what `sandbox.js`
//! asks of Docker, as one trait. [`crate::linux::Bubblewrap`] implements it
//! on the Linux kernel, [`crate::agent::GuestClient`] over a byte stream to
//! the guest program, and the lifecycle tests with a fake.
//!
//! Each chat has up to two boxes: the persistent one (no network) and the
//! networked one. This module also holds the two collectors every
//! implementation runs on a box's pipes: [`run_in_session`] (`sandbox.js`
//! `runInSession`) and [`collect_one_shot`] (`docker()`'s collection).

use std::time::Duration;

use async_trait::async_trait;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio_util::sync::CancellationToken;

/// Where a running command's output is copied as it arrives, beside its
/// collection (the guest agent streams it to the host).
pub type Tap = Option<tokio::sync::mpsc::UnboundedSender<Vec<u8>>>;

use crate::transcript::{
    Decoder, MAX_COLLECT_BYTES, SHELL_EXITED, Utf16, one_shot_overflow, session_overflow,
};
use crate::{SandboxError, Workspace};

/// A command's end in the persistent shell (`runInSession`'s result).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionRun {
    /// What it wrote, stdout and stderr in one stream.
    pub text: String,
    /// Its exit code (124 at the limit, -1 past the output limit).
    pub exit_code: i32,
    /// Whether the limit was reached.
    pub timed_out: bool,
    /// Whether the shell must be reset (a timeout or the output limit).
    pub reset: bool,
}

/// A one-shot command's end (`docker()`'s result for `oneShot`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OneShotRun {
    /// stdout and stderr in one stream, with the overflow line when it ran
    /// past the output limit.
    pub output: String,
    /// The exit code (`-1` when it was killed from outside).
    pub code: i32,
}

/// A program run in the persistent box beside the shell (`viewImage`,
/// `removeWorkFiles`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProgramRun {
    /// Its stdout.
    pub stdout: Vec<u8>,
    /// stdout and stderr in one stream (the error text of a failure).
    pub all: Vec<u8>,
    /// Its exit code.
    pub code: i32,
}

/// What `sandbox.js` asks of Docker.
#[async_trait]
pub trait Backend: Send + Sync + 'static {
    /// Makes the boxes possible: the root filesystem unpacked and checked,
    /// bubblewrap, user namespaces and the user session found.
    async fn prepare(&self) -> Result<(), SandboxError>;
    /// `ensureContainer`: the chat's box (`network` or not) running with
    /// this `/work`; one with another `/work` is replaced.
    async fn ensure(&self, workspace: &Workspace, network: bool) -> Result<(), SandboxError>;
    /// Whether the chat has a live persistent shell (`box.session`) in a
    /// box with this `/work`: the box is then up. (Today's container names
    /// hold the data folder's hash, so a shell is never another root's.)
    async fn has_session(&self, workspace: &Workspace) -> bool;
    /// `getSession` and `runInSession`: one command in the chat's persistent
    /// shell, started if there is none. Cancelled: [`SandboxError::Cancelled`].
    async fn session(
        &self,
        workspace: &Workspace,
        command: &str,
        ms: f64,
        tap: Tap,
        cancel: &CancellationToken,
    ) -> Result<SessionRun, SandboxError>;
    /// `killSession`: the persistent shell's process ended; what it started
    /// keeps running until [`Backend::end_processes`].
    async fn kill_session(&self, chat: &str);
    /// `oneShot`: a command in a fresh shell of the box, `limits` (shell
    /// builtins) first, `timeout -k 2`, and every process of the box but
    /// its init ended after it. A cancel ends the box's processes.
    // The arguments are oneShot's own (and the tap and the cancel).
    #[allow(clippy::too_many_arguments)]
    async fn one_shot(
        &self,
        workspace: &Workspace,
        network: bool,
        command: &str,
        ms: f64,
        limits: &str,
        tap: Tap,
        cancel: &CancellationToken,
    ) -> Result<OneShotRun, SandboxError>;
    /// `endProcesses`: `kill -KILL -1` in the box; nothing when it is not up.
    async fn end_processes(&self, chat: &str, network: bool);
    /// A program in the persistent box, `/work` its folder (`docker exec -w
    /// /work`), for at most `timeout`.
    async fn program(
        &self,
        workspace: &Workspace,
        argv: &[String],
        timeout: Duration,
    ) -> Result<ProgramRun, SandboxError>;
    /// `docker rm -f` of both of the chat's boxes; never fails.
    async fn remove(&self, chat: &str);
    /// Every box removed.
    async fn shutdown(&self);
}

/// A sentinel token, as `runInSession` makes one: `__MCB_` and 8 hex digits.
pub fn sentinel_token() -> String {
    use std::hash::{BuildHasher, Hasher};
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNT: AtomicU64 = AtomicU64::new(0);
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    hasher.write_u64(COUNT.fetch_add(1, Ordering::Relaxed));
    hasher.write_u128(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("the clock is after 1970")
            .as_nanos(),
    );
    format!("__MCB_{:08x}", hasher.finish() as u32)
}

/// What the shell's stdin gets for a command (`runInSession`).
pub fn session_input(command: &str, token: &str) -> String {
    format!("{command}\nprintf '\\n{token} %s\\n' \"$?\"\n")
}

/// How [`run_in_session`] ended without an answer.
#[derive(Debug, PartialEq, Eq)]
pub enum SessionEnd {
    /// Cancelled.
    Cancelled,
    /// The shell ended or could not be written: the message.
    Failed(String),
}

/// `runInSession`: the command and a sentinel line written to the shell's
/// stdin; its output collected until the sentinel echoes back with the exit
/// code, the limit passes (124, reset) or the output passes
/// [`MAX_COLLECT_BYTES`] (-1, reset). `ended` resolves when the shell's
/// process ends (the `docker exec` client's `close`).
pub async fn run_in_session<W, R, E>(
    stdin: &mut W,
    output: &mut R,
    ended: E,
    command: &str,
    ms: f64,
    tap: &Tap,
    cancel: &CancellationToken,
) -> Result<SessionRun, SessionEnd>
where
    W: AsyncWrite + Unpin + Send,
    R: AsyncRead + Unpin + Send,
    E: std::future::Future<Output = ()> + Send,
{
    tokio::pin!(ended);
    if cancel.is_cancelled() {
        return Err(SessionEnd::Cancelled);
    }
    let token = sentinel_token();
    let needle = format!("{token} ");
    let timer = tokio::time::sleep(Duration::from_secs_f64(ms / 1000.0));
    tokio::pin!(timer);
    let mut decoder = Decoder::default();
    let mut searched = 0usize;
    let input = session_input(command, &token);
    let mut written = false;
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        tokio::select! {
            biased;
            () = cancel.cancelled() => return Err(SessionEnd::Cancelled),
            () = &mut ended => return Err(SessionEnd::Failed(SHELL_EXITED.to_string())),
            () = &mut timer => {
                return Ok(SessionRun { text: decoder.text().to_string(), exit_code: 124, timed_out: true, reset: true });
            }
            result = stdin.write_all(input.as_bytes()), if !written => {
                written = true;
                if let Err(error) = result.and(stdin.flush().await) {
                    let message = if error.kind() == std::io::ErrorKind::BrokenPipe { "write EPIPE".to_string() } else { error.to_string() };
                    return Err(SessionEnd::Failed(format!("sandbox shell write failed: {message}")));
                }
            }
            read = output.read(&mut buffer) => {
                let n = match read {
                    Ok(0) | Err(_) => return Err(SessionEnd::Failed(SHELL_EXITED.to_string())),
                    Ok(n) => n,
                };
                decoder.push(&buffer[..n]);
                if let Some(tap) = tap {
                    let _ = tap.send(buffer[..n].to_vec());
                }
                if decoder.units() > MAX_COLLECT_BYTES {
                    let text = Utf16::from(decoder.text());
                    return Ok(SessionRun { text: session_overflow(&text), exit_code: -1, timed_out: false, reset: true });
                }
                let text = decoder.text();
                let from = searched.saturating_sub(needle.len() + 16);
                let from = (0..=from).rev().find(|at| text.is_char_boundary(*at)).unwrap_or(0);
                if let Some(found) = sentinel(text, from, &needle) {
                    let (at, code) = found;
                    let body = &text[..at];
                    // The sentinel printf adds its own leading newline; strip
                    // it plus the command's trailing one (/\n{1,2}$/).
                    let body = body.strip_suffix('\n').map(|b| b.strip_suffix('\n').unwrap_or(b)).unwrap_or(body);
                    return Ok(SessionRun { text: body.to_string(), exit_code: code, timed_out: false, reset: false });
                }
                searched = text.len();
            }
        }
    }
}

/// The first `<token> <digits>` followed by a non-digit at or after `from`:
/// where it starts and the code. (`runInSession` matches the digits that
/// have arrived; waiting for the line's end is the same answer without the
/// race of a code split between two reads.)
fn sentinel(text: &str, from: usize, needle: &str) -> Option<(usize, i32)> {
    let mut start = from;
    while let Some(offset) = text[start..].find(needle) {
        let at = start + offset;
        let digits: String = text[at + needle.len()..]
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        let after = at + needle.len() + digits.len();
        if !digits.is_empty() && after < text.len() {
            return Some((at, digits.parse().unwrap_or(i32::MAX)));
        }
        if digits.is_empty() {
            start = at + needle.len();
        } else {
            return None;
        }
    }
    None
}

/// `docker()`'s collection of a one-shot command: everything it writes, in
/// order, until it exits; past [`MAX_COLLECT_BYTES`] the rest is dropped,
/// `kill` is called and the overflow line is added.
pub async fn collect_one_shot<R, F>(output: &mut R, tap: &Tap, mut kill: F) -> (String, bool)
where
    R: AsyncRead + Unpin + Send,
    F: FnMut() + Send,
{
    let mut chunks = Vec::new();
    let mut collected = 0usize;
    let mut buffer = vec![0u8; 64 * 1024];
    let mut killed = false;
    loop {
        match output.read(&mut buffer).await {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                if let Some(tap) = tap {
                    let _ = tap.send(buffer[..n].to_vec());
                }
                collected += n;
                if collected > MAX_COLLECT_BYTES {
                    if !killed {
                        kill();
                        killed = true;
                    }
                    continue;
                }
                chunks.extend_from_slice(&buffer[..n]);
            }
        }
    }
    let mut text = String::from_utf8_lossy(&chunks).into_owned();
    if collected > MAX_COLLECT_BYTES {
        text.push_str(&one_shot_overflow());
    }
    (text, killed)
}

/// `oneShot`'s shell script: `limits`, then the command under `timeout -k
/// 2`, then every process of the box but its init ended.
pub fn one_shot_argv(limits: &str, ms: f64, command: &str) -> Vec<String> {
    vec![
        "bash".to_string(),
        "-c".to_string(),
        format!(
            "{limits}timeout -k 2 \"$1\" bash -c \"$2\"; code=$?; kill -KILL -1 2>/dev/null; exit $code"
        ),
        "mcb".to_string(),
        crate::transcript::js_number(ms / 1000.0),
        command.to_string(),
    ]
}

/// `docker exec` failed (`docker()` with `check`): today's error text,
/// which names Docker; `why` is the exit code or the signal's name.
pub fn exec_failed(why: &str, all: &[u8]) -> SandboxError {
    let text = Utf16::from(String::from_utf8_lossy(all).as_ref());
    let tail = text.slice(text.len().saturating_sub(2000), text.len());
    SandboxError::Failed(format!("Docker sandbox exec failed ({why}):\n{tail}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn session(script: &'static [&'static str], ms: f64) -> Result<SessionRun, SessionEnd> {
        let (mut ours, theirs) = tokio::io::duplex(1 << 20);
        let (mut read_half, mut write_half) = tokio::io::split(theirs);
        tokio::spawn(async move {
            let mut input = vec![0u8; 4096];
            let n = read_half.read(&mut input).await.unwrap();
            let token = String::from_utf8_lossy(&input[..n])
                .split("printf '\\n")
                .nth(1)
                .unwrap()
                .split(' ')
                .next()
                .unwrap()
                .to_string();
            for part in script {
                write_half
                    .write_all(part.replace("TOKEN", &token).as_bytes())
                    .await
                    .unwrap();
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            // The shell stays open: only the sentinel or the limit ends a run.
            std::future::pending::<()>().await;
        });
        let (mut r, mut w) = tokio::io::split(&mut ours);
        run_in_session(
            &mut w,
            &mut r,
            std::future::pending(),
            "x",
            ms,
            &None,
            &CancellationToken::new(),
        )
        .await
    }

    #[tokio::test]
    async fn the_sentinel_ends_the_command_with_its_code() {
        let run = session(&["out\n", "\nTOKEN 1", "27\n"], 60_000.0)
            .await
            .unwrap();
        assert_eq!(
            (run.text.as_str(), run.exit_code, run.reset),
            ("out", 127, false)
        );
        let run = session(&["a\n\n\n", "\nTOKEN 0\n"], 60_000.0)
            .await
            .unwrap();
        assert_eq!(run.text, "a\n\n");
    }

    #[tokio::test]
    async fn the_limit_resets_the_shell_with_what_it_wrote() {
        // A limit long enough that the shell has written before it ends.
        let run = session(&["partial"], 5000.0).await.unwrap();
        assert_eq!(
            run,
            SessionRun {
                text: "partial".into(),
                exit_code: 124,
                timed_out: true,
                reset: true
            }
        );
    }

    #[test]
    fn the_one_shot_script_is_todays() {
        assert_eq!(
            one_shot_argv("", 2500.0, "ls")[2..],
            [
                "timeout -k 2 \"$1\" bash -c \"$2\"; code=$?; kill -KILL -1 2>/dev/null; exit $code",
                "mcb",
                "2.5",
                "ls"
            ]
        );
    }
}
