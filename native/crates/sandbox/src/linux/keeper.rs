//! The keeper: one small single-threaded process per box, started by the
//! app as `<helper> keep` inside `systemd-run --user --scope -p
//! Delegate=yes`, its stdin the control socket ([`super::control`]).
//!
//! It makes two cgroups in its scope, `keeper` (itself, bubblewrap's outer
//! process, pasta) and `box` (today's container limits; the box's init and
//! every program entered into it), sets the box's own resource limits
//! ([`RLIMITS`]) on itself, starts the box, and then enters each
//! program the app asks for: `setns` into the box's namespaces (the network
//! namespace's owning user namespace first when it is not the box's, as in
//! the networked box, whose network namespace pasta's user namespace owns),
//! `chroot` to the box's root, `/work`, every capability set cleared and
//! `no_new_privs`, a fork so the program is in the box's pid namespace, the
//! signal mask and dispositions reset, the program moved into `box` and
//! exec'd. Its process outside the box waits for it and exits with its code
//! (128 + the signal when it was killed), as `docker exec` does.
//!
//! When the control socket closes (the app removed the box or ended), the
//! keeper kills the box and exits.
//!
//! The networked box: pasta makes a user namespace and a network namespace
//! and runs `<helper> netinit <file>` in them as their root, which turns
//! IPv6 off (`IPV6_OFF`), runs today's `EGRESS_RULES` with the root
//! filesystem's iptables in namespaces of its own that keep only
//! `CAP_NET_ADMIN` ([`run_rules`]: not under `/usr/bin/bwrap`, whose
//! AppArmor profile on Ubuntu 26.04 takes that capability away), and then
//! execs the box's bubblewrap, which makes the box's own user namespace
//! inside: nothing in the box has a capability over the network namespace.

use std::ffi::CString;
use std::io::Write;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use super::control::{self, Event, Request};

/// Today's container limits (`ensureContainer`: `--memory 512m
/// --memory-swap 512m --pids-limit 256 --cpus 2`) as the `box` cgroup's
/// files.
pub const LIMITS: [(&str, &str); 4] = [
    ("memory.max", "536870912"),
    ("memory.swap.max", "0"),
    ("pids.max", "256"),
    ("cpu.max", "200000 100000"),
];

/// The box's own resource limits, soft and hard, the same on every host
/// (`setrlimit` in the keeper, so the box's init, pasta and every program
/// entered into the box inherit them): where today's container took the
/// machine's (Docker gives a container its daemon's limits and the kernel's
/// core dump handler), a box answers the same on Linux, in the macOS VM and
/// in WSL.
///
/// | Limit | Value | Why |
/// |---|---|---|
/// | `RLIMIT_NOFILE` | 65536 | today's container's `ulimit -n` on adipc (the one recorded Docker run, `tests/diff/RESULTS.md`), which came from its systemd's `DefaultLimitNOFILESoft` |
/// | `RLIMIT_CORE` | 1 byte | no core dump: below a page no core file is written, and 1 is the kernel's value for "no dump" through a pipe handler (a limit of 0 still pipes the core to systemd-coredump, and bash then prints "(core dumped)"); `ulimit -c` shows 0 |
///
/// The hard limits are set too, so a command cannot raise them; a host
/// whose hard open-file limit is below 65536 refuses the box (the macOS
/// VM's agent raises its own as root first).
pub const RLIMITS: [(&str, u64); 2] = [("RLIMIT_NOFILE", 65536), ("RLIMIT_CORE", 1)];

/// Sets [`RLIMITS`] on this process, soft and hard.
pub fn set_rlimits() -> Result<(), String> {
    for (name, value) in RLIMITS {
        let resource = match name {
            "RLIMIT_NOFILE" => libc::RLIMIT_NOFILE,
            "RLIMIT_CORE" => libc::RLIMIT_CORE,
            other => unreachable!("no resource {other}"),
        };
        let limit = libc::rlimit {
            rlim_cur: value as libc::rlim_t,
            rlim_max: value as libc::rlim_t,
        };
        // SAFETY: setrlimit of this process with a valid rlimit.
        if unsafe { libc::setrlimit(resource, &limit) } != 0 {
            return Err(format!(
                "setting the box's {name} to {value}: {}",
                std::io::Error::last_os_error()
            ));
        }
    }
    Ok(())
}

/// The box the keeper starts.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Spec {
    /// bubblewrap on the host.
    pub bwrap: PathBuf,
    /// The box's argv after `bwrap` (its init last), without `--info-fd`.
    pub box_args: Vec<String>,
    /// The networked box: pasta's argv (the program first) before `--`,
    /// and the rules bubblewrap's argv after `bwrap`.
    pub network: Option<Network>,
    /// The program and arguments before `netinit` that run
    /// [`crate::agent::main`].
    pub helper: Vec<String>,
    /// A private folder for the box's files (its info, its log).
    pub run: PathBuf,
    /// The environment of every program entered into the box.
    pub env: Vec<String>,
    /// A guest's box (the macOS VM, the WSL2 distro: a
    /// [`super::Delegation::Subtree`]): its first program starts under a
    /// binfmt_misc of the box's own ([`own_binfmt`]). A Linux host's box reads
    /// the host's binfmt_misc, as Docker's containers do.
    pub binfmt: bool,
}

/// The helper's `binfmt FOLDER PROGRAM ARGS...`: a user and mount namespace
/// of the box's own holding an empty binfmt_misc, mounted on FOLDER (an
/// empty folder the keeper made in the box's run folder), then PROGRAM
/// exec'd in it.
///
/// Why. Since Linux 6.7 binfmt_misc is per user namespace: an exec looks for
/// a handler in the caller's user namespace and walks up to the first
/// ancestor that has an instance of binfmt_misc. WSL registers `WSLInterop`
/// (the `MZ` magic, interpreter `/init`) in the WSL VM's initial namespace,
/// shared by every distro, so without this a box's `MZ` file is handed to
/// WSL's `/init`. Under an empty instance of its own no handler is found and
/// the exec fails with `ENOEXEC`: bash prints `cannot execute binary file:
/// Exec format error` and 126, as in today's Docker box. The instance is the
/// user namespace's wherever it is mounted, so FOLDER serves as
/// `/proc/sys/fs/binfmt_misc` would.
///
/// How. `unshare(CLONE_NEWUSER | CLONE_NEWNS)`, then the caller's own uid
/// and gid mapped to themselves (not to root), so bubblewrap stays an
/// unprivileged program and a box's ids and files read as before; the
/// mount of binfmt_misc on FOLDER (the capability the new user namespace
/// gives lasts until the exec); then PROGRAM.
///
/// The mount's one tolerated error is `ENODEV`, "no such filesystem type":
/// the kernel has no binfmt_misc, so no handler is registered anywhere on
/// it, and the box starts (the macOS VM's kernel has it as a module its init
/// does not load; `GUEST.md`). A registered handler implies the filesystem
/// type exists, so `ENODEV` and "a handler reaches the box" cannot both be
/// true. FOLDER always exists (the keeper made it), so no other cause can
/// answer `ENODEV` or be mistaken for it: any other errno, `ENOENT`
/// included, refuses the box, naming the call and the errno
/// (`binfmt_mounted`).
pub fn own_binfmt(argv: &[&str]) -> ! {
    let [folder, program, args @ ..] = argv else {
        eprintln!("mcbench sandbox agent: binfmt needs a folder and a program");
        std::process::exit(2)
    };
    if let Err(error) = enter_own_binfmt(folder) {
        eprintln!("mcbench sandbox agent: {error}");
        std::process::exit(1)
    }
    let error = mcbench_spawn::command(program).args(args).exec();
    eprintln!("mcbench sandbox agent: {program}: {error}");
    std::process::exit(1)
}

