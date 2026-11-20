//! The Codex CLI in the app's WSL distro on Windows: the Linux route's box
//! ([`crate::proxy::codex_box_args`], the allow-list proxy, the relay at
//! `/run/mcbench-relay`), run by the guest base's agent as
//! `sandbox-agent codex <login> <tools> <harness> <uid>:<gid> <verb> ...`,
//! which the app starts as root with `wsl.exe --exec`.
//!
//! | What the box needs | On Linux | In the distro |
//! |---|---|---|
//! | The person's Codex login | `CODEX_HOME/auth.json` (else `~/.codex`) bound read-write at `/codex-home/auth.json` | the same file, the Windows one (`CODEX_HOME` or `%USERPROFILE%\.codex`): its folder mounted by drvfs at [`LOGIN`] (the agent's user only, `umask=077`), then the file alone bound read-write at `/codex-home/auth.json`; a refresh Codex writes in the box is written in place into the Windows file |
//! | The Codex install | the tools folder's, its programs execute-only (`0111`, [`crate::proxy::execute_only`]) once installed, bound read-only at `/opt/codex` | the tools folder mounted read-only with drvfs metadata at [`TOOLS`]; before each box the install's programs are given `0111` through a writable mount of it of the run's own (drvfs keeps the mode in the Windows file's metadata), the other files keep `0755`; the install bound read-only at `/opt/codex` |
//! | The chat's CODEX_HOME and workspace, the proxy's log | under the data folder's `harness/<chat>` | the same folders: the data folder's `harness` mounted by drvfs at [`HARNESS`] |
//! | The bridge's and the proxy's sockets, the prompt's images | a run folder of the app's | [`RUNS`]`/<run>/sock` in the distro's memory; the images are copied in from the chat's folder |
//! | The proxy | in the app | in this agent, outside the box, as the agent's user |
//! | The tool bridge | `mcp.sock`, served by the app | `mcp.sock`, served by `... bridge <run>`, whose stdin and stdout the app joins to a connection to its named pipe |
//! | The limits | the systemd scope's | a cgroup of the run's own with the same values ([`crate::proxy::CODEX_LIMITS`]) |
//!
//! The verbs: `cli <program> <args...>` runs Codex outside the box with
//! CODEX_HOME the login's folder, as the app runs it on the Linux host
//! (`--version`, `login --device-auth`, `logout`, `debug models`);
//! `bridge <run>` serves the run's `mcp.sock` for one connection over
//! stdin and stdout; `box <run> <images> <log> <hosts> -- <box...>` runs
//! the box with stdin and stdout its own. Each first makes sure the three
//! folders are mounted from the paths it is given.

/// Where the person's Codex login folder is mounted in the distro.
pub const LOGIN: &str = "/mnt/codex-login";
/// Where the app's tools folder is mounted, read only.
pub const TOOLS: &str = "/mnt/tools";
/// Where the data folder's `harness` folder is mounted.
pub const HARNESS: &str = "/mnt/harness";
/// Where the runs' sockets are, in the distro's memory.
pub const RUNS: &str = "/run/mcbench-codex";
/// The guest base's agent, which is also the box's relay.
pub const AGENT: &str = "/usr/bin/mcbench-sandbox-agent";

/// What a person reads when a Codex turn finds WSL not installed.
pub const WSL_MISSING: &str = "The Codex CLI runs in a Linux distribution under WSL, and WSL is not installed: an administrator installs it once with `wsl --install --no-distribution` and restarts Windows if asked, then try again";

/// What a person reads when the distro cannot be started, with the
/// distro's own reason after it.
pub const WSL_UNAVAILABLE: &str =
    "The Codex CLI runs in a Linux distribution under WSL, and it could not be started";

/// What a person reads before setup has prepared the distro.
pub const NOT_YET: &str =
    "The Codex CLI runs in the model's shell under WSL, which setup has not prepared yet";

