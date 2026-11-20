//! The history a request sends (`server/agent-context.js`
//! `contextMessages`): over the budget, the oldest complete tool exchanges
//! are archived to `/work/context/<hash>.json` and replaced by a note the
//! model reads; the thinking of messages kept across an archiving goes.
//!
//! It works on the messages as `server/store.js` holds them in memory, as
//! JSON values in their key order ([`mcbench_store::StoredMessage::in_memory_text`]
//! parsed): the archive's bytes and its name are `JSON.stringify` of them,
//! and both are model-visible.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use mcbench_providers::api::js;

/// What a message costs beside its JSON for each image (`IMAGE_BYTES`).
pub const IMAGE_BYTES: usize = 6_000;
/// The budget when `MCBENCH_CONTEXT_MAX_BYTES` is not set.
pub const DEFAULT_MAX_BYTES: usize = 512_000;
/// The newest exchanges never archived (`keepExchanges`' default).
pub const KEEP_EXCHANGES: usize = 6;

/// `MCBENCH_CONTEXT_MAX_BYTES` as `agent-context.js` reads it at load:
/// `Number(value || 512000)`, a safe integer of at least 16000.
pub fn max_bytes_from(value: Option<&str>) -> Result<usize, String> {
    let refused = || "MCBENCH_CONTEXT_MAX_BYTES must be an integer of at least 16000".to_string();
    let text = value.filter(|v| !v.is_empty());
    let Some(text) = text else {
        return Ok(DEFAULT_MAX_BYTES);
    };
    let number = js_number(text);
    if number.fract() != 0.0 || !(16_000.0..=9_007_199_254_740_991.0).contains(&number) {
        return Err(refused());
    }
    Ok(number as usize)
}

/// `Number(text)` for the decimal forms an operator writes.
fn js_number(text: &str) -> f64 {
    let text = js::trim(text);
    if text.is_empty() {
        return 0.0;
    }
    if let Some(hex) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        return u64::from_str_radix(hex, 16).map_or(f64::NAN, |v| v as f64);
    }
    if text
        .bytes()
        .all(|b| b.is_ascii_digit() || matches!(b, b'+' | b'-' | b'.' | b'e' | b'E'))
    {
        return text.parse().unwrap_or(f64::NAN);
    }
    f64::NAN
}

/// Where archives are written: the chat's `/work` (`sandbox.js` `workDir`,
/// `writeWorkFile`).
pub trait WorkFiles: Send + Sync {
    /// `workDir(chat)`: the folder, with its `images/` and `attachments/`.
    fn work_dir(&self, chat: &str) -> Result<PathBuf, String>;
    /// `writeWorkFile(chat, relative, text)`.
    fn write(&self, chat: &str, relative: &str, text: &str) -> Result<(), String>;
}

impl WorkFiles for mcbench_tools::WorkFolders {
    fn work_dir(&self, chat: &str) -> Result<PathBuf, String> {
        mcbench_tools::WorkFolders::work_dir(self, chat)
    }

    fn write(&self, chat: &str, relative: &str, text: &str) -> Result<(), String> {
        mcbench_tools::WorkFolders::write(self, chat, relative, text)
    }
}

/// The archives this process wrote (`written`): a file still as written
/// (the same inode, size, modification and change times) is not written
/// again.
#[derive(Default)]
pub struct Archives {
    written: Mutex<HashMap<String, String>>,
}

fn file_stat(file: &std::path::Path) -> Option<String> {
    let stat = std::fs::symlink_metadata(file).ok()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Some(format!(
            "{}:{}:{}.{}:{}.{}",
            stat.ino(),
            stat.size(),
            stat.mtime(),
            stat.mtime_nsec(),
            stat.ctime(),
            stat.ctime_nsec()
        ))
    }
    #[cfg(not(unix))]
    {
        Some(format!("{}:{:?}", stat.len(), stat.modified().ok()))
    }
}

impl Archives {
    fn write(
        &self,
        work: &dyn WorkFiles,
        chat: &str,
        dir: &std::path::Path,
        file: &str,
        text: &str,
    ) -> Result<(), String> {
        let key = format!("{chat}\n{file}");
        let at = dir.join(file);
        let mut written = self.written.lock().expect("the archives' lock");
        if let Some(known) = written.get(&key)
            && Some(known) == file_stat(&at).as_ref()
        {
            return Ok(());
        }
        work.write(chat, file, text)?;
        match file_stat(&at) {
            Some(stat) => written.insert(key, stat),
            None => written.remove(&key),
        };
        Ok(())
    }
}

