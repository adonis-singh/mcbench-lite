//! A provider's event stream as `llm.js` `readSse` reads it: the body
//! decoded as a streaming `TextDecoder` decodes it, cut into records at each
//! `\r?\n\r?\n`, each record's `data:` lines trimmed and joined with `\n`,
//! and each record whose data is neither empty nor `[DONE]` parsed with
//! `JSON.parse` (V8's message when it is not JSON). What is left after the
//! last record is emitted when it is not blank.

use serde_json::Value;

use super::http::Answer;
use super::{js, v8json};
use crate::ProviderError;

/// UTF-8 as a WHATWG `TextDecoder` with `{stream: true}` decodes it: a
/// leading byte order mark is dropped, each malformed sequence (its maximal
/// subpart) becomes U+FFFD, and a sequence cut at a chunk's end waits for
/// the next chunk.
#[derive(Default)]
pub struct Utf8Decoder {
    pending: Vec<u8>,
    started: bool,
}

impl Utf8Decoder {
    /// Decodes `bytes` after what is pending.
    pub fn decode(&mut self, bytes: &[u8]) -> String {
        let mut input = std::mem::take(&mut self.pending);
        input.extend_from_slice(bytes);
        let mut out = String::new();
        let mut rest: &[u8] = &input;
        loop {
            match std::str::from_utf8(rest) {
                Ok(valid) => {
                    out.push_str(valid);
                    break;
                }
                Err(error) => {
                    let (valid, after) = rest.split_at(error.valid_up_to());
                    out.push_str(std::str::from_utf8(valid).expect("checked valid"));
                    match error.error_len() {
                        Some(bad) => {
                            out.push('\u{FFFD}');
                            rest = &after[bad..];
                        }
                        None => {
                            self.pending = after.to_vec();
                            break;
                        }
                    }
                }
            }
        }
        if !self.started && !out.is_empty() {
            self.started = true;
            if let Some(stripped) = out.strip_prefix('\u{FEFF}') {
                return stripped.to_owned();
            }
        }
        out
    }

    /// The end of the input (`decoder.decode()`): a sequence left cut is
    /// U+FFFD.
    pub fn finish(&mut self) -> String {
        if self.pending.is_empty() {
            String::new()
        } else {
            self.pending.clear();
            "\u{FFFD}".into()
        }
    }
}

/// The end of the first `\r?\n\r?\n` in `text` at or after `from`, with its
/// start: the leftmost match a JavaScript regular expression finds.
fn record_break(text: &str, from: usize) -> Option<(usize, usize)> {
    let b = text.as_bytes();
    let newline = |at: usize| -> Option<usize> {
        if b.get(at) == Some(&b'\r') && b.get(at + 1) == Some(&b'\n') {
            Some(at + 2)
        } else if b.get(at) == Some(&b'\n') {
            Some(at + 1)
        } else {
            None
        }
    };
    (from..b.len()).find_map(|at| {
        let middle = newline(at)?;
        newline(middle).map(|end| (at, end))
    })
}

/// `record.split(/\r?\n/)`.
fn lines(record: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let b = record.as_bytes();
    let mut at = 0;
    while at < b.len() {
        if b[at] == b'\n' {
            let end = if at > start && b[at - 1] == b'\r' {
                at - 1
            } else {
                at
            };
            out.push(&record[start..end]);
            start = at + 1;
        }
        at += 1;
    }
    out.push(&record[start..]);
    out
}

/// One record's event, if it carries one.
fn event(record: &str) -> Result<Option<Value>, ProviderError> {
    let data: Vec<&str> = lines(record)
        .into_iter()
        .filter(|line| line.starts_with("data:"))
        .map(|line| js::trim(&line[5..]))
        .collect();
    let data = data.join("\n");
    if data.is_empty() || data == "[DONE]" {
        return Ok(None);
    }
    v8json::parse(&data).map(Some).map_err(ProviderError::new)
}

/// Reads `answer`'s events into `on_event` until the body ends; an error
/// from `on_event`, from a record's JSON or from the body ends the read.
pub async fn read(
    answer: &mut Answer,
    on_event: &mut (dyn FnMut(Value) -> Result<(), ProviderError> + Send),
) -> Result<(), ProviderError> {
    let mut decoder = Utf8Decoder::default();
    let mut buffer = String::new();
    while let Some(chunk) = answer
        .chunk()
        .await
        .map_err(|failure| ProviderError::new(failure.message()))?
    {
        buffer.push_str(&decoder.decode(&chunk));
        let mut start = 0;
        let mut records = Vec::new();
        while let Some((end, next)) = record_break(&buffer, start) {
            records.push(buffer[start..end].to_owned());
            start = next;
        }
        buffer.drain(..start);
        for record in records {
            if let Some(value) = event(&record)? {
                on_event(value)?;
            }
        }
    }
    if !js::trim(&buffer).is_empty()
        && let Some(value) = event(&buffer)?
    {
        on_event(value)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_break_where_javascripts_split_does() {
        let text = "a\r\n\r\nb\n\nc\r\n\nd\n\r\ne\r\n\rf";
        let mut start = 0;
        let mut parts = Vec::new();
        while let Some((end, next)) = record_break(text, start) {
            parts.push(&text[start..end]);
            start = next;
        }
        parts.push(&text[start..]);
        assert_eq!(parts, ["a", "b", "c", "d", "e\r\n\rf"]);
        assert_eq!(
            lines("event: ping\r\ndata: x\ny"),
            ["event: ping", "data: x", "y"]
        );
    }

    /// `readSse handles CRLF records split across chunks` (llm.test.js):
    /// the record's pieces arrive apart and are read once whole.
    #[test]
    fn crlf_records_split_across_chunks() {
        let mut decoder = Utf8Decoder::default();
        let mut buffer = String::new();
        let mut events = Vec::new();
        for chunk in ["event: ping\r\n", "data: {\"ok\":", "true}\r\n\r\n"] {
            buffer.push_str(&decoder.decode(chunk.as_bytes()));
            let mut start = 0;
            while let Some((end, next)) = record_break(&buffer, start) {
                if let Some(value) = event(&buffer[start..end]).unwrap() {
                    events.push(value);
                }
                start = next;
            }
            buffer.drain(..start);
        }
        assert_eq!(events, [serde_json::json!({"ok": true})]);
    }

    #[test]
    fn utf8_split_between_chunks_waits_and_bad_bytes_are_replaced() {
        let brick = "🧱".as_bytes();
        let mut decoder = Utf8Decoder::default();
        assert_eq!(
            decoder.decode(&[0xEF, 0xBB, 0xBF, b'a', brick[0], brick[1]]),
            "a"
        );
        assert_eq!(decoder.decode(&brick[2..]), "🧱");
        assert_eq!(
            decoder.decode(&[0xFF, b'b', 0xE2, 0x82, b'c']),
            "\u{FFFD}b\u{FFFD}c"
        );
    }
}
