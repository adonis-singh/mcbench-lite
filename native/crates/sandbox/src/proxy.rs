//! Codex's network on Linux: the allow-list CONNECT proxy on a Unix socket
//! (`server/egress-proxy.js`), the bubblewrap command line the Codex CLI
//! runs under with that proxy as its only network (today's
//! `codex-cli.js` `containerArgs`), and the relay inside it that
//! `container-shim.py` was (`sandbox-agent relay`).
//!
//! Today the CLI runs in a `--network none` container of the sandbox image;
//! here it runs under bubblewrap on the same root filesystem with only
//! loopback, and the relay listens on its `127.0.0.1:3128` and carries each
//! connection to the proxy's socket, mounted at `/run/mcbench`.

use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::sync::Arc;

#[cfg(unix)]
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// `egressAllowed(target, allowedHosts)`: `host:443` with a listed host.
pub fn egress_allowed(target: &str, allowed: &[&str]) -> bool {
    let Some((host, port)) = target.rsplit_once(':') else {
        return false;
    };
    let host_ok = !host.is_empty()
        && host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-');
    let port_ok = !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit());
    host_ok && port_ok && port == "443" && allowed.contains(&host.to_ascii_lowercase().as_str())
}

/// `new Date().toISOString()`.
pub fn iso_now() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("after 1970");
    let (secs, millis) = (now.as_secs() as i64, now.subsec_millis());
    let days = secs.div_euclid(86_400);
    let rest = secs.rem_euclid(86_400);
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{millis:03}Z",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

/// A running proxy.
#[cfg(unix)]
pub struct EgressProxy {
    stop: tokio_util::sync::CancellationToken,
    accepting: tokio::task::JoinHandle<()>,
}

#[cfg(unix)]
impl EgressProxy {
    /// Stops accepting (`server.close`); open tunnels run on.
    pub async fn close(self) {
        self.stop.cancel();
        let _ = self.accepting.await;
    }
}

#[cfg(unix)]
fn log(file: &Path, target: &str, allowed: bool) {
    use std::io::Write;
    let line = format!(
        "{}\n",
        serde_json::json!({ "at": iso_now(), "target": target, "allowed": allowed })
    );
    let mut out = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(file)
        .expect("the egress log");
    out.write_all(line.as_bytes()).expect("the egress log");
}

/// `startEgressProxy(socketPath, allowedHosts, logFile)`: an HTTP CONNECT
/// proxy on a Unix socket that opens tunnels only to the listed hosts on
/// port 443, refuses every other request, and logs each decision as a JSON
/// line `{"at","target","allowed"}`.
#[cfg(unix)]
pub async fn start_egress_proxy(
    socket: &Path,
    allowed: Vec<String>,
    log_file: PathBuf,
) -> std::io::Result<EgressProxy> {
    let listener = mcbench_relay::bind_unix(socket).and_then(|listener| {
        listener.set_nonblocking(true)?;
        tokio::net::UnixListener::from_std(listener)
    })?;
    let allowed = Arc::new(allowed);
    let log_file = Arc::new(log_file);
    let stop = tokio_util::sync::CancellationToken::new();
    let stopping = stop.clone();
    let accepting = tokio::spawn(async move {
        loop {
            let client = tokio::select! {
                () = stopping.cancelled() => return,
                accepted = listener.accept() => match accepted { Ok((client, _)) => client, Err(_) => continue },
            };
            let allowed = allowed.clone();
            let log_file = log_file.clone();
            tokio::spawn(async move {
                let _ = tunnel(client, &allowed, &log_file).await;
            });
        }
    });
    Ok(EgressProxy { stop, accepting })
}

#[cfg(unix)]
async fn tunnel(
    mut client: tokio::net::UnixStream,
    allowed: &[String],
    log_file: &Path,
) -> std::io::Result<()> {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 8192];
    let end = loop {
        let n = client.read(&mut chunk).await?;
        if n == 0 {
            return Ok(());
        }
        buffer.extend_from_slice(&chunk[..n]);
        if let Some(at) = buffer.windows(4).position(|w| w == b"\r\n\r\n") {
            break at + 4;
        }
        if buffer.len() > 64 * 1024 {
            return Ok(());
        }
    };
    let head = buffer.split_off(end);
    let text = String::from_utf8_lossy(&buffer).into_owned();
    let mut words = text.lines().next().unwrap_or("").split(' ');
    let (method, url) = (words.next().unwrap_or(""), words.next().unwrap_or(""));
    if method != "CONNECT" {
        log(log_file, &format!("{method} {url}"), false);
        client
            .write_all(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .await?;
        return client.shutdown().await;
    }
    let hosts: Vec<&str> = allowed.iter().map(String::as_str).collect();
    let ok = egress_allowed(url, &hosts);
    log(log_file, url, ok);
    if !ok {
        client.write_all(b"HTTP/1.1 403 Forbidden\r\n\r\n").await?;
        return client.shutdown().await;
    }
    let host = url.split(':').next().unwrap_or("");
    let mut upstream = tokio::net::TcpStream::connect((host, 443)).await?;
    client
        .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
        .await?;
    upstream.write_all(&head).await?;
    let _ = tokio::io::copy_bidirectional(&mut client, &mut upstream).await;
    Ok(())
}

