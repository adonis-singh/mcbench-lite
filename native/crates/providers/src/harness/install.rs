//! Installing the Claude Code or Codex CLI (`server/cli-install.js` and the
//! pins of `server/tools.js`): the pinned release is downloaded into the
//! tools folder, checked by sha256, and unpacked into `<name>-<version>`,
//! which appears whole (a rename), so a folder that exists is a finished
//! install. A CLI that is on PATH, or named by `MCBENCH_CLAUDE_BINARY` or
//! `MCBENCH_CODEX_BINARY`, is the one the app runs, and is not installed;
//! neither is one the tools folder has. `mcbench-setup` calls [`install`]
//! and [`CliInstaller`].

#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

use crate::changes::{Changes, Watcher};

/// A watcher hears of a download's progress at most this often; its start
/// and its end always.
pub const PROGRESS_EVERY: Duration = Duration::from_millis(100);

/// How a pinned download is unpacked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unpack {
    /// The download is the program.
    File,
    /// A `.tar.gz` unpacked into the install's folder.
    Tar,
}

/// One platform's pinned download.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Asset {
    /// Where it is downloaded from.
    pub url: String,
    /// Its sha256, hex.
    pub sha256: String,
    /// Its size in bytes.
    pub bytes: u64,
    /// The program's path inside the install's folder.
    pub bin: String,
}

/// A pinned program (`tools.js` `PINS`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pin {
    /// `claude` or `codex`.
    pub name: String,
    /// What the page and the log call it.
    pub title: String,
    /// The version, which names the folder.
    pub version: String,
    /// How it is unpacked.
    pub unpack: Unpack,
    /// Each platform's download, by `darwin-arm64`-style name.
    pub platforms: Vec<(String, Asset)>,
}

const CLAUDE_CODE: &str = "https://storage.googleapis.com/claude-code-dist-86c565f3-f756-42ad-8dfa-d59b1c096819/claude-code-releases/2.1.311";
const CODEX: &str = "https://github.com/openai/codex/releases/download/rust-v0.169.0";

fn asset(url: String, sha256: &str, bytes: u64, bin: &str) -> Asset {
    Asset {
        url,
        sha256: sha256.to_owned(),
        bytes,
        bin: bin.to_owned(),
    }
}

/// The pins of today's `tools.js`: Claude Code 2.1.311 and Codex CLI 0.169.0
/// for darwin-arm64, darwin-x64, linux-arm64 and linux-x64, and Claude
/// Code's win32-x64 build, which `tools.js` does not pin (its sha256 and size
/// are the release's own `manifest.json`'s). Codex has no Windows pin: on
/// Windows it runs in the app's WSL distro, so its install there is the
/// Linux one ([`platform_of`]).
pub fn pins() -> Vec<Pin> {
    vec![
        Pin {
            name: "claude".to_owned(),
            title: "Claude Code 2.1.311".to_owned(),
            version: "2.1.311".to_owned(),
            unpack: Unpack::File,
            platforms: vec![
                (
                    "darwin-arm64".to_owned(),
                    asset(
                        format!("{CLAUDE_CODE}/darwin-arm64/claude"),
                        "85d8c08707fcfd1e78ffa0204eefbc6ec9e4fe22f375fb2767ef85d957ed3161",
                        276185504,
                        "claude",
                    ),
                ),
                (
                    "darwin-x64".to_owned(),
                    asset(
                        format!("{CLAUDE_CODE}/darwin-x64/claude"),
                        "d34e1cb82015b050471a97574aa6ea6f698ecf3a97a61c980961f43ad9f838f5",
                        287420928,
                        "claude",
                    ),
                ),
                (
                    "linux-arm64".to_owned(),
                    asset(
                        format!("{CLAUDE_CODE}/linux-arm64/claude"),
                        "2e2496c9a8065df93acb0fca2d341f59a6b8777649b2e025ab8c9bcd5eef61f3",
                        293658808,
                        "claude",
                    ),
                ),
                (
                    "linux-x64".to_owned(),
                    asset(
                        format!("{CLAUDE_CODE}/linux-x64/claude"),
                        "86dc1fcc69498159eaf15f446613e847010f9036bfbbdffd4c49350be8489ada",
                        293776752,
                        "claude",
                    ),
                ),
                (
                    "win32-x64".to_owned(),
                    asset(
                        format!("{CLAUDE_CODE}/win32-x64/claude.exe"),
                        "966f4b2ce962298f5dee6a942e8c47c8f97392e69e728e622319a98b259c0680",
                        298870432,
                        "claude.exe",
                    ),
                ),
            ],
        },
        Pin {
            name: "codex".to_owned(),
            title: "Codex CLI 0.169.0".to_owned(),
            version: "0.169.0".to_owned(),
            unpack: Unpack::Tar,
            platforms: vec![
                (
                    "darwin-arm64".to_owned(),
                    asset(
                        format!("{CODEX}/codex-package-aarch64-apple-darwin.tar.gz"),
                        "8a232260df7a12c5e83e0b4b7659e1c8d9259b41942517bc0068d8ab55c36461",
                        172466618,
                        "bin/codex",
                    ),
                ),
                (
                    "darwin-x64".to_owned(),
                    asset(
                        format!("{CODEX}/codex-package-x86_64-apple-darwin.tar.gz"),
                        "0a3c392235b4712409495ced02bbe0662ce7a038232766d1c1c34af74e905c2f",
                        187188284,
                        "bin/codex",
                    ),
                ),
                (
                    "linux-arm64".to_owned(),
                    asset(
                        format!("{CODEX}/codex-package-aarch64-unknown-linux-musl.tar.gz"),
                        "c8d0b4ab2a97c595b70b10412d36d0d5464dcef1e1d3246b073d48017cdf5a4f",
                        194441793,
                        "bin/codex",
                    ),
                ),
                (
                    "linux-x64".to_owned(),
                    asset(
                        format!("{CODEX}/codex-package-x86_64-unknown-linux-musl.tar.gz"),
                        "3b8ed091149cc63a59046b9f094bcfa21cdb4dda15ede441ce369bcee4a71ba0",
                        192339692,
                        "bin/codex",
                    ),
                ),
            ],
        },
    ]
}

