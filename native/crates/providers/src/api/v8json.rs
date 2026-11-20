//! `JSON.parse` as V8 reads it: the value with `mcbench-json`'s parse, and
//! for text that is not JSON, the exact `SyntaxError` message V8 13.6 (Node
//! 24) gives, since a provider's malformed event reaches the user as that
//! message (`llm.js` `readSse`).
//!
//! The scanner follows V8's `src/json/json-parser.cc`: the token a character
//! starts, where the cursor stands when each error is reported, the message
//! templates of `src/common/message-template.h`, the context around the
//! position (ten characters on each side once the text is 21 or longer), and
//! line and column with `\r\n` counted once. Positions are UTF-16 code
//! units. `tests/json-errors.jsonl` holds V8's messages for every error class
//! (`tests/node/json-errors.js` writes it) and is compared on every run.

use serde_json::Value;

/// `JSON.parse(text)`: the value, or V8's error message.
pub fn parse(text: &str) -> Result<Value, String> {
    let units: Vec<u16> = text.encode_utf16().collect();
    let mut scanner = Scanner {
        s: &units,
        at: 0,
        error: None,
    };
    scanner.document();
    if let Some(message) = scanner.error {
        return Err(message);
    }
    mcbench_json::parse(text)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Token {
    String,
    Number,
    LBrace,
    RBrace,
    LBrack,
    RBrack,
    True,
    False,
    Null,
    Whitespace,
    Colon,
    Comma,
    Illegal,
    Eos,
}

fn token_of(c: u16) -> Token {
    match c {
        0x22 => Token::String,
        0x2D | 0x30..=0x39 => Token::Number,
        0x7B => Token::LBrace,
        0x7D => Token::RBrace,
        0x5B => Token::LBrack,
        0x5D => Token::RBrack,
        0x74 => Token::True,
        0x66 => Token::False,
        0x6E => Token::Null,
        0x20 | 0x09 | 0x0A | 0x0D => Token::Whitespace,
        0x3A => Token::Colon,
        0x2C => Token::Comma,
        _ => Token::Illegal,
    }
}

/// The message templates that take a position.
const EXPECTED_PROP_NAME_OR_RBRACE: &str = "Expected property name or '}' in JSON";
const EXPECTED_COMMA_OR_RBRACK: &str = "Expected ',' or ']' after array element in JSON";
const EXPECTED_COMMA_OR_RBRACE: &str = "Expected ',' or '}' after property value in JSON";
const EXPECTED_COLON: &str = "Expected ':' after property name in JSON";
const EXPECTED_DOUBLE_QUOTED: &str = "Expected double-quoted property name in JSON";
const UNTERMINATED_STRING: &str = "Unterminated string in JSON";
const BAD_CONTROL: &str = "Bad control character in string literal in JSON";
const BAD_UNICODE_ESCAPE: &str = "Bad Unicode escape in JSON";
const BAD_ESCAPE: &str = "Bad escaped character in JSON";
const NO_NUMBER_AFTER_MINUS: &str = "No number after minus sign in JSON";
const EXPONENT_MISSING: &str = "Exponent part is missing a number in JSON";
const UNTERMINATED_FRACTION: &str = "Unterminated fractional number in JSON";
const NON_WHITESPACE_AFTER: &str = "Unexpected non-whitespace character after JSON";

/// A frame of the value under way.
enum Frame {
    Object,
    Array,
}

struct Scanner<'a> {
    s: &'a [u16],
    at: usize,
    error: Option<String>,
}

