//! The runner, the fake CLI's helpers and the differential driver.

mod box_tests;
mod bridge_tests;
mod claude_tests;
mod codex_tests;
mod diff;

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use mcbench_providers::bridge::{BridgeAddress, relay_main};
use mcbench_providers::harness::claude_code::{ClaudeCode, ClaudeConfig};
use mcbench_providers::harness::codex_cli::{CodexCli, CodexConfig, Launch};
use mcbench_providers::harness::{BridgeCommand, TranscriptEntry};
use mcbench_providers::{ApiMessage, CompletionRequest, Delta, ProviderError, ToolSchema};
use serde_json::Map;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

type Test = (
    &'static str,
    fn() -> Pin<Box<dyn Future<Output = ()> + Send>>,
);

macro_rules! tests {
    ($($module:ident :: $name:ident),* $(,)?) => {
        vec![$((concat!(stringify!($module), "::", stringify!($name)), (|| Box::pin($module::$name()) as std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>) as fn() -> _)),*]
    };
}
pub(crate) use tests;

pub fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("tool-bridge") {
        relay_main(&BridgeAddress::UnixSocket(PathBuf::from(&args[2])));
    }
    if let Ok(dir) = std::env::var("MCBENCH_DIFF_HARNESS") {
        let filter = args.iter().skip(1).find(|a| !a.starts_with('-')).cloned();
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(4)
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(diff::run(PathBuf::from(dir), filter));
        return;
    }
    // Claude Code's working folder is under HOME: a scratch one, set while
    // this process has one thread, so nothing is written in the real home.
    let home = crate::scratch::Scratch::new("cli-home");
    // SAFETY: main has started no other thread yet, so nothing reads the
    // environment while it changes.
    unsafe { std::env::set_var("HOME", &*home) };
    // Every test runs with a temporary folder whose path is longer than a
    // Unix socket's address may be (104 bytes on macOS, 108 on Linux): no
    // socket of the app's may depend on its length.
    let tmp = crate::scratch::Scratch::new("cli-tmp");
    let tmpdir = long_folder(&tmp, LONG_TMPDIR);
    // SAFETY: as for HOME, no other thread has started.
    unsafe { std::env::set_var("TMPDIR", &tmpdir) };
    let filter = args.iter().skip(1).find(|a| !a.starts_with('-')).cloned();
    let mut all: Vec<Test> = Vec::new();
    all.extend(bridge_tests::all());
    all.extend(claude_tests::all());
    all.extend(codex_tests::all());
    all.extend(box_tests::all());
    all.push(("dump::a_failure_dump_never_holds_the_login", || {
        Box::pin(crate::dump::a_failure_dump_never_holds_the_login())
            as Pin<Box<dyn Future<Output = ()> + Send>>
    }));
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .unwrap();
    let mut failed = Vec::new();
    let mut ran = 0;
    for (name, test) in all {
        if filter.as_ref().is_some_and(|f| !name.contains(f.as_str())) {
            continue;
        }
        ran += 1;
        let outcome = runtime.block_on(async { tokio::spawn(test()).await });
        match outcome {
            Ok(()) => println!("test {name} ... ok"),
            Err(error) => {
                println!("test {name} ... FAILED");
                if let Ok(panic) = error.try_into_panic() {
                    if let Some(text) = panic.downcast_ref::<String>() {
                        println!("  {text}");
                    } else if let Some(text) = panic.downcast_ref::<&str>() {
                        println!("  {text}");
                    }
                }
                failed.push(name);
            }
        }
    }
    drop(runtime);
    drop(tmp);
    drop(home);
    println!(
        "\ncli: {} passed, {} failed",
        ran - failed.len(),
        failed.len()
    );
    if !failed.is_empty() {
        for name in &failed {
            println!("  failed: {name}");
        }
        std::process::exit(1);
    }
}

/// The scripted fake CLI in its own folder: `<dir>/bin/<name>`.
pub struct Fake {
    pub dir: PathBuf,
    pub program: PathBuf,
}

