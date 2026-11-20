//! The text a model reads from `bash`: `sandbox.js` `formatResult`,
//! `truncateMiddle` and the fixed notes and messages, byte for byte.
//!
//! Today's strings are JavaScript strings, so every length here is counted
//! in UTF-16 code units, as `String.prototype.slice` and `.length` count
//! them, and every size in UTF-8 bytes, as `Buffer.byteLength` counts it. A
//! cut that splits a surrogate pair leaves a lone surrogate in JavaScript;
//! Rust's `String` holds none, so it becomes U+FFFD here (what `Buffer`
//! writes for it; `JSON.stringify` would instead write `\udXXX`).
//!
//! Held by `tests/transcript_node.rs` against Node's own `formatResult` on
//! generated cases (`MCBENCH_SANDBOX_NODE`), and by the differential test
//! against Docker.

/// `MAX_OUTPUT_BYTES`: the model-facing budget, middle-truncated beyond it.
pub const MAX_OUTPUT_BYTES: usize = 20_000;
/// `MAX_COLLECT_BYTES`: the runaway guard while collecting.
pub const MAX_COLLECT_BYTES: usize = 8 * 1024 * 1024;
/// `MAX_TIMEOUT_MS`, as the timeout line writes it.
pub const MAX_TIMEOUT_MS: u64 = 120_000;

/// The persistent shell's note after a timeout (`persistent`).
pub const SESSION_RESET_NOTE: &str =
    "(shell session was reset: cwd and env cleared, /work files intact)";
/// The networked command's note (`networked`).
pub const NETWORK_NOTE: &str = "(one-shot networked shell — session state not kept)";
/// The write-disabled command's note (`restricted`).
pub const RESTRICTED_NOTE: &str =
    "(one-shot shell with file writes disabled — session state not kept)";
/// `exec`'s refusal when the networked command is off (`disableNetwork`).
pub const NETWORK_DISABLED: &str =
    "Network access is disabled in this workspace; the bash tool has no network mode here";
/// `runInSession`'s error when the shell ended under a command.
pub const SHELL_EXITED: &str = "Sandbox shell exited before the command completed";
/// The shell builtins a write-disabled command runs first (`restricted`).
pub const WRITES_DISABLED_LIMITS: &str = "ulimit -f 0; trap \"\" XFSZ; ";

/// What `runInSession` keeps when the persistent shell's output passed
/// [`MAX_COLLECT_BYTES`]: the first 4,000 code units and this line.
pub fn session_overflow(text: &Utf16) -> String {
    format!(
        "{}\n[output exceeded {MAX_COLLECT_BYTES} bytes — command killed, shell session reset]",
        text.slice(0, 4000)
    )
}

/// What `docker()` appends to a one-shot command's output past
/// [`MAX_COLLECT_BYTES`].
pub fn one_shot_overflow() -> String {
    format!("\n[output exceeded {MAX_COLLECT_BYTES} bytes — command killed]")
}

/// The halt's reason the next result carries (`halt`).
pub fn halt_reason(refusal: &str) -> String {
    format!("The sandbox's processes were ended: {refusal} Delete files in /work to continue.")
}

/// The storage message of a write-disabled command (`restricted`): the
/// refusal before it and, after it, `still`'s refusal or the all-clear.
pub fn storage_message(refusal: &str, still: Option<&str>) -> String {
    let after = match still {
        Some(still) => format!("After this command: {still}"),
        None => {
            "After this command the chat is back within its limits; the next command runs normally."
                .to_string()
        }
    };
    format!(
        "{refusal} While that lasts, bash runs each command in a fresh shell with file writes disabled (ulimit -f 0): it can read, delete and truncate files in /work, and anything that writes fails with \"File too large\". {after}"
    )
}

/// `move`'s error.
pub fn cannot_become(chat: &str, from: &str, next: &str) -> String {
    format!("The sandbox of {chat} is {from} and cannot become {next}")
}

/// A JavaScript number as `String(n)` writes it, for the values this text
/// has (finite, under 1e21, not under 1e-6).
pub fn js_number(n: f64) -> String {
    if n == 0.0 {
        "0".to_string()
    } else {
        format!("{n}")
    }
}

/// `formatResult(body, exitCode, durationMs, timedOut, ms, note)`.
pub fn format_result(
    body: &str,
    exit_code: i32,
    duration_ms: u64,
    timed_out: bool,
    ms: f64,
    note: Option<&str>,
) -> String {
    let text = if timed_out {
        format!(
            "command timed out after {} milliseconds (pass timeout_ms to allow more, max {MAX_TIMEOUT_MS})\n{body}",
            js_number(ms)
        )
    } else {
        body.to_string()
    };
    let cut = truncate_middle(&text, MAX_OUTPUT_BYTES);
    // Math.round(durationMs / 100) / 10
    let wall = ((duration_ms as f64 / 100.0) + 0.5).floor() / 10.0;
    let mut sections = vec![
        format!("Exit code: {exit_code}"),
        format!("Wall time: {} seconds", js_number(wall)),
    ];
    if let Some(total) = cut.total_lines {
        sections.push(format!("Total output lines: {total} (truncated)"));
    }
    if let Some(note) = note {
        sections.push(note.to_string());
    }
    sections.push("Output:".to_string());
    sections.push(cut.text);
    sections.join("\n")
}

/// `truncateMiddle`'s answer.
#[derive(Debug, PartialEq, Eq)]
pub struct Cut {
    /// The text, whole or cut.
    pub text: String,
    /// Its line count, when it was cut.
    pub total_lines: Option<usize>,
}

