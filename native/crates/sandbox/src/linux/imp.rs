//! [`super::Bubblewrap`] on Linux: the app's side of the keepers.

use std::collections::HashMap;
use std::os::fd::{AsRawFd, OwnedFd, RawFd};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use tokio::io::unix::AsyncFd;
use tokio::net::unix::pipe;
use tokio::sync::{oneshot, watch};
use tokio_util::sync::CancellationToken;

use super::control::{self, Event, Request};
use super::keeper::{Network, Rules, Spec};
use super::rootfs::{self, Unpacked};
use super::{
    BWRAP_MISSING, Bubblewrap, Delegation, EGRESS_RULES, NETWORK, SESSION_MISSING, USERNS_MISSING,
};
use crate::backend::{self, OneShotRun, ProgramRun, SessionEnd, SessionRun};
use crate::{SandboxError, Workspace, work};

/// What `prepare` found.
#[derive(Clone)]
struct Prepared {
    bwrap: PathBuf,
    /// `systemd-run` for [`Delegation::UserScope`].
    systemd_run: Option<PathBuf>,
    unpacked: Unpacked,
}

/// One box made at a time per (chat, networked).
type MakeLock = Arc<tokio::sync::Mutex<()>>;

/// The boxes and the persistent shells.
#[derive(Default)]
pub struct State {
    prepared: tokio::sync::OnceCell<Prepared>,
    boxes: Mutex<HashMap<(String, bool), Arc<BoxHandle>>>,
    making: Mutex<HashMap<(String, bool), MakeLock>>,
    sessions: Mutex<HashMap<String, Arc<Session>>>,
}

/// A running box: its keeper and the control socket.
struct BoxHandle {
    work: PathBuf,
    control: Arc<Control>,
    keeper: tokio::sync::Mutex<tokio::process::Child>,
    /// The keeper's cgroup folder made in a [`Delegation::Subtree`],
    /// removed after the keeper ends (systemd removes a scope itself).
    cgroup: Option<PathBuf>,
}

/// A box ends when the app's side goes: the keeper ends it (SIGKILL to its
/// init, which ends its PID namespace) when its control socket reaches the
/// end, which is this shutdown on a drop, or the kernel's close of the
/// app's descriptors when the app dies. Bubblewrap's `--die-with-parent`
/// does not do it where an AppArmor profile for bwrap moves the box's init
/// into `unpriv_bwrap`: the init's parent-death signal is cleared there.
impl Drop for BoxHandle {
    fn drop(&mut self) {
        self.control.close();
    }
}

/// The app's end of a keeper's control socket.
struct Control {
    socket: AsyncFd<OwnedFd>,
    next: AtomicU64,
    exits: Mutex<HashMap<u64, watch::Sender<Option<i32>>>>,
    ends: Mutex<HashMap<u64, oneshot::Sender<()>>>,
    first: Mutex<Option<oneshot::Sender<Event>>>,
    closed: watch::Sender<bool>,
}

impl Control {
    fn send(&self, request: &Request, fds: &[RawFd]) -> Result<(), SandboxError> {
        let data = serde_json::to_vec(request).expect("a request serialises");
        if data.len() > control::MAX_MESSAGE {
            return Err(SandboxError::Failed(
                "Docker sandbox exec failed (126):\nargument list too long".to_string(),
            ));
        }
        // A packet this small fits the socket's buffer; a full buffer is a
        // keeper that stopped reading, which the next wait reports.
        control::send(self.socket.get_ref().as_raw_fd(), &data, fds, false)
            .map_err(|error| SandboxError::Failed(format!("the sandbox's keeper is gone: {error}")))
    }

