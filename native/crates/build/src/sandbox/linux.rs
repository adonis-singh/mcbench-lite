//! Linux: `prlimit` and bubblewrap with `build-sandbox.js`'s arguments.
//!
//! The child runs in new user, mount, PID, network, IPC, UTS and cgroup
//! namespaces whose root is an empty tmpfs holding, read-only, `/usr`, the
//! dynamic loader's cache, the time zone and the child's program
//! (`mcbench-build-child`, in place of Node's binary and the five sources,
//! which the program embeds), its own
//! `/proc` and a minimal `/dev`, under `RLIMIT_DATA` of the heap cap plus
//! 2 GiB and no core dumps. A test holds the arguments against
//! `server/build-sandbox.js`.

use std::ffi::OsStr;
use std::path::Path;

use crate::process::ChildProcess;
use crate::{ChildSandbox, HEAP_LIMIT_MB};

/// `build-sandbox.js` `DATA_LIMIT_BYTES`.
pub const DATA_LIMIT_BYTES: u64 = (HEAP_LIMIT_MB + 2048) * 1024 * 1024;

/// The host entries bound as the host has them (`hostEntry`): symlinks into
/// `/usr` on merged-`/usr` systems, binds otherwise, nothing when missing.
pub const HOST_ENTRIES: [&str; 6] = [
    "/lib",
    "/lib64",
    "/bin",
    "/sbin",
    "/etc/ld.so.cache",
    "/etc/localtime",
];

fn host_entry(file: &str) -> Vec<String> {
    let Ok(stat) = std::fs::symlink_metadata(file) else {
        return Vec::new();
    };
    if stat.file_type().is_symlink() {
        let target = std::fs::read_link(file).expect("a symlink reads");
        vec![
            "--symlink".into(),
            target.to_str().expect("a UTF-8 link").into(),
            file.into(),
        ]
    } else {
        vec!["--ro-bind".into(), file.into(), file.into()]
    }
}

/// The arguments of `/usr/bin/prlimit` that start `program`; fails when
/// `program` does not exist.
pub fn args(program: &Path) -> std::io::Result<Vec<String>> {
    let given = program
        .to_str()
        .expect("the child's path is UTF-8")
        .to_string();
    let real = std::fs::canonicalize(program)?;
    let real = real
        .to_str()
        .expect("the child's path is UTF-8")
        .to_string();
    Ok(args_with(&given, &real, host_entry))
}

/// [`args`] for a program at `given` (its real path `real`) on a Linux whose
/// [`HOST_ENTRIES`] are what `host_entry` says (`--symlink`, `--ro-bind` or
/// nothing for each): this machine's for [`args`], the WSL2 distro's for
/// the Windows sandbox, which builds the command outside the distro.
pub fn args_with(given: &str, real: &str, host_entry: impl Fn(&str) -> Vec<String>) -> Vec<String> {
    let mut args: Vec<String> = [
        format!("--data={DATA_LIMIT_BYTES}"),
        "--core=0".into(),
        "--".into(),
        "/usr/bin/bwrap".into(),
        "--unshare-user".into(),
        "--unshare-ipc".into(),
        "--unshare-pid".into(),
        "--unshare-net".into(),
        "--unshare-uts".into(),
        "--unshare-cgroup".into(),
        "--die-with-parent".into(),
        "--new-session".into(),
        "--as-pid-1".into(),
        "--ro-bind".into(),
        "/usr".into(),
        "/usr".into(),
    ]
    .into();
    args.extend(HOST_ENTRIES.iter().flat_map(|file| host_entry(file)));
    args.extend(["--ro-bind".into(), real.to_string(), given.to_string()]);
    args.extend(
        [
            "--proc",
            "/proc",
            "--dev",
            "/dev",
            "--remount-ro",
            "/dev",
            "--remount-ro",
            "/",
            "--chdir",
            "/",
            "--",
        ]
        .map(String::from),
    );
    args.push(given.to_string());
    args
}

/// `prlimit --data=... --core=0 -- bwrap ... -- <program>`.
pub fn command(program: &Path) -> std::io::Result<std::process::Command> {
    let mut command = mcbench_spawn::command("/usr/bin/prlimit");
    command.args(args(program)?);
    Ok(command)
}

/// The kernel or a security module refuses bubblewrap the user namespace it
/// needs: the sentence `bash` (the sandbox crate's boxes) and `build` (this
/// child) answer. The Ubuntu remedy is Ubuntu's own profile for bubblewrap,
/// `bwrap-userns-restrict`: on 24.04 it is only in the package
/// `apparmor-profiles` (`/usr/share/apparmor/extra-profiles/`), whose
/// installation would also load some 25 other profiles in enforce mode, so
/// the sentence takes the one file out of the downloaded package; 26.04's
/// `apparmor` package ships and loads it as
/// `/etc/apparmor.d/bwrap-userns-restrict`. The check needs `--ro-bind / /`
/// (without a root the new namespace has no `true` to run, so `bwrap
/// --unshare-user true` fails where bubblewrap works) and `--unshare-net`
/// (24.04 refuses the loopback address first when the network is unshared).
pub const USERNS_MISSING: &str = "The model's bash shell and build tool run in a bubblewrap sandbox, and this system does not let unprivileged users create user namespaces, which bubblewrap needs: allow them (on Ubuntu 23.10 and later by loading Ubuntu's AppArmor profile for bubblewrap with `apt-get download apparmor-profiles`, `dpkg -x apparmor-profiles_*.deb apparmor-profiles`, `sudo cp apparmor-profiles/usr/share/apparmor/extra-profiles/bwrap-userns-restrict /etc/apparmor.d/` and `sudo apparmor_parser -r /etc/apparmor.d/bwrap-userns-restrict`, or with `sudo sysctl kernel.apparmor_restrict_unprivileged_userns=0`; elsewhere with `sudo sysctl kernel.unprivileged_userns_clone=1` or a non-zero `user.max_user_namespaces`), so that `bwrap --unshare-user --unshare-net --ro-bind / / true` works, then try again";

