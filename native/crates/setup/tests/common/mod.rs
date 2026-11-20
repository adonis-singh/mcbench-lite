//! A download server on loopback for the tests (as today's tests serve
//! with `http.createServer`): each path's bytes, single byte ranges
//! answered with 206, a script of failures for the first requests, and the
//! requests it was asked.

#![allow(dead_code)]

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// How the server answers one request.
#[derive(Clone, Debug)]
pub enum Answer {
    /// The bytes of the path (a range of them when asked), in two writes.
    Serve,
    /// The connection is closed at once.
    Drop,
    /// Half of the body, then the connection is closed.
    Short,
    /// This status and no body.
    Status(u16),
    /// Half of the body, then nothing more, the connection held open.
    Stall,
    /// A 302 to `<this address><the path>`.
    Redirect(String),
}

/// The server's state.
#[derive(Default)]
pub struct State {
    /// Each path's bytes.
    pub files: HashMap<String, Vec<u8>>,
    /// The answers to the first requests, in order; then `Serve`.
    pub script: Vec<Answer>,
    /// Each request: its path and its range header.
    pub requests: Vec<(String, Option<String>)>,
}

/// A running server.
pub struct Server {
    /// `http://127.0.0.1:<port>`.
    pub url: String,
    /// Its state.
    pub state: Arc<Mutex<State>>,
}

/// Serves `files` on loopback.
pub async fn serve(files: HashMap<String, Vec<u8>>) -> Server {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let state = Arc::new(Mutex::new(State {
        files,
        ..State::default()
    }));
    let shared = state.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            let state = shared.clone();
            tokio::spawn(async move {
                loop {
                    let mut head = Vec::new();
                    let mut byte = [0u8; 1];
                    while !head.ends_with(b"\r\n\r\n") {
                        match socket.read(&mut byte).await {
                            Ok(1) => head.push(byte[0]),
                            _ => return,
                        }
                    }
                    let text = String::from_utf8_lossy(&head).to_string();
                    let path = text.split(' ').nth(1).unwrap_or("/").to_string();
                    let range = text.lines().find_map(|l| {
                        l.to_ascii_lowercase()
                            .strip_prefix("range: bytes=")
                            .map(str::to_string)
                    });
                    let (answer, bytes) = {
                        let mut s = state.lock().unwrap();
                        let index = s.requests.len();
                        s.requests.push((path.clone(), range.clone()));
                        (
                            s.script.get(index).cloned().unwrap_or(Answer::Serve),
                            s.files.get(&path).cloned(),
                        )
                    };
                    match answer {
                        Answer::Drop => return,
                        Answer::Status(code) => {
                            let _ = socket
                                .write_all(
                                    format!("HTTP/1.1 {code} X\r\ncontent-length: 0\r\n\r\n")
                                        .as_bytes(),
                                )
                                .await;
                            continue;
                        }
                        Answer::Redirect(to) => {
                            let _ = socket
                                .write_all(
                                    format!("HTTP/1.1 302 Found\r\nlocation: {to}{path}\r\ncontent-length: 0\r\n\r\n")
                                        .as_bytes(),
                                )
                                .await;
                            continue;
                        }
                        Answer::Short | Answer::Serve | Answer::Stall => {}
                    }
                    let Some(bytes) = bytes else {
                        let _ = socket
                            .write_all(b"HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\n\r\n")
                            .await;
                        continue;
                    };
                    let (status, body) = match &range {
                        Some(r) => {
                            let (a, b) = r.trim().split_once('-').unwrap();
                            let (a, b): (usize, usize) = (a.parse().unwrap(), b.parse().unwrap());
                            (
                                "206 Partial Content",
                                bytes[a..=b.min(bytes.len() - 1)].to_vec(),
                            )
                        }
                        None => ("200 OK", bytes),
                    };
                    let head = format!(
                        "HTTP/1.1 {status}\r\ncontent-length: {}\r\n\r\n",
                        body.len()
                    );
                    if socket.write_all(head.as_bytes()).await.is_err() {
                        return;
                    }
                    let half = body.len() / 2;
                    if socket.write_all(&body[..half]).await.is_err() {
                        return;
                    }
                    if matches!(answer, Answer::Short) {
                        let _ = socket.flush().await;
                        return;
                    }
                    if matches!(answer, Answer::Stall) {
                        let _ = socket.flush().await;
                        std::future::pending::<()>().await;
                    }
                    let _ = socket.flush().await;
                    tokio::task::yield_now().await;
                    if socket.write_all(&body[half..]).await.is_err() {
                        return;
                    }
                }
            });
        }
    });
    Server { url, state }
}

