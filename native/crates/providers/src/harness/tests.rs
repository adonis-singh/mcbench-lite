//! `test/harness.test.js` ported (the cases that need no CLI; the rest are
//! in `tests/cli.rs`), and the golden comparison with today's Node output.

use std::sync::{Arc, Mutex};

use serde_json::{Map, Value, json};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

use super::codex_cli::{CodexCli, CodexConfig, Launch, LaunchSpec, Launched};
use super::relay::{Relay, State, Waiter};
use super::*;
use crate::Delta;

const PNG: &str = "data:image/png;base64,iVBORw0KGgo=";

fn transcript() -> Vec<Value> {
    vec![
        json!({ "role": "system", "content": "You build in Minecraft." }),
        json!({ "role": "user", "content": "Build a hut." }),
        json!({ "role": "assistant", "content": "Starting.", "tool_calls": [{ "id": "call-1", "type": "function", "function": { "name": "build", "arguments": "{\"summary\":\"hut\"}" } }] }),
        json!({ "role": "tool", "tool_call_id": "call-1", "content": "{\"ok\":true}" }),
        json!({ "role": "user", "content": [{ "type": "text", "text": "[see_build] Screenshots of the current build:" }, { "type": "image_url", "image_url": { "url": PNG } }] }),
        json!({ "role": "assistant", "content": "Done." }),
        json!({ "role": "user", "content": [{ "type": "text", "text": "Make the roof dark oak." }, { "type": "image_url", "image_url": { "url": PNG } }] }),
    ]
}

fn refs(values: &[Value]) -> Vec<&Value> {
    values.iter().collect()
}

#[test]
fn a_fresh_harness_session_gets_the_earlier_transcript_and_the_request_a_resumed_one_only_the_request()
 {
    let messages = transcript();
    let (fresh, images) = harness_prompt(&refs(&messages), true, Vec::new()).unwrap();
    assert!(fresh.starts_with("<conversation>\nUser:\nBuild a hut."));
    assert!(fresh.contains("Called build (call-1) with {\"summary\":\"hut\"}"));
    assert!(fresh.contains("[Image 1]"));
    assert!(fresh.ends_with("</conversation>\n\nMake the roof dark oak.\n[Image 2]"));
    assert_eq!(images.len(), 2);
    assert!(!fresh.contains("<instructions>"));
    let (resumed, images) = harness_prompt(&refs(&messages), false, Vec::new()).unwrap();
    assert_eq!(resumed, "Make the roof dark oak.\n[Image 1]");
    let (noted, _) = harness_prompt(
        &refs(&messages),
        false,
        vec!["<note>\nA\n</note>".to_owned()],
    )
    .unwrap();
    assert_eq!(
        noted,
        "<note>\nA\n</note>\n\nMake the roof dark oak.\n[Image 1]"
    );
    assert_eq!(
        images,
        vec![PromptImage {
            mime_type: "image/png".to_owned(),
            data: "iVBORw0KGgo=".to_owned()
        }]
    );
    assert_eq!(system_text(&refs(&messages)), "You build in Minecraft.");
}

fn e(id: &str, role: &str) -> TranscriptEntry {
    TranscriptEntry {
        id: id.to_owned(),
        role: role.to_owned(),
        see_build: false,
        stopped: false,
        reply: None,
    }
}

fn shots(id: &str) -> TranscriptEntry {
    TranscriptEntry {
        see_build: true,
        ..e(id, "user")
    }
}

fn stopped(id: &str, reply: Option<&str>) -> TranscriptEntry {
    TranscriptEntry {
        stopped: true,
        reply: reply.map(str::to_owned),
        ..e(id, "assistant")
    }
}

fn record(value: Value) -> Record {
    value.as_object().unwrap().clone()
}

#[test]
fn a_stored_session_continues_while_everything_after_its_latest_request_belongs_to_that_turn() {
    let r = record(json!({ "sessionId": "s", "lastSeen": "u1" }));
    assert!(!session_continues(None, &[e("u2", "user")]));
    assert!(session_continues(
        Some(&r),
        &[
            e("u1", "user"),
            e("a1", "assistant"),
            e("t1", "tool"),
            shots("s1"),
            e("a2", "assistant"),
            e("u2", "user")
        ]
    ));
    assert!(session_continues(
        Some(&r),
        &[
            e("u1", "user"),
            e("a1", "assistant"),
            e("t1", "tool"),
            stopped("a2", None),
            e("u2", "user")
        ]
    ));
    assert!(session_continues(
        Some(&r),
        &[
            e("u1", "user"),
            e("a1", "assistant"),
            e("t1", "tool"),
            e("e1", "error"),
            e("u2", "user")
        ]
    ));
    assert!(!session_continues(
        Some(&r),
        &[e("u0", "user"), e("u2", "user")]
    ));
    assert!(!session_continues(
        Some(&r),
        &[
            e("u1", "user"),
            e("a1", "assistant"),
            e("u9", "user"),
            e("a9", "assistant"),
            e("u2", "user")
        ]
    ));
    assert!(!session_continues(
        Some(&r),
        &[e("u1", "user"), e("t1", "tool"), shots("s1")]
    ));
}

