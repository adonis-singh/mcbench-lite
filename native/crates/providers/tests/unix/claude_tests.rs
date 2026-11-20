//! `test/claude-code.test.js` (21) and `claude-code-transcript.test.js` (2)
//! ported: the same cases against the scripted fake CLI, which plays the
//! test's stand-in `claude` (it announces mcbench's tools, calls `build` over
//! the bridge and streams the reply).

use std::path::Path;

use mcbench_providers::harness::claude_code::{
    self, ClaudeCode, ClaudeConfig, check_init, claude_args, cwd, stream_input, usage_of,
};
use mcbench_providers::harness::{self, PromptImage, read_session, write_session};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use super::claude_tests;
use super::{
    Deltas, Fake, Req, Test, as_json, bridge, claude, complete_claude, digest, flag, scratch, tests,
};

pub fn all() -> Vec<Test> {
    tests![
        claude_tests::arguments_remove_built_in_tools_pin_settings_off_and_carry_the_session_mode,
        claude_tests::a_completion_with_no_conversation_runs_with_its_system_prompt_in_place_and_saves_no_session,
        claude_tests::the_init_event_must_list_only_mcbench_tools_served_by_a_connected_server,
        claude_tests::a_first_turn_starts_a_saved_session_relays_its_tool_call_and_records_the_session,
        claude_tests::a_follow_up_resumes_the_recorded_session_with_only_the_new_request,
        claude_tests::a_follow_up_turn_is_written_to_the_running_process_instead_of_starting_another,
        claude_tests::after_a_revert_the_session_starts_over_from_the_rendered_transcript,
        claude_tests::a_built_in_tool_in_the_init_event_fails_the_turn_and_stops_claude_code,
        claude_tests::stream_events_become_status_lines_notices_a_task_list_and_usage_limit_state,
        claude_tests::a_turn_stopped_while_its_mcp_server_starts_never_spawns_claude_code_and_leaves_no_files,
        claude_tests::a_result_that_cannot_be_read_fails_the_turn_and_the_next_turn_resumes_the_session_its_init_recorded,
        claude_tests::a_turn_stopped_mid_turn_leaves_its_session_recorded_from_init_and_the_next_turn_resumes_it,
        claude_tests::a_session_recorded_under_a_chats_own_folder_is_not_resumed,
        claude_tests::a_changed_resolution_keeps_the_session_and_other_instructions_start_a_new_one,
        claude_tests::a_resumed_session_is_given_the_reply_the_user_stopped_while_it_streamed,
        claude_tests::every_run_starts_with_the_app_named_as_its_entrypoint,
        claude_tests::every_run_works_in_the_one_folder_outside_any_git_work_tree,
        claude_tests::archiving_a_conversation_kills_its_warm_process_and_shutdown_kills_every_one,
        claude_tests::a_follow_up_turn_forgets_tool_use_blocks_whose_mcp_call_never_came,
        claude_tests::a_process_that_cannot_be_spawned_has_exited_and_nothing_waits_for_it,
        claude_tests::claude_code_files_a_session_of_the_shared_working_directory_under_its_dashed_name,
        claude_tests::deleting_a_chat_deletes_its_session_transcript_once_its_process_has_exited_and_no_other_file,
        claude_tests::the_seams_complete_resumes_and_records_the_session_with_its_telemetry,
    ]
}

const BUILTINS: [&str; 4] = ["TaskCreate", "TaskUpdate", "TaskList", "TaskGet"];

fn allowed(chat: bool, extra: &[&str]) -> Vec<String> {
    let mut tools = vec!["mcp__mcbench__build".to_owned()];
    if chat {
        tools.extend(BUILTINS.iter().map(|s| (*s).to_owned()));
    }
    tools.extend(extra.iter().map(|s| (*s).to_owned()));
    tools
}

fn emit(event: Value) -> Value {
    json!({ "emit": event })
}

/// The test's stand-in turn: init, the thinking, the streamed `build` call,
/// its MCP call, the reply `Built: <result>`, the result.
fn turn(tools: &[String], tool_use: &str, usage: bool) -> Value {
    let input = json!({ "summary": "s", "program": "p" });
    let mut result = json!({ "type": "result", "subtype": "success", "is_error": false, "session_id": "sess-1", "result": "Built", "total_cost_usd": 0.01 });
    if usage {
        result["usage"] = json!({ "input_tokens": 6, "cache_creation_input_tokens": 7381, "cache_read_input_tokens": 14411, "output_tokens": 108, "output_tokens_details": { "thinking_tokens": 1 }, "iterations": [] });
    }
    json!([
        emit(json!({ "type": "system", "subtype": "init", "tools": tools, "mcp_servers": [{ "name": "mcbench", "status": "connected" }], "session_id": "sess-1" })),
        emit(json!({ "type": "stream_event", "event": { "type": "message_start", "message": {} } })),
        emit(json!({ "type": "stream_event", "event": { "type": "content_block_delta", "index": 0, "delta": { "type": "thinking_delta", "thinking": "plan" } } })),
        emit(json!({ "type": "stream_event", "event": { "type": "content_block_start", "index": 1, "content_block": { "type": "tool_use", "id": tool_use, "name": "mcp__mcbench__build", "input": {} } } })),
        emit(json!({ "type": "stream_event", "event": { "type": "content_block_delta", "index": 1, "delta": { "type": "input_json_delta", "partial_json": "{\"summary\":\"s\"," } } })),
        emit(json!({ "type": "stream_event", "event": { "type": "content_block_delta", "index": 1, "delta": { "type": "input_json_delta", "partial_json": "\"program\":\"p\"}" } } })),
        emit(json!({ "type": "assistant", "message": { "content": [{ "type": "tool_use", "id": tool_use, "name": "mcp__mcbench__build", "input": input }], "usage": { "input_tokens": 4, "cache_creation_input_tokens": 7264, "cache_read_input_tokens": 7147, "output_tokens": 60 } } })),
        { "call": { "name": "build", "arguments": input, "meta": { "claudecode/toolUseId": tool_use } }, "as": "r1" },
        { "reply": "r1", "prefix": "Built: " },
        emit(json!({ "type": "assistant", "message": { "content": [{ "type": "text", "text": "Built" }], "usage": { "input_tokens": 2, "cache_creation_input_tokens": 117, "cache_read_input_tokens": 7264, "output_tokens": 48 } } })),
        emit(result),
    ])
}

/// A script whose every process takes `turns` of the stand-in turn.
fn script(tools: &[String]) -> Value {
    let spawn = json!({ "mcp": true, "turns": (0..6).map(|i| turn(tools, &format!("toolu_{}", i + 1), true)).collect::<Vec<_>>() });
    json!({ "mode": "claude", "spawns": [spawn] })
}