/// This platform's name as the pins key it (`process.platform-process.arch`).
pub fn platform() -> String {
    let os = match std::env::consts::OS {
        "macos" => "darwin",
        "windows" => "win32",
        other => other,
    };
    let arch = match std::env::consts::ARCH {
        "aarch64" => "arm64",
        "x86_64" => "x64",
        other => other,
    };
    format!("{os}-{arch}")
}

/// The platform whose build of `name` runs on `platform`: Codex on Windows
/// runs in the app's WSL distro, so its install is the Linux one of the
/// same processor (`win32-x64` installs `linux-x64`); every other is
/// `platform` itself.
pub fn platform_of(name: &str, platform: &str) -> String {
    match (name, platform.strip_prefix("win32-")) {
        ("codex", Some(arch)) => format!("linux-{arch}"),
        _ => platform.to_owned(),
    }
}

impl Pin {
    /// This platform's download, if pinned.
    pub fn asset(&self, platform: &str) -> Option<&Asset> {
        self.platforms
            .iter()
            .find(|(p, _)| p == platform)
            .map(|(_, a)| a)
    }

    /// `installed`: the folder it is installed in, and its program there.
    pub fn installed(&self, tools_dir: &Path, platform: &str) -> (PathBuf, PathBuf) {
        let folder = tools_dir.join(format!("{}-{}", self.name, self.version));
        let bin = self
            .asset(platform)
            .map(|a| a.bin.clone())
            .unwrap_or_default();
        let program = folder.join(bin);
        (folder, program)
    }

    /// `downloadMb`: the download in MB, rounded.
    pub fn download_mb(&self, platform: &str) -> u64 {
        self.asset(platform)
            .map_or(0, |a| (a.bytes as f64 / 1e6).round() as u64)
    }
}

/// Bytes received so far, and the total when the server said it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Progress {
    /// Received.
    pub done: u64,
    /// The whole, when known.
    pub total: Option<u64>,
}

/// How many times a download that fails on the network is tried in all.
const DOWNLOAD_ATTEMPTS: u32 = 3;
const RETRY: Duration = Duration::from_millis(1000);

enum Failure {
    Network(String),
    Final(String),
}