/// What the Codex CLI's box is made of: the input of [`codex_command`],
/// the adapter `mcbench-providers`' `codex_cli::sandbox_codex_command`
/// calls with its `SandboxedCodex` (today's `containerArgs` and
/// `prepareRuntime` arguments).
#[derive(Clone, Debug)]
pub struct CodexRun {
    /// The root filesystem (the unpacked `rootfs/`, as
    /// [`crate::linux::rootfs::unpack`] answers it).
    pub rootfs: PathBuf,
    /// The root filesystem's environment (`image.json`'s `env`).
    pub image_env: Vec<String>,
    /// The static `mcbench-sandbox-agent`, bound read-only at
    /// `/run/mcbench-relay`: the network relay inside the box
    /// (`container-shim.py`'s part) and the tool bridge Codex's config names
    /// (`/run/mcbench-relay tool-bridge /run/mcbench/mcp.sock`, today
    /// `python3 /run/mcbench/mcp-bridge.py`).
    pub relay: PathBuf,
    /// The Codex install's folder, bound read-only at `/opt/codex`.
    pub install: PathBuf,
    /// The run's CODEX_HOME, bound read-write at `/codex-home`.
    pub home: PathBuf,
    /// The login file, bound read-write at `/codex-home/auth.json`.
    pub auth_file: PathBuf,
    /// The empty workspace, bound read-write at `/workspace`, the working
    /// folder.
    pub workspace: PathBuf,
    /// The run's socket folder, bound at `/run/mcbench`: `mcp.sock`, the
    /// prompt's images, and the proxy's `egress.sock`.
    pub sock_dir: PathBuf,
    /// The hosts the proxy admits on port 443.
    pub allowed_hosts: Vec<String>,
    /// The proxy's log (`<chat>/egress.ndjson`).
    pub proxy_log: PathBuf,
    /// The command inside, run in `/workspace`.
    pub command: Vec<String>,
}

/// What [`codex_command`] answers: the command to spawn (`systemd-run`
/// with the box's limits, then bubblewrap), the environment it needs from
/// the app's (the user session's bus; the box's own is set inside), and the
/// proxy's stop.
pub struct CodexCommand {
    /// The program (`systemd-run`).
    pub command: std::ffi::OsString,
    /// Its arguments.
    pub args: Vec<String>,
    /// The variables `systemd-run --user` needs, from the app's own
    /// environment (`PATH`, `XDG_RUNTIME_DIR`, `DBUS_SESSION_BUS_ADDRESS`).
    pub env: Vec<(String, String)>,
    /// Stops the proxy (`proxy.close`); open tunnels end with the run.
    pub close_proxy: Box<dyn FnOnce() -> Result<(), String> + Send>,
}

/// Starts the allow-list proxy on `<sock_dir>/egress.sock` (on a thread of
/// its own, so it runs whatever runtime the caller is on) and answers the
/// command line that runs Codex under bubblewrap with it as its only
/// network.
#[cfg(unix)]
pub fn codex_command(run: &CodexRun) -> Result<CodexCommand, String> {
    let socket = run.sock_dir.join("egress.sock");
    let (allowed, log) = (run.allowed_hosts.clone(), run.proxy_log.clone());
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let thread = std::thread::Builder::new()
        .name("egress-proxy".into())
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("a tokio runtime");
            runtime.block_on(async move {
                match start_egress_proxy(&socket, allowed, log).await {
                    Ok(proxy) => {
                        let _ = ready_tx.send(Ok(()));
                        let _ = stop_rx.await;
                        proxy.close().await;
                    }
                    Err(error) => {
                        let _ = ready_tx.send(Err(format!(
                            "the egress proxy on {}: {error}",
                            socket.display()
                        )));
                    }
                }
            });
        })
        .map_err(|error| error.to_string())?;
    ready_rx
        .recv()
        .map_err(|_| "the egress proxy's thread ended".to_string())??;
    let env = ["PATH", "XDG_RUNTIME_DIR", "DBUS_SESSION_BUS_ADDRESS"]
        .into_iter()
        .filter_map(|key| {
            std::env::var(key)
                .ok()
                .map(|value| (key.to_string(), value))
        })
        .collect();
    let args = codex_bwrap_args(run);
    Ok(CodexCommand {
        command: args[0].clone().into(),
        args: args[1..].to_vec(),
        env,
        close_proxy: Box::new(move || {
            let _ = stop_tx.send(());
            thread
                .join()
                .map_err(|_| "the egress proxy's thread panicked".to_string())
        }),
    })
}