fn fixture(name: &str, chat: bool) -> (crate::scratch::Scratch, Fake, ClaudeCode) {
    let dir = scratch(&format!("claude-{name}"));
    let fake = Fake::new(&dir.join("fake"), "claude", &script(&allowed(chat, &[])));
    let provider = claude(&dir.join("data"), &fake.program);
    (dir, fake, provider)
}

fn system(text: &str) -> Value {
    json!({ "role": "system", "content": text })
}

fn user(text: &str) -> Value {
    json!({ "role": "user", "content": text })
}

fn m(id: &str, role: &str) -> Value {
    json!({ "id": id, "role": role, "content": "" })
}

fn terms() -> Value {
    json!({ "instructions": digest("You build."), "resolution": digest("") })
}

fn hut() -> (Value, Value) {
    (
        json!([m("u1", "user")]),
        json!([system("You build."), user("Build a hut.")]),
    )
}

fn record_of(extra: Value) -> serde_json::Map<String, Value> {
    let mut record = json!({ "sessionId": "sess-1", "cwd": cwd().to_string_lossy() });
    for (k, v) in terms().as_object().unwrap() {
        record[k] = v.clone();
    }
    for (k, v) in extra.as_object().unwrap() {
        record[k] = v.clone();
    }
    record.as_object().unwrap().clone()
}

/// One whole turn: its tool call answered with `ok`, then its reply.
async fn run_turn(provider: &ClaudeCode, req: Req) -> Value {
    let cancel = CancellationToken::new();
    let deltas = Deltas::default();
    let first = complete_claude(provider, &req.build(), &deltas, &cancel)
        .await
        .unwrap();
    let mut messages = req.messages.as_array().unwrap().clone();
    let call = as_json(&first);
    messages.push(json!({ "role": "assistant", "content": call["content"], "tool_calls": call["tool_calls"] }));
    messages.push(
        json!({ "role": "tool", "tool_call_id": call["tool_calls"][0]["id"], "content": "ok" }),
    );
    let next = Req {
        messages: Value::Array(messages),
        ..req
    };
    let reply = complete_claude(provider, &next.build(), &deltas, &cancel)
        .await
        .unwrap();
    if let Some((id, _)) = &next.conversation {
        provider.close_chat(id);
    }
    as_json(&reply)
}

pub async fn arguments_remove_built_in_tools_pin_settings_off_and_carry_the_session_mode() {
    let allowed_tools = vec!["mcp__mcbench__build".to_owned()];
    let bridge_args = vec!["tool-bridge".to_owned(), "/tmp/x.sock".to_owned()];
    let args = |effort: Option<&str>, session: Option<(&str, bool)>, one_shot: bool| {
        claude_args(
            "claude-opus-5-5",
            effort,
            "S",
            &allowed_tools,
            ("/app/mcbench-lite", &bridge_args),
            session,
            one_shot,
            &[],
        )
    };
    let fresh = args(Some("high"), Some(("abc", false)), false);
    assert_eq!(
        &fresh[..5],
        [
            "-p",
            "--output-format",
            "stream-json",
            "--verbose",
            "--model"
        ]
    );
    assert_eq!(flag(&fresh, "--effort"), Some("high"));
    assert_eq!(flag(&fresh, "--thinking-display"), Some("summarized"));
    assert_eq!(flag(&fresh, "--tools"), Some(""));
    assert!(
        fresh.contains(&"--strict-mcp-config".to_owned())
            && fresh.contains(&"--disable-slash-commands".to_owned())
    );
    assert_eq!(flag(&fresh, "--setting-sources"), Some(""));
    assert_eq!(
        serde_json::from_str::<Value>(flag(&fresh, "--mcp-config").unwrap()).unwrap(),
        json!({ "mcpServers": { "mcbench": { "type": "stdio", "command": "/app/mcbench-lite", "args": ["tool-bridge", "/tmp/x.sock"] } } })
    );
    assert_eq!(flag(&fresh, "--session-id"), Some("abc"));
    assert_eq!(flag(&fresh, "--input-format"), Some("stream-json"));
    assert!(fresh.contains(&"--include-partial-messages".to_owned()));
    assert_eq!(
        flag(&args(Some("high"), Some(("abc", true)), false), "--resume"),
        Some("abc")
    );
    let batch = args(None, None, false);
    assert!(batch.contains(&"--no-session-persistence".to_owned()));
    assert!(
        !batch.contains(&"--effort".to_owned())
            && !batch.contains(&"--input-format".to_owned())
            && !batch.contains(&"--session-id".to_owned())
    );
    assert_eq!(batch[batch.len() - 1], "S");
    assert_eq!(batch[batch.len() - 2], "--append-system-prompt");
    let one_shot = args(Some("high"), None, true);
    assert!(
        one_shot.contains(&"--no-session-persistence".to_owned())
            && one_shot.contains(&"--include-partial-messages".to_owned())
    );
    assert_eq!(flag(&one_shot, "--input-format"), Some("stream-json"));
    assert!(
        !one_shot
            .iter()
            .any(|a| a == "--session-id" || a == "--resume" || a == "--append-system-prompt")
    );
    assert_eq!(&one_shot[one_shot.len() - 2..], ["--system-prompt", "S"]);
    let config = ClaudeConfig {
        data_dir: "/d".into(),
        binary: "claude".into(),
        home: None,
        bridge: bridge(),
    };
    let env: std::collections::HashMap<String, String> = config
        .env()
        .into_iter()
        .map(|(k, v)| {
            (
                k.to_string_lossy().into_owned(),
                v.to_string_lossy().into_owned(),
            )
        })
        .collect();
    assert_eq!(env.get("ANTHROPIC_API_KEY"), None);
    assert_eq!(
        env.get("MCP_TOOL_TIMEOUT").map(String::as_str),
        Some("2147483647")
    );
    assert_eq!(
        env.get("CLAUDE_CODE_ENABLE_FINE_GRAINED_TOOL_STREAMING")
            .map(String::as_str),
        Some("1")
    );
    assert_eq!(
        env.get("CLAUDE_CODE_DISABLE_AUTO_MEMORY")
            .map(String::as_str),
        Some("1")
    );
}