    async fn read_events(self: Arc<Self>) {
        loop {
            let Ok(mut guard) = self.socket.readable().await else {
                break;
            };
            match guard.try_io(|fd| control::receive(fd.get_ref().as_raw_fd(), true)) {
                Err(_would_block) => continue,
                Ok(Ok(Some((data, _)))) => {
                    let Ok(event) = serde_json::from_slice::<Event>(&data) else {
                        break;
                    };
                    self.dispatch(event);
                }
                Ok(_) => break,
            }
        }
        let _ = self.closed.send(true);
        for (_, exit) in self.exits.lock().expect("exits").drain() {
            let _ = exit.send(Some(-1));
        }
        self.ends.lock().expect("ends").clear();
        self.first.lock().expect("first").take();
    }

    fn dispatch(&self, event: Event) {
        match event {
            Event::Ready { .. } | Event::Failed { .. } => {
                if let Some(first) = self.first.lock().expect("first").take() {
                    let _ = first.send(event);
                }
            }
            Event::Exited { id, code } => {
                if let Some(exit) = self.exits.lock().expect("exits").remove(&id) {
                    let _ = exit.send(Some(code));
                }
            }
            Event::SpawnFailed { id, .. } => {
                if let Some(exit) = self.exits.lock().expect("exits").remove(&id) {
                    let _ = exit.send(Some(127));
                }
            }
            Event::Ended { id } => {
                if let Some(end) = self.ends.lock().expect("ends").remove(&id) {
                    let _ = end.send(());
                }
            }
        }
    }

    /// A program entered into the box with these stdin, stdout and stderr;
    /// answers its id and its exit code's watch (`None` while it runs).
    fn spawn(
        &self,
        argv: Vec<String>,
        stdio: [&OwnedFd; 3],
    ) -> Result<(u64, watch::Receiver<Option<i32>>), SandboxError> {
        let id = self.next.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = watch::channel(None);
        if *self.closed.borrow() {
            let _ = tx.send(Some(-1));
            return Ok((id, rx));
        }
        self.exits.lock().expect("exits").insert(id, tx);
        self.send(
            &Request::Spawn { id, argv },
            &stdio.map(|fd| fd.as_raw_fd()),
        )?;
        Ok((id, rx))
    }

    fn kill(&self, id: u64) {
        let _ = self.send(&Request::Kill { id }, &[]);
    }

    async fn end(&self) {
        let id = self.next.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();
        self.ends.lock().expect("ends").insert(id, tx);
        if self.send(&Request::End { id }, &[]).is_ok() {
            let _ = rx.await;
        }
    }

    fn close(&self) {
        // SAFETY: shutdown of a socket we hold; the keeper sees the end.
        unsafe { libc::shutdown(self.socket.get_ref().as_raw_fd(), libc::SHUT_RDWR) };
    }
}

/// A chat's persistent shell.
struct Session {
    id: u64,
    control: Arc<Control>,
    stdin: tokio::sync::Mutex<pipe::Sender>,
    output: tokio::sync::Mutex<pipe::Receiver>,
    exited: watch::Receiver<Option<i32>>,
}

fn sender(fd: OwnedFd) -> pipe::Sender {
    control::set_nonblocking(&fd).expect("O_NONBLOCK on a pipe");
    pipe::Sender::from_owned_fd(fd).expect("a pipe's write end")
}

fn receiver(fd: OwnedFd) -> pipe::Receiver {
    control::set_nonblocking(&fd).expect("O_NONBLOCK on a pipe");
    pipe::Receiver::from_owned_fd(fd).expect("a pipe's read end")
}

fn dev_null() -> OwnedFd {
    std::fs::File::open("/dev/null").expect("/dev/null").into()
}

fn failed(error: impl std::fmt::Display) -> SandboxError {
    SandboxError::Failed(error.to_string())
}

fn random_hex(len: usize) -> String {
    let mut out = String::new();
    while out.len() < len {
        out.push_str(backend::sentinel_token().trim_start_matches("__MCB_"));
    }
    out.truncate(len);
    out
}

impl Bubblewrap {
    async fn prepared(&self) -> Result<Prepared, SandboxError> {
        self.state
            .prepared
            .get_or_try_init(|| self.find())
            .await
            .cloned()
    }

