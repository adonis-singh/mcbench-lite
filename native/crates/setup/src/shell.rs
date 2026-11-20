//! Where the model's shell's files come from: the `bash` root filesystem
//! and, where a guest runs the boxes (macOS, Windows), the guest base, each
//! a file and its sha256 ([`files`]).
//!
//! A shipped build (feature `shipped-data`) takes them from the release's
//! pinned assets, `native/sandbox-assets.lock` ([`LOCK`], compiled in):
//! this platform's lines ([`Assets::wanted`]) are fetched from the lock's
//! `url` (or, when the build had `MCBENCH_SANDBOX_ASSETS_URL` set, from
//! that address: a staging build) into `<data folder>/sandbox-assets/`
//! ([`fetch`]), each checked against its size and sha256 before anything
//! uses it and kept under a name holding its sha256, so a later start
//! fetches nothing. While the lock's `url` is [`UNPUBLISHED`] and the build
//! names no address, the shell step is unavailable with [`NOT_INCLUDED`].
//! A shipped build reads no environment variable for these files.
//!
//! Any other build (tests, `native/check.sh`, development) fetches nothing:
//! it takes the files the environment names (`MCBENCH_SANDBOX_ROOTFS` with
//! `MCBENCH_SANDBOX_ROOTFS_SHA256`, `MCBENCH_SANDBOX_GUEST` with
//! `MCBENCH_SANDBOX_GUEST_SHA256`), and the shell step is unavailable with
//! [`NOT_INCLUDED`] without them.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio_util::sync::CancellationToken;

use crate::steps::{StepError, Steps};

/// A file and its sha256 (hex).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pinned {
    /// The file.
    pub file: PathBuf,
    /// Its sha256, hex.
    pub sha256: String,
}

/// The model's shell's files.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShellFiles {
    /// The root filesystem's tar.zst (`mcbench-sandbox`'s `rootfs/build.sh`).
    pub rootfs: Pinned,
    /// The guest base's tar.zst (`mcbench-sandbox`'s `guest/build.sh`):
    /// only where a guest runs the boxes.
    #[cfg(not(target_os = "linux"))]
    pub guest: Pinned,
}

/// The shell step's sentence, for the person in setup, when this build has
/// no model's shell.
pub const NOT_INCLUDED: &str =
    "This build of the app does not include the model's shell, so bash is not available";

/// The release's pinned assets of this checkout (`native/sandbox-assets.lock`,
/// written by `native/packaging/sandbox-assets.sh`).
pub const LOCK: &str = include_str!("../../../sandbox-assets.lock");

/// The lock's `url` while the assets are not published.
pub const UNPUBLISHED: &str = "not-published-yet";

/// The address the build named for the assets (`MCBENCH_SANDBOX_ASSETS_URL`
/// at build time), which replaces the lock's `url`.
pub const BUILD_ADDRESS: Option<&str> = option_env!("MCBENCH_SANDBOX_ASSETS_URL");

/// One pinned asset: a lock line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Asset {
    /// `rootfs` or `guest-base`.
    pub kind: String,
    /// The architecture it is for (`x86_64`, `aarch64`).
    pub arch: String,
    /// Its file name at the address.
    pub name: String,
    /// Its sha256, hex.
    pub sha256: String,
    /// Its size in bytes.
    pub size: u64,
}

/// What the lock records.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Assets {
    /// Every asset, in the lock's order.
    pub list: Vec<Asset>,
    /// The folder that holds the files.
    pub url: String,
}

/// The assets one platform needs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Wanted {
    /// The root filesystem of the guest's architecture.
    pub rootfs: Asset,
    /// The guest base, where a guest runs the boxes.
    #[cfg(not(target_os = "linux"))]
    pub guest: Asset,
}

impl Assets {
    /// Reads a lock's text.
    pub fn parse(text: &str) -> Result<Assets, String> {
        let mut list = Vec::new();
        let mut url = None;
        for line in text
            .lines()
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
        {
            match line.split('\t').collect::<Vec<_>>()[..] {
                ["url", address] => url = Some(address.to_string()),
                [kind, arch, name, sha256, size] => list.push(Asset {
                    kind: kind.into(),
                    arch: arch.into(),
                    name: name.into(),
                    sha256: sha256.into(),
                    size: size.parse().map_err(|_| {
                        format!("The sandbox assets' lock line {line:?} has no size")
                    })?,
                }),
                _ => return Err(format!("The sandbox assets' lock has a line {line:?}")),
            }
        }
        Ok(Assets {
            list,
            url: url.ok_or("The sandbox assets' lock has no url line")?,
        })
    }

