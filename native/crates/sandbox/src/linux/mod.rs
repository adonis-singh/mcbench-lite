//! Linux: bubblewrap on the host kernel, one long-lived box per chat (and
//! one networked box per chat) in a systemd user scope with today's limits,
//! each started and entered by its keeper (the module `keeper`, which
//! exists on Linux only, so it is named here without a link).
//!
//! The persistent box is today's `docker run --network none --memory 512m
//! --memory-swap 512m --pids-limit 256 --cpus 2 --read-only --tmpfs
//! /tmp:size=256m --cap-drop all --security-opt no-new-privileges --user
//! <uid>:<gid> -v <work>:/work <image>` with `sleep infinity` as its pid 1;
//! the networked box is the same with pasta's network behind today's rules.
//! What a model can see differ from Docker's only where bubblewrap's
//! containment differs from Docker's (`tests/docker_diff.rs` lists each).
//!
//! The real code is under `cfg(target_os = "linux")`; elsewhere the types
//! exist and their bodies are `todo!` (the guests run this module through
//! [`crate::agent`]).

#[cfg(target_os = "linux")]
pub mod control;
#[cfg(target_os = "linux")]
mod imp;
#[cfg(target_os = "linux")]
pub mod keeper;
#[cfg(target_os = "linux")]
pub use imp::user_scope_works;
#[cfg(target_os = "linux")]
pub mod refused;
pub mod rootfs;

use std::ffi::OsString;
use std::path::PathBuf;

/// bubblewrap is not on `PATH` (today's `DOCKER_MISSING`).
pub const BWRAP_MISSING: &str = "The model's bash shell runs in a bubblewrap sandbox, and bubblewrap is not installed: install it with your distribution's package manager (the package is called bubblewrap), so that `bwrap --version` works, then try again";
/// The kernel or a security module refuses unprivileged user namespaces,
/// and its reading of bubblewrap's stderr: the build child's, so that
/// `bash` and `build` answer one sentence by one rule.
pub use mcbench_build::sandbox::linux::{USERNS_MISSING, classify_bwrap_failure};
/// No user systemd session (today's `DOCKER_NOT_RUNNING`).
pub const SESSION_MISSING: &str = "The model's bash shell runs in a bubblewrap sandbox whose limits are set by systemd, and this login has no systemd user session: log in through a session that starts `systemd --user` (desktop and ssh logins on systemd distributions do), so that `systemd-run --user --scope true` works, then try again";

/// The networked box's addresses on pasta's network (the netspike's): its
/// own, the gateway, and the DNS address pasta forwards to the host's
/// resolver.
pub const NETWORK: [(&str, &str); 3] = [
    ("address", "10.0.2.100"),
    ("gateway", "10.0.2.2"),
    ("dns", "10.0.2.3"),
];

/// Today's `EGRESS_RULES`, generated from `server/sandbox.js`.
pub const EGRESS_RULES: &str = include_str!("../../contract/egress-rules.sh");
/// Today's `IPV6_OFF`, generated from `server/sandbox.js`.
pub const IPV6_OFF: &str = include_str!("../../contract/ipv6-off.json");

/// The `/proc/sys` files and values of [`IPV6_OFF`].
pub fn ipv6_off() -> Vec<(String, String)> {
    let map: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(IPV6_OFF).expect("ipv6-off.json is an object");
    map.into_iter()
        .map(|(key, value)| {
            (
                format!("/proc/sys/{}", key.replace('.', "/")),
                value
                    .as_str()
                    .expect("a sysctl value is a string")
                    .to_string(),
            )
        })
        .collect()
}

/// A program on a `PATH`.
pub fn find_program(name: &str, path: &std::ffi::OsStr) -> Option<PathBuf> {
    std::env::split_paths(path)
        .map(|dir| dir.join(name))
        .find(|file| {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                file.metadata()
                    .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
            }
            #[cfg(not(unix))]
            {
                file.is_file()
            }
        })
}

/// The Linux sandbox's boxes.
pub struct Bubblewrap {
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    config: BubblewrapConfig,
    #[cfg(target_os = "linux")]
    state: imp::State,
}

/// Who gives each box's keeper a cgroup v2 folder of its own that it may
/// divide (the keeper's `keeper` and `box` cgroups and today's limits are
/// the same code either way, in the module `keeper`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Delegation {
    /// A Linux host: the keeper is started by `systemd-run --user --scope
    /// -p Delegate=yes`, and the user's systemd manager delegates the
    /// scope (no user session: [`SESSION_MISSING`]).
    UserScope,
    /// A guest (the macOS VM, the WSL2 distro): the guest's init made this
    /// cgroup folder, enabled the `memory`, `pids` and `cpu` controllers in
    /// it and gave it to the agent's user; each box's keeper starts in a
    /// child folder of it made by the agent, removed when the box ends. The
    /// agent itself runs in a leaf beside them (cgroup v2 lets only a
    /// writer of the common ancestor's `cgroup.procs` move a process).
    Subtree(PathBuf),
}