pub async fn a_completion_with_no_conversation_runs_with_its_system_prompt_in_place_and_saves_no_session()
 {
    // A one-shot run has no chat to relay a tool call through (each
    // completion without a conversation starts a run, today as here), so the
    // stand-in answers it without one.
    let dir = scratch("claude-one-shot");
    let tools = allowed(false, &[]);
    let all = turn(&tools, "toolu_1", true);
    let steps = json!([
        all[0],
        emit(
            json!({ "type": "stream_event", "event": { "type": "content_block_delta", "index": 0, "delta": { "type": "text_delta", "text": "Stone bench" } } })
        ),
        all[9],
        all[10],
    ]);
    let fake = Fake::new(
        &dir.join("fake"),
        "claude",
        &json!({ "mode": "claude", "spawns": [{ "mcp": true, "turns": [steps] }] }),
    );
    let provider = claude(&dir.join("data"), &fake.program);
    let req = Req::new(json!([
        system("Name this chat in 2 to 6 words."),
        user("Build a tiny stone bench, three blocks wide.")
    ]));
    let reply = complete_claude(
        &provider,
        &req.build(),
        &Deltas::default(),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(reply["content"], "Stone bench");
    let args = fake.args();
    assert_eq!(
        flag(&args, "--system-prompt"),
        Some("Name this chat in 2 to 6 words.")
    );
    assert!(!args.contains(&"--append-system-prompt".to_owned()));
    assert!(
        args.contains(&"--no-session-persistence".to_owned())
            && !args.contains(&"--session-id".to_owned())
    );
    assert_eq!(
        fake.input()["message"]["content"],
        json!([{ "type": "text", "text": "Build a tiny stone bench, three blocks wide." }])
    );
    provider.shutdown().await;
    let _ = std::fs::remove_dir_all(&dir);
}

pub async fn the_init_event_must_list_only_mcbench_tools_served_by_a_connected_server() {
    let allowed = vec![
        "mcp__mcbench__build".to_owned(),
        "mcp__mcbench__bash".to_owned(),
    ];
    let connected = json!([{ "name": "mcbench", "status": "connected" }]);
    check_init(
        &json!({ "tools": ["mcp__mcbench__build"], "mcp_servers": connected }),
        &allowed,
        &[],
    )
    .unwrap();
    assert_eq!(
        check_init(
            &json!({ "tools": ["mcp__mcbench__build", "Bash"], "mcp_servers": connected }),
            &allowed,
            &[]
        )
        .unwrap_err(),
        "Claude Code exposed tools outside mcbench: Bash"
    );
    assert_eq!(check_init(&json!({ "tools": allowed, "mcp_servers": [{ "name": "mcbench", "status": "failed" }] }), &allowed, &[]).unwrap_err(), "mcbench MCP server status: failed");
    assert_eq!(
        usage_of(&json!({ "usage": { "input_tokens": 1, "output_tokens": 2, "output_tokens_details": { "thinking_tokens": 5 } } })).unwrap(),
        json!({ "input_tokens": 1, "output_tokens": 2, "output_tokens_details": { "reasoning_tokens": 5 } })
    );
    let input = stream_input(
        "hi",
        &[PromptImage {
            mime_type: "image/png".to_owned(),
            data: "AA==".to_owned(),
        }],
    );
    assert_eq!(
        serde_json::from_str::<Value>(&input).unwrap(),
        json!({ "type": "user", "message": { "role": "user", "content": [{ "type": "text", "text": "hi" }, { "type": "image", "source": { "type": "base64", "media_type": "image/png", "data": "AA==" } }] } })
    );
}

pub async fn a_first_turn_starts_a_saved_session_relays_its_tool_call_and_records_the_session() {
    let (dir, fake, provider) = fixture("first", true);
    let (transcript, messages) = hut();
    let mut req = Req::new(messages).chat("conv-1", Some(transcript));
    req.effort = Some("high".to_owned());
    req.chat = true;
    let cancel = CancellationToken::new();
    let deltas = Deltas::default();
    let first = as_json(
        &complete_claude(&provider, &req.build(), &deltas, &cancel)
            .await
            .unwrap(),
    );
    let seen = deltas.take();
    // Told once the CLI reported its start (init), before its first output.
    assert_eq!(
        seen.iter().filter(|d| d.get("started").is_some()).count(),
        1
    );
    assert_eq!(seen[0], json!({ "started": true }));
    assert_eq!(first["reasoning"], "plan");
    assert_eq!(first["finish_reason"], "tool_calls");
    assert_eq!(first["tool_calls"][0]["function"]["name"], "build");
    let id = first["tool_calls"][0]["id"].as_str().unwrap().to_owned();
    assert!(id.starts_with("toolu_"));
    assert_eq!(
        serde_json::from_str::<Value>(
            first["tool_calls"][0]["function"]["arguments"]
                .as_str()
                .unwrap()
        )
        .unwrap(),
        json!({ "summary": "s", "program": "p" })
    );
    let keys: Vec<String> = seen[1..]
        .iter()
        .map(|d| d.as_object().unwrap().keys().next().unwrap().clone())
        .collect();
    assert_eq!(keys, ["reasoning", "toolCall", "toolCall", "toolCall"]);
    let streamed: Vec<&Value> = seen[2..].iter().map(|d| &d["toolCall"]).collect();
    assert!(
        streamed
            .iter()
            .all(|c| c["id"] == id.as_str() && c["name"] == "build" && c["index"] == 1)
    );
    assert_eq!(
        streamed
            .iter()
            .map(|c| c["args"].as_str().unwrap())
            .collect::<String>(),
        "{\"summary\":\"s\",\"program\":\"p\"}"
    );
    let mut messages = req.messages.as_array().unwrap().clone();
    messages.push(json!({ "role": "assistant", "content": "", "tool_calls": first["tool_calls"] }));
    messages.push(json!({ "role": "tool", "tool_call_id": id, "content": "{\"ok\":true}" }));
    let next = Req {
        messages: Value::Array(messages),
        ..req
    };
    let last = as_json(
        &complete_claude(&provider, &next.build(), &deltas, &cancel)
            .await
            .unwrap(),
    );
    assert_eq!(last["content"], "Built: {\"ok\":true}");
    assert_eq!(last["finish_reason"], "stop");
    assert_eq!(
        last["usage"],
        json!({ "input_tokens": 6, "cache_creation_input_tokens": 7381, "cache_read_input_tokens": 14411, "output_tokens": 108, "iterations": [], "output_tokens_details": { "reasoning_tokens": 1 } })
    );
    // The context reading is the latest request's input, not the sum.
    assert_eq!(first["contextTokens"], 14415);
    assert_eq!(last["contextTokens"], 7383);
    let args = fake.args();
    let session = flag(&args, "--session-id").unwrap();
    assert!(session.len() == 36 && session.chars().all(|c| c.is_ascii_hexdigit() || c == '-'));
    assert_eq!(flag(&args, "--model"), Some("claude-opus-5-5"));
    assert_eq!(flag(&args, "--effort"), Some("high"));
    assert_eq!(flag(&args, "--append-system-prompt"), Some("You build."));
    assert_eq!(
        flag(&args, "--allowedTools"),
        Some("mcp__mcbench__build,TaskCreate,TaskUpdate,TaskList,TaskGet")
    );
    assert_eq!(
        flag(&args, "--tools"),
        Some("TaskCreate,TaskUpdate,TaskList,TaskGet")
    );
    assert_eq!(
        fake.input()["message"]["content"],
        json!([{ "type": "text", "text": "Build a hut." }])
    );
    assert_eq!(
        read_session(&dir.join("data"), "conv-1", "claude-code").unwrap(),
        Some(record_of(json!({ "lastSeen": "u1", "tasks": [] })))
    );
    provider.shutdown().await;
    let _ = std::fs::remove_dir_all(&dir);
}

pub async fn a_follow_up_resumes_the_recorded_session_with_only_the_new_request() {
    let (dir, fake, provider) = fixture("follow-up", false);
    write_session(
        &dir.join("data"),
        "conv-2",
        "claude-code",
        &record_of(json!({ "lastSeen": "u1", "tasks": [] })),
    )
    .unwrap();
    let transcript = json!([m("u1", "user"), m("a1", "assistant"), m("u2", "user")]);
    let messages = json!([system("You build."), user("Build a hut."), { "role": "assistant", "content": "Built" }, user("Make it taller.")]);
    run_turn(
        &provider,
        Req::new(messages).chat("conv-2", Some(transcript)),
    )
    .await;
    let args = fake.args();
    assert_eq!(flag(&args, "--resume"), Some("sess-1"));
    assert!(!args.contains(&"--session-id".to_owned()) && !args.contains(&"--effort".to_owned()));
    assert_eq!(
        fake.input()["message"]["content"],
        json!([{ "type": "text", "text": "Make it taller." }])
    );
    assert_eq!(
        read_session(&dir.join("data"), "conv-2", "claude-code").unwrap(),
        Some(record_of(json!({ "lastSeen": "u2", "tasks": [] })))
    );
    provider.shutdown().await;
    let _ = std::fs::remove_dir_all(&dir);
}

fn later(transcript: &Value, ids: &[(&str, &str)]) -> Value {
    let mut out = transcript.as_array().unwrap().clone();
    out.extend(ids.iter().map(|(id, role)| m(id, role)));
    Value::Array(out)
}

pub async fn a_follow_up_turn_is_written_to_the_running_process_instead_of_starting_another() {
    let (dir, fake, provider) = fixture("warm", false);
    let (first, messages) = hut();
    run_turn(
        &provider,
        Req::new(messages).chat("conv-warm", Some(first.clone())),
    )
    .await;
    let second = later(&first, &[("a1", "assistant"), ("u2", "user")]);
    let reply = run_turn(&provider, Req::new(json!([system("You build."), user("Build a hut."), { "role": "assistant", "content": "Built" }, user("Make it taller.")])).chat("conv-warm", Some(second.clone()))).await;
    assert_eq!(reply["content"], "Built: ok");
    assert_eq!(fake.spawns(), 1);
    assert_eq!(
        fake.input()["message"]["content"],
        json!([{ "type": "text", "text": "Make it taller." }])
    );
    assert_eq!(
        read_session(&dir.join("data"), "conv-warm", "claude-code").unwrap(),
        Some(record_of(json!({ "lastSeen": "u2", "tasks": [] })))
    );
    // Other settings (here the model) need a process started with them.
    let third = later(&second, &[("a2", "assistant"), ("u3", "user")]);
    let mut req =
        Req::new(json!([system("You build."), user("Add a door.")])).chat("conv-warm", Some(third));
    req.model = "claude-sonnet-5-5".to_owned();
    run_turn(&provider, req).await;
    assert_eq!(fake.spawns(), 2);
    assert_eq!(flag(&fake.args(), "--resume"), Some("sess-1"));
    provider.shutdown().await;
    let _ = std::fs::remove_dir_all(&dir);
}

pub async fn after_a_revert_the_session_starts_over_from_the_rendered_transcript() {
    let (dir, fake, provider) = fixture("revert", false);
    write_session(
        &dir.join("data"),
        "conv-3",
        "claude-code",
        &record_of(json!({ "sessionId": "sess-old", "lastSeen": "gone", "tasks": [] })),
    )
    .unwrap();
    let transcript = json!([m("u1", "user"), m("a1", "assistant"), m("u9", "user")]);
    run_turn(&provider, Req::new(json!([system("You build."), user("Build a hut."), { "role": "assistant", "content": "Built" }, user("Add a door.")])).chat("conv-3", Some(transcript))).await;
    let args = fake.args();
    assert!(args.contains(&"--session-id".to_owned()) && !args.contains(&"--resume".to_owned()));
    assert_ne!(flag(&args, "--session-id"), Some("sess-old"));
    assert_eq!(
        fake.input()["message"]["content"][0]["text"],
        "<conversation>\nUser:\nBuild a hut.\n\nAssistant:\nBuilt\n</conversation>\n\nAdd a door."
    );
    provider.shutdown().await;
    let _ = std::fs::remove_dir_all(&dir);
}

pub async fn a_built_in_tool_in_the_init_event_fails_the_turn_and_stops_claude_code() {
    let dir = scratch("claude-extra-tool");
    let fake = Fake::new(
        &dir.join("fake"),
        "claude",
        &json!({ "mode": "claude", "spawns": [{ "mcp": true, "turns": [turn(&allowed(false, &["Bash"]), "toolu_1", true)] }] }),
    );
    let provider = claude(&dir.join("data"), &fake.program);
    let error = complete_claude(
        &provider,
        &Req::new(json!([user("go")])).build(),
        &Deltas::default(),
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert_eq!(
        error.message,
        "Claude Code exposed tools outside mcbench: Bash"
    );
    super::until("the stopped process has exited", || {
        provider.live_processes() == 0
    })
    .await;
    let _ = std::fs::remove_dir_all(&dir);
}

pub async fn stream_events_become_status_lines_notices_a_task_list_and_usage_limit_state() {
    let dir = scratch("claude-events");
    let tools = allowed(true, &[]);
    let created = |id: &str, subject: &str, call: &str| {
        emit(
            json!({ "type": "user", "message": { "content": [{ "type": "tool_result", "tool_use_id": call }] }, "tool_use_result": { "task": { "id": id, "subject": subject } } }),
        )
    };
    let update = |call: &str, input: Value| {
        let task = input["taskId"].clone();
        vec![
            emit(
                json!({ "type": "assistant", "message": { "content": [{ "type": "tool_use", "id": call, "name": "TaskUpdate", "input": input }], "usage": { "input_tokens": 1, "cache_creation_input_tokens": 0, "cache_read_input_tokens": 0, "output_tokens": 1 } } }),
            ),
            emit(
                json!({ "type": "user", "message": { "content": [{ "type": "tool_result", "tool_use_id": call }] }, "tool_use_result": { "success": true, "taskId": task } }),
            ),
        ]
    };
    let mut steps = vec![
        emit(
            json!({ "type": "system", "subtype": "init", "tools": tools, "mcp_servers": [{ "name": "mcbench", "status": "connected" }], "session_id": "sess-1" }),
        ),
        emit(json!({ "type": "system", "subtype": "status", "status": "compacting" })),
        emit(
            json!({ "type": "system", "subtype": "api_retry", "attempt": 2, "max_retries": 10, "error_status": 529 }),
        ),
        emit(json!({ "type": "system", "subtype": "status", "status": "requesting" })),
        emit(json!({ "type": "system", "subtype": "compact_boundary" })),
        emit(
            json!({ "type": "system", "subtype": "model_fallback", "level": "warning", "content": "Switched to Claude Sonnet 5.5" }),
        ),
        emit(
            json!({ "type": "rate_limit_event", "rate_limit_info": { "status": "allowed_warning", "rateLimitType": "five_hour", "resetsAt": 1791118200 } }),
        ),
        created("1", "Lay the floor", "c1"),
        created("2", "Raise the walls", "c2"),
        created("3", "Roof", "c3"),
    ];
    steps.extend(update(
        "u1",
        json!({ "taskId": "1", "status": "completed" }),
    ));
    steps.extend(update(
        "u2",
        json!({ "taskId": "2", "status": "in_progress", "activeForm": "Raising the walls" }),
    ));
    steps.extend(update(
        "u3",
        json!({ "taskId": "9", "status": "completed" }),
    ));
    for (input, written, read) in [(3, 120, 7026), (2, 117, 7147), (2, 117, 7264)] {
        steps.push(emit(json!({ "type": "assistant", "message": { "content": [{ "type": "text", "text": "step" }], "usage": { "input_tokens": input, "cache_creation_input_tokens": written, "cache_read_input_tokens": read, "output_tokens": 10 } } })));
    }
    steps.push(emit(json!({ "type": "result", "subtype": "success", "is_error": false, "session_id": "sess-1", "usage": { "input_tokens": 1 } })));
    let fake = Fake::new(
        &dir.join("fake"),
        "claude",
        &json!({ "mode": "claude", "spawns": [{ "mcp": true, "turns": [steps] }] }),
    );
    let provider = claude(&dir.join("data"), &fake.program);
    let mut req = Req::new(json!([user("go")]));
    req.chat = true;
    let deltas = Deltas::default();
    let reply = as_json(
        &complete_claude(&provider, &req.build(), &deltas, &CancellationToken::new())
            .await
            .unwrap(),
    );
    let seen = deltas.take();
    let status: Vec<&Value> = seen.iter().filter_map(|d| d.get("status")).collect();
    assert_eq!(
        status,
        [
            &json!("Compacting context"),
            &json!("Retrying after HTTP 529 (attempt 2 of 10)"),
            &Value::Null
        ]
    );
    let notices: Vec<&Value> = seen.iter().filter_map(|d| d.get("notice")).collect();
    assert_eq!(
        notices,
        [
            &json!({ "level": "info", "text": "Claude Code compacted the conversation to fit its context window" }),
            &json!({ "level": "warning", "text": "Switched to Claude Sonnet 5.5" })
        ]
    );
    let plan = seen
        .iter()
        .filter_map(|d| d.get("plan"))
        .next_back()
        .unwrap();
    assert_eq!(
        *plan,
        json!([{ "text": "Lay the floor", "status": "done" }, { "text": "Raise the walls", "status": "active" }, { "text": "Roof", "status": "pending" }])
    );
    assert_eq!(reply["rateLimit"]["status"], "allowed_warning");
    // Each request's assistant event overwrites the reading.
    assert_eq!(reply["contextTokens"], 7383);
    assert_eq!(reply["plan"].as_array().unwrap().len(), 3);
    provider.shutdown().await;
    let _ = std::fs::remove_dir_all(&dir);
}

pub async fn a_turn_stopped_while_its_mcp_server_starts_never_spawns_claude_code_and_leaves_no_files()
 {
    let (dir, fake, provider) = fixture("stop-start", false);
    // On a runtime of one thread: the stop comes while the bridge starts,
    // as `controller.abort()` right after `session.start` does today.
    let data = dir.join("data");
    let program = fake.program.clone();
    let outcome = tokio::task::spawn_blocking(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async move {
            let provider = std::sync::Arc::new(claude(&data, &program));
            let cancel = CancellationToken::new();
            let (transcript, messages) = hut();
            let request = Req::new(messages)
                .chat("conv-stop", Some(transcript))
                .build();
            let p = provider.clone();
            let c = cancel.clone();
            let started =
                tokio::spawn(
                    async move { complete_claude(&p, &request, &Deltas::default(), &c).await },
                );
            // Until the process waits for its bridge (`spawning`).
            while provider.process_state("conv-stop") != Some("spawning") {
                tokio::task::yield_now().await;
            }
            cancel.cancel();
            let error = started.await.unwrap().unwrap_err();
            (error.message, provider.live_processes())
        })
    })
    .await
    .unwrap();
    assert_eq!(
        outcome,
        ("Claude Code was stopped while starting".to_owned(), 0)
    );
    super::sleep(200).await;
    assert_eq!(fake.spawns(), 0);
    assert_eq!(
        read_session(&dir.join("data"), "conv-stop", "claude-code").unwrap(),
        None
    );
    drop(provider);
    let _ = std::fs::remove_dir_all(&dir);
}

