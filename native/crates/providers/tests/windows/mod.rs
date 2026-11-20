//! Claude Code on Windows, end to end, against a stand-in written in Rust:
//! this test binary copied as `claude.exe` (it knows itself by that name)
//! speaks the stream-json protocol of `tests/fakes/fake_cli.py`'s Claude
//! mode, starts the bridge from its `--mcp-config` as the real CLI does, and
//! records what it was started with. The bridge is the real one: the
//! provider's named pipe, reached by this binary started as `<this test>
//! tool-bridge <pipe>` (`mcbench_relay::relay_main`, what `mcbench-lite
//! tool-bridge` runs). Every test's name starts with `windows_`.

use std::collections::BTreeMap;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use mcbench_providers::bridge::{BridgeAddress, relay_main};
use mcbench_providers::harness::claude_code::{ClaudeCode, ClaudeConfig, cwd};
use mcbench_providers::harness::codex_cli::CodexConfig;
use mcbench_providers::harness::install::{self, Lookup};
use mcbench_providers::harness::{BridgeCommand, Logins};
use mcbench_providers::login::{SignIns, State};
use mcbench_providers::{ApiMessage, CompletionRequest, Delta, ProviderId, ToolSchema};
use serde_json::{Map, Value, json};
use tokio_util::sync::CancellationToken;

use crate::scratch::Scratch;

mod codex;

type Test = (
    &'static str,
    fn() -> std::pin::Pin<Box<dyn Future<Output = ()> + Send>>,
);

macro_rules! tests {
    ($($name:ident),* $(,)?) => {
        vec![$((stringify!($name), (|| Box::pin($name()) as std::pin::Pin<Box<dyn Future<Output = ()> + Send>>) as fn() -> _)),*]
    };
}

pub fn main() {
    let args: Vec<String> = std::env::args().collect();
    let exe = std::env::current_exe().unwrap();
    if exe.file_stem().and_then(|s| s.to_str()) == Some("claude") {
        stand_in(&args[1..]);
    }
    match args.get(1).map(String::as_str) {
        Some("tool-bridge") => relay_main(&BridgeAddress::NamedPipe(args[2].clone())),
        Some("sleep-child") => loop {
            std::thread::sleep(Duration::from_secs(3600));
        },
        _ => {}
    }
    // Claude Code's account and working folder are under the profile: a
    // scratch one, set while this process has one thread.
    let profile = Scratch::new("windows-profile");
    // SAFETY: main has started no other thread yet, so nothing reads the
    // environment while it changes.
    unsafe {
        std::env::set_var("USERPROFILE", &*profile);
        std::env::remove_var("CLAUDE_CONFIG_DIR");
    }
    let filter = args.iter().skip(1).find(|a| !a.starts_with('-')).cloned();
    let all: Vec<Test> = tests![
        windows_a_turn_calls_one_mcbench_tool_through_the_named_pipe_with_every_platforms_arguments,
        windows_stopping_a_turn_ends_claude_code_and_every_child_of_it,
        windows_a_dropped_run_ends_claude_code_and_every_child_of_it,
        windows_the_sign_in_drives_claude_code_through_its_prompt_over_pipes,
        windows_a_signed_in_profile_reads_as_signed_in,
        windows_claude_exe_on_path_or_named_with_backslashes_is_found,
        windows_a_failure_dump_never_holds_the_login,
        windows_codex_wsl,
        windows_codex_wsl_package,
        windows_codex_wsl_proc,
        windows_codex_wsl_apply_patch,
        windows_codex_nowsl,
    ];
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .unwrap();
    let mut failed = Vec::new();
    let mut ran = 0;
    for (name, test) in all {
        // The Codex tests need WSL, or a Windows without it: each runs
        // only when named.
        let named_only = name.starts_with("windows_codex_");
        if filter
            .as_ref()
            .map_or(named_only, |f| !name.contains(f.as_str()))
        {
            continue;
        }
        ran += 1;
        match runtime.block_on(async { tokio::spawn(test()).await }) {
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
    drop(profile);
    println!(
        "\ncli: {} passed, {} failed",
        ran - failed.len(),
        failed.len()
    );
    if !failed.is_empty() {
        std::process::exit(1);
    }
}

// The stand-in.

fn record(dir: &Path, entry: Value) {
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("record.jsonl"))
        .unwrap();
    writeln!(file, "{entry}").unwrap();
}

fn emit(event: Value) {
    let mut out = std::io::stdout().lock();
    writeln!(out, "{event}").unwrap();
    out.flush().unwrap();
}

fn flag<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter()
        .position(|a| a == name)
        .map(|i| args[i + 1].as_str())
}