/// The relay's port inside the box (`container-shim.py`'s).
pub const RELAY_PORT: u16 = 3128;

/// The command line that runs Codex as today's container did: a systemd
/// user scope with today's `--memory 4g --pids-limit 512 --cpus 4`, then
/// [`codex_box_args`] as the host's user. The scope is collected when it
/// ends (`--collect`), so one killed at its memory limit leaves no failed
/// unit in the person's session.
#[cfg(unix)]
pub fn codex_bwrap_args(spec: &CodexRun) -> Vec<String> {
    let mut args: Vec<String> = [
        "systemd-run",
        "--user",
        "--scope",
        "--quiet",
        "--collect",
        "-p",
        "MemoryMax=4G",
        "-p",
        "MemorySwapMax=4G",
        "-p",
        "TasksMax=512",
        "-p",
        "CPUQuota=400%",
        "--",
    ]
    .map(String::from)
    .into();
    args.extend(codex_box_args(spec, libc_ids()));
    args
}

/// The limits of Codex's box where no systemd sets them (the app's WSL
/// distro: `sandbox-agent codex ... box`), as the cgroup files that the
/// scope of [`codex_bwrap_args`] sets: `MemoryMax=4G`, `MemorySwapMax=4G`,
/// `TasksMax=512`, `CPUQuota=400%`.
pub const CODEX_LIMITS: [(&str, &str); 4] = [
    ("memory.max", "4294967296"),
    ("memory.swap.max", "4294967296"),
    ("pids.max", "512"),
    ("cpu.max", "400000 100000"),
];

/// Codex's box, the same on Linux and in the app's WSL distro: bubblewrap
/// with only loopback, every capability dropped, the user `ids`, and only
/// the listed mounts; the relay is its first program and its pid 1
/// (`--as-pid-1`: no init of bubblewrap's holds the app's pipes in the
/// box), and is not dumpable (`relay`, Linux's). In the distro the host does not
/// know the root filesystem's environment, and the agent puts it in where
/// the host's spec has it ([`insert_image_env`]).
pub fn codex_box_args(spec: &CodexRun, (uid, gid): (u32, u32)) -> Vec<String> {
    let path = |p: &Path| p.to_string_lossy().into_owned();
    let proxy = format!("http://127.0.0.1:{RELAY_PORT}");
    let mut args: Vec<String> = [
        "bwrap",
        "--unshare-user",
        "--uid",
        &uid.to_string(),
        "--gid",
        &gid.to_string(),
        "--disable-userns",
        "--unshare-pid",
        "--as-pid-1",
        "--unshare-ipc",
        "--unshare-uts",
        "--unshare-net",
        "--unshare-cgroup",
        "--die-with-parent",
        "--new-session",
        // The root filesystem under a writable layer in memory, as the
        // container had one (mount points for the mounts below included).
        "--cap-drop",
        "ALL",
        "--overlay-src",
        &path(&spec.rootfs),
        "--tmp-overlay",
        "/",
        "--proc",
        "/proc",
        "--dev",
        "/dev",
        "--perms",
        "1777",
        "--tmpfs",
        "/tmp",
        "--tmpfs",
        "/run",
        "--ro-bind",
        &path(&spec.install),
        "/opt/codex",
        "--bind",
        &path(&spec.home),
        "/codex-home",
        "--bind",
        &path(&spec.auth_file),
        "/codex-home/auth.json",
        "--bind",
        &path(&spec.workspace),
        "/workspace",
        "--bind",
        &path(&spec.sock_dir),
        "/run/mcbench",
        "--ro-bind",
        &path(&spec.relay),
        "/run/mcbench-relay",
        "--chdir",
        "/workspace",
        "--clearenv",
    ]
    .map(String::from)
    .into();
    let env = with_image_env(
        &[
            ("CODEX_HOME", "/codex-home"),
            ("HOME", "/codex-home"),
            ("HTTPS_PROXY", proxy.as_str()),
            ("HTTP_PROXY", proxy.as_str()),
            ("ALL_PROXY", proxy.as_str()),
            ("SSL_CERT_FILE", "/etc/ssl/certs/ca-certificates.crt"),
        ],
        &spec.image_env,
    );
    for (key, value) in env {
        args.extend(["--setenv".to_string(), key, value]);
    }
    args.extend(
        [
            "/run/mcbench-relay",
            "relay",
            &RELAY_PORT.to_string(),
            "/run/mcbench/egress.sock",
            "--",
        ]
        .map(String::from),
    );
    args.extend(spec.command.iter().cloned());
    args
}