    fn one(&self, kind: &str, arch: &str) -> Result<Asset, String> {
        self.list
            .iter()
            .find(|a| a.kind == kind && a.arch == arch)
            .cloned()
            .ok_or_else(|| format!("The sandbox assets' lock has no {kind} for {arch}"))
    }

    /// The assets a guest of architecture `arch` (this machine's: the boxes
    /// run on it, or in a guest of its architecture) needs.
    pub fn wanted(&self, arch: &str) -> Result<Wanted, String> {
        Ok(Wanted {
            rootfs: self.one("rootfs", arch)?,
            #[cfg(not(target_os = "linux"))]
            guest: self.one("guest-base", arch)?,
        })
    }

    /// Where the files are fetched from: `build` (the build's address) when
    /// given, else the lock's `url`; `None` while that is [`UNPUBLISHED`].
    pub fn address(&self, build: Option<&str>) -> Option<String> {
        match build {
            Some(address) => Some(address.to_string()),
            None if self.url == UNPUBLISHED => None,
            None => Some(self.url.clone()),
        }
    }
}

/// The model's shell's files, on disk, for the data folder `folder`, or the
/// shell step's error (the module's doc): in a shipped build the pinned
/// assets fetched with `client`, their progress told to `steps`' `shell`
/// step; in any other build the files the environment names.
pub async fn files(
    steps: Arc<Steps>,
    folder: PathBuf,
    client: reqwest::Client,
    cancel: CancellationToken,
) -> Result<ShellFiles, StepError> {
    let build = Build {
        shipped: cfg!(feature = "shipped-data"),
        lock: LOCK,
        address: BUILD_ADDRESS,
        arch: std::env::consts::ARCH,
    };
    files_of(
        &build,
        &|name| std::env::var(name).ok(),
        steps,
        folder,
        client,
        cancel,
    )
    .await
}

/// What [`files`] knows of the build it is in.
#[derive(Clone, Copy, Debug)]
pub struct Build<'a> {
    /// Whether it is a shipped build (feature `shipped-data`).
    pub shipped: bool,
    /// Its lock's text ([`LOCK`]).
    pub lock: &'a str,
    /// The address it was built with ([`BUILD_ADDRESS`]).
    pub address: Option<&'a str>,
    /// The guest's architecture (this machine's).
    pub arch: &'a str,
}

/// [`files`] for the build `build`, reading an environment variable with
/// `var` (which a shipped build never calls).
pub async fn files_of(
    build: &Build<'_>,
    var: &(dyn Fn(&str) -> Option<String> + Sync),
    steps: Arc<Steps>,
    folder: PathBuf,
    client: reqwest::Client,
    cancel: CancellationToken,
) -> Result<ShellFiles, StepError> {
    if build.shipped {
        let assets = Assets::parse(build.lock).expect("native/sandbox-assets.lock is a lock");
        let Some(url) = assets.address(build.address) else {
            return Err(StepError::Unavailable(NOT_INCLUDED.into()));
        };
        // A machine whose architecture the release has no assets for has
        // no model's shell either.
        let wanted = assets
            .wanted(build.arch)
            .map_err(|_| StepError::Unavailable(NOT_INCLUDED.into()))?;
        fetch(&wanted, &url, &steps, &folder, &client, &cancel).await
    } else {
        named(|name| var(name).filter(|v| !v.is_empty()))
            .ok_or_else(|| StepError::Unavailable(NOT_INCLUDED.into()))
    }
}

/// The folder of the data folder `folder` the assets are kept in.
pub fn kept_in(folder: &Path) -> PathBuf {
    folder.join("sandbox-assets")
}

/// `wanted`, fetched from `<url>/<file name>` into [`kept_in`]`(folder)`
/// unless kept there already, each checked against its size and sha256
/// before it is kept (`crate::fetch::file`); the bytes received are told
/// to `steps`' `shell` step. A failed fetch keeps nothing of its file and
/// fails with one sentence.
pub async fn fetch(
    wanted: &Wanted,
    url: &str,
    steps: &Steps,
    folder: &Path,
    client: &reqwest::Client,
    cancel: &CancellationToken,
) -> Result<ShellFiles, StepError> {
    let dir = kept_in(folder);
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| StepError::Failed(format!("{}: {e}", dir.display())))?;
    let list = [
        &wanted.rootfs,
        #[cfg(not(target_os = "linux"))]
        &wanted.guest,
    ];
    let total: u64 = list.iter().map(|a| a.size).sum();
    let mut before = 0u64;
    let mut kept = Vec::new();
    for asset in list {
        let file = dir.join(format!("{}-{}", asset.sha256, asset.name));
        let there = tokio::fs::try_exists(&file)
            .await
            .map_err(|e| StepError::Failed(format!("{}: {e}", file.display())))?;
        if !there {
            let address = format!("{}/{}", url.trim_end_matches('/'), asset.name);
            let progress = |b: crate::steps::Bytes| {
                let _ = steps.progress(
                    "shell",
                    crate::steps::Bytes {
                        done: before + b.done,
                        total: Some(total),
                    },
                );
            };
            crate::fetch::file(
                client,
                &address,
                &file,
                &asset.sha256,
                asset.size,
                &progress,
                cancel,
            )
            .await
            .map_err(|cause| {
                StepError::Failed(format!(
                    "The download of the model's shell from {address} failed: {cause}; try again"
                ))
            })?;
        }
        before += asset.size;
        kept.push(Pinned {
            file,
            sha256: asset.sha256.clone(),
        });
    }
    let mut kept = kept.into_iter();
    Ok(ShellFiles {
        rootfs: kept.next().expect("the root filesystem"),
        #[cfg(not(target_os = "linux"))]
        guest: kept.next().expect("the guest base"),
    })
}