/// `download`: `url` into `target`, whose sha256 must be `sha256`; the
/// file appears whole once its checksum matched. A failure on the network
/// (no connection, a 5xx answer, a body that ends early) is tried again,
/// three times in all, a second apart; anything else fails at once.
pub async fn download(
    url: &str,
    target: &Path,
    sha256: &str,
    on_progress: &(dyn Fn(Progress) + Send + Sync),
) -> Result<(), String> {
    // The client every provider shares (`api::http::client`).
    let client = crate::api::http::client();
    let mut attempt = 1;
    loop {
        match download_once(&client, url, target, sha256, on_progress).await {
            Ok(()) => return Ok(()),
            Err(Failure::Final(message)) => return Err(message),
            Err(Failure::Network(message)) => {
                if attempt == DOWNLOAD_ATTEMPTS {
                    return Err(format!("{message} (tried {DOWNLOAD_ATTEMPTS} times)"));
                }
                eprintln!(
                    "[tools] {message}: trying again ({} of {DOWNLOAD_ATTEMPTS})",
                    attempt + 1
                );
                attempt += 1;
                tokio::time::sleep(RETRY).await;
            }
        }
    }
}

fn root_cause(error: &dyn std::error::Error) -> String {
    let mut cause = error;
    while let Some(next) = cause.source() {
        cause = next;
    }
    cause.to_string()
}

async fn download_once(
    client: &reqwest::Client,
    url: &str,
    target: &Path,
    sha256: &str,
    on_progress: &(dyn Fn(Progress) + Send + Sync),
) -> Result<(), Failure> {
    let failed = |why: String| Failure::Network(format!("The download of {url} failed: {why}"));
    let mut response = client
        .get(url)
        .send()
        .await
        .map_err(|e| failed(root_cause(&e)))?;
    let status = response.status().as_u16();
    if status >= 500 {
        return Err(failed(format!("HTTP {status}")));
    }
    if !response.status().is_success() {
        return Err(Failure::Final(format!(
            "The download of {url} failed (HTTP {status})"
        )));
    }
    let total = response.content_length().filter(|t| *t > 0);
    let temporary = PathBuf::from(format!(
        "{}.{}.download",
        target.display(),
        std::process::id()
    ));
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| Failure::Final(format!("mkdir {}: {e}", parent.display())))?;
    }
    let mut file = tokio::fs::File::create(&temporary)
        .await
        .map_err(|e| Failure::Final(format!("open {}: {e}", temporary.display())))?;
    let mut hash = Sha256::new();
    let mut done = 0u64;
    let body: Result<(), Failure> = async {
        loop {
            let chunk = response.chunk().await.map_err(|e| failed(root_cause(&e)))?;
            let Some(chunk) = chunk else { break };
            hash.update(&chunk);
            file.write_all(&chunk)
                .await
                .map_err(|e| failed(e.to_string()))?;
            done += chunk.len() as u64;
            on_progress(Progress { done, total });
        }
        if let Some(total) = total
            && done < total
        {
            return Err(failed(format!("it ended after {done} of {total} bytes")));
        }
        file.flush().await.map_err(|e| failed(e.to_string()))?;
        Ok(())
    }
    .await;
    drop(file);
    if let Err(error) = body {
        let _ = std::fs::remove_file(&temporary);
        return Err(error);
    }
    let got: String = hash.finalize().iter().map(|b| format!("{b:02x}")).collect();
    if got != sha256 {
        let _ = std::fs::remove_file(&temporary);
        return Err(Failure::Final(format!(
            "{url} downloaded with sha256 {got}, but mcbench-lite expects {sha256}. It was not kept."
        )));
    }
    std::fs::rename(&temporary, target)
        .map_err(|e| Failure::Final(format!("rename {}: {e}", target.display())))
}

fn alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        // SAFETY: kill(2) with signal 0 sends nothing; it only checks that
        // the process exists, and touches no memory of this process.
        let result = unsafe { libc::kill(pid as libc::pid_t, 0) };
        result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    }
    #[cfg(not(unix))]
    {
        pid == std::process::id()
    }
}

/// `withLock`: runs `work` while this process holds the file `lock`, which
/// another process waits for (every 200 ms); a lock whose holder is gone is
/// taken over.
async fn with_lock<T>(
    lock: &Path,
    work: impl std::future::Future<Output = Result<T, String>>,
) -> Result<T, String> {
    if let Some(parent) = lock.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
    }
    loop {
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(lock)
        {
            Ok(mut file) => {
                std::io::Write::write_all(&mut file, std::process::id().to_string().as_bytes())
                    .map_err(|e| format!("write {}: {e}", lock.display()))?;
                break;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                let holder: u32 = std::fs::read_to_string(lock)
                    .ok()
                    .and_then(|t| t.trim().parse().ok())
                    .unwrap_or(0);
                if holder == 0 || !alive(holder) {
                    let _ = std::fs::remove_file(lock);
                    continue;
                }
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
            Err(e) => return Err(format!("open {}: {e}", lock.display())),
        }
    }
    let result = work.await;
    let _ = std::fs::remove_file(lock);
    result
}

