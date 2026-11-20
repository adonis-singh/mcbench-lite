//! The texture fetch: the client-jar manifest's entries from inside
//! Mojang's client jar by HTTP range requests (Mojang's CDN answers a
//! single range with 206; a multi-range request gets the whole jar), the
//! entries coalesced into ranges across gaps of up to [`GAP`] bytes (12
//! requests for the 1.21.10 manifest), at most [`CONNECTIONS`] at once over
//! one HTTP/2 connection. Every entry is checked against the manifest (its
//! stored size, then its size, CRC32 and sha256) and nothing that fails is
//! kept. A range whose bytes all checked is written to the fetch's folder
//! (it appears whole, by a rename), so a fetch that stopped resumes with
//! the ranges it lacks.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use tokio_util::sync::CancellationToken;

use crate::jar::{Entry, Manifest};
use crate::steps::Bytes;

/// The largest gap between two entries one range spans (measured on the
/// 1.21.10 manifest: 12 ranges and 1,042,364 bytes; 64 KiB gives 5 ranges
/// and 1,219,441 bytes and no faster fetch).
pub const GAP: u64 = 16 * 1024;
/// Ranges fetched at once.
pub const CONNECTIONS: usize = 16;

/// One range request: bytes `[start, end)` of the jar, holding `entries`.
#[derive(Clone, Debug)]
pub struct Range {
    /// Its first byte.
    pub start: u64,
    /// One past its last byte.
    pub end: u64,
    /// The entries in it, by offset.
    pub entries: Vec<Entry>,
}

/// The manifest's entries in ranges across gaps of at most `gap` bytes.
pub fn ranges(entries: &[Entry], gap: u64) -> Vec<Range> {
    let mut list = entries.to_vec();
    list.sort_by_key(|e| e.offset);
    let mut out: Vec<Range> = Vec::new();
    for e in list {
        let end = e.data + e.stored;
        match out.last_mut() {
            Some(r) if e.offset <= r.end + gap => {
                r.end = r.end.max(end);
                r.entries.push(e);
            }
            _ => out.push(Range {
                start: e.offset,
                end,
                entries: vec![e],
            }),
        }
    }
    out
}

/// The client every download shares: HTTP/2 or HTTP/1.1 by ALPN, no proxy,
/// Mozilla's roots with ring's crypto.
pub fn client() -> reqwest::Client {
    let roots = rustls::RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    let mut tls = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .expect("ring supports TLS 1.2 and 1.3")
    .with_root_certificates(roots)
    .with_no_client_auth();
    tls.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    reqwest::Client::builder()
        .tls_backend_preconfigured(tls)
        .no_proxy()
        .connect_timeout(Duration::from_secs(30))
        .build()
        .expect("the HTTP client's settings are valid")
}

/// How a fetch is made.
#[derive(Clone, Debug)]
pub struct Options {
    /// The jar's address (the manifest's `url`; a local server in tests).
    pub url: String,
    /// The largest gap a range spans ([`GAP`]).
    pub gap: u64,
    /// Ranges at once ([`CONNECTIONS`]).
    pub connections: usize,
    /// For measurements only: the bytes per second the client reads at
    /// most (the plan's 20 Mbit/s row).
    pub rate: Option<f64>,
}

impl Options {
    /// The app's fetch of `manifest`'s jar.
    pub fn of(manifest: &Manifest) -> Options {
        Options {
            url: manifest.url.clone(),
            gap: GAP,
            connections: CONNECTIONS,
            rate: None,
        }
    }
}

/// What a fetch got: every entry's bytes and stored bytes, by name.
pub struct Fetched {
    /// Each entry's bytes, checked.
    pub bytes: HashMap<String, Vec<u8>>,
    /// Each entry's stored bytes (the client-entries zip holds them).
    pub stored: HashMap<String, Vec<u8>>,
    /// The bytes received over the network (0 for a range the folder held).
    pub received: u64,
    /// The requests made.
    pub requests: usize,
}

/// A range's stored entries checked against the manifest.
/// A range's entries: each one's name, bytes and stored bytes.
type Checked = Vec<(String, Vec<u8>, Vec<u8>)>;

fn check(range: &Range, body: &[u8]) -> Result<Checked, String> {
    if body.len() as u64 != range.end - range.start {
        return Err(format!(
            "The range {}-{} of the client jar came with {} bytes",
            range.start,
            range.end,
            body.len()
        ));
    }
    range
        .entries
        .iter()
        .map(|e| {
            let at = (e.data - range.start) as usize;
            let stored = body[at..at + e.stored as usize].to_vec();
            let bytes = e.unpack(&stored)?;
            Ok((e.name.clone(), bytes, stored))
        })
        .collect()
}

