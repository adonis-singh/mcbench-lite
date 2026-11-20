//! The text a model reads from a failed run, as today's JavaScript makes it:
//! `mc-dsl.js` `formatProgramError` over `build-sandbox.js`'s errors, and the
//! checks `runProgram` makes of the child's untrusted answer.

/// JavaScript's `\s` and what `String.prototype.trim` removes: WhiteSpace
/// (TAB, VT, FF, SP, NBSP, ZWNBSP and the Zs category) and LineTerminator
/// (LF, CR, LS, PS). Rust's `char::is_whitespace` differs (it has U+0085 and
/// lacks U+FEFF).
pub(crate) fn js_space(c: char) -> bool {
    matches!(
        c,
        '\u{9}' | '\u{a}' | '\u{b}' | '\u{c}' | '\u{d}' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200a}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202f}'
                | '\u{205f}'
                | '\u{3000}'
                | '\u{feff}'
    )
}

/// `String.prototype.trim`.
pub(crate) fn js_trim(text: &str) -> &str {
    text.trim_matches(js_space)
}

/// `/^\s+at /`.
fn is_frame(line: &str) -> bool {
    let rest = line.trim_start_matches(js_space);
    rest.len() < line.len() && rest.starts_with("at ")
}

/// `/^build\.js:\d+/`.
fn is_build_line(line: &str) -> bool {
    line.strip_prefix("build.js:")
        .is_some_and(|rest| rest.starts_with(|c: char| c.is_ascii_digit()))
}

/// `mc-dsl.js` `formatProgramError` of an error whose `stack` is `stack`:
/// the head (the decorated source excerpt when it starts with `build.js:N`,
/// otherwise its last line) and the stack frames inside the program.
pub(crate) fn format_stack(stack: &str) -> String {
    let lines: Vec<&str> = stack.split('\n').collect();
    let first_frame = lines.iter().position(|line| is_frame(line));
    let (head, frames): (&[&str], Vec<&str>) = match first_frame {
        None => (&lines, Vec::new()),
        Some(index) => (
            &lines[..index],
            lines[index..]
                .iter()
                .copied()
                .filter(|line| line.contains("build.js:"))
                .collect(),
        ),
    };
    // head.slice(-1) of an empty head is empty.
    let kept: &[&str] = if head.first().is_some_and(|line| is_build_line(line)) {
        head
    } else {
        &head[head.len().saturating_sub(1)..]
    };
    let joined: Vec<&str> = kept.iter().copied().chain(frames).collect();
    js_trim(&joined.join("\n")).to_string()
}

/// What `formatProgramError` makes of `new Error(message)` raised in
/// `build-sandbox.js` itself (a crash, the wall clock, a malformed answer):
/// its stack's head is `Error: <message>` and none of its frames is in
/// `build.js`.
pub(crate) fn format_parent_error(message: &str) -> String {
    format_stack(&format!(
        "Error: {message}\n    at runProgram (build-sandbox.js)"
    ))
}

/// What `formatProgramError` makes of the child's `FAIL` (`build-sandbox.js`
/// `ProgramError`): the stack when there is one, otherwise `String(err)`,
/// which is the message.
pub(crate) fn format_failure(stack: &str, message: &str) -> String {
    if stack.is_empty() {
        message.to_string()
    } else {
        format_stack(stack)
    }
}

/// The JSON the child writes, made parseable for `serde_json` the way
/// `JSON.parse` reads it: `JSON.stringify` escapes a lone surrogate as
/// `\udXXX`, which `JSON.parse` keeps and `serde_json` refuses; a Rust
/// `String` cannot hold one, so each becomes U+FFFD (`\ufffd`), one UTF-16
/// unit as the surrogate was.
pub(crate) fn replace_lone_surrogates(json: &str) -> std::borrow::Cow<'_, str> {
    if !json.contains("\\u") {
        return std::borrow::Cow::Borrowed(json);
    }
    let bytes = json.as_bytes();
    let hex = |at: usize| -> Option<u16> {
        let digits = json.get(at..at + 4)?;
        u16::from_str_radix(digits, 16).ok()
    };
    let escape_at = |at: usize| -> Option<u16> {
        if bytes.get(at) == Some(&b'\\') && bytes.get(at + 1) == Some(&b'u') {
            hex(at + 2)
        } else {
            None
        }
    };
    let mut out = String::with_capacity(json.len());
    let mut i = 0;
    let mut in_string = false;
    let mut last = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'"' => {
                in_string = !in_string;
                i += 1;
            }
            b'\\' if in_string => {
                if bytes.get(i + 1) != Some(&b'u') {
                    i += 2;
                    continue;
                }
                let Some(unit) = hex(i + 2) else {
                    i += 2;
                    continue;
                };
                if (0xd800..0xdc00).contains(&unit)
                    && escape_at(i + 6).is_some_and(|next| (0xdc00..0xe000).contains(&next))
                {
                    i += 12;
                } else if (0xd800..0xe000).contains(&unit) {
                    out.push_str(&json[last..i]);
                    out.push_str("\\ufffd");
                    i += 6;
                    last = i;
                } else {
                    i += 6;
                }
            }
            _ => i += 1,
        }
    }
    out.push_str(&json[last..]);
    std::borrow::Cow::Owned(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_decorated_stack_keeps_its_excerpt_and_the_program_frames() {
        let stack = "build.js:4\n  mc.set([0, parts[1].h, 0], 'stone');\n                  ^\n\nTypeError: Cannot read properties of undefined (reading 'h')\n    at module.exports (build.js:4:19)\n    at runProgram (/server/mc-dsl.js:500:3)";
        assert_eq!(
            format_stack(stack),
            "build.js:4\n  mc.set([0, parts[1].h, 0], 'stone');\n                  ^\n\nTypeError: Cannot read properties of undefined (reading 'h')\n    at module.exports (build.js:4:19)"
        );
    }

    #[test]
    fn a_plain_stack_keeps_its_last_head_line() {
        assert_eq!(
            format_parent_error("Program process exceeded 10000 ms and was terminated"),
            "Error: Program process exceeded 10000 ms and was terminated"
        );
        assert_eq!(format_stack("one\ntwo"), "two");
        assert_eq!(format_stack("    at x (y.js:1:1)"), "");
        assert_eq!(format_failure("", "42"), "42");
    }

    #[test]
    fn lone_surrogates_become_replacement_characters() {
        assert_eq!(
            replace_lone_surrogates(r#"["a\udc00b","\ud83d\ude00","\\udc00","\ud800"]"#),
            r#"["a\ufffdb","\ud83d\ude00","\\udc00","\ufffd"]"#
        );
    }
}