    /// Finds bubblewrap (and systemd-run for a user scope), unpacks the
    /// root filesystem, and tries a user namespace and the delegation.
    async fn find(&self) -> Result<Prepared, SandboxError> {
        let path = std::env::var_os("PATH").unwrap_or_default();
        let bwrap = super::find_program("bwrap", &path)
            .ok_or_else(|| SandboxError::Unavailable(BWRAP_MISSING.to_string()))?;
        let systemd_run = match &self.config.delegation {
            Delegation::UserScope => Some(
                super::find_program("systemd-run", &path)
                    .ok_or_else(|| SandboxError::Unavailable(SESSION_MISSING.to_string()))?,
            ),
            Delegation::Subtree(dir) => {
                delegated_controllers(dir).map_err(SandboxError::Failed)?;
                None
            }
        };
        let (tarball, sha256) = self.config.rootfs.clone();
        let root = self.config.root.clone();
        let unpacked =
            tokio::task::spawn_blocking(move || rootfs::unpack(&tarball, &sha256, &root))
                .await
                .expect("the unpack task")
                .map_err(SandboxError::Failed)?;
        let probe = tokio::process::Command::from(mcbench_spawn::command(&bwrap))
            .args([
                "--unshare-user",
                "--unshare-pid",
                "--die-with-parent",
                "--ro-bind",
            ])
            .arg(&unpacked.rootfs)
            .args(["/", "--proc", "/proc", "--dev", "/dev", "/usr/bin/true"])
            .stdin(Stdio::null())
            .output()
            .await
            .map_err(failed)?;
        if !probe.status.success() {
            let stderr = String::from_utf8_lossy(&probe.stderr);
            return Err(match super::classify_bwrap_failure(&stderr) {
                Some(message) => SandboxError::Unavailable(message.to_string()),
                None => SandboxError::Failed(format!("bubblewrap failed: {}", stderr.trim_end())),
            });
        }
        if let Some(systemd_run) = &systemd_run
            && !probe_scope(systemd_run, &bwrap).await
        {
            return Err(SandboxError::Unavailable(SESSION_MISSING.to_string()));
        }
        let _ = USERNS_MISSING;
        Ok(Prepared {
            bwrap,
            systemd_run,
            unpacked,
        })
    }