fn get<'a>(message: &'a Value, key: &str) -> Option<&'a Value> {
    js::get(Some(message), key)
}

/// `measure(message)`: its JSON's bytes and `IMAGE_BYTES` per image.
pub fn measure(message: &Value) -> usize {
    let images = match get(message, "content") {
        Some(Value::Array(parts)) => parts
            .iter()
            .filter(|part| match get(part, "type").and_then(Value::as_str) {
                Some("image_url") => true,
                Some("attachment") => get(part, "attachment")
                    .and_then(|a| get(a, "type"))
                    .and_then(Value::as_str)
                    .is_some_and(|t| t.starts_with("image/")),
                _ => false,
            })
            .count(),
        _ => 0,
    };
    mcbench_json::stringify(message).len() + images * IMAGE_BYTES
}

struct Exchange {
    start: usize,
    end: usize,
    text: String,
    file: String,
    replacement: Value,
    saves: usize,
}

fn outcome_of(first: &Value, message: &Value) -> Result<Value, String> {
    let id = get(message, "tool_call_id");
    let call = get(first, "tool_calls")
        .and_then(Value::as_array)
        .and_then(|calls| calls.iter().find(|call| get(call, "id") == id))
        .ok_or("Cannot read properties of undefined (reading 'function')")?;
    let mut out = Map::new();
    if let Some(name) = get(call, "function").and_then(|f| get(f, "name")) {
        out.insert("tool".into(), name.clone());
    }
    let result = get(message, "content")
        .and_then(Value::as_str)
        .and_then(|text| mcbench_json::parse(text).ok());
    if js::truthy(result.as_ref()) {
        let result = result.expect("truthy");
        let field = |key: &str| get(&result, key).cloned();
        for key in ["ok", "worldRevision", "report"] {
            if let Some(value) = field(key) {
                out.insert(key.into(), value);
            }
        }
        match get(&result, "tests") {
            None | Some(Value::Null) => {}
            Some(Value::Array(tests)) => {
                let reports = tests
                    .iter()
                    .map(|test| match test {
                        Value::Null => Err("Cannot read properties of null (reading 'report')"),
                        test => Ok(get(test, "report").cloned().unwrap_or(Value::Null)),
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                out.insert("reports".into(), Value::Array(reports));
            }
            Some(_) => return Err("result.tests?.map is not a function".into()),
        }
        if let Some(Value::String(error)) = get(&result, "error") {
            out.insert("error".into(), Value::String(js::slice_utf16(error, 400)));
        }
    }
    Ok(Value::Object(out))
}

/// `toolExchanges(conversation, messages)`.
fn tool_exchanges(messages: &[Value], sizes: &[usize]) -> Result<Vec<Exchange>, String> {
    let mut exchanges = Vec::new();
    let mut i = 0;
    while i < messages.len() {
        let first = &messages[i];
        let calls = match get(first, "tool_calls") {
            Some(Value::Array(calls)) if !calls.is_empty() => calls,
            _ => {
                i += 1;
                continue;
            }
        };
        let mut ids: Vec<&Value> = Vec::new();
        for call in calls {
            let id = get(call, "id").unwrap_or(&Value::Null);
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
        let mut end = i + 1;
        while end < messages.len()
            && get(&messages[end], "role").and_then(Value::as_str) == Some("tool")
            && ids.contains(&get(&messages[end], "tool_call_id").unwrap_or(&Value::Null))
        {
            end += 1;
        }
        if end - i - 1 != ids.len() {
            i += 1;
            continue;
        }
        while end < messages.len() && js::truthy(get(&messages[end], "seeBuild")) {
            end += 1;
        }
        let original = &messages[i..end];
        let text = mcbench_json::stringify(&Value::Array(original.to_vec()));
        let hash: String = Sha256::digest(text.as_bytes())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        let file = format!("context/{}.json", &hash[..24]);
        let outcomes = original
            .iter()
            .filter(|m| get(m, "role").and_then(Value::as_str) == Some("tool"))
            .map(|m| outcome_of(first, m))
            .collect::<Result<Vec<_>, _>>()?;
        let content = match get(first, "content") {
            Some(Value::String(text)) => text.as_str(),
            _ => "",
        };
        let mut replacement = Map::new();
        replacement.insert("role".into(), Value::String("assistant".into()));
        replacement.insert(
            "content".into(),
            Value::String(format!(
                "{content}\n[Earlier tool exchange archived at /work/{file}. Outcomes: {}. Read that file with bash if the original program, observations, or output is needed.]",
                mcbench_json::stringify(&Value::Array(outcomes))
            )),
        );
        let replacement = Value::Object(replacement);
        let bytes: usize = sizes[i..end].iter().sum();
        let saves = bytes.saturating_sub(measure(&replacement));
        exchanges.push(Exchange {
            start: i,
            end,
            text,
            file,
            replacement,
            saves,
        });
        i = end;
    }
    Ok(exchanges)
}

#[cfg(test)]
#[path = "context_tests.rs"]
mod tests;

/// `contextMessages(conversation, { maxBytes, keepExchanges })` over the
/// chat's messages in memory: the history the next request sends, archives
/// written through `work`.
pub fn context_messages(
    chat: &str,
    all: &[Value],
    max_bytes: usize,
    keep_exchanges: usize,
    archives: &Archives,
    work: &dyn WorkFiles,
) -> Result<Vec<Value>, String> {
    let messages: Vec<&Value> = all
        .iter()
        .filter(|m| get(m, "role").and_then(Value::as_str) != Some("error"))
        .collect();
    let messages: Vec<Value> = messages.into_iter().cloned().collect();
    let sizes: Vec<usize> = messages.iter().map(measure).collect();
    let exchanges = tool_exchanges(&messages, &sizes)?;
    let mut requests: Vec<usize> = messages
        .iter()
        .enumerate()
        .filter(|(_, m)| get(m, "role").and_then(Value::as_str) == Some("assistant"))
        .map(|(i, _)| i)
        .collect();
    requests.push(messages.len());
    let (mut sent, mut next, mut saved, mut archived, mut cut) = (0usize, 0, 0usize, 0, 0);
    // `maxBytes / 2` is a JavaScript division.
    let half = max_bytes as f64 / 2.0;
    for at in requests {
        while next < at {
            sent += sizes[next];
            next += 1;
        }
        if sent - saved <= max_bytes {
            continue;
        }
        let complete = exchanges.iter().filter(|e| e.end <= at).count();
        while (archived as i64) < complete as i64 - keep_exchanges as i64
            && (sent - saved) as f64 > half
        {
            saved += exchanges[archived].saves;
            archived += 1;
        }
        cut = at;
    }
    if sent - saved > max_bytes {
        return Err(format!(
            "Protected user messages and recent tool exchanges exceed MCBENCH_CONTEXT_MAX_BYTES ({}). Increase that setting or continue in a new chat; history was not silently discarded.",
            max_bytes
        ));
    }
    // Each output message with the index of the stored one it is.
    let mut output: Vec<(Option<usize>, Value)> = Vec::new();
    let mut index = 0;
    let replaced: Vec<&Exchange> = exchanges[..archived]
        .iter()
        .filter(|e| e.saves > 0)
        .collect();
    let dir = if replaced.is_empty() {
        None
    } else {
        Some(work.work_dir(chat)?)
    };
    for exchange in replaced {
        output.extend((index..exchange.start).map(|i| (Some(i), messages[i].clone())));
        archives.write(
            work,
            chat,
            dir.as_ref().expect("made above"),
            &exchange.file,
            &exchange.text,
        )?;
        output.push((None, exchange.replacement.clone()));
        index = exchange.end;
    }
    output.extend((index..messages.len()).map(|i| (Some(i), messages[i].clone())));
    Ok(output
        .into_iter()
        .map(|(origin, message)| {
            let kept = origin.is_some_and(|i| i < cut);
            if !kept || !js::truthy(get(&message, "reasoning_details")) {
                return message;
            }
            match message {
                Value::Object(map) => Value::Object(
                    map.into_iter()
                        .filter(|(key, _)| key != "reasoning_details")
                        .collect(),
                ),
                other => other,
            }
        })
        .collect())
}
