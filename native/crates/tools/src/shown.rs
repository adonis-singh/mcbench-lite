//! What a tool message holds and what the interface shows of a call and its
//! result, computed here so the interface never parses tool JSON
//! (`transcript-seam.md`):
//!
//! | Item | Today's code |
//! |---|---|
//! | [`message_content`] | `agent.js` `toolMessageContent` |
//! | [`call_of`] ([`call_shown`] for a stored call's text) | `agent.js` `runCall`'s `tool_start` (`summary`, `program`), for a fresh call and a stored one alike |
//! | [`tool_shown`] | `web/app.js` `itemsFromMessages` for a stored tool message (lines 384 to 398), which a fresh result's content goes through too |
//!
//! Where the server's `tool_start` line and the page's stored line differ
//! today, the call's line is the server's (the seam's condition); over the
//! fixture's 1,314 stored calls one class differs: a `bash` call whose
//! arguments also hold `summary` (the page shows that summary).

use std::path::PathBuf;

use serde_json::{Map, Value};

use crate::js;

/// `toolMessageContent(name, result)`: a successful `bash` call's transcript
/// as plain terminal text (a JSON envelope would turn every newline into
/// `\n`), anything else as `JSON.stringify(result)`.
pub fn message_content(name: &str, result: &Value) -> String {
    if name == "bash" && js::truthy(result.get("ok")) {
        return match result.get("text") {
            Some(Value::String(text)) => text.clone(),
            Some(other) => js::to_string(other),
            // `undefined` content is dropped by `JSON.stringify` of the
            // message: the stored row has none.
            None => String::new(),
        };
    }
    js::stringify(result)
}

/// The counts of a build's diff, as a result shows them (numbers, as the
/// result holds them).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Diff {
    /// `added`.
    pub added: f64,
    /// `removed`.
    pub removed: f64,
    /// `changed`.
    pub changed: f64,
}

/// What the interface shows of a tool's result, stored or fresh.
#[derive(Clone, Debug, PartialEq)]
pub struct ToolShown {
    /// The result's `ok` (a tool message that is not JSON, a `bash`
    /// transcript, is ok).
    pub ok: bool,
    /// The result's `cancelled`.
    pub cancelled: bool,
    /// The result's `error`.
    pub error: Option<String>,
    /// The result's `diff`, when it is ok.
    pub diff: Option<Diff>,
    /// The text shown under the call: the result's `output` when a string;
    /// for `test_build` the whole result as JSON indented by 2 in stored key
    /// order; for `bash` whose content is not JSON, the content itself.
    pub output: Option<String>,
    /// The images the call attached or saved, as paths in the data folder
    /// (a fresh `see_build` or `view_image` result; a stored one's images
    /// are its `see_build` user message's).
    pub images: Vec<PathBuf>,
}

/// `itemsFromMessages`' reading of a tool message of call `name`
/// (`web/app.js` 384 to 398), with no images.
pub fn tool_shown(name: &str, content: &str) -> ToolShown {
    let mut shown = ToolShown {
        ok: true,
        cancelled: false,
        error: None,
        diff: None,
        output: None,
        images: Vec::new(),
    };
    // `null` throws at `result.cancelled`, into the same catch.
    let result = match mcbench_json::parse(content) {
        Ok(result) if !result.is_null() => result,
        _ => {
            // bash transcripts are stored as plain terminal text, not JSON.
            if name == "bash" {
                shown.output = Some(content.to_string());
            }
            return shown;
        }
    };
    let field = |key: &str| match &result {
        Value::Object(map) => map.get(key),
        _ => None,
    };
    shown.cancelled = js::truthy(field("cancelled"));
    shown.ok = js::truthy(field("ok"));
    if shown.ok
        && let Some(diff) = field("diff").filter(|diff| js::truthy(Some(diff)))
    {
        let count = |key: &str| diff.get(key).and_then(Value::as_f64).unwrap_or(0.0);
        shown.diff = Some(Diff {
            added: count("added"),
            removed: count("removed"),
            changed: count("changed"),
        });
    }
    if let Some(Value::String(output)) = field("output") {
        shown.output = Some(output.clone());
    }
    if name == "test_build" {
        shown.output = Some(mcbench_json::stringify_pretty(&result, 2));
    }
    if let Some(Value::String(error)) = field("error") {
        shown.error = Some(error.clone());
    }
    shown
}

