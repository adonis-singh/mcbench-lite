//! Windows: bubblewrap as a non-root user inside the app's private WSL2
//! distro, automount and interop off. WSL itself is installed once by an
//! administrator ([`WSL_MISSING`] says so; [`wsl_state`] tells the setup
//! screen whether that step is needed). The distro runs the guest agent,
//! and this module is the lifecycle over [`GuestClient`] talking to it over
//! the stdin and stdout of one `wsl.exe`, as the macOS module does over its
//! VM's vsock.
//!
//! | Question | How |
//! |---|---|
//! | The distro | `mcbench-lite` (one per Windows user; `MCBENCH_WSL_DISTRO` names another for tests), imported with `wsl --import <name> <sandbox>\.wsl <tar> --version 2` from the guest base's `userland.tar` with what WSL needs added ([`import_tar`]: `/etc/wsl.conf` with automount, `fstab`, interop and the GPU off, the user `mcbench` 1000:1000 as the default, `/usr/bin/sh` and `/usr/bin/mount` (busybox), `/usr/sbin`, where WSL links `mount.drvfs` to its init at each start (without it the distro does not start: `Wsl/Service/CreateInstance/E_UNEXPECTED`), `/usr/lib/mcbench/wsl-init`). Its disk is `<sandbox>\.wsl\ext4.vhdx` |
//! | Found again | `wsl --list --quiet` names it and `<sandbox>\.wsl\imported` holds the sha256 of the tar it was imported from; another sha256 (a new guest base or a new `wsl-init`) unregisters it and imports it again (the distro holds nothing that is not made again: the unpacked root filesystem and the build child's copy). A distro of that name whose disk is not in this data folder is not touched: the sandbox is unavailable, saying so |
//! | Removed | `wsl --unregister <name>`, which deletes its disk (the app's uninstaller does this) |
//! | Other distros | Untouched: the distro is a distro of its own in WSL's one VM, beside the user's; they share the VM's kernel and its cgroup tree, in which this distro's boxes have a folder of their own (`mcbench-<name>`) |
//! | `/work` | The data folder's `sandbox` folder mounted by drvfs (`metadata`, owned by the agent's user) at `/mnt/sandbox` by `wsl-init` as root, the root filesystem's folder read only at `/mnt/rootfs`; automount off mounts nothing else of Windows. The chats' files stay on NTFS, so the host's attachments, pictures, files and storage watchdog work on them as on Linux and macOS |
//! | The agent | The guest base's own `/usr/bin/mcbench-sandbox-agent` (the static musl program, pinned with the guest base), started by `wsl-init` as `guest -`: it becomes the user and serves on stdin and stdout |
//! | Stopping | [`crate::Sandbox::shutdown`] removes the boxes, then the `wsl.exe` is killed; its end closes the agent's stdin, and the agent removes every box and exits. When the app dies its job object ends the `wsl.exe` the same way. WSL stops the distro once nothing runs in it |
//!
//! The guest base is [`crate::Config`]'s `guest`, from setup
//! (`mcbench_setup::shell::files`). [`import`] imports the distro from it
//! alone, so the shell step imports it as soon as the guest base is on disk
//! and checked: the build child (`mcbench_build`'s WSL sandbox) needs only
//! the distro, the boxes also the root filesystem.
//!
//! Installing WSL and restarting Windows for it, for the setup screen
//! (`ui-seam-wsl-install.md`): [`setup_state`] reads whether WSL is ready,
//! must be installed, or waits for a restart; [`install_wsl`] runs
//! `wsl.exe --install --no-distribution` elevated; [`record_restart`] notes
//! that this boot of Windows must end first; [`restart_computer`] registers
//! the app to start once at the next sign-in and asks Windows to restart.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::agent::{GuestClient, WorkMap};
use crate::lifecycle::Sandboxes;
use crate::{GuestBase, Workspace};

/// The distro's name, its `wsl.exe` and what setup found of it, shared with
/// the build child's sandbox and Codex's box.
#[cfg(windows)]
pub use mcbench_build::sandbox::wsl;

/// The sentence a model reads when WSL is not installed.
pub const WSL_MISSING: &str = "The model's bash shell runs in a Linux distribution under WSL, and WSL is not installed: an administrator installs it once with `wsl --install --no-distribution` and restarts Windows if asked, then try again";

/// The sentence a model reads when the distro cannot be started, with the
/// step's own words after it.
pub const WSL_UNAVAILABLE: &str =
    "The model's bash shell runs in a Linux distribution under WSL, and it could not be started";

/// Where the data folder's `sandbox` folder is in the distro.
pub const GUEST_SANDBOX: &str = "/mnt/sandbox";

/// The distro's start script (`guest/wsl-init`).
pub const WSL_INIT: &str = include_str!("../guest/wsl-init");

/// Where [`WSL_INIT`] is in the distro.
pub const WSL_INIT_PATH: &str = "usr/lib/mcbench/wsl-init";

/// The distro's `/etc/wsl.conf`: no systemd, no drive mounted, no `fstab`,
/// no Windows program started from Linux and no Windows `PATH`, the user
/// [`USER`] by default, and no GPU (`[gpu] enabled=false`, per distro: WSL
/// then mounts neither its GPU libraries nor Windows' driver store under
/// `/usr/lib/wsl`, which a box binding `/usr` would see).
pub const WSL_CONF: &str = "[boot]\nsystemd=false\n\n[automount]\nenabled=false\nmountFsTab=false\n\n[interop]\nenabled=false\nappendWindowsPath=false\n\n[user]\ndefault=mcbench\n\n[network]\ngenerateHosts=true\ngenerateResolvConf=true\n\n[gpu]\nenabled=false\n";

/// The distro's user (the boxes' user), as both Windows sandboxes name it.
pub const USER: &str = "mcbench";

/// That user's ids, which `id` shows in a box.
pub const GUEST_USER: (u32, u32) = (1000, 1000);