/// `install`: installs `pin` into `tools_dir` unless it is installed, in
/// one process at a time, telling `on_progress` how far its download is.
/// Answers the path of its program.
pub async fn install(
    pin: &Pin,
    tools_dir: &Path,
    platform: &str,
    on_progress: &(dyn Fn(Progress) + Send + Sync),
) -> Result<PathBuf, String> {
    let asset = pin
        .asset(platform)
        .ok_or_else(|| format!("mcbench-lite has no pinned {} for {platform}", pin.title))?;
    let (folder, program) = pin.installed(tools_dir, platform);
    if folder.exists() {
        return Ok(program);
    }
    // Private to the account, as the data folder is.
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder
        .create(tools_dir)
        .map_err(|e| format!("mkdir {}: {e}", tools_dir.display()))?;
    let lock = PathBuf::from(format!("{}.lock", folder.display()));
    with_lock(&lock, async {
        if folder.exists() {
            return Ok(());
        }
        let pid = std::process::id();
        let staging = PathBuf::from(format!("{}.{pid}.partial", folder.display()));
        let _ = std::fs::remove_dir_all(&staging);
        std::fs::create_dir(&staging).map_err(|e| format!("mkdir {}: {e}", staging.display()))?;
        let unpack = match pin.unpack {
            Unpack::File => "file",
            Unpack::Tar => "tar",
        };
        let archive = tools_dir.join(format!("{}-{}.{pid}.{unpack}", pin.name, pin.version));
        let result = async {
            let target = match pin.unpack {
                Unpack::File => staging.join(&asset.bin),
                Unpack::Tar => archive.clone(),
            };
            download(&asset.url, &target, &asset.sha256, on_progress).await?;
            match pin.unpack {
                Unpack::File => {
                    #[cfg(unix)]
                    std::fs::set_permissions(
                        &target,
                        std::os::unix::fs::PermissionsExt::from_mode(0o755),
                    )
                    .map_err(|e| format!("chmod {}: {e}", target.display()))?;
                }
                Unpack::Tar => {
                    let file = std::fs::File::open(&archive)
                        .map_err(|e| format!("open {}: {e}", archive.display()))?;
                    tar::Archive::new(flate2::read::GzDecoder::new(file))
                        .unpack(&staging)
                        .map_err(|e| format!("tar -xzf {}: {e}", archive.display()))?;
                    // On Linux Codex runs in its box beside the commands it
                    // runs for the model: its programs execute-only, so
                    // the kernel makes them undumpable and no command opens
                    // their /proc/<pid>/fd (the app's pipes).
                    #[cfg(target_os = "linux")]
                    mcbench_sandbox::proxy::execute_only(&staging)?;
                }
            }
            std::fs::rename(&staging, &folder)
                .map_err(|e| format!("rename {}: {e}", folder.display()))
        }
        .await;
        let _ = std::fs::remove_file(&archive);
        let _ = std::fs::remove_dir_all(&staging);
        result
    })
    .await?;
    Ok(program)
}

/// What the CLI lookups read of the app's environment, read once by the
/// core: `PATH`, `MCBENCH_CLAUDE_BINARY` and `MCBENCH_CODEX_BINARY`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Lookup {
    /// `PATH`.
    pub path: std::ffi::OsString,
    /// `MCBENCH_CLAUDE_BINARY`, when set and not empty.
    pub claude_binary: Option<String>,
    /// `MCBENCH_CODEX_BINARY`, when set and not empty.
    pub codex_binary: Option<String>,
}

impl Lookup {
    /// This process's.
    pub fn from_env() -> Lookup {
        let named = |key: &str| std::env::var(key).ok().filter(|v| !v.is_empty());
        Lookup {
            path: std::env::var_os("PATH").unwrap_or_default(),
            claude_binary: named("MCBENCH_CLAUDE_BINARY"),
            codex_binary: named("MCBENCH_CODEX_BINARY"),
        }
    }

    /// The variable's value for `name` (`PROGRAMS`).
    pub fn named(&self, name: &str) -> Option<&str> {
        match name {
            "claude" => self.claude_binary.as_deref(),
            "codex" => self.codex_binary.as_deref(),
            _ => None,
        }
    }