/// `truncateMiddle(text, maxBytes)`: Codex-style middle truncation, the head
/// and the tail kept (in code units) and what was cut reported.
pub fn truncate_middle(text: &str, max_bytes: usize) -> Cut {
    if text.len() <= max_bytes {
        return Cut {
            text: text.to_string(),
            total_lines: None,
        };
    }
    let units = Utf16::from(text);
    let half = max_bytes / 2;
    let len = units.len();
    let head = units.slice(0, half);
    // `text.slice(-half)`: with half 0 that is `slice(-0)`, the whole text.
    let tail = if half == 0 {
        units.slice(0, len)
    } else {
        units.slice(len.saturating_sub(half), len)
    };
    let lines = |s: &str| s.matches('\n').count() + 1;
    let total = lines(text);
    let kept = lines(&head) + lines(&tail);
    Cut {
        text: format!(
            "{head}\n[... omitted {} of {total} lines ...]\n{tail}",
            total.saturating_sub(kept)
        ),
        total_lines: Some(total),
    }
}

/// A string as JavaScript holds it: UTF-16 code units.
pub struct Utf16(Vec<u16>);

impl From<&str> for Utf16 {
    fn from(text: &str) -> Self {
        Utf16(text.encode_utf16().collect())
    }
}

impl Utf16 {
    /// `.length`.
    pub fn len(&self) -> usize {
        self.0.len()
    }
    /// Whether it is empty.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    /// `.slice(start, end)` for `start <= end`, with a lone surrogate at a
    /// cut written as U+FFFD.
    pub fn slice(&self, start: usize, end: usize) -> String {
        let end = end.min(self.0.len());
        let start = start.min(end);
        String::from_utf16_lossy(&self.0[start..end])
    }
}

/// UTF-8 decoded as a stream: a character split between chunks is joined,
/// an invalid sequence becomes U+FFFD; the text is counted in UTF-16 code
/// units as it grows. (Today's persistent shell decodes each pipe read on
/// its own, `chunk.toString('utf8')`, so a character that a read happens to
/// split becomes two U+FFFD there; where reads split is up to the kernel.)
#[derive(Default)]
pub struct Decoder {
    pending: Vec<u8>,
    text: String,
    units: usize,
}

impl Decoder {
    /// Adds a chunk.
    pub fn push(&mut self, chunk: &[u8]) {
        self.pending.extend_from_slice(chunk);
        let mut at = 0;
        loop {
            match std::str::from_utf8(&self.pending[at..]) {
                Ok(valid) => {
                    self.add(valid.to_string());
                    at = self.pending.len();
                    break;
                }
                Err(error) => {
                    let valid = error.valid_up_to();
                    let good = std::str::from_utf8(&self.pending[at..at + valid])
                        .expect("valid_up_to is valid")
                        .to_string();
                    self.add(good);
                    at += valid;
                    match error.error_len() {
                        Some(bad) => {
                            self.add("\u{FFFD}".to_string());
                            at += bad;
                        }
                        None => break,
                    }
                }
            }
        }
        self.pending.drain(..at);
    }

    fn add(&mut self, text: String) {
        self.units += text.encode_utf16().count();
        self.text.push_str(&text);
    }

    /// The text so far (a character still incomplete is left out).
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Its length in UTF-16 code units.
    pub fn units(&self) -> usize {
        self.units
    }

    /// The text, with an incomplete character at the end as U+FFFD (what
    /// `Buffer.toString` writes for it).
    pub fn finish(mut self) -> String {
        if !self.pending.is_empty() {
            self.text.push('\u{FFFD}');
        }
        self.text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_output_is_kept_whole() {
        assert_eq!(
            format_result("hi", 0, 1234, false, 10_000.0, None),
            "Exit code: 0\nWall time: 1.2 seconds\nOutput:\nhi"
        );
        assert_eq!(
            format_result("", 124, 1050, true, 1000.0, Some(SESSION_RESET_NOTE)),
            "Exit code: 124\nWall time: 1.1 seconds\n(shell session was reset: cwd and env cleared, /work files intact)\nOutput:\ncommand timed out after 1000 milliseconds (pass timeout_ms to allow more, max 120000)\n"
        );
        assert_eq!(
            format_result("x", 0, 40, false, 1e4, None),
            "Exit code: 0\nWall time: 0 seconds\nOutput:\nx"
        );
        assert_eq!(
            format_result("x", 0, 250, false, 1e4, None),
            "Exit code: 0\nWall time: 0.3 seconds\nOutput:\nx"
        );
    }

    #[test]
    fn long_output_keeps_its_head_and_tail_in_code_units() {
        let text = "é".repeat(15_000);
        let cut = truncate_middle(&text, MAX_OUTPUT_BYTES);
        assert_eq!(cut.total_lines, Some(1));
        assert_eq!(
            cut.text,
            format!(
                "{0}\n[... omitted 0 of 1 lines ...]\n{0}",
                "é".repeat(10_000)
            )
        );
        let lines: String = (0..30_000).map(|i| format!("{i}\n")).collect();
        let cut = truncate_middle(&lines, MAX_OUTPUT_BYTES);
        assert_eq!(cut.total_lines, Some(30_001));
    }

    #[test]
    fn the_decoder_joins_a_character_split_between_chunks() {
        let mut decoder = Decoder::default();
        let bytes = "a😀b".as_bytes();
        decoder.push(&bytes[..2]);
        decoder.push(&bytes[2..]);
        decoder.push(&[0xff, b'c']);
        assert_eq!(decoder.units(), 6);
        assert_eq!(decoder.finish(), "a😀b\u{FFFD}c");
    }
}
