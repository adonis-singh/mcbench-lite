//! Pinned downloads (`server/tools.js`): a download is kept only when its
//! sha256 is the pinned one, and appears whole (a rename); a program is
//! pinned per platform and installed into a folder named after its version,
//! which appears whole, so a folder that exists is a finished install. The
//! native app's pinned programs are the provider CLIs (Claude Code and
//! Codex, installed on request: `mcbench-providers`); the `bash` root
//! filesystem is a pinned download the sandbox unpacks.

use std::path::{Path, PathBuf};
use std::time::Duration;

use sha2::Digest;

use crate::jar::hex;
use crate::steps::Bytes;

/// A download's address and the sha256 and size of its bytes.
#[derive(Clone, Debug, PartialEq)]
pub struct Download {
    /// Its address.
    pub url: String,
    /// The sha256 of its bytes.
    pub sha256: String,
    /// Its size in bytes (the one figure the interface states).
    pub bytes: u64,
}

/// How a program's download is unpacked.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Unpack {
    /// The file is the program, made executable.
    File,
    /// A `.tar.gz`, unpacked by the system's `tar`.
    Tar,
}

/// One platform's download of a program.
#[derive(Clone, Debug, PartialEq)]
pub struct PlatformPin {
    /// The platform (`darwin-arm64`, as Node names `process.platform` and
    /// `process.arch`).
    pub platform: &'static str,
    /// Its download.
    pub download: Download,
    /// The program's path in its folder.
    pub bin: &'static str,
}

/// A program pinned to a version and, per platform, to its download.
#[derive(Clone, Debug, PartialEq)]
pub struct Program {
    /// Its name, which names its folder.
    pub name: &'static str,
    /// What the interface calls it.
    pub title: &'static str,
    /// Its version.
    pub version: &'static str,
    /// How it is unpacked.
    pub unpack: Unpack,
    /// Its downloads.
    pub platforms: Vec<PlatformPin>,
}

/// This machine's platform as Node names it (`darwin-arm64`, `linux-x64`,
/// `win32-x64`): the pins' keys.
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

const CODEX: &str = "https://github.com/openai/codex/releases/download/rust-v0.169.0";
const CLAUDE_CODE: &str = "https://storage.googleapis.com/claude-code-dist-86c565f3-f756-42ad-8dfa-d59b1c096819/claude-code-releases/2.1.311";

fn pin(
    platform: &'static str,
    url: String,
    sha256: &str,
    bytes: u64,
    bin: &'static str,
) -> PlatformPin {
    PlatformPin {
        platform,
        download: Download {
            url,
            sha256: sha256.into(),
            bytes,
        },
        bin,
    }
}

/// The provider CLIs' pins, as `server/tools.js` pins them (held by a test
/// against Node's `PINS`).
pub fn programs() -> Vec<Program> {
    vec![
        Program {
            name: "claude",
            title: "Claude Code 2.1.311",
            version: "2.1.311",
            unpack: Unpack::File,
            platforms: vec![
                pin(
                    "darwin-arm64",
                    format!("{CLAUDE_CODE}/darwin-arm64/claude"),
                    "85d8c08707fcfd1e78ffa0204eefbc6ec9e4fe22f375fb2767ef85d957ed3161",
                    276185504,
                    "claude",
                ),
                pin(
                    "darwin-x64",
                    format!("{CLAUDE_CODE}/darwin-x64/claude"),
                    "d34e1cb82015b050471a97574aa6ea6f698ecf3a97a61c980961f43ad9f838f5",
                    287420928,
                    "claude",
                ),
                pin(
                    "linux-arm64",
                    format!("{CLAUDE_CODE}/linux-arm64/claude"),
                    "2e2496c9a8065df93acb0fca2d341f59a6b8777649b2e025ab8c9bcd5eef61f3",
                    293658808,
                    "claude",
                ),
                pin(
                    "linux-x64",
                    format!("{CLAUDE_CODE}/linux-x64/claude"),
                    "86dc1fcc69498159eaf15f446613e847010f9036bfbbdffd4c49350be8489ada",
                    293776752,
                    "claude",
                ),
            ],
        },
        Program {
            name: "codex",
            title: "Codex CLI 0.169.0",
            version: "0.169.0",
            unpack: Unpack::Tar,
            platforms: vec![
                pin(
                    "darwin-arm64",
                    format!("{CODEX}/codex-package-aarch64-apple-darwin.tar.gz"),
                    "8a232260df7a12c5e83e0b4b7659e1c8d9259b41942517bc0068d8ab55c36461",
                    172466618,
                    "bin/codex",
                ),
                pin(
                    "darwin-x64",
                    format!("{CODEX}/codex-package-x86_64-apple-darwin.tar.gz"),
                    "0a3c392235b4712409495ced02bbe0662ce7a038232766d1c1c34af74e905c2f",
                    187188284,
                    "bin/codex",
                ),
                pin(
                    "linux-arm64",
                    format!("{CODEX}/codex-package-aarch64-unknown-linux-musl.tar.gz"),
                    "c8d0b4ab2a97c595b70b10412d36d0d5464dcef1e1d3246b073d48017cdf5a4f",
                    194441793,
                    "bin/codex",
                ),
                pin(
                    "linux-x64",
                    format!("{CODEX}/codex-package-x86_64-unknown-linux-musl.tar.gz"),
                    "3b8ed091149cc63a59046b9f094bcfa21cdb4dda15ede441ce369bcee4a71ba0",
                    192339692,
                    "bin/codex",
                ),
            ],
        },
    ]
}

