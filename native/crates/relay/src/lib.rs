//! # mcbench-relay
//!
//! The tool bridge's relay (`server/mcp-bridge.py`): a CLI starts it as an
//! ordinary stdio MCP server, and it carries its stdin to the main process's
//! bridge (a Unix socket on macOS and Linux, a named pipe on Windows) and
//! the bridge's answers to its stdout. It is a crate of its own so that the
//! two programs that run it share one copy:
//! `mcbench-lite tool-bridge <address>` (through `mcbench_providers::bridge`)
//! and, inside Codex's box on Linux, the static
//! `mcbench-sandbox-agent tool-bridge <socket>`
//! (`mcbench-sandbox` may not depend on `mcbench-providers`, and the app's
//! own binary, linked against glibc, the GPU libraries and V8, does not run
//! on the box's root filesystem). It depends on tokio and, on macOS and
//! Linux, libc.
//!
//! It also holds the one rule for every Unix socket of the app's (the
//! bridge's, and the Codex box's egress proxy's): where a run's socket
//! folder is made (`socket_base`) and how a socket is bound and reached
//! (`bind_unix`, `connect_unix`), so that no socket's address depends on
//! how long the temporary folder's path is.
//!
//! ## Seams
//!
//! | Seam | What crosses | Who calls whom | Thread or process |
//! |---|---|---|---|
//! | the bridge | bytes, both ways, until the bridge ends | `mcbench-providers` and `mcbench-sandbox`'s agent call [`relay_main`] | a process of its own, a current-thread runtime |
//! | a Unix socket's place | a socket's path in; a bound listener or a connected stream out | `mcbench-providers` (the bridge, a run's socket folder) and `mcbench-sandbox` (the egress proxy, and its relay in the box) call `socket_base`, `bind_unix` and `connect_unix` | the caller's |
//!
//! ## Decided
//!
//! A Unix socket's address holds at most 103 bytes on macOS and 107 on
//! Linux (`sun_path` less its NUL), and a temporary folder's path can be
//! longer (a user's `TMPDIR`, a test runner's). On Linux a socket is bound
//! and reached as `/proc/self/fd/<descriptor of its folder>/<name>`, which
//! Linux resolves through the open folder: an address of at most 36 bytes
//! for the app's names wherever the folder is, and the folder stays where
//! `std::env::temp_dir` puts it, with the permissions it had. macOS has no
//! such path (no `/proc`; its `bindat` is not public), so there a run's
//! socket folder is made in the account's own temporary folder as the
//! system names it (`confstr(_CS_DARWIN_USER_TEMP_DIR)`, of the fixed form
//! `/var/folders/<2>/<30>/T/`, 49 bytes, private to the account), whatever
//! `TMPDIR` says: the longest address is then 76 bytes. Node's server puts
//! the socket in `os.tmpdir()`; its path reaches no model.

#![allow(clippy::needless_doctest_main)]

use std::path::PathBuf;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// Where the bridge listens: a Unix socket on macOS and Linux, a named
/// pipe on Windows. Never a TCP port.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BridgeAddress {
    /// A Unix socket's path.
    UnixSocket(PathBuf),
    /// A named pipe's name, `\\.\pipe\...`.
    NamedPipe(String),
}

impl BridgeAddress {
    /// The address as the command line names it.
    pub fn to_arg(&self) -> String {
        match self {
            BridgeAddress::UnixSocket(path) => path.to_string_lossy().into_owned(),
            BridgeAddress::NamedPipe(name) => name.clone(),
        }
    }
}

/// `mcbench-lite tool-bridge <address>`: relays stdin to the address
/// and the address to stdout until the address ends, as `mcp-bridge.py`:
/// stdin's end shuts the connection's write side, and the process exits 0
/// when the connection ends. It is a process of its own and runs a
/// current-thread runtime. A connection that fails exits 1 with the reason.
pub fn relay_main(address: &BridgeAddress) -> ! {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .build()
        .unwrap_or_else(|e| fail(&format!("mcbench-lite tool-bridge: {e}")));
    let code = runtime.block_on(async {
        match address {
            BridgeAddress::UnixSocket(path) => {
                #[cfg(unix)]
                {
                    let connected = connect_unix(path).and_then(|stream| {
                        stream.set_nonblocking(true)?;
                        tokio::net::UnixStream::from_std(stream)
                    });
                    match connected {
                        Ok(stream) => relay(stream).await,
                        Err(e) => fail(&format!(
                            "mcbench-lite tool-bridge: connect {}: {e}",
                            path.display()
                        )),
                    }
                }
                #[cfg(not(unix))]
                fail(&format!(
                    "mcbench-lite tool-bridge: a Unix socket ({}) needs macOS or Linux",
                    path.display()
                ))
            }
            BridgeAddress::NamedPipe(name) => {
                #[cfg(windows)]
                {
                    match connect_pipe(name).await {
                        Ok(stream) => relay(stream).await,
                        Err(e) => fail(&format!("mcbench-lite tool-bridge: connect {name}: {e}")),
                    }
                }
                #[cfg(not(windows))]
                fail(&format!(
                    "mcbench-lite tool-bridge: a named pipe ({name}) needs Windows"
                ))
            }
        }
    });
    std::process::exit(code)
}

