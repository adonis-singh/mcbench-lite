//! The bridge: every step of `tests/golden/mcp-steps.json` against
//! `bridge::serve`, answered byte for byte as today's `mcp.js` answered it
//! (`tests/golden/mcp.jsonl`, written by `tests/golden/mcp.js` with the same
//! host), and the relay (`mcbench-lite tool-bridge`) end to end.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use mcbench_providers::ToolSchema;
use mcbench_providers::bridge::{self, BridgeAddress, BridgeHost, McpContent};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;

use super::bridge_tests;
use super::{Test, scratch, sleep, tests};

pub fn all() -> Vec<Test> {
    tests![
        bridge_tests::every_protocol_step_is_answered_as_mcp_js_answers_it,
        bridge_tests::the_relay_carries_a_session_between_stdio_and_the_socket_and_exits_when_it_ends,
        bridge_tests::a_relay_whose_address_does_not_answer_exits_1_with_the_reason,
        bridge_tests::a_runs_sockets_work_while_the_temporary_folders_path_is_200_bytes,
    ]
}

/// A connection to the Unix socket at `path`, made as the relay makes it.
fn connect(path: &Path) -> tokio::net::UnixStream {
    let stream = mcbench_relay::connect_unix(path).unwrap();
    stream.set_nonblocking(true).unwrap();
    tokio::net::UnixStream::from_std(stream).unwrap()
}

/// The golden's host (see `tests/golden/mcp.js`). Its `slow` answers once
/// the test gives it a permit (the golden's host answers it after 300 ms):
/// the test gives it once what must come before it has come, so the order
/// of the answers is not decided by how fast the machine runs.
struct Host {
    slow: tokio::sync::Semaphore,
}

impl Default for Host {
    fn default() -> Host {
        Host {
            slow: tokio::sync::Semaphore::new(0),
        }
    }
}