/// Whether this Windows can run the distro, for the setup screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WslState {
    /// WSL is installed and answers (`wsl.exe --status` succeeds): nothing
    /// to do.
    Ready,
    /// The Virtual Machine Platform feature WSL 2 needs is on, but WSL is
    /// not installed: installing it needs no restart.
    FeatureOnly,
    /// The feature is off: installing WSL turns it on, which needs an
    /// administrator and a restart.
    FeatureOff,
}

/// This Windows' [`WslState`], read without elevation and without changing
/// anything: `wsl.exe --status`, then the `VirtualMachinePlatform` optional
/// feature's `InstallState` from WMI's `Win32_OptionalFeature` (1 enabled,
/// 2 disabled, 3 absent), through PowerShell's `Get-CimInstance`. WMI
/// answers a standard user in a desktop session (measured: a non-elevated
/// interactive session read `InstallState`), not one logged on over SSH
/// (`Access denied`). Elsewhere than Windows an error.
pub fn wsl_state() -> Result<WslState, String> {
    #[cfg(windows)]
    return windows_wsl_state();
    #[cfg(not(windows))]
    Err("WSL is Windows'".to_string())
}

#[cfg(windows)]
fn windows_wsl_state() -> Result<WslState, String> {
    use mcbench_build::sandbox::wsl;
    let program = wsl::program()?;
    let status = wsl::manage()?
        .arg("--status")
        .output()
        .map_err(|e| format!("{}: {e}", program.display()))?;
    if status.status.success() {
        return Ok(WslState::Ready);
    }
    let powershell = wsl::system_program(r"WindowsPowerShell\v1.0\powershell.exe")?;
    let mut command = mcbench_spawn::command(&powershell);
    let feature = command
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "(Get-CimInstance -ClassName Win32_OptionalFeature -Filter \"Name='VirtualMachinePlatform'\").InstallState",
        ])
        .output()
        .map_err(|e| format!("{}: {e}", powershell.display()))?;
    let state = String::from_utf8_lossy(&feature.stdout).trim().to_string();
    match (feature.status.success(), state.as_str()) {
        (true, "1") => Ok(WslState::FeatureOnly),
        (true, "2" | "3") => Ok(WslState::FeatureOff),
        _ => Err(format!(
            "reading the VirtualMachinePlatform feature's state with {}: {} (output {state:?}): {}",
            powershell.display(),
            feature.status,
            String::from_utf8_lossy(&feature.stderr).trim()
        )),
    }
}

/// The Windows sandbox.
pub struct WslDistro {
    /// The lifecycle over the guest.
    pub guest: Sandboxes<GuestClient>,
    agent: Arc<Mutex<Option<tokio::process::Child>>>,
}

impl WslDistro {
    /// The sandbox over the guest agent's byte stream (`reader` from it,
    /// `writer` to it); `map` gives each `/work`'s path inside the distro.
    pub fn over<R, W>(
        reader: R,
        writer: W,
        map: WorkMap,
        budget: std::sync::Arc<dyn crate::Budget>,
    ) -> Self
    where
        R: tokio::io::AsyncRead + Unpin + Send + 'static,
        W: tokio::io::AsyncWrite + Unpin + Send + 'static,
    {
        WslDistro {
            guest: Sandboxes::new(GuestClient::new(reader, writer, map), budget),
            agent: Arc::default(),
        }
    }

    /// Imports (when needed) and starts the distro from the configuration's
    /// guest base in the background (on the current tokio runtime), then
    /// [`WslDistro::over`] the agent's stream, and answers at once: calls
    /// wait for the agent, and when the distro cannot start every call
    /// answers [`crate::SandboxError::Unavailable`] with [`WSL_MISSING`] or
    /// [`WSL_UNAVAILABLE`] and the reason.
    #[cfg(not(target_os = "linux"))]
    pub fn start(config: crate::Config) -> Self {
        let guest = config.guest.clone();
        let (ours, theirs) = tokio::io::duplex(1 << 20);
        let (reader, writer) = tokio::io::split(ours);
        let root = config.root.clone();
        let map: WorkMap = Arc::new(move |workspace: &Workspace| guest_path(&root, workspace));
        let sandbox = Self::over(reader, writer, map, config.budget.clone());
        let slot = sandbox.agent.clone();
        tokio::spawn(async move {
            match distro::start(&config.root, &config.rootfs, guest).await {
                Ok(mut child) => {
                    let stdout = child.stdout.take().expect("piped stdout");
                    let stdin = child.stdin.take().expect("piped stdin");
                    *slot.lock().expect("the agent") = Some(child);
                    let mut stream = tokio::io::join(stdout, stdin);
                    let mut theirs = theirs;
                    let _ = tokio::io::copy_bidirectional(&mut theirs, &mut stream).await;
                    if let Some(mut child) = slot.lock().expect("the agent").take() {
                        let _ = child.start_kill();
                    }
                }
                Err(message) => crate::macos::answer_unavailable(theirs, message).await,
            }
        });
        sandbox
    }

    /// Ends the agent's `wsl.exe` now (the agent then removes its boxes).
    fn stop(&self) {
        if let Some(mut child) = self.agent.lock().expect("the agent").take() {
            let _ = child.start_kill();
        }
    }
}

impl Drop for WslDistro {
    fn drop(&mut self) {
        self.stop();
    }
}