pub async fn a_result_that_cannot_be_read_fails_the_turn_and_the_next_turn_resumes_the_session_its_init_recorded()
 {
    let dir = scratch("claude-bad-result");
    let tools = allowed(false, &[]);
    let fake = Fake::new(
        &dir.join("fake"),
        "claude",
        &json!({ "mode": "claude", "spawns": [
        { "mcp": true, "turns": [turn(&tools, "toolu_1", false)] },
        { "mcp": true, "turns": [turn(&tools, "toolu_2", true)] },
    ] }),
    );
    let provider = claude(&dir.join("data"), &fake.program);
    let (transcript, messages) = hut();
    let req = Req::new(messages.clone()).chat("conv-bad-result", Some(transcript.clone()));
    let cancel = CancellationToken::new();
    let first = complete_claude(&provider, &req.build(), &Deltas::default(), &cancel)
        .await
        .unwrap();
    let mut next = messages.as_array().unwrap().clone();
    next.push(
        json!({ "role": "assistant", "content": "", "tool_calls": as_json(&first)["tool_calls"] }),
    );
    next.push(json!({ "role": "tool", "tool_call_id": as_json(&first)["tool_calls"][0]["id"], "content": "ok" }));
    let error = complete_claude(
        &provider,
        &Req {
            messages: Value::Array(next),
            ..req
        }
        .build(),
        &Deltas::default(),
        &cancel,
    )
    .await
    .unwrap_err();
    assert_eq!(
        error.message,
        "Cannot destructure property 'output_tokens_details' of 'result.usage' as it is undefined."
    );
    super::until("the failed turn's process has exited", || {
        provider.live_processes() == 0
    })
    .await;
    let failed = later(
        &transcript,
        &[
            ("a1", "assistant"),
            ("t1", "tool"),
            ("e1", "error"),
            ("u2", "user"),
        ],
    );
    run_turn(
        &provider,
        Req::new(json!([
            system("You build."),
            user("Build a hut."),
            user("Try again.")
        ]))
        .chat("conv-bad-result", Some(failed)),
    )
    .await;
    assert_eq!(flag(&fake.args(), "--resume"), Some("sess-1"));
    assert_eq!(
        fake.input()["message"]["content"],
        json!([{ "type": "text", "text": "Try again." }])
    );
    provider.shutdown().await;
    let _ = std::fs::remove_dir_all(&dir);
}