/// What a call shows before its result: its one-line summary and the
/// program (or command, or `test_build`'s arguments) under it.
#[derive(Clone, Debug, PartialEq)]
pub struct CallShown {
    /// The summary line.
    pub summary: String,
    /// `args && (name === 'test_build' ? JSON.stringify(args, null, 2) :
    /// args.program ?? args.command)`: JSON, because a model may send a
    /// value that is not a string (the event carries it as it is); `None`
    /// where the expression is `undefined` (the event has no `program`).
    pub program: Option<Value>,
}

/// `see_build`'s shots when a call names none (`DEFAULT_SHOTS`).
const DEFAULT_SHOT_VIEWS: [&str; 2] = ["iso_se", "iso_nw"];

/// [`call_of`] for a stored call's raw arguments text
/// (`call.function.arguments`): `JSON.parse(arguments || '{}')`, refused
/// unless an object.
pub fn call_shown(name: &str, arguments: &str) -> CallShown {
    let text = if arguments.is_empty() {
        "{}"
    } else {
        arguments
    };
    match mcbench_json::parse(text) {
        Ok(args @ Value::Object(_)) => call_of(name, &args, false),
        Ok(args) => call_of(name, &args, true),
        Err(_) => call_of(name, &Value::Null, true),
    }
}

/// `runCall`'s `tool_start` line (`agent.js` 706 to 720) for call `name`
/// whose arguments parsed as `args` (`null` when they did not parse),
/// `invalid` when the loop refused them: the one function behind the
/// streamed `tool_start` and a stored call's line.
pub fn call_of(name: &str, args: &Value, invalid: bool) -> CallShown {
    let empty = Map::new();
    let map = match args {
        Value::Object(map) => map,
        _ => &empty,
    };
    let summary = if invalid {
        format!("{} (invalid arguments)", tool_name(name))
    } else {
        summary(name, map)
    };
    let program = if !js::truthy(Some(args)) {
        Some(args.clone())
    } else if name == "test_build" {
        Some(Value::String(mcbench_json::stringify_pretty(args, 2)))
    } else {
        match map.get("program") {
            Some(Value::Null) | None => map.get("command").cloned(),
            some => some.cloned(),
        }
    };
    CallShown { summary, program }
}

fn tool_name(name: &str) -> &str {
    if name.is_empty() { "tool" } else { name }
}