#[async_trait::async_trait]
impl crate::Sandbox for WslDistro {
    async fn prepare(&self) -> Result<(), crate::SandboxError> {
        crate::Sandbox::prepare(&self.guest).await
    }
    async fn exec(
        &self,
        workspace: &Workspace,
        command: &crate::BashCommand,
        cancel: &tokio_util::sync::CancellationToken,
    ) -> Result<crate::BashRun, crate::SandboxError> {
        crate::Sandbox::exec(&self.guest, workspace, command, cancel).await
    }
    async fn view_image(
        &self,
        workspace: &Workspace,
        path: &str,
    ) -> Result<Vec<u8>, crate::SandboxError> {
        crate::Sandbox::view_image(&self.guest, workspace, path).await
    }
    async fn remove_work_files(
        &self,
        workspace: &Workspace,
        paths: &[String],
    ) -> Result<(), crate::SandboxError> {
        crate::Sandbox::remove_work_files(&self.guest, workspace, paths).await
    }
    async fn reset(&self, chat: &str) {
        crate::Sandbox::reset(&self.guest, chat).await
    }
    async fn destroy(&self, workspace: &Workspace) -> Result<(), crate::SandboxError> {
        crate::Sandbox::destroy(&self.guest, workspace).await
    }
    /// The boxes removed by the agent, then its `wsl.exe` ended.
    async fn shutdown(&self) {
        crate::Sandbox::shutdown(&self.guest).await;
        self.stop();
    }
    fn network_access(&self) -> bool {
        crate::Sandbox::network_access(&self.guest)
    }
    fn disable_network(&self) {
        crate::Sandbox::disable_network(&self.guest)
    }
}

/// A chat's `/work` inside the distro: its folder under the sandbox folder,
/// under [`GUEST_SANDBOX`], with `/` between the parts.
/// An error, not a panic, when the chat's folder is not under `root` as
/// written (a `\\?\` form of one beside a plain form of the other is such a
/// difference).
#[cfg_attr(target_os = "linux", allow(dead_code))]
fn guest_path(root: &Path, workspace: &Workspace) -> Result<PathBuf, crate::SandboxError> {
    let failed = |why: &str| {
        crate::SandboxError::Failed(format!(
            "the chat's /work {} {why} the sandbox folder {}",
            workspace.host.display(),
            root.display()
        ))
    };
    let inside = workspace
        .host
        .strip_prefix(root)
        .map_err(|_| failed("is not in"))?;
    let mut path = GUEST_SANDBOX.to_string();
    for part in inside.components() {
        path.push('/');
        path.push_str(
            part.as_os_str()
                .to_str()
                .ok_or_else(|| failed("has a name that is not UTF-8, under"))?,
        );
    }
    Ok(PathBuf::from(path))
}

/// A Windows folder as drvfs takes it: a drive-letter path, without the
/// `\\?\` prefix of a canonical path. A network path (`\\server\share` or
/// `\\?\UNC\...`) and a path with `;` (drvfs's mount source cannot carry it:
/// measured in the Windows VM) are refused before anything is mounted, with
/// the path named.
pub fn drvfs_path(path: &Path) -> Result<String, String> {
    let text = path
        .to_str()
        .ok_or_else(|| format!("the folder {} is not UTF-8", path.display()))?;
    let plain = text.strip_prefix(r"\\?\").unwrap_or(text);
    let bytes = plain.as_bytes();
    let drive =
        bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'\\';
    if !drive {
        return Err(format!(
            "the folder {text} is not on a drive with a letter, and WSL mounts only those"
        ));
    }
    if plain.contains(';') {
        return Err(format!(
            "the folder {text} has a ';' in its path, which WSL's drvfs cannot mount"
        ));
    }
    Ok(plain.to_string())
}

/// Busybox's programs WSL itself runs in the distro: `/bin/sh` (the users'
/// shell) and `/bin/mount`, which WSL's `mount.drvfs` runs.
pub const LINKS: [&str; 2] = ["usr/bin/sh", "usr/bin/mount"];

/// The tar the distro is imported from: the guest base's `userland.tar`
/// with `/etc/passwd` and `/etc/group` naming [`USER`], [`WSL_CONF`],
/// [`LINKS`] and [`WSL_INIT`] added.
pub fn import_tar(userland: &[u8]) -> Result<Vec<u8>, String> {
    let (uid, gid) = GUEST_USER;
    let replaced: [(&str, String, u32); 4] = [
        (
            "etc/passwd",
            format!(
                "root:x:0:0:root:/root:/usr/bin/sh\n{USER}:x:{uid}:{gid}::/var/mcbench:/usr/bin/sh\n"
            ),
            0o644,
        ),
        ("etc/group", format!("root:x:0:\n{USER}:x:{gid}:\n"), 0o644),
        ("etc/wsl.conf", WSL_CONF.to_string(), 0o644),
        (WSL_INIT_PATH, WSL_INIT.to_string(), 0o755),
    ];
    let mut out = tar::Builder::new(Vec::new());
    let mut archive = tar::Archive::new(userland);
    for entry in archive
        .entries()
        .map_err(|e| format!("userland.tar: {e}"))?
    {
        let mut entry = entry.map_err(|e| format!("userland.tar: {e}"))?;
        let path = entry
            .path()
            .map_err(|e| format!("userland.tar: {e}"))?
            .to_string_lossy()
            .into_owned();
        if replaced.iter().any(|(name, _, _)| *name == path)
            || LINKS.iter().any(|link| *link == path)
        {
            continue;
        }
        let header = entry.header().clone();
        out.append(&header, &mut entry)
            .map_err(|e| format!("the import tar: {e}"))?;
    }
    let mut add_dir = |name: &str| -> Result<(), String> {
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Directory);
        header.set_mode(0o755);
        header.set_uid(0);
        header.set_gid(0);
        header.set_mtime(0);
        header.set_size(0);
        out.append_data(&mut header, name, std::io::empty())
            .map_err(|e| format!("the import tar: {e}"))
    };
    // WSL links /sbin/mount.drvfs to its init at every start: /sbin is
    // usr/sbin, which the guest base does not have.
    add_dir("usr/sbin")?;
    add_dir("usr/lib/mcbench")?;
    for (name, text, mode) in &replaced {
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Regular);
        header.set_mode(*mode);
        header.set_uid(0);
        header.set_gid(0);
        header.set_mtime(0);
        header.set_size(text.len() as u64);
        out.append_data(&mut header, name, text.as_bytes())
            .map_err(|e| format!("the import tar: {e}"))?;
    }
    for link in LINKS {
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Symlink);
        header.set_mode(0o777);
        header.set_uid(0);
        header.set_gid(0);
        header.set_mtime(0);
        header.set_size(0);
        out.append_link(&mut header, link, "busybox")
            .map_err(|e| format!("the import tar: {e}"))?;
    }
    out.into_inner().map_err(|e| format!("the import tar: {e}"))
}