/// The box's variables: the root filesystem's (`image.json`'s `env`,
/// `KEY=value`), then `ours`, in that order.
pub fn with_image_env(ours: &[(&str, &str)], image_env: &[String]) -> Vec<(String, String)> {
    let mut env: Vec<(String, String)> = image_env
        .iter()
        .filter_map(|e| e.split_once('='))
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    env.extend(ours.iter().map(|(k, v)| (k.to_string(), v.to_string())));
    env
}

/// The box's arguments with the root filesystem's variables put in where
/// [`codex_box_args`] puts them when the host knows them: right after
/// `--clearenv`, before the box's own.
pub fn insert_image_env(box_args: &[String], image_env: &[String]) -> Vec<String> {
    let at = box_args
        .iter()
        .position(|a| a == "--clearenv")
        .map_or(box_args.len(), |i| i + 1);
    let mut args = box_args[..at].to_vec();
    for (key, value) in with_image_env(&[], image_env) {
        args.extend(["--setenv".to_string(), key, value]);
    }
    args.extend(box_args[at..].iter().cloned());
    args
}

#[cfg(unix)]
fn libc_ids() -> (u32, u32) {
    // SAFETY: getuid and getgid cannot fail.
    unsafe { (libc::getuid(), libc::getgid()) }
}

/// `sandbox-agent relay <port> <socket> -- <command...>`
/// (`container-shim.py`): listens on `127.0.0.1:<port>`, relays each
/// connection to the Unix socket, runs the command, passes SIGTERM and
/// SIGINT on, and exits with its code. It is the box's pid 1
/// ([`codex_box_args`]): it reaps every process orphaned in the box while
/// the command runs, and exits when the command has, which ends the rest.
/// It holds the app's stdin, stdout and stderr, so it makes itself not
/// dumpable first: no process of the box can then open its
/// `/proc/<pid>/fd`.
#[cfg(target_os = "linux")]
pub fn relay(port: u16, socket: &Path, command: &[String]) -> ! {
    use std::io::{Read, Write};
    undumpable();
    let listener = std::net::TcpListener::bind(("127.0.0.1", port)).unwrap_or_else(|e| {
        eprintln!("relay: 127.0.0.1:{port}: {e}");
        std::process::exit(1)
    });
    let socket = socket.to_path_buf();
    std::thread::spawn(move || {
        for client in listener.incoming().flatten() {
            let Ok(upstream) = mcbench_relay::connect_unix(&socket) else {
                continue;
            };
            let pipe = |mut from: Box<dyn Read + Send>,
                        mut to: Box<dyn Write + Send>,
                        close: Box<dyn Fn() + Send>| {
                std::thread::spawn(move || {
                    let _ = std::io::copy(&mut from, &mut to);
                    close();
                });
            };
            let (c1, c2, u1, u2) = (
                client.try_clone(),
                client.try_clone(),
                upstream.try_clone(),
                upstream.try_clone(),
            );
            let (Ok(c1), Ok(c2), Ok(u1), Ok(u2)) = (c1, c2, u1, u2) else {
                continue;
            };
            pipe(
                Box::new(c1),
                Box::new(u1),
                Box::new(move || drop(u2.shutdown(std::net::Shutdown::Write))),
            );
            pipe(
                Box::new(upstream),
                Box::new(client),
                Box::new(move || drop(c2.shutdown(std::net::Shutdown::Write))),
            );
        }
    });
    let child = mcbench_spawn::command(&command[0])
        .args(&command[1..])
        .spawn()
        .unwrap_or_else(|e| {
            eprintln!("relay: {}: {e}", command[0]);
            std::process::exit(127)
        });
    let pid = child.id() as i32;
    static CHILD: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);
    CHILD.store(pid, std::sync::atomic::Ordering::SeqCst);
    extern "C" fn pass(signal: libc::c_int) {
        // SAFETY: kill is async-signal-safe; the pid is our child's.
        unsafe { libc::kill(CHILD.load(std::sync::atomic::Ordering::SeqCst), signal) };
    }
    // SAFETY: installing a handler that only calls kill.
    unsafe {
        libc::signal(libc::SIGTERM, pass as *const () as libc::sighandler_t);
        libc::signal(libc::SIGINT, pass as *const () as libc::sighandler_t);
    }
    // As pid 1: every child, the box's orphans included, is reaped here
    // until the command is.
    let status = loop {
        let mut status: libc::c_int = 0;
        // SAFETY: waitpid with a pointer to a status this frame owns.
        let reaped = unsafe { libc::waitpid(-1, &mut status, 0) };
        if reaped == pid {
            break status;
        }
        if reaped < 0 {
            let error = std::io::Error::last_os_error();
            if error.kind() != std::io::ErrorKind::Interrupted {
                eprintln!("relay: waiting for {}: {error}", command[0]);
                std::process::exit(1)
            }
        }
    };
    // container-shim.py's `sys.exit(child.wait())`: a child a signal
    // ended is `-signal`, which exits as `256 - signal` (247 for SIGKILL).
    std::process::exit(if libc::WIFEXITED(status) {
        libc::WEXITSTATUS(status)
    } else {
        (-libc::WTERMSIG(status)) & 0xff
    })
}

