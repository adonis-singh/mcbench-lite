//! Code colouring as today's page does it (`web/app.js` `CODE_TOKEN`,
//! `highlightCode`): comments, strings, numbers, a JavaScript keyword list
//! and calls, over any language.

use regex::Regex;
use std::ops::Range;
use std::sync::LazyLock;

/// A token's kind: `c` comment, `s` string, `n` number, `k` keyword, `f`
/// a call (`--tk-*` in `web/style.css`).
pub type Kind = char;

// par-chat (stage four's parity pass): `CODE_TOKEN` matched as
// JavaScript's engine does without the `u` flag: `\b`, `\d` and `\w` are
// ASCII, `.` stops at `\n`, `\r`, U+2028 and U+2029, `\s` is JavaScript's
// set, and the call alternative's lookahead `(?=\s*\()` is tried at every
// position where an identifier can start (inside a longer word too, as the
// engine's scan does: in `$if` the `if` is a keyword), after the first four
// alternatives at the same position.
/// The first four alternatives: comments, strings, numbers, keywords.
static TOKEN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r#"(//[^\n]*|/\*(?s:.)*?\*/)"#,
        r#"|('(?:[^'\\\n]|\\[^\n\r\u{2028}\u{2029}])*'?|"(?:[^"\\\n]|\\[^\n\r\u{2028}\u{2029}])*"?|`(?:[^`\\]|\\[^\n\r\u{2028}\u{2029}])*`?)"#,
        r#"|((?-u:\b)0x[0-9a-fA-F]+(?-u:\b)|(?-u:\b)[0-9]+(?:\.[0-9]+)?(?-u:\b))"#,
        r#"|((?-u:\b)(?:const|let|var|function|return|if|else|for|while|of|in|new|typeof|module|exports|true|false|null|undefined|break|continue|throw|try|catch)(?-u:\b))"#,
    ))
    .expect("the token pattern compiles")
});

/// JavaScript's `\s`.
fn js_space(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\u{0B}' | '\u{0C}' | '\r' | ' ' | '\u{A0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200A}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202F}'
                | '\u{205F}'
                | '\u{3000}'
                | '\u{FEFF}'
    )
}

/// The call alternative at `at`: `[A-Za-z_$][\w$]*(?=\s*\()` with ASCII
/// `\w` (only the whole run can be followed by `(`, so no shorter one).
fn call_at(text: &str, at: usize) -> Option<Range<usize>> {
    let bytes = text.as_bytes();
    let first = *bytes.get(at)?;
    if !(first.is_ascii_alphabetic() || first == b'_' || first == b'$') {
        return None;
    }
    let mut end = at + 1;
    while end < bytes.len()
        && (bytes[end].is_ascii_alphanumeric() || matches!(bytes[end], b'_' | b'$'))
    {
        end += 1;
    }
    text[end..]
        .trim_start_matches(js_space)
        .starts_with('(')
        .then_some(at..end)
}

/// The coloured tokens of `text`, in order (`highlightCode`).
pub fn tokens(text: &str) -> Vec<(Range<usize>, Kind)> {
    let mut out = Vec::new();
    let mut at = 0;
    // The first four's next match, kept while no call passes it.
    let mut next: Option<(Range<usize>, Kind)> = None;
    while at < text.len() {
        if next.as_ref().is_none_or(|(range, _)| range.start < at) {
            next = TOKEN.captures_at(text, at).map(|found| {
                let kind = match (1..=4)
                    .find(|i| found.get(*i).is_some())
                    .expect("one group matched")
                {
                    1 => 'c',
                    2 => 's',
                    3 => 'n',
                    _ => 'k',
                };
                (found.get(0).expect("a match").range(), kind)
            });
        }
        let limit = next.as_ref().map_or(text.len(), |(range, _)| range.start);
        // A call that starts before the first four's match wins; at the
        // same place they come first.
        let call = (at..limit)
            .filter(|i| text.is_char_boundary(*i))
            .find_map(|i| call_at(text, i));
        let (range, kind) = match call {
            Some(range) => (range, 'f'),
            None => match next.take() {
                Some(token) => token,
                None => break,
            },
        };
        // Every alternative takes a character, so this moves on.
        at = range.end;
        out.push((range, kind));
    }
    out
}
// end of par-chat's block

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_coloured_as_the_page_does() {
        let text = "const x = place(1, \"a\") // done";
        let kinds: Vec<(&str, char)> = tokens(text)
            .into_iter()
            .map(|(r, k)| (&text[r], k))
            .collect();
        assert_eq!(
            kinds,
            vec![
                ("const", 'k'),
                ("place", 'f'),
                ("1", 'n'),
                ("\"a\"", 's'),
                ("// done", 'c')
            ]
        );
    }

    // par-chat: the engine's edge cases.
    fn kinds(text: &str) -> Vec<(&str, char)> {
        tokens(text)
            .into_iter()
            .map(|(r, k)| (&text[r], k))
            .collect()
    }

    #[test]
    fn a_keyword_after_a_dollar_or_inside_a_non_ascii_word_is_coloured_as_javascript_does() {
        // `$` is not a word character: `\bif\b` matches inside `$if`.
        assert_eq!(kinds("$if = 1"), vec![("if", 'k'), ("1", 'n')]);
        // `é` is not one either (no `u` flag): `\b` falls on both sides of `for`.
        assert_eq!(kinds("éforé"), vec![("for", 'k')]);
        // A non-ASCII digit is no `\d`.
        assert_eq!(kinds("x = \u{663}"), vec![]);
    }

    #[test]
    fn a_call_is_the_whole_ascii_run_before_its_bracket_wherever_it_starts() {
        assert_eq!(kinds("1foo (x)"), vec![("foo", 'f')]);
        // `\w` is ASCII: `é` ends the run, so `aé(` is no call.
        assert_eq!(kinds("aé("), vec![]);
        assert_eq!(kinds("foo bar("), vec![("bar", 'f')]);
        // JavaScript's `\s` has U+FEFF and not U+0085.
        assert_eq!(kinds("f\u{FEFF}("), vec![("f", 'f')]);
        assert_eq!(kinds("f\u{85}("), vec![]);
        // A keyword before a bracket is a keyword (it comes first).
        assert_eq!(kinds("if (x)"), vec![("if", 'k')]);
    }

    #[test]
    fn an_escape_in_a_string_stops_at_javascripts_line_ends() {
        assert_eq!(kinds("'a\\\u{2028}b'"), vec![("'a", 's'), ("'", 's')]);
        assert_eq!(kinds("\"a\\\"b\""), vec![("\"a\\\"b\"", 's')]);
    }
}