/// Whether a Codex turn can start in the distro, from what setup's shell
/// step found (`mcbench_build::sandbox::wsl::state`): the sentence a
/// person reads when not.
#[cfg(windows)]
pub fn ready() -> Result<(), String> {
    match &*mcbench_build::sandbox::wsl::state().borrow() {
        mcbench_build::Readiness::Ready => Ok(()),
        mcbench_build::Readiness::Waiting => Err(NOT_YET.to_owned()),
        mcbench_build::Readiness::Unavailable(said) if said == crate::windows::WSL_MISSING => {
            Err(WSL_MISSING.to_owned())
        }
        mcbench_build::Readiness::Unavailable(said) => Err(format!(
            "{WSL_UNAVAILABLE}: {}",
            said.strip_prefix(&format!("{}: ", crate::windows::WSL_UNAVAILABLE))
                .unwrap_or(said)
        )),
    }
}

#[cfg(target_os = "linux")]
pub use imp::main;

#[cfg(target_os = "linux")]
mod imp {
    use std::io::{Read, Write};
    use std::os::unix::process::CommandExt;
    use std::path::{Path, PathBuf};

    use super::{HARNESS, LOGIN, RUNS, TOOLS};

    fn fail(what: impl std::fmt::Display) -> ! {
        eprintln!("mcbench codex: {what}");
        std::process::exit(1)
    }

    /// `/proc/mounts`' octal escapes undone (`\134` is `\`).
    fn unescape(field: &str) -> String {
        let bytes = field.as_bytes();
        let mut out = Vec::new();
        let mut i = 0;
        while i < bytes.len() {
            let octal = bytes
                .get(i + 1..i + 4)
                .filter(|d| bytes[i] == b'\\' && d.iter().all(|b| (b'0'..=b'7').contains(b)));
            match octal {
                Some(d) => {
                    out.push((d[0] - b'0') * 64 + (d[1] - b'0') * 8 + (d[2] - b'0'));
                    i += 4;
                }
                None => {
                    out.push(bytes[i]);
                    i += 1;
                }
            }
        }
        String::from_utf8_lossy(&out).into_owned()
    }

    /// What is mounted at `point`, by its source.
    fn mounted(point: &str) -> Option<String> {
        let mounts = std::fs::read_to_string("/proc/mounts").unwrap_or_else(|e| fail(e));
        mounts.lines().rev().find_map(|line| {
            let mut fields = line.split(' ');
            let source = fields.next()?;
            (unescape(fields.next()?) == point).then(|| unescape(source))
        })
    }

    /// The three folders mounted from `shares` (Windows paths), one caller
    /// at a time; a folder mounted from another path is mounted again.
    fn ensure_shares(shares: [(&str, &str); 3], (uid, gid): (u32, u32)) {
        std::fs::create_dir_all(RUNS).unwrap_or_else(|e| fail(format!("{RUNS}: {e}")));
        let lock = std::fs::File::create(format!("{RUNS}.lock")).unwrap_or_else(|e| fail(e));
        // SAFETY: flock of a file this process opened; the lock goes with
        // the descriptor, at the latest when the process ends.
        if unsafe { libc::flock(std::os::fd::AsRawFd::as_raw_fd(&lock), libc::LOCK_EX) } != 0 {
            fail(std::io::Error::last_os_error());
        }
        let options = [
            format!("uid={uid},gid={gid},umask=077"),
            "ro,metadata,umask=022".to_string(),
            format!("metadata,uid={uid},gid={gid},umask=022,fmask=133"),
        ];
        for ((point, source), options) in [LOGIN, TOOLS, HARNESS]
            .into_iter()
            .zip(shares.map(|(_, s)| s))
            .zip(options)
        {
            match mounted(point) {
                Some(found) if found == source => continue,
                Some(_) => umount(Path::new(point)),
                None => {}
            }
            drvfs(source, Path::new(point), &options);
        }
    }