/// A scratch folder that is removed when it is dropped, also when the test
/// that made it fails (a panic unwinds through its owner).
#[derive(Debug)]
pub struct Scratch(PathBuf);

impl Scratch {
    /// The folder's path, owned.
    pub fn path_buf(&self) -> PathBuf {
        self.0.clone()
    }
}

impl std::ops::Deref for Scratch {
    type Target = std::path::Path;
    fn deref(&self) -> &std::path::Path {
        &self.0
    }
}

impl AsRef<std::path::Path> for Scratch {
    fn as_ref(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A new, empty scratch folder for one test, removed when dropped.
pub fn scratch(name: &str) -> Scratch {
    let dir =
        std::env::temp_dir().join(format!("mcbench-setup-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    Scratch(dir)
}

/// The names in a folder, sorted.
pub fn names(dir: &std::path::Path) -> Vec<String> {
    let mut list: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    list.sort();
    list
}

/// The sha256 of bytes in hexadecimal.
pub fn sha256(bytes: &[u8]) -> String {
    mcbench_setup::bundle::sha256(bytes)
}

/// A plain HTTP client for loopback.
pub fn client() -> reqwest::Client {
    mcbench_setup::fetch::client()
}

/// The client-jar manifest of one material (a 2x2 texture), its address
/// `jar_url`, as `client-jar.json`'s bytes, and the jar it names.
pub fn tiny_manifest(jar_url: &str) -> (Vec<u8>, Vec<u8>) {
    use mcbench_setup::jar::{Entry, Manifest, write_zip};
    let png = mcbench_setup::bake::encode_png(
        2,
        2,
        &[
            255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 128,
        ],
    )
    .unwrap();
    let mut entry = Entry {
        name: "assets/minecraft/textures/block/test.png".into(),
        offset: 0,
        data: 0,
        stored: png.len() as u64,
        size: png.len() as u64,
        method: 0,
        crc32: crc32fast::hash(&png),
        sha256: sha256(&png),
    };
    let jar = write_zip(&[(&entry, &png)]);
    let found = &mcbench_setup::jar::entries(&jar).unwrap()[0];
    entry.offset = found.offset;
    entry.data = found.data;
    let manifest = Manifest {
        minecraft: "1.21.10".into(),
        url: jar_url.into(),
        size: jar.len() as u64,
        sha1: String::new(),
        entries: vec![entry],
        materials: vec!["minecraft:block/test".into()],
        entity_atlas: vec![],
        particles: vec![("minecraft:test".into(), "block/test".into())],
    };
    (serde_json::to_vec(&manifest).unwrap(), jar)
}

/// A data folder of [`tiny_manifest`]'s material in `dir`; the jar.
pub fn tiny_data(dir: &std::path::Path, jar_url: &str) -> Vec<u8> {
    let (manifest, jar) = tiny_manifest(jar_url);
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join("client-jar.json"), manifest).unwrap();
    jar
}

/// A bundle of every shipped file and its lock, as `Data::unpack` takes
/// them (`client-jar.json` is [`tiny_manifest`]'s, each other file a line
/// naming it), and the jar.
pub fn tiny_bundle(jar_url: &str) -> (Vec<u8>, mcbench_setup::bundle::Lock, Vec<u8>) {
    use mcbench_setup::bundle::{FILES, Lock, tar};
    let (manifest, jar) = tiny_manifest(jar_url);
    let files: Vec<(&str, Vec<u8>)> = FILES
        .iter()
        .map(|n| match *n {
            "client-jar.json" => (*n, manifest.clone()),
            _ => (*n, format!("the file {n}\n").into_bytes()),
        })
        .collect();
    let list: Vec<(&str, &[u8])> = files.iter().map(|(n, b)| (*n, b.as_slice())).collect();
    let bundle = ruzstd::encoding::compress_to_vec(
        &tar(&list).unwrap()[..],
        ruzstd::encoding::CompressionLevel::Fastest,
    );
    let lock = Lock {
        minecraft: "1.21.10".into(),
        sha256: sha256(&bundle),
        size: bundle.len() as u64,
        url: "https://example.invalid/bundle".into(),
        zstd: "test".into(),
        files: files
            .iter()
            .map(|(n, b)| (n.to_string(), sha256(b), b.len() as u64))
            .collect(),
    };
    (bundle, lock, jar)
}