#[async_trait::async_trait]
impl BridgeHost for Host {
    fn tools(&self) -> Vec<ToolSchema> {
        vec![
            ToolSchema(r#"{"type":"function","function":{"name":"echo","description":"Echo","parameters":{"type":"object","properties":{"n":{"type":"number"}}}}}"#.to_owned()),
            ToolSchema(r#"{"type":"function","function":{"name":"nodesc","parameters":{"type":"object"}}}"#.to_owned()),
        ]
    }

    async fn call(
        &self,
        name: &str,
        arguments: Value,
        call_id: &str,
        tool_use_id: Option<&str>,
        cancel: &CancellationToken,
    ) -> Result<Vec<McpContent>, String> {
        match name {
            "fail" => return Err(format!("failed: {name}")),
            "image" => {
                return Ok(vec![
                    McpContent(json!({ "type": "image", "data": "AA==", "mimeType": "image/png" })),
                    McpContent(json!({ "type": "text", "text": "shot" })),
                ]);
            }
            "slow" => self.slow.acquire().await.unwrap().forget(),
            "hang" => {
                cancel.cancelled().await;
                std::future::pending::<()>().await;
            }
            _ => {}
        }
        let text =
            json!({ "name": name, "args": arguments, "callId": call_id, "toolUseId": tool_use_id });
        Ok(vec![McpContent(
            json!({ "type": "text", "text": mcbench_json::stringify(&text) }),
        )])
    }
}

pub async fn every_protocol_step_is_answered_as_mcp_js_answers_it() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden");
    let steps: Vec<Value> =
        serde_json::from_str(&std::fs::read_to_string(root.join("mcp-steps.json")).unwrap())
            .unwrap();
    let expected: Vec<Value> = std::fs::read_to_string(root.join("mcp.jsonl"))
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let dir = crate::scratch::Scratch::for_sockets("bridge-golden");
    let socket = dir.join("mcp.sock");
    let host = Arc::new(Host::default());
    let server = bridge::serve(&BridgeAddress::UnixSocket(socket.clone()), host.clone()).unwrap();
    for (step, expected) in steps.iter().zip(&expected) {
        let (name, expected) = (
            step["name"].as_str().unwrap(),
            expected["received"].as_str().unwrap(),
        );
        let stream = connect(&socket);
        let (mut read, mut write) = stream.into_split();
        let got = Arc::new(std::sync::Mutex::new(Vec::<u8>::new()));
        let received = tokio::spawn({
            let got = got.clone();
            async move {
                let mut chunk = vec![0u8; 65536];
                while let Ok(n @ 1..) = read.read(&mut chunk).await {
                    got.lock().unwrap().extend_from_slice(&chunk[..n]);
                }
            }
        });
        let text = || String::from_utf8(got.lock().unwrap().clone()).unwrap();
        for part in step["send"].as_array().unwrap() {
            write
                .write_all(part.as_str().unwrap().as_bytes())
                .await
                .unwrap();
            sleep(step["gap"].as_u64().unwrap_or(20)).await;
        }
        // A slow call answers once every answer the golden has before its
        // own has come.
        let slow = |line: &str| line.contains(r#"\"name\":\"slow\""#);
        let slow_answers = expected.lines().filter(|l| slow(l)).count();
        if slow_answers > 0 {
            let before: String = expected
                .lines()
                .filter(|l| !slow(l))
                .map(|l| format!("{l}\n"))
                .collect();
            until(name, || text() == before).await;
            host.slow.add_permits(slow_answers);
        }
        until(name, || text() == expected).await;
        // What the golden waited for: anything more that came in it is
        // compared below.
        sleep(step["wait"].as_u64().unwrap()).await;
        if step["end"].as_bool().unwrap_or(false) {
            write.shutdown().await.unwrap();
        }
        sleep(100).await;
        drop(write);
        if tokio::time::timeout(Duration::from_secs(30), received)
            .await
            .is_err()
        {
            panic!("step {name} kept its connection open");
        }
        assert_eq!(text(), expected, "step {name}");
    }
    server.stop();
    assert!(!socket.exists(), "stopping removes the socket");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Waits until `done`, failing after 30 seconds (a hang, not a slow
/// machine).
async fn until(what: &str, done: impl Fn() -> bool) {
    let started = std::time::Instant::now();
    while !done() {
        assert!(started.elapsed() < Duration::from_secs(30), "step {what}");
        sleep(5).await;
    }
}

pub async fn the_relay_carries_a_session_between_stdio_and_the_socket_and_exits_when_it_ends() {
    let dir = crate::scratch::Scratch::for_sockets("relay");
    let socket = dir.join("mcp.sock");
    let server = bridge::serve(
        &BridgeAddress::UnixSocket(socket.clone()),
        Arc::new(Host::default()),
    )
    .unwrap();
    let mut relay = tokio::process::Command::new(std::env::current_exe().unwrap())
        .args(["tool-bridge", &socket.to_string_lossy()])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut stdin = relay.stdin.take().unwrap();
    let mut lines = tokio::io::BufReader::new(relay.stdout.take().unwrap()).lines();
    stdin
        .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}\n")
        .await
        .unwrap();
    assert_eq!(
        lines.next_line().await.unwrap().unwrap(),
        r#"{"jsonrpc":"2.0","id":1,"result":{}}"#
    );
    let big = "x".repeat(200_000);
    let request = json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": { "name": "echo", "arguments": { "big": big } } });
    stdin
        .write_all((mcbench_json::stringify(&request) + "\n").as_bytes())
        .await
        .unwrap();
    let answer: Value = serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
    let echoed: Value =
        serde_json::from_str(answer["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(echoed["args"]["big"].as_str().unwrap().len(), 200_000);
    // stdin's end shuts the connection's write side; the server ends its
    // side, and the relay exits 0.
    drop(stdin);
    let status = tokio::time::timeout(Duration::from_secs(60), relay.wait())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(status.code(), Some(0));
    server.stop();
    let _ = std::fs::remove_dir_all(&dir);
}

pub async fn a_relay_whose_address_does_not_answer_exits_1_with_the_reason() {
    let dir = scratch("relay-nowhere");
    let out = tokio::process::Command::new(std::env::current_exe().unwrap())
        .args(["tool-bridge", &dir.join("none.sock").to_string_lossy()])
        .stdin(std::process::Stdio::null())
        .output()
        .await
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).starts_with("mcbench-lite tool-bridge: connect "));
    let _ = std::fs::remove_dir_all(&dir);
}

/// Removes a folder when dropped, so also when an assertion fails.
struct Removed(std::path::PathBuf);

impl Drop for Removed {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub async fn a_runs_sockets_work_while_the_temporary_folders_path_is_200_bytes() {
    let tmp = std::env::temp_dir();
    assert!(
        tmp.as_os_str().len() >= super::LONG_TMPDIR,
        "the tests' temporary folder is {} bytes: {}",
        tmp.as_os_str().len(),
        tmp.display()
    );
    // A run's runtime folder as a Codex run makes it, its socket folder
    // `sock`, and in it the bridge and the egress proxy.
    let runtime = Removed(mcbench_providers::harness::runtime_dir().unwrap());
    let sock = runtime.0.join("sock");
    std::fs::create_dir_all(&sock).unwrap();
    let bridge_socket = sock.join("mcp.sock");
    let egress_socket = sock.join("egress.sock");
    println!(
        "  temporary folder {} bytes, egress socket's path {} bytes",
        tmp.as_os_str().len(),
        egress_socket.as_os_str().len()
    );
    if cfg!(target_os = "macos") {
        // Bound by its whole path, in the account's own temporary folder.
        assert!(
            egress_socket.as_os_str().len() <= 103,
            "{}",
            egress_socket.display()
        );
    } else {
        // Bound through its folder: its path is longer than any address.
        assert!(
            egress_socket.as_os_str().len() > 107,
            "{}",
            egress_socket.display()
        );
    }
    let server = bridge::serve(
        &BridgeAddress::UnixSocket(bridge_socket.clone()),
        Arc::new(Host::default()),
    )
    .unwrap();
    let log = sock.join("egress.ndjson");
    let proxy = mcbench_sandbox::proxy::start_egress_proxy(
        &egress_socket,
        vec!["chatgpt.com".into()],
        log.clone(),
    )
    .await
    .unwrap();

    // The bridge, through the relay a CLI starts.
    let mut relay = tokio::process::Command::new(std::env::current_exe().unwrap())
        .args(["tool-bridge", &bridge_socket.to_string_lossy()])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut stdin = relay.stdin.take().unwrap();
    let mut lines = tokio::io::BufReader::new(relay.stdout.take().unwrap()).lines();
    stdin
        .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}\n")
        .await
        .unwrap();
    assert_eq!(
        lines.next_line().await.unwrap().unwrap(),
        r#"{"jsonrpc":"2.0","id":1,"result":{}}"#
    );
    drop(stdin);
    let status = tokio::time::timeout(Duration::from_secs(60), relay.wait())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(status.code(), Some(0));

    // The egress proxy, reached as its relay in the box reaches it.
    let mut client = connect(&egress_socket);
    client
        .write_all(b"CONNECT example.com:443 HTTP/1.1\r\nHost: example.com:443\r\n\r\n")
        .await
        .unwrap();
    let mut reply = String::new();
    client.read_to_string(&mut reply).await.unwrap();
    assert!(reply.starts_with("HTTP/1.1 403"), "{reply}");
    assert!(
        std::fs::read_to_string(&log)
            .unwrap()
            .contains("example.com:443")
    );

    proxy.close().await;
    server.stop();
    assert!(
        !bridge_socket.exists(),
        "stopping the bridge removes its socket"
    );
}
