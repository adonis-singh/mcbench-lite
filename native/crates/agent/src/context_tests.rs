//! `agent-context.test.js` (3) and `agent-context-kept.test.js` (2).

use super::*;
use serde_json::json;

/// `/work` in a folder of its own, recording each write.
struct Recorded {
    root: PathBuf,
    writes: Mutex<Vec<(String, String, String)>>,
}

impl Recorded {
    fn new(name: &str) -> Recorded {
        let root =
            std::env::temp_dir().join(format!("mcbench-context-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        Recorded {
            root,
            writes: Mutex::new(Vec::new()),
        }
    }
    fn writes(&self) -> Vec<(String, String, String)> {
        self.writes.lock().unwrap().clone()
    }
    fn read(&self, chat: &str, file: &str) -> String {
        std::fs::read_to_string(self.root.join(chat).join(file)).unwrap()
    }
}

impl Drop for Recorded {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

impl WorkFiles for Recorded {
    fn work_dir(&self, chat: &str) -> Result<PathBuf, String> {
        let dir = self.root.join(chat);
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        Ok(dir)
    }
    fn write(&self, chat: &str, relative: &str, text: &str) -> Result<(), String> {
        let file = self.work_dir(chat)?.join(relative);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, text).unwrap();
        self.writes
            .lock()
            .unwrap()
            .push((chat.into(), relative.into(), text.into()));
        Ok(())
    }
}

fn stringify(messages: &[Value]) -> String {
    mcbench_json::stringify(&Value::Array(messages.to_vec()))
}

fn plain(message: &Value) -> Value {
    let mut message = message.clone();
    message
        .as_object_mut()
        .unwrap()
        .shift_remove("reasoning_details");
    message
}

fn role(message: &Value) -> &str {
    message["role"].as_str().unwrap()
}

fn text(value: &Value) -> String {
    mcbench_json::stringify(value)
}

#[test]
fn context_offloads_whole_old_exchanges_retains_user_instructions_and_recent_signed_reasoning_and_exposes_lossless_archives()
 {
    let work = Recorded::new("offloads");
    let archives = Archives::default();
    let mut messages = vec![json!({ "role": "user", "content": "Keep the original red border." })];
    for i in 0..12 {
        if i == 4 {
            messages.push(json!({ "role": "user", "content": "Make the floor blue." }));
        }
        messages.push(json!({ "role": "assistant", "content": format!("Step {i}"),
            "reasoning_details": [{ "type": "thinking", "thinking": "fixture", "signature": format!("signed-{i}") }],
            "tool_calls": [{ "id": format!("t{i}"), "function": { "name": "build", "arguments": text(&json!({ "program": "x".repeat(5000) })) } }] }));
        messages.push(json!({ "role": "tool", "tool_call_id": format!("t{i}"),
            "content": text(&json!({ "ok": true, "worldRevision": i, "logs": ["z".repeat(5000)] })) }));
    }
    let before = stringify(&messages);
    let result = context_messages("fixture", &messages, 45_000, 3, &archives, &work).unwrap();
    assert!(stringify(&result).len() <= 45_000);
    let users = |list: &[Value]| {
        list.iter()
            .filter(|m| role(m) == "user")
            .cloned()
            .collect::<Vec<_>>()
    };
    assert_eq!(users(&result), users(&messages));
    let tail = |list: &[Value]| list[list.len() - 6..].iter().map(plain).collect::<Vec<_>>();
    assert_eq!(tail(&result), tail(&messages));
    let stub = result
        .iter()
        .find(|m| {
            m["content"]
                .as_str()
                .is_some_and(|c| c.contains("Earlier tool exchange archived"))
        })
        .unwrap();
    let content = stub["content"].as_str().unwrap();
    let at = content.find("/work/context/").unwrap() + "/work/".len();
    let file = &content[at..at + "context/".len() + 24 + ".json".len()];
    assert_eq!(work.read("fixture", file), stringify(&messages[1..3]));
    assert_eq!(stringify(&messages), before);
    assert_eq!(
        context_messages("fixture", &messages, 45_000, 3, &archives, &work).unwrap(),
        result,
        "stable archives preserve the replay prefix"
    );
    for message in result.iter().filter(|m| role(m) == "tool") {
        assert!(result.iter().any(|m| {
            m["tool_calls"]
                .as_array()
                .is_some_and(|calls| calls.iter().any(|c| c["id"] == message["tool_call_id"]))
        }));
    }
}

#[test]
fn what_a_request_sends_only_grows_until_an_archiving_which_drops_the_thinking_it_would_invalidate()
{
    let work = Recorded::new("grows");
    let archives = Archives::default();
    let mut messages = vec![json!({ "role": "user", "content": "Build a castle." })];
    for i in 0..30 {
        messages.push(json!({ "role": "assistant", "content": format!("Step {i}"),
            "reasoning_details": [{ "type": "thinking", "thinking": "fixture", "signature": format!("signed-{i}") }],
            "tool_calls": [{ "id": format!("t{i}"), "function": { "name": "see_build", "arguments": text(&json!({ "program": "x".repeat(3000) })) } }] }));
        messages.push(json!({ "role": "tool", "tool_call_id": format!("t{i}"), "content": text(&json!({ "ok": true, "worldRevision": i })) }));
        messages.push(json!({ "role": "user", "seeBuild": true, "content": [{ "type": "text", "text": format!("shot {i}") },
            { "type": "image_url", "image_url": { "url": format!("/api/images/shot-{i}.jpg") } }] }));
    }
    let mut sent = Vec::new();
    for i in 1..=messages.len() {
        if i < messages.len() && role(&messages[i]) != "assistant" {
            continue;
        }
        sent.push(context_messages("grow", &messages[..i], 60_000, 3, &archives, &work).unwrap());
    }
    let signed = |list: &[Value]| {
        list.iter()
            .filter(|m| m.get("reasoning_details").is_some())
            .count()
    };
    let mut archivings = 0;
    for n in 1..sent.len() {
        let (before, now) = (&sent[n - 1], &sent[n]);
        if !before.iter().zip(now.iter()).all(|(a, b)| a == b) {
            archivings += 1;
            assert!(now.iter().all(|m| m.get("reasoning_details").is_none()));
            continue;
        }
        assert_eq!(signed(now), signed(before) + 1);
    }
    assert!(
        archivings >= 2 && archivings <= sent.len() / 3,
        "archivings: {archivings}"
    );
    assert_eq!(
        sent.last().unwrap(),
        &context_messages("grow", &messages, 60_000, 3, &archives, &work).unwrap()
    );
}

#[test]
fn unmatched_tool_calls_and_protected_user_requirements_are_never_silently_trimmed() {
    let work = Recorded::new("protected");
    let archives = Archives::default();
    let messages = vec![json!({ "role": "user", "content": "important ".repeat(4000) })];
    let error = context_messages(
        "fixture",
        &messages,
        16_000,
        KEEP_EXCHANGES,
        &archives,
        &work,
    )
    .unwrap_err();
    assert!(error.contains("Protected user messages"));
    assert_eq!(
        context_messages(
            "fixture",
            &messages,
            100_000,
            KEEP_EXCHANGES,
            &archives,
            &work
        )
        .unwrap()[0],
        messages[0]
    );
}

/// `agent-context-kept.test.js`'s first case holds the cached `contextMessages`
/// to a copy without the caches; this port keeps no cache of sizes or
/// archives, so the case holds two runs over the growing chat (each with its
/// own archive record) to each other, and their archive files too.
#[test]
fn a_long_chat_sends_byte_identical_requests_with_its_sizes_and_archives_kept() {
    let (one, two) = (Recorded::new("long-1"), Recorded::new("long-2"));
    let (a, b) = (Archives::default(), Archives::default());
    let mut messages: Vec<Value> = Vec::new();
    let (mut checked, mut refused) = (0, 0);
    for turn in 0..20 {
        messages.push(json!({ "id": format!("u{turn}"), "role": "user", "content": format!("Turn {turn}: build more.") }));
        for step in 0..4 {
            let i = turn * 4 + step;
            let name = ["build", "see_build", "test_build", "bash"][step];
            let mut assistant = json!({ "id": format!("a{i}"), "role": "assistant",
                "content": if step == 0 { format!("Working on {turn}") } else { String::new() } });
            if turn % 3 == 0 {
                assistant["reasoning_details"] =
                    json!([{ "type": "thinking", "thinking": "x", "signature": format!("s{i}") }]);
            }
            assistant["tool_calls"] = json!([{ "id": format!("c{i}"), "type": "function",
                "function": { "name": name, "arguments": text(&json!({ "program": "p".repeat(800 + (i % 7) * 300) })) } }]);
            messages.push(assistant);
            messages.push(json!({ "id": format!("r{i}"), "role": "tool", "tool_call_id": format!("c{i}"),
                "content": text(&json!({ "ok": true, "worldRevision": i, "logs": ["l".repeat(400)] })) }));
            if name == "see_build" {
                messages.push(json!({ "id": format!("s{i}"), "role": "user", "seeBuild": true, "content": [
                    { "type": "text", "text": "[see_build] Screenshots" },
                    { "type": "image_url", "image_url": { "url": format!("/api/images/a{i}.jpg") } }] }));
            }
            for (max, keep) in [
                (DEFAULT_MAX_BYTES, KEEP_EXCHANGES),
                (60_000, 4),
                (20_000, 2),
            ] {
                let x = context_messages("long", &messages, max, keep, &a, &one);
                assert_eq!(x, context_messages("long", &messages, max, keep, &b, &two));
                refused += usize::from(x.is_err());
                checked += 1;
            }
        }
        messages.push(json!({ "id": format!("d{turn}"), "role": "assistant", "content": format!("Done {turn}") }));
    }
    assert_eq!(checked, 20 * 4 * 3);
    assert_eq!(one.writes(), two.writes());
    assert!(
        one.writes().len() > 10,
        "archive files written: {}",
        one.writes().len()
    );
    assert!(refused > 0 && refused < checked, "refused {refused}");
}

#[test]
fn an_archive_the_model_changed_or_removed_in_work_is_written_again_at_the_next_request_one_left_alone_is_not()
 {
    let work = Recorded::new("rewrite");
    let archives = Archives::default();
    let mut messages = vec![json!({ "id": "u", "role": "user", "content": "Build." })];
    for i in 0..12 {
        messages.push(json!({ "id": format!("a{i}"), "role": "assistant", "content": "", "tool_calls": [{ "id": format!("t{i}"), "type": "function",
            "function": { "name": "build", "arguments": text(&json!({ "program": "x".repeat(5000) })) } }] }));
        messages.push(
            json!({ "id": format!("r{i}"), "role": "tool", "tool_call_id": format!("t{i}"),
            "content": text(&json!({ "ok": true, "logs": ["z".repeat(5000)] })) }),
        );
    }
    let first = context_messages("rewrite", &messages, 45_000, 3, &archives, &work).unwrap();
    let files: Vec<String> = work.writes().into_iter().map(|(_, file, _)| file).collect();
    assert!(files.len() >= 3, "archives written: {}", files.len());
    let archived: Vec<String> = files.iter().map(|f| work.read("rewrite", f)).collect();
    assert_eq!(
        context_messages("rewrite", &messages, 45_000, 3, &archives, &work).unwrap(),
        first
    );
    assert_eq!(
        work.writes().len(),
        files.len(),
        "untouched archives are not written again"
    );
    let dir = work.root.join("rewrite");
    std::fs::remove_file(dir.join(&files[0])).unwrap();
    std::fs::write(dir.join(&files[1]), "edited").unwrap();
    assert_eq!(
        context_messages("rewrite", &messages, 45_000, 3, &archives, &work).unwrap(),
        first
    );
    let mut again: Vec<String> = work.writes()[files.len()..]
        .iter()
        .map(|(_, f, _)| f.clone())
        .collect();
    again.sort();
    let mut expected = vec![files[0].clone(), files[1].clone()];
    expected.sort();
    assert_eq!(again, expected);
    assert_eq!(
        files
            .iter()
            .map(|f| work.read("rewrite", f))
            .collect::<Vec<_>>(),
        archived
    );
}