/// Today's system prompt for a resolution, from the agent's contract files
/// (generated from `server/prompt.js`; the budgets' holes filled alike).
fn system_prompt(resolution: u32) -> String {
    let fill = |text: &str| {
        let mut out = String::new();
        let mut rest = text;
        while let Some(at) = rest.find('⟦') {
            out.push_str(&rest[..at]);
            let end = rest[at..].find('⟧').unwrap() + at + '⟧'.len_utf8();
            out.push_str("12345");
            rest = &rest[end..];
        }
        out.push_str(rest);
        out
    };
    let base = fill(include_str!(
        "../../../agent/contract/prompt/image.network.txt"
    ));
    match resolution {
        1 => base,
        5 => format!(
            "{base}\n\n{}",
            fill(include_str!(
                "../../../agent/contract/prompt/resolution-5.image.txt"
            ))
        ),
        _ => unreachable!(),
    }
}

#[test]
fn a_resumed_session_is_given_the_reply_the_user_stopped_in_its_latest_turn_and_a_resolution_that_changed_since_it_was_told_one()
 {
    let at = |resolution| {
        vec![
            json!({ "role": "system", "content": system_prompt(resolution) }),
            json!({ "role": "user", "content": "Go on." }),
        ]
    };
    let (one, five) = (at(1), at(5));
    let standard = session_terms(&refs(&one));
    let large = session_terms(&refs(&five));
    assert_eq!(standard.instructions, large.instructions);
    assert_ne!(standard.resolution, large.resolution);
    let as_record =
        |t: &Terms| record(json!({ "instructions": t.instructions, "resolution": t.resolution }));
    assert!(resolution_note(&as_record(&standard), &refs(&one)).is_empty());
    let to_large = resolution_note(&as_record(&standard), &refs(&five)).remove(0);
    assert!(to_large.starts_with("<resolution-change>\nThe chat's resolution has changed. From this request on, this section replaces the Resolution section of your instructions and any earlier resolution change:\n\n## Resolution\nThis chat's resolution is "));
    assert!(to_large.ends_with("\n</resolution-change>"));
    let to_standard = resolution_note(&as_record(&large), &refs(&one)).remove(0);
    assert!(to_standard.contains("changed to Standard. From this request on, the Resolution section of your instructions and any earlier resolution change no longer apply"));

    let r = record(json!({ "sessionId": "s", "lastSeen": "u1" }));
    let messages = vec![
        e("u0", "user"),
        stopped("a0", Some("Old turn")),
        e("u1", "user"),
        stopped("a1", Some("First the floor, then")),
        e("u2", "user"),
    ];
    assert_eq!(
        stopped_reply_note(&r, &messages),
        vec!["<stopped-reply>\nThe user stopped your previous reply while you were writing it, and this session does not hold it. This is the text you had written, as the user saw it:\n\nAssistant:\nFirst the floor, then\n</stopped-reply>".to_owned()]
    );
    let mut quiet = messages.clone();
    quiet[3] = stopped("a1", None);
    assert!(stopped_reply_note(&r, &quiet).is_empty());
    quiet[3] = TranscriptEntry {
        ..e("a1", "assistant")
    };
    assert!(stopped_reply_note(&r, &quiet).is_empty());
}

fn scratch(name: &str) -> crate::scratch::Scratch {
    crate::scratch::Scratch::new(&format!("unit-{name}"))
}

#[tokio::test]
async fn session_records_live_under_the_data_directory_per_conversation_and_harness() {
    let data = scratch("records");
    assert_eq!(read_session(&data, "conv", "claude-code").unwrap(), None);
    let written = record(json!({ "sessionId": "abc", "lastSeen": "m1" }));
    write_session(&data, "conv", "claude-code", &written).unwrap();
    assert_eq!(
        read_session(&data, "conv", "claude-code").unwrap(),
        Some(written)
    );
    assert_eq!(
        conversation_dir(&data, "conv").parent().unwrap(),
        data.join("harness")
    );
    let claude = super::claude_code::ClaudeCode::new(
        super::claude_code::ClaudeConfig {
            data_dir: data.to_path_buf(),
            binary: "claude".into(),
            home: Some(data.join("config")),
            bridge: BridgeCommand {
                program: "x".into(),
            },
        },
        None,
    );
    claude.destroy("conv").await.unwrap();
    assert_eq!(read_session(&data, "conv", "claude-code").unwrap(), None);
    let _ = std::fs::remove_dir_all(&data);
}

/// A completion's waiter, and what it reads.
struct Next {
    deltas: tokio::sync::mpsc::UnboundedReceiver<Delta>,
    done: oneshot::Receiver<Result<Map<String, Value>, String>>,
}

fn next(relay: &mut Relay, with_deltas: bool) -> Next {
    let (waiter, deltas, done) = Waiter::new(with_deltas);
    relay.next(waiter);
    Next { deltas, done }
}

impl Next {
    fn deltas(&mut self) -> Vec<Value> {
        let mut out = Vec::new();
        while let Ok(d) = self.deltas.try_recv() {
            out.push(delta_json(&d));
        }
        out
    }

    fn answer(&mut self) -> Option<Result<Map<String, Value>, String>> {
        self.done.try_recv().ok()
    }
}

fn answered(
    rx: &mut oneshot::Receiver<super::relay::CallAnswer>,
) -> Option<super::relay::CallAnswer> {
    rx.try_recv().ok()
}