/// Fetches every entry of `manifest`, keeping checked ranges in `folder`,
/// telling `progress` the bytes received of the ranges' total. On the first
/// failure it starts no more ranges, stops the requests under way and
/// returns once every range it started has ended, so no range is still
/// being written into `folder` when it returns.
pub async fn fetch(
    client: &reqwest::Client,
    manifest: &Manifest,
    options: &Options,
    folder: &Path,
    progress: &(dyn Fn(Bytes) + Send + Sync),
    cancel: &CancellationToken,
) -> Result<Fetched, String> {
    tokio::fs::create_dir_all(folder)
        .await
        .map_err(|e| format!("{}: {e}", folder.display()))?;
    let all = ranges(&manifest.entries, options.gap);
    let total: u64 = all.iter().map(|r| r.end - r.start).sum();
    let received = Arc::new(AtomicU64::new(0));
    let started = Instant::now();
    let mut fetched = Fetched {
        bytes: HashMap::new(),
        stored: HashMap::new(),
        received: 0,
        requests: 0,
    };
    let mut missing = Vec::new();
    for range in all {
        let file = folder.join(format!("{}-{}", range.start, range.end));
        match tokio::fs::read(&file).await {
            Ok(body) => match check(&range, &body) {
                Ok(entries) => {
                    received.fetch_add(range.end - range.start, Ordering::Relaxed);
                    for (name, bytes, stored) in entries {
                        fetched.bytes.insert(name.clone(), bytes);
                        fetched.stored.insert(name, stored);
                    }
                }
                Err(_) => {
                    tokio::fs::remove_file(&file)
                        .await
                        .map_err(|e| format!("{}: {e}", file.display()))?;
                    missing.push(range);
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => missing.push(range),
            Err(e) => return Err(format!("{}: {e}", file.display())),
        }
    }
    progress(Bytes {
        done: received.load(Ordering::Relaxed),
        total: Some(total),
    });
    let mut network = 0u64;
    fetched.requests = missing.len();
    let mut queue = missing.into_iter();
    let mut running = tokio::task::JoinSet::new();
    let stop = cancel.child_token();
    let mut failed = None;
    loop {
        while failed.is_none() && running.len() < options.connections.max(1) {
            let Some(range) = queue.next() else { break };
            let (client, url, received, folder, rate) = (
                client.clone(),
                options.url.clone(),
                received.clone(),
                folder.to_path_buf(),
                options.rate,
            );
            let token = stop.clone();
            running.spawn(async move {
                let body = tokio::select! {
                    body = get_range(&client, &url, &range, &received, started, rate) => body?,
                    _ = token.cancelled() => return Err("The texture download was stopped".to_string()),
                };
                let entries = check(&range, &body)?;
                let file = folder.join(format!("{}-{}", range.start, range.end));
                let part = folder.join(format!("{}-{}.part", range.start, range.end));
                tokio::fs::write(&part, &body).await.map_err(|e| format!("{}: {e}", part.display()))?;
                tokio::fs::rename(&part, &file).await.map_err(|e| format!("{}: {e}", file.display()))?;
                Ok::<_, String>((body.len() as u64, entries))
            });
        }
        let Some(done) = running.join_next().await else {
            break;
        };
        let (bytes, entries) = match done
            .map_err(|e| format!("A texture download task failed: {e}"))
            .and_then(|outcome| outcome)
        {
            Ok(done) => done,
            Err(error) => {
                if failed.is_none() {
                    failed = Some(error);
                    stop.cancel();
                }
                continue;
            }
        };
        network += bytes;
        for (name, bytes, stored) in entries {
            fetched.bytes.insert(name.clone(), bytes);
            fetched.stored.insert(name, stored);
        }
        progress(Bytes {
            done: received.load(Ordering::Relaxed),
            total: Some(total),
        });
    }
    if let Some(error) = failed {
        return Err(error);
    }
    fetched.received = network;
    Ok(fetched)
}

/// Fetches the whole of `url` (redirects followed, to any host; no range
/// request, so a fetch that failed starts again from its first byte) into
/// `target`, whose bytes must be exactly `size` long with sha256 `sha256`,
/// telling `progress` the bytes received. The bytes are written to
/// `<target>.<this process's id>.part`, which is renamed to `target` only
/// once both checks passed (so processes fetching the same file at once,
/// parallel benchmark turns on a fresh folder, never write one part file
/// together: each checks its own and the rename is atomic); on any failure
/// or on `cancel` the part file is removed and the answer is the cause.
pub async fn file(
    client: &reqwest::Client,
    url: &str,
    target: &Path,
    sha256: &str,
    size: u64,
    progress: &(dyn Fn(Bytes) + Send + Sync),
    cancel: &CancellationToken,
) -> Result<(), String> {
    let part =
        std::path::PathBuf::from(format!("{}.{}.part", target.display(), std::process::id()));
    let written = tokio::select! {
        written = file_part(client, url, &part, sha256, size, progress) => written,
        _ = cancel.cancelled() => Err("it was stopped".to_string()),
    };
    match written {
        Ok(()) => tokio::fs::rename(&part, target)
            .await
            .map_err(|e| format!("{}: {e}", target.display())),
        Err(cause) => {
            match tokio::fs::remove_file(&part).await {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(format!("{cause}, and {}: {e}", part.display())),
            }
            Err(cause)
        }
    }
}

/// [`file`]'s bytes into `part`, checked.
async fn file_part(
    client: &reqwest::Client,
    url: &str,
    part: &Path,
    sha256: &str,
    size: u64,
    progress: &(dyn Fn(Bytes) + Send + Sync),
) -> Result<(), String> {
    use sha2::Digest;
    use tokio::io::AsyncWriteExt;
    let mut response = client
        .get(url)
        .send()
        .await
        .map_err(|e| crate::pins::cause(&e))?;
    let status = response.status().as_u16();
    if !(200..300).contains(&status) {
        return Err(format!("HTTP {status}"));
    }
    let mut out = tokio::fs::File::create(part)
        .await
        .map_err(|e| format!("{}: {e}", part.display()))?;
    let mut hash = sha2::Sha256::new();
    let mut done = 0u64;
    progress(Bytes {
        done,
        total: Some(size),
    });
    while let Some(chunk) = response.chunk().await.map_err(|e| crate::pins::cause(&e))? {
        done += chunk.len() as u64;
        if done > size {
            return Err(format!("it is longer than {size} bytes"));
        }
        hash.update(&chunk);
        out.write_all(&chunk)
            .await
            .map_err(|e| format!("{}: {e}", part.display()))?;
        progress(Bytes {
            done,
            total: Some(size),
        });
    }
    if done < size {
        return Err(format!("it ended after {done} of {size} bytes"));
    }
    out.flush()
        .await
        .map_err(|e| format!("{}: {e}", part.display()))?;
    let got = crate::jar::hex(&hash.finalize());
    if got != sha256 {
        return Err(format!("its sha256 is {got}, not the pinned {sha256}"));
    }
    Ok(())
}

/// One range's body: 206 and exactly its bytes.
async fn get_range(
    client: &reqwest::Client,
    url: &str,
    range: &Range,
    received: &AtomicU64,
    started: Instant,
    rate: Option<f64>,
) -> Result<Vec<u8>, String> {
    let failed =
        |why: String| format!("The download of Minecraft's textures from {url} failed: {why}");
    let mut response = client
        .get(url)
        .header("range", format!("bytes={}-{}", range.start, range.end - 1))
        .send()
        .await
        .map_err(|e| failed(e.to_string()))?;
    if response.status().as_u16() != 206 {
        return Err(failed(format!(
            "HTTP {} for the range {}-{}",
            response.status().as_u16(),
            range.start,
            range.end - 1
        )));
    }
    let mut body = Vec::with_capacity((range.end - range.start) as usize);
    while let Some(chunk) = response.chunk().await.map_err(|e| failed(e.to_string()))? {
        let total = received.fetch_add(chunk.len() as u64, Ordering::Relaxed) + chunk.len() as u64;
        if let Some(rate) = rate {
            let due = Duration::from_secs_f64(total as f64 / rate);
            let now = started.elapsed();
            if due > now {
                tokio::time::sleep(due - now).await;
            }
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, offset: u64, stored: u64) -> Entry {
        Entry {
            name: name.into(),
            offset,
            data: offset + 30 + name.len() as u64,
            stored,
            size: stored,
            method: 0,
            crc32: 0,
            sha256: String::new(),
        }
    }

    #[test]
    fn entries_are_coalesced_across_small_gaps_only() {
        let list = [
            entry("a", 0, 100),
            entry("b", 140, 10),
            entry("c", 100_000, 5),
        ];
        let r = ranges(&list, 16 * 1024);
        assert_eq!(r.len(), 2);
        assert_eq!((r[0].start, r[0].end), (0, 140 + 31 + 10));
        assert_eq!(r[0].entries.len(), 2);
        assert_eq!((r[1].start, r[1].end), (100_000, 100_000 + 31 + 5));
    }
}