/// Makes this process not dumpable (`prctl(PR_SET_DUMPABLE, 0)`): a
/// process of its user can then not open its `/proc/<pid>/fd`, `mem` or
/// `environ`. It holds until this process execs another program, so each
/// program of the box's that holds the app's pipes calls it itself (the
/// relay, the tool bridge); Codex's programs are installed execute-only,
/// which the kernel makes undumpable at exec ([`execute_only`]).
#[cfg(target_os = "linux")]
pub fn undumpable() {
    // SAFETY: prctl with PR_SET_DUMPABLE and 0 changes only this process's
    // dumpable flag and reads no memory.
    if unsafe { libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0) } != 0 {
        eprintln!(
            "prctl(PR_SET_DUMPABLE, 0): {}",
            std::io::Error::last_os_error()
        );
        std::process::exit(1)
    }
}

/// Gives every program under `folder` (a Codex package's) the mode 0111:
/// executable, not readable. The kernel makes a process whose program its
/// user cannot read not dumpable at exec, so no process of the box (Codex's
/// own commands included) can open Codex's `/proc/<pid>/fd`, where its
/// stdin, stdout and stderr are the app's pipes. A program is what its ELF
/// header says one is ([`is_program`]); the package's shared libraries and
/// data files keep their modes, since they are read.
#[cfg(unix)]
pub fn execute_only(folder: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let mut stack = vec![folder.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = std::fs::read_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        for entry in entries {
            let entry = entry.map_err(|e| format!("{}: {e}", dir.display()))?;
            let path = entry.path();
            let meta =
                std::fs::symlink_metadata(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            if meta.is_dir() {
                stack.push(path);
            } else if meta.is_file()
                && meta.permissions().mode() & 0o111 != 0
                && meta.permissions().mode() & 0o7777 != 0o111
                && is_program(&path).map_err(|e| format!("{}: {e}", path.display()))?
            {
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o111))
                    .map_err(|e| format!("chmod 0111 {}: {e}", path.display()))?;
            }
        }
    }
    Ok(())
}