/// What [`Bubblewrap`] is made from.
#[derive(Clone, Debug)]
pub struct BubblewrapConfig {
    /// The data folder's `sandbox` folder (the root filesystem is unpacked
    /// into `.rootfs-<sha>` there, boxes keep their files in `.run`).
    pub root: PathBuf,
    /// The root filesystem's tar.zst and its sha256.
    pub rootfs: (PathBuf, String),
    /// The program and arguments that run [`crate::agent::main`].
    pub helper: Vec<OsString>,
    /// Who delegates each box's cgroup.
    pub delegation: Delegation,
}

impl Bubblewrap {
    /// The boxes, made under `root` from the root filesystem at `rootfs`,
    /// their keepers run by `helper` in cgroups `delegation` gives.
    pub fn new(
        root: PathBuf,
        rootfs: (PathBuf, String),
        helper: Vec<OsString>,
        delegation: Delegation,
    ) -> Self {
        Bubblewrap {
            config: BubblewrapConfig {
                root,
                rootfs,
                helper,
                delegation,
            },
            #[cfg(target_os = "linux")]
            state: imp::State::default(),
        }
    }
}

#[cfg(not(target_os = "linux"))]
#[async_trait::async_trait]
impl crate::Backend for Bubblewrap {
    async fn prepare(&self) -> Result<(), crate::SandboxError> {
        todo!("bubblewrap runs on the Linux kernel; elsewhere the guest runs it")
    }
    async fn ensure(&self, _: &crate::Workspace, _: bool) -> Result<(), crate::SandboxError> {
        todo!("bubblewrap runs on the Linux kernel")
    }
    async fn has_session(&self, _: &crate::Workspace) -> bool {
        todo!("bubblewrap runs on the Linux kernel")
    }
    async fn session(
        &self,
        _: &crate::Workspace,
        _: &str,
        _: f64,
        _: crate::backend::Tap,
        _: &tokio_util::sync::CancellationToken,
    ) -> Result<crate::backend::SessionRun, crate::SandboxError> {
        todo!("bubblewrap runs on the Linux kernel")
    }
    async fn kill_session(&self, _: &str) {
        todo!("bubblewrap runs on the Linux kernel")
    }
    async fn one_shot(
        &self,
        _: &crate::Workspace,
        _: bool,
        _: &str,
        _: f64,
        _: &str,
        _: crate::backend::Tap,
        _: &tokio_util::sync::CancellationToken,
    ) -> Result<crate::backend::OneShotRun, crate::SandboxError> {
        todo!("bubblewrap runs on the Linux kernel")
    }
    async fn end_processes(&self, _: &str, _: bool) {
        todo!("bubblewrap runs on the Linux kernel")
    }
    async fn program(
        &self,
        _: &crate::Workspace,
        _: &[String],
        _: std::time::Duration,
    ) -> Result<crate::backend::ProgramRun, crate::SandboxError> {
        todo!("bubblewrap runs on the Linux kernel")
    }
    async fn remove(&self, _: &str) {
        todo!("bubblewrap runs on the Linux kernel")
    }
    async fn shutdown(&self) {
        todo!("bubblewrap runs on the Linux kernel")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // sandbox-docker-missing.test.js: "with Docker not installed, bash and
    // the reaper fail with DOCKER_MISSING": bubblewrap not installed.
    #[test]
    fn with_bubblewrap_not_installed_the_sandbox_is_unavailable_with_bwrap_missing() {
        let empty =
            std::env::temp_dir().join(format!("mcbench-sandbox-nobwrap-{}", std::process::id()));
        std::fs::create_dir_all(&empty).unwrap();
        assert_eq!(find_program("bwrap", empty.as_os_str()), None);
        assert!(BWRAP_MISSING.starts_with(
            "The model's bash shell runs in a bubblewrap sandbox, and bubblewrap is not installed"
        ));
        std::fs::remove_dir(&empty).unwrap();
    }

    // The same, user namespaces refused (bubblewrap's own words on a
    // kernel that refuses them, and on Ubuntu's AppArmor restriction).
    #[test]
    fn with_user_namespaces_refused_the_sandbox_is_unavailable_with_userns_missing() {
        for stderr in [
            "bwrap: No permissions to create a new namespace, likely because the kernel does not allow non-privileged user namespaces. On e.g. debian this can be enabled with 'sysctl kernel.unprivileged_userns_clone=1'.\n",
            "bwrap: setting up uid map: Permission denied\n",
        ] {
            assert_eq!(classify_bwrap_failure(stderr), Some(USERNS_MISSING));
        }
        assert_eq!(
            classify_bwrap_failure(
                "bwrap: Can't find source path /nope: No such file or directory\n"
            ),
            None
        );
    }

    #[test]
    fn ipv6_off_names_the_proc_files_of_todays_sysctls() {
        assert_eq!(
            ipv6_off(),
            [
                (
                    "/proc/sys/net/ipv6/conf/all/disable_ipv6".to_string(),
                    "1".to_string()
                ),
                (
                    "/proc/sys/net/ipv6/conf/default/disable_ipv6".to_string(),
                    "1".to_string()
                )
            ]
        );
    }
}