#[test]
fn the_relay_surfaces_harness_tool_calls_one_at_a_time_and_rejects_the_rest_when_the_turn_ends() {
    let mut session = Relay::new("Test", None).unwrap();
    let mut first = next(&mut session, true);
    session.update(Some("Placing."), None, None);
    let (mut c1, _) = session.call("c1", "build", json!({ "summary": "a" }));
    let (mut c2, _) = session.call("c2", "bash", json!({ "command": "ls" }));
    let message = first.answer().unwrap().unwrap();
    assert_eq!(message["content"], "Placing.");
    assert_eq!(
        message["tool_calls"],
        json!([{ "id": "c1", "type": "function", "function": { "name": "build", "arguments": "{\"summary\":\"a\"}" } }])
    );
    let keys: Vec<String> = first
        .deltas()
        .iter()
        .map(|d| d.as_object().unwrap().keys().next().unwrap().clone())
        .collect();
    assert_eq!(keys, ["content", "toolCall"]);
    session
        .deliver(vec![json!({ "type": "text", "text": "ok" })])
        .unwrap();
    assert_eq!(
        answered(&mut c1),
        Some(Ok(vec![json!({ "type": "text", "text": "ok" })]))
    );
    let mut second = next(&mut session, false);
    assert_eq!(
        second.answer().unwrap().unwrap()["tool_calls"][0]["id"],
        "c2"
    );
    session
        .deliver(vec![json!({ "type": "text", "text": "listing" })])
        .unwrap();
    assert!(answered(&mut c2).unwrap().is_ok());
    let mut last = next(&mut session, false);
    session.update(
        Some("Finished."),
        Some("checked"),
        Some(json!({ "input_tokens": 3, "output_tokens": 1 })),
    );
    session.finish(None, None);
    let last = last.answer().unwrap().unwrap();
    assert_eq!(
        Value::Object(last),
        json!({ "role": "assistant", "content": "Finished.", "reasoning": "checked", "finish_reason": "stop", "usage": { "input_tokens": 3, "output_tokens": 1 } })
    );
    assert_eq!(session.state, State::Closed);

    let mut stopped = Relay::new("Test", None).unwrap();
    let (mut waiting, _) = stopped.call("c3", "build", json!({}));
    stopped.close();
    assert_eq!(
        answered(&mut waiting),
        Some(Err(
            "The mcbench turn ended before this tool ran.".to_owned()
        ))
    );
    assert_eq!(
        next(&mut stopped, false).answer(),
        Some(Err("Test run closed".to_owned()))
    );
}

#[test]
fn streamed_tool_arguments_reach_the_ui_as_written_even_while_another_call_runs_and_are_not_resent_whole()
 {
    let mut session = Relay::new("Test", None).unwrap();
    let mut first = next(&mut session, true);
    session.draft_tool_call("t1", 1, "build");
    session.draft_args("t1", "{\"summary\":").unwrap();
    session.draft_args("t1", "\"a\"}").unwrap();
    session.draft_tool_call("t2", 2, "build");
    session.draft_args("t2", "{\"summary\":").unwrap();
    let (_c1, _) = session.call("t1", "build", json!({ "summary": "a" }));
    first.answer().unwrap().unwrap();
    let calls: Vec<Value> = first
        .deltas()
        .into_iter()
        .map(|d| d["toolCall"].clone())
        .collect();
    assert_eq!(
        calls,
        vec![
            json!({ "index": 1, "id": "t1", "name": "build", "args": "" }),
            json!({ "index": 1, "id": "t1", "name": "build", "args": "{\"summary\":" }),
            json!({ "index": 1, "id": "t1", "name": "build", "args": "\"a\"}" }),
            json!({ "index": 2, "id": "t2", "name": "build", "args": "" }),
            json!({ "index": 2, "id": "t2", "name": "build", "args": "{\"summary\":" }),
        ]
    );
    session.draft_args("t2", "\"b\"}").unwrap();
    let (mut c2, _) = session.call("t2", "build", json!({ "summary": "b" }));
    session
        .deliver(vec![json!({ "type": "text", "text": "ok" })])
        .unwrap();
    let mut second = next(&mut session, true);
    assert_eq!(
        second.answer().unwrap().unwrap()["tool_calls"][0]["id"],
        "t2"
    );
    let calls: Vec<Value> = second
        .deltas()
        .into_iter()
        .map(|d| d["toolCall"].clone())
        .collect();
    assert_eq!(
        calls,
        vec![json!({ "index": 2, "id": "t2", "name": "build", "args": "\"b\"}" })]
    );
    session.close();
    assert_eq!(
        answered(&mut c2),
        Some(Err(
            "The mcbench turn ended before this tool ran.".to_owned()
        ))
    );
}

fn python(dir: &Path, name: &str, source: &str) -> PathBuf {
    let file = dir.join(name);
    std::fs::write(&file, source).unwrap();
    file
}

async fn run_lines(
    script: &Path,
    env: Vec<(std::ffi::OsString, std::ffi::OsString)>,
    input: &str,
    refuse: Option<&str>,
    on_stop: Box<dyn FnOnce() + Send>,
) -> (
    Vec<Value>,
    Result<process::Exit, String>,
    process::JsonLinesRun,
) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let sink = seen.clone();
    let refuse = refuse.map(str::to_owned);
    let (send, done) = oneshot::channel();
    let python: std::ffi::OsString = "python3".into();
    let args = vec![script.to_string_lossy().into_owned()];
    let run = process::run_json_lines(
        process::Launch {
            name: "The CLI",
            command: &python,
            args: &args,
            cwd: script.parent().unwrap(),
            env: &env,
        },
        input.as_bytes().to_vec(),
        false,
        Box::new(move |event: Value| {
            if refuse.as_deref() == event["type"].as_str() {
                return Err("refused".to_owned());
            }
            sink.lock().unwrap().push(event);
            Ok(())
        }),
        on_stop,
        Box::new(move |outcome| {
            let _ = send.send(outcome);
        }),
    )
    .unwrap();
    let outcome = done.await.unwrap();
    let seen = seen.lock().unwrap().clone();
    (seen, outcome, run)
}