fn enter_own_binfmt(folder: &str) -> Result<(), String> {
    let target = CString::new(folder).map_err(|_| format!("the folder {folder:?} holds a NUL"))?;
    // SAFETY: getuid and getgid cannot fail.
    let (uid, gid) = unsafe { (libc::getuid(), libc::getgid()) };
    // SAFETY: unshare takes no pointer.
    if unsafe { libc::unshare(libc::CLONE_NEWUSER | libc::CLONE_NEWNS) } != 0 {
        return Err(format!(
            "unshare(CLONE_NEWUSER | CLONE_NEWNS): {}",
            std::io::Error::last_os_error()
        ));
    }
    for (file, text) in [
        ("/proc/self/setgroups", "deny".to_string()),
        ("/proc/self/uid_map", format!("{uid} {uid} 1")),
        ("/proc/self/gid_map", format!("{gid} {gid} 1")),
    ] {
        std::fs::write(file, &text).map_err(|e| format!("writing {text:?} to {file}: {e}"))?;
    }
    let mounted = {
        let source = c"binfmt_misc";
        // SAFETY: mount with NUL-terminated strings that live across the call
        // and no data.
        let rc = unsafe {
            libc::mount(
                source.as_ptr(),
                target.as_ptr(),
                source.as_ptr(),
                libc::MS_NOSUID | libc::MS_NODEV | libc::MS_NOEXEC,
                std::ptr::null(),
            )
        };
        if rc == 0 {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error().raw_os_error().unwrap_or(0))
        }
    };
    binfmt_mounted(folder, mounted)
}

