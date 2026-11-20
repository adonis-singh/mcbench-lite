//! What the updater's integration tests share: a file server on
//! 127.0.0.1 and the app's HTTP client.

// Each test file includes this module and uses a part of it.
#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::Arc;

use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// The `/hang…` paths whose request a server has read whole and will never
/// answer.
static HUNG: std::sync::Mutex<std::collections::BTreeSet<String>> =
    std::sync::Mutex::new(std::collections::BTreeSet::new());

/// Whether a server has read the whole request for `path` (a `/hang…`
/// path) and is holding it unanswered.
pub fn hung(path: &str) -> bool {
    HUNG.lock().unwrap().contains(path)
}

/// How many requests each served URL has had.
static REQUESTS: std::sync::Mutex<std::collections::BTreeMap<String, usize>> =
    std::sync::Mutex::new(std::collections::BTreeMap::new());

/// How many requests the server has read for `url` (its whole address).
pub fn requests(url: &str) -> usize {
    REQUESTS.lock().unwrap().get(url).copied().unwrap_or(0)
}

/// What a served body starts with to stall after it is sent.
pub const STALL: &[u8] = b"STALL";

/// Serves `files` by path on 127.0.0.1 (an ephemeral port); a path that
/// starts with `/hang` never answers ([`hung`] says when it was read),
/// and a body that starts with [`STALL`] stops before its end.
/// Returns the base URL.
pub async fn serve(files: HashMap<String, Vec<u8>>) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let files = Arc::new(files);
    let served = base.clone();
    tokio::spawn(async move {
        loop {
            let (mut socket, _) = listener.accept().await.unwrap();
            let files = files.clone();
            let base_of_this = served.clone();
            tokio::spawn(async move {
                let mut request = Vec::new();
                let mut buf = [0u8; 1024];
                while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                    let n = socket.read(&mut buf).await.unwrap();
                    if n == 0 {
                        return;
                    }
                    request.extend_from_slice(&buf[..n]);
                }
                let line = String::from_utf8_lossy(&request);
                let path = line.split(' ').nth(1).unwrap().to_string();
                *REQUESTS
                    .lock()
                    .unwrap()
                    .entry(format!("{base_of_this}{path}"))
                    .or_default() += 1;
                if path.starts_with("/hang") {
                    HUNG.lock().unwrap().insert(path.clone());
                    std::future::pending::<()>().await;
                }
                let (status, body) = match files.get(&path) {
                    Some(body) => ("200 OK", body.clone()),
                    None => ("404 Not Found", Vec::new()),
                };
                // A body that starts with STALL is sent, then the answer
                // stops with 100 bytes still owed.
                let stalls = body.starts_with(STALL);
                let head = format!(
                    "HTTP/1.1 {status}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                    body.len() + if stalls { 100 } else { 0 }
                );
                socket.write_all(head.as_bytes()).await.unwrap();
                socket.write_all(&body).await.unwrap();
                if stalls {
                    std::future::pending::<()>().await;
                }
            });
        }
    });
    base
}

/// The app's own client (`mcbench_setup::fetch::client()`), so the tests
/// run the updater over the settings it has in the app.
pub fn client() -> reqwest::Client {
    mcbench_setup::fetch::client()
}