#[cfg(windows)]
mod distro {
    //! The distro's import and its agent's start.

    use std::path::{Path, PathBuf};
    use std::process::Stdio;
    use std::time::Duration;

    use mcbench_build::sandbox::wsl;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

    use super::{WSL_MISSING, WSL_UNAVAILABLE};
    use crate::GuestBase;

    /// How long the distro's start may take (a cold start of WSL's VM took
    /// 7 s in the Windows VM).
    const START_WITHIN: Duration = Duration::from_secs(120);

    fn sha256(bytes: &[u8]) -> String {
        use sha2::Digest;
        sha2::Sha256::digest(bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }

    fn run(command: &mut std::process::Command, what: &str) -> Result<(), String> {
        let output = command.output().map_err(|e| format!("{what}: {e}"))?;
        if output.status.success() {
            Ok(())
        } else {
            Err(format!(
                "{what}: {} {}",
                String::from_utf8_lossy(&output.stdout).trim(),
                String::from_utf8_lossy(&output.stderr).trim()
            ))
        }
    }

    /// The distro imported from `guest`'s userland (again when that changed),
    /// and what the build child's sandbox knows of it told
    /// (`mcbench_build::sandbox::wsl`): ready, imported anew (the programs
    /// put into the old disk are gone), or unavailable with the sentence.
    pub fn import(root: &Path, guest: &GuestBase) -> Result<String, String> {
        let result = import_once(root, guest);
        match &result {
            Ok((_, true)) => wsl::imported_now(),
            Ok((_, false)) => wsl::found(),
            Err(said) => wsl::unavailable(said.clone()),
        }
        result.map(|(name, _)| name)
    }

    /// The distro's name, and whether it was imported now.
    fn import_once(root: &Path, guest: &GuestBase) -> Result<(String, bool), String> {
        let unavailable = |reason: String| format!("{WSL_UNAVAILABLE}: {reason}");
        // A folder drvfs cannot mount is refused before anything is made.
        super::drvfs_path(root).map_err(unavailable)?;
        let program = wsl::program().map_err(unavailable)?;
        let status = wsl::manage()
            .map_err(unavailable)?
            .arg("--status")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|e| unavailable(format!("{}: {e}", program.display())))?;
        if !status.success() {
            return Err(WSL_MISSING.to_string());
        }
        let base = crate::macos::unpack_guest(guest, root).map_err(unavailable)?;
        let userland = std::fs::read(base.join("userland.tar"))
            .map_err(|e| unavailable(format!("the guest base's userland.tar: {e}")))?;
        let tar = super::import_tar(&userland).map_err(unavailable)?;
        let mark = sha256(&tar);
        let dir = root.join(".wsl");
        std::fs::create_dir_all(&dir)
            .map_err(|e| unavailable(format!("{}: {e}", dir.display())))?;
        let name = wsl::distro();
        let marker = dir.join("imported");
        let imported = match std::fs::read_to_string(&marker) {
            Ok(text) => Some(text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(unavailable(format!("{}: {e}", marker.display()))),
        };
        if let Some(disk) = wsl::registered(&name).map_err(unavailable)? {
            let canonical = |path: &Path| {
                std::fs::canonicalize(path)
                    .map_err(|e| unavailable(format!("{}: {e}", path.display())))
            };
            if canonical(&disk)? != canonical(&dir)? {
                return Err(unavailable(format!(
                    "a WSL distro named {name} exists whose disk is in {}, not in {}",
                    disk.display(),
                    dir.display()
                )));
            }
            if imported.as_deref().map(str::trim) == Some(mark.as_str()) {
                return Ok((name, false));
            }
            run(
                wsl::manage()
                    .map_err(unavailable)?
                    .args(["--unregister", &name]),
                "wsl --unregister",
            )
            .map_err(unavailable)?;
        }
        if imported.is_some() {
            std::fs::remove_file(&marker)
                .map_err(|e| unavailable(format!("{}: {e}", marker.display())))?;
        }
        let file = dir.join("import.tar");
        std::fs::write(&file, &tar).map_err(|e| unavailable(format!("{}: {e}", file.display())))?;
        let result = run(
            wsl::manage()
                .map_err(unavailable)?
                .args(["--import", &name])
                .arg(&dir)
                .arg(&file)
                .args(["--version", "2"]),
            "wsl --import",
        );
        std::fs::remove_file(&file).map_err(|e| unavailable(format!("{}: {e}", file.display())))?;
        result.map_err(unavailable)?;
        std::fs::write(&marker, &mark)
            .map_err(|e| unavailable(format!("{}: {e}", marker.display())))?;
        Ok((name, true))
    }

    /// Imports the distro when needed, starts `wsl-init` as root in it and
    /// waits for its "ready"; the `wsl.exe` whose stdin and stdout are the
    /// agent's, in the app's job. Its stderr goes on to
    /// `<sandbox>\.wsl\agent.log`.
    pub async fn start(
        root: &Path,
        rootfs: &(PathBuf, String),
        guest: GuestBase,
    ) -> Result<tokio::process::Child, String> {
        let unavailable = |reason: String| format!("{WSL_UNAVAILABLE}: {reason}");
        let name = {
            let root = root.to_path_buf();
            tokio::task::spawn_blocking(move || import(&root, &guest))
                .await
                .expect("the import task")?
        };
        let (tarball, sha256) = rootfs;
        let folder = tarball.parent().ok_or_else(|| {
            unavailable(format!(
                "the root filesystem {} has no folder",
                tarball.display()
            ))
        })?;
        let file = tarball
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| {
                unavailable(format!(
                    "the root filesystem {} has no name",
                    tarball.display()
                ))
            })?;
        let (uid, gid) = super::GUEST_USER;
        let sandbox = super::drvfs_path(root).map_err(unavailable)?;
        let rootfs_folder = super::drvfs_path(folder).map_err(unavailable)?;
        let program = wsl::program().map_err(unavailable)?;
        let mut command =
            tokio::process::Command::from(wsl::exec(&name, "root").map_err(unavailable)?);
        command
            .args([
                "/usr/bin/busybox",
                "sh",
                &format!("/{}", super::WSL_INIT_PATH),
            ])
            .arg(sandbox)
            .arg(rootfs_folder)
            .arg(file)
            .arg(sha256)
            .arg(format!("{uid}:{gid}"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let mut child = command
            .spawn()
            .map_err(|e| unavailable(format!("{}: {e}", program.display())))?;
        wsl::adopt(&child).map_err(|e| unavailable(format!("the app's job: {e}")))?;
        let stderr = child.stderr.take().expect("piped stderr");
        let log = root.join(".wsl").join("agent.log");
        let mut log = tokio::fs::File::create(&log)
            .await
            .map_err(|e| unavailable(format!("{}: {e}", log.display())))?;
        let mut lines = tokio::io::BufReader::new(stderr).lines();
        let mut last = String::new();
        let waited = tokio::time::timeout(START_WITHIN, async {
            while let Ok(Some(line)) = lines.next_line().await {
                let _ = log.write_all(format!("{line}\n").as_bytes()).await;
                if line == "mcbench wsl init: ready" {
                    return true;
                }
                last = line;
            }
            false
        })
        .await;
        match waited {
            Ok(true) => {}
            Ok(false) => {
                let _ = child.start_kill();
                return Err(unavailable(if last.is_empty() {
                    "wsl-init ended without a word".to_string()
                } else {
                    last.trim_start_matches("mcbench wsl init: ").to_string()
                }));
            }
            Err(_) => {
                let _ = child.start_kill();
                return Err(unavailable(format!(
                    "the distro {name} did not start within {} s",
                    START_WITHIN.as_secs()
                )));
            }
        }
        tokio::spawn(async move {
            while let Ok(Some(line)) = lines.next_line().await {
                let _ = log.write_all(format!("{line}\n").as_bytes()).await;
            }
        });
        Ok(child)
    }
}

#[cfg(not(windows))]
mod distro {
    //! WSL is Windows'.

    use std::path::{Path, PathBuf};

    use crate::GuestBase;

    #[cfg_attr(target_os = "linux", allow(dead_code))]
    pub async fn start(
        _: &Path,
        _: &(PathBuf, String),
        _: GuestBase,
    ) -> Result<tokio::process::Child, String> {
        Err(format!("{}: WSL is Windows'", super::WSL_UNAVAILABLE))
    }

    pub fn import(_: &Path, _: &GuestBase) -> Result<String, String> {
        Err(format!("{}: WSL is Windows'", super::WSL_UNAVAILABLE))
    }
}

/// Imports the app's distro from `guest`'s userland, with its disk under
/// `root` (the data folder's sandbox folder), unless it is already imported
/// from the same tar: all the build child needs. Blocking (it reads and
/// unpacks the guest base and runs `wsl --import`). An error is
/// [`WSL_MISSING`] or [`WSL_UNAVAILABLE`] with the reason.
pub fn import(root: &Path, guest: &GuestBase) -> Result<(), String> {
    distro::import(root, guest).map(drop)
}

/// Where setup's shell step stands with WSL.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WslSetup {
    /// WSL answers: the distro can be imported.
    Ready,
    /// WSL must be installed ([`install_wsl`]).
    Install,
    /// WSL was installed in this boot of Windows and works only after a
    /// restart ([`restart_computer`]).
    Restart,
}

/// The file under `<root>\.wsl` that holds the boot of Windows in which
/// WSL was installed and did not yet answer ([`record_restart`]).
pub const RESTART_FILE: &str = "restart-pending";

/// Where setup's shell step stands with WSL, for the sandbox folder
/// `root`: [`WslSetup::Restart`] while the restart file names this boot of
/// Windows (`wsl.exe --status` already succeeds then, though no distro can
/// start before the restart: measured in the Windows VM); otherwise, the
/// restart file of an earlier boot removed as stale, [`WslSetup::Ready`]
/// when [`wsl_state`] is ready and [`WslSetup::Install`] when not.
/// Blocking.
pub fn setup_state(root: &Path) -> Result<WslSetup, String> {
    let file = root.join(".wsl").join(RESTART_FILE);
    match std::fs::read_to_string(&file) {
        Ok(recorded) if recorded.trim() == boot_id()?.to_string() => {
            return Ok(WslSetup::Restart);
        }
        Ok(_) => std::fs::remove_file(&file).map_err(|e| format!("{}: {e}", file.display()))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(format!("{}: {e}", file.display())),
    }
    Ok(match wsl_state()? {
        WslState::Ready => WslSetup::Ready,
        WslState::FeatureOnly | WslState::FeatureOff => WslSetup::Install,
    })
}

/// Records, under the sandbox folder `root`, that WSL was installed in this
/// boot of Windows and needs a restart ([`setup_state`] reads it).
pub fn record_restart(root: &Path) -> Result<(), String> {
    let dir = root.join(".wsl");
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let file = dir.join(RESTART_FILE);
    std::fs::write(&file, boot_id()?.to_string()).map_err(|e| format!("{}: {e}", file.display()))
}

/// How [`install_wsl`] failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InstallFailure {
    /// The administrator's prompt was declined (Windows error 1223,
    /// `ERROR_CANCELLED`).
    Declined,
    /// Windows' words, or the installer's exit code.
    Failed(String),
    /// WSL was installed, but the restart it owes could not be recorded:
    /// the restart file's path and error.
    Record(String),
}