pub async fn a_turn_stopped_mid_turn_leaves_its_session_recorded_from_init_and_the_next_turn_resumes_it()
 {
    let (dir, fake, provider) = fixture("stopped", true);
    let (transcript, messages) = hut();
    let mut req = Req::new(messages).chat("conv-stopped", Some(transcript.clone()));
    req.chat = true;
    let cancel = CancellationToken::new();
    // The tool call is pending: Claude Code has taken the request.
    complete_claude(&provider, &req.build(), &Deltas::default(), &cancel)
        .await
        .unwrap();
    assert_eq!(
        read_session(&dir.join("data"), "conv-stopped", "claude-code").unwrap(),
        Some(record_of(json!({ "lastSeen": "u1", "tasks": [] })))
    );
    cancel.cancel();
    provider.close_chat("conv-stopped");
    super::until("the stopped turn's process has exited", || {
        provider.live_processes() == 0
    })
    .await;
    let mut stopped = transcript.as_array().unwrap().clone();
    stopped.push(m("a1", "assistant"));
    stopped.push(json!({ "id": "s1", "role": "user", "seeBuild": true, "content": [] }));
    stopped.push(m("t1", "tool"));
    stopped.push(
        json!({ "id": "a2", "role": "assistant", "content": "(stopped by user)", "stopped": true }),
    );
    stopped.push(m("u2", "user"));
    let mut next = Req::new(json!([
        system("You build."),
        user("Build a hut."),
        user("Carry on.")
    ]))
    .chat("conv-stopped", Some(Value::Array(stopped)));
    next.chat = true;
    run_turn(&provider, next).await;
    let args = fake.args();
    assert_eq!(flag(&args, "--resume"), Some("sess-1"));
    assert!(!args.contains(&"--session-id".to_owned()));
    assert_eq!(
        fake.input()["message"]["content"],
        json!([{ "type": "text", "text": "Carry on." }])
    );
    assert_eq!(
        read_session(&dir.join("data"), "conv-stopped", "claude-code").unwrap(),
        Some(record_of(json!({ "lastSeen": "u2", "tasks": [] })))
    );
    provider.shutdown().await;
    let _ = std::fs::remove_dir_all(&dir);
}

