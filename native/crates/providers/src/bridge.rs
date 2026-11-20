//! The tool bridge between a provider's CLI and the app (`server/mcp.js`
//! and `server/mcp-bridge.py`).
//!
//! The CLI starts `mcbench-lite tool-bridge <address>` as an ordinary stdio
//! MCP server (inside Codex's box on Linux, the static
//! `mcbench-sandbox-agent tool-bridge <socket>`: the same relay, from the
//! `mcbench-relay` crate both depend on); that process ([`relay_main`])
//! relays its stdin to the address
//! and the address to its stdout. The main process listens at the address
//! ([`serve`]) and answers each connection's newline-delimited JSON-RPC 2.0
//! exactly as `mcp.js` `serveMcp` does: `initialize`, `ping`, `tools/list`,
//! `tools/call` (with `_meta.callId` and `_meta["claudecode/toolUseId"]`),
//! `notifications/cancelled`, error codes -32700, -32601 and -32603, and the
//! host's content blocks (text, and images with their data and type) passed
//! through. JSON is read as `JSON.parse` reads it and written as
//! `JSON.stringify` writes it (`mcbench-json`).
//!
//! The address is a Unix socket on macOS and Linux and a named pipe on
//! Windows. Nothing listens on TCP. A Unix socket is bound and reached as
//! `mcbench_relay::bind_unix` and `connect_unix` do it, in a runtime
//! folder made by `harness::runtime_dir`, so its address does not depend on
//! how long the temporary folder's path is.
//!
//! Where it differs from `mcp.js`, by design: a cancelled `tools/call`
//! (`notifications/cancelled`, or its connection closing) is answered at
//! once with the reason and the host's future is dropped, where `mcp.js`
//! signals the host and answers when the host rejects (every host of today
//! rejects at once with that reason, except Claude Code's while it waits for
//! the call's `tool_use` block, which it does at the block); a line that
//! parses to `null`, or a request whose `params` is `null`, is ignored, where
//! `mcp.js` throws in its line handler (an unhandled rejection).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use futures_util::StreamExt;
use futures_util::stream::FuturesUnordered;
use serde_json::{Map, Value, json};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::ToolSchema;

pub use mcbench_relay::{BridgeAddress, relay_main};

/// One MCP content block a tool call answers with (`text`, or `image`
/// with its data and type), as `mcp.js` passes them through.
#[derive(Clone, Debug, PartialEq)]
pub struct McpContent(pub serde_json::Value);

/// The main process's side: the tools a CLI session is offered and how a
/// call is answered (`mcp.js` `host.tools`, `host.call`).
#[async_trait]
pub trait BridgeHost: Send + Sync {
    /// The tools for `tools/list`, as the turn offers them (`agent.js`
    /// `toolsFor`: `{"type":"function","function":{...}}`).
    fn tools(&self) -> Vec<ToolSchema>;
    /// Answers `tools/call`: `call_id` is `_meta.callId` or the request
    /// id, `tool_use_id` Claude Code's `_meta["claudecode/toolUseId"]`.
    /// `cancel` is cancelled when the call is (the client's
    /// `notifications/cancelled`, or its connection closing); the future
    /// is then dropped.
    async fn call(
        &self,
        name: &str,
        arguments: serde_json::Value,
        call_id: &str,
        tool_use_id: Option<&str>,
        cancel: &CancellationToken,
    ) -> Result<Vec<McpContent>, String>;
}

/// A bridge serving one CLI session until it is stopped.
pub struct BridgeServer {
    stop: CancellationToken,
    socket: Option<PathBuf>,
}

impl std::fmt::Debug for BridgeServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BridgeServer")
            .field("socket", &self.socket)
            .finish()
    }
}

impl BridgeServer {
    /// Stops serving and closes every connection, including one a killed
    /// CLI left behind (`mcp.js` `shutdown`). The socket file is removed, as
    /// Node's server removes it when it closes.
    pub fn stop(self) {
        drop(self);
    }
}

