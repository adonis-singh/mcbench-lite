//! The Codex CLI on Windows: a turn in Linux's box inside the app's WSL
//! distro, against the scripted fake Codex (`tests/fakes/fake_cli.py`, run
//! by the box's own python3) installed as the tools folder's Linux Codex.
//! `windows_wsl_*` needs WSL and the shell's files (`MCBENCH_SANDBOX_ROOTFS`,
//! `MCBENCH_SANDBOX_GUEST`, their `_SHA256`, `MCBENCH_WSL_DISTRO` naming a
//! test distro); `windows_nowsl_*` needs a Windows without WSL. Both run only
//! when named, and fail, never skip, when what they need is missing.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use mcbench_providers::harness::BridgeCommand;
use mcbench_providers::harness::codex_cli::{CodexCli, CodexConfig, Launch, WslBox};
use mcbench_providers::{ApiMessage, CompletionRequest, Delta};
use mcbench_sandbox::wsl_codex::{self, AGENT, TOOLS};
use serde_json::{Map, Value, json};
use tokio_util::sync::CancellationToken;

use crate::scratch::Scratch;

fn var(key: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| panic!("{key} is needed"))
}

/// The box as the app's headless crate makes it (`codex_wsl`), over this
/// test's folders.
fn wsl_box(data: &Path, tools: &Path, login: &Path, sha256: String) -> (WslBox, Vec<String>) {
    let windows = |p: &Path| mcbench_sandbox::windows::drvfs_path(p).unwrap();
    let harness = data.join("harness");
    for folder in [login, tools, &harness] {
        std::fs::create_dir_all(folder).unwrap();
    }
    let (uid, gid) = mcbench_sandbox::windows::GUEST_USER;
    let mut agent: Vec<String> = [
        "--distribution",
        &mcbench_sandbox::windows::wsl::distro(),
        "--user",
        "root",
        "--cd",
        "/",
        "--exec",
        AGENT,
        "codex",
    ]
    .map(String::from)
    .into();
    agent.extend([
        windows(login),
        windows(tools),
        windows(&harness),
        format!("{uid}:{gid}"),
    ]);
    let folder = format!("{TOOLS}/codex-0.169.0");
    let mut prefix = agent.clone();
    prefix.extend(["cli".to_owned(), format!("{folder}/bin/codex")]);
    let wsl = WslBox {
        program: mcbench_sandbox::windows::wsl::program().unwrap().into(),
        agent,
        harness,
        install: (folder, "/opt/codex/bin/codex".to_owned()),
        ids: (uid, gid),
        rootfs: Arc::new(move || {
            wsl_codex::ready()?;
            Ok(format!("/var/mcbench/.rootfs-{sha256}/rootfs"))
        }),
    };
    (wsl, prefix)
}

fn provider(data: &Path, login: &Path, wsl: WslBox, prefix: Vec<String>) -> CodexCli {
    CodexCli::new(
        CodexConfig {
            data_dir: data.to_path_buf(),
            binary: wsl.program.clone(),
            prefix,
            home: Some(login.to_path_buf()),
            bridge: BridgeCommand {
                program: std::env::current_exe().unwrap(),
            },
            launch: Some(Launch::Wsl(wsl)),
        },
        Arc::new(mcbench_providers::catalog::CodexModels::default()),
    )
}

fn request(chat: &str, messages: Value) -> CompletionRequest {
    CompletionRequest {
        conversation_id: Some(chat.to_owned()),
        model: "gpt-6-astra".to_owned(),
        effort: None,
        messages: messages
            .as_array()
            .unwrap()
            .iter()
            .map(|m| ApiMessage(m.as_object().unwrap().clone()))
            .collect(),
        tools: vec![mcbench_providers::ToolSchema(
            json!({ "type": "function", "function": { "name": "build", "description": "build", "parameters": { "type": "object" } } }).to_string(),
        )],
        chat: false,
        transcript: None,
    }
}