    fn spec(
        &self,
        prepared: &Prepared,
        workspace: &Workspace,
        network: bool,
        run: &Path,
    ) -> Result<Spec, SandboxError> {
        let hostname = random_hex(12);
        // SAFETY: getuid and getgid cannot fail.
        let (uid, gid) = unsafe { (libc::getuid(), libc::getgid()) };
        let hosts = "127.0.0.1\tlocalhost\n::1\tlocalhost ip6-localhost ip6-loopback\nfe00::\tip6-localnet\nff00::\tip6-mcastprefix\nff02::1\tip6-allnodes\nff02::2\tip6-allrouters\n";
        let address = NETWORK[0].1;
        let dns = NETWORK[2].1;
        let files = [
            (
                "hosts",
                if network {
                    format!("{hosts}{address}\t{hostname}\n")
                } else {
                    hosts.to_string()
                },
            ),
            ("hostname", format!("{hostname}\n")),
            (
                "resolv.conf",
                if network {
                    format!("nameserver {dns}\n")
                } else {
                    String::new()
                },
            ),
        ];
        for (name, text) in &files {
            std::fs::write(run.join(name), text).map_err(failed)?;
        }
        let rootfs = prepared.unpacked.rootfs.to_string_lossy().into_owned();
        let run_file = |name: &str| run.join(name).to_string_lossy().into_owned();
        let image_path = prepared
            .unpacked
            .image
            .env
            .iter()
            .find_map(|e| e.strip_prefix("PATH="))
            .unwrap_or("/usr/bin:/bin")
            .to_string();
        let mut box_args: Vec<String> = [
            "--unshare-user",
            "--uid",
            &uid.to_string(),
            "--gid",
            &gid.to_string(),
            "--disable-userns",
            "--unshare-pid",
            "--unshare-ipc",
            "--unshare-uts",
            "--unshare-cgroup",
        ]
        .map(String::from)
        .into();
        if !network {
            box_args.push("--unshare-net".into());
        }
        box_args.extend(
            [
                "--die-with-parent",
                "--hostname",
                &hostname,
                "--cap-drop",
                "ALL",
                "--ro-bind",
                &rootfs,
                "/",
                "--proc",
                "/proc",
                "--dev",
                "/dev",
                "--ro-bind",
                "/sys",
                "/sys",
                "--perms",
                "1777",
                "--size",
                "67108864",
                "--tmpfs",
                "/dev/shm",
                "--perms",
                "1777",
                "--size",
                "268435456",
                "--tmpfs",
                "/tmp",
                "--bind",
                &workspace.host.to_string_lossy(),
                "/work",
                "--ro-bind",
                &run_file("hosts"),
                "/etc/hosts",
                "--ro-bind",
                &run_file("hostname"),
                "/etc/hostname",
                "--ro-bind",
                &run_file("resolv.conf"),
                "/etc/resolv.conf",
                "--chdir",
                "/work",
                "--clearenv",
                "--as-pid-1",
                // The box's init: `sleep infinity`, its stdin, stdout and
                // stderr /dev/null. bubblewrap starts it with the keeper's
                // `box.log` as stdout and stderr (for bubblewrap's own
                // startup errors), and a process in the box could reopen
                // that host file through `/proc/1/fd` and write to it past
                // the storage budget; the shell lets go of it and execs.
                "/bin/sh",
                "-c",
                "exec sleep infinity </dev/null >/dev/null 2>&1",
            ]
            .map(String::from),
        );
        let network = network.then(|| Network {
            pasta: [
                &prepared.unpacked.pasta.to_string_lossy(),
                "--quiet",
                "--config-net",
                "-4",
                "-I",
                "eth0",
                "-a",
                address,
                "-n",
                "24",
                "-g",
                NETWORK[1].1,
                "--dns-forward",
                dns,
                "--map-host-loopback",
                "none",
                "-t",
                "none",
                "-u",
                "none",
                "-T",
                "none",
                "-U",
                "none",
                "--no-dhcp",
            ]
            .map(String::from)
            .into(),
            rules: Rules {
                rootfs: rootfs.clone(),
                resolv: run_file("resolv.conf"),
                path: image_path.clone(),
                script: EGRESS_RULES.to_string(),
            },
            sysctls: super::ipv6_off(),
        });
        let mut env = prepared.unpacked.image.env.clone();
        env.push(format!("HOSTNAME={hostname}"));
        env.push("HOME=/".to_string());
        Ok(Spec {
            bwrap: prepared.bwrap.clone(),
            box_args,
            network,
            helper: self
                .config
                .helper
                .iter()
                .map(|a| a.to_string_lossy().into_owned())
                .collect(),
            run: run.to_path_buf(),
            env,
            binfmt: matches!(self.config.delegation, Delegation::Subtree(_)),
        })
    }