fn summary(name: &str, args: &Map<String, Value>) -> String {
    match name {
        "build" => {
            let summary = args.get("summary");
            if js::truthy(summary) {
                js::to_string(summary.expect("truthy"))
            } else {
                "build".into()
            }
        }
        "test_build" => format!(
            "Test build in Minecraft{}",
            match js::array(args.get("tests")) {
                Some(tests) if tests.len() > 1 => format!(" ({} tests)", tests.len()),
                _ => String::new(),
            }
        ),
        "see_build" => {
            let views: Vec<String> = match js::array(args.get("shots")) {
                Some(shots) => shots
                    .iter()
                    .map(|shot| {
                        let field = |key: &str| match shot {
                            Value::Object(map) => map.get(key),
                            _ => None,
                        };
                        if js::truthy(field("view")) {
                            js::to_string(field("view").expect("truthy"))
                        } else if js::truthy(field("eye")) {
                            "camera".into()
                        } else {
                            "iso_se".into()
                        }
                    })
                    .collect(),
                None => DEFAULT_SHOT_VIEWS.iter().map(|v| v.to_string()).collect(),
            };
            format!("Look at build ({})", views.join(", "))
        }
        "bash" => {
            let command = js::string_or_empty(args.get("command"));
            let first = command.split('\n').next().expect("split gives one part");
            format!("$ {}", js::utf16_prefix(first, 80))
        }
        "view_image" => format!("Look at /work/{}", js::string_or_empty(args.get("path"))),
        "prompt_library" => {
            if js::truthy(args.get("save")) || js::truthy(args.get("remove")) {
                "Update prompt library".into()
            } else {
                "Read prompt library".into()
            }
        }
        _ => tool_name(name).to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_bash_transcript_is_plain_text_and_anything_else_json() {
        assert_eq!(
            message_content(
                "bash",
                &json!({"ok": true, "text": "Exit code: 0\nOutput:\nhi"})
            ),
            "Exit code: 0\nOutput:\nhi"
        );
        assert_eq!(
            message_content("bash", &json!({"ok": false, "error": "x"})),
            r#"{"ok":false,"error":"x"}"#
        );
        assert_eq!(
            message_content("build", &json!({"ok": true, "n": 2.0})),
            r#"{"ok":true,"n":2}"#
        );
    }

    #[test]
    fn a_call_shows_the_servers_summary_and_program() {
        assert_eq!(
            call_shown("build", r#"{"summary":"Walls","program":"x"}"#),
            CallShown {
                summary: "Walls".into(),
                program: Some(json!("x"))
            }
        );
        assert_eq!(
            call_shown("build", "{"),
            CallShown {
                summary: "build (invalid arguments)".into(),
                program: Some(Value::Null)
            }
        );
        assert_eq!(
            call_shown("build", "[1]").summary,
            "build (invalid arguments)"
        );
        assert_eq!(call_shown("build", "").summary, "build");
        assert_eq!(call_shown("build", "{}").program, None);
        assert_eq!(
            call_shown("build", r#"{"program":5}"#).program,
            Some(json!(5))
        );
        assert_eq!(
            call_shown(
                "see_build",
                r#"{"shots":[{"eye":[1,2,3]},{},{"view":"top"}]}"#
            )
            .summary,
            "Look at build (camera, iso_se, top)"
        );
        assert_eq!(
            call_shown("see_build", "{}").summary,
            "Look at build (iso_se, iso_nw)"
        );
        assert_eq!(
            call_shown("bash", r#"{"command":"ls\npwd"}"#),
            CallShown {
                summary: "$ ls".into(),
                program: Some(json!("ls\npwd"))
            }
        );
        assert_eq!(
            call_shown("test_build", r#"{"tests":[{},{}]}"#).summary,
            "Test build in Minecraft (2 tests)"
        );
        assert_eq!(
            call_shown("prompt_library", r#"{"save":[]}"#).summary,
            "Update prompt library"
        );
        assert_eq!(call_shown("", "{}").summary, "tool");
    }

    #[test]
    fn a_result_shows_as_the_page_reads_a_stored_one() {
        let shown = tool_shown(
            "build",
            r#"{"ok":true,"diff":{"added":3,"removed":0,"changed":1}}"#,
        );
        assert_eq!(
            shown.diff,
            Some(Diff {
                added: 3.0,
                removed: 0.0,
                changed: 1.0
            })
        );
        let bash = tool_shown("bash", "Exit code: 0");
        assert!(bash.ok);
        assert_eq!(bash.output.as_deref(), Some("Exit code: 0"));
        let failed = tool_shown(
            "bash",
            r#"{"ok":false,"error":"full","output":"Exit code: 1"}"#,
        );
        assert!(!failed.ok);
        assert_eq!(failed.error.as_deref(), Some("full"));
        assert_eq!(failed.output.as_deref(), Some("Exit code: 1"));
        let test = tool_shown("test_build", r#"{"ok":true,"tests":[]}"#);
        assert_eq!(
            test.output.as_deref(),
            Some("{\n  \"ok\": true,\n  \"tests\": []\n}")
        );
        let cancelled = tool_shown(
            "build",
            r#"{"ok":false,"cancelled":true,"error":"Cancelled by user"}"#,
        );
        assert!(cancelled.cancelled);
    }
}