async fn complete(
    provider: &CodexCli,
    request: &CompletionRequest,
) -> Result<Map<String, Value>, mcbench_providers::ProviderError> {
    let mut on_delta = |_: Delta| {};
    provider
        .complete_turn(request, &mut on_delta, &CancellationToken::new())
        .await
}

/// The fake Codex as the tools folder's Linux install.
fn fake_install(tools: &Path, script: &Value) {
    let install = tools.join("codex-0.169.0");
    let bin = install.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::write(
        bin.join("codex"),
        include_str!("../fakes/fake_cli.py").replace("\r\n", "\n"),
    )
    .unwrap();
    std::fs::write(bin.join("script.json"), script.to_string()).unwrap();
    std::fs::write(
        install.join("codex-package.json"),
        r#"{"entrypoint":"bin/codex"}"#,
    )
    .unwrap();
}

/// Everything a failed turn's box left in the chat's folder (the proxy's
/// log, the fake's record of what it did, CODEX_HOME's names), printed with
/// the turn's error (its stderr: the agent's and the box's own words).
fn left_by_the_box(data: &Path, error: &str) -> ! {
    let chat = mcbench_providers::harness::conversation_dir(data, "c1");
    println!("{}", crate::dump::of_chat(&chat));
    panic!("{error}")
}