    /// Starts a box: its keeper in a user scope, the box, and the reader of
    /// its events.
    async fn make(
        &self,
        prepared: &Prepared,
        workspace: &Workspace,
        network: bool,
    ) -> Result<Arc<BoxHandle>, SandboxError> {
        work::work_dir(workspace).map_err(SandboxError::Failed)?;
        let run = self.config.root.join(".run").join(random_hex(16));
        std::fs::create_dir_all(&run).map_err(failed)?;
        let spec = self.spec(prepared, workspace, network, &run)?;
        let (ours, theirs) = control::pair().map_err(failed)?;
        let (mut command, cgroup) = match (&self.config.delegation, &prepared.systemd_run) {
            (Delegation::UserScope, Some(systemd_run)) => {
                let mut command =
                    tokio::process::Command::from(mcbench_spawn::command(systemd_run));
                command
                    .args([
                        "--user",
                        "--scope",
                        "--quiet",
                        // A scope that ended failed (a command killed for
                        // memory) is not kept in the person's session.
                        "--collect",
                        "-p",
                        "Delegate=yes",
                        "-p",
                        "OOMPolicy=continue",
                        "--",
                    ])
                    .args(&self.config.helper);
                (command, None)
            }
            (Delegation::Subtree(dir), None) => {
                let cgroup = dir.join(random_hex(16));
                std::fs::create_dir(&cgroup)
                    .map_err(|e| failed(format!("making {}: {e}", cgroup.display())))?;
                let procs: OwnedFd = std::fs::OpenOptions::new()
                    .write(true)
                    .open(cgroup.join("cgroup.procs"))
                    .map_err(|e| failed(format!("{}/cgroup.procs: {e}", cgroup.display())))?
                    .into();
                let mut command =
                    tokio::process::Command::from(mcbench_spawn::command(&self.config.helper[0]));
                command.args(&self.config.helper[1..]);
                let fd = procs.as_raw_fd();
                // SAFETY: pre_exec in the forked child: write(2) of a short
                // buffer to a descriptor opened before the fork is
                // async-signal-safe. "0" moves the writer.
                unsafe {
                    command.pre_exec(move || {
                        if libc::write(fd, b"0".as_ptr() as *const _, 1) != 1 {
                            return Err(std::io::Error::last_os_error());
                        }
                        Ok(())
                    });
                }
                // `procs` stays open until the spawn below has forked.
                (command, Some((cgroup, procs)))
            }
            _ => unreachable!("prepare finds systemd-run exactly for a user scope"),
        };
        command
            .arg("keep")
            .stdin(Stdio::from(theirs))
            .stdout(Stdio::null())
            // Not killed on drop: the keeper ends its box when the control
            // socket closes (BoxHandle's drop), and a SIGKILL first would
            // leave the box's init behind under unpriv_bwrap.
            .stderr(Stdio::piped());
        let spawned = command.spawn();
        let cgroup = cgroup.map(|(dir, procs)| {
            drop(procs);
            dir
        });
        let mut keeper = match spawned {
            Ok(keeper) => keeper,
            Err(error) => {
                if let Some(dir) = &cgroup {
                    remove_cgroup(dir).await;
                }
                return Err(failed(error));
            }
        };
        let mut stderr = keeper.stderr.take().expect("piped stderr");
        let (closed, _) = watch::channel(false);
        control::set_nonblocking(&ours).map_err(failed)?;
        let (first_tx, first_rx) = oneshot::channel();
        let control = Arc::new(Control {
            // SAFETY: the OwnedFd owns the descriptor and keeps it open until
            // the AsyncFd drops it.
            socket: unsafe { AsyncFd::register(ours) }.map_err(failed)?,
            next: AtomicU64::new(1),
            exits: Mutex::new(HashMap::new()),
            ends: Mutex::new(HashMap::new()),
            first: Mutex::new(Some(first_tx)),
            closed,
        });
        tokio::spawn(control.clone().read_events());
        control.send(&Request::Start(Box::new(spec)), &[])?;
        let started = first_rx.await;
        match started {
            Ok(Event::Ready { .. }) => {}
            other => {
                let _ = keeper.wait().await;
                let mut text = String::new();
                use tokio::io::AsyncReadExt;
                let _ = stderr.read_to_string(&mut text).await;
                let message = match other {
                    Ok(Event::Failed { message }) => message,
                    _ => text.trim_end().to_string(),
                };
                let _ = std::fs::remove_dir_all(&run);
                if let Some(dir) = &cgroup {
                    remove_cgroup(dir).await;
                }
                return Err(SandboxError::Failed(format!(
                    "The bash sandbox's box did not start: {message}"
                )));
            }
        }
        // The keeper's later words (none, normally) are not kept.
        tokio::spawn(async move {
            use tokio::io::AsyncReadExt;
            let mut sink = Vec::new();
            let _ = stderr.read_to_end(&mut sink).await;
        });
        Ok(Arc::new(BoxHandle {
            work: workspace.host.clone(),
            control,
            keeper: tokio::sync::Mutex::new(keeper),
            cgroup,
        }))
    }

