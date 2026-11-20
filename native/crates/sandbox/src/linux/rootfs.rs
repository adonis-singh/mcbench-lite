//! The pinned root filesystem: a tar.zst release asset that
//! `rootfs/build.sh` makes from `sandbox/Dockerfile`, unpacked and checked
//! once into the sandbox folder.
//!
//! The tarball holds, sorted by name and owned by 0:0:
//!
//! | Entry | What |
//! |---|---|
//! | `image.json` | the image's environment (`Config.Env`) and what it was built from |
//! | `pasta` | pasta, static, run on the host to give the networked box its network |
//! | `pasta.avx2` | on x86_64, pasta built for AVX2, which `pasta` execs in its place on a processor that has AVX2 (and which, missing, it logs as "Can't run AVX2 build, using non-AVX2 version: No such file or directory") |
//! | `rootfs/` | `docker export` of today's image, `/.dockerenv` and device nodes left out |
//!
//! The box binds `rootfs/` read-only at `/`; `pasta`, `pasta.avx2` and
//! `image.json` stay outside it. The pinned x86_64 asset of
//! `native/sandbox-assets.lock` (c373ac73…) was made before `rootfs/build.sh`
//! shipped `pasta.avx2` and has none.

use std::io::Read;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use sha2::{Digest, Sha256};

/// `image.json`.
#[derive(Deserialize, Debug, Clone)]
pub struct Image {
    /// The image's `Config.Env`.
    pub env: Vec<String>,
}

/// An unpacked, checked root filesystem.
#[derive(Debug, Clone)]
pub struct Unpacked {
    /// The folder the tarball was unpacked into.
    pub dir: PathBuf,
    /// `<dir>/rootfs`.
    pub rootfs: PathBuf,
    /// `<dir>/pasta`.
    pub pasta: PathBuf,
    /// `<dir>/image.json`.
    pub image: Image,
}

/// The programs the boxes run from the root filesystem.
pub const REQUIRED: [&str; 6] = [
    "usr/bin/bash",
    "usr/bin/sleep",
    "usr/bin/timeout",
    "usr/sbin/iptables",
    "usr/local/bin/python3",
    "usr/bin/sh",
];

fn sha256_of(file: &Path) -> Result<String, String> {
    let mut input = std::fs::File::open(file).map_err(|e| format!("{}: {e}", file.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1 << 20];
    loop {
        let n = input
            .read(&mut buffer)
            .map_err(|e| format!("{}: {e}", file.display()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

/// Unpacks `tarball` (pinned by `sha256`) under `root`, once: a folder
/// `.rootfs-<sha256>` holding a `.complete` marker is taken as it is.
pub fn unpack(tarball: &Path, sha256: &str, root: &Path) -> Result<Unpacked, String> {
    let dir = root.join(format!(".rootfs-{sha256}"));
    if !dir.join(".complete").exists() {
        let got = sha256_of(tarball)?;
        if got != sha256 {
            return Err(format!(
                "The bash sandbox's root filesystem {} has sha256 {got}, not the pinned {sha256}",
                tarball.display()
            ));
        }
        std::fs::create_dir_all(root).map_err(|e| format!("{}: {e}", root.display()))?;
        let partial = root.join(format!(".rootfs-{sha256}.partial-{}", std::process::id()));
        if partial.exists() {
            make_writable(&partial);
            std::fs::remove_dir_all(&partial).map_err(|e| format!("{}: {e}", partial.display()))?;
        }
        let input =
            std::fs::File::open(tarball).map_err(|e| format!("{}: {e}", tarball.display()))?;
        let decoder = ruzstd::decoding::StreamingDecoder::new(std::io::BufReader::new(input))
            .map_err(|e| format!("{}: {e}", tarball.display()))?;
        let mut archive = tar::Archive::new(decoder);
        archive.set_preserve_permissions(true);
        archive.set_preserve_mtime(true);
        archive.set_unpack_xattrs(false);
        archive
            .unpack(&partial)
            .map_err(|e| format!("unpacking {}: {e}", tarball.display()))?;
        // On the disk before it counts as complete: a machine (or the macOS
        // VM) stopped right after must not keep a marker over lost files.
        sync_file_system(&partial)?;
        std::fs::write(partial.join(".complete"), sha256).map_err(|e| e.to_string())?;
        std::fs::rename(&partial, &dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        sync_file_system(&dir)?;
    }
    check(&dir)
}

/// `syncfs(2)` of the file system that holds `dir` (Linux; the root
/// filesystem is unpacked only there and in the guests).
fn sync_file_system(dir: &Path) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        use std::os::fd::AsRawFd;
        let folder = std::fs::File::open(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        // SAFETY: syncfs on a descriptor we hold.
        if unsafe { libc::syncfs(folder.as_raw_fd()) } != 0 {
            return Err(format!(
                "syncing {}: {}",
                dir.display(),
                std::io::Error::last_os_error()
            ));
        }
    }
    #[cfg(not(target_os = "linux"))]
    let _ = dir;
    Ok(())
}

/// A folder the box cannot have written to may still hold read-only
/// folders from a failed unpack; they are made writable to be removed.
fn make_writable(dir: &Path) {
    #[cfg(not(unix))]
    let _ = dir;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                if entry.file_type().is_ok_and(|t| t.is_dir()) {
                    let _ = std::fs::set_permissions(
                        entry.path(),
                        std::fs::Permissions::from_mode(0o755),
                    );
                    make_writable(&entry.path());
                }
            }
        }
    }
}

/// The unpacked folder has what the boxes run.
pub fn check(dir: &Path) -> Result<Unpacked, String> {
    let rootfs = dir.join("rootfs");
    for program in REQUIRED {
        let file = rootfs.join(program);
        if std::fs::symlink_metadata(&file).is_err() {
            return Err(format!(
                "The bash sandbox's root filesystem has no /{program} ({})",
                dir.display()
            ));
        }
    }
    let pasta = dir.join("pasta");
    if !pasta.is_file() {
        return Err(format!(
            "The bash sandbox's root filesystem has no pasta ({})",
            dir.display()
        ));
    }
    let image: Image = serde_json::from_slice(
        &std::fs::read(dir.join("image.json"))
            .map_err(|e| format!("{}/image.json: {e}", dir.display()))?,
    )
    .map_err(|e| format!("{}/image.json: {e}", dir.display()))?;
    Ok(Unpacked {
        dir: dir.to_path_buf(),
        rootfs,
        pasta,
        image,
    })
}
