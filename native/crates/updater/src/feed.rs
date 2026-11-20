//! What `mcbench-feed` does (feature `feed`): the updater's key pair, the
//! deterministic release archives, the assets' signatures and
//! `latest.json`, written with the reader's own types, signed comment and
//! asset names, so the writer and the reader cannot drift.
//!
//! | Command | What it does |
//! |---|---|
//! | `keygen <folder>` | writes `minisign.pub` and `minisign.key` (no password: the key is a CI secret or an offline file) into `<folder>`; refuses when either exists |
//! | `pack [--as <name>] <folder> <out.tar.gz>` | a deterministic archive of `<folder>`'s contents, under one root folder `<name>` with `--as` |
//! | `sign [--chain <file>] <release folder> <version> <target>...` | signs each named target's asset in the folder with the key file `MCBENCH_SIGNING_KEY_FILE` names and writes `latest.json` there; `--chain` copies the `keys` array of an existing `latest.json` through |
//! | `sign-key <next minisign.pub>` | prints the `keys` entry in which the current key (`MCBENCH_SIGNING_KEY_FILE`) signs the next public key, for a rotation |
//!
//! Nothing here prints or writes a secret key except `keygen`'s
//! `minisign.key`.

use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Read, Write as _};
use std::path::{Path, PathBuf};

use minisign::{PublicKey, PublicKeyBox, SecretKey, SecretKeyBox};
use sha2::{Digest, Sha256};

use crate::Target;
use crate::latest::{AssetEntry, KeyEntry, Latest, hex, parse_version, signed_comment};

/// The trusted comment of a key-chain entry's signature (LATEST.md; the
/// updater does not read it).
pub const KEY_COMMENT: &str = "mcbench-lite update key";

/// The environment variable naming the secret key file `sign` and
/// `sign-key` use.
pub const SIGNING_KEY_VAR: &str = "MCBENCH_SIGNING_KEY_FILE";

/// The public key file `keygen` writes.
pub const PUBLIC_FILE: &str = "minisign.pub";

/// The secret key file `keygen` writes.
pub const SECRET_FILE: &str = "minisign.key";

/// Writes a new key pair into `folder` (made if missing): `minisign.pub`
/// and `minisign.key`, the secret key unencrypted and, on Unix, readable by
/// its owner only. Refuses when either file exists. Returns the public
/// key's base64 line.
pub fn keygen(folder: &Path) -> Result<String, String> {
    fs::create_dir_all(folder).map_err(|e| format!("{} cannot be made: {e}", folder.display()))?;
    let public = folder.join(PUBLIC_FILE);
    let secret = folder.join(SECRET_FILE);
    for path in [&public, &secret] {
        if fs::symlink_metadata(path).is_ok() {
            return Err(format!(
                "{} exists; keygen never overwrites a key.",
                path.display()
            ));
        }
    }
    let pair = minisign::KeyPair::generate_unencrypted_keypair()
        .map_err(|e| format!("A key pair cannot be made: {e}"))?;
    let public_text = pair
        .pk
        .to_box()
        .map_err(|e| format!("The public key cannot be written: {e}"))?
        .into_string();
    let secret_text = pair
        .sk
        .to_box(Some("mcbench-lite update secret key, unencrypted"))
        .map_err(|e| format!("The secret key cannot be written: {e}"))?
        .into_string();
    write_new(&secret, secret_text.as_bytes(), 0o600)?;
    write_new(&public, public_text.as_bytes(), 0o644)?;
    Ok(pair.pk.to_base64())
}

fn write_new(path: &Path, bytes: &[u8], mode: u32) -> Result<(), String> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(mode);
    }
    #[cfg(not(unix))]
    let _ = mode;
    options
        .open(path)
        .and_then(|mut f| f.write_all(bytes).and_then(|()| f.sync_all()))
        .map_err(|e| format!("{} cannot be written: {e}", path.display()))
}

/// What one archive entry is.
enum Kind {
    Folder,
    File { executable: bool },
    Link(PathBuf),
}

/// Whether the file with `meta` at `path` is an executable: any execute bit
/// on Unix; the `.exe` extension on Windows, which has no execute bit.
fn executable(path: &Path, meta: &fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = path;
        meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        let _ = meta;
        path.extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("exe"))
    }
}