/// What [`install_wsl`] left.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Installed {
    /// WSL answers: the distro can be imported now.
    Ready,
    /// Windows must restart first; recorded under the sandbox folder
    /// ([`record_restart`]).
    Restart,
}

/// Runs `wsl.exe --install --no-distribution` elevated (`ShellExecuteExW`
/// with the `runas` verb: Windows shows its own administrator's prompt),
/// waits for it to end, and says whether Windows must restart first: when
/// the virtualisation feature was off before ([`WslState::FeatureOff`]:
/// Windows turns it on at the next boot), or when WSL does not answer
/// after. That restart is recorded under the sandbox folder `root`.
/// Blocking. Elsewhere than Windows a failure ([`wsl_state`]'s).
pub fn install_wsl(root: &Path) -> Result<Installed, InstallFailure> {
    let before = wsl_state().map_err(InstallFailure::Failed)?;
    #[cfg(windows)]
    os::install_wsl()?;
    if before != WslState::FeatureOff
        && wsl_state().map_err(InstallFailure::Failed)? == WslState::Ready
    {
        return Ok(Installed::Ready);
    }
    record_restart(root).map_err(InstallFailure::Record)?;
    Ok(Installed::Restart)
}

/// Registers `program` (the app, its path quoted) to start once at the
/// person's next sign-in, under their own
/// `HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\RunOnce`
/// (value [`RUN_ONCE_VALUE`]), then asks Windows for an ordinary restart
/// (`InitiateShutdownW` with `SHUTDOWN_RESTART`, nothing forced: a program
/// with unsaved work may hold it up or cancel it). When Windows refuses,
/// the value is removed again and the error is Windows' words. Blocking.
/// Elsewhere than Windows a failure.
pub fn restart_computer(program: &Path) -> Result<(), String> {
    #[cfg(windows)]
    return os::restart_computer(program);
    #[cfg(not(windows))]
    {
        let _ = program;
        Err("WSL is Windows'".into())
    }
}