/// Whether `path` is an ELF program rather than a library: its type is
/// `ET_EXEC`, or `ET_DYN` with the dynamic flag `DF_1_PIE` (a
/// position-independent program, the static-pie programs of Codex's Linux
/// packages among them; a shared library has no such flag). Anything that
/// is not a 64-bit little-endian ELF file is not one.
pub fn is_program(path: &Path) -> std::io::Result<bool> {
    use std::io::{Read, Seek, SeekFrom};
    const ET_EXEC: u16 = 2;
    const ET_DYN: u16 = 3;
    const PT_DYNAMIC: u32 = 2;
    const DT_NULL: u64 = 0;
    const DT_FLAGS_1: u64 = 0x6fff_fffb;
    const DF_1_PIE: u64 = 0x0800_0000;
    let mut file = std::fs::File::open(path)?;
    let mut header = [0u8; 64];
    match file.read_exact(&mut header) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(false),
        Err(e) => return Err(e),
    }
    // ELFCLASS64, ELFDATA2LSB.
    if &header[..4] != b"\x7fELF" || header[4] != 2 || header[5] != 1 {
        return Ok(false);
    }
    let u16_at = |b: &[u8], at: usize| u16::from_le_bytes([b[at], b[at + 1]]);
    let u32_at = |b: &[u8], at: usize| u32::from_le_bytes(b[at..at + 4].try_into().unwrap());
    let u64_at = |b: &[u8], at: usize| u64::from_le_bytes(b[at..at + 8].try_into().unwrap());
    match u16_at(&header, 16) {
        ET_EXEC => return Ok(true),
        ET_DYN => {}
        _ => return Ok(false),
    }
    let (phoff, phentsize, phnum) = (
        u64_at(&header, 32),
        u16_at(&header, 54) as usize,
        u16_at(&header, 56) as usize,
    );
    let mut table = vec![0u8; phentsize * phnum];
    file.seek(SeekFrom::Start(phoff))?;
    file.read_exact(&mut table)?;
    for entry in table.chunks_exact(phentsize) {
        if u32_at(entry, 0) != PT_DYNAMIC {
            continue;
        }
        let (offset, size) = (u64_at(entry, 8), u64_at(entry, 32) as usize);
        let mut dynamic = vec![0u8; size];
        file.seek(SeekFrom::Start(offset))?;
        file.read_exact(&mut dynamic)?;
        for tag in dynamic.as_chunks::<16>().0 {
            match u64_at(&tag[..], 0) {
                DT_NULL => break,
                DT_FLAGS_1 => return Ok(u64_at(&tag[..], 8) & DF_1_PIE != 0),
                _ => {}
            }
        }
    }
    Ok(false)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    // The benchmark's batch tests: "the egress proxy admits only
    // allowlisted hosts on port 443 and logs every request"
    #[tokio::test]
    async fn the_egress_proxy_admits_only_allowlisted_hosts_on_port_443_and_logs_every_request() {
        assert!(egress_allowed("chatgpt.com:443", &["chatgpt.com"]));
        assert!(!egress_allowed("chatgpt.com:80", &["chatgpt.com"]));
        assert!(!egress_allowed("example.com:443", &["chatgpt.com"]));
        assert!(!egress_allowed(
            "chatgpt.com.evil.test:443",
            &["chatgpt.com"]
        ));
        assert!(egress_allowed("ChatGPT.com:443", &["chatgpt.com"]));
        assert!(!egress_allowed("chat gpt.com:443", &["chatgpt.com"]));
        // Where a run's socket folder is made, and reached as a run
        // reaches it.
        let dir = mcbench_relay::socket_base()
            .unwrap()
            .join(format!("mcbench-egress-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let socket = dir.join("egress.sock");
        let log_file = dir.join("egress.ndjson");
        let proxy = start_egress_proxy(&socket, vec!["chatgpt.com".into()], log_file.clone())
            .await
            .unwrap();
        let client = mcbench_relay::connect_unix(&socket).unwrap();
        client.set_nonblocking(true).unwrap();
        let mut client = tokio::net::UnixStream::from_std(client).unwrap();
        client
            .write_all(b"CONNECT example.com:443 HTTP/1.1\r\nHost: example.com:443\r\n\r\n")
            .await
            .unwrap();
        let mut reply = String::new();
        client.read_to_string(&mut reply).await.unwrap();
        proxy.close().await;
        assert!(reply.starts_with("HTTP/1.1 403"), "{reply}");
        let lines: Vec<(String, bool)> = std::fs::read_to_string(&log_file)
            .unwrap()
            .lines()
            .map(|line| {
                let entry: serde_json::Value = serde_json::from_str(line).unwrap();
                (
                    entry["target"].as_str().unwrap().to_string(),
                    entry["allowed"].as_bool().unwrap(),
                )
            })
            .collect();
        assert_eq!(lines, [("example.com:443".to_string(), false)]);
        let first: serde_json::Map<String, serde_json::Value> = serde_json::from_str(
            std::fs::read_to_string(&log_file)
                .unwrap()
                .lines()
                .next()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            first.keys().collect::<Vec<_>>(),
            ["at", "target", "allowed"]
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn iso_dates_are_javascripts() {
        let now = iso_now();
        assert_eq!(now.len(), 24);
        assert!(now.ends_with('Z') && now.as_bytes()[10] == b'T');
    }
}