impl Scanner<'_> {
    fn peek_char(&self) -> Option<u16> {
        self.s.get(self.at).copied()
    }

    fn skip_whitespace(&mut self) -> Token {
        while let Some(c) = self.peek_char() {
            let token = token_of(c);
            if token != Token::Whitespace {
                return token;
            }
            self.at += 1;
        }
        Token::Eos
    }

    /// `(line N column M)` of the cursor.
    fn location(&self) -> (usize, usize) {
        let mut line = 1;
        let mut last_break = 0;
        let mut i = 0;
        while i < self.at {
            if self.s[i] == 0x0D && i + 1 < self.at && self.s[i + 1] == 0x0A {
                i += 1;
            }
            if self.s[i] == 0x0D || self.s[i] == 0x0A {
                line += 1;
                last_break = i + 1;
            }
            i += 1;
        }
        (line, 1 + i - last_break)
    }

    fn positioned(&self, template: &str) -> String {
        let (line, column) = self.location();
        format!(
            "{template} at position {} (line {line} column {column})",
            self.at
        )
    }

    fn text(&self, from: usize, to: usize) -> String {
        String::from_utf16_lossy(&self.s[from..to])
    }

    /// `ReportUnexpectedToken(token, message)`.
    fn report(&mut self, token: Token, message: Option<&str>) {
        if self.error.is_some() {
            return;
        }
        let text = match message {
            Some(template) => self.positioned(template),
            None => match token {
                Token::Eos => "Unexpected end of JSON input".to_owned(),
                Token::Number => self.positioned("Unexpected number in JSON"),
                Token::String => self.positioned("Unexpected string in JSON"),
                _ => {
                    let whole = self.text(0, self.s.len());
                    if ["NaN", "Infinity", "undefined", "[object Object]"].contains(&whole.as_str())
                    {
                        format!("\"{whole}\" is not valid JSON")
                    } else {
                        self.with_ellipses()
                    }
                }
            },
        };
        self.error = Some(text);
    }

    /// `GetErrorMessageWithEllipses`.
    fn with_ellipses(&self) -> String {
        const CONTEXT: usize = 10;
        let token = self.text(self.at, self.at + 1);
        let length = self.s.len();
        let pos = self.at;
        if length < CONTEXT * 2 + 1 {
            return format!(
                "Unexpected token '{token}', \"{}\" is not valid JSON",
                self.text(0, length)
            );
        }
        if pos < CONTEXT {
            format!(
                "Unexpected token '{token}', \"{}\"... is not valid JSON",
                self.text(0, pos + CONTEXT)
            )
        } else if pos < length - CONTEXT {
            format!(
                "Unexpected token '{token}', ...\"{}\"... is not valid JSON",
                self.text(pos - CONTEXT, pos + CONTEXT)
            )
        } else {
            format!(
                "Unexpected token '{token}', ...\"{}\" is not valid JSON",
                self.text(pos - CONTEXT, length)
            )
        }
    }

    /// `ReportUnexpectedCharacter(c)`: the token the character starts.
    fn report_character(&mut self) {
        let token = match self.peek_char() {
            Some(c) if c <= 0xFF => token_of(c),
            Some(_) => Token::Illegal,
            None => Token::Eos,
        };
        self.report(token, None);
    }

    fn failed(&self) -> bool {
        self.error.is_some()
    }

    /// `ExpectNext(token, message)`.
    fn expect_next(&mut self, token: Token, message: &str) -> bool {
        let peeked = self.skip_whitespace();
        if peeked != token {
            self.report(peeked, Some(message));
            return false;
        }
        true
    }

    /// `Check(token)`: skips whitespace, consumes it when it is next.
    fn check(&mut self, token: Token) -> bool {
        if self.skip_whitespace() == token {
            if token != Token::Eos {
                self.at += 1;
            }
            return true;
        }
        false
    }

    fn document(&mut self) {
        self.value();
        if self.failed() {
            return;
        }
        if !self.check(Token::Eos) {
            let token = self.skip_whitespace();
            self.report(token, Some(NON_WHITESPACE_AFTER));
        }
    }

    fn value(&mut self) {
        let mut stack: Vec<Frame> = Vec::new();
        'value: loop {
            // One value.
            let token = self.skip_whitespace();
            match token {
                Token::String => {
                    self.at += 1;
                    self.string();
                }
                Token::Number => self.number(),
                Token::LBrace => {
                    self.at += 1;
                    if self.check(Token::RBrace) {
                        // An empty object: falls to the continuations.
                    } else {
                        if !self.expect_next(Token::String, EXPECTED_PROP_NAME_OR_RBRACE) {
                            return;
                        }
                        self.at += 1;
                        self.string();
                        if self.failed() || !self.expect_next(Token::Colon, EXPECTED_COLON) {
                            return;
                        }
                        self.at += 1;
                        stack.push(Frame::Object);
                        continue 'value;
                    }
                }
                Token::LBrack => {
                    self.at += 1;
                    if !self.check(Token::RBrack) {
                        stack.push(Frame::Array);
                        continue 'value;
                    }
                }
                Token::True => self.literal("true"),
                Token::False => self.literal("false"),
                Token::Null => self.literal("null"),
                _ => {
                    self.report_character();
                    return;
                }
            }
            if self.failed() {
                return;
            }
            // The continuations of the values under way.
            loop {
                match stack.last() {
                    None => return,
                    Some(Frame::Object) => {
                        if self.check(Token::Comma) {
                            if !self.expect_next(Token::String, EXPECTED_DOUBLE_QUOTED) {
                                return;
                            }
                            self.at += 1;
                            self.string();
                            if self.failed() || !self.expect_next(Token::Colon, EXPECTED_COLON) {
                                return;
                            }
                            self.at += 1;
                            continue 'value;
                        }
                        let token = self.skip_whitespace();
                        if token != Token::RBrace {
                            self.report(token, Some(EXPECTED_COMMA_OR_RBRACE));
                            return;
                        }
                        self.at += 1;
                        stack.pop();
                    }
                    Some(Frame::Array) => {
                        if self.check(Token::Comma) {
                            continue 'value;
                        }
                        let token = self.skip_whitespace();
                        if token != Token::RBrack {
                            self.report(token, Some(EXPECTED_COMMA_OR_RBRACK));
                            return;
                        }
                        self.at += 1;
                        stack.pop();
                    }
                }
            }
        }
    }

    /// `ScanLiteral`: the first character was matched.
    fn literal(&mut self, word: &str) {
        let word: Vec<u16> = word.encode_utf16().collect();
        let remaining = self.s.len() - self.at;
        if remaining >= word.len() && self.s[self.at + 1..self.at + word.len()] == word[1..] {
            self.at += word.len();
            return;
        }
        self.at += 1;
        for expected in word
            .iter()
            .skip(1)
            .take((word.len() - 1).min(remaining - 1))
        {
            if self.s[self.at] != *expected {
                self.report_character();
                return;
            }
            self.at += 1;
        }
        self.report(Token::Eos, None);
    }

    /// `ScanJsonString`, the opening quote consumed.
    fn string(&mut self) {
        loop {
            let Some(c) = self.peek_char() else {
                self.report(Token::Illegal, Some(UNTERMINATED_STRING));
                return;
            };
            match c {
                0x22 => {
                    self.at += 1;
                    return;
                }
                0x5C => {
                    self.at += 1;
                    // An escape that is not a Latin-1 character (or the
                    // end) is reported as the character it is.
                    let Some(e) = self.peek_char().filter(|e| *e <= 0xFF) else {
                        self.report_character();
                        return;
                    };
                    match e {
                        0x22 | 0x5C | 0x2F | 0x62 | 0x66 | 0x6E | 0x72 | 0x74 => self.at += 1,
                        0x75 => {
                            for _ in 0..4 {
                                self.at += 1;
                                if !self.peek_char().is_some_and(|h| {
                                    (h as u8 as u16 == h) && (h as u8).is_ascii_hexdigit()
                                }) {
                                    self.report(Token::Illegal, Some(BAD_UNICODE_ESCAPE));
                                    return;
                                }
                            }
                            self.at += 1;
                        }
                        _ => {
                            self.report(Token::Illegal, Some(BAD_ESCAPE));
                            return;
                        }
                    }
                }
                0x00..=0x1F => {
                    self.report(Token::Illegal, Some(BAD_CONTROL));
                    return;
                }
                _ => self.at += 1,
            }
        }
    }

    fn digit(&self) -> bool {
        self.peek_char().is_some_and(|c| (0x30..=0x39).contains(&c))
    }

    /// `ParseJsonNumber`.
    fn number(&mut self) {
        if self.peek_char() == Some(0x2D) {
            self.at += 1;
        }
        if self.peek_char() == Some(0x30) {
            self.at += 1;
            if self.digit() {
                self.report(Token::Number, None);
                return;
            }
        } else {
            if !self.digit() {
                self.report(Token::Illegal, Some(NO_NUMBER_AFTER_MINUS));
                return;
            }
            while self.digit() {
                self.at += 1;
            }
        }
        if self.peek_char() == Some(0x2E) {
            self.at += 1;
            if !self.digit() {
                self.report(Token::Illegal, Some(UNTERMINATED_FRACTION));
                return;
            }
            while self.digit() {
                self.at += 1;
            }
        }
        if matches!(self.peek_char(), Some(0x65 | 0x45)) {
            self.at += 1;
            if matches!(self.peek_char(), Some(0x2B | 0x2D)) {
                self.at += 1;
            }
            if !self.digit() {
                self.report(Token::Illegal, Some(EXPONENT_MISSING));
                return;
            }
            while self.digit() {
                self.at += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// V8's messages for every error class, written by
    /// `tests/node/json-errors.js` with Node 24.13.1.
    #[test]
    fn v8s_messages_for_every_error_class() {
        let golden = include_str!("../../tests/json-errors.jsonl");
        let mut count = 0;
        let mut wrong = Vec::new();
        for line in golden.lines() {
            let case: Value = serde_json::from_str(line).unwrap();
            let text = case["text"].as_str().unwrap();
            let expected = case["error"].as_str();
            let got = parse(text).err();
            if got.as_deref() != expected {
                wrong.push(format!("{text:?}\n  got  {got:?}\n  want {expected:?}"));
            }
            count += 1;
        }
        assert!(count > 100, "{count} cases");
        assert!(
            wrong.is_empty(),
            "{} of {count} differ:\n{}",
            wrong.len(),
            wrong[..wrong.len().min(25)].join("\n")
        );
    }
}
