//! `test/codex-cli.test.js` (14) and `codex-cli-shims.test.js` (2) ported.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use mcbench_providers::Provider;
use mcbench_providers::bridge::{self, BridgeAddress, BridgeHost, McpContent};
use mcbench_providers::catalog::{CodexModels, models_from_cache};
use mcbench_providers::harness::codex_cli::{
    self, CodexCli, CodexConfig, Launch, LaunchSpec, Launched, check_item, codex_install,
    config_toml, context_tokens_of, host_args, prepare_home, usage_of,
};
use mcbench_providers::harness::{self, read_session};
use mcbench_providers::{ToolSchema, harness::BridgeCommand};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use super::codex_tests;
use super::{
    Deltas, Fake, Req, Test, as_json, bridge, codex, complete_codex, digest, scratch, small_tool,
    tests,
};

pub fn all() -> Vec<Test> {
    tests![
        codex_tests::the_model_catalogue_lists_visible_models_in_priority_order_with_their_native_efforts,
        codex_tests::the_sandbox_configuration_pins_the_model_effort_instructions_disabled_features_and_the_mcp_server,
        codex_tests::the_host_run_ignores_user_config_and_rules_passes_the_same_settings_as_overrides_and_denies_writes,
        codex_tests::only_text_reasoning_plan_error_and_mcbench_tool_items_pass,
        codex_tests::turn_usage_maps_onto_the_openai_token_accounting,
        codex_tests::the_harness_has_a_launcher_for_linux_and_macos_and_refuses_other_platforms,
        codex_tests::a_host_turn_reaches_mcbenchs_tools_through_code_mode,
        codex_tests::the_sandboxs_config_toml_gives_a_turn_mcbenchs_tools_and_no_warning,
        codex_tests::a_thread_is_recorded_when_it_starts_a_stopped_turns_included_and_closing_after_exit_stops_nothing,
        codex_tests::a_run_whose_release_fails_after_it_exits_is_logged_and_the_turn_is_answered,
        codex_tests::the_context_reading_is_the_last_requests_input_from_the_threads_own_rollout_file,
        codex_tests::a_completed_turn_reports_its_last_requests_input_and_a_resumed_turn_the_new_last_one,
        codex_tests::a_failed_turn_reports_codexs_failure_and_reads_no_rollout_file,
        codex_tests::concurrent_catalogue_lookups_share_one_debug_models_run,
        codex_tests::every_kind_of_shim_runs_the_codex_it_leads_to_and_the_sandbox_mounts_that_install,
        codex_tests::a_shim_that_leads_nowhere_fails_naming_the_command_and_a_program_outside_an_install_is_not_mounted,
        codex_tests::a_thread_id_is_a_uuid_as_codex_writes_one,
        codex_tests::a_resumed_threads_id_comes_after_dashes_whatever_it_holds,
        codex_tests::a_thread_id_that_is_not_codexs_fails_the_turn_and_never_reaches_the_next_turns_command,
    ]
}

/// The thread id Codex 0.169.0 writes (`thread.started`, observed) is a
/// lowercase hyphenated UUID; nothing else is one.
pub async fn a_thread_id_is_a_uuid_as_codex_writes_one() {
    for id in [
        "01a1f543-87cf-75d1-aa79-b40d57f6efd4",
        "00000000-0000-0000-0000-000000000000",
    ] {
        assert!(codex_cli::is_thread_id(id), "{id}");
    }
    for id in [
        "-cfeatures.shell_tool=true",
        "--last",
        "th-1",
        "",
        "01A1F543-87CF-75D1-AA79-B40D57F6EFD4",
        "01a1f54387cf75d1aa79b40d57f6efd4",
        "01a1f543-87cf-75d1-aa79-b40d57f6efd4 ",
        "01a1f543-87cf-75d1-aa79-b40d57f6efd",
        "01a1f543-87cf-75d1-aa79-b40d57f6efdg",
        "01a1f543_87cf_75d1_aa79_b40d57f6efd4",
    ] {
        assert!(!codex_cli::is_thread_id(id), "{id}");
    }
    assert_eq!(
        codex_cli::NOT_A_THREAD_ID,
        "The Codex CLI started a thread whose id is not a UUID, the form of every Codex thread id, and mcbench stopped the turn"
    );
}

/// A resumed thread's id goes after `--`, just before the prompt's `-`,
/// on the host's command line and the box's, so Codex reads whatever it
/// holds as the session id (Codex 0.169.0, offline: `--` then a real id
/// resumes that thread, then a missing one fails as without `--`, then an
/// option-shaped value is no option).
pub async fn a_resumed_threads_id_comes_after_dashes_whatever_it_holds() {
    let id = "-cfeatures.shell_tool=true";
    let bridge_args = ["tool-bridge".to_owned(), "/tmp/m.sock".to_owned()];
    let host = host_args(
        "m",
        None,
        "S",
        ("/app/mcbench-lite", &bridge_args),
        true,
        Some(id),
        &["/tmp/image-1.png".to_owned()],
    );
    assert_eq!(&host[..2], ["exec", "resume"]);
    assert_eq!(
        &host[host.len() - 5..],
        ["-i", "/tmp/image-1.png", "--", id, "-"]
    );
    assert_eq!(host.iter().filter(|a| *a == id).count(), 1);
    let spec = |resume: bool| LaunchSpec {
        dir: "/d/harness/c1".into(),
        runtime_dir: "/r".into(),
        socket: "/r/sock/mcp.sock".into(),
        model: "gpt-6-astra".to_owned(),
        effort: None,
        system_prompt: "S".to_owned(),
        resume,
        thread_id: resume.then(|| id.to_owned()),
        images: Vec::new(),
    };
    let images = vec!["/run/mcbench/image-1.png".to_owned()];
    assert_eq!(
        codex_cli::box_command("/opt/codex/bin/codex", &spec(true), images.clone()),
        [
            "/opt/codex/bin/codex",
            "exec",
            "resume",
            "--json",
            "--skip-git-repo-check",
            "-i",
            "/run/mcbench/image-1.png",
            "--",
            id,
            "-"
        ]
    );
    assert_eq!(
        codex_cli::box_command("/opt/codex/bin/codex", &spec(false), images),
        [
            "/opt/codex/bin/codex",
            "exec",
            "--json",
            "--skip-git-repo-check",
            "-i",
            "/run/mcbench/image-1.png",
            "-"
        ]
    );
}