    /// `onPath`: whether `program` is on PATH as a spawn would find it.
    pub fn on_path(&self, program: &str) -> bool {
        on_path(program, &self.path)
    }
}

/// `onPath`: whether `program` is on `path` as a spawn would find it: on
/// Windows a name without an extension is looked for as `<name>.exe`, as
/// `CreateProcess` and Rust's `Command` look for it.
pub fn on_path(program: &str, path: &std::ffi::OsStr) -> bool {
    let file = if cfg!(windows) && Path::new(program).extension().is_none() {
        format!("{program}.exe")
    } else {
        program.to_owned()
    };
    std::env::split_paths(path)
        .filter(|folder| !folder.as_os_str().is_empty())
        .any(|folder| executable(&folder.join(&file)))
}

fn executable(file: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let Ok(path) = std::ffi::CString::new(file.as_os_str().as_bytes()) else {
            return false;
        };
        // SAFETY: access(2) reads the NUL-terminated path, which lives
        // until the call returns.
        unsafe { libc::access(path.as_ptr(), libc::X_OK) == 0 }
    }
    #[cfg(not(unix))]
    {
        file.is_file()
    }
}

/// `cliProgram`: the program the app runs for `name` when no variable
/// names one: the one on PATH, else the tools folder's.
pub fn cli_program(pin: &Pin, tools_dir: &Path, platform: &str, lookup: &Lookup) -> PathBuf {
    let target = platform_of(&pin.name, platform);
    if target != platform {
        // Run in the WSL distro: only the tools folder's Linux install.
        return pin.installed(tools_dir, &target).1;
    }
    if lookup.on_path(&pin.name) {
        PathBuf::from(&pin.name)
    } else {
        pin.installed(tools_dir, platform).1
    }
}

/// `cliInstalled`: whether the program the app runs for `name` is there
/// to run: the one its variable names (a path that exists, or a bare
/// command on PATH), else one on PATH, else the tools folder's.
pub fn cli_installed(pin: &Pin, tools_dir: &Path, platform: &str, lookup: &Lookup) -> bool {
    let target = platform_of(&pin.name, platform);
    if target != platform {
        return pin.installed(tools_dir, &target).1.exists();
    }
    match lookup.named(&pin.name) {
        Some(named) if named.contains(std::path::is_separator) => Path::new(named).exists(),
        Some(named) => lookup.on_path(named),
        None => lookup.on_path(&pin.name) || pin.installed(tools_dir, platform).1.exists(),
    }
}

/// The variable that names each CLI's program (`PROGRAMS`).
pub const PROGRAMS: [(&str, &str); 2] = [
    ("claude", "MCBENCH_CLAUDE_BINARY"),
    ("codex", "MCBENCH_CODEX_BINARY"),
];

/// An install as the page reads it (`view`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstallView {
    /// `running`, `done` or `failed`.
    pub state: String,
    /// Its download's progress while running.
    pub progress: Option<Progress>,
    /// Why it failed.
    pub error: Option<String>,
}

/// Why an install could not start, with the HTTP status today's route
/// answers (400: no such CLI; 409: installed already).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstallRefused {
    /// The status.
    pub status: u16,
    /// The message.
    pub message: String,
}

/// What does the install: `install` above, or a test's stand-in.
pub type Installer = Arc<
    dyn Fn(
            Pin,
            Arc<dyn Fn(Progress) + Send + Sync>,
        )
            -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<PathBuf, String>> + Send>>
        + Send
        + Sync,
>;

/// `cli-install.js`: one install per CLI at a time; the latest stays until
/// the next starts, so the page can show how it ended.
pub struct CliInstaller {
    tools_dir: PathBuf,
    platform: String,
    lookup: Lookup,
    installer: Installer,
    on_installed: Arc<dyn Fn(&str) + Send + Sync>,
    installs: Arc<Mutex<HashMap<String, InstallView>>>,
    changes: Arc<Changes<(String, InstallView)>>,
}

impl CliInstaller {
    /// Installs into `tools_dir` with [`install`]; `on_installed` runs when
    /// one is done (Settings reads the runtimes again).
    pub fn new(
        tools_dir: PathBuf,
        lookup: Lookup,
        on_installed: Arc<dyn Fn(&str) + Send + Sync>,
    ) -> Self {
        let dir = tools_dir.clone();
        let platform = platform();
        let installer_platform = platform.clone();
        let installer: Installer = Arc::new(
            move |pin: Pin, progress: Arc<dyn Fn(Progress) + Send + Sync>| {
                let dir = dir.clone();
                let platform = platform_of(&pin.name, &installer_platform);
                Box::pin(async move { install(&pin, &dir, &platform, &*progress).await })
            },
        );
        Self::with_installer(tools_dir, platform, lookup, installer, on_installed)
    }