/// Claude Code's config folder as it finds it.
fn config_dir() -> PathBuf {
    std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("USERPROFILE").unwrap()))
}

/// One MCP client over the bridge's stdio, as Claude Code is one.
struct Mcp {
    input: std::process::ChildStdin,
    output: std::io::BufReader<std::process::ChildStdout>,
    next: u64,
}

impl Mcp {
    fn send(&mut self, method: &str, params: Value) -> Value {
        self.next += 1;
        let id = self.next;
        writeln!(
            self.input,
            "{}",
            json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })
        )
        .unwrap();
        self.input.flush().unwrap();
        loop {
            let mut line = String::new();
            assert!(
                self.output.read_line(&mut line).unwrap() > 0,
                "the bridge ended"
            );
            let reply: Value = serde_json::from_str(&line).unwrap();
            if reply["id"] == json!(id) {
                return reply;
            }
        }
    }

    fn notify(&mut self, method: &str) {
        writeln!(
            self.input,
            "{}",
            json!({ "jsonrpc": "2.0", "method": method, "params": {} })
        )
        .unwrap();
        self.input.flush().unwrap();
    }
}

fn stand_in(args: &[String]) -> ! {
    let dir = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let mode = std::fs::read_to_string(dir.join("mode")).unwrap_or_default();
    let env: BTreeMap<String, String> = std::env::vars().collect();
    record(
        &dir,
        json!({ "start": { "argv": args, "env": env, "cwd": std::env::current_dir().unwrap(), "pid": std::process::id() } }),
    );
    let mut stdin = std::io::stdin().lock();
    match args.first().map(String::as_str) {
        Some("--version") => {
            println!("2.1.311 (Claude Code)");
            std::process::exit(0)
        }
        Some("auth") if args.get(1).map(String::as_str) == Some("login") => {
            print!(
                "Opening browser to sign in…\nIf the browser didn't open, visit: https://claude.com/cai/oauth/authorize?code=true&state=stand-in\nPaste code here if prompted > "
            );
            std::io::stdout().flush().unwrap();
            let mut code = String::new();
            stdin.read_line(&mut code).unwrap();
            record(&dir, json!({ "code": code.trim_end() }));
            std::fs::write(
                config_dir().join(".claude.json"),
                json!({ "oauthAccount": { "emailAddress": "stand-in@example.test", "organizationType": "claude_max", "organizationRateLimitTier": "default_claude_max_20x" } }).to_string(),
            )
            .unwrap();
            println!("Login successful.");
            std::process::exit(0)
        }
        Some("auth") => {
            let _ = std::fs::remove_file(config_dir().join(".claude.json"));
            std::process::exit(0)
        }
        _ => {}
    }
    let mut line = String::new();
    stdin.read_line(&mut line).unwrap();
    record(&dir, json!({ "turn": line.trim_end() }));
    let server: Value = serde_json::from_str(flag(args, "--mcp-config").unwrap()).unwrap();
    let server = &server["mcpServers"]["mcbench"];
    let mut bridge = std::process::Command::new(server["command"].as_str().unwrap())
        .args(
            server["args"]
                .as_array()
                .unwrap()
                .iter()
                .map(|a| a.as_str().unwrap()),
        )
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut pids = vec![bridge.id()];
    if mode.trim() == "hang" {
        // Ended with the stand-in's tree, by the job object under test.
        #[allow(clippy::zombie_processes)]
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .arg("sleep-child")
            .spawn()
            .unwrap();
        pids.push(child.id());
    }
    record(&dir, json!({ "children": pids }));
    let mut mcp = Mcp {
        input: bridge.stdin.take().unwrap(),
        output: std::io::BufReader::new(bridge.stdout.take().unwrap()),
        next: 0,
    };
    mcp.send(
        "initialize",
        json!({ "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "stand-in-claude", "version": "1" } }),
    );
    mcp.notify("notifications/initialized");
    mcp.send("tools/list", json!({}));
    emit(
        json!({ "type": "system", "subtype": "init", "tools": ["mcp__mcbench__build"], "mcp_servers": [{ "name": "mcbench", "status": "connected" }], "session_id": "sess-1" }),
    );
    if mode.trim() == "hang" {
        loop {
            std::thread::sleep(Duration::from_secs(3600));
        }
    }
    let input = json!({ "summary": "s", "program": "p" });
    emit(json!({ "type": "stream_event", "event": { "type": "message_start", "message": {} } }));
    emit(
        json!({ "type": "stream_event", "event": { "type": "content_block_start", "index": 1, "content_block": { "type": "tool_use", "id": "toolu_1", "name": "mcp__mcbench__build", "input": {} } } }),
    );
    emit(
        json!({ "type": "stream_event", "event": { "type": "content_block_delta", "index": 1, "delta": { "type": "input_json_delta", "partial_json": "{\"summary\":\"s\",\"program\":\"p\"}" } } }),
    );
    emit(
        json!({ "type": "assistant", "message": { "content": [{ "type": "tool_use", "id": "toolu_1", "name": "mcp__mcbench__build", "input": input }], "usage": { "input_tokens": 4, "cache_creation_input_tokens": 7264, "cache_read_input_tokens": 7147, "output_tokens": 60 } } }),
    );
    let result = mcp.send(
        "tools/call",
        json!({ "name": "build", "arguments": input, "_meta": { "claudecode/toolUseId": "toolu_1" } }),
    );
    let text = format!(
        "Built: {}",
        result["result"]["content"][0]["text"].as_str().unwrap()
    );
    emit(
        json!({ "type": "stream_event", "event": { "type": "content_block_delta", "index": 9, "delta": { "type": "text_delta", "text": text } } }),
    );
    emit(
        json!({ "type": "assistant", "message": { "content": [{ "type": "text", "text": "Built" }], "usage": { "input_tokens": 2, "cache_creation_input_tokens": 117, "cache_read_input_tokens": 7264, "output_tokens": 48 } } }),
    );
    emit(
        json!({ "type": "result", "subtype": "success", "is_error": false, "session_id": "sess-1", "result": "Built", "total_cost_usd": 0.01, "usage": { "input_tokens": 6, "cache_creation_input_tokens": 7381, "cache_read_input_tokens": 14411, "output_tokens": 108, "output_tokens_details": { "thinking_tokens": 1 }, "iterations": [] } }),
    );
    // As Claude Code does, the process waits for the next turn; the app
    // closes its stdin when it is done with it.
    let mut next = String::new();
    let _ = stdin.read_line(&mut next);
    std::process::exit(0)
}

// The tests' helpers.

/// The stand-in in its own folder: `<dir>/claude.exe`, in `mode`.
fn stand_in_at(dir: &Path, mode: &str) -> PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    let program = dir.join("claude.exe");
    std::fs::copy(std::env::current_exe().unwrap(), &program).unwrap();
    std::fs::write(dir.join("mode"), mode).unwrap();
    program
}

/// The stand-in's records written so far: complete lines only, since the
/// stand-in may be in the middle of appending one while a test reads (the
/// part after the last newline is a line still being written).
fn records(dir: &Path) -> Vec<Value> {
    let text = std::fs::read_to_string(dir.join("record.jsonl")).unwrap_or_default();
    let complete = &text[..text.rfind('\n').map_or(0, |end| end + 1)];
    complete
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

fn config(data: &Path, binary: &Path) -> ClaudeConfig {
    ClaudeConfig {
        data_dir: data.to_path_buf(),
        binary: binary.into(),
        home: None,
        bridge: BridgeCommand {
            program: std::env::current_exe().unwrap(),
        },
    }
}

fn request(messages: Value) -> CompletionRequest {
    CompletionRequest {
        conversation_id: None,
        model: "claude-opus-5-5".to_owned(),
        effort: None,
        messages: messages
            .as_array()
            .unwrap()
            .iter()
            .map(|m| ApiMessage(m.as_object().unwrap().clone()))
            .collect(),
        tools: vec![ToolSchema(
            json!({ "type": "function", "function": { "name": "build", "description": "build", "parameters": { "type": "object" } } }).to_string(),
        )],
        chat: false,
        transcript: None,
    }
}

async fn complete(
    provider: &ClaudeCode,
    request: &CompletionRequest,
    cancel: &CancellationToken,
) -> Result<Map<String, Value>, mcbench_providers::ProviderError> {
    let mut on_delta = |_: Delta| {};
    provider.complete_turn(request, &mut on_delta, cancel).await
}

/// Waits until `done`, failing after a minute (a hang, not a slow machine).
async fn until(what: &str, done: impl Fn() -> bool) {
    let started = std::time::Instant::now();
    while !done() {
        assert!(started.elapsed() < Duration::from_secs(60), "{what}");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// Whether the process `pid` still runs.
fn running(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, WAIT_TIMEOUT};
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject,
    };
    // SAFETY: OpenProcess takes no pointer; a null answer is a process
    // that is gone (or never was).
    let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
    if handle.is_null() {
        return false;
    }
    // SAFETY: `handle` was just opened and is closed right after.
    let waited = unsafe { WaitForSingleObject(handle, 0) };
    // SAFETY: as above.
    unsafe { CloseHandle(handle) };
    waited == WAIT_TIMEOUT
}

/// The stand-in's pid and its children's, once it recorded them.
async fn tree_of(dir: &Path) -> Vec<u32> {
    until("the stand-in recorded its children", || {
        records(dir).iter().any(|r| r.get("children").is_some())
    })
    .await;
    let records = records(dir);
    let mut pids = vec![records[0]["start"]["pid"].as_u64().unwrap() as u32];
    let children = records.iter().find_map(|r| r.get("children")).unwrap();
    pids.extend(
        children
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p.as_u64().unwrap() as u32),
    );
    pids
}

// The tests.

async fn windows_a_failure_dump_never_holds_the_login() {
    crate::dump::a_failure_dump_never_holds_the_login().await
}

async fn windows_codex_wsl() {
    codex::windows_wsl_codex_turns_in_linuxs_box_reach_mcbench_and_only_the_login().await
}

async fn windows_codex_wsl_package() {
    codex::windows_wsl_codex_the_pinned_package_installs_and_runs_from_the_tools_folder().await
}

async fn windows_codex_wsl_apply_patch() {
    codex::windows_wsl_codex_applies_a_patch_in_its_box_from_its_execute_only_install().await
}

async fn windows_codex_wsl_proc() {
    codex::windows_wsl_codex_a_command_in_its_box_reaches_nothing_of_codex_or_the_app_through_proc()
        .await
}

async fn windows_codex_nowsl() {
    codex::windows_nowsl_a_codex_turn_without_wsl_says_wsl_is_needed().await
}

async fn windows_a_turn_calls_one_mcbench_tool_through_the_named_pipe_with_every_platforms_arguments()
 {
    let dir = Scratch::new("windows-turn");
    let program = stand_in_at(&dir.join("cli"), "turn");
    let provider = ClaudeCode::new(config(&dir.join("data"), &program), None);
    let messages = json!([{ "role": "system", "content": "You build." }, { "role": "user", "content": "Build a hut." }]);
    let cancel = CancellationToken::new();
    // A chat's turn, as the app sends one: the process stays for the call's
    // answer.
    let chat = |messages: Value| CompletionRequest {
        conversation_id: Some("c1".to_owned()),
        transcript: Some(vec![mcbench_providers::harness::TranscriptEntry {
            id: "u1".to_owned(),
            role: "user".to_owned(),
            see_build: false,
            stopped: false,
            reply: None,
        }]),
        ..request(messages)
    };
    let first = complete(&provider, &chat(messages.clone()), &cancel)
        .await
        .unwrap();
    let call = mcbench_providers::harness::message_json(&first);
    assert_eq!(call["tool_calls"][0]["function"]["name"], "build", "{call}");
    let mut next = messages.as_array().unwrap().clone();
    next.push(json!({ "role": "assistant", "content": call["content"], "tool_calls": call["tool_calls"] }));
    next.push(
        json!({ "role": "tool", "tool_call_id": call["tool_calls"][0]["id"], "content": "ok" }),
    );
    let reply = complete(&provider, &chat(Value::Array(next)), &cancel)
        .await
        .unwrap();
    let reply = mcbench_providers::harness::message_json(&reply);
    assert!(
        reply["content"].to_string().contains("Built: ok"),
        "{reply}"
    );
    provider.shutdown().await;
    let start = &records(&dir.join("cli"))[0]["start"];
    let argv: Vec<String> = serde_json::from_value(start["argv"].clone()).unwrap();
    // The arguments every platform passes; only the bridge's address is
    // Windows' own, a named pipe.
    assert_eq!(
        &argv[..5],
        [
            "-p",
            "--output-format",
            "stream-json",
            "--verbose",
            "--model"
        ]
    );
    assert_eq!(flag(&argv, "--tools"), Some(""));
    assert!(argv.contains(&"--strict-mcp-config".to_owned()));
    assert_eq!(flag(&argv, "--setting-sources"), Some(""));
    assert_eq!(flag(&argv, "--allowedTools"), Some("mcp__mcbench__build"));
    let mcp: Value = serde_json::from_str(flag(&argv, "--mcp-config").unwrap()).unwrap();
    let server = &mcp["mcpServers"]["mcbench"];
    assert_eq!(server["type"], "stdio");
    assert_eq!(
        server["command"],
        std::env::current_exe().unwrap().to_string_lossy().as_ref()
    );
    assert_eq!(server["args"][0], "tool-bridge");
    assert!(
        server["args"][1]
            .as_str()
            .unwrap()
            .starts_with(r"\\.\pipe\mcbench-"),
        "{server}"
    );
    assert_eq!(
        mcbench_providers::harness::claude_code::claude_args(
            "claude-opus-5-5",
            None,
            flag(
                &argv,
                if argv.contains(&"--system-prompt".to_owned()) {
                    "--system-prompt"
                } else {
                    "--append-system-prompt"
                }
            )
            .unwrap(),
            &["mcp__mcbench__build".to_owned()],
            (
                server["command"].as_str().unwrap(),
                &[
                    "tool-bridge".to_owned(),
                    server["args"][1].as_str().unwrap().to_owned()
                ]
            ),
            flag(&argv, "--session-id").map(|id| (id, false)),
            argv.contains(&"--system-prompt".to_owned()),
            &[],
        ),
        argv
    );
    // The environment: Claude Code's own variables as on every platform,
    // and nothing of this process's but what a CLI needs.
    let env: BTreeMap<String, String> = serde_json::from_value(start["env"].clone()).unwrap();
    for (key, value) in [
        ("MCP_TOOL_TIMEOUT", "2147483647"),
        ("MCP_TIMEOUT", "120000"),
        ("MAX_MCP_OUTPUT_TOKENS", "200000"),
        ("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC", "1"),
        ("DISABLE_AUTOUPDATER", "1"),
        ("CLAUDE_CODE_ENABLE_FINE_GRAINED_TOOL_STREAMING", "1"),
        ("CLAUDE_CODE_ENTRYPOINT", "mcbench-lite"),
        ("CLAUDE_CODE_DISABLE_AUTO_MEMORY", "1"),
    ] {
        assert_eq!(env.get(key).map(String::as_str), Some(value), "{key}");
    }
    let ours = config(&dir, &program).env();
    let keys: Vec<String> = ours
        .iter()
        .map(|(k, _)| k.to_string_lossy().into_owned())
        .collect();
    for key in env.keys() {
        assert!(
            keys.iter().any(|k| k.eq_ignore_ascii_case(key)),
            "{key} came from elsewhere"
        );
    }
    assert_eq!(
        env.get("USERPROFILE").map(PathBuf::from),
        std::env::var_os("USERPROFILE").map(PathBuf::from)
    );
    assert_eq!(PathBuf::from(start["cwd"].as_str().unwrap()), cwd());
    assert!(cwd().starts_with(std::env::var_os("USERPROFILE").unwrap()));
}

async fn windows_stopping_a_turn_ends_claude_code_and_every_child_of_it() {
    let dir = Scratch::new("windows-stop");
    let program = stand_in_at(&dir.join("cli"), "hang");
    let provider = Arc::new(ClaudeCode::new(config(&dir.join("data"), &program), None));
    let cancel = CancellationToken::new();
    let turn = {
        let provider = provider.clone();
        let cancel = cancel.clone();
        tokio::spawn(async move {
            let messages = json!([{ "role": "user", "content": "Build a hut." }]);
            complete(&provider, &request(messages), &cancel).await
        })
    };
    let pids = tree_of(&dir.join("cli")).await;
    assert_eq!(pids.len(), 3, "the CLI, its bridge and its other child");
    assert!(pids.iter().all(|p| running(*p)));
    cancel.cancel();
    let _ = turn.await.unwrap();
    until("the stand-in's whole tree has ended", || {
        pids.iter().all(|p| !running(*p))
    })
    .await;
    println!("  stopped: pids {pids:?} all ended");
    provider.shutdown().await;
}

async fn windows_a_dropped_run_ends_claude_code_and_every_child_of_it() {
    let dir = Scratch::new("windows-drop");
    let program = stand_in_at(&dir.join("cli"), "hang");
    let data = dir.join("data");
    // A runtime of its own, dropped with the run's task in it: the app's
    // end, however it ends, drops (or loses) the job's handle the same way.
    let pids = std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        let cli = program.parent().unwrap().to_path_buf();
        let pids = runtime.block_on(async move {
            let provider = Arc::new(ClaudeCode::new(config(&data, &program), None));
            let turn = provider.clone();
            tokio::spawn(async move {
                let messages = json!([{ "role": "user", "content": "Build a hut." }]);
                let _ = complete(&turn, &request(messages), &CancellationToken::new()).await;
            });
            let pids = tree_of(&cli).await;
            assert!(pids.iter().all(|p| running(*p)));
            std::mem::forget(provider);
            pids
        });
        drop(runtime);
        pids
    })
    .join()
    .unwrap();
    until("the stand-in's whole tree has ended", || {
        pids.iter().all(|p| !running(*p))
    })
    .await;
    println!("  dropped: pids {pids:?} all ended");
}