pub async fn windows_wsl_codex_turns_in_linuxs_box_reach_mcbench_and_only_the_login() {
    let scratch = Scratch::new("windows-wsl-codex");
    // The distro, its agent and the unpacked root filesystem, as setup's
    // shell step leaves them.
    let sandbox_root = scratch.join("sandbox");
    std::fs::create_dir_all(&sandbox_root).unwrap();
    let sha256 = var("MCBENCH_SANDBOX_ROOTFS_SHA256");
    // The distro is the test's (MCBENCH_WSL_DISTRO), whose disk is in the
    // sandbox folder MCBENCH_SANDBOX_SCRATCH names (check.sh's WSL step
    // imports it there): a distro of that name with its disk elsewhere is
    // not the app's, and the app does not touch it.
    let distro = mcbench_sandbox::windows::WslDistro::start(mcbench_sandbox::Config {
        root: PathBuf::from(var("MCBENCH_SANDBOX_SCRATCH")),
        rootfs: (PathBuf::from(var("MCBENCH_SANDBOX_ROOTFS")), sha256.clone()),
        helper: Vec::new(),
        budget: Arc::new(mcbench_sandbox::testing::TestBudget::new(
            sandbox_root,
            1 << 30,
        )),
        guest: mcbench_sandbox::GuestBase {
            asset: PathBuf::from(var("MCBENCH_SANDBOX_GUEST")),
            sha256: var("MCBENCH_SANDBOX_GUEST_SHA256"),
        },
    });
    mcbench_sandbox::Sandbox::prepare(&distro).await.unwrap();
    // The person's Windows Codex login (a stand-in's) and another file of
    // their Codex folder, which the box must not see.
    let login = scratch.join("profile").join(".codex");
    std::fs::create_dir_all(&login).unwrap();
    std::fs::write(login.join("auth.json"), r#"{"tokens":"before"}"#).unwrap();
    std::fs::write(login.join("history.jsonl"), "the person's own history").unwrap();
    let tools = scratch.join("tools");
    let data = scratch.join("data");
    let item = |key: &str| json!({ "id": format!("item_{key}"), "type": "mcp_tool_call", "server": "mcbench", "tool": "build", "arguments": {} });
    let steps = json!([
        { "emit": { "type": "thread.started", "thread_id": "01a1f500-0000-7000-8000-000000000009" } },
        { "emit": { "type": "item.started", "item": item("r1") } },
        { "call": { "name": "build", "arguments": { "summary": "s" } }, "as": "r1" },
        { "emit": { "type": "item.completed", "item": item("r1") } },
        { "reply": "r1", "prefix": "Built: ", "codex": true },
        { "refresh": "{\"tokens\":\"refreshed in the box\"}" },
        { "mounts": true },
        { "probe": ["chatgpt.com", "example.com"] },
    ]);
    fake_install(
        &tools,
        &json!({ "mode": "codex", "spawns": [{ "mcp": true, "turns": [steps] }] }),
    );
    let (wsl, prefix) = wsl_box(&data, &tools, &login, sha256);
    let provider = provider(&data, &login, wsl, prefix);
    let messages = json!([{ "role": "user", "content": "Build a hut." }]);
    let first = mcbench_providers::harness::message_json(
        &complete(&provider, &request("c1", messages.clone()))
            .await
            .unwrap_or_else(|e| left_by_the_box(&data, &e.message)),
    );
    assert_eq!(
        first["tool_calls"][0]["function"]["name"], "build",
        "{first}"
    );
    let mut next = messages.as_array().unwrap().clone();
    next.push(json!({ "role": "assistant", "content": "", "tool_calls": first["tool_calls"] }));
    next.push(json!({ "role": "tool", "tool_call_id": first["tool_calls"][0]["id"], "content": "built in the box" }));
    let last = mcbench_providers::harness::message_json(
        &complete(&provider, &request("c1", Value::Array(next)))
            .await
            .unwrap_or_else(|e| left_by_the_box(&data, &e.message)),
    );
    let text = last["content"].as_str().unwrap().to_owned();
    println!("  inside the box:\n{text}");
    let lines: Vec<&str> = text.lines().filter(|l| !l.is_empty()).collect();
    assert_eq!(
        lines[0], "Built: built in the box",
        "the tool's answer crossed the relay"
    );
    // The box's mounts: nothing of Windows but the login file and the
    // chat's own folders; nothing else of the person's Codex folder.
    let mounts: Vec<&str> = lines
        .iter()
        .filter_map(|l| l.strip_prefix("mount "))
        .collect();
    let home: Vec<&str> = lines
        .iter()
        .filter_map(|l| l.strip_prefix("home "))
        .collect();
    assert!(mounts.contains(&"/codex-home/auth.json"), "{mounts:?}");
    assert!(!mounts.iter().any(|m| m.starts_with("/mnt")), "{mounts:?}");
    assert!(
        home.contains(&"auth.json") && !home.contains(&"history.jsonl"),
        "{home:?}"
    );
    assert!(lines.contains(&"chatgpt.com HTTP/1.1 200 Connection Established"));
    assert!(lines.contains(&"example.com HTTP/1.1 403 Forbidden"));
    // The refresh Codex wrote in the box is in the person's own Windows file.
    // Compared, never printed: the login's contents stay out of every log.
    assert!(
        std::fs::read_to_string(login.join("auth.json")).unwrap()
            == r#"{"tokens":"refreshed in the box"}"#,
        "the Windows auth.json is not the box's refresh"
    );
    assert!(
        std::fs::read_to_string(login.join("history.jsonl")).unwrap() == "the person's own history",
        "the person's other file changed"
    );
    // Codex outside the box (`cli`) is the pinned package's test: this fake
    // is a python3 script, and python3 is in the box's root filesystem, not
    // in the distro's own.
    mcbench_sandbox::Sandbox::shutdown(&distro).await;
}

/// `codex_proc`'s check in Linux's box inside the app's WSL distro, as on
/// Linux. The app's memory is not measured here.
/// The Linux test `the_pinned_codex_applies_a_patch_in_its_box_from_its_execute_only_install`
/// (`harness/install/tests.rs`) in the distro's box: the real pinned Codex
/// installed through the pin into the tools folder, whose programs the
/// agent makes execute-only for the box (drvfs metadata), runs its
/// apply_patch entry point (`--codex-run-as-apply-patch <patch>`, Codex
/// 0.169.0's `codex-rs/arg0/src/lib.rs`) against a file of the chat's
/// workspace; the file's new contents are read on Windows. The patch is
/// read from a file in the box (`$(cat ...)`), so its line ends never cross
/// wsl.exe's command line; `exec` makes Codex the shell's own process.
pub async fn windows_wsl_codex_applies_a_patch_in_its_box_from_its_execute_only_install() {
    use mcbench_providers::harness::install;
    const PATCH: &str = "*** Begin Patch\n*** Update File: hello.txt\n@@\n-hello\n+hello from the box\n*** End Patch";
    let scratch = Scratch::new("windows-wsl-codex-apply-patch");
    let sandbox_root = scratch.join("sandbox");
    std::fs::create_dir_all(&sandbox_root).unwrap();
    let sha256 = var("MCBENCH_SANDBOX_ROOTFS_SHA256");
    // The test's distro, as in the /proc test above.
    let distro = mcbench_sandbox::windows::WslDistro::start(mcbench_sandbox::Config {
        root: PathBuf::from(var("MCBENCH_SANDBOX_SCRATCH")),
        rootfs: (PathBuf::from(var("MCBENCH_SANDBOX_ROOTFS")), sha256.clone()),
        helper: Vec::new(),
        budget: Arc::new(mcbench_sandbox::testing::TestBudget::new(
            sandbox_root,
            1 << 30,
        )),
        guest: mcbench_sandbox::GuestBase {
            asset: PathBuf::from(var("MCBENCH_SANDBOX_GUEST")),
            sha256: var("MCBENCH_SANDBOX_GUEST_SHA256"),
        },
    });
    mcbench_sandbox::Sandbox::prepare(&distro).await.unwrap();
    let tools = scratch.join("tools");
    let login = scratch.join("profile").join(".codex");
    let data = scratch.join("data");
    let mut pin = install::pins()
        .into_iter()
        .find(|p| p.name == "codex")
        .unwrap();
    let platform = install::platform_of("codex", &install::platform());
    for (key, asset) in &mut pin.platforms {
        if *key == platform {
            asset.url = serve_once(PathBuf::from(var("MCBENCH_CODEX_PACKAGE")));
        }
    }
    install::install(&pin, &tools, &platform, &|_: install::Progress| {})
        .await
        .unwrap();
    let (wsl, _) = wsl_box(&data, &tools, &login, sha256);
    std::fs::write(login.join("auth.json"), "{}").unwrap();
    let chat = wsl.harness.join("c1");
    for folder in ["codex-home", "workspace", "images-applypatch"] {
        std::fs::create_dir_all(chat.join(folder)).unwrap();
    }
    std::fs::write(chat.join("workspace").join("hello.txt"), "hello\n").unwrap();
    std::fs::write(chat.join("workspace").join("patch.txt"), PATCH).unwrap();
    let in_distro = |folder: &str| {
        PathBuf::from(format!(
            "{}/c1/{folder}",
            mcbench_sandbox::wsl_codex::HARNESS
        ))
    };
    let run = "applypatch";
    let spec = mcbench_sandbox::proxy::CodexRun {
        rootfs: PathBuf::from((wsl.rootfs)().unwrap()),
        image_env: Vec::new(),
        relay: PathBuf::from(AGENT),
        install: PathBuf::from(&wsl.install.0),
        home: in_distro("codex-home"),
        auth_file: PathBuf::from(format!("{}/auth.json", wsl_codex::LOGIN)),
        workspace: in_distro("workspace"),
        sock_dir: PathBuf::from(format!("{}/{run}/sock", wsl_codex::RUNS)),
        allowed_hosts: Vec::new(),
        proxy_log: in_distro("egress.ndjson"),
        command: [
            "sh",
            "-c",
            "stat -c 'mode %a' /opt/codex/bin/codex; exec /opt/codex/bin/codex --codex-run-as-apply-patch \"$(cat /workspace/patch.txt)\"",
        ]
        .map(String::from)
        .into(),
    };
    let out = std::process::Command::new(&wsl.program)
        .args(mcbench_providers::harness::codex_cli::wsl_box_args(
            &wsl,
            run,
            &in_distro("images-applypatch"),
            &spec,
        ))
        .env("WSL_UTF8", "1")
        .env_remove("WSLENV")
        .output()
        .unwrap();
    let said = format!(
        "exit {:?}; stdout: {}; stderr: {}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout).trim(),
        String::from_utf8_lossy(&out.stderr).trim()
    );
    println!("  codex --codex-run-as-apply-patch in the distro's box: {said}");
    mcbench_sandbox::Sandbox::shutdown(&distro).await;
    assert_eq!(
        std::fs::read_to_string(chat.join("workspace").join("hello.txt")).unwrap(),
        "hello from the box\n",
        "the patched file on Windows; Codex said {said}"
    );
    assert!(
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .next()
            .is_some_and(|l| l == "mode 111"),
        "the box's Codex is execute-only; {said}"
    );
    assert_eq!(out.status.code(), Some(0), "Codex said {said}");
}

pub async fn windows_wsl_codex_a_command_in_its_box_reaches_nothing_of_codex_or_the_app_through_proc()
 {
    let scratch = Scratch::new("windows-wsl-codex-proc");
    let sandbox_root = scratch.join("sandbox");
    std::fs::create_dir_all(&sandbox_root).unwrap();
    let sha256 = var("MCBENCH_SANDBOX_ROOTFS_SHA256");
    // The distro is the test's (MCBENCH_WSL_DISTRO), whose disk is in the
    // sandbox folder MCBENCH_SANDBOX_SCRATCH names (check.sh's WSL step
    // imports it there): a distro of that name with its disk elsewhere is
    // not the app's, and the app does not touch it.
    let distro = mcbench_sandbox::windows::WslDistro::start(mcbench_sandbox::Config {
        root: PathBuf::from(var("MCBENCH_SANDBOX_SCRATCH")),
        rootfs: (PathBuf::from(var("MCBENCH_SANDBOX_ROOTFS")), sha256.clone()),
        helper: Vec::new(),
        budget: Arc::new(mcbench_sandbox::testing::TestBudget::new(
            sandbox_root,
            1 << 30,
        )),
        guest: mcbench_sandbox::GuestBase {
            asset: PathBuf::from(var("MCBENCH_SANDBOX_GUEST")),
            sha256: var("MCBENCH_SANDBOX_GUEST_SHA256"),
        },
    });
    mcbench_sandbox::Sandbox::prepare(&distro).await.unwrap();
    let login = scratch.join("profile").join(".codex");
    std::fs::create_dir_all(&login).unwrap();
    std::fs::write(login.join("auth.json"), "{}").unwrap();
    let tools = scratch.join("tools");
    let data = scratch.join("data");
    fake_install(&tools, &crate::codex_proc::script());
    let (wsl, prefix) = wsl_box(&data, &tools, &login, sha256);
    let provider = provider(&data, &login, wsl, prefix);
    let messages = json!([{ "role": "user", "content": "Look around." }]);
    let mut outcomes = Vec::new();
    for _ in 0..2 {
        outcomes.push(
            complete(&provider, &request("c1", messages.clone()))
                .await
                .map(|m| {
                    mcbench_providers::harness::message_json(&m)["content"]
                        .as_str()
                        .unwrap_or_default()
                        .to_owned()
                })
                .map_err(|e| e.message),
        );
    }
    mcbench_sandbox::Sandbox::shutdown(&distro).await;
    crate::codex_proc::check(&outcomes[0], &outcomes[1], None);
}

pub async fn windows_nowsl_a_codex_turn_without_wsl_says_wsl_is_needed() {
    let scratch = Scratch::new("windows-nowsl-codex");
    let root = scratch.join("sandbox");
    std::fs::create_dir_all(&root).unwrap();
    // What setup's shell step finds and records on a Windows without WSL.
    assert_eq!(
        mcbench_sandbox::windows::setup_state(&root).unwrap(),
        mcbench_sandbox::windows::WslSetup::Install
    );
    mcbench_sandbox::windows::wsl::unavailable(mcbench_sandbox::windows::WSL_MISSING.to_owned());
    let data = scratch.join("data");
    let login = scratch.join("login");
    let (wsl, prefix) = wsl_box(&data, &scratch.join("tools"), &login, "ab".to_owned());
    let provider = provider(&data, &login, wsl, prefix);
    let error = complete(
        &provider,
        &request("c1", json!([{ "role": "user", "content": "go" }])),
    )
    .await
    .unwrap_err();
    assert!(
        error.message.contains(wsl_codex::WSL_MISSING),
        "{}",
        error.message
    );
    println!("  the turn: {}", error.message);
}

/// One file served once over HTTP on 127.0.0.1, as the release host
/// serves it; the answer is its address.
fn serve_once(file: PathBuf) -> String {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = format!("http://{}/file", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        let (mut client, _) = listener.accept().unwrap();
        let mut head = Vec::new();
        let mut byte = [0u8; 1];
        while !head.ends_with(b"\r\n\r\n") {
            client.read_exact(&mut byte).unwrap();
            head.push(byte[0]);
        }
        let mut body = std::fs::File::open(&file).unwrap();
        let length = body.metadata().unwrap().len();
        write!(
            client,
            "HTTP/1.1 200 OK\r\nContent-Length: {length}\r\nConnection: close\r\n\r\n"
        )
        .unwrap();
        std::io::copy(&mut body, &mut client).unwrap();
    });
    address
}