/// BlueMap's version, as `server/bluemap.js` pins it (`VERSION`).
pub const BLUEMAP_VERSION: &str = "5.22";

/// BlueMap's CLI jar, as `server/bluemap.js` pins it (`JAR_URL`,
/// `JAR_SHA256`; held by a test against that file): a shared page shows
/// builds with the web app inside it (`mcbench_tools::share_viewer`).
pub fn bluemap() -> Download {
    Download {
        url: format!(
            "https://github.com/BlueMap-Minecraft/BlueMap/releases/download/v{BLUEMAP_VERSION}/bluemap-{BLUEMAP_VERSION}-cli.jar"
        ),
        sha256: "a2289d649f6a83e04cf222abcd3d4f2df4e32e61147015f921c05066f63eefb7".into(),
        bytes: 6_633_996,
    }
}

/// A download that fails on the network (no connection, a 5xx answer, a
/// body that ends early) is tried again, this many times in all,
/// [`RETRY`] apart. Any other failure, a checksum that does not match or a
/// 4xx answer, fails at once.
pub const ATTEMPTS: u32 = 3;
/// The wait between attempts.
pub const RETRY: Duration = Duration::from_secs(1);

enum Failure {
    Network(String),
    Other(String),
}

/// Downloads `url` to `target`, whose sha256 must be `sha256`, telling
/// `progress` the bytes received. The file appears whole (a rename) only
/// once its checksum matched; nothing of a failed download is kept.
pub async fn download(
    client: &reqwest::Client,
    url: &str,
    target: &Path,
    sha256: &str,
    progress: &(dyn Fn(Bytes) + Send + Sync),
) -> Result<(), String> {
    let mut attempt = 1;
    loop {
        match download_once(client, url, target, sha256, progress).await {
            Ok(()) => return Ok(()),
            Err(Failure::Other(message)) => return Err(message),
            Err(Failure::Network(message)) if attempt == ATTEMPTS => {
                return Err(format!("{message} (tried {ATTEMPTS} times)"));
            }
            Err(Failure::Network(_)) => {
                attempt += 1;
                tokio::time::sleep(RETRY).await;
            }
        }
    }
}

async fn download_once(
    client: &reqwest::Client,
    url: &str,
    target: &Path,
    sha256: &str,
    progress: &(dyn Fn(Bytes) + Send + Sync),
) -> Result<(), Failure> {
    let failed = |why: String| Failure::Network(format!("The download of {url} failed: {why}"));
    let mut response = client
        .get(url)
        .send()
        .await
        .map_err(|e| failed(cause(&e)))?;
    let status = response.status().as_u16();
    if status >= 500 {
        return Err(failed(format!("HTTP {status}")));
    }
    if !(200..300).contains(&status) {
        return Err(Failure::Other(format!(
            "The download of {url} failed (HTTP {status})"
        )));
    }
    let total = response.content_length().filter(|&n| n > 0);
    let parent = target
        .parent()
        .ok_or_else(|| Failure::Other(format!("{} has no folder", target.display())))?;
    tokio::fs::create_dir_all(parent)
        .await
        .map_err(|e| Failure::Other(format!("{}: {e}", parent.display())))?;
    let temporary = PathBuf::from(format!(
        "{}.{}.download",
        target.display(),
        std::process::id()
    ));
    let written = async {
        use tokio::io::AsyncWriteExt;
        let mut file = tokio::fs::File::create(&temporary)
            .await
            .map_err(|e| Failure::Other(format!("{}: {e}", temporary.display())))?;
        let mut hash = sha2::Sha256::new();
        let mut done = 0u64;
        while let Some(chunk) = response.chunk().await.map_err(|e| failed(cause(&e)))? {
            hash.update(&chunk);
            file.write_all(&chunk)
                .await
                .map_err(|e| Failure::Other(format!("{}: {e}", temporary.display())))?;
            done += chunk.len() as u64;
            progress(Bytes { done, total });
        }
        if let Some(total) = total
            && done < total
        {
            return Err(failed(format!("it ended after {done} of {total} bytes")));
        }
        file.flush()
            .await
            .map_err(|e| Failure::Other(format!("{}: {e}", temporary.display())))?;
        Ok(hex(&hash.finalize()))
    }
    .await;
    let got = match written {
        Ok(got) => got,
        Err(e) => {
            let _ = tokio::fs::remove_file(&temporary).await;
            return Err(e);
        }
    };
    if got != sha256 {
        let _ = tokio::fs::remove_file(&temporary).await;
        return Err(Failure::Other(format!(
            "{url} downloaded with sha256 {got}, but mcbench-lite expects {sha256}. It was not kept."
        )));
    }
    tokio::fs::rename(&temporary, target)
        .await
        .map_err(|e| Failure::Other(format!("{}: {e}", target.display())))
}