impl Drop for BridgeServer {
    fn drop(&mut self) {
        self.stop.cancel();
        if let Some(socket) = self.socket.take() {
            let _ = std::fs::remove_file(socket);
        }
    }
}

/// Listens at `address` and answers each connection's MCP requests with
/// `host`, as `mcp.js` `serveMcp` does. Called on the core's runtime,
/// which the connections' tasks are spawned on. A Unix socket's folder is
/// made if it is missing; a socket that exists already is an error, as
/// `listen` fails on one.
pub fn serve(address: &BridgeAddress, host: Arc<dyn BridgeHost>) -> Result<BridgeServer, String> {
    let stop = CancellationToken::new();
    match address {
        BridgeAddress::UnixSocket(path) => {
            #[cfg(unix)]
            {
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent)
                        .map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
                }
                let listener = mcbench_relay::bind_unix(path)
                    .and_then(|listener| {
                        listener.set_nonblocking(true)?;
                        tokio::net::UnixListener::from_std(listener)
                    })
                    .map_err(|e| format!("listen {}: {e}", path.display()))?;
                let token = stop.clone();
                tokio::spawn(async move {
                    loop {
                        tokio::select! {
                            _ = token.cancelled() => return,
                            accepted = listener.accept() => match accepted {
                                Ok((stream, _)) => {
                                    tokio::spawn(connection(stream, host.clone(), token.clone()));
                                }
                                Err(_) => return,
                            },
                        }
                    }
                });
                Ok(BridgeServer {
                    stop,
                    socket: Some(path.clone()),
                })
            }
            #[cfg(not(unix))]
            {
                let _ = (path, host);
                Err("A Unix socket bridge needs macOS or Linux".to_owned())
            }
        }
        BridgeAddress::NamedPipe(name) => {
            #[cfg(windows)]
            {
                use tokio::net::windows::named_pipe::ServerOptions;
                let mut server = ServerOptions::new()
                    .first_pipe_instance(true)
                    .create(name)
                    .map_err(|e| format!("listen {name}: {e}"))?;
                let token = stop.clone();
                let name = name.clone();
                tokio::spawn(async move {
                    loop {
                        tokio::select! {
                            _ = token.cancelled() => return,
                            connected = server.connect() => {
                                if connected.is_err() {
                                    return;
                                }
                                let next = match ServerOptions::new().create(&name) {
                                    Ok(next) => next,
                                    Err(_) => return,
                                };
                                let stream = std::mem::replace(&mut server, next);
                                tokio::spawn(connection(stream, host.clone(), token.clone()));
                            }
                        }
                    }
                });
                Ok(BridgeServer { stop, socket: None })
            }
            #[cfg(not(windows))]
            {
                let _ = (name, host);
                Err("A named pipe bridge needs Windows".to_owned())
            }
        }
    }
}

/// A request id as a JavaScript `Map` keys it (SameValueZero): numbers by
/// value, strings by value, objects and arrays by identity (never equal).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum IdKey {
    Number(u64),
    String(String),
    Bool(bool),
    Null,
    Unique(u64),
}

fn id_key(id: &Value, unique: &mut u64) -> IdKey {
    match id {
        Value::Number(n) => {
            let x = n.as_f64().unwrap_or(f64::NAN);
            // -0 and 0 are the same key.
            IdKey::Number(if x == 0.0 { 0 } else { x.to_bits() })
        }
        Value::String(s) => IdKey::String(s.clone()),
        Value::Bool(b) => IdKey::Bool(*b),
        Value::Null => IdKey::Null,
        _ => {
            *unique += 1;
            IdKey::Unique(*unique)
        }
    }
}