/// The folder in which a run makes its private socket folder
/// (`mcb-XXXXXX`, mode 0700): on Linux the system's temporary folder
/// (`std::env::temp_dir`), on macOS the account's own temporary folder as
/// the system names it (`confstr(_CS_DARWIN_USER_TEMP_DIR)`), whatever
/// `TMPDIR` says, since a macOS socket is bound by its whole path.
#[cfg(unix)]
pub fn socket_base() -> std::io::Result<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::ffi::OsStringExt;
        let mut buffer = vec![0u8; libc::PATH_MAX as usize];
        // SAFETY: confstr writes at most `buffer.len()` bytes, its NUL
        // included, into a buffer this function owns, and answers the size
        // the whole value needs (0 on an error).
        let needed = unsafe {
            libc::confstr(
                libc::_CS_DARWIN_USER_TEMP_DIR,
                buffer.as_mut_ptr().cast(),
                buffer.len(),
            )
        };
        if needed == 0 || needed > buffer.len() {
            return Err(std::io::Error::other(format!(
                "confstr(_CS_DARWIN_USER_TEMP_DIR): {}",
                std::io::Error::last_os_error()
            )));
        }
        buffer.truncate(needed - 1);
        Ok(PathBuf::from(std::ffi::OsString::from_vec(buffer)))
    }
    #[cfg(not(target_os = "macos"))]
    {
        Ok(std::env::temp_dir())
    }
}

/// Binds a Unix socket at `path` and listens on it, by an address whose
/// length does not depend on the path's (the crate's Decided section).
#[cfg(unix)]
pub fn bind_unix(path: &std::path::Path) -> std::io::Result<std::os::unix::net::UnixListener> {
    by_address(path, |address| {
        std::os::unix::net::UnixListener::bind(address)
    })
}

/// Connects to the Unix socket at `path`, as [`bind_unix`] binds it.
#[cfg(unix)]
pub fn connect_unix(path: &std::path::Path) -> std::io::Result<std::os::unix::net::UnixStream> {
    by_address(path, |address| {
        std::os::unix::net::UnixStream::connect(address)
    })
}

/// Linux: `path` as `/proc/self/fd/<its folder, opened>/<its name>`, the
/// folder held open while `act` binds or connects.
#[cfg(all(unix, not(target_os = "macos")))]
fn by_address<T>(
    path: &std::path::Path,
    act: impl FnOnce(&std::path::Path) -> std::io::Result<T>,
) -> std::io::Result<T> {
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::OpenOptionsExt;
    let (Some(folder), Some(name)) = (path.parent(), path.file_name()) else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "a socket's path names a file in a folder",
        ));
    };
    let folder = if folder.as_os_str().is_empty() {
        std::path::Path::new(".")
    } else {
        folder
    };
    // O_PATH: the folder is only named through, so it needs no read
    // permission, as a path's folders need none.
    let folder = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_PATH | libc::O_DIRECTORY)
        .open(folder)?;
    act(&std::path::Path::new("/proc/self/fd")
        .join(folder.as_raw_fd().to_string())
        .join(name))
}

/// macOS: `path` itself; a run's socket folder is in [`socket_base`].
#[cfg(target_os = "macos")]
fn by_address<T>(
    path: &std::path::Path,
    act: impl FnOnce(&std::path::Path) -> std::io::Result<T>,
) -> std::io::Result<T> {
    act(path)
}

fn fail(message: &str) -> ! {
    eprintln!("{message}");
    std::process::exit(1)
}

#[cfg(windows)]
async fn connect_pipe(
    name: &str,
) -> std::io::Result<tokio::net::windows::named_pipe::NamedPipeClient> {
    use tokio::net::windows::named_pipe::ClientOptions;
    // ERROR_PIPE_BUSY: every instance is taken; the server makes the next
    // one as it accepts, so the client tries again shortly.
    const ERROR_PIPE_BUSY: i32 = 231;
    loop {
        match ClientOptions::new().open(name) {
            Ok(client) => return Ok(client),
            Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY) => {
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
            Err(e) => return Err(e),
        }
    }
}

async fn relay<S>(stream: S) -> i32
where
    S: AsyncRead + AsyncWrite + Send + 'static,
{
    let (mut from, mut to) = tokio::io::split(stream);
    // Upstream: stdin to the connection; its end shuts the write side.
    tokio::spawn(async move {
        let mut stdin = tokio::io::stdin();
        let mut buffer = vec![0u8; 65536];
        loop {
            match stdin.read(&mut buffer).await {
                Ok(0) | Err(_) => {
                    let _ = to.shutdown().await;
                    return;
                }
                Ok(n) => {
                    if to.write_all(&buffer[..n]).await.is_err() {
                        return;
                    }
                }
            }
        }
    });
    let mut stdout = tokio::io::stdout();
    let mut buffer = vec![0u8; 65536];
    loop {
        match from.read(&mut buffer).await {
            Ok(0) | Err(_) => return 0,
            Ok(n) => {
                if stdout.write_all(&buffer[..n]).await.is_err() || stdout.flush().await.is_err() {
                    return 0;
                }
            }
        }
    }
}