/// [`USERNS_MISSING`] when bubblewrap's own words on stderr (a line it
/// starts with `bwrap: `) say it was refused a namespace or what it does in
/// one; `None` for any other failure, among them a crash of the program it
/// ran.
pub fn classify_bwrap_failure(stderr: &str) -> Option<&'static str> {
    let refused = [
        "No permissions to create",
        "setting up uid map",
        "Creating new namespace failed",
        "unshare",
        "Operation not permitted",
        "Permission denied",
    ];
    stderr
        .lines()
        .filter_map(|line| line.strip_prefix("bwrap: "))
        .any(|said| refused.iter().any(|words| said.contains(words)))
        .then_some(USERNS_MISSING)
}

/// The child under `prlimit` and bubblewrap ([`command`]).
pub struct Bubblewrap;

impl ChildSandbox for Bubblewrap {
    fn spawn(&self, program: &Path, env: &[(&str, &OsStr)]) -> std::io::Result<ChildProcess> {
        ChildProcess::start(command(program)?, env)
    }

    fn refusal(&self, stderr: &str) -> Option<&'static str> {
        classify_bwrap_failure(stderr)
    }

    fn spare_during_build(&self) -> bool {
        // The spawn leaves the build's path once its request is written, and
        // a build that follows straight on finds the process ready, which
        // hides bwrap's ~0.7 ms of extra startup (build-sandbox.js).
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Each refusal bubblewrap gives, verbatim: Ubuntu 24.04's with no
    // profile for bwrap (kernel.apparmor_restrict_unprivileged_userns=1,
    // read in Ubuntu's 24.04.5 cloud image with bubblewrap
    // 0.9.0-1ubuntu0.3, 2026-11-20): the box's probe without a network
    // namespace, then the build child's with one; and a kernel that refuses
    // unprivileged user namespaces outright (Debian's sysctl at 0).
    #[test]
    fn each_refusal_bubblewrap_gives_is_userns_missing() {
        for stderr in [
            "bwrap: setting up uid map: Permission denied\n",
            "bwrap: loopback: Failed RTM_NEWADDR: Operation not permitted\n",
            "bwrap: No permissions to create a new namespace, likely because the kernel does not allow non-privileged user namespaces. On e.g. debian this can be enabled with 'sysctl kernel.unprivileged_userns_clone=1'.\n",
        ] {
            assert_eq!(
                classify_bwrap_failure(stderr),
                Some(USERNS_MISSING),
                "{stderr}"
            );
            assert_eq!(Bubblewrap.refusal(stderr), Some(USERNS_MISSING), "{stderr}");
        }
    }

    // What is not a refusal: bubblewrap's other failures (the old check's
    // own where bubblewrap works: nothing is bound into the new root, so
    // there is no `true`), and the child's crash, which may use the same
    // words but not as bubblewrap.
    #[test]
    fn other_failures_and_the_child_s_crash_are_not_userns_missing() {
        for stderr in [
            "bwrap: execvp true: No such file or directory\n",
            "bwrap: Can't find source path /nope: No such file or directory\n",
            "Error: EPERM: Operation not permitted, open '/x'\n",
            "#\n# Fatal error in , line 0\n# unshare failed: Permission denied\n",
        ] {
            assert_eq!(classify_bwrap_failure(stderr), None, "{stderr}");
        }
    }

    // The sentence's remedies in order, each command as it is run: Ubuntu's
    // own profile for bubblewrap taken out of `apparmor-profiles` without
    // installing it, the sysctl, the other distributions'; then a check
    // that fails while the namespace is refused and works once bubblewrap
    // can make a box.
    #[test]
    fn the_sentence_names_ubuntu_s_profile_first_then_the_sysctls_and_a_check_that_separates() {
        let commands: Vec<&str> = USERNS_MISSING.split('`').skip(1).step_by(2).collect();
        assert_eq!(
            commands,
            [
                "apt-get download apparmor-profiles",
                "dpkg -x apparmor-profiles_*.deb apparmor-profiles",
                "sudo cp apparmor-profiles/usr/share/apparmor/extra-profiles/bwrap-userns-restrict /etc/apparmor.d/",
                "sudo apparmor_parser -r /etc/apparmor.d/bwrap-userns-restrict",
                "sudo sysctl kernel.apparmor_restrict_unprivileged_userns=0",
                "sudo sysctl kernel.unprivileged_userns_clone=1",
                "user.max_user_namespaces",
                "bwrap --unshare-user --unshare-net --ro-bind / / true",
            ]
        );
        assert!(USERNS_MISSING.starts_with("The model's bash shell and build tool run in a bubblewrap sandbox, and this system does not let unprivileged users create user namespaces, which bubblewrap needs: allow them (on Ubuntu 23.10 and later by loading Ubuntu's AppArmor profile for bubblewrap with "));
        assert!(USERNS_MISSING.ends_with(
            ", so that `bwrap --unshare-user --unshare-net --ro-bind / / true` works, then try again"
        ));
    }
}