impl Fake {
    pub fn new(dir: &Path, name: &str, script: &Value) -> Fake {
        let _ = std::fs::remove_dir_all(dir);
        let bin = dir.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let program = bin.join(name);
        std::fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fakes/fake_cli.py"),
            &program,
        )
        .unwrap();
        std::fs::set_permissions(
            &program,
            std::os::unix::fs::PermissionsExt::from_mode(0o755),
        )
        .unwrap();
        std::fs::write(bin.join("script.json"), mcbench_json::stringify(script)).unwrap();
        Fake {
            dir: dir.to_path_buf(),
            program,
        }
    }

    /// Every line the fake recorded.
    pub fn records(&self) -> Vec<Value> {
        match std::fs::read_to_string(self.dir.join("bin/record.jsonl")) {
            Ok(text) => text
                .lines()
                .map(|l| serde_json::from_str(l).unwrap())
                .collect(),
            Err(_) => Vec::new(),
        }
    }

    /// The latest start a fake recorded in `folder` (inside Codex's box
    /// it records under CODEX_HOME). Only the box test, which is Linux's.
    #[cfg(target_os = "linux")]
    pub fn records_in(&self, folder: &Path) -> Value {
        let text = std::fs::read_to_string(folder.join("record.jsonl")).unwrap();
        text.lines()
            .rev()
            .map(|l| serde_json::from_str::<Value>(l).unwrap())
            .find_map(|r| r.get("start").cloned())
            .expect("a start")
    }

    /// How many times it was started.
    pub fn spawns(&self) -> usize {
        self.records()
            .iter()
            .filter(|r| r.get("start").is_some())
            .count()
    }

    /// The argv of its latest start.
    pub fn args(&self) -> Vec<String> {
        let start = self
            .records()
            .into_iter()
            .rev()
            .find_map(|r| r.get("start").cloned())
            .expect("a start");
        serde_json::from_value(start["argv"].clone()).unwrap()
    }

    pub fn last_start(&self) -> Value {
        self.records()
            .into_iter()
            .rev()
            .find_map(|r| r.get("start").cloned())
            .expect("a start")
    }

    /// Its latest stdin as text (Claude: one stream-json line).
    pub fn input_text(&self) -> String {
        let input = self
            .records()
            .into_iter()
            .rev()
            .find_map(|r| r.get("stdin").cloned())
            .expect("an input");
        input.as_str().unwrap().to_owned()
    }

    /// Its latest stdin as Claude's stream-json message.
    pub fn input(&self) -> Value {
        serde_json::from_str(&self.input_text()).unwrap()
    }
}

pub fn flag<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter()
        .position(|a| a == name)
        .map(|i| args[i + 1].as_str())
}

pub fn bridge() -> BridgeCommand {
    BridgeCommand {
        program: std::env::current_exe().unwrap(),
    }
}

/// The shortest path, in bytes, of the temporary folder the tests run with.
pub const LONG_TMPDIR: usize = 200;

/// A new folder in `dir` whose path has at least `bytes` bytes.
pub fn long_folder(dir: &Path, bytes: usize) -> PathBuf {
    let used = dir.as_os_str().len() + 1;
    let folder = dir.join("t".repeat(bytes.saturating_sub(used).max(1)));
    std::fs::create_dir_all(&folder).expect("the long folder can be made");
    folder
}

pub fn scratch(name: &str) -> crate::scratch::Scratch {
    crate::scratch::Scratch::new(name)
}

pub fn claude(data: &Path, binary: &Path) -> ClaudeCode {
    ClaudeCode::new(
        ClaudeConfig {
            data_dir: data.to_path_buf(),
            binary: binary.into(),
            home: None,
            bridge: bridge(),
        },
        None,
    )
}

pub fn codex(data: &Path, binary: &Path, login: &Path, launch: Option<Launch>) -> CodexCli {
    CodexCli::new(
        CodexConfig {
            data_dir: data.to_path_buf(),
            binary: binary.into(),
            prefix: Vec::new(),
            home: Some(login.to_path_buf()),
            bridge: bridge(),
            launch,
        },
        Arc::new(mcbench_providers::catalog::CodexModels::default()),
    )
}

pub fn tool(file: &str) -> ToolSchema {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../agent/contract/tools")
        .join(format!("{file}.json"));
    // The contract's files hold the budgets as holes (⟦key⟧); both sides
    // fill them with the same number.
    let text = std::fs::read_to_string(path).unwrap();
    let mut out = String::new();
    let mut rest = text.as_str();
    while let Some(at) = rest.find('⟦') {
        out.push_str(&rest[..at]);
        let end = rest[at..].find('⟧').unwrap() + at + '⟧'.len_utf8();
        out.push_str("12345");
        rest = &rest[end..];
    }
    out.push_str(rest);
    ToolSchema(mcbench_json::stringify(&mcbench_json::parse(&out).unwrap()))
}

