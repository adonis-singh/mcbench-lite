//! The stand-in's turns (dev only): what a turn streams and what it stores
//! as it goes, in the order of a real turn (inventory `style-viewer-routes.md`
//! Part 3, the turn stream): a plain one (reasoning, then the answer word by
//! word) and a full one that streams every item kind the transcript shows
//! (reasoning, text before tools, tool calls written in parts, run and
//! finished with their diff, a test's progress, shots, a failed command, a
//! harness's status line, notices of each level, a plan updated, a
//! completion retry that drops what it had streamed, and the final answer).
//! The stored rows are what the core would store, so the read after the
//! turn shows what the live turn showed.

use mcbench_agent::{CallShown, ProgressShown, TurnEvent};
use serde_json::{Value, json};

/// One step of a turn.
pub enum Step {
    /// An event to publish.
    Event(TurnEvent),
    /// A streamed piece (a delta): published, then the stand-in's rate.
    Delta(TurnEvent),
    /// A row to store: its id prefix and payload.
    Store(&'static str, Value),
    /// A `see_build` shot: the stand-in copies the picture to the store's
    /// `images/<chat>/<name>` and stores the shots message.
    Shot(&'static str),
}

/// Which turns the stand-in streams.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Script {
    /// Reasoning, then the answer.
    Plain,
    /// Every item kind.
    Full,
}

fn words(text: &str, reasoning: bool) -> Vec<Step> {
    text.split_inclusive(' ')
        .map(|word| {
            Step::Delta(if reasoning {
                TurnEvent::ReasoningDelta(word.into())
            } else {
                TurnEvent::AssistantDelta(word.into())
            })
        })
        .collect()
}

/// The reasoning of every stand-in turn.
pub const REASONING: &str = "Thinking about where the third lamp goes.";

/// A plain turn: reasoning, the answer word by word, stored.
pub fn plain(answer: &str) -> Vec<Step> {
    let mut steps = words(REASONING, true);
    steps.push(Step::Event(TurnEvent::Reasoning(REASONING.into())));
    steps.extend(words(answer, false));
    steps.push(Step::Event(TurnEvent::Assistant {
        text: answer.into(),
        last: true,
    }));
    steps.push(Step::Store(
        "a",
        json!({ "role": "assistant", "content": answer, "reasoning": REASONING }),
    ));
    steps
}

/// A tool call written in `parts` (its arguments as they stream), started,
/// and finished with `content` as the tool returned it; `progress` lines
/// stream while it runs; a `see_build` result carries `images`.
fn call(
    id: &str,
    name: &str,
    arguments: &Value,
    progress: &[&str],
    content: &str,
    images: Vec<std::path::PathBuf>,
) -> Vec<Step> {
    let text = arguments.to_string();
    let third = text.len() / 3;
    let cuts = [0, text[..third].len(), text[..2 * third].len(), text.len()];
    let mut steps: Vec<Step> = cuts
        .windows(2)
        .enumerate()
        .map(|(n, w)| {
            Step::Delta(TurnEvent::ToolCallDelta {
                index: 0,
                id: Some(id.into()),
                name: (n == 0).then(|| name.to_string()),
                arguments: text[w[0]..w[1]].to_string(),
            })
        })
        .collect();
    let shown: CallShown = mcbench_agent::call_shown(name, &text);
    steps.push(Step::Event(TurnEvent::ToolStart {
        call_id: id.into(),
        name: name.into(),
        shown: Box::new(shown),
    }));
    for line in progress {
        steps.push(Step::Delta(TurnEvent::ToolProgress {
            call_id: id.into(),
            name: name.into(),
            progress: ProgressShown {
                message: (*line).into(),
            },
            raw: json!({ "message": line }),
        }));
    }
    let mut result = mcbench_agent::tool_shown(name, content);
    result.images = images;
    steps.push(Step::Event(TurnEvent::ToolResult {
        call_id: id.into(),
        name: name.into(),
        shown: Box::new(result),
        result: serde_json::from_str(content).unwrap_or(Value::String(content.into())),
        image_urls: None,
    }));
    steps.push(Step::Store(
        "t",
        json!({ "role": "tool", "tool_call_id": id, "content": content }),
    ));
    steps
}

fn tool_call(id: &str, name: &str, arguments: &Value) -> Value {
    json!({ "id": id, "type": "function", "function": { "name": name, "arguments": arguments.to_string() } })
}

/// A full turn, `picture` the shot a `see_build` returns.
pub fn full(answer: &str, picture: &std::path::Path) -> Vec<Step> {
    let opening = "I will lay a stone base first, then check it in Minecraft.";
    let program =
        "world.fill((0, 0, 0), (2, 0, 2), \"stone\");\nworld.set((1, 1, 1), \"redstone_lamp\");";
    let build_args = json!({ "summary": "Build a stone base and a lamp", "program": program });
    let test_args = json!({ "tests": [
        { "name": "lamp lights", "ticks": 20 },
        { "name": "lever toggles", "ticks": 10 }
    ] });
    let see_args = json!({ "shots": [{ "view": "iso_se" }] });
    let bash_args = json!({ "command": "cat /work/notes.txt" });
    let plan = |statuses: [&str; 3]| {
        json!([
            { "text": "Lay the stone base", "status": statuses[0] },
            { "text": "Place the lamp", "status": statuses[1] },
            { "text": "Check it in Minecraft", "status": statuses[2] }
        ])
    };
    let mut steps = words(REASONING, true);
    steps.push(Step::Event(TurnEvent::Reasoning(REASONING.into())));
    steps.extend(words(opening, false));
    steps.push(Step::Event(TurnEvent::Assistant {
        text: opening.into(),
        last: false,
    }));
    steps.push(Step::Event(TurnEvent::Plan(plan([
        "active", "pending", "pending",
    ]))));
    steps.push(Step::Store(
        "a",
        json!({
            "role": "assistant", "content": opening, "reasoning": REASONING,
            "tool_calls": [tool_call("call-build", "build", &build_args)]
        }),
    ));
    steps.extend(call(
        "call-build",
        "build",
        &build_args,
        &[],
        &json!({ "ok": true, "diff": { "added": 10, "removed": 0, "changed": 1 } }).to_string(),
        Vec::new(),
    ));
    steps.push(Step::Event(TurnEvent::HarnessStatus(Some(
        "Compacting context".into(),
    ))));
    steps.push(Step::Event(TurnEvent::Notice {
        level: "info".into(),
        text: "Context compacted to 40% of the window".into(),
    }));
    steps.push(Step::Event(TurnEvent::HarnessStatus(None)));
    steps.push(Step::Event(TurnEvent::Plan(plan([
        "done", "done", "active",
    ]))));
    // A completion that breaks off: what it streamed is dropped.
    steps.extend(words("Checking the lamp ", false));
    steps.push(Step::Event(TurnEvent::CompletionRetry {
        attempt: 2,
        delay_ms: 0,
        error: "The provider is overloaded".into(),
    }));
    steps.push(Step::Store(
        "a",
        json!({
            "role": "assistant", "content": "",
            "notices": [{ "level": "info", "text": "Context compacted to 40% of the window" }],
            "tool_calls": [
                tool_call("call-test", "test_build", &test_args),
                tool_call("call-see", "see_build", &see_args),
                tool_call("call-bash", "bash", &bash_args)
            ]
        }),
    ));
    steps.extend(call(
        "call-test",
        "test_build",
        &test_args,
        &[
            "Test 1: tick 5 of 20; Test 2: waiting",
            "Test 1: tick 20 of 20; Test 2: tick 10 of 10",
        ],
        &json!({ "ok": true, "tests": [
            { "name": "lamp lights", "passed": true },
            { "name": "lever toggles", "passed": true }
        ] })
        .to_string(),
        Vec::new(),
    ));
    steps.extend(call(
        "call-see",
        "see_build",
        &see_args,
        &[],
        &json!({ "ok": true }).to_string(),
        vec![picture.to_path_buf()],
    ));
    steps.push(Step::Shot("shot-1.png"));
    steps.extend(call(
        "call-bash",
        "bash",
        &bash_args,
        &[],
        &json!({ "ok": false, "error": "cat: /work/notes.txt: No such file or directory" })
            .to_string(),
        Vec::new(),
    ));
    steps.push(Step::Event(TurnEvent::Notice {
        level: "warning".into(),
        text: "The model fell back to a smaller context window".into(),
    }));
    steps.push(Step::Event(TurnEvent::Plan(plan(["done", "done", "done"]))));
    steps.extend(words(answer, false));
    steps.push(Step::Event(TurnEvent::Assistant {
        text: answer.into(),
        last: true,
    }));
    steps.push(Step::Store(
        "a",
        json!({
            "role": "assistant", "content": answer,
            "notices": [{ "level": "warning", "text": "The model fell back to a smaller context window" }],
            "plan": plan(["done", "done", "done"])
        }),
    ));
    steps
}