/// Every entry under `folder`, by its path inside the archive (`/`
/// between the parts), sorted.
fn entries(folder: &Path) -> Result<BTreeMap<String, (PathBuf, Kind)>, String> {
    let mut out = BTreeMap::new();
    let mut stack = vec![folder.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let read =
            fs::read_dir(&dir).map_err(|e| format!("{} cannot be read: {e}", dir.display()))?;
        for entry in read {
            let path = entry
                .map_err(|e| format!("{} cannot be read: {e}", dir.display()))?
                .path();
            let rel = path
                .strip_prefix(folder)
                .expect("a walked path is inside its folder");
            let parts: Option<Vec<&str>> =
                rel.components().map(|c| c.as_os_str().to_str()).collect();
            let name = parts
                .ok_or_else(|| format!("{} is not a name in UTF-8.", path.display()))?
                .join("/");
            let meta = fs::symlink_metadata(&path)
                .map_err(|e| format!("{} cannot be read: {e}", path.display()))?;
            let kind = if meta.file_type().is_symlink() {
                Kind::Link(
                    fs::read_link(&path)
                        .map_err(|e| format!("{} cannot be read: {e}", path.display()))?,
                )
            } else if meta.is_dir() {
                stack.push(path.clone());
                Kind::Folder
            } else {
                Kind::File {
                    executable: executable(&path, &meta),
                }
            };
            out.insert(name, (path, kind));
        }
    }
    Ok(out)
}

/// A tar header with every field that varies between machines fixed:
/// time 0, owner and group 0 with no names.
fn header(kind: tar::EntryType, mode: u32, size: u64) -> tar::Header {
    let mut header = tar::Header::new_gnu();
    header.set_entry_type(kind);
    header.set_mode(mode);
    header.set_size(size);
    header.set_mtime(0);
    header.set_uid(0);
    header.set_gid(0);
    header
}

/// Writes `out`, a `.tar.gz` of `folder`'s contents (under one root folder
/// `root` when given) that is the same bytes for the same files on any
/// machine: entries sorted by path, times 0, owner 0, mode 0755 for
/// folders and executables and 0644 for everything else, symlinks kept as
/// symlinks, and a gzip header with time 0 and no name.
pub fn pack(folder: &Path, root: Option<&str>, out: &Path) -> Result<(), String> {
    let list = entries(folder)?;
    let file =
        fs::File::create(out).map_err(|e| format!("{} cannot be made: {e}", out.display()))?;
    let gz = flate2::GzBuilder::new()
        .mtime(0)
        .operating_system(255)
        .write(file, flate2::Compression::best());
    let mut tar = tar::Builder::new(gz);
    let written = |e: io::Error| format!("{} cannot be written: {e}", out.display());
    let inside = |name: &str| match root {
        Some(root) => format!("{root}/{name}"),
        None => name.to_string(),
    };
    if let Some(root) = root {
        let mut h = header(tar::EntryType::Directory, 0o755, 0);
        tar.append_data(&mut h, format!("{root}/"), io::empty())
            .map_err(written)?;
    }
    for (name, (path, kind)) in &list {
        let name = inside(name);
        match kind {
            Kind::Folder => {
                let mut h = header(tar::EntryType::Directory, 0o755, 0);
                tar.append_data(&mut h, format!("{name}/"), io::empty())
                    .map_err(written)?;
            }
            Kind::File { executable } => {
                let data = fs::File::open(path)
                    .map_err(|e| format!("{} cannot be read: {e}", path.display()))?;
                let size = data
                    .metadata()
                    .map_err(|e| format!("{} cannot be read: {e}", path.display()))?
                    .len();
                let mode = if *executable { 0o755 } else { 0o644 };
                let mut h = header(tar::EntryType::Regular, mode, size);
                tar.append_data(&mut h, &name, data).map_err(written)?;
            }
            Kind::Link(target) => {
                let mut h = header(tar::EntryType::Symlink, 0o644, 0);
                tar.append_link(&mut h, &name, target).map_err(written)?;
            }
        }
    }
    tar.into_inner()
        .and_then(|gz| gz.finish())
        .and_then(|f| f.sync_all())
        .map_err(written)
}

/// The secret key in the file `path` (`MCBENCH_SIGNING_KEY_FILE`), and its
/// public key. The sentences never hold the file's text.
pub fn signing_key(path: &Path) -> Result<(SecretKey, PublicKey), String> {
    let text = fs::read_to_string(path).map_err(|e| {
        format!(
            "{SIGNING_KEY_VAR} names {}, which cannot be read: {e}",
            path.display()
        )
    })?;
    let secret = SecretKeyBox::from_string(&text)
        .and_then(SecretKeyBox::into_unencrypted_secret_key)
        .map_err(|e| {
            format!(
                "{SIGNING_KEY_VAR} names {}, which is not an unencrypted minisign secret key: {e}",
                path.display()
            )
        })?;
    let public = PublicKey::from_secret_key(&secret).map_err(|e| {
        format!(
            "{SIGNING_KEY_VAR} names {}, whose public key cannot be derived: {e}",
            path.display()
        )
    })?;
    Ok((secret, public))
}