/// A line that claims to be a `thread.started` of an option-shaped id
/// (what a process in Codex's box can write to Codex's stdout) fails the
/// turn with [`codex_cli::NOT_A_THREAD_ID`] and is never stored: after
/// Codex's own thread the record keeps Codex's id and the next turn's
/// command (`box_command`, as the box runs it) resumes that id after `--`;
/// alone, nothing is stored and the next turn starts a new thread. No
/// race: the fake writes the lines in order.
pub async fn a_thread_id_that_is_not_codexs_fails_the_turn_and_never_reaches_the_next_turns_command()
 {
    const OWN: &str = "01a1f500-0000-7000-8000-0000000000e1";
    const FORGED: &str = "-cfeatures.shell_tool=true";
    let dir = scratch("codex-forged-thread");
    // Each launch's chat and the command the box would run for it.
    type Commands = Arc<Mutex<Vec<(String, Vec<String>)>>>;
    let commands: Commands = Arc::default();
    let script = dir.join("codex.py");
    let seen = commands.clone();
    let launch_script = script.clone();
    let launch = Launch::Custom(Arc::new(move |spec: &LaunchSpec, _| {
        let chat = spec.dir.file_name().unwrap().to_string_lossy().into_owned();
        seen.lock().unwrap().push((
            chat,
            codex_cli::box_command("/opt/codex/bin/codex", spec, Vec::new()),
        ));
        std::fs::create_dir_all(spec.dir.join("workspace")).unwrap();
        let mut env: Vec<_> = std::env::vars_os().collect();
        env.push(("CODEX_HOME".into(), spec.dir.join("codex-home").into()));
        Ok(Launched {
            command: "python3".into(),
            args: vec![launch_script.to_string_lossy().into_owned()],
            cwd: spec.dir.join("workspace"),
            env,
            on_stop: Box::new(|| {}),
            release: Box::new(|| Ok(())),
        })
    }));
    let provider = with_launcher(&dir, launch);
    let emit = |ids: &[&str]| {
        let mut source = "import json, sys\nsys.stdin.read()\n".to_owned();
        for id in ids {
            source.push_str(&format!(
                "print(json.dumps({{'type': 'thread.started', 'thread_id': {id:?}}}), flush=True)\n"
            ));
        }
        source.push_str("print(json.dumps({'type': 'item.completed', 'item': {'type': 'agent_message', 'text': 'Done'}}), flush=True)\n");
        std::fs::write(&script, source).unwrap();
    };
    let turn = async |chat: &str, transcript: Value| {
        let messages = if transcript.as_array().unwrap().len() == 1 {
            json!([{ "role": "user", "content": "Build a hut." }])
        } else {
            json!([{ "role": "user", "content": "Build a hut." }, { "role": "assistant", "content": "" }, { "role": "user", "content": "Again." }])
        };
        let req = Req::new(messages).chat(chat, Some(transcript));
        complete_codex(
            &provider,
            &req.build(),
            &Deltas::default(),
            &CancellationToken::new(),
        )
        .await
        .map(|m| {
            as_json(&m)["content"]
                .as_str()
                .unwrap_or_default()
                .to_owned()
        })
        .map_err(|e| e.message)
    };
    let first = json!([{ "id": "u1", "role": "user" }]);
    let next = json!([{ "id": "u1", "role": "user" }, { "id": "a1", "role": "assistant" }, { "id": "u2", "role": "user" }]);
    let command_of = |chat: &str| {
        commands
            .lock()
            .unwrap()
            .iter()
            .rev()
            .find(|(c, _)| c == chat)
            .map(|(_, command)| command.clone())
            .unwrap()
    };
    for (chat, ids) in [("after-own", vec![OWN, FORGED]), ("alone", vec![FORGED])] {
        emit(&ids);
        assert_eq!(
            turn(chat, first.clone()).await,
            Err(codex_cli::NOT_A_THREAD_ID.to_owned()),
            "{chat}"
        );
        let stored = read_session(&dir.join("data"), chat, codex_cli::NAME)
            .unwrap()
            .map(|r| r["threadId"].clone());
        emit(&[OWN]);
        assert_eq!(
            turn(chat, next.clone()).await,
            Ok("Done".to_owned()),
            "{chat}"
        );
        let command = command_of(chat);
        println!("  {chat}: stored {stored:?}; the next turn's command: {command:?}");
        assert!(!command.iter().any(|a| a == FORGED), "{chat}: {command:?}");
        if chat == "after-own" {
            assert_eq!(stored, Some(json!(OWN)));
            assert_eq!(&command[command.len() - 3..], ["--", OWN, "-"]);
        } else {
            assert_eq!(stored, None);
            assert_eq!(&command[1..3], ["exec", "--json"]);
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

fn level(effort: &str) -> Value {
    json!({ "effort": effort, "description": "" })
}

pub async fn the_model_catalogue_lists_visible_models_in_priority_order_with_their_native_efforts()
{
    let models: Vec<Value> = models_from_cache(&json!({ "models": [
        { "slug": "gpt-reserve", "display_name": "Reserve", "visibility": "hide", "supported_reasoning_levels": [level("low")], "default_reasoning_level": "low", "input_modalities": ["text", "image"], "priority": 1 },
        { "slug": "gpt-5.6-sol", "display_name": "GPT-5.6-Sol", "visibility": "list", "additional_speed_tiers": ["fast"], "service_tiers": [{ "id": "priority", "name": "Fast", "description": "2x speed" }], "supported_reasoning_levels": (["low", "medium", "high", "xhigh", "max", "ultra"].map(level)), "default_reasoning_level": "low", "input_modalities": ["text", "image"], "priority": 5 },
        { "slug": "gpt-6-astra", "display_name": "GPT-6-Astra", "visibility": "list", "supported_reasoning_levels": (["low", "medium", "high"].map(level)), "default_reasoning_level": "medium", "input_modalities": ["text"], "priority": 2 },
    ] })).iter().map(|m| m.to_json()).collect();
    assert_eq!(
        models
            .iter()
            .map(|m| m["id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["gpt-6-astra", "gpt-5.6-sol"]
    );
    assert_eq!(
        mcbench_json::stringify(&models[1]),
        mcbench_json::stringify(&json!({
            "id": "gpt-5.6-sol", "name": "GPT-5.6-Sol", "created": 0, "known": true, "imageInput": true, "imageOutput": false, "tools": true,
            "reasoning": true, "efforts": ["low", "medium", "high", "xhigh", "max", "ultra"], "defaultEffort": "low", "mandatoryReasoning": true,
            "contextWindow": null, "fast": true, "fastNote": "2x speed",
        }))
    );
    assert_eq!(models[0]["imageInput"], false);
}

pub async fn the_sandbox_configuration_pins_the_model_effort_instructions_disabled_features_and_the_mcp_server()
 {
    let toml = config_toml("gpt-6-astra", Some("high"), "Build \"well\".\nLine two");
    assert!(toml.starts_with("model = \"gpt-6-astra\"\nmodel_reasoning_effort = \"high\"\napproval_policy = \"never\"\nsandbox_mode = \"danger-full-access\"\n"));
    assert!(toml.contains("developer_instructions = \"Build \\\"well\\\".\\nLine two\"\n"));
    assert!(toml.contains("\nfeatures.shell_tool = false\n"));
    assert!(toml.contains("\nfeatures.code_mode_host = true\n"));
    assert!(
        !toml
            .lines()
            .any(|l| l.starts_with("features.multi_agent = "))
    );
    assert!(toml.contains("\nfeatures.multi_agent_v2 = false\n"));
    assert!(toml.contains("\nagents.enabled = false\n"));
    // The bridge inside the sandbox is the static agent's relay (today python3 and mcp-bridge.py).
    assert!(toml.contains("\nmcp_servers.mcbench.command = \"/run/mcbench-relay\"\nmcp_servers.mcbench.args = [\"tool-bridge\", \"/run/mcbench/mcp.sock\"]\n"));
    assert!(!config_toml("m", None, "s").contains("model_reasoning_effort"));
}

pub async fn the_host_run_ignores_user_config_and_rules_passes_the_same_settings_as_overrides_and_denies_writes()
 {
    let bridge_args = vec!["tool-bridge".to_owned(), "/tmp/m.sock".to_owned()];
    let args = host_args(
        "gpt-6-astra",
        Some("low"),
        "S",
        ("/app/mcbench-lite", &bridge_args),
        false,
        None,
        &["/tmp/image-1.png".to_owned()],
    );
    assert_eq!(
        &args[..6],
        [
            "exec",
            "--json",
            "--skip-git-repo-check",
            "--ignore-user-config",
            "--ignore-rules",
            "-c"
        ]
    );
    let overrides: Vec<&String> = args
        .iter()
        .enumerate()
        .filter(|(i, _)| *i > 0 && args[i - 1] == "-c")
        .map(|(_, a)| a)
        .collect();
    let has = |o: &str| overrides.iter().any(|x| *x == o);
    assert!(has("model=\"gpt-6-astra\"") && has("model_reasoning_effort=\"low\""));
    assert!(has("sandbox_mode=\"read-only\"") && has("approval_policy=\"never\""));
    assert!(has("features.shell_tool=false") && has("developer_instructions=\"S\""));
    assert!(has("agents.enabled=false") && has("features.multi_agent_v2=false"));
    assert!(
        !overrides
            .iter()
            .any(|o| o.starts_with("features.multi_agent="))
    );
    assert!(has("features.code_mode_host=true"));
    assert!(has("mcp_servers.mcbench.command=\"/app/mcbench-lite\""));
    assert!(has(
        "mcp_servers.mcbench.args=[\"tool-bridge\", \"/tmp/m.sock\"]"
    ));
    assert!(has(
        "mcp_servers.mcbench.default_tools_approval_mode=\"approve\""
    ));
    assert_eq!(&args[args.len() - 3..], ["-i", "/tmp/image-1.png", "-"]);
    let resumed = host_args(
        "m",
        None,
        "S",
        ("/app/mcbench-lite", &bridge_args),
        true,
        Some("thread-1"),
        &[],
    );
    assert_eq!(&resumed[..3], ["exec", "resume", "--json"]);
    assert_eq!(&resumed[resumed.len() - 3..], ["--", "thread-1", "-"]);
    assert!(
        !resumed
            .iter()
            .any(|a| a.starts_with("model_reasoning_effort"))
    );
    let config = CodexConfig {
        data_dir: "/d".into(),
        binary: "codex".into(),
        prefix: Vec::new(),
        home: Some("/h".into()),
        bridge: bridge(),
        launch: None,
    };
    assert!(
        !config
            .host_env(None)
            .iter()
            .any(|(k, _)| k == "OPENAI_API_KEY")
    );
}

pub async fn only_text_reasoning_plan_error_and_mcbench_tool_items_pass() {
    for kind in [
        "command_execution",
        "file_change",
        "web_search",
        "collab_tool_call",
        "image_generation",
        "some_future_tool",
    ] {
        assert_eq!(
            check_item(&json!({ "type": kind })).unwrap_err(),
            format!("Codex used a tool outside mcbench: {kind}")
        );
    }
    assert_eq!(
        check_item(&json!({ "type": "mcp_tool_call", "server": "other", "tool": "x" }))
            .unwrap_err(),
        "Codex used a tool outside mcbench: mcp_tool_call other"
    );
    for item in [
        json!({ "type": "mcp_tool_call", "server": "mcbench", "tool": "build" }),
        json!({ "type": "agent_message", "text": "hi" }),
        json!({ "type": "reasoning", "text": "thinking" }),
        json!({ "type": "todo_list", "items": [] }),
        json!({ "type": "error", "message": "a warning" }),
    ] {
        check_item(&item).unwrap();
    }
}

pub async fn turn_usage_maps_onto_the_openai_token_accounting() {
    assert_eq!(
        usage_of(Some(&json!({ "input_tokens": 10, "cached_input_tokens": 4, "output_tokens": 6, "reasoning_output_tokens": 2 }))).unwrap(),
        json!({ "input_tokens": 10, "output_tokens": 6, "input_tokens_details": { "cached_tokens": 4 }, "output_tokens_details": { "reasoning_tokens": 2 } })
    );
}

pub async fn the_harness_has_a_launcher_for_linux_and_macos_and_refuses_other_platforms() {
    let files = codex_cli::SandboxFiles {
        rootfs: "/r".into(),
        image_env: Vec::new(),
        relay: "/a".into(),
    };
    let launch = Launch::for_this_platform(std::sync::Arc::new(move || Ok(files.clone())), || {
        unreachable!("WSL is Windows'")
    });
    if cfg!(target_os = "linux") {
        assert!(matches!(launch, Some(Launch::Sandboxed(_))));
    } else if cfg!(target_os = "macos") {
        assert!(matches!(launch, Some(Launch::Host)));
    }
    let dir = scratch("codex-no-launcher");
    let provider = codex(
        &dir.join("data"),
        Path::new("codex"),
        &dir.join("login"),
        None,
    );
    let error = complete_codex(
        &provider,
        &Req::new(json!([{ "role": "user", "content": "go" }])).build(),
        &Deltas::default(),
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert!(
        error.message.starts_with(
            "The Codex CLI harness runs on Linux (in the model's shell) or macOS (on the host), not "
        ),
        "{}",
        error.message
    );
    let _ = std::fs::remove_dir_all(&dir);
}

struct Echo {
    tools: Vec<ToolSchema>,
    calls: Mutex<Vec<String>>,
}

#[async_trait::async_trait]
impl BridgeHost for Echo {
    fn tools(&self) -> Vec<ToolSchema> {
        self.tools.clone()
    }
    async fn call(
        &self,
        name: &str,
        _: Value,
        _: &str,
        _: Option<&str>,
        _: &CancellationToken,
    ) -> Result<Vec<McpContent>, String> {
        self.calls.lock().unwrap().push(name.to_owned());
        Ok(vec![McpContent(
            json!({ "type": "text", "text": format!("{name} done") }),
        )])
    }
}

fn tool_with_properties(name: &str) -> ToolSchema {
    ToolSchema(mcbench_json::stringify(
        &json!({ "type": "function", "function": { "name": name, "description": name, "parameters": { "type": "object", "properties": {} } } }),
    ))
}

fn code_mode() -> Value {
    json!({ "mode": "codex", "spawns": [{ "mcp": true, "code_mode": true, "turns": [] }] })
}

fn items(stdout: &str) -> Vec<Value> {
    stdout
        .lines()
        .map(|l| serde_json::from_str::<Value>(l).unwrap())
        .filter(|e| e["type"] == "item.completed")
        .map(|e| e["item"].clone())
        .collect()
}

pub async fn a_host_turn_reaches_mcbenchs_tools_through_code_mode() {
    // The launch a turn makes on the host, run to its end with a host of
    // the test's (two tools), as today's test runs LAUNCHERS.darwin.
    let dir = scratch("codex-code-mode");
    let fake = Fake::new(&dir.join("fake"), "codex", &code_mode());
    let config = CodexConfig {
        data_dir: dir.join("data"),
        binary: fake.program.clone().into(),
        prefix: Vec::new(),
        home: Some(dir.join("login")),
        bridge: bridge(),
        launch: Some(Launch::Host),
    };
    let host = Arc::new(Echo {
        tools: vec![
            tool_with_properties("build"),
            tool_with_properties("see_build"),
        ],
        calls: Mutex::new(Vec::new()),
    });
    let sockets = crate::scratch::Scratch::for_sockets("codex-code-mode");
    let socket = sockets.join("mcp.sock");
    let server = bridge::serve(&BridgeAddress::UnixSocket(socket.clone()), host.clone()).unwrap();
    let (command, args) = config.bridge.for_address(&socket.to_string_lossy());
    let home = dir.join("chat").join("codex-home");
    std::fs::create_dir_all(dir.join("chat").join("workspace")).unwrap();
    let out = tokio::process::Command::new(&fake.program)
        .args(host_args(
            "gpt-6-luna",
            Some("low"),
            "S",
            (&command, &args),
            false,
            None,
            &[],
        ))
        .current_dir(dir.join("chat").join("workspace"))
        .env_clear()
        .envs(config.host_env(Some(&home)))
        .stdin(std::process::Stdio::null())
        .output()
        .await
        .unwrap();
    server.stop();
    let found = items(&String::from_utf8_lossy(&out.stdout));
    assert_eq!(*host.calls.lock().unwrap(), ["build", "see_build"]);
    let summary: Vec<(String, String, String)> = found
        .iter()
        .map(|i| {
            (
                i["type"].as_str().unwrap().to_owned(),
                i["tool"].as_str().unwrap().to_owned(),
                i["result"]["content"][0]["text"]
                    .as_str()
                    .unwrap()
                    .to_owned(),
            )
        })
        .collect();
    assert_eq!(
        summary,
        [
            ("mcp_tool_call".into(), "build".into(), "build done".into()),
            (
                "mcp_tool_call".into(),
                "see_build".into(),
                "see_build done".into()
            )
        ]
    );
    for item in &found {
        check_item(item).unwrap();
    }
    let _ = std::fs::remove_dir_all(&dir);
}

pub async fn the_sandboxs_config_toml_gives_a_turn_mcbenchs_tools_and_no_warning() {
    let dir = scratch("codex-config");
    let fake = Fake::new(&dir.join("fake"), "codex", &code_mode());
    let home = dir.join("codex-home");
    let sockets = crate::scratch::Scratch::for_sockets("codex-config");
    let socket = sockets.join("mcp.sock");
    prepare_home(&home).unwrap();
    let exe = std::env::current_exe().unwrap();
    let toml = config_toml("gpt-6-luna", Some("low"), "S")
        .replace(
            "\"/run/mcbench-relay\"",
            &mcbench_json::stringify(&json!(exe.to_string_lossy())),
        )
        .replace(
            "\"/run/mcbench/mcp.sock\"",
            &mcbench_json::stringify(&json!(socket.to_string_lossy())),
        );
    std::fs::write(home.join("config.toml"), toml).unwrap();
    let host = Arc::new(Echo {
        tools: vec![tool_with_properties("build")],
        calls: Mutex::new(Vec::new()),
    });
    let server = bridge::serve(&BridgeAddress::UnixSocket(socket.clone()), host.clone()).unwrap();
    let out = tokio::process::Command::new(&fake.program)
        .args(["exec", "--json", "--skip-git-repo-check", "-"])
        .current_dir(&dir)
        .env("CODEX_HOME", &home)
        .stdin(std::process::Stdio::null())
        .output()
        .await
        .unwrap();
    server.stop();
    assert_eq!(
        *host.calls.lock().unwrap(),
        ["build"],
        "{} {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        items(&String::from_utf8_lossy(&out.stdout))
            .iter()
            .map(|i| i["type"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["mcp_tool_call"]
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[derive(Default)]
struct Seen {
    stops: usize,
    released: usize,
}

/// A stand-in launcher: `codex exec` is a Python script; its CODEX_HOME is
/// the run's `<dir>/codex-home`, as both real launchers give it.
fn fake_launcher(script: PathBuf, seen: Arc<Mutex<Seen>>, release_fails: bool) -> Launch {
    Launch::Custom(Arc::new(move |spec: &LaunchSpec, _| {
        std::fs::create_dir_all(spec.dir.join("workspace")).unwrap();
        let mut env: Vec<_> = std::env::vars_os().collect();
        env.push(("CODEX_HOME".into(), spec.dir.join("codex-home").into()));
        let (a, b) = (seen.clone(), seen.clone());
        Ok(Launched {
            command: "python3".into(),
            args: vec![script.to_string_lossy().into_owned()],
            cwd: spec.dir.join("workspace"),
            env,
            on_stop: Box::new(move || a.lock().unwrap().stops += 1),
            release: Box::new(move || {
                b.lock().unwrap().released += 1;
                if release_fails {
                    Err("the proxy would not close".to_owned())
                } else {
                    Ok(())
                }
            }),
        })
    }))
}

fn with_launcher(dir: &Path, launch: Launch) -> CodexCli {
    CodexCli::new(
        CodexConfig {
            data_dir: dir.join("data"),
            binary: "codex".into(),
            prefix: Vec::new(),
            home: Some(dir.join("login")),
            bridge: bridge(),
            launch: Some(launch),
        },
        Arc::new(CodexModels::default()),
    )
}

const ON_TERM_EXIT_0: &str =
    "import json, signal, sys, os\nsignal.signal(signal.SIGTERM, lambda *a: os._exit(0))\n";

pub async fn a_thread_is_recorded_when_it_starts_a_stopped_turns_included_and_closing_after_exit_stops_nothing()
 {
    let dir = scratch("codex-thread");
    let script = dir.join("codex.py");
    std::fs::write(&script, format!("{ON_TERM_EXIT_0}print(json.dumps({{'type': 'thread.started', 'thread_id': '01a1f500-0000-7000-8000-000000000001'}}), flush=True)\nsys.stdin.read()\nprint(json.dumps({{'type': 'item.completed', 'item': {{'type': 'agent_message', 'text': 'Done'}}}}), flush=True)\n")).unwrap();
    let seen = Arc::new(Mutex::new(Seen::default()));
    let provider = with_launcher(&dir, fake_launcher(script.clone(), seen.clone(), false));
    let transcript = json!([{ "id": "u1", "role": "user" }]);
    let req = Req::new(json!([{ "role": "user", "content": "Build a hut." }]))
        .chat("conv-codex", Some(transcript));
    let done = complete_codex(
        &provider,
        &req.build(),
        &Deltas::default(),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(done["content"], "Done");
    assert_eq!(
        read_session(&dir.join("data"), "conv-codex", "codex-cli")
            .unwrap()
            .unwrap()["threadId"],
        "01a1f500-0000-7000-8000-000000000001"
    );
    provider.close_chat("conv-codex");
    assert_eq!(seen.lock().unwrap().stops, 0);

    // Stopped: the process exits 0 on SIGTERM; the thread was recorded
    // when Codex named it, so the next turn resumes it.
    std::fs::write(&script, format!("{ON_TERM_EXIT_0}import time\nprint(json.dumps({{'type': 'thread.started', 'thread_id': '01a1f500-0000-7000-8000-000000000002'}}), flush=True)\nwhile True: time.sleep(1)\n")).unwrap();
    let cancel = CancellationToken::new();
    // Stopped once Codex has named the new thread (and it is recorded).
    let stopping = {
        let (cancel, data) = (cancel.clone(), dir.join("data"));
        async move {
            super::until("the new thread is recorded", || {
                read_session(&data, "conv-codex", "codex-cli")
                    .unwrap()
                    .is_some_and(|record| {
                        record["threadId"] == "01a1f500-0000-7000-8000-000000000002"
                    })
            })
            .await;
            cancel.cancel();
        }
    };
    let (built, seen_deltas) = (req.build(), Deltas::default());
    let (outcome, ()) = tokio::join!(
        complete_codex(&provider, &built, &seen_deltas, &cancel),
        stopping
    );
    assert_eq!(outcome.unwrap_err().message, "This operation was aborted");
    super::until("the stopped run is released", || {
        seen.lock().unwrap().released >= 2
    })
    .await;
    assert_eq!(seen.lock().unwrap().stops, 1);
    let empty = digest("");
    let record = read_session(&dir.join("data"), "conv-codex", "codex-cli")
        .unwrap()
        .unwrap();
    assert_eq!(
        Value::Object(record),
        json!({ "threadId": "01a1f500-0000-7000-8000-000000000002", "home": "chat", "instructions": empty, "resolution": empty, "lastSeen": "u1" })
    );
    let _ = std::fs::remove_dir_all(&dir);
}

pub async fn a_run_whose_release_fails_after_it_exits_is_logged_and_the_turn_is_answered() {
    let dir = scratch("codex-release");
    let script = dir.join("codex.py");
    std::fs::write(&script, "import json, sys\nprint(json.dumps({'type': 'thread.started', 'thread_id': '01a1f500-0000-7000-8000-000000000003'}), flush=True)\nsys.stdin.read()\nprint(json.dumps({'type': 'item.completed', 'item': {'type': 'agent_message', 'text': 'Done'}}), flush=True)\n").unwrap();
    let seen = Arc::new(Mutex::new(Seen::default()));
    let provider = with_launcher(&dir, fake_launcher(script, seen.clone(), true));
    let req =
        Req::new(json!([{ "role": "user", "content": "Build a hut." }])).chat("conv-release", None);
    let done = complete_codex(
        &provider,
        &req.build(),
        &Deltas::default(),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(done["content"], "Done");
    super::until("the run is released", || seen.lock().unwrap().released >= 1).await;
    assert_eq!(
        seen.lock().unwrap().released,
        1,
        "released, and its failure logged (stderr), not a panic"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

fn token_count(input: Option<u64>, cached: u64) -> Value {
    json!({ "type": "event_msg", "timestamp": "2026-11-11T12:00:01.000Z", "payload": { "type": "token_count", "rate_limits": {}, "info": input.map(|i| json!({
        "total_token_usage": { "input_tokens": 999999, "cached_input_tokens": 0, "output_tokens": 1, "reasoning_output_tokens": 0, "total_tokens": 1000000 },
        "last_token_usage": { "input_tokens": i, "cached_input_tokens": cached, "cache_write_input_tokens": 0, "output_tokens": 283, "reasoning_output_tokens": 65, "total_tokens": i + 283 },
        "last_usage_includes_reasoning": true, "model_context_window": 258400 })) } })
}

fn write_rollout(home: &Path, day: &str, thread: &str, events: &[Value]) {
    let folder = day
        .split('-')
        .fold(home.join("sessions"), |p, part| p.join(part));
    std::fs::create_dir_all(&folder).unwrap();
    let text: String = events
        .iter()
        .map(|e| mcbench_json::stringify(e) + "\n")
        .collect();
    std::fs::write(
        folder.join(format!("rollout-{day}T12-00-00-{thread}.jsonl")),
        text,
    )
    .unwrap();
}

pub async fn the_context_reading_is_the_last_requests_input_from_the_threads_own_rollout_file() {
    let home = scratch("codex-rollout");
    write_rollout(&home, "2026-11-10", "th-a", &[token_count(Some(5000), 0)]);
    write_rollout(
        &home,
        "2026-11-11",
        "th-b",
        &[
            json!({ "type": "session_meta", "payload": { "id": "th-b" } }),
            token_count(Some(40000), 0),
            token_count(Some(98982), 81408),
            token_count(None, 0),
        ],
    );
    assert_eq!(context_tokens_of(&home, "th-a").unwrap(), 5000);
    assert_eq!(context_tokens_of(&home, "th-b").unwrap(), 98982);
    assert_eq!(
        context_tokens_of(&home, "th-none").unwrap_err(),
        format!(
            "No Codex rollout file for thread th-none under {}",
            home.join("sessions").display()
        )
    );
    write_rollout(
        &home,
        "2026-11-11",
        "th-c",
        &[
            json!({ "type": "session_meta", "payload": { "id": "th-c" } }),
            token_count(None, 0),
        ],
    );
    let error = context_tokens_of(&home, "th-c").unwrap_err();
    assert!(
        error.starts_with("No token_count event with usage in ")
            && error.ends_with("rollout-2026-11-11T12-00-00-th-c.jsonl"),
        "{error}"
    );
    let _ = std::fs::remove_dir_all(&home);
}

/// A fake `codex exec` whose completed turn writes its rollout file like
/// Codex: session_meta on a new thread, then token_count events, and a
/// turn.completed whose usage is the sum over the turn's requests.
fn rollout_script(thread: &str, append: bool, requests: &[(u64, u64)], sum: u64) -> String {
    let mut events = Vec::new();
    if !append {
        events.push(json!({ "type": "session_meta", "payload": { "id": thread } }));
        events.push(token_count(None, 0));
    }
    events.extend(requests.iter().map(|(i, c)| token_count(Some(*i), *c)));
    events.push(token_count(None, 0));
    format!(
        "import json, os, sys\nfolder = os.path.join(os.environ['CODEX_HOME'], 'sessions', '2026', '11', '11')\nevents = json.loads({events:?})\nprint(json.dumps({{'type': 'thread.started', 'thread_id': '{thread}'}}), flush=True)\nsys.stdin.read()\nos.makedirs(folder, exist_ok=True)\nwith open(os.path.join(folder, 'rollout-2026-11-11T12-00-00-{thread}.jsonl'), 'a') as f:\n    f.write(''.join(json.dumps(e) + '\\n' for e in events))\nprint(json.dumps({{'type': 'item.completed', 'item': {{'type': 'agent_message', 'text': 'Done'}}}}))\nprint(json.dumps({{'type': 'turn.completed', 'usage': {{'input_tokens': {sum}, 'cached_input_tokens': 81408, 'output_tokens': 566, 'reasoning_output_tokens': 130}}}}), flush=True)\n",
        events = mcbench_json::stringify(&Value::Array(events)),
    )
}

pub async fn a_completed_turn_reports_its_last_requests_input_and_a_resumed_turn_the_new_last_one()
{
    let dir = scratch("codex-context");
    let script = dir.join("codex.py");
    std::fs::write(
        &script,
        rollout_script(
            "01a1f500-0000-7000-8000-000000000005",
            false,
            &[(40000, 0), (98982, 81408)],
            138982,
        ),
    )
    .unwrap();
    let provider = with_launcher(
        &dir,
        fake_launcher(script.clone(), Arc::new(Mutex::new(Seen::default())), false),
    );
    let first = Req::new(json!([{ "role": "user", "content": "Build a hut." }])).chat(
        "conv-context",
        Some(json!([{ "id": "u1", "role": "user" }])),
    );
    let done = as_json(
        &complete_codex(
            &provider,
            &first.build(),
            &Deltas::default(),
            &CancellationToken::new(),
        )
        .await
        .unwrap(),
    );
    assert_eq!(done["content"], "Done");
    assert_eq!(done["contextTokens"], 98982);
    assert_eq!(done["usage"]["input_tokens"], 138982);
    std::fs::write(
        &script,
        rollout_script(
            "01a1f500-0000-7000-8000-000000000005",
            true,
            &[(120500, 99000)],
            120500,
        ),
    )
    .unwrap();
    let messages = json!([{ "role": "user", "content": "Build a hut." }, { "role": "assistant", "content": "Done" }, { "role": "user", "content": "Add a door." }]);
    let second = Req::new(messages).chat("conv-context", Some(json!([{ "id": "u1", "role": "user" }, { "id": "a1", "role": "assistant" }, { "id": "u2", "role": "user" }])));
    let resumed = as_json(
        &complete_codex(
            &provider,
            &second.build(),
            &Deltas::default(),
            &CancellationToken::new(),
        )
        .await
        .unwrap(),
    );
    assert_eq!(resumed["contextTokens"], 120500);
    let sessions = harness::conversation_dir(&dir.join("data"), "conv-context")
        .join("codex-home")
        .join("sessions");
    let mut files = 0;
    let mut stack = vec![sessions];
    while let Some(folder) = stack.pop() {
        for entry in std::fs::read_dir(folder).unwrap().flatten() {
            if entry.file_type().unwrap().is_dir() {
                stack.push(entry.path());
            } else if entry.path().extension().is_some_and(|e| e == "jsonl") {
                files += 1;
            }
        }
    }
    assert_eq!(files, 1);
    let _ = std::fs::remove_dir_all(&dir);
}

pub async fn a_failed_turn_reports_codexs_failure_and_reads_no_rollout_file() {
    let dir = scratch("codex-failed");
    let script = dir.join("codex.py");
    std::fs::write(&script, "import json, sys\nprint(json.dumps({'type': 'thread.started', 'thread_id': '01a1f500-0000-7000-8000-000000000004'}), flush=True)\nsys.stdin.read()\nprint(json.dumps({'type': 'turn.failed', 'error': {'message': 'usage limit reached'}}), flush=True)\n").unwrap();
    let provider = with_launcher(
        &dir,
        fake_launcher(script, Arc::new(Mutex::new(Seen::default())), false),
    );
    let req =
        Req::new(json!([{ "role": "user", "content": "Build a hut." }])).chat("conv-failed", None);
    let error = complete_codex(
        &provider,
        &req.build(),
        &Deltas::default(),
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert_eq!(error.message, "Codex turn failed: usage limit reached");
    let _ = std::fs::remove_dir_all(&dir);
}

pub async fn concurrent_catalogue_lookups_share_one_debug_models_run() {
    let dir = scratch("codex-catalogue");
    let counter = dir.join("debug-models.txt");
    let binary = dir.join("codex");
    std::fs::write(&binary, format!("#!/usr/bin/env python3\nimport json, time\nopen({:?}, 'a').write('run\\n')\ntime.sleep(0.1)\nprint(json.dumps({{'models': [{{'slug': 'gpt-6-astra', 'display_name': 'Astra', 'visibility': 'list', 'priority': 1}}]}}))\n", counter.to_string_lossy())).unwrap();
    std::fs::set_permissions(&binary, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let provider = codex(&dir.join("data"), &binary, &dir.join("login"), None);
    let (a, b) = tokio::join!(provider.models(), provider.models());
    let (a, b) = (a.unwrap(), b.unwrap());
    assert_eq!(a, b);
    assert_eq!(a[0].id, "gpt-6-astra");
    assert_eq!(
        std::fs::read_to_string(&counter).unwrap().lines().count(),
        1
    );
    let _ = std::fs::remove_dir_all(&dir);
}

// The shims: each leads to its own fake Codex install (a codex-package.json
// and bin/codex, tests/fakes/fake_codex_install.py). The data folder, where
// the install is found from, is outside the temporary folder (the fake, like
// Codex on Linux, makes no apply_patch link for a CODEX_HOME under it):
// cargo's CARGO_TARGET_TMPDIR, in the target folder.

fn write(file: &Path, text: &str) -> PathBuf {
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(file, text).unwrap();
    std::fs::set_permissions(file, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    file.to_path_buf()
}

fn link(target: &Path, file: &Path) -> PathBuf {
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(target, file).unwrap();
    file.to_path_buf()
}

fn install(folder: &Path) -> PathBuf {
    write(
        &folder.join("codex-package.json"),
        "{\"layoutVersion\":1,\"version\":\"0.167.1\",\"variant\":\"codex\",\"entrypoint\":\"bin/codex\"}",
    );
    let fake = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fakes/fake_codex_install.py"),
    )
    .unwrap();
    write(&folder.join("bin/codex"), &fake);
    std::fs::canonicalize(folder).unwrap()
}

/// One program that starts the tool it is named as (mise's).
fn multiplexer(file: &Path, program: &Path) -> PathBuf {
    write(
        file,
        &format!(
            "#!/bin/sh\ncase \"$(basename \"$0\")\" in\n  codex) exec {:?} \"$@\" ;;\nesac\necho \"multiplexer ERROR no tasks defined in $(pwd). Are you in a project directory?\" >&2\nexit 1\n",
            program.to_string_lossy()
        ),
    )
}

/// asdf's shim and the manager it execs.
fn asdf(dir: &Path, name: &str, program: &Path) -> PathBuf {
    let manager = write(
        &dir.join("asdf/bin/asdf"),
        &format!(
            "#!/bin/sh\nif [ \"$1\" = exec ] && [ \"$2\" = codex ]; then shift 2; exec {:?} \"$@\"; fi\necho \"asdf: unknown command\" >&2\nexit 1\n",
            program.to_string_lossy()
        ),
    );
    write(
        &dir.join("shims").join(name),
        &format!(
            "#!/bin/sh\nexec {:?} exec \"codex\" \"$@\"\n",
            manager.to_string_lossy()
        ),
    )
}

/// npm's global `codex`: a symlink to a launcher that starts the platform
/// package's program as a child.
fn npm(dir: &Path) -> (PathBuf, PathBuf) {
    let pkg = dir.join("lib/node_modules/@openai/codex");
    let folder = install(&pkg.join("vendor/x86_64-unknown-linux-musl"));
    write(
        &pkg.join("bin/codex.py"),
        &format!(
            "#!/usr/bin/env python3\nimport subprocess, sys\nsys.exit(subprocess.call([{:?}] + sys.argv[1:]))\n",
            folder.join("bin/codex").to_string_lossy()
        ),
    );
    let shim = link(
        Path::new("../lib/node_modules/@openai/codex/bin/codex.py"),
        &dir.join("bin/codex"),
    );
    (shim, folder)
}

/// The shims' scratch, and its real path (the fakes compare real paths).
fn shims_root() -> (crate::scratch::Scratch, PathBuf) {
    let root = crate::scratch::Scratch::new("codex-shims");
    let real = std::fs::canonicalize(&*root).unwrap();
    (root, real)
}

pub async fn every_kind_of_shim_runs_the_codex_it_leads_to_and_the_sandbox_mounts_that_install() {
    let (_guard, scratch) = shims_root();
    let root = |name: &str| scratch.join(name);
    let mise = install(&root("mise/installs/codex/0.167.1"));
    multiplexer(&root("mise/bin/mise"), &mise.join("bin/codex"));
    let mise_shim = link(&root("mise/bin/mise"), &root("mise/shims/codex"));
    let asdf_install = install(&root("asdf/installs/codex/0.167.1"));
    let asdf_shim = asdf(&root("asdf"), "codex", &asdf_install.join("bin/codex"));
    let (npm_shim, npm_install) = npm(&root("npm"));
    let direct = install(&root("direct"));
    let standalone = install(&root("standalone/releases/0.167.1"));
    let standalone_shim = link(&standalone.join("bin/codex"), &root("standalone/bin/codex"));
    let named = install(&root("named/installs/0.167.1"));
    multiplexer(&root("named/mise"), &named.join("bin/codex"));
    let named_shim = link(&root("named/mise"), &root("named/elsewhere/codex"));
    // Every command by its path: a bare command is found on the PATH the
    // app gives its children, which this test process cannot change.
    let cases = [
        ("a mise-style symlink to a multiplexer", mise_shim, mise),
        ("an asdf-style shell-script shim", asdf_shim, asdf_install),
        (
            "an npm-style symlink to a launcher that starts the program as a child",
            npm_shim,
            npm_install,
        ),
        (
            "the program itself",
            direct.join("bin/codex"),
            direct.clone(),
        ),
        (
            "a symlink to the program (Homebrew, the standalone installer)",
            standalone_shim,
            standalone,
        ),
        (
            "MCBENCH_CODEX_BINARY naming a shim by path",
            named_shim,
            named,
        ),
    ];
    for (kind, binary, folder) in cases {
        let data = scratch.join("data");
        let config = CodexConfig {
            data_dir: data.clone(),
            binary: binary.clone().into(),
            prefix: Vec::new(),
            home: Some(scratch.join("codex-home")),
            bridge: bridge(),
            launch: Some(Launch::Host),
        };
        let provider = CodexCli::new(config.clone(), Arc::new(CodexModels::default()));
        assert_eq!(
            provider.version().await.unwrap(),
            "codex-cli 0.167.1",
            "{kind}"
        );
        assert_eq!(
            provider
                .models()
                .await
                .unwrap()
                .iter()
                .map(|m| m.id.clone())
                .collect::<Vec<_>>(),
            ["fake-model"],
            "{kind}"
        );
        // A turn's launch on the host runs the command, and it is the
        // install's program that runs.
        let home = scratch.join("turn").join("codex-home");
        let out = tokio::process::Command::new(&binary)
            .args(host_args(
                "fake-model",
                None,
                "S",
                ("/x", &["tool-bridge".to_owned(), "/s".to_owned()]),
                false,
                None,
                &[],
            ))
            .env_clear()
            .envs(config.host_env(Some(&home)))
            .stdin(std::process::Stdio::null())
            .output()
            .await
            .unwrap();
        let first: Value = serde_json::from_str(
            String::from_utf8_lossy(&out.stdout)
                .lines()
                .next()
                .unwrap_or("null"),
        )
        .unwrap();
        assert_eq!(
            first,
            json!({ "type": "fake.ran", "executable": folder.join("bin/codex").to_string_lossy() }),
            "{kind}"
        );
        // The sandbox mounts the install the command runs and starts its program.
        let found = codex_install(&config).await.unwrap();
        assert_eq!(
            (found.folder.clone(), found.program.as_str()),
            (folder.clone(), "/opt/codex/bin/codex"),
            "{kind}"
        );
    }
    let _ = std::fs::remove_dir_all(&scratch);
}

pub async fn a_shim_that_leads_nowhere_fails_naming_the_command_and_a_program_outside_an_install_is_not_mounted()
 {
    let (_guard, scratch) = shims_root();
    let nowhere = scratch.join("nowhere");
    multiplexer(
        &nowhere.join("mise"),
        &nowhere.join("installs/codex/bin/codex"),
    );
    let shim = link(&nowhere.join("mise"), &nowhere.join("shims/codex"));
    let config = CodexConfig {
        data_dir: scratch.join("data"),
        binary: shim.clone().into(),
        prefix: Vec::new(),
        home: Some(scratch.join("codex-home")),
        bridge: bridge(),
        launch: Some(Launch::Host),
    };
    let provider = CodexCli::new(config.clone(), Arc::new(CodexModels::default()));
    let line = shim.to_string_lossy().into_owned();
    assert!(
        provider
            .version()
            .await
            .unwrap_err()
            .starts_with(&format!("Command failed: {line} --version"))
    );
    assert!(
        provider
            .models()
            .await
            .unwrap_err()
            .message
            .starts_with(&format!("Command failed: {line} debug models"))
    );
    assert!(
        codex_install(&config)
            .await
            .unwrap_err()
            .starts_with(&format!("Command failed: {line} --version"))
    );
    // A Codex with no codex-package.json above it.
    let fake = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fakes/fake_codex_install.py"),
    )
    .unwrap();
    let bare = write(&scratch.join("bare/bin/codex"), &fake);
    let config = CodexConfig {
        binary: bare.clone().into(),
        prefix: Vec::new(),
        ..config
    };
    assert_eq!(
        CodexCli::new(config.clone(), Arc::new(CodexModels::default()))
            .version()
            .await
            .unwrap(),
        "codex-cli 0.167.1"
    );
    let error = codex_install(&config).await.unwrap_err();
    assert!(
        error.starts_with(&format!(
            "`{} --version` (codex-cli 0.167.1) started {}, which no codex-package.json",
            bare.to_string_lossy(),
            bare.to_string_lossy()
        )),
        "{error}"
    );
    let _ = std::fs::remove_dir_all(&scratch);
    let _ = (
        small_tool,
        codex_cli::NAME,
        BridgeCommand {
            program: PathBuf::new(),
        },
        Fake::new,
    );
}