/// What the binfmt_misc mount on `folder` answered means for the box
/// ([`own_binfmt`]): mounted, or `ENODEV` (no binfmt_misc in the kernel),
/// starts it; any other errno refuses it.
fn binfmt_mounted(folder: &str, result: Result<(), i32>) -> Result<(), String> {
    match result {
        Ok(()) | Err(libc::ENODEV) => Ok(()),
        Err(errno) => Err(format!(
            "mount(\"binfmt_misc\", {folder:?}, \"binfmt_misc\"): {}",
            std::io::Error::from_raw_os_error(errno)
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::binfmt_mounted;

    #[test]
    fn a_box_starts_on_its_own_binfmt_misc_or_without_one_and_any_other_mount_error_refuses_it() {
        let folder = "/var/mcbench/.run/0123/binfmt";
        assert_eq!(binfmt_mounted(folder, Ok(())), Ok(()));
        assert_eq!(binfmt_mounted(folder, Err(libc::ENODEV)), Ok(()));
        let refused = binfmt_mounted(folder, Err(libc::EPERM)).unwrap_err();
        assert!(
            refused.starts_with(
                "mount(\"binfmt_misc\", \"/var/mcbench/.run/0123/binfmt\", \"binfmt_misc\"): "
            ) && refused.contains("os error 1)"),
            "{refused}"
        );
        // ENOENT refuses too: the folder is always there.
        for errno in [libc::ENOENT, libc::EACCES] {
            let refused = binfmt_mounted(folder, Err(errno)).unwrap_err();
            assert!(refused.contains(&format!("os error {errno})")), "{refused}");
        }
    }
}

/// The networked box's parts.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Network {
    /// pasta's argv, the program first.
    pub pasta: Vec<String>,
    /// What netinit runs today's rules in ([`run_rules`]).
    pub rules: Rules,
    /// The sysctls set before the rules (`IPV6_OFF`), as `/proc/sys` paths.
    pub sysctls: Vec<(String, String)>,
}

/// The rules' runner ([`run_rules`]).
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Rules {
    /// The unpacked root filesystem's folder (its `/`).
    pub rootfs: String,
    /// The box's `resolv.conf`, at the runner's `/etc/resolv.conf`.
    pub resolv: String,
    /// The runner's `PATH` (the image's).
    pub path: String,
    /// The script `sh -c` runs (`EGRESS_RULES`).
    pub script: String,
}

fn fail(what: &str, error: impl std::fmt::Display) -> ! {
    eprintln!("mcbench sandbox keeper: {what}: {error}");
    std::process::exit(1)
}

/// The keeper's cgroups.
struct Cgroups {
    keeper: OwnedFd,
    boxed: OwnedFd,
    /// The `box` cgroup's folder, bound read-only at the box's
    /// `/sys/fs/cgroup` (Docker's view: the box's own limits and use).
    box_dir: PathBuf,
}

fn own_cgroup() -> PathBuf {
    let text = std::fs::read_to_string("/proc/self/cgroup")
        .unwrap_or_else(|e| fail("reading /proc/self/cgroup", e));
    let path = text
        .lines()
        .find_map(|line| line.strip_prefix("0::"))
        .unwrap_or_else(|| {
            fail(
                "finding the cgroup",
                "no cgroup v2 line in /proc/self/cgroup (cgroup v1 is not supported)",
            )
        });
    PathBuf::from("/sys/fs/cgroup").join(path.trim_start_matches('/'))
}

fn open_procs(dir: &Path) -> OwnedFd {
    let file = std::fs::OpenOptions::new()
        .write(true)
        .open(dir.join("cgroup.procs"))
        .unwrap_or_else(|e| fail(&format!("opening {}/cgroup.procs", dir.display()), e));
    file.into()
}

/// Writes a pid ("0": the writer) to a `cgroup.procs` opened earlier; the
/// open-time credentials and cgroup namespace decide (Linux 5.16 and later).
fn move_into(procs: RawFd, pid: &str) -> bool {
    // SAFETY: write of a short buffer to a descriptor we hold.
    unsafe { libc::write(procs, pid.as_ptr() as *const _, pid.len()) == pid.len() as isize }
}

impl Cgroups {
    fn make() -> Cgroups {
        let scope = own_cgroup();
        for name in ["keeper", "box"] {
            std::fs::create_dir(scope.join(name)).unwrap_or_else(|e| {
                fail(
                    &format!("making the {name} cgroup in {}", scope.display()),
                    e,
                )
            });
        }
        let keeper = open_procs(&scope.join("keeper"));
        if !move_into(keeper.as_raw_fd(), "0") {
            fail(
                "moving the keeper into its cgroup",
                std::io::Error::last_os_error(),
            );
        }
        std::fs::write(scope.join("cgroup.subtree_control"), "+memory +pids +cpu")
            .unwrap_or_else(|e| fail("enabling the memory, pids and cpu controllers for the box (the user manager must delegate them)", e));
        for (file, value) in LIMITS {
            std::fs::write(scope.join("box").join(file), value)
                .unwrap_or_else(|e| fail(&format!("setting {file}"), e));
        }
        let boxed = open_procs(&scope.join("box"));
        Cgroups {
            keeper,
            boxed,
            box_dir: scope.join("box"),
        }
    }
}

/// The box's namespaces and root, opened once, and the order to join them.
///
/// A namespace can only be joined by a process with `CAP_SYS_ADMIN` in the
/// user namespace that owns it, and joining a user namespace gives every
/// capability in it and none in its ancestors. The box's namespaces are
/// owned by different user namespaces: bubblewrap's (pid, ipc, uts, mount,
/// cgroup, and the network of the persistent box), pasta's (the networked
/// box's network), and the init's own (`--disable-userns` makes one more,
/// inside bubblewrap's). So the user namespaces are joined outermost first,
/// each followed by the namespaces it owns.
struct Entry {
    /// (namespace, what it is) in the order to join.
    plan: Vec<(OwnedFd, &'static str)>,
    root: OwnedFd,
    last_cap: i32,
    /// The keeper's control socket, closed first in every child.
    socket: RawFd,
}

fn open_path(path: &Path, flags: i32) -> OwnedFd {
    let name = CString::new(path.as_os_str().as_bytes()).expect("no NUL in a /proc path");
    // SAFETY: open of a NUL-terminated path.
    let fd = unsafe { libc::open(name.as_ptr(), flags | libc::O_CLOEXEC) };
    if fd < 0 {
        fail(
            &format!("opening {}", path.display()),
            std::io::Error::last_os_error(),
        );
    }
    // SAFETY: a new descriptor that nobody else owns.
    unsafe { OwnedFd::from_raw_fd(fd) }
}

fn inode(fd: &OwnedFd) -> (u64, u64) {
    // SAFETY: fstat of a descriptor we hold into a zeroed struct.
    unsafe {
        let mut stat: libc::stat = std::mem::zeroed();
        if libc::fstat(fd.as_raw_fd(), &mut stat) != 0 {
            fail("fstat of a namespace", std::io::Error::last_os_error());
        }
        (stat.st_dev, stat.st_ino)
    }
}

/// `NS_GET_USERNS`: the user namespace that owns a namespace.
const NS_GET_USERNS: libc::Ioctl = 0xb701;
/// `NS_GET_PARENT`: a user namespace's parent.
const NS_GET_PARENT: libc::Ioctl = 0xb702;

/// An ioctl that answers a namespace descriptor, or `None` (`EPERM` past
/// the caller's own user namespace).
fn ns_ioctl(fd: &OwnedFd, request: libc::Ioctl) -> Option<OwnedFd> {
    // SAFETY: an ioctl on a namespace descriptor that answers a new one,
    // which nobody else owns.
    unsafe {
        let got = libc::ioctl(fd.as_raw_fd(), request);
        (got >= 0).then(|| OwnedFd::from_raw_fd(got))
    }
}

/// How many user namespaces lie between this one and the keeper's.
fn depth(user: &OwnedFd) -> usize {
    let mut depth = 0;
    let mut at = ns_ioctl(user, NS_GET_PARENT);
    while let Some(parent) = at {
        depth += 1;
        at = ns_ioctl(&parent, NS_GET_PARENT);
    }
    depth
}

/// A user namespace, its depth, and the namespaces it owns.
type Owner = (OwnedFd, usize, Vec<(OwnedFd, &'static str)>);

impl Entry {
    fn open(pid: i32, socket: RawFd) -> Entry {
        let ns = |name: &str| {
            open_path(
                &PathBuf::from(format!("/proc/{pid}/ns/{name}")),
                libc::O_RDONLY,
            )
        };
        let user = ns("user");
        // (owner, depth, its namespaces)
        let mut owners: Vec<Owner> = Vec::new();
        for (name, what) in [
            ("net", "setns net"),
            ("pid", "setns pid"),
            ("ipc", "setns ipc"),
            ("uts", "setns uts"),
            ("mnt", "setns mnt"),
            ("cgroup", "setns cgroup"),
        ] {
            let fd = ns(name);
            let owner = ns_ioctl(&fd, NS_GET_USERNS)
                .unwrap_or_else(|| fail("NS_GET_USERNS", std::io::Error::last_os_error()));
            match owners
                .iter_mut()
                .find(|(known, _, _)| inode(known) == inode(&owner))
            {
                Some((_, _, owned)) => owned.push((fd, what)),
                None => {
                    let d = depth(&owner);
                    owners.push((owner, d, vec![(fd, what)]));
                }
            }
        }
        if !owners
            .iter()
            .any(|(owner, _, _)| inode(owner) == inode(&user))
        {
            let d = depth(&user);
            owners.push((user, d, Vec::new()));
        }
        owners.sort_by_key(|(_, d, _)| *d);
        let mut plan = Vec::new();
        for (owner, _, owned) in owners {
            plan.push((owner, "setns user"));
            plan.extend(owned);
        }
        let root = open_path(
            &PathBuf::from(format!("/proc/{pid}/root")),
            libc::O_PATH | libc::O_DIRECTORY,
        );
        let last_cap = std::fs::read_to_string("/proc/sys/kernel/cap_last_cap")
            .unwrap_or_else(|e| fail("reading cap_last_cap", e))
            .trim()
            .parse()
            .unwrap_or_else(|e| fail("cap_last_cap", e));
        Entry {
            plan,
            root,
            last_cap,
            socket,
        }
    }

    /// Joins every namespace (in a forked child).
    ///
    /// # Safety
    /// Only in a single-threaded forked child.
    unsafe fn join(&self, err: RawFd) {
        for (fd, what) in &self.plan {
            // SAFETY: setns on descriptors we hold; refuse only writes and exits.
            unsafe {
                if libc::setns(fd.as_raw_fd(), 0) != 0 {
                    refuse(err, what);
                }
            }
        }
    }
}

/// Writes `mcbench sandbox: entering the box failed at <step>: errno <n>`
/// to `fd` and exits with 125, as `docker exec` does when it cannot start
/// a process. Async-signal-safe: no allocation.
unsafe fn refuse(fd: RawFd, step: &str) -> ! {
    // SAFETY: writes of buffers on the stack to a descriptor the caller holds.
    unsafe {
        let errno = *libc::__errno_location();
        let mut digits = [0u8; 12];
        let mut at = digits.len();
        let mut n = errno.unsigned_abs();
        loop {
            at -= 1;
            digits[at] = b'0' + (n % 10) as u8;
            n /= 10;
            if n == 0 {
                break;
            }
        }
        for part in [
            &b"mcbench sandbox: entering the box failed at "[..],
            step.as_bytes(),
            b": errno ",
            &digits[at..],
            b"\n",
        ] {
            libc::write(fd, part.as_ptr() as *const _, part.len());
        }
        libc::_exit(125)
    }
}

/// Runs in the forked child, single-threaded: never returns.
fn enter_and_exec(
    entry: &Entry,
    cgroups: &Cgroups,
    stdio: [RawFd; 3],
    argv: &[CString],
    env: &[CString],
    path: &[Vec<u8>],
) -> ! {
    // SAFETY: only async-signal-safe calls on descriptors and buffers made
    // before the fork; the keeper is single-threaded, so nothing else held a
    // lock at the fork.
    unsafe {
        libc::close(entry.socket);
        let err = stdio[2];
        entry.join(err);
        if libc::fchdir(entry.root.as_raw_fd()) != 0 || libc::chroot(c".".as_ptr()) != 0 {
            refuse(err, "chroot");
        }
        if libc::chdir(c"/work".as_ptr()) != 0 {
            refuse(err, "chdir /work");
        }
        // Every capability: the bounding set (needs CAP_SETPCAP, which
        // joining the user namespace gave), ambient, then the rest.
        for cap in 0..=entry.last_cap {
            libc::prctl(libc::PR_CAPBSET_DROP, cap, 0, 0, 0);
        }
        libc::prctl(
            libc::PR_CAP_AMBIENT,
            libc::PR_CAP_AMBIENT_CLEAR_ALL,
            0,
            0,
            0,
        );
        #[repr(C)]
        struct Header {
            version: u32,
            pid: i32,
        }
        #[repr(C)]
        struct Data {
            effective: u32,
            permitted: u32,
            inheritable: u32,
        }
        let header = Header {
            version: 0x2008_0522,
            pid: 0,
        };
        let data = [
            Data {
                effective: 0,
                permitted: 0,
                inheritable: 0,
            },
            Data {
                effective: 0,
                permitted: 0,
                inheritable: 0,
            },
        ];
        if libc::syscall(libc::SYS_capset, &header, data.as_ptr()) != 0 {
            refuse(err, "capset");
        }
        if libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0 {
            refuse(err, "no_new_privs");
        }
        // The two calls every program of a box is refused (`refused`): a
        // vsock socket and io_uring, as today's Docker refuses them.
        if !super::refused::install() {
            refuse(err, "the refused calls' filter");
        }
        let child = libc::fork();
        if child < 0 {
            refuse(err, "fork");
        }
        if child == 0 {
            if !move_into(cgroups.boxed.as_raw_fd(), "0") {
                refuse(err, "joining the box's cgroup");
            }
            for (to, from) in stdio.iter().enumerate() {
                if libc::dup2(*from, to as i32) < 0 {
                    libc::_exit(125);
                }
            }
            libc::syscall(libc::SYS_close_range, 3u32, u32::MAX, 0u32);
            let mut set: libc::sigset_t = std::mem::zeroed();
            libc::sigemptyset(&mut set);
            libc::sigprocmask(libc::SIG_SETMASK, &set, std::ptr::null_mut());
            for signal in 1..libc::SIGRTMAX() {
                if signal != libc::SIGKILL && signal != libc::SIGSTOP {
                    libc::signal(signal, libc::SIG_DFL);
                }
            }
            let mut args: Vec<*const libc::c_char> = argv.iter().map(|a| a.as_ptr()).collect();
            args.push(std::ptr::null());
            let mut envp: Vec<*const libc::c_char> = env.iter().map(|e| e.as_ptr()).collect();
            envp.push(std::ptr::null());
            // argv[0] by the box's PATH, as `docker exec` finds it.
            let name = argv[0].as_bytes();
            if name.contains(&b'/') {
                libc::execve(argv[0].as_ptr(), args.as_ptr(), envp.as_ptr());
            } else {
                for candidate in path {
                    libc::execve(candidate.as_ptr() as *const _, args.as_ptr(), envp.as_ptr());
                }
            }
            libc::_exit(127);
        }
        for fd in stdio {
            libc::close(fd);
        }
        let mut status = 0;
        while libc::waitpid(child, &mut status, 0) < 0 {}
        if libc::WIFEXITED(status) {
            libc::_exit(libc::WEXITSTATUS(status));
        }
        libc::_exit(128 + libc::WTERMSIG(status))
    }
}

/// Makes `fd` the exec'd program's fd 3 (bubblewrap's `--info-fd 3`): a
/// dup2, or, when it already is 3, its close-on-exec cleared (dup2 of a
/// descriptor onto itself leaves the flag). Async-signal-safe.
fn as_fd_3(fd: RawFd) -> bool {
    // SAFETY: dup2 and fcntl on a descriptor the caller holds.
    unsafe {
        if fd == 3 {
            libc::fcntl(3, libc::F_SETFD, 0) == 0
        } else {
            libc::dup2(fd, 3) == 3
        }
    }
}

/// `/tmp` and `/dev/shm` made `noexec`, as Docker mounts its `--tmpfs`
/// and `/dev/shm` (bubblewrap's `--tmpfs` has no such option): a remount of
/// bubblewrap's own tmpfs mounts from the user namespace that owns the
/// box's mount namespace.
fn no_exec_tmpfs(entry: &Entry) -> Result<(), String> {
    // SAFETY: fork of the single-threaded keeper; the child makes syscalls
    // on descriptors opened before, and exits.
    unsafe {
        let child = libc::fork();
        if child == 0 {
            libc::close(entry.socket);
            let fail =
                |step: i32| -> ! { libc::_exit(step * 30 + (*libc::__errno_location()).min(29)) };
            // Up to the mount namespace: its owner has the capability.
            for (fd, what) in &entry.plan {
                if libc::setns(fd.as_raw_fd(), 0) != 0 {
                    fail(1);
                }
                if *what == "setns mnt" {
                    break;
                }
            }
            if libc::fchdir(entry.root.as_raw_fd()) != 0 || libc::chroot(c".".as_ptr()) != 0 {
                fail(2);
            }
            let flags = libc::MS_REMOUNT
                | libc::MS_BIND
                | libc::MS_NOSUID
                | libc::MS_NODEV
                | libc::MS_NOEXEC;
            for (step, target) in [(3, c"/tmp"), (4, c"/dev/shm")] {
                if libc::mount(
                    std::ptr::null(),
                    target.as_ptr(),
                    std::ptr::null(),
                    flags,
                    std::ptr::null(),
                ) != 0
                {
                    fail(step);
                }
            }
            libc::_exit(0);
        }
        let mut status = 0;
        while libc::waitpid(child, &mut status, 0) < 0 {}
        match libc::WEXITSTATUS(status) {
            0 => Ok(()),
            code => Err(format!(
                "making /tmp and /dev/shm noexec: step {} of 4: {}",
                code / 30,
                std::io::Error::from_raw_os_error(code % 30)
            )),
        }
    }
}

/// `kill -KILL -1` from inside the box.
fn end_inside(entry: &Entry) -> ! {
    // SAFETY: as in enter_and_exec; stderr is the keeper's.
    unsafe {
        libc::close(entry.socket);
        entry.join(2);
        let child = libc::fork();
        if child == 0 {
            libc::kill(-1, libc::SIGKILL);
            libc::_exit(0);
        }
        let mut status = 0;
        while libc::waitpid(child, &mut status, 0) < 0 {}
        libc::_exit(0)
    }
}

struct Keeper {
    socket: OwnedFd,
    spec: Spec,
    cgroups: Cgroups,
    /// bubblewrap's outer process, or pasta.
    outer: Child,
    init: i32,
    entry: Entry,
    path: Vec<Vec<u8>>,
    env: Vec<CString>,
    /// Spawned programs: their process outside the box -> their id.
    running: Vec<(i32, u64)>,
    /// `End`s: their process -> their id.
    ending: Vec<(i32, u64)>,
}

fn send(socket: &OwnedFd, event: &Event) {
    let data = serde_json::to_vec(event).expect("an event serialises");
    // The app may be gone; then the next receive ends the keeper.
    let _ = control::send(socket.as_raw_fd(), &data, &[], false);
}

fn log_tail(run: &Path) -> String {
    let text = std::fs::read(run.join("box.log")).unwrap_or_default();
    let text = String::from_utf8_lossy(&text);
    let tail: String = text
        .chars()
        .rev()
        .take(2000)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    tail.trim_end().to_string()
}

fn pid_of_parent(pid: i32) -> Option<i32> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    stat.rsplit_once(") ")?.1.split(' ').nth(1)?.parse().ok()
}

/// The host pid of pid 1 of the pid namespace `inode`.
fn init_of(inode: u64) -> Option<i32> {
    let wanted = format!("pid:[{inode}]");
    std::fs::read_dir("/proc")
        .ok()?
        .flatten()
        .find_map(|entry| {
            let pid: i32 = entry.file_name().to_str()?.parse().ok()?;
            if std::fs::read_link(entry.path().join("ns/pid"))
                .ok()?
                .to_str()?
                != wanted
            {
                return None;
            }
            let status = std::fs::read_to_string(entry.path().join("status")).ok()?;
            let nspid = status
                .lines()
                .find_map(|line| line.strip_prefix("NSpid:"))?;
            (nspid.split_whitespace().last()? == "1").then_some(pid)
        })
}

/// Starts the box; answers bubblewrap's outer process (or pasta) and the
/// init's pid. bubblewrap writes the init's pid to `<run>/info` (its
/// `--info-fd`), which is polled.
fn start(spec: &Spec, cgroups: &Cgroups) -> Result<(Child, i32), String> {
    let mut spec = spec.clone();
    let at = spec
        .box_args
        .iter()
        .position(|a| a == "--chdir")
        .ok_or("the box's arguments have no --chdir")?;
    let view = [
        "--ro-bind".to_string(),
        cgroups.box_dir.to_string_lossy().into_owned(),
        "/sys/fs/cgroup".to_string(),
    ];
    spec.box_args.splice(at..at, view);
    let spec = &spec;
    let log = std::fs::File::create(spec.run.join("box.log")).map_err(|e| e.to_string())?;
    let boxed = cgroups.boxed.as_raw_fd();
    let info_path = spec.run.join("info");
    let info = std::fs::File::create(&info_path).map_err(|e| e.to_string())?;
    let info_fd = info.as_raw_fd();
    // The box's first program: bubblewrap, or pasta for the networked box
    // (the rules' runner and bubblewrap run under it).
    let mut argv: Vec<String> = match &spec.network {
        None => [
            spec.bwrap.to_string_lossy().into_owned(),
            "--info-fd".into(),
            "3".into(),
        ]
        .into_iter()
        .chain(spec.box_args.iter().cloned())
        .collect(),
        Some(network) => {
            std::fs::write(
                spec.run.join("netinit.json"),
                serde_json::to_vec(spec).expect("a spec serialises"),
            )
            .map_err(|e| e.to_string())?;
            network
                .pasta
                .iter()
                .cloned()
                .chain(["--".to_string()])
                .chain(spec.helper.iter().cloned())
                .chain([
                    "netinit".to_string(),
                    spec.run.join("netinit.json").to_string_lossy().into_owned(),
                ])
                .collect()
        }
    };
    // A guest's box: that program runs under a binfmt_misc of the box's
    // own ([`own_binfmt`], the helper's `binfmt`), so bubblewrap and, in the
    // networked box, pasta and the rules' runner sit under it. Its instance
    // is mounted on an empty folder of the box's run folder, made here, so
    // the mount's target always exists.
    if spec.binfmt {
        let folder = spec.run.join("binfmt");
        std::fs::create_dir(&folder).map_err(|e| format!("{}: {e}", folder.display()))?;
        argv = spec
            .helper
            .iter()
            .cloned()
            .chain(["binfmt".to_string(), folder.to_string_lossy().into_owned()])
            .chain(argv)
            .collect();
    }
    let mut command = mcbench_spawn::command(&argv[0]);
    command.args(&argv[1..]);
    match &spec.network {
        None => {
            // SAFETY: pre_exec in the forked child: write and dup2 are
            // async-signal-safe.
            unsafe {
                command.pre_exec(move || {
                    if !move_into(boxed, "0") || !as_fd_3(info_fd) {
                        return Err(std::io::Error::last_os_error());
                    }
                    Ok(())
                });
            }
        }
        Some(_) => {
            // SAFETY: pre_exec in the forked child: write and prctl are
            // async-signal-safe. pasta dies with the keeper.
            unsafe {
                command.pre_exec(move || {
                    if !move_into(boxed, "0")
                        || libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL, 0, 0, 0) != 0
                    {
                        return Err(std::io::Error::last_os_error());
                    }
                    Ok(())
                });
            }
        }
    }
    let mut outer = command
        .stdin(Stdio::null())
        .stdout(log.try_clone().map_err(|e| e.to_string())?)
        .stderr(log)
        .spawn()
        .map_err(|e| format!("{}: {e}", command.get_program().to_string_lossy()))?;
    drop(info);
    let deadline = Instant::now() + Duration::from_secs(30);
    let text = loop {
        if let Ok(text) = std::fs::read_to_string(&info_path)
            && text.contains('}')
        {
            break text;
        }
        if matches!(outer.try_wait(), Ok(Some(_))) || Instant::now() > deadline {
            break String::new();
        }
        std::thread::sleep(Duration::from_millis(1));
    };
    // `child-pid` is a pid in bubblewrap's own pid namespace, which pasta
    // gives one of its own: the init is found on the host as pid 1 of the
    // box's pid namespace.
    let init = serde_json::from_str::<serde_json::Value>(&text)
        .ok()
        .and_then(|v| v["pid-namespace"].as_u64())
        .and_then(init_of);
    let Some(init) = init else {
        let _ = outer.kill();
        let _ = outer.wait();
        return Err(log_tail(&spec.run));
    };
    // bubblewrap writes the info right after it clones the box's init, before
    // the init has its mounts and root: the box is ready once its init runs
    // `sleep infinity` (the box arguments' shell has exec'd it).
    let command = "sleep\0infinity\0";
    loop {
        if std::fs::read(format!("/proc/{init}/cmdline"))
            .is_ok_and(|cmdline| cmdline == command.as_bytes())
        {
            break;
        }
        if matches!(outer.try_wait(), Ok(Some(_))) || Instant::now() > deadline {
            let _ = outer.kill();
            let _ = outer.wait();
            return Err(log_tail(&spec.run));
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    // bubblewrap's outer processes (and pasta) count against nothing of the box.
    let keeper = cgroups.keeper.as_raw_fd();
    let mut outside = vec![outer.id() as i32];
    if spec.network.is_some()
        && let Some(monitor) = pid_of_parent(init)
    {
        outside.push(monitor);
    }
    for pid in outside {
        move_into(keeper, &pid.to_string());
    }
    Ok((outer, init))
}

/// `<helper> keep`: the keeper's main. Its stdin is the control socket.
pub fn keep() -> ! {
    // SAFETY: fd 0 is the control socket the app gave us.
    let socket = unsafe { OwnedFd::from_raw_fd(0) };
    let spec = match control::receive(socket.as_raw_fd(), false) {
        Ok(Some((data, _))) => match serde_json::from_slice::<Request>(&data) {
            Ok(Request::Start(spec)) => *spec,
            other => fail("the first message", format!("{other:?}")),
        },
        other => fail("the first message", format!("{other:?}")),
    };
    let cgroups = Cgroups::make();
    super::refused::prepare();
    if let Err(message) = set_rlimits() {
        send(&socket, &Event::Failed { message });
        std::process::exit(1)
    }
    let (outer, init) = match start(&spec, &cgroups) {
        Ok(started) => started,
        Err(message) => {
            send(&socket, &Event::Failed { message });
            std::process::exit(1)
        }
    };
    let entry = Entry::open(init, socket.as_raw_fd());
    if let Err(message) = no_exec_tmpfs(&entry) {
        send(&socket, &Event::Failed { message });
        // SAFETY: a signal to the box's init, in a namespace we own.
        unsafe { libc::kill(init, libc::SIGKILL) };
        std::process::exit(1)
    }
    let env: Vec<CString> = spec
        .env
        .iter()
        .map(|e| CString::new(e.as_str()).expect("no NUL in the environment"))
        .collect();
    let path = spec
        .env
        .iter()
        .find_map(|e| e.strip_prefix("PATH="))
        .unwrap_or("/usr/bin:/bin")
        .split(':')
        .map(|dir| dir.as_bytes().to_vec())
        .collect();
    let mut keeper = Keeper {
        socket,
        spec,
        cgroups,
        outer,
        init,
        entry,
        path,
        env,
        running: Vec::new(),
        ending: Vec::new(),
    };
    send(&keeper.socket, &Event::Ready { pid: init });
    keeper.serve()
}

impl Keeper {
    fn serve(&mut self) -> ! {
        // SIGCHLD through a signalfd, beside the socket in one poll.
        // SAFETY: signal mask and signalfd calls on this single thread.
        let signals = unsafe {
            let mut set: libc::sigset_t = std::mem::zeroed();
            libc::sigemptyset(&mut set);
            libc::sigaddset(&mut set, libc::SIGCHLD);
            libc::sigprocmask(libc::SIG_BLOCK, &set, std::ptr::null_mut());
            let fd = libc::signalfd(-1, &set, libc::SFD_CLOEXEC | libc::SFD_NONBLOCK);
            if fd < 0 {
                fail("signalfd", std::io::Error::last_os_error());
            }
            OwnedFd::from_raw_fd(fd)
        };
        loop {
            self.reap();
            let mut polls = [
                libc::pollfd {
                    fd: self.socket.as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                },
                libc::pollfd {
                    fd: signals.as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                },
            ];
            // SAFETY: poll on two descriptors we hold.
            let ready = unsafe { libc::poll(polls.as_mut_ptr(), 2, -1) };
            if ready < 0 {
                continue;
            }
            if polls[1].revents != 0 {
                let mut info = [0u8; 128];
                // SAFETY: draining the signalfd into a buffer of its record size.
                while unsafe {
                    libc::read(signals.as_raw_fd(), info.as_mut_ptr() as *mut _, info.len())
                } > 0
                {}
            }
            if polls[0].revents != 0 {
                match control::receive(self.socket.as_raw_fd(), false) {
                    Ok(Some((data, fds))) => self.handle(&data, fds),
                    _ => self.finish(),
                }
            }
        }
    }

    fn handle(&mut self, data: &[u8], fds: Vec<OwnedFd>) {
        let request: Request =
            serde_json::from_slice(data).unwrap_or_else(|e| fail("a request", e));
        match request {
            Request::Start(_) => fail("a request", "a second Start"),
            Request::Spawn { id, argv } => {
                let Ok(stdio) = <[OwnedFd; 3]>::try_from(fds) else {
                    send(
                        &self.socket,
                        &Event::SpawnFailed {
                            id,
                            message: "a spawn needs three descriptors".into(),
                        },
                    );
                    return;
                };
                let argv: Vec<CString> =
                    match argv.iter().map(|a| CString::new(a.as_str())).collect() {
                        Ok(argv) => argv,
                        Err(_) => {
                            send(
                                &self.socket,
                                &Event::SpawnFailed {
                                    id,
                                    message: "an argument holds a NUL byte".into(),
                                },
                            );
                            return;
                        }
                    };
                let candidates: Vec<Vec<u8>> = self
                    .path
                    .iter()
                    .map(|dir| {
                        let mut path = dir.clone();
                        path.push(b'/');
                        path.extend_from_slice(argv[0].as_bytes());
                        path.push(0);
                        path
                    })
                    .collect();
                // SAFETY: fork of this single-threaded process; the child only
                // runs enter_and_exec, which never returns.
                let pid = unsafe { libc::fork() };
                if pid == 0 {
                    enter_and_exec(
                        &self.entry,
                        &self.cgroups,
                        stdio.each_ref().map(|fd| fd.as_raw_fd()),
                        &argv,
                        &self.env,
                        &candidates,
                    );
                }
                drop(stdio);
                if pid < 0 {
                    send(
                        &self.socket,
                        &Event::SpawnFailed {
                            id,
                            message: std::io::Error::last_os_error().to_string(),
                        },
                    );
                } else {
                    self.running.push((pid, id));
                }
            }
            Request::Kill { id } => {
                if let Some((pid, _)) = self.running.iter().find(|(_, of)| *of == id) {
                    // SAFETY: a signal to our own child, not yet reaped.
                    unsafe { libc::kill(*pid, libc::SIGKILL) };
                }
            }
            Request::End { id } => {
                // SAFETY: as for Spawn.
                let pid = unsafe { libc::fork() };
                if pid == 0 {
                    end_inside(&self.entry);
                }
                if pid < 0 {
                    send(&self.socket, &Event::Ended { id });
                } else {
                    self.ending.push((pid, id));
                }
            }
        }
    }

    /// Reaps every ended child: programs, `End`s, and the box itself.
    fn reap(&mut self) {
        loop {
            let mut status = 0;
            // SAFETY: waitpid for any child, without blocking.
            let pid = unsafe { libc::waitpid(-1, &mut status, libc::WNOHANG) };
            if pid <= 0 {
                return;
            }
            if pid == self.outer.id() as i32 {
                // The box ended under us (its init was killed, or pasta).
                self.finish();
            }
            if let Some(at) = self.running.iter().position(|(p, _)| *p == pid) {
                let (_, id) = self.running.remove(at);
                let code = if libc::WIFEXITED(status) {
                    libc::WEXITSTATUS(status)
                } else {
                    -1
                };
                send(&self.socket, &Event::Exited { id, code });
            }
            if let Some(at) = self.ending.iter().position(|(p, _)| *p == pid) {
                let (_, id) = self.ending.remove(at);
                send(&self.socket, &Event::Ended { id });
            }
        }
    }

    /// Kills the box and every program's outer process, and exits.
    fn finish(&mut self) -> ! {
        // SAFETY: signals to the box's init (in a namespace we own) and to
        // our own children.
        unsafe {
            libc::kill(self.init, libc::SIGKILL);
            libc::kill(self.outer.id() as i32, libc::SIGKILL);
            for (pid, _) in &self.running {
                libc::kill(*pid, libc::SIGKILL);
            }
        }
        let _ = self.outer.wait();
        let _ = std::fs::remove_dir_all(&self.spec.run);
        std::process::exit(0)
    }
}

/// The host's device nodes bound into the rules' runner's `/dev`, as
/// bubblewrap's `--dev` gives them.
const RULES_DEVICES: [&str; 5] = ["null", "zero", "full", "random", "urandom"];

/// Today's rules (`sh -c EGRESS_RULES`), run by netinit itself rather than
/// under `/usr/bin/bwrap`, which an AppArmor profile for bwrap (Ubuntu
/// 26.04's `bwrap-userns-restrict`) puts in `unpriv_bwrap`, refusing
/// `CAP_NET_ADMIN`. netinit is root of pasta's user namespace, in pasta's
/// network namespace; a child of it unshares a mount, PID, IPC and UTS
/// namespace (owned by pasta's user namespace), and its child, PID 1
/// there, mounts what bubblewrap's arguments did (tmpfs `/run`, `/tmp` and
/// `/dev` with the host's null, zero, full, random and urandom, its own
/// `/proc`, the box's `resolv.conf` on `/etc/resolv.conf`; nothing of it is
/// seen outside; the root filesystem itself read-only, as `--ro-bind` gave
/// it), chroots to the root filesystem, keeps only
/// `CAP_NET_ADMIN` (bounding, inheritable, permitted and effective sets,
/// no ambient), sets `no_new_privs` and a parent-death signal, and execs
/// `sh -c` with only `PATH` in its environment. Its exit status, as
/// `waitpid` gives it.
pub fn run_rules(rules: &Rules) -> std::process::ExitStatus {
    use std::os::unix::process::ExitStatusExt;
    let c = |s: &str| CString::new(s).expect("no NUL in a path or the script");
    let at = |rel: &str| c(&format!("{}/{rel}", rules.rootfs.trim_end_matches('/')));
    let root = c(&rules.rootfs);
    let resolv = c(&rules.resolv);
    let resolv_at = at("etc/resolv.conf");
    let tmpfs = [at("run"), at("tmp"), at("dev")];
    let devices: Vec<(CString, CString)> = RULES_DEVICES
        .iter()
        .map(|name| (c(&format!("/dev/{name}")), at(&format!("dev/{name}"))))
        .collect();
    let proc_at = at("proc");
    let argv = [c("sh"), c("-c"), c(&rules.script)];
    let env = [c(&format!("PATH={}", rules.path))];
    let sh: Vec<CString> = rules
        .path
        .split(':')
        .map(|dir| c(&format!("{dir}/sh")))
        .collect();
    let last_cap: i32 = std::fs::read_to_string("/proc/sys/kernel/cap_last_cap")
        .unwrap_or_else(|e| fail("reading cap_last_cap", e))
        .trim()
        .parse()
        .unwrap_or_else(|e| fail("cap_last_cap", e));
    // SAFETY: netinit is single-threaded here; between fork and exec the
    // children make only syscalls on buffers made before the fork, and on
    // failure write a fixed message and _exit.
    unsafe {
        let outer = libc::fork();
        if outer < 0 {
            fail("forking the rules' runner", std::io::Error::last_os_error());
        }
        if outer == 0 {
            libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL, 0, 0, 0);
            if libc::unshare(
                libc::CLONE_NEWNS | libc::CLONE_NEWPID | libc::CLONE_NEWIPC | libc::CLONE_NEWUTS,
            ) != 0
            {
                refuse(2, "the rules' namespaces");
            }
            let inner = libc::fork();
            if inner < 0 {
                refuse(2, "the rules' fork");
            }
            if inner == 0 {
                libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL, 0, 0, 0);
                let none = std::ptr::null::<libc::c_char>();
                if libc::mount(
                    none,
                    c"/".as_ptr(),
                    none,
                    libc::MS_REC | libc::MS_PRIVATE,
                    std::ptr::null(),
                ) != 0
                {
                    refuse(2, "the rules' mount propagation");
                }
                // The root filesystem read-only, as bubblewrap's `--ro-bind`
                // gave it: bound onto itself here, and that bind remounted
                // read-only with the flags its filesystem is mounted with
                // (in a user namespace's mount namespace they are locked,
                // and a remount that drops one is refused).
                if libc::mount(
                    root.as_ptr(),
                    root.as_ptr(),
                    none,
                    libc::MS_BIND | libc::MS_REC,
                    std::ptr::null(),
                ) != 0
                {
                    refuse(2, "the rules' root bind");
                }
                let mut vfs: libc::statvfs = std::mem::zeroed();
                if libc::statvfs(root.as_ptr(), &mut vfs) != 0 {
                    refuse(2, "the rules' root flags");
                }
                let mut read_only = libc::MS_BIND | libc::MS_REMOUNT | libc::MS_RDONLY;
                for (held, flag) in [
                    (ST_NOSUID, libc::MS_NOSUID),
                    (ST_NODEV, libc::MS_NODEV),
                    (ST_NOEXEC, libc::MS_NOEXEC),
                    (ST_NOATIME, libc::MS_NOATIME),
                    (ST_NODIRATIME, libc::MS_NODIRATIME),
                    (ST_RELATIME, libc::MS_RELATIME),
                ] {
                    if vfs.f_flag & held != 0 {
                        read_only |= flag;
                    }
                }
                if libc::mount(none, root.as_ptr(), none, read_only, std::ptr::null()) != 0 {
                    refuse(2, "the rules' read-only root");
                }
                let nosuid = libc::MS_NOSUID | libc::MS_NODEV;
                for target in &tmpfs {
                    if libc::mount(
                        c"tmpfs".as_ptr(),
                        target.as_ptr(),
                        c"tmpfs".as_ptr(),
                        nosuid,
                        c"mode=0755".as_ptr() as *const _,
                    ) != 0
                    {
                        refuse(2, "the rules' tmpfs");
                    }
                }
                for (from, to) in &devices {
                    let fd = libc::open(
                        to.as_ptr(),
                        libc::O_CREAT | libc::O_WRONLY | libc::O_CLOEXEC,
                        0o666,
                    );
                    if fd < 0 {
                        refuse(2, "the rules' /dev");
                    }
                    libc::close(fd);
                    if libc::mount(
                        from.as_ptr(),
                        to.as_ptr(),
                        none,
                        libc::MS_BIND,
                        std::ptr::null(),
                    ) != 0
                    {
                        refuse(2, "the rules' /dev");
                    }
                }
                if libc::mount(
                    c"proc".as_ptr(),
                    proc_at.as_ptr(),
                    c"proc".as_ptr(),
                    nosuid | libc::MS_NOEXEC,
                    std::ptr::null(),
                ) != 0
                {
                    refuse(2, "the rules' /proc");
                }
                if libc::mount(
                    resolv.as_ptr(),
                    resolv_at.as_ptr(),
                    none,
                    libc::MS_BIND,
                    std::ptr::null(),
                ) != 0
                {
                    refuse(2, "the rules' resolv.conf");
                }
                if libc::chroot(root.as_ptr()) != 0 || libc::chdir(c"/".as_ptr()) != 0 {
                    refuse(2, "the rules' chroot");
                }
                // Every drop is checked: this is uid 0 of pasta's
                // namespace, whose permitted set after `execve` is its
                // inheritable set or its bounding set, so a capability left
                // in the bounding set would come back to `sh`.
                for cap in 0..=last_cap {
                    if cap != CAP_NET_ADMIN as i32
                        && libc::prctl(libc::PR_CAPBSET_DROP, cap, 0, 0, 0) != 0
                    {
                        refuse(2, "the rules' bounding set");
                    }
                }
                if libc::prctl(
                    libc::PR_CAP_AMBIENT,
                    libc::PR_CAP_AMBIENT_CLEAR_ALL,
                    0,
                    0,
                    0,
                ) != 0
                {
                    refuse(2, "the rules' ambient set");
                }
                #[repr(C)]
                struct Header {
                    version: u32,
                    pid: i32,
                }
                #[repr(C)]
                struct Data {
                    effective: u32,
                    permitted: u32,
                    inheritable: u32,
                }
                let only = 1u32 << CAP_NET_ADMIN;
                let header = Header {
                    version: 0x2008_0522,
                    pid: 0,
                };
                let data = [
                    Data {
                        effective: only,
                        permitted: only,
                        inheritable: only,
                    },
                    Data {
                        effective: 0,
                        permitted: 0,
                        inheritable: 0,
                    },
                ];
                if libc::syscall(libc::SYS_capset, &header, data.as_ptr()) != 0 {
                    refuse(2, "the rules' capabilities");
                }
                if libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0 {
                    refuse(2, "the rules' no_new_privs");
                }
                let args: Vec<*const libc::c_char> = argv
                    .iter()
                    .map(|a| a.as_ptr())
                    .chain([std::ptr::null()])
                    .collect();
                let envp: Vec<*const libc::c_char> = env
                    .iter()
                    .map(|e| e.as_ptr())
                    .chain([std::ptr::null()])
                    .collect();
                for candidate in &sh {
                    libc::execve(candidate.as_ptr(), args.as_ptr(), envp.as_ptr());
                }
                refuse(2, "the rules' exec of sh");
            }
            let mut status = 0;
            while libc::waitpid(inner, &mut status, 0) < 0 {}
            if libc::WIFEXITED(status) {
                libc::_exit(libc::WEXITSTATUS(status));
            }
            libc::_exit(128 + libc::WTERMSIG(status))
        }
        let mut status = 0;
        while libc::waitpid(outer, &mut status, 0) < 0 {}
        std::process::ExitStatus::from_raw(status)
    }
}

/// `CAP_NET_ADMIN`'s bit (`linux/capability.h`).
const CAP_NET_ADMIN: u32 = 12;

/// `statvfs`'s mount flags (`f_flag`, the kernel's `ST_*` of
/// `linux/statfs.h`, the same on every architecture). Named here because
/// the `libc` crate has `ST_RELATIME` for glibc only, and the guest agents
/// (the macOS VM's and the WSL distro's) are built for musl.
const ST_NOSUID: libc::c_ulong = 0x0002;
const ST_NODEV: libc::c_ulong = 0x0004;
const ST_NOEXEC: libc::c_ulong = 0x0008;
const ST_NOATIME: libc::c_ulong = 0x0400;
const ST_NODIRATIME: libc::c_ulong = 0x0800;
const ST_RELATIME: libc::c_ulong = 0x1000;

/// `<helper> netinit <spec>`: run by pasta as root of its user namespace in
/// its network namespace. IPv6 off, today's rules ([`run_rules`]), then the
/// box.
pub fn netinit(spec_file: &Path) -> ! {
    let spec: Spec = serde_json::from_slice(
        &std::fs::read(spec_file).unwrap_or_else(|e| fail("reading the spec", e)),
    )
    .unwrap_or_else(|e| fail("the spec", e));
    let network = spec
        .network
        .as_ref()
        .unwrap_or_else(|| fail("the spec", "no network part"));
    for (path, value) in &network.sysctls {
        std::fs::write(path, value).unwrap_or_else(|e| fail(&format!("writing {path}"), e));
    }
    // Ping sockets for the box's group (the one group mapped here).
    std::fs::write("/proc/sys/net/ipv4/ping_group_range", "0 0")
        .unwrap_or_else(|e| fail("ping_group_range", e));
    let rules = run_rules(&network.rules);
    if !rules.success() {
        fail("the egress rules", format!("iptables exited with {rules}"));
    }
    let info = std::fs::OpenOptions::new()
        .write(true)
        .open(spec.run.join("info"))
        .unwrap_or_else(|e| fail("the info file", e));
    let fd = info.as_raw_fd();
    let mut command = mcbench_spawn::command(&spec.bwrap);
    command.args(["--info-fd", "3"]).args(&spec.box_args);
    // SAFETY: dup2 in the forked child before exec.
    unsafe {
        command.pre_exec(move || {
            if !as_fd_3(fd) {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let error = command.exec();
    let _ = std::io::stderr()
        .write_all(format!("mcbench sandbox netinit: bwrap: {error}\n").as_bytes());
    std::process::exit(1)
}