/// The pinned Codex `linux-x64` package (`MCBENCH_CODEX_PACKAGE`, the
/// vendor's release file) installed into the Windows tools folder by the
/// app's own install, then run outside the box through the agent's `cli`
/// from the drvfs-mounted tools folder; what it leaves in CODEX_HOME is
/// listed. No sign-in, and no command that asks for an account.
pub async fn windows_wsl_codex_the_pinned_package_installs_and_runs_from_the_tools_folder() {
    use mcbench_providers::harness::install;
    let scratch = Scratch::new("windows-wsl-codex-package");
    let tools = scratch.join("tools");
    let login = scratch.join("profile").join(".codex");
    let mut pin = install::pins()
        .into_iter()
        .find(|p| p.name == "codex")
        .unwrap();
    let platform = install::platform_of("codex", &install::platform());
    assert_eq!(platform, "linux-x64");
    for (key, asset) in &mut pin.platforms {
        if *key == platform {
            asset.url = serve_once(PathBuf::from(var("MCBENCH_CODEX_PACKAGE")));
        }
    }
    let none = |_: install::Progress| {};
    let program = install::install(&pin, &tools, &platform, &none)
        .await
        .unwrap();
    assert_eq!(
        program,
        tools.join("codex-0.169.0").join("bin").join("codex")
    );
    for file in [
        "codex-package.json",
        "bin/codex-code-mode-host",
        "bin/codex-exec-server",
        "codex-path/rg",
        "codex-resources/bwrap",
    ] {
        assert!(tools.join("codex-0.169.0").join(file).is_file(), "{file}");
    }
    let (wsl, prefix) = wsl_box(&scratch.join("data"), &tools, &login, "ab".to_owned());
    let codex = provider(&scratch.join("data"), &login, wsl.clone(), prefix.clone());
    let version = codex.version().await.unwrap_or_else(|e| panic!("{e}"));
    println!("  codex --version through cli: {version}");
    assert_eq!(version, "codex-cli 0.169.0");
    let status = std::process::Command::new(&wsl.program)
        .args(&prefix)
        .args(["login", "status"])
        .env("WSL_UTF8", "1")
        .env_remove("WSLENV")
        .output()
        .unwrap();
    println!(
        "  codex login status: exit {:?}, {}{}",
        status.status.code(),
        String::from_utf8_lossy(&status.stdout).trim(),
        String::from_utf8_lossy(&status.stderr).trim()
    );
    let left = crate::dump::listing(&login);
    let sqlite: Vec<&String> = left
        .iter()
        .filter(|f| f.contains(".sqlite") || f.contains(".db"))
        .collect();
    println!("  SQLite files: {sqlite:?}");
}
