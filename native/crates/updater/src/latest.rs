//! `latest.json` (LATEST.md): its types and the signed comment, which
//! `mcbench-feed` writes with the same code; the key chain, the update
//! check, the verified download, and the same checks made again without the
//! network on a download waiting beside the install.

use std::collections::BTreeMap;
use std::io::{Read, Write as _};
use std::path::{Path, PathBuf};

use minisign_verify::{PublicKey, Signature, StreamVerifier};
use reqwest::Url;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;

use crate::Target;
use crate::swap::beside;

include!(concat!(env!("OUT_DIR"), "/update_feed.rs"));

/// The sentence of a check or download the caller's token stopped.
const CANCELLED: &str = "The update was cancelled.";

/// The longest a check's request and body take in all, and the longest a
/// download goes without receiving a byte (the shared client has only a
/// connect timeout).
pub const STALL: std::time::Duration = std::time::Duration::from_secs(60);

/// The largest size `latest.json` may give an asset (512 MiB), refused
/// before anything is downloaded: the size is bound by the signature, which
/// can only be checked over the downloaded bytes.
pub const MAX_ASSET: u64 = 512 * 1024 * 1024;

/// The compiled-in update key and feed address: `MCBENCH_UPDATE_KEY_FILE`
/// and `MCBENCH_UPDATE_FEED` at build time; `None` for a build without an
/// updater.
pub(crate) fn compiled_in() -> Option<(UpdateKey, &'static str)> {
    UPDATE_FEED.map(|(key, url)| {
        (
            UpdateKey::from_base64(key).expect("build.rs decoded the compiled-in key"),
            url,
        )
    })
}

/// A minisign public key the updater trusts.
#[derive(Clone, Debug)]
pub struct UpdateKey {
    base64: String,
    key: PublicKey,
}

impl UpdateKey {
    /// A key from its base64 line (the second line of `minisign.pub`).
    pub fn from_base64(base64: &str) -> Result<UpdateKey, String> {
        let key = PublicKey::from_base64(base64)
            .map_err(|e| format!("{base64:?} is not a minisign public key: {e}"))?;
        Ok(UpdateKey {
            base64: base64.to_string(),
            key,
        })
    }

    /// Its base64 line.
    pub fn base64(&self) -> &str {
        &self.base64
    }
}

/// `latest.json`.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Latest {
    pub(crate) version: String,
    pub(crate) keys: Vec<KeyEntry>,
    pub(crate) assets: BTreeMap<String, AssetEntry>,
}

/// One entry of `latest.json`'s key chain.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct KeyEntry {
    pub(crate) public_key: String,
    pub(crate) signature: String,
}

/// One entry of `latest.json`'s `assets`.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AssetEntry {
    pub(crate) name: String,
    pub(crate) size: u64,
    pub(crate) sha256: String,
    pub(crate) signature: String,
}

/// A newer release for this install, checked as far as can be before its
/// asset is read.
#[derive(Clone)]
pub struct Release {
    /// Its version (`MAJOR.MINOR.PATCH`).
    pub version: String,
    /// The asset's file name.
    pub name: String,
    /// The asset's size in bytes.
    pub size: u64,
    target: Target,
    url: Url,
    sha256: String,
    signature: Signature,
    key: UpdateKey,
}

impl std::fmt::Debug for Release {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Release({} {} from {})",
            self.version, self.name, self.url
        )
    }
}

impl std::fmt::Debug for Downloaded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Downloaded({} {})", self.version, self.path.display())
    }
}

/// A downloaded asset whose size, sha256 and signature were checked. The
/// file is removed when this is dropped, unless [`Downloaded::keep`] was
/// called.
pub struct Downloaded {
    pub(crate) path: PathBuf,
    pub(crate) target: Target,
    pub(crate) version: String,
}

impl Downloaded {
    /// Where the file is.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The release's version.
    pub fn version(&self) -> &str {
        &self.version
    }