fn sign_reader(
    secret: &SecretKey,
    public: &PublicKey,
    data: impl Read,
    comment: &str,
) -> Result<String, String> {
    minisign::sign(Some(public), secret, data, Some(comment), None)
        .map(|s| s.into_string())
        .map_err(|e| format!("The signature cannot be made: {e}"))
}

/// Signs the asset of each of `targets` in `release` (its fixed name,
/// [`Target::asset`]) with `secret`, prehashed, with the trusted comment
/// LATEST.md gives, and writes `release/latest.json` for `version`. The
/// `keys` array is `chain`'s (an earlier `latest.json`), or empty; a chain
/// whose last key is not this key's public key is refused, since every app
/// would refuse the feed. Fails when an asset is missing.
pub fn sign(
    release: &Path,
    version: &str,
    targets: &[Target],
    secret: &SecretKey,
    public: &PublicKey,
    chain: Option<&Path>,
) -> Result<PathBuf, String> {
    parse_version(version, "The release's")?;
    if targets.is_empty() {
        return Err("sign names no target.".to_string());
    }
    let keys = match chain {
        None => Vec::new(),
        Some(path) => {
            let bytes =
                fs::read(path).map_err(|e| format!("{} cannot be read: {e}", path.display()))?;
            let latest: Latest = serde_json::from_slice(&bytes)
                .map_err(|e| format!("{} is not a latest.json (LATEST.md): {e}", path.display()))?;
            let own = public.to_base64();
            match latest.keys.last() {
                Some(last) if last.public_key != own => {
                    return Err(format!(
                        "The chain in {} ends at the key {}, and the signing key is {own}: apps would refuse this feed.",
                        path.display(),
                        last.public_key
                    ));
                }
                _ => latest.keys,
            }
        }
    };
    let mut assets = BTreeMap::new();
    for &target in targets {
        let name = target.asset();
        let path = release.join(name);
        let open = || {
            fs::File::open(&path).map_err(|e| {
                format!(
                    "The asset for {} is missing: {} cannot be read ({e}).",
                    target.name(),
                    path.display()
                )
            })
        };
        let (size, sha256) =
            sha256_of(open()?).map_err(|e| format!("{} cannot be read: {e}", path.display()))?;
        let comment = signed_comment(version, target.name(), name, size, &sha256);
        let signature = sign_reader(secret, public, open()?, &comment)?;
        assets.insert(
            target.name().to_string(),
            AssetEntry {
                name: name.to_string(),
                size,
                sha256,
                signature,
            },
        );
    }
    let latest = Latest {
        version: version.to_string(),
        keys,
        assets,
    };
    let out = release.join("latest.json");
    let mut text = serde_json::to_string_pretty(&latest).expect("latest.json serializes");
    text.push('\n');
    fs::write(&out, text).map_err(|e| format!("{} cannot be written: {e}", out.display()))?;
    Ok(out)
}

/// The size and sha256 of what `data` reads.
fn sha256_of(mut data: impl Read) -> io::Result<(u64, String)> {
    let mut sha256 = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    let mut size = 0u64;
    loop {
        let n = data.read(&mut buf)?;
        if n == 0 {
            return Ok((size, hex(&sha256.finalize())));
        }
        size += n as u64;
        sha256.update(&buf[..n]);
    }
}

/// The `keys` entry (JSON) in which `secret` signs the next public key in
/// the file `next` (a `minisign.pub`), for a rotation.
pub fn sign_key(next: &Path, secret: &SecretKey, public: &PublicKey) -> Result<String, String> {
    let text =
        fs::read_to_string(next).map_err(|e| format!("{} cannot be read: {e}", next.display()))?;
    let next_key = PublicKeyBox::from_string(&text)
        .and_then(PublicKeyBox::into_public_key)
        .map_err(|e| format!("{} is not a minisign public key: {e}", next.display()))?
        .to_base64();
    let signature = sign_reader(secret, public, next_key.as_bytes(), KEY_COMMENT)?;
    Ok(serde_json::to_string_pretty(&KeyEntry {
        public_key: next_key,
        signature,
    })
    .expect("a key entry serializes"))
}