#[cfg_attr(
    windows,
    ignore = "runs its fake CLI with python3, which Windows does not have"
)]
#[tokio::test]
async fn run_json_lines_parses_stdout_lines_stops_the_process_when_a_line_is_refused_and_reports_the_exit()
 {
    let dir = scratch("lines");
    let emit = python(
        &dir,
        "emit.py",
        "import json, os, sys\ninput = sys.stdin.read()\nprint(json.dumps({'type': 'echo', 'input': input}))\nprint(json.dumps({'type': 'done'}))\nsys.stderr.write('warned\\n')\nsys.exit(int(os.environ.get('EXIT_CODE', '0')))\n",
    );
    let mut env: Vec<_> = std::env::vars_os().collect();
    env.push(("EXIT_CODE".into(), "3".into()));
    let (seen, exit, _) = run_lines(&emit, env, "hello", None, Box::new(|| {})).await;
    assert_eq!(
        seen,
        vec![
            json!({ "type": "echo", "input": "hello" }),
            json!({ "type": "done" })
        ]
    );
    let exit = exit.unwrap();
    assert_eq!(exit.code, Some(3));
    assert!(exit.stderr.contains("warned"));

    let hang = python(
        &dir,
        "hang.py",
        "import json, time\nprint(json.dumps({'type': 'bad'}), flush=True)\nwhile True: time.sleep(1)\n",
    );
    let (_, refused, _) = run_lines(
        &hang,
        std::env::vars_os().collect(),
        "",
        Some("bad"),
        Box::new(|| {}),
    )
    .await;
    assert_eq!(refused.unwrap_err(), "refused");
    let _ = std::fs::remove_dir_all(&dir);
}