/// JavaScript's `String(value)` for a JSON value.
pub(crate) fn js_string(value: &Value) -> String {
    match value {
        Value::Null => "null".to_owned(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => mcbench_json::number_to_string(n.as_f64().unwrap_or(f64::NAN)),
        Value::String(s) => s.clone(),
        Value::Array(items) => items
            .iter()
            .map(|item| match item {
                Value::Null => String::new(),
                other => js_string(other),
            })
            .collect::<Vec<_>>()
            .join(","),
        Value::Object(_) => "[object Object]".to_owned(),
    }
}

/// JavaScript's truthiness of a JSON value.
pub(crate) fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|x| x != 0.0 && !x.is_nan()),
        Some(Value::String(s)) => !s.is_empty(),
        Some(_) => true,
    }
}

/// The longest line read from a CLI's stdout or a bridge client, in bytes
/// (64 MiB). The longest line Codex 0.169.0 sends is an `item.completed`
/// of an `mcp_tool_call`, which carries the whole tool result (its exec
/// events' `McpToolCallItemResult`), and Claude Code's stream-json echoes
/// a tool result too: mcbench's largest is a see_build of six shots
/// (`MAX_SHOTS`) of 2184x1640 JPEG at quality 90 (`shot.js`), at most
/// 6,696,684 bytes each (measured: uniform noise, no chroma subsampling),
/// 53,573,472 bytes in base64. Node's `readline` reads without a limit; a
/// longer line is a process in the box writing to the app's pipe.
pub(crate) const MAX_LINE: usize = 64 << 20;

/// Splits a byte stream into lines as Node's `readline` does: at `\n`,
/// `\r\n` or a lone `\r`, decoding UTF-8 (invalid bytes become U+FFFD). A
/// line longer than [`MAX_LINE`] ends it: [`LineSplitter::too_long`], and
/// no more lines.
pub(crate) struct LineSplitter {
    buffer: Vec<u8>,
    after_cr: bool,
    too_long: bool,
}

impl LineSplitter {
    pub(crate) fn new() -> Self {
        LineSplitter {
            buffer: Vec::new(),
            after_cr: false,
            too_long: false,
        }
    }

    /// Feeds bytes; returns the lines they complete, up to a line longer
    /// than [`MAX_LINE`].
    pub(crate) fn feed(&mut self, bytes: &[u8]) -> Vec<String> {
        let mut lines = Vec::new();
        let mut rest = bytes;
        while !self.too_long && !rest.is_empty() {
            if self.after_cr {
                self.after_cr = false;
                if rest[0] == b'\n' {
                    rest = &rest[1..];
                    continue;
                }
            }
            let end = rest.iter().position(|b| *b == b'\n' || *b == b'\r');
            let part = &rest[..end.unwrap_or(rest.len())];
            if self.buffer.len() + part.len() > MAX_LINE {
                self.too_long = true;
                self.buffer = Vec::new();
                break;
            }
            self.buffer.extend_from_slice(part);
            let Some(end) = end else {
                break;
            };
            lines.push(self.take());
            self.after_cr = rest[end] == b'\r';
            rest = &rest[end + 1..];
        }
        lines
    }

    /// Whether a line longer than [`MAX_LINE`] came.
    pub(crate) fn too_long(&self) -> bool {
        self.too_long
    }

    /// The last line, which no line end closed, once the input ends.
    pub(crate) fn finish(&mut self) -> Option<String> {
        if self.buffer.is_empty() {
            None
        } else {
            Some(self.take())
        }
    }

    fn take(&mut self) -> String {
        String::from_utf8_lossy(&std::mem::take(&mut self.buffer)).into_owned()
    }
}

/// JavaScript's `String.prototype.trim` emptiness test.
pub(crate) fn blank(line: &str) -> bool {
    line.trim_matches(|c: char| c.is_whitespace() || c == '\u{feff}')
        .is_empty()
}

struct Inflight {
    token: CancellationToken,
    reason: Arc<Mutex<Option<String>>>,
}

impl Inflight {
    fn abort(&self, reason: &str) {
        let mut slot = self.reason.lock().unwrap();
        if slot.is_none() {
            *slot = Some(reason.to_owned());
        }
        drop(slot);
        self.token.cancel();
    }
}