fn logins(binary: &Path) -> Logins {
    Logins {
        claude: config(Path::new("C:\\unused"), binary),
        codex: CodexConfig {
            data_dir: PathBuf::from("C:\\unused"),
            binary: "codex".into(),
            prefix: Vec::new(),
            home: None,
            bridge: BridgeCommand {
                program: std::env::current_exe().unwrap(),
            },
            launch: None,
        },
    }
}

async fn windows_the_sign_in_drives_claude_code_through_its_prompt_over_pipes() {
    let dir = Scratch::new("windows-sign-in");
    let program = stand_in_at(&dir.join("cli"), "turn");
    let config_file = PathBuf::from(std::env::var_os("USERPROFILE").unwrap()).join(".claude.json");
    let _ = std::fs::remove_file(&config_file);
    let sign_ins = SignIns::new(Arc::new(logins(&program)));
    sign_ins
        .start(ProviderId::ClaudeCode, Some("claudeai"))
        .unwrap();
    until(
        "the sign-in shows its address and asks for the code",
        || {
            sign_ins
                .state(ProviderId::ClaudeCode)
                .unwrap()
                .is_some_and(|v| v.state == State::Waiting && v.wants_input)
        },
    )
    .await;
    let view = sign_ins.state(ProviderId::ClaudeCode).unwrap().unwrap();
    assert_eq!(
        view.url.as_deref(),
        Some("https://claude.com/cai/oauth/authorize?code=true&state=stand-in")
    );
    sign_ins
        .input(ProviderId::ClaudeCode, "code#state")
        .await
        .unwrap();
    until("the sign-in is done", || {
        sign_ins
            .state(ProviderId::ClaudeCode)
            .unwrap()
            .is_some_and(|v| v.state == State::Done)
    })
    .await;
    let records = records(&dir.join("cli"));
    assert_eq!(
        records[0]["start"]["argv"],
        json!(["auth", "login", "--claudeai"])
    );
    assert_eq!(records[0]["start"]["env"]["BROWSER"], "true");
    assert_eq!(records[1]["code"], "code#state");
    assert!(config_file.is_file());
    sign_ins.logout(ProviderId::ClaudeCode).await.unwrap();
    assert!(!config_file.exists());
}