pub fn small_tool(name: &str) -> ToolSchema {
    ToolSchema(mcbench_json::stringify(
        &json!({ "type": "function", "function": { "name": name, "description": name, "parameters": { "type": "object" } } }),
    ))
}

pub fn entries(transcript: &Value) -> Vec<TranscriptEntry> {
    transcript
        .as_array()
        .unwrap()
        .iter()
        .map(|m| TranscriptEntry {
            id: m["id"].as_str().unwrap().to_owned(),
            role: m["role"].as_str().unwrap().to_owned(),
            see_build: m["seeBuild"].as_bool().unwrap_or(false),
            stopped: m["stopped"].as_bool().unwrap_or(false),
            reply: m["reply"].as_str().map(str::to_owned),
        })
        .collect()
}

pub struct Req {
    pub conversation: Option<(String, Option<Value>)>,
    pub messages: Value,
    pub tools: Vec<ToolSchema>,
    pub model: String,
    pub effort: Option<String>,
    pub chat: bool,
}

impl Req {
    pub fn new(messages: Value) -> Req {
        Req {
            conversation: None,
            messages,
            tools: vec![small_tool("build")],
            model: "claude-opus-5-5".to_owned(),
            effort: None,
            chat: false,
        }
    }

    pub fn chat(mut self, id: &str, transcript: Option<Value>) -> Req {
        self.conversation = Some((id.to_owned(), transcript));
        self
    }

    pub fn build(&self) -> CompletionRequest {
        CompletionRequest {
            conversation_id: self.conversation.as_ref().map(|(id, _)| id.clone()),
            model: self.model.clone(),
            effort: self.effort.clone(),
            messages: self
                .messages
                .as_array()
                .unwrap()
                .iter()
                .map(|m| ApiMessage(m.as_object().unwrap().clone()))
                .collect(),
            tools: self.tools.clone(),
            chat: self.chat,
            transcript: self
                .conversation
                .as_ref()
                .and_then(|(_, t)| t.as_ref().map(entries)),
        }
    }
}

/// Deltas a completion streamed, as today's `onDelta` objects.
#[derive(Clone, Default)]
pub struct Deltas(pub Arc<Mutex<Vec<Value>>>);

impl Deltas {
    pub fn take(&self) -> Vec<Value> {
        std::mem::take(&mut self.0.lock().unwrap())
    }
}

pub async fn complete_claude(
    provider: &ClaudeCode,
    req: &CompletionRequest,
    deltas: &Deltas,
    cancel: &CancellationToken,
) -> Result<Map<String, Value>, ProviderError> {
    let sink = deltas.0.clone();
    let mut on_delta = move |d: Delta| {
        sink.lock()
            .unwrap()
            .push(mcbench_providers::harness::delta_json(&d))
    };
    provider.complete_turn(req, &mut on_delta, cancel).await
}

pub async fn complete_codex(
    provider: &CodexCli,
    req: &CompletionRequest,
    deltas: &Deltas,
    cancel: &CancellationToken,
) -> Result<Map<String, Value>, ProviderError> {
    let sink = deltas.0.clone();
    let mut on_delta = move |d: Delta| {
        sink.lock()
            .unwrap()
            .push(mcbench_providers::harness::delta_json(&d))
    };
    provider.complete_turn(req, &mut on_delta, cancel).await
}

/// The message as JSON (`role`, `content`, the rest, `tool_calls`).
pub fn as_json(message: &Map<String, Value>) -> Value {
    mcbench_providers::harness::message_json(message)
}

pub fn digest(text: &str) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(text.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

pub async fn sleep(ms: u64) {
    tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
}

/// Waits until `done`, failing after a minute (a hang, not a slow machine).
pub async fn until(what: &str, done: impl Fn() -> bool) {
    let started = std::time::Instant::now();
    while !done() {
        assert!(
            started.elapsed() < std::time::Duration::from_secs(60),
            "{what}"
        );
        sleep(10).await;
    }
}