/// One client connection: lines in, replies out, until either side ends or
/// the server stops.
async fn connection<S>(stream: S, host: Arc<dyn BridgeHost>, stop: CancellationToken)
where
    S: AsyncRead + AsyncWrite + Send + 'static,
{
    let (mut reader, mut writer) = tokio::io::split(stream);
    // `None` ends the connection's writes: what was queued before it goes
    // out, anything after it is dropped (a reply to a call the closing
    // connection cancelled, which Node writes after `end`).
    let (send, mut outbox) = mpsc::unbounded_channel::<Option<String>>();
    let closed = CancellationToken::new();
    let writer_closed = closed.clone();
    let writing = tokio::spawn(async move {
        while let Some(Some(line)) = outbox.recv().await {
            if writer.write_all(line.as_bytes()).await.is_err() {
                writer_closed.cancel();
                return;
            }
        }
        let _ = writer.shutdown().await;
    });
    let inflight: Arc<Mutex<HashMap<IdKey, Arc<Inflight>>>> = Arc::new(Mutex::new(HashMap::new()));
    let mut unique = 0u64;
    let mut lines = LineSplitter::new();
    let mut chunk = vec![0u8; 65536];
    // Calls are polled here, first in the order their lines came.
    let mut calls: FuturesUnordered<CallFuture> = FuturesUnordered::new();
    loop {
        let read = tokio::select! {
            _ = stop.cancelled() => break,
            _ = closed.cancelled() => break,
            Some(()) = calls.next(), if !calls.is_empty() => continue,
            read = reader.read(&mut chunk) => read,
        };
        let n = match read {
            Ok(0) | Err(_) => {
                if let Some(line) = lines.finish() {
                    calls.extend(handle_line(&line, &host, &send, &inflight, &mut unique));
                }
                break;
            }
            Ok(n) => n,
        };
        for line in lines.feed(&chunk[..n]) {
            calls.extend(handle_line(&line, &host, &send, &inflight, &mut unique));
        }
        // A line longer than any request a CLI sends: the client is not
        // one, and its connection ends.
        if lines.too_long() {
            break;
        }
    }
    // The client is gone (or the server stopped): the connection ends, as
    // Node ends a socket whose other side ended, and its calls are cancelled.
    let _ = send.send(None);
    let cancelled: Vec<Arc<Inflight>> = inflight.lock().unwrap().values().cloned().collect();
    for call in cancelled {
        call.abort("MCP client disconnected");
    }
    if !stop.is_cancelled() {
        while calls.next().await.is_some() {}
    }
    drop(calls);
    drop(send);
    if stop.is_cancelled() {
        writing.abort();
    } else {
        let _ = writing.await;
    }
}

fn reply(send: &mpsc::UnboundedSender<Option<String>>, id: &Value, body: (&str, Value)) {
    let mut message = Map::new();
    message.insert("jsonrpc".to_owned(), json!("2.0"));
    message.insert("id".to_owned(), id.clone());
    message.insert(body.0.to_owned(), body.1);
    let _ = send.send(Some(
        mcbench_json::stringify(&Value::Object(message)) + "\n",
    ));
}

fn error(code: i64, message: &str) -> Value {
    let mut error = Map::new();
    error.insert("code".to_owned(), json!(code));
    error.insert("message".to_owned(), json!(message));
    Value::Object(error)
}

/// A `tools/call` under way.
type CallFuture = std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>;