/// A reqwest error's deepest cause, as Node's fetch says `error.cause.message`.
pub(crate) fn cause(error: &reqwest::Error) -> String {
    let mut source: &dyn std::error::Error = error;
    while let Some(next) = source.source() {
        source = next;
    }
    source.to_string()
}

/// The folder `program` is installed in, and the path of its program there.
pub fn installed(
    program: &Program,
    dir: &Path,
    platform: &str,
) -> Result<(PathBuf, PathBuf), String> {
    let pin = program
        .platforms
        .iter()
        .find(|p| p.platform == platform)
        .ok_or_else(|| {
            format!(
                "mcbench-lite has no pinned {} for {platform}",
                program.title
            )
        })?;
    let folder = dir.join(format!("{}-{}", program.name, program.version));
    let bin = folder.join(pin.bin);
    Ok((folder, bin))
}

/// Installs `program` for `platform` into `dir` unless it is installed, one
/// process at a time (a lock beside its folder), telling `progress` how far
/// its download is. Answers the path of its program.
pub async fn install(
    client: &reqwest::Client,
    program: &Program,
    dir: &Path,
    platform: &str,
    progress: &(dyn Fn(Bytes) + Send + Sync),
) -> Result<PathBuf, String> {
    let (folder, bin) = installed(program, dir, platform)?;
    let pin = program
        .platforms
        .iter()
        .find(|p| p.platform == platform)
        .expect("installed found it");
    if folder.is_dir() {
        return Ok(bin);
    }
    create_private(dir)?;
    let lock = PathBuf::from(format!("{}.lock", folder.display()));
    with_lock(&lock, async {
        if folder.is_dir() {
            return Ok(());
        }
        let staging = PathBuf::from(format!(
            "{}.{}.partial",
            folder.display(),
            std::process::id()
        ));
        crate::data::remove(&staging)?;
        std::fs::create_dir_all(&staging).map_err(|e| format!("{}: {e}", staging.display()))?;
        let archive = dir.join(format!(
            "{}-{}.{}.archive",
            program.name,
            program.version,
            std::process::id()
        ));
        let result = async {
            let target = if program.unpack == Unpack::File {
                staging.join(pin.bin)
            } else {
                archive.clone()
            };
            download(
                client,
                &pin.download.url,
                &target,
                &pin.download.sha256,
                progress,
            )
            .await?;
            match program.unpack {
                Unpack::File => executable(&target)?,
                Unpack::Tar => {
                    let status = tokio::process::Command::from(mcbench_spawn::command("tar"))
                        .arg("-xzf")
                        .arg(&archive)
                        .arg("-C")
                        .arg(&staging)
                        .status()
                        .await
                        .map_err(|e| format!("tar: {e}"))?;
                    if !status.success() {
                        return Err(format!("tar could not unpack {}", archive.display()));
                    }
                }
            }
            std::fs::rename(&staging, &folder).map_err(|e| format!("{}: {e}", folder.display()))
        }
        .await;
        crate::data::remove(&archive)?;
        crate::data::remove(&staging)?;
        result
    })
    .await?;
    Ok(bin)
}

/// The tools folder, private to the person's account as the data folder
/// is: the programs in it run as them, and are checked only when they are
/// downloaded.
fn create_private(dir: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)
            .map_err(|e| format!("{}: {e}", dir.display()))
    }
    #[cfg(not(unix))]
    {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))
    }
}

fn executable(file: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(file, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| format!("{}: {e}", file.display()))
    }
    #[cfg(not(unix))]
    {
        let _ = file;
        Ok(())
    }
}

/// Runs `work` while this process holds the file `lock`, which another
/// process waits for (every 200 ms); a lock whose holder is gone is taken
/// over (`server/pid-lock.js` withLock).
pub async fn with_lock<T>(
    lock: &Path,
    work: impl std::future::Future<Output = Result<T, String>>,
) -> Result<T, String> {
    if let Some(parent) = lock.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    loop {
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(lock)
        {
            Ok(mut file) => {
                use std::io::Write;
                file.write_all(std::process::id().to_string().as_bytes())
                    .map_err(|e| format!("{}: {e}", lock.display()))?;
                break;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                let holder = std::fs::read_to_string(lock).unwrap_or_default();
                if !crate::instance::alive(&holder) {
                    let _ = std::fs::remove_file(lock);
                    continue;
                }
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
            Err(e) => return Err(format!("{}: {e}", lock.display())),
        }
    }
    let result = work.await;
    let _ = std::fs::remove_file(lock);
    result
}