    fn live_box(&self, chat: &str, network: bool) -> Option<Arc<BoxHandle>> {
        let boxes = self.state.boxes.lock().expect("boxes");
        boxes
            .get(&(chat.to_string(), network))
            .filter(|b| !*b.control.closed.borrow())
            .cloned()
    }

    async fn take_box(&self, chat: &str, network: bool) {
        let taken = self
            .state
            .boxes
            .lock()
            .expect("boxes")
            .remove(&(chat.to_string(), network));
        if let Some(handle) = taken {
            handle.control.close();
            let mut keeper = handle.keeper.lock().await;
            if tokio::time::timeout(Duration::from_secs(10), keeper.wait())
                .await
                .is_err()
            {
                let _ = keeper.kill().await;
            }
            if let Some(dir) = &handle.cgroup {
                remove_cgroup(dir).await;
            }
        }
    }

    fn session_of(&self, chat: &str) -> Option<Arc<Session>> {
        self.state
            .sessions
            .lock()
            .expect("sessions")
            .get(chat)
            .filter(|s| s.exited.borrow().is_none())
            .cloned()
    }

    fn start_session(&self, chat: &str, handle: &BoxHandle) -> Result<Arc<Session>, SandboxError> {
        let (stdin_read, stdin_write) = control::pipe().map_err(failed)?;
        let (out_read, out_write) = control::pipe().map_err(failed)?;
        let (id, exited) = handle.control.spawn(
            vec!["bash".to_string()],
            [&stdin_read, &out_write, &out_write],
        )?;
        drop((stdin_read, out_write));
        let session = Arc::new(Session {
            id,
            control: handle.control.clone(),
            stdin: tokio::sync::Mutex::new(sender(stdin_write)),
            output: tokio::sync::Mutex::new(receiver(out_read)),
            exited,
        });
        self.state
            .sessions
            .lock()
            .expect("sessions")
            .insert(chat.to_string(), session.clone());
        Ok(session)
    }

    async fn run_program(
        &self,
        handle: &BoxHandle,
        argv: Vec<String>,
        separate: bool,
        timeout: Duration,
        tap: backend::Tap,
        cancel: Option<&CancellationToken>,
    ) -> Result<(Vec<u8>, Vec<u8>, i32, bool), SandboxError> {
        let null = dev_null();
        let (out_read, out_write) = control::pipe().map_err(failed)?;
        let (err_read, err_write) = if separate {
            let (r, w) = control::pipe().map_err(failed)?;
            (Some(r), Some(w))
        } else {
            (None, None)
        };
        let err_fd = err_write.as_ref().unwrap_or(&out_write);
        let (id, exited) = handle.control.spawn(argv, [&null, &out_write, err_fd])?;
        drop((null, out_write, err_write));
        let all = Arc::new(Mutex::new(Vec::new()));
        let killed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let control = handle.control.clone();
        let kill = {
            let control = control.clone();
            let killed = killed.clone();
            move || {
                killed.store(true, Ordering::SeqCst);
                control.kill(id);
            }
        };
        let collect_stdout = {
            let all = all.clone();
            let kill = kill.clone();
            async move {
                let mut out = receiver(out_read);
                if separate {
                    let mut stdout = Vec::new();
                    let mut buffer = vec![0u8; 64 * 1024];
                    loop {
                        use tokio::io::AsyncReadExt;
                        match out.read(&mut buffer).await {
                            Ok(0) | Err(_) => break,
                            Ok(n) => {
                                stdout.extend_from_slice(&buffer[..n]);
                                all.lock().expect("all").extend_from_slice(&buffer[..n]);
                            }
                        }
                    }
                    stdout
                } else {
                    let (text, _) = backend::collect_one_shot(&mut out, &tap, kill).await;
                    text.into_bytes()
                }
            }
        };
        let collect_stderr = {
            let all = all.clone();
            async move {
                let Some(err_read) = err_read else { return };
                let mut err = receiver(err_read);
                let mut buffer = vec![0u8; 64 * 1024];
                loop {
                    use tokio::io::AsyncReadExt;
                    match err.read(&mut buffer).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => all.lock().expect("all").extend_from_slice(&buffer[..n]),
                    }
                }
            }
        };
        let mut waiting = exited.clone();
        let ended = async move {
            let _ = waiting.wait_for(Option::is_some).await;
        };
        let work = async { tokio::join!(collect_stdout, collect_stderr, ended).0 };
        tokio::pin!(work);
        let deadline = tokio::time::sleep(timeout);
        tokio::pin!(deadline);
        let mut cancelled = false;
        let stdout = loop {
            tokio::select! {
                stdout = &mut work => break stdout,
                () = &mut deadline, if !killed.load(Ordering::SeqCst) => kill(),
                () = async { match cancel { Some(token) => token.cancelled().await, None => std::future::pending().await } }, if !cancelled => {
                    cancelled = true;
                    // Today a cancel ends the box's processes (oneShot).
                    control.end().await;
                }
            }
        };
        let code = exited.borrow().expect("ended");
        let killed = killed.load(Ordering::SeqCst);
        let all = std::mem::take(&mut *all.lock().expect("all"));
        Ok((stdout, all, if killed { -1 } else { code }, killed))
    }
}