fn handle_line(
    line: &str,
    host: &Arc<dyn BridgeHost>,
    send: &mpsc::UnboundedSender<Option<String>>,
    inflight: &Arc<Mutex<HashMap<IdKey, Arc<Inflight>>>>,
    unique: &mut u64,
) -> Option<CallFuture> {
    if blank(line) {
        return None;
    }
    let message = match mcbench_json::parse(line) {
        Ok(message) => message,
        Err(_) => {
            reply(send, &Value::Null, ("error", error(-32700, "Parse error")));
            return None;
        }
    };
    let Value::Object(message) = message else {
        // A number, string or array has no `id`: a notification of no
        // method. `null` is ignored (see the module's notes).
        return None;
    };
    let empty = Map::new();
    let params = match message.get("params") {
        None => &empty,
        Some(Value::Object(params)) => params,
        Some(Value::Null) => return None,
        Some(_) => &empty,
    };
    let method = message.get("method");
    let Some(id) = message.get("id") else {
        if method == Some(&json!("notifications/cancelled"))
            && let Some(request) = params.get("requestId")
        {
            let key = id_key(request, unique);
            let call = inflight.lock().unwrap().get(&key).cloned();
            if let Some(call) = call {
                call.abort("Cancelled by harness");
            }
        }
        return None;
    };
    match method.and_then(Value::as_str) {
        Some("initialize") => {
            let mut result = Map::new();
            if let Some(version) = params.get("protocolVersion") {
                result.insert("protocolVersion".to_owned(), version.clone());
            }
            result.insert(
                "capabilities".to_owned(),
                json!({ "tools": { "listChanged": false } }),
            );
            result.insert(
                "serverInfo".to_owned(),
                json!({ "name": "mcbench", "version": "1" }),
            );
            reply(send, id, ("result", Value::Object(result)));
        }
        Some("ping") => reply(send, id, ("result", json!({}))),
        Some("tools/list") => {
            let tools: Vec<Value> = host
                .tools()
                .iter()
                .map(|schema| {
                    let tool = mcbench_json::parse(&schema.0)
                        .expect("a tool schema is JSON text (agent.js toolsFor)");
                    let function = tool.get("function").cloned().unwrap_or(Value::Null);
                    let mut entry = Map::new();
                    for (from, to) in [
                        ("name", "name"),
                        ("description", "description"),
                        ("parameters", "inputSchema"),
                    ] {
                        if let Some(value) = function.get(from) {
                            entry.insert(to.to_owned(), value.clone());
                        }
                    }
                    Value::Object(entry)
                })
                .collect();
            reply(send, id, ("result", json!({ "tools": tools })));
        }
        Some("tools/call") => {
            let key = id_key(id, unique);
            let call = Arc::new(Inflight {
                token: CancellationToken::new(),
                reason: Arc::new(Mutex::new(None)),
            });
            inflight.lock().unwrap().insert(key.clone(), call.clone());
            let name = params
                .get("name")
                .map(js_string)
                .unwrap_or_else(|| "undefined".to_owned());
            let arguments = match params.get("arguments") {
                None | Some(Value::Null) => Value::Object(Map::new()),
                Some(arguments) => arguments.clone(),
            };
            let meta = params.get("_meta");
            let call_id = match meta.and_then(|meta| meta.get("callId")) {
                None | Some(Value::Null) => js_string(id),
                Some(call_id) => js_string(call_id),
            };
            let tool_use_id = meta
                .and_then(|meta| meta.get("claudecode/toolUseId"))
                .filter(|value| truthy(Some(value)))
                .map(js_string);
            let host = host.clone();
            let send = send.clone();
            let inflight = inflight.clone();
            let id = id.clone();
            return Some(Box::pin(async move {
                let answer = tokio::select! {
                    answer = host.call(&name, arguments, &call_id, tool_use_id.as_deref(), &call.token) => answer,
                    _ = call.token.cancelled() => Err(call.reason.lock().unwrap().clone().unwrap_or_default()),
                };
                match answer {
                    Ok(content) => {
                        let content: Vec<Value> = content.into_iter().map(|c| c.0).collect();
                        reply(
                            &send,
                            &id,
                            ("result", json!({ "content": content, "isError": false })),
                        )
                    }
                    Err(message) => reply(&send, &id, ("error", error(-32603, &message))),
                }
                // `inflight.delete(id)` removes whichever call holds the id.
                inflight.lock().unwrap().remove(&key);
            }));
        }
        _ => {
            let method = method
                .map(js_string)
                .unwrap_or_else(|| "undefined".to_owned());
            reply(
                send,
                id,
                (
                    "error",
                    error(-32601, &format!("Method not found: {method}")),
                ),
            );
        }
    }
    None
}