async fn windows_a_signed_in_profile_reads_as_signed_in() {
    let profile = PathBuf::from(std::env::var_os("USERPROFILE").unwrap());
    let config = config(Path::new("C:\\unused"), Path::new("claude"));
    let _ = std::fs::remove_file(profile.join(".claude.json"));
    assert!(!config.stored_login().unwrap());
    // What `claude auth login` leaves in the profile of a person signed in
    // on Windows (a stand-in's values).
    std::fs::write(
        profile.join(".claude.json"),
        json!({ "numStartups": 3, "oauthAccount": { "emailAddress": "person@example.test", "organizationType": "claude_max", "organizationRateLimitTier": "default_claude_max_20x" } }).to_string(),
    )
    .unwrap();
    assert_eq!(config.config_dir(), profile);
    assert!(config.stored_login().unwrap());
    let account = config.account().unwrap().unwrap();
    assert_eq!(account["email"], "person@example.test");
    assert_eq!(account["plan"], "Claude Max 20x");
    std::fs::remove_file(profile.join(".claude.json")).unwrap();
}

async fn windows_claude_exe_on_path_or_named_with_backslashes_is_found() {
    let dir = Scratch::new("windows-path");
    let program = stand_in_at(&dir.join("bin"), "turn");
    let pin = install::pins()
        .into_iter()
        .find(|p| p.name == "claude")
        .unwrap();
    let lookup = Lookup {
        path: std::env::join_paths([dir.join("bin")]).unwrap(),
        claude_binary: None,
        codex_binary: None,
    };
    assert!(lookup.on_path("claude"));
    assert!(install::cli_installed(&pin, &dir, "win32-x64", &lookup));
    let named = Lookup {
        path: Default::default(),
        claude_binary: Some(program.to_string_lossy().into_owned()),
        codex_binary: None,
    };
    assert!(install::cli_installed(&pin, &dir, "win32-x64", &named));
    let (folder, installed) = pin.installed(&dir, "win32-x64");
    assert_eq!(installed, folder.join("claude.exe"));
}