pub async fn a_session_recorded_under_a_chats_own_folder_is_not_resumed() {
    let (dir, fake, provider) = fixture("old-folder", false);
    let mut old = serde_json::Map::new();
    old.insert("sessionId".to_owned(), json!("sess-old"));
    old.insert("lastSeen".to_owned(), json!("u1"));
    old.insert("tasks".to_owned(), json!([]));
    write_session(&dir.join("data"), "conv-old-folder", "claude-code", &old).unwrap();
    let transcript = json!([m("u1", "user"), m("a1", "assistant"), m("u2", "user")]);
    let reply = run_turn(&provider, Req::new(json!([system("You build."), user("Build a hut."), { "role": "assistant", "content": "Built" }, user("Make it taller.")])).chat("conv-old-folder", Some(transcript))).await;
    assert_eq!(reply["content"], "Built: ok");
    let args = fake.args();
    assert!(args.contains(&"--session-id".to_owned()) && !args.contains(&"--resume".to_owned()));
    assert!(
        fake.input()["message"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .starts_with("<conversation>\nUser:\nBuild a hut.")
    );
    assert_eq!(
        read_session(&dir.join("data"), "conv-old-folder", "claude-code").unwrap(),
        Some(record_of(json!({ "lastSeen": "u2", "tasks": [] })))
    );
    provider.shutdown().await;
    let _ = std::fs::remove_dir_all(&dir);
}

pub async fn a_changed_resolution_keeps_the_session_and_other_instructions_start_a_new_one() {
    let (dir, fake, provider) = fixture("resolution", false);
    let (hut_transcript, hut_messages) = hut();
    run_turn(
        &provider,
        Req::new(hut_messages).chat("conv-resolution", Some(hut_transcript.clone())),
    )
    .await;
    let before = fake.spawns();
    let large = "You build.\n\n## Resolution\nLarger.";
    let second = later(&hut_transcript, &[("a1", "assistant"), ("u2", "user")]);
    run_turn(&provider, Req::new(json!([system(large), user("Build a hut."), { "role": "assistant", "content": "Built" }, user("Make it taller.")])).chat("conv-resolution", Some(second.clone()))).await;
    // The warm process takes the turn: its session keeps its own instructions.
    assert_eq!(fake.spawns(), before);
    let text = fake.input()["message"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(
        text.starts_with("<resolution-change>\nThe chat's resolution has changed."),
        "{text}"
    );
    assert!(
        text.ends_with("\n\n## Resolution\nLarger.\n</resolution-change>\n\nMake it taller."),
        "{text}"
    );
    let mut expected = record_of(json!({ "lastSeen": "u2", "tasks": [] }));
    expected.insert(
        "resolution".to_owned(),
        json!(digest("## Resolution\nLarger.")),
    );
    let mut ordered = serde_json::Map::new();
    for key in [
        "sessionId",
        "cwd",
        "instructions",
        "resolution",
        "lastSeen",
        "tasks",
    ] {
        ordered.insert(key.to_owned(), expected[key].clone());
    }
    assert_eq!(
        read_session(&dir.join("data"), "conv-resolution", "claude-code").unwrap(),
        Some(ordered)
    );
    // Told once.
    let third = later(&second, &[("a2", "assistant"), ("u3", "user")]);
    run_turn(
        &provider,
        Req::new(json!([system(large), user("Add a door.")]))
            .chat("conv-resolution", Some(third.clone())),
    )
    .await;
    assert_eq!(
        fake.input()["message"]["content"],
        json!([{ "type": "text", "text": "Add a door." }])
    );
    // Other instructions: a new session with the history and those instructions.
    let fourth = later(&third, &[("a3", "assistant"), ("u4", "user")]);
    run_turn(&provider, Req::new(json!([system("You build carefully."), user("Build a hut."), { "role": "assistant", "content": "Built" }, user("Add a roof.")])).chat("conv-resolution", Some(fourth))).await;
    let args = fake.args();
    assert!(args.contains(&"--session-id".to_owned()) && !args.contains(&"--resume".to_owned()));
    assert_eq!(
        flag(&args, "--append-system-prompt"),
        Some("You build carefully.")
    );
    assert!(
        fake.input()["message"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .starts_with("<conversation>\nUser:\nBuild a hut.")
    );
    provider.shutdown().await;
    let _ = std::fs::remove_dir_all(&dir);
}

pub async fn a_resumed_session_is_given_the_reply_the_user_stopped_while_it_streamed() {
    let (dir, fake, provider) = fixture("stopped-reply", false);
    write_session(
        &dir.join("data"),
        "conv-stopped-reply",
        "claude-code",
        &record_of(json!({ "lastSeen": "u1", "tasks": [] })),
    )
    .unwrap();
    let transcript = json!([m("u1", "user"), { "id": "a1", "role": "assistant", "content": "(stopped by user)", "stopped": true, "reply": "First the floor, then the" }, m("u2", "user")]);
    run_turn(&provider, Req::new(json!([system("You build."), user("Build a hut."), { "role": "assistant", "content": "(stopped by user)" }, user("continue")])).chat("conv-stopped-reply", Some(transcript))).await;
    assert_eq!(flag(&fake.args(), "--resume"), Some("sess-1"));
    assert_eq!(
        fake.input()["message"]["content"][0]["text"],
        "<stopped-reply>\nThe user stopped your previous reply while you were writing it, and this session does not hold it. This is the text you had written, as the user saw it:\n\nAssistant:\nFirst the floor, then the\n</stopped-reply>\n\ncontinue"
    );
    provider.shutdown().await;
    let _ = std::fs::remove_dir_all(&dir);
}

pub async fn every_run_starts_with_the_app_named_as_its_entrypoint() {
    let config = ClaudeConfig {
        data_dir: "/d".into(),
        binary: "claude".into(),
        home: None,
        bridge: bridge(),
    };
    assert!(
        config
            .env()
            .iter()
            .any(|(k, v)| k == "CLAUDE_CODE_ENTRYPOINT" && v == "mcbench-lite")
    );
}

pub async fn every_run_works_in_the_one_folder_outside_any_git_work_tree() {
    assert_eq!(
        cwd(),
        harness::home_dir()
            .join(".cache")
            .join("mcbench")
            .join("claude-cwd")
    );
    std::fs::create_dir_all(cwd()).unwrap();
    let inside = std::process::Command::new("git")
        .args([
            "-C",
            &cwd().to_string_lossy(),
            "rev-parse",
            "--is-inside-work-tree",
        ])
        .output()
        .unwrap();
    assert!(!inside.status.success());
    let (dir, fake, provider) = fixture("cwd", false);
    let real = |p: &Path| std::fs::canonicalize(p).unwrap();
    let (transcript, messages) = hut();
    run_turn(
        &provider,
        Req::new(messages.clone()).chat("conv-cwd-a", Some(transcript.clone())),
    )
    .await;
    assert_eq!(
        real(Path::new(fake.last_start()["cwd"].as_str().unwrap())),
        real(&cwd())
    );
    run_turn(
        &provider,
        Req::new(messages).chat("conv-cwd-b", Some(transcript)),
    )
    .await;
    assert_eq!(
        real(Path::new(fake.last_start()["cwd"].as_str().unwrap())),
        real(&cwd())
    );
    run_turn(
        &provider,
        Req::new(json!([system("Name this chat."), user("A hut")])),
    )
    .await;
    assert_eq!(
        real(Path::new(fake.last_start()["cwd"].as_str().unwrap())),
        real(&cwd())
    );
    assert!(
        !harness::conversation_dir(&dir.join("data"), "conv-cwd-a")
            .join("claude-cwd")
            .exists()
    );
    provider.shutdown().await;
    let _ = std::fs::remove_dir_all(&dir);
}

pub async fn archiving_a_conversation_kills_its_warm_process_and_shutdown_kills_every_one() {
    let (dir, _fake, provider) = fixture("archive", false);
    let (transcript, messages) = hut();
    run_turn(
        &provider,
        Req::new(messages.clone()).chat("conv-archive", Some(transcript.clone())),
    )
    .await;
    assert_eq!(provider.process_state("conv-archive"), Some("idle"));
    provider.destroy("conv-archive").await.unwrap();
    assert_eq!(provider.process_state("conv-archive"), None);
    assert_eq!(provider.live_processes(), 0);
    run_turn(
        &provider,
        Req::new(messages).chat("conv-shutdown", Some(transcript)),
    )
    .await;
    assert_eq!(provider.process_state("conv-shutdown"), Some("idle"));
    provider.shutdown().await;
    assert_eq!(provider.live_processes(), 0);
    let _ = std::fs::remove_dir_all(&dir);
}

pub async fn a_follow_up_turn_forgets_tool_use_blocks_whose_mcp_call_never_came() {
    let dir = scratch("claude-blocks");
    let tools = allowed(false, &[]);
    let mut stale = turn(&tools, "toolu_1", true).as_array().unwrap().clone();
    // A tool_use block on stdout whose MCP call never comes.
    stale.insert(1, emit(json!({ "type": "assistant", "message": { "content": [{ "type": "tool_use", "id": "toolu_stale", "name": "mcp__mcbench__build", "input": {} }], "usage": { "input_tokens": 1, "cache_creation_input_tokens": 0, "cache_read_input_tokens": 0 } } })));
    let fake = Fake::new(
        &dir.join("fake"),
        "claude",
        &json!({ "mode": "claude", "spawns": [{ "mcp": true, "turns": [stale, turn(&tools, "toolu_2", true)] }] }),
    );
    let provider = claude(&dir.join("data"), &fake.program);
    let (transcript, messages) = hut();
    run_turn(
        &provider,
        Req::new(messages).chat("conv-blocks", Some(transcript.clone())),
    )
    .await;
    let second = later(&transcript, &[("a1", "assistant"), ("u2", "user")]);
    let reply = run_turn(&provider, Req::new(json!([system("You build."), user("Build a hut."), { "role": "assistant", "content": "Built" }, user("Make it taller.")])).chat("conv-blocks", Some(second))).await;
    assert_eq!(reply["content"], "Built: ok");
    assert_eq!(fake.spawns(), 1, "the same process took the turn");
    provider.shutdown().await;
    let _ = std::fs::remove_dir_all(&dir);
}

pub async fn a_process_that_cannot_be_spawned_has_exited_and_nothing_waits_for_it() {
    let (dir, _fake, provider) = fixture("no-spawn", false);
    let (transcript, messages) = hut();
    let mut req = Req::new(messages).chat("conv-no-spawn", Some(transcript));
    // A NUL byte in an argument fails the spawn at once.
    req.model = "claude\0opus".to_owned();
    let error = complete_claude(
        &provider,
        &req.build(),
        &Deltas::default(),
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert!(error.message.contains("null bytes"), "{}", error.message);
    assert_eq!(provider.live_processes(), 0);
    assert_eq!(provider.process_state("conv-no-spawn"), None);
    provider.close_chat("conv-no-spawn");
    provider.shutdown().await;
    let runtime: Vec<_> = std::fs::read_dir(std::env::temp_dir())
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with("mcb-"))
        .collect();
    let _ = runtime;
    let _ = std::fs::remove_dir_all(&dir);
}

pub async fn claude_code_files_a_session_of_the_shared_working_directory_under_its_dashed_name() {
    let home = harness::home_dir();
    assert_eq!(
        cwd(),
        home.join(".cache").join("mcbench").join("claude-cwd")
    );
    let dashed: String = cwd()
        .to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let config = ClaudeConfig {
        data_dir: "/d".into(),
        binary: "claude".into(),
        home: None,
        bridge: bridge(),
    };
    assert_eq!(
        config.transcript_file("s-1"),
        home.join(".claude")
            .join("projects")
            .join(&dashed)
            .join("s-1.jsonl")
    );
    let elsewhere = ClaudeConfig {
        home: Some("/config".into()),
        ..config
    };
    assert_eq!(
        elsewhere.transcript_file("s-1"),
        Path::new("/config/projects")
            .join(&dashed)
            .join("s-1.jsonl")
    );
}

pub async fn deleting_a_chat_deletes_its_session_transcript_once_its_process_has_exited_and_no_other_file()
 {
    let dir = scratch("claude-transcript");
    let fake = Fake::new(&dir.join("fake"), "claude", &script(&allowed(false, &[])));
    let config = ClaudeConfig {
        data_dir: dir.join("data"),
        binary: fake.program.clone().into(),
        home: Some(dir.join("config")),
        bridge: bridge(),
    };
    let provider = ClaudeCode::new(config.clone(), None);
    let folder = config.transcript_file("x").parent().unwrap().to_path_buf();
    std::fs::create_dir_all(&folder).unwrap();
    let files: Vec<_> = ["sess-1", "other-session", "sess-1-2"]
        .iter()
        .map(|id| folder.join(format!("{id}.jsonl")))
        .collect();
    for file in &files {
        std::fs::write(file, "{}\n").unwrap();
    }
    std::fs::create_dir_all(folder.join("sess-1")).unwrap();
    // The chat's warm process, which the deletion waits for.
    let (transcript, messages) = hut();
    run_turn(&provider, Req::new(messages).chat("chat", Some(transcript))).await;
    assert_eq!(provider.process_state("chat"), Some("idle"));
    provider.destroy("chat").await.unwrap();
    assert_eq!(
        read_session(&config.data_dir, "chat", claude_code::NAME).unwrap(),
        None
    );
    assert_eq!(provider.live_processes(), 0);
    assert_eq!(
        files.iter().map(|f| f.exists()).collect::<Vec<_>>(),
        [false, true, true]
    );
    assert!(
        folder.join("sess-1").exists(),
        "the session folder beside it stays"
    );
    provider.destroy("never-used").await.unwrap();
    assert_eq!(
        files.iter().map(|f| f.exists()).collect::<Vec<_>>(),
        [false, true, true]
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// `Provider::complete`, the one call the agent loop makes: with the
/// request's transcript it resumes and records as `complete_turn` does, and
/// its message carries `chatCompletion`'s telemetry.
pub async fn the_seams_complete_resumes_and_records_the_session_with_its_telemetry() {
    use mcbench_providers::Provider;
    let (dir, fake, provider) = fixture("seam", false);
    write_session(
        &dir.join("data"),
        "conv-seam",
        "claude-code",
        &record_of(json!({ "lastSeen": "u1", "tasks": [] })),
    )
    .unwrap();
    let transcript = json!([m("u1", "user"), m("a1", "assistant"), m("u2", "user")]);
    let req = Req::new(json!([system("You build."), user("Build a hut."), { "role": "assistant", "content": "Built" }, user("Make it taller.")]))
        .chat("conv-seam", Some(transcript));
    let cancel = CancellationToken::new();
    let mut seen = Vec::new();
    let mut sink = |d: mcbench_providers::Delta| seen.push(d);
    let first = provider
        .complete(&req.build(), &mut sink, &cancel)
        .await
        .unwrap();
    assert!(matches!(
        seen.first(),
        Some(mcbench_providers::Delta::Started)
    ));
    assert_eq!(first.tool_calls.len(), 1);
    assert_eq!(
        first.telemetry.provider,
        mcbench_providers::ProviderId::ClaudeCode
    );
    assert_eq!(flag(&fake.args(), "--resume"), Some("sess-1"));
    assert_eq!(
        fake.input()["message"]["content"],
        json!([{ "type": "text", "text": "Make it taller." }])
    );
    let mut messages = req.messages.as_array().unwrap().clone();
    messages.push(json!({ "role": "assistant", "content": "", "tool_calls": [first.tool_calls[0].to_json()] }));
    messages
        .push(json!({ "role": "tool", "tool_call_id": first.tool_calls[0].id, "content": "ok" }));
    let next = Req {
        messages: Value::Array(messages),
        ..req
    };
    let last = provider
        .complete(&next.build(), &mut |_| {}, &cancel)
        .await
        .unwrap();
    assert_eq!(last.content, "Built: ok");
    let telemetry = last.telemetry.to_json();
    assert_eq!(telemetry["contextTokens"].as_f64(), Some(7383.0));
    assert_eq!(
        telemetry["usage"]["inputTokens"].as_f64(),
        Some(14411.0 + 7381.0 + 6.0)
    );
    assert_eq!(
        read_session(&dir.join("data"), "conv-seam", "claude-code").unwrap(),
        Some(record_of(json!({ "lastSeen": "u2", "tasks": [] })))
    );
    provider.shutdown().await;
}