// A line of 64 MiB is read; a longer one stops the CLI, the turn fails
// with the sentence, and no more of it is kept (the writer below never
// ends its line and would write for ever).
#[cfg_attr(
    windows,
    ignore = "runs its fake CLI with python3, which Windows does not have"
)]
#[tokio::test]
async fn run_json_lines_reads_a_line_of_64_mib_and_stops_the_cli_at_a_longer_one() {
    assert_eq!(crate::bridge::MAX_LINE, 64 << 20);
    let dir = scratch("long-lines");
    let longest = python(
        &dir,
        "longest.py",
        "import json, sys\ntext = 'x' * ((64 << 20) - len(json.dumps({'type': 'long', 'text': ''})))\nsys.stdout.write(json.dumps({'type': 'long', 'text': text}) + '\\n')\n",
    );
    let (seen, exit, _) = run_lines(
        &longest,
        std::env::vars_os().collect(),
        "",
        None,
        Box::new(|| {}),
    )
    .await;
    assert_eq!(exit.unwrap().code, Some(0));
    // `{"type": "long", "text": ""}` is 28 bytes of the line.
    assert_eq!(
        seen.iter()
            .map(|e| e["text"].as_str().unwrap().len())
            .collect::<Vec<_>>(),
        [(64 << 20) - 28]
    );
    let flood = python(
        &dir,
        "flood.py",
        "import sys\nblock = b'x' * (1 << 20)\nwhile True: sys.stdout.buffer.write(block)\n",
    );
    let (seen, stopped, _) = run_lines(
        &flood,
        std::env::vars_os().collect(),
        "",
        None,
        Box::new(|| {}),
    )
    .await;
    assert!(seen.is_empty());
    assert_eq!(
        stopped.unwrap_err(),
        "The CLI wrote a line of more than 64 MiB to mcbench, longer than any event it sends, and mcbench stopped the turn"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_relay_session_cannot_start_on_an_aborted_signal_and_a_closed_one_refuses_calls_and_drops_harness_events()
 {
    assert_eq!(
        Relay::new("Test", Some("stopped first")).err(),
        Some("stopped first".to_owned())
    );
    let mut session = Relay::new("Test", None).unwrap();
    session.draft_tool_call("t1", 1, "build");
    session.close();
    assert_eq!(session.state, State::Closed);
    let (mut call, _) = session.call("c1", "build", json!({}));
    assert_eq!(
        answered(&mut call),
        Some(Err(
            "The mcbench turn ended before this tool ran.".to_owned()
        ))
    );
    session.draft_args("t1", "{\"summary\":").unwrap();
    session.update(Some("late"), None, None);
    assert_eq!(session.text, "");
    assert_eq!(
        session.deliver(Vec::new()),
        Err("Test run closed".to_owned())
    );
}

#[test]
fn a_harness_that_dies_while_its_tool_runs_fails_the_delivery_at_once_and_its_unanswered_calls_never_run()
 {
    let mut session = Relay::new("Test", None).unwrap();
    let (mut first, _) = session.call("c1", "build", json!({}));
    assert_eq!(
        next(&mut session, false).answer().unwrap().unwrap()["tool_calls"][0]["id"],
        "c1"
    );
    let (mut queued, _) = session.call("c2", "build", json!({}));
    session.finish(Some("Test exited 1".to_owned()), None);
    assert_eq!(session.state, State::Ended);
    assert_eq!(
        answered(&mut first),
        Some(Err(
            "The mcbench turn ended before this tool ran.".to_owned()
        ))
    );
    assert_eq!(
        answered(&mut queued),
        Some(Err(
            "The mcbench turn ended before this tool ran.".to_owned()
        ))
    );
    assert_eq!(
        session.deliver(vec![json!({ "type": "text", "text": "ok" })]),
        Err("Test exited 1".to_owned())
    );
    assert_eq!(session.state, State::Closed);
}

#[test]
fn a_session_record_is_saved_only_for_a_turn_that_completed_normally() {
    let mut saves = 0;
    let mut save = || -> Result<(), String> {
        saves += 1;
        Ok(())
    };
    let mut done = Relay::new("Test", None).unwrap();
    done.finish(None, Some(&mut save));
    assert_eq!(
        next(&mut done, false).answer().unwrap().unwrap()["finish_reason"],
        "stop"
    );
    let mut stopped = Relay::new("Test", None).unwrap();
    stopped.close();
    stopped.finish(None, Some(&mut save));
    let mut cut = Relay::new("Test", None).unwrap();
    let _call = cut.call("c1", "build", json!({}));
    cut.finish(None, Some(&mut save));
    assert_eq!(
        next(&mut cut, false).answer(),
        Some(Err("Test ended with a tool call unanswered".to_owned()))
    );
    drop(save);
    assert_eq!(saves, 1);
    let mut broken = Relay::new("Test", None).unwrap();
    let mut failing = || -> Result<(), String> { Err("disk full".to_owned()) };
    broken.finish(None, Some(&mut failing));
    assert_eq!(
        next(&mut broken, false).answer(),
        Some(Err("disk full".to_owned()))
    );
}

#[test]
fn a_tool_call_cancelled_by_its_mcp_request_never_surfaces_and_a_running_one_drops_its_result() {
    let mut session = Relay::new("Test", None).unwrap();
    let (mut first, first_serial) = session.call("c1", "build", json!({}));
    assert_eq!(
        next(&mut session, false).answer().unwrap().unwrap()["tool_calls"][0]["id"],
        "c1"
    );
    let (_second, second_serial) = session.call("c2", "build", json!({}));
    // The bridge answers a cancelled call itself and drops it here.
    session.cancel_call("c2", second_serial);
    session.cancel_call("c1", first_serial);
    drop(first.try_recv());
    session
        .deliver(vec![json!({ "type": "text", "text": "ignored" })])
        .unwrap();
    let mut last = next(&mut session, false);
    assert!(last.answer().is_none(), "the cancelled call never surfaces");
    session.finish(None, None);
    assert_eq!(last.answer().unwrap().unwrap()["finish_reason"], "stop");
}

fn custom_codex(data: &Path, launch: super::codex_cli::Launcher) -> CodexCli {
    CodexCli::new(
        CodexConfig {
            data_dir: data.to_path_buf(),
            binary: "codex".into(),
            prefix: Vec::new(),
            home: Some(data.join("login")),
            bridge: BridgeCommand {
                program: "x".into(),
            },
            launch: Some(Launch::Custom(launch)),
        },
        Arc::new(crate::catalog::CodexModels::default()),
    )
}

fn request(conversation: &str) -> crate::CompletionRequest {
    crate::CompletionRequest {
        conversation_id: Some(conversation.to_owned()),
        model: "gpt-6-astra".to_owned(),
        effort: None,
        messages: vec![crate::ApiMessage(
            json!({ "role": "user", "content": "Build a hut." })
                .as_object()
                .unwrap()
                .clone(),
        )],
        tools: Vec::new(),
        chat: false,
        transcript: None,
    }
}

#[tokio::test]
async fn a_run_that_fails_to_start_is_closed_and_forgotten() {
    let data = scratch("fails-to-start");
    let made = Arc::new(Mutex::new(0));
    let count = made.clone();
    let codex = custom_codex(
        &data,
        Arc::new(move |_, _| {
            *count.lock().unwrap() += 1;
            Err("no binary".to_owned())
        }),
    );
    let mut sink = |_: Delta| {};
    let cancel = CancellationToken::new();
    for _ in 0..2 {
        let error = codex
            .complete_turn(&request("c"), &mut sink, &cancel)
            .await
            .unwrap_err();
        assert_eq!(error.message, "no binary");
    }
    assert_eq!(*made.lock().unwrap(), 2, "each completion starts a new run");
    assert_eq!(codex.live_sessions(), 0);
    let _ = std::fs::remove_dir_all(&data);
}

#[cfg_attr(
    windows,
    ignore = "runs its fake CLI with python3, which Windows does not have"
)]
#[tokio::test]
async fn run_json_lines_stop_does_nothing_once_the_process_has_exited() {
    let dir = scratch("quick");
    let quick = python(
        &dir,
        "quick.py",
        "import json\nprint(json.dumps({'type': 'done'}))\n",
    );
    let stops = Arc::new(Mutex::new(0));
    let counter = stops.clone();
    let (_, exit, run) = run_lines(
        &quick,
        std::env::vars_os().collect(),
        "",
        None,
        Box::new(move || *counter.lock().unwrap() += 1),
    )
    .await;
    assert_eq!(exit.unwrap().code, Some(0));
    run.stop();
    assert_eq!(*stops.lock().unwrap(), 0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg_attr(
    windows,
    ignore = "runs its fake CLI with python3, which Windows does not have"
)]
#[tokio::test]
async fn a_run_closed_while_it_runs_is_stopped_released_forgotten_and_its_waiter_answered() {
    // Today's case is a run whose `stop` throws; a Rust run's stop cannot
    // fail, so what is left to hold is the rest: closing a running run
    // stops it, releases it, forgets it and answers its waiter.
    let data = scratch("close-running");
    let released = Arc::new(Mutex::new(0));
    let stopped = Arc::new(Mutex::new(0));
    let (r, s) = (released.clone(), stopped.clone());
    // The process says it runs by making this file: the run is recorded as
    // running under the lock the close takes, before the process can.
    let running = data.join("running");
    let marker = running.clone();
    let codex = Arc::new(custom_codex(
        &data,
        Arc::new(move |spec: &LaunchSpec, _| {
            std::fs::create_dir_all(spec.dir.join("workspace")).unwrap();
            let (r, s) = (r.clone(), s.clone());
            Ok(Launched {
                command: "python3".into(),
                args: vec![
                    "-c".to_owned(),
                    "import sys, time\nopen(sys.argv[1], 'w').close()\nwhile True: time.sleep(1)"
                        .to_owned(),
                    marker.to_string_lossy().into_owned(),
                ],
                cwd: spec.dir.join("workspace"),
                env: std::env::vars_os().collect(),
                on_stop: Box::new(move || *s.lock().unwrap() += 1),
                release: Box::new(move || {
                    *r.lock().unwrap() += 1;
                    Ok(())
                }),
            })
        }),
    ));
    let waiting = {
        let codex = codex.clone();
        tokio::spawn(async move {
            let mut sink = |_: Delta| {};
            codex
                .complete_turn(&request("stuck"), &mut sink, &CancellationToken::new())
                .await
        })
    };
    let started = std::time::Instant::now();
    let until = |what: &str, done: &dyn Fn() -> bool| {
        assert!(
            started.elapsed() < std::time::Duration::from_secs(60),
            "{what}"
        );
        !done()
    };
    while until("the process runs", &|| running.exists()) {
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    codex.close_chat("stuck");
    assert_eq!(
        waiting.await.unwrap().unwrap_err().message,
        "Codex run closed"
    );
    assert_eq!(codex.live_sessions(), 0);
    while until("the run is released", &|| *released.lock().unwrap() >= 1) {
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    assert_eq!(*stopped.lock().unwrap(), 1);
    assert_eq!(*released.lock().unwrap(), 1);
    let _ = std::fs::remove_dir_all(&data);
}

#[test]
fn a_session_record_whose_write_is_cut_short_leaves_no_record_that_fails_to_parse() {
    let data = scratch("cut");
    // A write the disk cut short leaves only its temporary file.
    let dir = conversation_dir(&data, "conv-cut");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join(format!("claude-code.json.{}.tmp", std::process::id())),
        "{\n  \"sessio",
    )
    .unwrap();
    assert_eq!(
        read_session(&data, "conv-cut", "claude-code").unwrap(),
        None
    );
    let written = record(json!({ "sessionId": "abc", "lastSeen": "m2" }));
    write_session(&data, "conv-cut", "claude-code", &written).unwrap();
    assert_eq!(
        read_session(&data, "conv-cut", "claude-code").unwrap(),
        Some(written)
    );
    assert!(
        !dir.join(format!("claude-code.json.{}.tmp", std::process::id()))
            .exists()
    );
    let _ = std::fs::remove_dir_all(&data);
}

/// Node's `hostArgs` for a resumed thread with the thread id where the
/// Rust command line has it: after `--`, just before the prompt's `-`
/// (`codex_cli::host_args`), so Codex reads it as the session id whatever
/// it holds. The one place the command line differs from Node's.
fn resume_after_dashes(name: &str, input: &Value, output: &Value) -> Value {
    if name != "hostArgs" || input["resume"] != json!(true) {
        return output.clone();
    }
    let mut args = output.as_array().unwrap().clone();
    let id = args.remove(2);
    let at = args.len() - 1;
    args.insert(at, id);
    args.insert(at, json!("--"));
    Value::Array(args)
}

/// Each golden line: the Rust function's answer for the input, against
/// what today's Node gave (`tests/golden/generate.js`).
#[test]
fn golden_matches_node() {
    let text = include_str!("../../tests/golden/harness.jsonl");
    let mut checked = 0;
    for line in text.lines() {
        let case = mcbench_json::parse(line).unwrap();
        let input = &case["input"];
        let ours: Result<Value, String> = golden(case["fn"].as_str().unwrap(), input);
        let output = case
            .get("output")
            .map(|o| resume_after_dashes(case["fn"].as_str().unwrap(), input, o));
        match (output.as_ref(), case.get("error"), &ours) {
            (Some(expected), _, Ok(got)) => assert_eq!(
                mcbench_json::stringify(got),
                mcbench_json::stringify(expected),
                "{} {}",
                case["fn"],
                mcbench_json::stringify(input)
            ),
            (_, Some(expected), Err(got)) => assert_eq!(
                got,
                expected.as_str().unwrap(),
                "{} {}",
                case["fn"],
                mcbench_json::stringify(input)
            ),
            _ => panic!(
                "{} {}: Node gave {}, Rust {:?}",
                case["fn"],
                mcbench_json::stringify(input),
                line,
                ours
            ),
        }
        checked += 1;
    }
    assert!(checked > 250, "{checked} golden cases");
}

fn values(input: &Value, key: &str) -> Vec<Value> {
    input[key].as_array().unwrap().clone()
}

fn transcript_of(input: &Value) -> Vec<TranscriptEntry> {
    input["transcript"]
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

fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .map(|a| a.iter().map(|s| s.as_str().unwrap().to_owned()).collect())
        .unwrap_or_default()
}

fn golden(name: &str, input: &Value) -> Result<Value, String> {
    use super::claude_code as cc;
    use super::codex_cli as cx;
    let bridge_args = vec!["tool-bridge".to_owned(), "/run/x/mcp.sock".to_owned()];
    Ok(match name {
        "harnessPrompt" => {
            let messages = values(input, "messages");
            let (text, images) = harness_prompt(
                &refs(&messages),
                input["history"].as_bool().unwrap(),
                strings(&input["notes"]),
            )?;
            json!({ "text": text, "images": images.iter().map(|i| json!({ "mimeType": i.mime_type, "data": i.data })).collect::<Vec<_>>() })
        }
        "systemText" => json!(system_text(&refs(&values(input, "messages")))),
        "sessionTerms" => {
            let t = session_terms(&refs(&values(input, "messages")));
            json!({ "instructions": t.instructions, "resolution": t.resolution })
        }
        "resolutionNote" => json!(resolution_note(
            input["record"].as_object().unwrap(),
            &refs(&values(input, "messages"))
        )),
        "splitResolution" => {
            let (base, resolution) = split_resolution(input["text"].as_str().unwrap());
            json!({ "base": base, "resolution": resolution })
        }
        "resolutionUpdate" => json!(resolution_update(input["section"].as_str().unwrap())),
        "stoppedReplyNote" => json!(stopped_reply_note(
            input["record"].as_object().unwrap(),
            &transcript_of(input)
        )),
        "sessionContinues" => json!(session_continues(
            Some(input["record"].as_object().unwrap()),
            &transcript_of(input)
        )),
        "pendingResult" => match pending_result(
            &refs(&values(input, "messages")),
            input["callId"].as_str().unwrap(),
        )? {
            Some(blocks) => Value::Array(blocks),
            None => Value::Null,
        },
        "claudeArgs" => {
            let session = input.get("session").filter(|s| !s.is_null());
            json!(cc::claude_args(
                input["model"].as_str().unwrap(),
                input["effort"].as_str(),
                input["systemPrompt"].as_str().unwrap(),
                &strings(&input["allowedTools"]),
                ("<bridge>", &bridge_args),
                session.map(|s| (s["id"].as_str().unwrap(), s["resume"].as_bool().unwrap())),
                input["oneShot"].as_bool().unwrap(),
                &strings(&input["builtins"]),
            ))
        }
        "checkInit" => {
            cc::check_init(
                &input["event"],
                &strings(&input["allowed"]),
                &strings(&input["builtins"]),
            )?;
            Value::Null
        }
        "claudeUsageOf" => cc::usage_of(&input["result"])?,
        "claudeContextTokensOf" => cc::context_tokens_of(&input["usage"]),
        "streamInput" => {
            let images: Vec<PromptImage> = input["images"]
                .as_array()
                .unwrap()
                .iter()
                .map(|i| PromptImage {
                    mime_type: i["mimeType"].as_str().unwrap().to_owned(),
                    data: i["data"].as_str().unwrap().to_owned(),
                })
                .collect();
            json!(cc::stream_input(input["text"].as_str().unwrap(), &images))
        }
        "configToml" => json!(cx::config_toml(
            input["model"].as_str().unwrap(),
            input["effort"].as_str(),
            input["systemPrompt"].as_str().unwrap()
        )),
        "hostArgs" => json!(cx::host_args(
            input["model"].as_str().unwrap(),
            input["effort"].as_str(),
            input["systemPrompt"].as_str().unwrap(),
            ("<bridge>", &bridge_args),
            input["resume"].as_bool().unwrap(),
            input["threadId"].as_str(),
            &strings(&input["images"]),
        )),
        "checkItem" => {
            cx::check_item(&input["item"])?;
            Value::Null
        }
        "codexUsageOf" => cx::usage_of(Some(&input["usage"]))?,
        "modelsFromCache" => Value::Array(
            crate::catalog::models_from_cache(&input["cache"])
                .iter()
                .map(|m| m.to_json())
                .collect(),
        ),
        other => panic!("no golden function {other}"),
    })
}

#[test]
fn a_login_file_that_cannot_be_read_is_the_error_todays_stored_login_throws() {
    use crate::login::{Login, LoginPrograms};
    let home = scratch("corrupt-login");
    std::fs::write(home.join(".claude.json"), "not json").unwrap();
    let bridge = BridgeCommand {
        program: "x".into(),
    };
    let logins = Logins {
        claude: super::claude_code::ClaudeConfig {
            data_dir: home.to_path_buf(),
            binary: "claude".into(),
            home: Some(home.to_path_buf()),
            bridge: bridge.clone(),
        },
        codex: CodexConfig {
            data_dir: home.to_path_buf(),
            binary: "codex".into(),
            prefix: Vec::new(),
            home: Some(home.join("codex")),
            bridge,
            launch: None,
        },
    };
    // V8's sentence, as `JSON.parse` throws it in `oauthAccount`.
    assert_eq!(
        logins.stored(Login::Claude),
        Err("Unexpected token 'o', \"not json\" is not valid JSON".to_owned())
    );
    std::fs::write(
        home.join(".claude.json"),
        "{\"oauthAccount\":{\"emailAddress\":\"a@b\"}}",
    )
    .unwrap();
    assert_eq!(logins.stored(Login::Claude), Ok(true));
    assert_eq!(logins.stored(Login::Codex), Ok(false));
}

/// Codex's box on Windows is Linux's box: the agent's `box` verb in the
/// WSL distro, then the bubblewrap arguments Linux runs, for the distro's
/// paths and user, which are Linux's exactly once the agent has put the
/// root filesystem's environment where the host's spec has it.
#[test]
fn the_box_in_the_wsl_distro_is_linuxs_box() {
    use mcbench_sandbox::proxy::{CodexRun, codex_box_args, insert_image_env};
    use mcbench_sandbox::wsl_codex::{AGENT, LOGIN, RUNS};
    let agent: Vec<String> = [
        "--distribution",
        "mcbench-lite",
        "--user",
        "root",
        "--cd",
        "/",
        "--exec",
        AGENT,
        "codex",
        r"C:\Users\p\.codex",
        r"C:\Users\p\.mcbench-lite\tools",
        r"C:\Users\p\AppData\Local\mcbench-lite\harness",
        "1000:1000",
    ]
    .map(String::from)
    .into();
    let wsl = codex_cli::WslBox {
        program: "wsl.exe".into(),
        agent: agent.clone(),
        harness: "/unused".into(),
        install: (
            "/mnt/tools/codex-0.169.0".into(),
            "/opt/codex/bin/codex".into(),
        ),
        ids: (1000, 1000),
        rootfs: Arc::new(|| Ok("/var/mcbench/.rootfs-ab/rootfs".into())),
    };
    let run = CodexRun {
        rootfs: "/var/mcbench/.rootfs-ab/rootfs".into(),
        image_env: Vec::new(),
        relay: AGENT.into(),
        install: "/mnt/tools/codex-0.169.0".into(),
        home: "/mnt/harness/c1/codex-home".into(),
        auth_file: format!("{LOGIN}/auth.json").into(),
        workspace: "/mnt/harness/c1/workspace".into(),
        sock_dir: format!("{RUNS}/mcb-abc123/sock").into(),
        allowed_hosts: codex_cli::ALLOWED_HOSTS.map(String::from).into(),
        proxy_log: "/mnt/harness/c1/egress.ndjson".into(),
        command: [
            "/opt/codex/bin/codex",
            "exec",
            "--json",
            "--skip-git-repo-check",
            "-",
        ]
        .map(String::from)
        .into(),
    };
    let args = codex_cli::wsl_box_args(
        &wsl,
        "mcb-abc123",
        std::path::Path::new("/mnt/harness/c1/images-mcb-abc123"),
        &run,
    );
    assert_eq!(args[..agent.len()], agent);
    assert_eq!(
        args[agent.len()..agent.len() + 6],
        [
            "box",
            "mcb-abc123",
            "/mnt/harness/c1/images-mcb-abc123",
            "/mnt/harness/c1/egress.ndjson",
            "chatgpt.com,auth.openai.com",
            "--"
        ]
    );
    let boxed = &args[agent.len() + 6..];
    let image_env = vec![
        "PATH=/usr/local/bin:/usr/bin".to_owned(),
        "LANG=C.UTF-8".to_owned(),
    ];
    let linux = CodexRun {
        image_env: image_env.clone(),
        ..run.clone()
    };
    assert_eq!(
        insert_image_env(boxed, &image_env),
        codex_box_args(&linux, (1000, 1000))
    );
    // The login: one file, bound read-write at /codex-home/auth.json, as on
    // Linux; nothing else of the person's Codex folder is named.
    assert!(boxed.windows(3).any(|w| w[0] == "--bind"
        && w[1] == format!("{LOGIN}/auth.json")
        && w[2] == "/codex-home/auth.json"));
    assert_eq!(boxed.iter().filter(|a| a.starts_with(LOGIN)).count(), 1);
    // Linux runs the same box under its systemd scope.
    #[cfg(unix)]
    {
        let on_linux = mcbench_sandbox::proxy::codex_bwrap_args(&linux);
        let at = on_linux.iter().position(|a| a == "bwrap").unwrap();
        let ids = (
            on_linux[at + 3].parse().unwrap(),
            on_linux[at + 5].parse().unwrap(),
        );
        assert_eq!(on_linux[at..], codex_box_args(&linux, ids));
    }
}

#[test]
fn codex_on_windows_installs_the_linux_build_for_the_distro() {
    assert_eq!(install::platform_of("codex", "win32-x64"), "linux-x64");
    assert_eq!(install::platform_of("codex", "win32-arm64"), "linux-arm64");
    assert_eq!(install::platform_of("claude", "win32-x64"), "win32-x64");
    assert_eq!(
        install::platform_of("codex", "darwin-arm64"),
        "darwin-arm64"
    );
    assert_eq!(install::platform_of("codex", "linux-x64"), "linux-x64");
}