/// The `RunOnce` value [`restart_computer`] writes.
pub const RUN_ONCE_VALUE: &str = "mcbench-lite";

/// Whether Windows took the restart request, from `InitiateShutdownW`'s
/// answer: done (0), or a shutdown already under way (1115,
/// `ERROR_SHUTDOWN_IN_PROGRESS`: a second press after the first was taken).
/// Anything else is a refusal.
pub fn restart_taken(code: u32) -> bool {
    const ERROR_SHUTDOWN_IN_PROGRESS: u32 = 1115;
    code == 0 || code == ERROR_SHUTDOWN_IN_PROGRESS
}

/// This boot of Windows: the boot counter Windows keeps in
/// `HKEY_LOCAL_MACHINE\SYSTEM\CurrentControlSet\Control\Session
/// Manager\Memory Management\PrefetchParameters` (`BootId`, one more at
/// every boot), which a standard user reads. Elsewhere than Windows an
/// error.
pub fn boot_id() -> Result<u32, String> {
    #[cfg(windows)]
    return os::boot_id();
    #[cfg(not(windows))]
    Err("WSL is Windows'".into())
}

#[cfg(windows)]
mod os {
    //! The Windows calls of installing WSL and restarting.

    use std::path::Path;

    use windows_sys::Win32::Foundation::{
        CloseHandle, ERROR_CANCELLED, ERROR_SUCCESS, GetLastError, HANDLE, LUID, WAIT_OBJECT_0,
    };
    use windows_sys::Win32::Security::{
        AdjustTokenPrivileges, LUID_AND_ATTRIBUTES, LookupPrivilegeValueW, SE_PRIVILEGE_ENABLED,
        TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES, TOKEN_QUERY,
    };
    use windows_sys::Win32::System::Com::{
        COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx, CoUninitialize,
    };
    use windows_sys::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_DWORD,
        REG_OPTION_NON_VOLATILE, REG_SZ, RegCloseKey, RegCreateKeyExW, RegDeleteValueW,
        RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
    };
    use windows_sys::Win32::System::Shutdown::{
        InitiateShutdownW, SHTDN_REASON_FLAG_PLANNED, SHTDN_REASON_MAJOR_APPLICATION,
        SHTDN_REASON_MINOR_INSTALLATION, SHUTDOWN_RESTART,
    };
    use windows_sys::Win32::System::Threading::{
        GetCurrentProcess, GetExitCodeProcess, INFINITE, OpenProcessToken, WaitForSingleObject,
    };
    use windows_sys::Win32::UI::Shell::{
        SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_HIDE;

    use super::InstallFailure;

    /// `text` as UTF-16 with its terminating NUL.
    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain([0]).collect()
    }

    /// Windows' words for `code` (a Win32 error).
    fn said(code: u32) -> String {
        std::io::Error::from_raw_os_error(code as i32).to_string()
    }

    /// A handle closed when dropped.
    struct Handle(HANDLE);

    impl Drop for Handle {
        fn drop(&mut self) {
            // SAFETY: the handle is open and owned here; it is closed once.
            unsafe { CloseHandle(self.0) };
        }
    }

    /// A registry key closed when dropped.
    struct Key(HKEY);

    impl Drop for Key {
        fn drop(&mut self) {
            // SAFETY: the key is open and owned here; it is closed once.
            unsafe { RegCloseKey(self.0) };
        }
    }

    /// COM on this thread, as `ShellExecuteExW` asks (apartment-threaded,
    /// no OLE1 DDE), let go when dropped.
    struct Com;

    impl Com {
        fn init() -> Result<Com, String> {
            // SAFETY: a reserved null pointer and the documented flags; the
            // matching CoUninitialize runs in Drop on this same thread.
            let result = unsafe {
                CoInitializeEx(
                    std::ptr::null(),
                    (COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) as u32,
                )
            };
            if result < 0 {
                return Err(format!(
                    "CoInitializeEx: {}",
                    std::io::Error::from_raw_os_error(result)
                ));
            }
            Ok(Com)
        }
    }

    impl Drop for Com {
        fn drop(&mut self) {
            // SAFETY: COM was initialised on this thread by Com::init.
            unsafe { CoUninitialize() };
        }
    }

    pub fn install_wsl() -> Result<(), InstallFailure> {
        let program = mcbench_build::sandbox::wsl::program().map_err(InstallFailure::Failed)?;
        let verb = wide("runas");
        let file = wide(
            program
                .to_str()
                .ok_or_else(|| InstallFailure::Failed("wsl.exe's path is not UTF-8".into()))?,
        );
        let parameters = wide("--install --no-distribution");
        let _com = Com::init().map_err(InstallFailure::Failed)?;
        // SAFETY: an all-zero SHELLEXECUTEINFOW is a valid start (null
        // pointers, no flags); the fields set below point at the NUL-
        // terminated strings above, which outlive the call.
        let mut info: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
        info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
        info.fMask = SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC;
        info.lpVerb = verb.as_ptr();
        info.lpFile = file.as_ptr();
        info.lpParameters = parameters.as_ptr();
        info.nShow = SW_HIDE;
        // SAFETY: `info` is a valid SHELLEXECUTEINFOW of its own size.
        if unsafe { ShellExecuteExW(&mut info) } == 0 {
            // SAFETY: read at once after the failed call.
            let code = unsafe { GetLastError() };
            return Err(if code == ERROR_CANCELLED {
                InstallFailure::Declined
            } else {
                InstallFailure::Failed(said(code))
            });
        }
        if info.hProcess.is_null() {
            return Err(InstallFailure::Failed(
                "Windows started wsl.exe without giving its process".into(),
            ));
        }
        let process = Handle(info.hProcess);
        // SAFETY: `process` is the open handle of the started wsl.exe.
        if unsafe { WaitForSingleObject(process.0, INFINITE) } != WAIT_OBJECT_0 {
            // SAFETY: read at once after the failed wait.
            return Err(InstallFailure::Failed(format!(
                "waiting for wsl.exe: {}",
                said(unsafe { GetLastError() })
            )));
        }
        let mut code = 0u32;
        // SAFETY: the open process handle and one out-pointer to a u32.
        if unsafe { GetExitCodeProcess(process.0, &mut code) } == 0 {
            // SAFETY: read at once after the failed call.
            return Err(InstallFailure::Failed(said(unsafe { GetLastError() })));
        }
        if code == 0 {
            Ok(())
        } else {
            Err(InstallFailure::Failed(format!(
                "wsl.exe --install --no-distribution ended with exit code {code:#010x}"
            )))
        }
    }

    const RUN_ONCE: &str = r"Software\Microsoft\Windows\CurrentVersion\RunOnce";

    pub fn restart_computer(program: &Path) -> Result<(), String> {
        let program = program
            .to_str()
            .ok_or_else(|| format!("the app's path {} is not UTF-8", program.display()))?;
        let key = run_once()?;
        let name = wide(super::RUN_ONCE_VALUE);
        let data = wide(&format!("\"{program}\""));
        // SAFETY: an open key, a NUL-terminated name, and REG_SZ data of
        // the given byte length (with its NUL, as REG_SZ wants).
        let set = unsafe {
            RegSetValueExW(
                key.0,
                name.as_ptr(),
                0,
                REG_SZ,
                data.as_ptr().cast(),
                (data.len() * 2) as u32,
            )
        };
        if set != ERROR_SUCCESS {
            return Err(said(set));
        }
        let restarted = enable_shutdown().and_then(|()| {
            let reason = SHTDN_REASON_MAJOR_APPLICATION
                | SHTDN_REASON_MINOR_INSTALLATION
                | SHTDN_REASON_FLAG_PLANNED;
            // SAFETY: null machine (this one) and message; no grace period,
            // the restart flag only (nothing forced).
            let code = unsafe {
                InitiateShutdownW(
                    std::ptr::null(),
                    std::ptr::null(),
                    0,
                    SHUTDOWN_RESTART,
                    reason,
                )
            };
            if super::restart_taken(code) {
                Ok(())
            } else {
                Err(said(code))
            }
        });
        if let Err(refused) = restarted {
            // SAFETY: the open key and the NUL-terminated value name.
            let removed = unsafe { RegDeleteValueW(key.0, name.as_ptr()) };
            if removed != ERROR_SUCCESS {
                return Err(format!(
                    "{refused}; removing the RunOnce value {}: {}",
                    super::RUN_ONCE_VALUE,
                    said(removed)
                ));
            }
            return Err(refused);
        }
        Ok(())
    }

    /// The person's own RunOnce key, opened to set and remove a value
    /// (made when the person has none).
    fn run_once() -> Result<Key, String> {
        let path = wide(RUN_ONCE);
        let mut key: HKEY = std::ptr::null_mut();
        // SAFETY: a predefined root key, a NUL-terminated subkey path, null
        // class and security attributes and disposition, one out-pointer.
        let code = unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                path.as_ptr(),
                0,
                std::ptr::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_SET_VALUE,
                std::ptr::null(),
                &mut key,
                std::ptr::null_mut(),
            )
        };
        if code != ERROR_SUCCESS {
            return Err(format!("HKEY_CURRENT_USER\\{RUN_ONCE}: {}", said(code)));
        }
        Ok(Key(key))
    }

    /// Enables the shutdown privilege in this process's token (every
    /// interactive user holds it, disabled).
    fn enable_shutdown() -> Result<(), String> {
        let mut token: HANDLE = std::ptr::null_mut();
        // SAFETY: this process's pseudo handle and one out-pointer.
        if unsafe {
            OpenProcessToken(
                GetCurrentProcess(),
                TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY,
                &mut token,
            )
        } == 0
        {
            // SAFETY: read at once after the failed call.
            return Err(said(unsafe { GetLastError() }));
        }
        let token = Handle(token);
        let name = wide("SeShutdownPrivilege");
        let mut luid = LUID {
            LowPart: 0,
            HighPart: 0,
        };
        // SAFETY: null system (this one), a NUL-terminated privilege name,
        // one out-pointer.
        if unsafe { LookupPrivilegeValueW(std::ptr::null(), name.as_ptr(), &mut luid) } == 0 {
            // SAFETY: read at once after the failed call.
            return Err(said(unsafe { GetLastError() }));
        }
        let privileges = TOKEN_PRIVILEGES {
            PrivilegeCount: 1,
            Privileges: [LUID_AND_ATTRIBUTES {
                Luid: luid,
                Attributes: SE_PRIVILEGE_ENABLED,
            }],
        };
        // SAFETY: the open token, one TOKEN_PRIVILEGES of its own size, no
        // previous state asked for.
        let ok = unsafe {
            AdjustTokenPrivileges(
                token.0,
                0,
                &privileges,
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        // SAFETY: read at once after the call (it sets ERROR_NOT_ALL_ASSIGNED
        // on success when the token lacks the privilege).
        let code = unsafe { GetLastError() };
        if ok == 0 || code != ERROR_SUCCESS {
            return Err(said(code));
        }
        Ok(())
    }

    pub fn boot_id() -> Result<u32, String> {
        const KEY: &str = r"SYSTEM\CurrentControlSet\Control\Session Manager\Memory Management\PrefetchParameters";
        let path = wide(KEY);
        let mut key: HKEY = std::ptr::null_mut();
        // SAFETY: a predefined root key, a NUL-terminated subkey path, one
        // out-pointer.
        let code = unsafe {
            RegOpenKeyExW(
                HKEY_LOCAL_MACHINE,
                path.as_ptr(),
                0,
                KEY_QUERY_VALUE,
                &mut key,
            )
        };
        if code != ERROR_SUCCESS {
            return Err(format!("HKEY_LOCAL_MACHINE\\{KEY}: {}", said(code)));
        }
        let key = Key(key);
        let name = wide("BootId");
        let mut kind = 0u32;
        let mut value = 0u32;
        let mut size = std::mem::size_of::<u32>() as u32;
        // SAFETY: the open key, a NUL-terminated value name, out-pointers to
        // a u32 type, a u32 buffer and its byte size.
        let code = unsafe {
            RegQueryValueExW(
                key.0,
                name.as_ptr(),
                std::ptr::null(),
                &mut kind,
                (&raw mut value).cast(),
                &mut size,
            )
        };
        if code != ERROR_SUCCESS {
            return Err(format!("HKEY_LOCAL_MACHINE\\{KEY}\\BootId: {}", said(code)));
        }
        if kind != REG_DWORD {
            return Err(format!(
                "HKEY_LOCAL_MACHINE\\{KEY}\\BootId is of type {kind}, not REG_DWORD"
            ));
        }
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_chats_work_is_under_the_drvfs_mount_with_slashes() {
        let root = PathBuf::from("data").join("sandbox");
        let workspace = Workspace {
            chat: "c1".into(),
            host: root.join("c1"),
        };
        assert_eq!(
            guest_path(&root, &workspace).unwrap(),
            PathBuf::from("/mnt/sandbox/c1")
        );
        let elsewhere = Workspace {
            chat: "c1".into(),
            host: PathBuf::from("other").join("c1"),
        };
        assert!(matches!(
            guest_path(&root, &elsewhere),
            Err(crate::SandboxError::Failed(m)) if m.contains("is not in the sandbox folder")
        ));
        assert_eq!(
            drvfs_path(Path::new(r"\\?\C:\x\sandbox")).unwrap(),
            r"C:\x\sandbox"
        );
        assert_eq!(drvfs_path(Path::new(r"D:\x")).unwrap(), r"D:\x");
        for refused in [
            r"\\?\UNC\server\share\sandbox",
            r"\\server\share\sandbox",
            r"C:\a;b\sandbox",
        ] {
            let error = drvfs_path(Path::new(refused)).unwrap_err();
            assert!(error.contains(refused), "{error}");
        }
    }

    // A second `RestartComputer` after Windows took the first: the restart is
    // under way, and the RunOnce value stays.
    #[test]
    fn a_restart_already_under_way_is_taken_and_any_other_code_is_a_refusal() {
        assert!(restart_taken(0));
        assert!(restart_taken(1115));
        for refused in [5, 1190, 1191, 1314] {
            assert!(!restart_taken(refused), "{refused}");
        }
    }

    #[test]
    fn the_import_tar_is_the_userland_with_the_distros_files() {
        let mut userland = tar::Builder::new(Vec::new());
        for (name, text) in [
            ("etc/passwd", "root:x:0:0:root:/root:/bin/sh\n"),
            ("usr/bin/busybox", "BB"),
        ] {
            let mut header = tar::Header::new_gnu();
            header.set_size(text.len() as u64);
            header.set_mode(0o755);
            userland
                .append_data(&mut header, name, text.as_bytes())
                .unwrap();
        }
        let tar = import_tar(&userland.into_inner().unwrap()).unwrap();
        let mut archive = tar::Archive::new(&tar[..]);
        let mut seen = Vec::new();
        for entry in archive.entries().unwrap() {
            let mut entry = entry.unwrap();
            let path = entry.path().unwrap().to_string_lossy().into_owned();
            let mut text = String::new();
            std::io::Read::read_to_string(&mut entry, &mut text).unwrap();
            if path == "etc/passwd" {
                assert_eq!(
                    text,
                    "root:x:0:0:root:/root:/usr/bin/sh\nmcbench:x:1000:1000::/var/mcbench:/usr/bin/sh\n"
                );
            }
            if path == "usr/bin/sh" {
                assert_eq!(
                    entry.link_name().unwrap().unwrap().to_str(),
                    Some("busybox")
                );
            }
            seen.push(path);
        }
        assert_eq!(
            seen,
            [
                "usr/bin/busybox",
                "usr/sbin",
                "usr/lib/mcbench",
                "etc/passwd",
                "etc/group",
                "etc/wsl.conf",
                "usr/lib/mcbench/wsl-init",
                "usr/bin/sh",
                "usr/bin/mount"
            ]
        );
    }

    #[cfg(windows)]
    #[test]
    fn the_user_is_the_build_sandboxs() {
        assert_eq!(GUEST_USER, mcbench_build::sandbox::wsl::USER_IDS);
        assert_eq!(USER, mcbench_build::sandbox::wsl::USER);
    }
}