/// The delegated folder has the controllers the box's limits need.
fn delegated_controllers(dir: &Path) -> Result<(), String> {
    let file = dir.join("cgroup.subtree_control");
    let text = std::fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?;
    let enabled: Vec<&str> = text.split_whitespace().collect();
    for controller in ["memory", "pids", "cpu"] {
        if !enabled.contains(&controller) {
            return Err(format!(
                "The bash sandbox's cgroup {} does not delegate the {controller} controller (it has: {})",
                dir.display(),
                text.trim()
            ));
        }
    }
    Ok(())
}

/// Removes a keeper's cgroup folder of a [`Delegation::Subtree`] (its
/// `keeper` and `box` children first). A cgroup can be removed once its
/// last process has exited, which follows the keeper's own exit by a
/// moment; after a second of tries it is left (empty).
async fn remove_cgroup(dir: &Path) {
    for _ in 0..100 {
        for child in ["box", "keeper"] {
            let _ = std::fs::remove_dir(dir.join(child));
        }
        if std::fs::remove_dir(dir).is_ok() || !dir.exists() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// `systemd-run --user --scope` can start a scope with a delegated cgroup.
async fn probe_scope(systemd_run: &Path, program: &Path) -> bool {
    tokio::process::Command::from(mcbench_spawn::command(systemd_run))
        .args([
            "--user",
            "--scope",
            "--quiet",
            "--collect",
            "-p",
            "Delegate=yes",
            "--",
        ])
        .arg(program)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
        .is_ok_and(|status| status.success())
}

/// The probe of the user session, for the test that runs it without one.
pub async fn user_scope_works(systemd_run: &Path, program: &Path, env: &[(&str, &str)]) -> bool {
    tokio::process::Command::from(mcbench_spawn::command(systemd_run))
        .args([
            "--user",
            "--scope",
            "--quiet",
            "--collect",
            "-p",
            "Delegate=yes",
            "--",
        ])
        .arg(program)
        .arg("--version")
        .env_clear()
        .envs(env.iter().copied())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
        .is_ok_and(|status| status.success())
}

#[async_trait]
impl crate::Backend for Bubblewrap {
    async fn prepare(&self) -> Result<(), SandboxError> {
        self.prepared().await.map(|_| ())
    }

    async fn ensure(&self, workspace: &Workspace, network: bool) -> Result<(), SandboxError> {
        let prepared = self.prepared().await?;
        let key = (workspace.chat.clone(), network);
        let lock = self
            .state
            .making
            .lock()
            .expect("making")
            .entry(key.clone())
            .or_default()
            .clone();
        let _one = lock.lock().await;
        if let Some(handle) = self.live_box(&workspace.chat, network) {
            if handle.work == workspace.host {
                return Ok(());
            }
            // A recovered or forked chat with another /work keeps its id:
            // its boxes are made again (today's mount check).
            self.kill_session(&workspace.chat).await;
            self.remove(&workspace.chat).await;
        }
        self.take_box(&workspace.chat, network).await;
        let handle = self.make(&prepared, workspace, network).await?;
        self.state.boxes.lock().expect("boxes").insert(key, handle);
        Ok(())
    }

    async fn has_session(&self, workspace: &Workspace) -> bool {
        self.session_of(&workspace.chat).is_some()
            && self
                .live_box(&workspace.chat, false)
                .is_some_and(|b| b.work == workspace.host)
    }

    async fn session(
        &self,
        workspace: &Workspace,
        command: &str,
        ms: f64,
        tap: backend::Tap,
        cancel: &CancellationToken,
    ) -> Result<SessionRun, SandboxError> {
        let session = match self.session_of(&workspace.chat) {
            Some(session) => session,
            None => {
                let handle = self.live_box(&workspace.chat, false).ok_or_else(|| {
                    SandboxError::Failed(crate::transcript::SHELL_EXITED.to_string())
                })?;
                self.start_session(&workspace.chat, &handle)?
            }
        };
        let mut stdin = session.stdin.lock().await;
        let mut output = session.output.lock().await;
        let mut exited = session.exited.clone();
        let ended = async move {
            let _ = exited.wait_for(Option::is_some).await;
        };
        match backend::run_in_session(&mut *stdin, &mut *output, ended, command, ms, &tap, cancel)
            .await
        {
            Ok(run) => Ok(run),
            Err(SessionEnd::Cancelled) => Err(SandboxError::Cancelled),
            Err(SessionEnd::Failed(message)) => Err(SandboxError::Failed(message)),
        }
    }

    async fn kill_session(&self, chat: &str) {
        let session = self.state.sessions.lock().expect("sessions").remove(chat);
        if let Some(session) = session {
            session.control.kill(session.id);
        }
    }

    async fn one_shot(
        &self,
        workspace: &Workspace,
        network: bool,
        command: &str,
        ms: f64,
        limits: &str,
        tap: backend::Tap,
        cancel: &CancellationToken,
    ) -> Result<OneShotRun, SandboxError> {
        let handle = self.live_box(&workspace.chat, network).ok_or_else(|| {
            SandboxError::Failed("Docker sandbox exec failed (1):\nthe box is gone".to_string())
        })?;
        let argv = backend::one_shot_argv(limits, ms, command);
        let timeout = Duration::from_secs_f64(ms / 1000.0) + Duration::from_secs(30);
        let (output, _, code, _) = self
            .run_program(&handle, argv, false, timeout, tap, Some(cancel))
            .await?;
        Ok(OneShotRun {
            output: String::from_utf8(output).expect("collect_one_shot answers text"),
            code,
        })
    }

    async fn end_processes(&self, chat: &str, network: bool) {
        if let Some(handle) = self.live_box(chat, network) {
            handle.control.end().await;
        }
    }

    async fn program(
        &self,
        workspace: &Workspace,
        argv: &[String],
        timeout: Duration,
    ) -> Result<ProgramRun, SandboxError> {
        let handle = self.live_box(&workspace.chat, false).ok_or_else(|| {
            SandboxError::Failed("Docker sandbox exec failed (1):\nthe box is gone".to_string())
        })?;
        let (stdout, all, code, _) = self
            .run_program(&handle, argv.to_vec(), true, timeout, None, None)
            .await?;
        Ok(ProgramRun { stdout, all, code })
    }

    async fn remove(&self, chat: &str) {
        self.kill_session(chat).await;
        tokio::join!(self.take_box(chat, false), self.take_box(chat, true));
    }

    async fn shutdown(&self) {
        let keys: Vec<(String, bool)> = self
            .state
            .boxes
            .lock()
            .expect("boxes")
            .keys()
            .cloned()
            .collect();
        for (chat, network) in keys {
            self.take_box(&chat, network).await;
        }
    }
}