    fn drvfs(source: &str, point: &Path, options: &str) {
        std::fs::create_dir_all(point)
            .unwrap_or_else(|e| fail(format!("{}: {e}", point.display())));
        let status = mcbench_spawn::command("/sbin/mount.drvfs")
            .arg(source)
            .arg(point)
            .args(["-o", options])
            .status()
            .unwrap_or_else(|e| fail(format!("/sbin/mount.drvfs: {e}")));
        if !status.success() {
            fail(format!(
                "mounting {source} at {}: {status}",
                point.display()
            ));
        }
    }

    fn umount(point: &Path) {
        let path = std::ffi::CString::new(point.as_os_str().as_encoded_bytes()).expect("no NUL");
        // SAFETY: umount2 of a NUL-terminated path that lives until the
        // call returns.
        if unsafe { libc::umount2(path.as_ptr(), libc::MNT_DETACH) } != 0 {
            fail(format!(
                "unmounting {}: {}",
                point.display(),
                std::io::Error::last_os_error()
            ));
        }
    }

    /// The box's Codex install (bound at `/opt/codex`, a folder under
    /// [`TOOLS`]) with its programs execute-only, as the Linux install
    /// leaves them: through a writable drvfs mount of the tools folder
    /// (`tools`, its Windows path) of the run's own, whose modes drvfs keeps
    /// in the Windows files' metadata and the read-only mount shows.
    fn execute_only(boxed: &[String], tools: &str, run_folder: &Path) {
        let install = boxed
            .windows(3)
            .find(|w| w[0] == "--ro-bind" && w[2] == "/opt/codex")
            .map(|w| PathBuf::from(&w[1]))
            .unwrap_or_else(|| fail("the box binds no Codex install at /opt/codex"));
        let under = install
            .strip_prefix(TOOLS)
            .unwrap_or_else(|_| fail(format!("{} is not under {TOOLS}", install.display())));
        let writable = run_folder.join("tools");
        drvfs(tools, &writable, "metadata,umask=022");
        let made = crate::proxy::execute_only(&writable.join(under));
        umount(&writable);
        made.unwrap_or_else(|e| fail(e));
    }