    /// Leaves the file where it is (a download waiting for the next start).
    pub fn keep(self) {
        std::mem::forget(self);
    }
}

impl Drop for Downloaded {
    fn drop(&mut self) {
        // The file is consumed by a successful AppImage swap; anything left
        // is ours.
        let _ = std::fs::remove_file(&self.path);
    }
}

/// The version `text`, as three numbers, or the sentence that it is not
/// `MAJOR.MINOR.PATCH`; `whose` begins the sentence.
pub(crate) fn parse_version(text: &str, whose: &str) -> Result<[u64; 3], String> {
    let parts: Vec<_> = text.split('.').collect();
    let numbers: Option<Vec<u64>> = parts
        .iter()
        .map(|p| {
            (!p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
                .then(|| p.parse().ok())
                .flatten()
        })
        .collect();
    match numbers.as_deref() {
        Some(&[a, b, c]) => Ok([a, b, c]),
        _ => Err(format!(
            "{whose} version {text:?} is not MAJOR.MINOR.PATCH."
        )),
    }
}

async fn cancellable<T>(
    cancel: &CancellationToken,
    work: impl Future<Output = Result<T, reqwest::Error>>,
    what: &str,
) -> Result<T, String> {
    tokio::select! {
        _ = cancel.cancelled() => Err(CANCELLED.to_string()),
        result = work => result.map_err(|e| format!("{what} failed: {e}")),
    }
}

async fn get(
    client: &reqwest::Client,
    url: &Url,
    cancel: &CancellationToken,
) -> Result<reqwest::Response, String> {
    let response = cancellable(
        cancel,
        client.get(url.clone()).send(),
        &format!("Fetching {url}"),
    )
    .await?;
    if response.status() != reqwest::StatusCode::OK {
        return Err(format!("Fetching {url} answered {}.", response.status()));
    }
    Ok(response)
}

/// The signed comment of an asset's signature (LATEST.md).
pub(crate) fn signed_comment(
    version: &str,
    target: &str,
    name: &str,
    size: u64,
    sha256: &str,
) -> String {
    format!("version:{version}\ttarget:{target}\tfile:{name}\tsize:{size}\tsha256:{sha256}")
}

/// A sha256 as 64 lower-case hexadecimal digits.
pub(crate) fn hex(digest: &[u8]) -> String {
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// The key that signs this release: the trusted key, followed along
/// `latest.json`'s key chain from just after its own entry (from the start
/// when it has none), each entry signed by the key before it.
fn follow_keys(trusted: &UpdateKey, keys: &[KeyEntry]) -> Result<UpdateKey, String> {
    let start = keys
        .iter()
        .position(|k| k.public_key == trusted.base64)
        .map_or(0, |i| i + 1);
    let mut current = trusted.clone();
    for entry in &keys[start..] {
        let next = UpdateKey::from_base64(&entry.public_key)
            .map_err(|e| format!("latest.json's key chain holds a key that is not one: {e}"))?;
        let signature = Signature::decode(&entry.signature).map_err(|e| {
            format!(
                "latest.json's signature of the key {} is not a minisign signature: {e}",
                entry.public_key
            )
        })?;
        current
            .key
            .verify(entry.public_key.as_bytes(), &signature, false)
            .map_err(|e| match e {
                minisign_verify::Error::UnexpectedKeyId => format!(
                    "latest.json's key {} is signed by a key this build does not trust (it trusts {}).",
                    entry.public_key, current.base64
                ),
                e => format!(
                    "latest.json's signature of the key {} does not verify against {}: {e}.",
                    entry.public_key, current.base64
                ),
            })?;
        current = next;
    }
    Ok(current)
}

/// Fetches `latest.json` from `latest_url` and checks it against the running
/// version `current` and the trusted `key`: `None` when `current` is the
/// latest, the release when it is newer, and an error sentence for a
/// downgrade, a missing or foreign asset, an untrusted key or an entry that
/// does not match its signed comment. Returns `latest.json`'s bytes too,
/// which a download waiting for the next start is kept with.
pub async fn check(
    client: &reqwest::Client,
    latest_url: &str,
    current: &str,
    target: Target,
    key: &UpdateKey,
    cancel: &CancellationToken,
) -> Result<Option<(Release, Vec<u8>)>, String> {
    let (url, body) = fetch_latest(client, latest_url, cancel).await?;
    let release = release_of(&url, &body, current, target, key)?;
    Ok(release.map(|r| (r, body)))
}

/// Fetches `latest.json`'s bytes from `latest_url`, the request and body
/// within [`STALL`] in all; every error is a failure to fetch, never a
/// refusal of the release.
pub(crate) async fn fetch_latest(
    client: &reqwest::Client,
    latest_url: &str,
    cancel: &CancellationToken,
) -> Result<(Url, Vec<u8>), String> {
    let url = Url::parse(latest_url).map_err(|e| format!("{latest_url} is not a URL: {e}"))?;
    let fetch = async {
        let response = get(client, &url, cancel).await?;
        cancellable(cancel, response.bytes(), &format!("Fetching {url}")).await
    };
    let body = tokio::time::timeout(STALL, fetch).await.map_err(|_| {
        format!(
            "Fetching {url} took longer than {} seconds.",
            STALL.as_secs()
        )
    })??;
    Ok((url, body.to_vec()))
}

/// [`check`]'s checks of the `latest.json` bytes `body` fetched from `url`;
/// every error is a refusal of the release.
pub(crate) fn release_of(
    url: &Url,
    body: &[u8],
    current: &str,
    target: Target,
    key: &UpdateKey,
) -> Result<Option<Release>, String> {
    let latest: Latest = serde_json::from_slice(body)
        .map_err(|e| format!("latest.json at {url} is not in its format (LATEST.md): {e}"))?;

    let offered = parse_version(&latest.version, "latest.json's")?;
    let running = parse_version(current, "This app's")?;
    if offered < running {
        return Err(format!(
            "latest.json offers version {}, older than this app's {current}: a downgrade is refused.",
            latest.version
        ));
    }
    if offered == running {
        return Ok(None);
    }

    let signer = follow_keys(key, &latest.keys)?;
    let entry = latest
        .assets
        .get(target.name())
        .ok_or_else(|| format!("latest.json has no asset for {}.", target.name()))?;
    if entry.size > MAX_ASSET {
        return Err(format!(
            "latest.json gives {} {} bytes, more than the {MAX_ASSET} bytes (512 MiB) a release asset may be.",
            entry.name, entry.size
        ));
    }
    let signature = Signature::decode(&entry.signature).map_err(|e| {
        format!(
            "The signature of {} in latest.json is not a minisign signature: {e}",
            entry.name
        )
    })?;
    signer.key.verify_stream(&signature).map_err(|e| match e {
        minisign_verify::Error::UnexpectedKeyId => format!(
            "{} is signed by a key this build does not trust (it trusts {}).",
            entry.name, signer.base64
        ),
        e => format!("The signature of {} cannot be checked: {e}.", entry.name),
    })?;

    let signed = signature.trusted_comment();
    let fields: Vec<(&str, &str)> = signed
        .split('\t')
        .filter_map(|f| f.split_once(':'))
        .collect();
    let field = |name: &str| fields.iter().find(|(k, _)| *k == name).map(|(_, v)| *v);
    if field("target") != Some(target.name()) {
        return Err(format!(
            "The asset latest.json lists for {} is signed as the asset for {}.",
            target.name(),
            field("target").unwrap_or("no target")
        ));
    }
    let listed = signed_comment(
        &latest.version,
        target.name(),
        &entry.name,
        entry.size,
        &entry.sha256,
    );
    if signed != listed {
        return Err(format!(
            "latest.json's entry for {} does not match its signed comment: latest.json gives {listed:?}, the signature {signed:?}.",
            target.name()
        ));
    }
    if entry.name != target.asset() {
        return Err(format!(
            "latest.json names the asset for {} {:?}, where its file is always {}.",
            target.name(),
            entry.name,
            target.asset()
        ));
    }
    let asset_url = url
        .join(&entry.name)
        .expect("an asset's fixed name joins onto a URL");
    Ok(Some(Release {
        version: latest.version,
        name: entry.name.clone(),
        size: entry.size,
        target,
        url: asset_url,
        sha256: entry.sha256.clone(),
        signature,
        key: signer,
    }))
}

/// Why a check or a download did not give a verified release.
#[derive(Debug)]
pub(crate) enum Problem {
    /// The release itself was refused: `latest.json` or the asset is not
    /// what the key, the signed comment and the version allow. The same
    /// `latest.json` gives the same refusal, so it is remembered.
    Refused(String),
    /// Anything else (no network, a timeout, a server's error, a short
    /// answer, a file that cannot be written); the next check tries again.
    Failed(String),
}

impl Problem {
    pub(crate) fn sentence(self) -> String {
        match self {
            Problem::Refused(s) | Problem::Failed(s) => s,
        }
    }
}

/// The checks of an asset's bytes as they arrive: its size, its sha256 and
/// its minisign signature.
struct Verifier<'a> {
    release: &'a Release,
    signature: StreamVerifier<'a>,
    sha256: Sha256,
    received: u64,
}

impl<'a> Verifier<'a> {
    fn new(release: &'a Release) -> Verifier<'a> {
        Verifier {
            release,
            signature: release
                .key
                .key
                .verify_stream(&release.signature)
                .expect("release_of matched the signature's key"),
            sha256: Sha256::new(),
            received: 0,
        }
    }

    fn update(&mut self, chunk: &[u8]) -> Result<(), Problem> {
        self.received += chunk.len() as u64;
        if self.received > self.release.size {
            return Err(Problem::Refused(format!(
                "The download of {} is larger than the {} bytes latest.json gives: it is not the released file.",
                self.release.name, self.release.size
            )));
        }
        self.sha256.update(chunk);
        self.signature.update(chunk);
        Ok(())
    }

    fn finish(mut self) -> Result<(), Problem> {
        let release = self.release;
        if self.received != release.size {
            // A connection that ended early, not a refusal of the release.
            return Err(Problem::Failed(format!(
                "The download of {} ended at {} of the {} bytes latest.json gives.",
                release.name, self.received, release.size
            )));
        }
        let got = hex(&self.sha256.finalize());
        if got != release.sha256 {
            return Err(Problem::Refused(format!(
                "The download of {} has sha256 {got} where latest.json gives {}: it is not the released file.",
                release.name, release.sha256
            )));
        }
        self.signature.finalize().map_err(|_| {
            Problem::Refused(format!(
                "The signature of {} does not verify against {}: the file or its signed comment was changed after signing.",
                release.name, release.key.base64
            ))
        })
    }
}

/// Where a download for the payload `path` is kept: `.<name>.update`
/// beside it, on the same file system, so the swap is a rename.
pub(crate) fn update_file(path: &Path) -> PathBuf {
    beside(path, ".update")
}

/// Downloads `release`'s asset to `.<name>.update` beside `beside` (the
/// payload the swap replaces), checking its size and sha256 while it streams
/// and its minisign signature at the end. Nothing is replaced here. The
/// file is written with `std::fs` on the calling thread.
pub async fn download(
    client: &reqwest::Client,
    release: &Release,
    beside: &Path,
    cancel: &CancellationToken,
) -> Result<Downloaded, String> {
    download_checked(client, release, beside, cancel)
        .await
        .map_err(Problem::sentence)
}

/// [`download`], telling a refusal of the asset from a failure to fetch it.
pub(crate) async fn download_checked(
    client: &reqwest::Client,
    release: &Release,
    beside: &Path,
    cancel: &CancellationToken,
) -> Result<Downloaded, Problem> {
    let failed = Problem::Failed;
    let downloaded = Downloaded {
        path: update_file(beside),
        target: release.target,
        version: release.version.clone(),
    };
    // std::fs on the updater's own thread: no blocking-pool thread is
    // needed for the file.
    let mut file = std::fs::File::create(&downloaded.path).map_err(|e| {
        failed(format!(
            "{} cannot be created: {e}",
            downloaded.path.display()
        ))
    })?;
    let stalled = || {
        format!(
            "The download of {} received nothing for {} seconds.",
            release.name,
            STALL.as_secs()
        )
    };
    let mut response = tokio::time::timeout(STALL, get(client, &release.url, cancel))
        .await
        .map_err(|_| failed(stalled()))?
        .map_err(failed)?;
    let mut verifier = Verifier::new(release);
    let what = format!("Downloading {}", release.url);
    while let Some(chunk) =
        tokio::time::timeout(STALL, cancellable(cancel, response.chunk(), &what))
            .await
            .map_err(|_| failed(stalled()))?
            .map_err(failed)?
    {
        verifier.update(&chunk)?;
        file.write_all(&chunk).map_err(|e| {
            failed(format!(
                "{} cannot be written: {e}",
                downloaded.path.display()
            ))
        })?;
    }
    verifier.finish()?;
    file.sync_all().map_err(|e| {
        failed(format!(
            "{} cannot be written: {e}",
            downloaded.path.display()
        ))
    })?;
    Ok(downloaded)
}

/// Makes again, without the network, every check of [`check`] and
/// [`download`] on a download waiting at `file`, with the `latest.json`
/// bytes `latest` it was checked against (fetched from `latest_url`): the
/// key chain from `key`, the signed comment, a version newer than
/// `current`, the size, the sha256 and the signature. Blocking (it reads
/// the file).
pub fn verify_waiting(
    latest_url: &str,
    latest: &[u8],
    file: &Path,
    current: &str,
    target: Target,
    key: &UpdateKey,
) -> Result<Downloaded, String> {
    let url = Url::parse(latest_url).map_err(|e| format!("{latest_url} is not a URL: {e}"))?;
    let release = release_of(&url, latest, current, target, key)?.ok_or_else(|| {
        format!("The waiting update is version {current}, which this app already is.")
    })?;
    let mut reader = std::fs::File::open(file)
        .map_err(|e| format!("The waiting update {} cannot be read: {e}", file.display()))?;
    let mut verifier = Verifier::new(&release);
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = reader
            .read(&mut buf)
            .map_err(|e| format!("The waiting update {} cannot be read: {e}", file.display()))?;
        if n == 0 {
            break;
        }
        verifier.update(&buf[..n]).map_err(Problem::sentence)?;
    }
    verifier.finish().map_err(Problem::sentence)?;
    Ok(Downloaded {
        path: file.to_path_buf(),
        target,
        version: release.version.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builds_have_no_updater() {
        assert!(
            UPDATE_FEED.is_none(),
            "tests are built without MCBENCH_UPDATE_KEY_FILE and MCBENCH_UPDATE_FEED"
        );
        assert!(compiled_in().is_none());
    }

    #[test]
    fn versions() {
        assert_eq!(parse_version("1.20.3", "x"), Ok([1, 20, 3]));
        assert!(parse_version("1.2.0.1", "x").is_err());
        assert!(parse_version("1.2", "x").is_err());
        assert!(parse_version("1.2.-3", "x").is_err());
        assert!(parse_version("1.2.3-rc1", "x").is_err());
        assert_eq!(
            parse_version("v1.2.3", "latest.json's").unwrap_err(),
            "latest.json's version \"v1.2.3\" is not MAJOR.MINOR.PATCH."
        );
        assert!(parse_version("1.10.0", "x").unwrap() > parse_version("1.9.9", "x").unwrap());
    }
}