/// The files `var` names, when every one is named.
fn named(var: impl Fn(&str) -> Option<String>) -> Option<ShellFiles> {
    let pinned = |file: &str, sha256: &str| {
        Some(Pinned {
            file: PathBuf::from(var(file)?),
            sha256: var(sha256)?,
        })
    };
    Some(ShellFiles {
        rootfs: pinned("MCBENCH_SANDBOX_ROOTFS", "MCBENCH_SANDBOX_ROOTFS_SHA256")?,
        #[cfg(not(target_os = "linux"))]
        guest: pinned("MCBENCH_SANDBOX_GUEST", "MCBENCH_SANDBOX_GUEST_SHA256")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn of(vars: &[(&str, &str)]) -> Option<ShellFiles> {
        let vars: Vec<(String, String)> = vars
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        named(move |name| vars.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone()))
    }

    #[test]
    fn the_files_are_the_ones_the_variables_name_and_every_one_is_needed() {
        let all = [
            ("MCBENCH_SANDBOX_ROOTFS", "/r.tar.zst"),
            ("MCBENCH_SANDBOX_ROOTFS_SHA256", "aa"),
            ("MCBENCH_SANDBOX_GUEST", "/g.tar.zst"),
            ("MCBENCH_SANDBOX_GUEST_SHA256", "bb"),
        ];
        assert_eq!(
            of(&all),
            Some(ShellFiles {
                rootfs: Pinned {
                    file: "/r.tar.zst".into(),
                    sha256: "aa".into()
                },
                #[cfg(not(target_os = "linux"))]
                guest: Pinned {
                    file: "/g.tar.zst".into(),
                    sha256: "bb".into()
                },
            })
        );
        assert_eq!(of(&all[1..]), None);
        assert_eq!(of(&[all[0]]), None);
        // The guest base is needed only where a guest runs the boxes.
        assert_eq!(of(&all[..2]).is_some(), cfg!(target_os = "linux"));
    }

    #[test]
    fn the_checkouts_lock_reads_and_its_architectures_lines_are_taken() {
        let assets = Assets::parse(LOCK).unwrap();
        assert!(LOCK.lines().any(|l| l == "url\tnot-published-yet") == (assets.url == UNPUBLISHED));
        for arch in ["x86_64", "aarch64"] {
            let wanted = assets.wanted(arch).unwrap();
            assert_eq!(wanted.rootfs.kind, "rootfs");
            assert_eq!(wanted.rootfs.arch, arch);
            assert_eq!(wanted.rootfs.name, format!("rootfs-{arch}.tar.zst"));
            #[cfg(not(target_os = "linux"))]
            {
                assert_eq!(wanted.guest.kind, "guest-base");
                assert_eq!(wanted.guest.name, format!("guest-base-{arch}.tar.zst"));
            }
        }
        let x86 = assets.wanted("x86_64").unwrap().rootfs;
        assert_eq!(
            x86.sha256,
            "786d552ac02865a6542f32605d6bfb47f2c5148b0f8ce78b27b7c4aa75fd955f"
        );
        assert_eq!(x86.size, 82_756_487);
        assert!(assets.wanted("riscv64").is_err());
    }

    #[test]
    fn the_address_is_the_builds_then_the_locks_and_none_while_unpublished() {
        let mut assets = Assets::parse(LOCK).unwrap();
        assets.url = UNPUBLISHED.into();
        assert_eq!(assets.address(None), None);
        assert_eq!(
            assets.address(Some("http://10.0.2.2:3191")),
            Some("http://10.0.2.2:3191".into())
        );
        assets.url = "https://example.invalid/release".into();
        assert_eq!(
            assets.address(None),
            Some("https://example.invalid/release".into())
        );
        assert_eq!(
            assets.address(Some("http://127.0.0.1:3190")),
            Some("http://127.0.0.1:3190".into())
        );
    }
}