    /// The same with another installer (tests).
    pub fn with_installer(
        tools_dir: PathBuf,
        platform: String,
        lookup: Lookup,
        installer: Installer,
        on_installed: Arc<dyn Fn(&str) + Send + Sync>,
    ) -> Self {
        CliInstaller {
            tools_dir,
            platform,
            lookup,
            installer,
            on_installed,
            installs: Arc::new(Mutex::new(HashMap::new())),
            changes: Arc::default(),
        }
    }

    /// Calls `watcher` with each change of an install's view, of every
    /// CLI: its start, its download's progress (at most once per
    /// [`PROGRESS_EVERY`]) and its end ([`crate::changes`]). Set once.
    pub fn watch(&self, watcher: Watcher<(String, InstallView)>) {
        self.changes.watch(watcher);
    }

    fn pin(name: &str) -> Result<Pin, InstallRefused> {
        pins()
            .into_iter()
            .find(|p| p.name == name)
            .ok_or_else(|| InstallRefused {
                status: 400,
                message: format!("No CLI named {name} can be installed: claude or codex"),
            })
    }

    /// `start`: begins installing `name`, or answers the install under way.
    pub fn start(&self, name: &str) -> Result<InstallView, InstallRefused> {
        let pin = Self::pin(name)?;
        let mut installs = self.installs.lock().unwrap();
        if let Some(running) = installs.get(name).filter(|i| i.state == "running") {
            return Ok(running.clone());
        }
        if self.lookup.named(name).is_some()
            || cli_installed(&pin, &self.tools_dir, &self.platform, &self.lookup)
        {
            return Err(InstallRefused {
                status: 409,
                message: format!("{name} is installed already, and the app runs that one"),
            });
        }
        let view = InstallView {
            state: "running".to_owned(),
            progress: None,
            error: None,
        };
        installs.insert(name.to_owned(), view.clone());
        self.changes.changed((name.to_owned(), view));
        drop(installs);
        eprintln!(
            "[tools] installing {} ({} MB) into {}",
            pin.title,
            pin.download_mb(&platform_of(&pin.name, &self.platform)),
            self.tools_dir.display()
        );
        let progress = {
            let installs = self.installs.clone();
            let changes = self.changes.clone();
            let name = name.to_owned();
            let told: Mutex<Option<std::time::Instant>> = Mutex::new(None);
            Arc::new(move |bytes: Progress| {
                let mut installs = installs.lock().unwrap();
                let Some(install) = installs.get_mut(&name) else {
                    return;
                };
                if install.progress == Some(bytes) {
                    return;
                }
                install.progress = Some(bytes);
                let mut told = told.lock().unwrap();
                if told.is_none_or(|at| at.elapsed() >= PROGRESS_EVERY) {
                    *told = Some(std::time::Instant::now());
                    changes.changed((name.clone(), install.clone()));
                }
            })
        };
        let job = (self.installer)(pin.clone(), progress);
        let view = self.installs.lock().unwrap().get(name).cloned().unwrap();
        let installs = self.installs.clone();
        let changes = self.changes.clone();
        let on_installed = self.on_installed.clone();
        let key = name.to_owned();
        let title = pin.title.clone();
        tokio::spawn(async move {
            let name = key;
            let outcome = job.await;
            if outcome.is_ok() {
                on_installed(&name);
            }
            let mut installs = installs.lock().unwrap();
            let install = installs.get_mut(&name).unwrap();
            install.progress = None;
            match outcome {
                Ok(_) => {
                    install.state = "done".to_owned();
                    eprintln!("[tools] {title} installed");
                }
                Err(error) => {
                    install.state = "failed".to_owned();
                    eprintln!("[tools] {title} was not installed: {error}");
                    install.error = Some(error);
                }
            }
            changes.changed((name, install.clone()));
        });
        Ok(view)
    }

    /// `state`: the latest install of `name`, or `None` when none started.
    pub fn state(&self, name: &str) -> Result<Option<InstallView>, InstallRefused> {
        Self::pin(name)?;
        Ok(self.installs.lock().unwrap().get(name).cloned())
    }
}