    fn sock_dir(run: &str, (uid, gid): (u32, u32)) -> PathBuf {
        if run.is_empty() || !run.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
            fail(format!("the run {run:?} is not a run's name"));
        }
        let folder = Path::new(RUNS).join(run);
        let sock = folder.join("sock");
        std::fs::create_dir_all(&sock).unwrap_or_else(|e| fail(format!("{}: {e}", sock.display())));
        for dir in [&folder, &sock] {
            std::os::unix::fs::chown(dir, Some(uid), Some(gid))
                .unwrap_or_else(|e| fail(format!("{}: {e}", dir.display())));
        }
        sock
    }

    /// `codex <login> <tools> <harness> <uid>:<gid> <verb> ...`, as root.
    pub fn main(login: &str, tools: &str, harness: &str, ids: &str, verb: &[String]) -> ! {
        let ids = ids
            .split_once(':')
            .and_then(|(u, g)| u.parse().ok().zip(g.parse().ok()))
            .unwrap_or_else(|| fail(format!("{ids} is not <uid>:<gid>")));
        ensure_shares(
            [("login", login), ("tools", tools), ("harness", harness)],
            ids,
        );
        match verb
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .as_slice()
        {
            ["cli", program, args @ ..] => {
                crate::agent::become_user(ids.0, ids.1);
                let error = mcbench_spawn::command(program)
                    .args(args)
                    .env_clear()
                    .env("PATH", "/usr/bin")
                    .env("HOME", LOGIN)
                    .env("CODEX_HOME", LOGIN)
                    .env("BROWSER", "true")
                    .current_dir(LOGIN)
                    .exec();
                fail(format!("{program}: {error}"))
            }
            ["bridge", run] => bridge(&sock_dir(run, ids), ids),
            ["box", run, images, log, hosts, "--", boxed @ ..] if !boxed.is_empty() => run_box(
                run,
                &sock_dir(run, ids),
                Path::new(images),
                log,
                hosts,
                boxed,
                ids,
                tools,
            ),
            _ => fail(
                "usage: codex <login> <tools> <harness> <uid>:<gid> cli <program> <args...> | bridge <run> | box <run> <images> <log> <hosts> -- <box...>",
            ),
        }
    }

    /// The run's `mcp.sock` for one connection, carried over stdin and
    /// stdout; "ready" on stderr once it listens.
    fn bridge(sock: &Path, (uid, gid): (u32, u32)) -> ! {
        crate::agent::become_user(uid, gid);
        let listener = mcbench_relay::bind_unix(&sock.join("mcp.sock")).unwrap_or_else(|e| fail(e));
        eprintln!("ready");
        let (connection, _) = listener.accept().unwrap_or_else(|e| fail(e));
        let mut to_bridge = connection.try_clone().unwrap_or_else(|e| fail(e));
        std::thread::spawn(move || {
            let _ = std::io::copy(&mut std::io::stdin(), &mut to_bridge);
            let _ = to_bridge.shutdown(std::net::Shutdown::Write);
        });
        let mut from_bridge = connection;
        let mut out = std::io::stdout();
        let mut buffer = [0u8; 65536];
        loop {
            match from_bridge.read(&mut buffer) {
                Ok(0) | Err(_) => std::process::exit(0),
                Ok(n) => {
                    if out
                        .write_all(&buffer[..n])
                        .and_then(|()| out.flush())
                        .is_err()
                    {
                        std::process::exit(0)
                    }
                }
            }
        }
    }

    /// The cgroup v2 root of the distro.
    fn cgroup_root() -> PathBuf {
        let mounts = std::fs::read_to_string("/proc/mounts").unwrap_or_else(|e| fail(e));
        mounts
            .lines()
            .find_map(|line| {
                let fields: Vec<&str> = line.split(' ').collect();
                (fields.get(2) == Some(&"cgroup2")).then(|| PathBuf::from(fields[1]))
            })
            .unwrap_or_else(|| fail("no cgroup2 file system is mounted"))
    }

    fn write(file: &Path, text: &str) {
        std::fs::write(file, text).unwrap_or_else(|e| fail(format!("{}: {e}", file.display())));
    }

    #[allow(clippy::too_many_arguments)]
    fn run_box(
        run: &str,
        sock: &Path,
        images: &Path,
        log: &str,
        hosts: &str,
        boxed: &[&str],
        (uid, gid): (u32, u32),
        tools: &str,
    ) -> ! {
        // When the wsl.exe that started this ends, its relay in the distro
        // goes, and this process with it; the box has --die-with-parent.
        // SAFETY: prctl with PR_SET_PDEATHSIG and a valid signal.
        unsafe { libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) };
        let rootfs = boxed
            .iter()
            .position(|a| *a == "--overlay-src")
            .and_then(|i| boxed.get(i + 1))
            .map(PathBuf::from)
            .unwrap_or_else(|| fail("the box names no root filesystem"));
        let unpacked = crate::linux::rootfs::check(rootfs.parent().unwrap_or(&rootfs))
            .unwrap_or_else(|e| fail(e));
        let boxed: Vec<String> = boxed.iter().map(|a| (*a).to_string()).collect();
        let boxed = crate::proxy::insert_image_env(&boxed, &unpacked.image.env);
        execute_only(&boxed, tools, sock.parent().unwrap_or(sock));
        if let Ok(entries) = std::fs::read_dir(images) {
            for entry in entries.flatten() {
                let to = sock.join(entry.file_name());
                std::fs::copy(entry.path(), &to)
                    .unwrap_or_else(|e| fail(format!("{}: {e}", to.display())));
            }
        }
        let socket = sock.join("egress.sock");
        let allowed: Vec<String> = hosts.split(',').map(str::to_string).collect();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let (proxy_socket, log) = (socket.clone(), PathBuf::from(log));
        std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("a tokio runtime");
            runtime.block_on(async move {
                match crate::proxy::start_egress_proxy(&proxy_socket, allowed, log).await {
                    Ok(_proxy) => {
                        let _ = ready_tx.send(Ok(()));
                        std::future::pending::<()>().await
                    }
                    Err(error) => {
                        let _ = ready_tx.send(Err(error.to_string()));
                    }
                }
            });
        });
        match ready_rx.recv() {
            Ok(Ok(())) => {}
            Ok(Err(error)) => fail(format!("the egress proxy on {}: {error}", socket.display())),
            Err(_) => fail("the egress proxy's thread ended"),
        }
        std::os::unix::fs::chown(&socket, Some(uid), Some(gid))
            .unwrap_or_else(|e| fail(format!("{}: {e}", socket.display())));
        let root = cgroup_root();
        // Where wsl.exe started this process: it goes back there, a leaf,
        // before its own cgroup is removed (WSL's cgroup root holds no
        // process: writing a pid there is EBUSY).
        let started_in = std::fs::read_to_string("/proc/self/cgroup")
            .unwrap_or_else(|e| fail(format!("/proc/self/cgroup: {e}")))
            .lines()
            .find_map(|l| {
                l.strip_prefix("0::")
                    .map(|p| root.join(p.trim_start_matches('/')))
            })
            .unwrap_or_else(|| fail("this process is in no cgroup v2 folder"));
        write(&root.join("cgroup.subtree_control"), "+memory +pids +cpu");
        let ours = root.join(format!("mcbench-codex-{run}"));
        std::fs::create_dir_all(&ours).unwrap_or_else(|e| fail(format!("{}: {e}", ours.display())));
        for (file, value) in crate::proxy::CODEX_LIMITS {
            write(&ours.join(file), value);
        }
        write(&ours.join("cgroup.procs"), &std::process::id().to_string());
        // As the distro's bash boxes start: under an empty binfmt_misc of
        // the box's own, so an MZ file in the box never reaches WSL's /init
        // (`linux::keeper::own_binfmt`).
        let binfmt = sock.parent().unwrap_or(sock).join("binfmt");
        std::fs::create_dir_all(&binfmt)
            .unwrap_or_else(|e| fail(format!("{}: {e}", binfmt.display())));
        std::os::unix::fs::chown(&binfmt, Some(uid), Some(gid))
            .unwrap_or_else(|e| fail(format!("{}: {e}", binfmt.display())));
        let status = mcbench_spawn::command(super::AGENT)
            .arg("binfmt")
            .arg(&binfmt)
            .args(&boxed)
            .env_clear()
            .env("PATH", "/usr/bin")
            .uid(uid)
            .gid(gid)
            .status()
            .unwrap_or_else(|e| fail(format!("{}: {e}", boxed[0])));
        if !status.success() {
            // What the run's cgroup counted (an OOM kill, the task limit),
            // for whoever reads why the box ended.
            let events = |file: &str| {
                std::fs::read_to_string(ours.join(file))
                    .unwrap_or_else(|e| e.to_string())
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
            };
            eprintln!(
                "mcbench codex: the box ended with {status}; memory.events: {}; pids.events: {}",
                events("memory.events"),
                events("pids.events")
            );
        }
        write(
            &started_in.join("cgroup.procs"),
            &std::process::id().to_string(),
        );
        let _ = std::fs::remove_dir(&ours);
        let _ = std::fs::remove_dir_all(sock.parent().unwrap_or(sock));
        use std::os::unix::process::ExitStatusExt;
        std::process::exit(
            status
                .code()
                .unwrap_or_else(|| (-status.signal().unwrap_or(0)) & 0xff),
        )
    }

    #[cfg(test)]
    mod tests {
        #[test]
        fn mount_sources_are_read_with_their_escapes_undone() {
            assert_eq!(
                super::unescape(r"C:\134Users\134bench\134.codex"),
                r"C:\Users\bench\.codex"
            );
            assert_eq!(super::unescape(r"/mnt/a\040b"), "/mnt/a b");
        }
    }
}
