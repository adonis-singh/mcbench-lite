// Gson as the fixes use it on text components: JsonParser.parseString (a
// lenient JsonReader, ported from Gson 2.11's), GsonHelper.toStableString
// (a JsonWriter with sorted keys), and DFU's JsonOps conversions to and from
// NBT. Text is read as UTF-16 units, as a Java string is.

use super::ops::Nbt;
use crate::level::refuse;
use crate::nbt::{Compound, Tag};

// A JsonElement. A number keeps its text (LazilyParsedNumber, or the boxed
// number's toString).
#[derive(Clone, Debug, PartialEq)]
pub enum Je {
    Null,
    Bool(bool),
    Num(String),
    Str(String),
    Arr(Vec<Je>),
    // JsonObject: members in insertion order; a repeated name replaces the
    // value in place.
    Obj(Vec<(String, Je)>),
}

impl Je {
    pub fn get(&self, key: &str) -> Option<&Je> {
        match self {
            Je::Obj(m) => m.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn is_primitive(&self) -> bool {
        matches!(self, Je::Bool(_) | Je::Num(_) | Je::Str(_))
    }

    // JsonPrimitive.getAsString.
    pub fn as_string(&self) -> String {
        match self {
            Je::Bool(b) => b.to_string(),
            Je::Num(n) | Je::Str(n) => n.clone(),
            _ => refuse("JsonElement.getAsString of a non-primitive (the data fixer)"),
        }
    }
}

// A failure of the parse (JsonParseException and the other exceptions the
// callers catch). Eof is the reader's EOFException, which before the first
// token makes the document JsonNull.
#[derive(Debug, PartialEq)]
pub enum Malformed {
    Bad,
    Eof,
}

use Malformed::{Bad, Eof};

type R<T> = Result<T, Malformed>;

const PEEKED_NONE: u8 = 0;
const BEGIN_OBJECT: u8 = 1;
const END_OBJECT: u8 = 2;
const BEGIN_ARRAY: u8 = 3;
const END_ARRAY: u8 = 4;
const TRUE: u8 = 5;
const FALSE: u8 = 6;
const NULL: u8 = 7;
const SINGLE_QUOTED: u8 = 8;
const DOUBLE_QUOTED: u8 = 9;
const UNQUOTED: u8 = 10;
const SINGLE_QUOTED_NAME: u8 = 12;
const DOUBLE_QUOTED_NAME: u8 = 13;
const UNQUOTED_NAME: u8 = 14;
const LONG: u8 = 15;
const NUMBER: u8 = 16;
const EOF: u8 = 17;

// JsonScope.
const EMPTY_ARRAY: u8 = 1;
const NONEMPTY_ARRAY: u8 = 2;
const EMPTY_OBJECT: u8 = 3;
const DANGLING_NAME: u8 = 4;
const NONEMPTY_OBJECT: u8 = 5;
const EMPTY_DOCUMENT: u8 = 6;
const NONEMPTY_DOCUMENT: u8 = 7;

struct Reader {
    buf: Vec<u16>,
    pos: usize,
    peeked: u8,
    peeked_long: i64,
    peeked_number_length: usize,
    stack: Vec<u8>,
    // Strictness.STRICT (StrictJsonParser) rather than LENIENT.
    strict: bool,
}

fn is_literal(c: u16) -> bool {
    !matches!(c, 0x23 | 0x2f | 0x3b | 0x3d | 0x5c | 0x09 | 0x0a | 0x0c | 0x0d | 0x20 | 0x2c | 0x3a | 0x5b | 0x5d | 0x7b | 0x7d)
}

impl Reader {
    fn new(text: &str, strict: bool) -> Reader {
        let mut buf: Vec<u16> = text.encode_utf16().collect();
        // fillBuffer skips a byte order mark that starts the document.
        if buf.first() == Some(&0xfeff) {
            buf.remove(0);
        }
        Reader { buf, pos: 0, peeked: PEEKED_NONE, peeked_long: 0, peeked_number_length: 0, stack: vec![EMPTY_DOCUMENT], strict }
    }

    // checkLenient.
    fn lenient(&self) -> R<()> {
        if self.strict {
            Err(Bad)
        } else {
            Ok(())
        }
    }

    // isLiteral, which checks leniency for the characters only lenient JSON
    // takes as literals' ends.
    fn lit(&self, c: u16) -> R<bool> {
        if matches!(c, 0x23 | 0x2f | 0x3b | 0x3d | 0x5c) {
            self.lenient()?;
        }
        Ok(is_literal(c))
    }

    fn at(&self, i: usize) -> Option<u16> {
        self.buf.get(i).copied()
    }

    fn top(&mut self) -> &mut u8 {
        self.stack.last_mut().unwrap()
    }

    fn peek(&mut self) -> R<u8> {
        if self.peeked == PEEKED_NONE {
            self.do_peek()?;
        }
        Ok(self.peeked)
    }

    fn do_peek(&mut self) -> R<u8> {
        let peek_stack = *self.stack.last().unwrap();
        if peek_stack == EMPTY_ARRAY {
            *self.top() = NONEMPTY_ARRAY;
        } else if peek_stack == NONEMPTY_ARRAY {
            match self.next_non_whitespace(true)? {
                Some(0x2c) => {}
                Some(0x3b) => self.lenient()?,
                Some(0x5d) => return self.set(END_ARRAY),
                _ => return Err(Bad),
            }
        } else if peek_stack == EMPTY_OBJECT || peek_stack == NONEMPTY_OBJECT {
            *self.top() = DANGLING_NAME;
            if peek_stack == NONEMPTY_OBJECT {
                match self.next_non_whitespace(true)? {
                    Some(0x2c) => {}
                    Some(0x3b) => self.lenient()?,
                    Some(0x7d) => return self.set(END_OBJECT),
                    _ => return Err(Bad),
                }
            }
            return match self.next_non_whitespace(true)? {
                Some(0x22) => self.set(DOUBLE_QUOTED_NAME),
                Some(0x27) => {
                    self.lenient()?;
                    self.set(SINGLE_QUOTED_NAME)
                }
                Some(0x7d) => {
                    if peek_stack != NONEMPTY_OBJECT {
                        self.set(END_OBJECT)
                    } else {
                        Err(Bad)
                    }
                }
                Some(c) => {
                    self.lenient()?;
                    self.pos -= 1;
                    if self.lit(c)? {
                        self.set(UNQUOTED_NAME)
                    } else {
                        Err(Bad)
                    }
                }
                None => Err(Bad),
            };
        } else if peek_stack == DANGLING_NAME {
            *self.top() = NONEMPTY_OBJECT;
            match self.next_non_whitespace(true)? {
                Some(0x3a) => {}
                Some(0x3d) => {
                    self.lenient()?;
                    if self.at(self.pos) == Some(0x3e) {
                        self.pos += 1;
                    }
                }
                _ => return Err(Bad),
            }
        } else if peek_stack == EMPTY_DOCUMENT {
            if !self.strict {
                self.consume_non_execute_prefix()?;
            }
            *self.top() = NONEMPTY_DOCUMENT;
        } else if peek_stack == NONEMPTY_DOCUMENT {
            match self.next_non_whitespace(false)? {
                None => return self.set(EOF),
                Some(_) => {
                    self.lenient()?;
                    self.pos -= 1
                }
            }
        }
        let c = self.next_non_whitespace(true)?;
        match c {
            Some(0x22) => self.set(DOUBLE_QUOTED),
            Some(0x27) => {
                self.lenient()?;
                self.set(SINGLE_QUOTED)
            }
            Some(0x5b) => self.set(BEGIN_ARRAY),
            Some(0x5d) if peek_stack == EMPTY_ARRAY => self.set(END_ARRAY),
            Some(0x5d) | Some(0x2c) | Some(0x3b) => {
                if peek_stack != EMPTY_ARRAY && peek_stack != NONEMPTY_ARRAY {
                    return Err(Bad);
                }
                self.lenient()?;
                self.pos -= 1;
                self.set(NULL)
            }
            Some(0x7b) => self.set(BEGIN_OBJECT),
            None => Err(Bad),
            Some(_) => {
                self.pos -= 1;
                let r = self.peek_keyword()?;
                if r != PEEKED_NONE {
                    return Ok(r);
                }
                let r = self.peek_number()?;
                if r != PEEKED_NONE {
                    return Ok(r);
                }
                match self.at(self.pos) {
                    Some(c) if self.lit(c)? => {
                        self.lenient()?;
                        self.set(UNQUOTED)
                    }
                    _ => Err(Bad),
                }
            }
        }
    }

    fn set(&mut self, p: u8) -> R<u8> {
        self.peeked = p;
        Ok(p)
    }

    fn peek_keyword(&mut self) -> R<u8> {
        let c = match self.at(self.pos) {
            Some(c) => c,
            None => return Ok(PEEKED_NONE),
        };
        let (keyword, upper, peeking) = match c {
            0x74 | 0x54 => ("true", "TRUE", TRUE),
            0x66 | 0x46 => ("false", "FALSE", FALSE),
            0x6e | 0x4e => ("null", "NULL", NULL),
            _ => return Ok(PEEKED_NONE),
        };
        let (k, u): (Vec<u16>, Vec<u16>) = (keyword.encode_utf16().collect(), upper.encode_utf16().collect());
        for i in 0..k.len() {
            match self.at(self.pos + i) {
                Some(c) if c == k[i] || (!self.strict && c == u[i]) => {}
                _ => return Ok(PEEKED_NONE),
            }
        }
        if let Some(c) = self.at(self.pos + k.len()) {
            if self.lit(c)? {
                return Ok(PEEKED_NONE);
            }
        }
        self.pos += k.len();
        self.peeked = peeking;
        Ok(peeking)
    }

    fn peek_number(&mut self) -> R<u8> {
        const NONE: u8 = 0;
        const SIGN: u8 = 1;
        const DIGIT: u8 = 2;
        const DECIMAL: u8 = 3;
        const FRACTION_DIGIT: u8 = 4;
        const EXP_E: u8 = 5;
        const EXP_SIGN: u8 = 6;
        const EXP_DIGIT: u8 = 7;
        let mut value: i64 = 0;
        let mut negative = false;
        let mut fits_in_long = true;
        let mut last = NONE;
        let mut i = 0;
        loop {
            // JsonReader's buffer holds 1024 chars: a longer literal is no number.
            if i == 1024 {
                return Ok(PEEKED_NONE);
            }
            let Some(c) = self.at(self.pos + i) else { break };
            match c {
                0x2b => {
                    if last != EXP_E {
                        return Ok(PEEKED_NONE);
                    }
                    last = EXP_SIGN;
                }
                0x2d => {
                    if last == NONE {
                        negative = true;
                        last = SIGN;
                    } else if last == EXP_E {
                        last = EXP_SIGN;
                    } else {
                        return Ok(PEEKED_NONE);
                    }
                }
                0x2e => {
                    if last != DIGIT {
                        return Ok(PEEKED_NONE);
                    }
                    last = DECIMAL;
                }
                0x45 | 0x65 => {
                    if last != DIGIT && last != FRACTION_DIGIT {
                        return Ok(PEEKED_NONE);
                    }
                    last = EXP_E;
                }
                _ => {
                    if !(0x30..=0x39).contains(&c) {
                        if self.lit(c)? {
                            return Ok(PEEKED_NONE);
                        }
                        break;
                    }
                    let d = (c - 0x30) as i64;
                    if last == SIGN || last == NONE {
                        value = -d;
                        last = DIGIT;
                    } else if last == DIGIT {
                        if value == 0 {
                            return Ok(PEEKED_NONE);
                        }
                        let new_value = value.wrapping_mul(10).wrapping_sub(d);
                        fits_in_long &= value > -922337203685477580 || (value == -922337203685477580 && new_value < value);
                        value = new_value;
                    } else if last == DECIMAL {
                        last = FRACTION_DIGIT;
                    } else if last == EXP_E || last == EXP_SIGN {
                        last = EXP_DIGIT;
                    }
                }
            }
            i += 1;
        }
        if last == DIGIT && fits_in_long && (value != i64::MIN || negative) && (value != 0 || !negative) {
            self.peeked_long = if negative { value } else { -value };
            self.pos += i;
            self.peeked = LONG;
            Ok(LONG)
        } else if last == DIGIT || last == FRACTION_DIGIT || last == EXP_DIGIT {
            self.peeked_number_length = i;
            self.peeked = NUMBER;
            Ok(NUMBER)
        } else {
            Ok(PEEKED_NONE)
        }
    }

    // nextNonWhitespace: None at the end (throwOnEof makes that an error at
    // the call sites that need a character).
    fn next_non_whitespace(&mut self, throw_on_eof: bool) -> R<Option<u16>> {
        loop {
            let Some(c) = self.at(self.pos) else {
                return if throw_on_eof { Err(Eof) } else { Ok(None) };
            };
            self.pos += 1;
            match c {
                0x0a | 0x20 | 0x0d | 0x09 => {}
                0x2f => {
                    if self.at(self.pos).is_some() {
                        self.lenient()?;
                    }
                    match self.at(self.pos) {
                        None => return Ok(Some(c)),
                        Some(0x2a) => {
                            self.pos += 1;
                            // skipTo("*/")
                            loop {
                                if self.pos + 2 > self.buf.len() {
                                    return Err(Bad);
                                }
                                if self.buf[self.pos] == 0x2a && self.buf[self.pos + 1] == 0x2f {
                                    break;
                                }
                                self.pos += 1;
                            }
                            self.pos += 2;
                        }
                        Some(0x2f) => {
                            self.pos += 1;
                            self.skip_to_end_of_line();
                        }
                        Some(_) => return Ok(Some(c)),
                    }
                }
                0x23 => {
                    self.lenient()?;
                    self.skip_to_end_of_line()
                }
                _ => return Ok(Some(c)),
            }
        }
    }

    fn skip_to_end_of_line(&mut self) {
        while let Some(c) = self.at(self.pos) {
            self.pos += 1;
            if c == 0x0a || c == 0x0d {
                break;
            }
        }
    }

    fn consume_non_execute_prefix(&mut self) -> R<()> {
        self.next_non_whitespace(true)?;
        self.pos -= 1;
        let prefix: [u16; 5] = [0x29, 0x5d, 0x7d, 0x27, 0x0a];
        if self.pos + 5 <= self.buf.len() && self.buf[self.pos..self.pos + 5] == prefix {
            self.pos += 5;
        }
        Ok(())
    }

    fn next_quoted_value(&mut self, quote: u16) -> R<Vec<u16>> {
        let mut out = Vec::new();
        loop {
            let Some(c) = self.at(self.pos) else { return Err(Bad) };
            self.pos += 1;
            if self.strict && c < 32 {
                return Err(Bad);
            }
            if c == quote {
                return Ok(out);
            }
            if c == 0x5c {
                out.push(self.read_escape_character()?);
            } else {
                out.push(c);
            }
        }
    }

    fn read_escape_character(&mut self) -> R<u16> {
        let Some(escaped) = self.at(self.pos) else { return Err(Bad) };
        self.pos += 1;
        Ok(match escaped {
            0x0a | 0x27 if self.strict => return Err(Bad),
            0x0a | 0x27 | 0x22 | 0x2f | 0x5c => escaped,
            0x62 => 0x08,
            0x66 => 0x0c,
            0x6e => 0x0a,
            0x72 => 0x0d,
            0x74 => 0x09,
            0x75 => {
                if self.pos + 4 > self.buf.len() {
                    return Err(Bad);
                }
                let mut result: u32 = 0;
                for i in 0..4 {
                    let c = self.buf[self.pos + i];
                    result <<= 4;
                    result += match c {
                        0x30..=0x39 => (c - 0x30) as u32,
                        0x61..=0x66 => (c - 0x61 + 10) as u32,
                        0x41..=0x46 => (c - 0x41 + 10) as u32,
                        _ => return Err(Bad),
                    };
                }
                self.pos += 4;
                result as u16
            }
            _ => return Err(Bad),
        })
    }

    fn next_unquoted_value(&mut self) -> Vec<u16> {
        let start = self.pos;
        while let Some(c) = self.at(self.pos) {
            if !is_literal(c) {
                break;
            }
            self.pos += 1;
        }
        self.buf[start..self.pos].to_vec()
    }

    fn next_string(&mut self) -> R<Vec<u16>> {
        let p = self.peek()?;
        let result = match p {
            UNQUOTED => self.next_unquoted_value(),
            SINGLE_QUOTED => self.next_quoted_value(0x27)?,
            DOUBLE_QUOTED => self.next_quoted_value(0x22)?,
            LONG => self.peeked_long.to_string().encode_utf16().collect(),
            NUMBER => {
                let s = self.buf[self.pos..self.pos + self.peeked_number_length].to_vec();
                self.pos += self.peeked_number_length;
                s
            }
            _ => return Err(Bad),
        };
        self.peeked = PEEKED_NONE;
        Ok(result)
    }

    fn next_name(&mut self) -> R<Vec<u16>> {
        let p = self.peek()?;
        let result = match p {
            UNQUOTED_NAME => self.next_unquoted_value(),
            SINGLE_QUOTED_NAME => self.next_quoted_value(0x27)?,
            DOUBLE_QUOTED_NAME => self.next_quoted_value(0x22)?,
            _ => return Err(Bad),
        };
        self.peeked = PEEKED_NONE;
        Ok(result)
    }

    // TypeAdapters.JSON_ELEMENT.read.
    fn read(&mut self, depth: usize) -> R<Je> {
        if depth > 2000 {
            // Gson would overflow its stack (a JsonParseException).
            return Err(Bad);
        }
        match self.peek()? {
            BEGIN_ARRAY => {
                self.stack.push(EMPTY_ARRAY);
                self.peeked = PEEKED_NONE;
                let mut items = Vec::new();
                loop {
                    let p = self.peek()?;
                    if p == END_ARRAY || p == END_OBJECT || p == EOF {
                        break;
                    }
                    items.push(self.read(depth + 1)?);
                }
                if self.peek()? != END_ARRAY {
                    return Err(Bad);
                }
                self.stack.pop();
                self.peeked = PEEKED_NONE;
                Ok(Je::Arr(items))
            }
            BEGIN_OBJECT => {
                self.stack.push(EMPTY_OBJECT);
                self.peeked = PEEKED_NONE;
                let mut members: Vec<(String, Je)> = Vec::new();
                loop {
                    let p = self.peek()?;
                    if p == END_ARRAY || p == END_OBJECT || p == EOF {
                        break;
                    }
                    let name = utf16(self.next_name()?);
                    let value = self.read(depth + 1)?;
                    match members.iter_mut().find(|(k, _)| *k == name) {
                        Some(m) => m.1 = value,
                        None => members.push((name, value)),
                    }
                }
                if self.peek()? != END_OBJECT {
                    return Err(Bad);
                }
                self.stack.pop();
                self.peeked = PEEKED_NONE;
                Ok(Je::Obj(members))
            }
            NUMBER | LONG => Ok(Je::Num(utf16(self.next_string()?))),
            SINGLE_QUOTED | DOUBLE_QUOTED | UNQUOTED => Ok(Je::Str(utf16(self.next_string()?))),
            TRUE => {
                self.peeked = PEEKED_NONE;
                Ok(Je::Bool(true))
            }
            FALSE => {
                self.peeked = PEEKED_NONE;
                Ok(Je::Bool(false))
            }
            NULL => {
                self.peeked = PEEKED_NONE;
                Ok(Je::Null)
            }
            _ => Err(Bad),
        }
    }
}

// A Java string of UTF-16 units: one this port cannot hold (a lone
// surrogate) is refused.
fn utf16(units: Vec<u16>) -> String {
    match String::from_utf16(&units) {
        Ok(s) => s,
        Err(_) => refuse("a lone surrogate in JSON text (the data fixer)"),
    }
}

// JsonParser.parseString: a lenient read of one value (an empty document is
// JsonNull), and nothing after it but whitespace and comments.
pub fn parse_lenient(text: &str) -> R<Je> {
    let mut r = Reader::new(text, false);
    // Streams.parse: an end of input before the first token is JsonNull.
    let element = match r.peek() {
        Err(Eof) => return Ok(Je::Null),
        Err(e) => return Err(e),
        Ok(_) => r.read(0).map_err(|_| Bad)?,
    };
    if element != Je::Null && r.peek().map_err(|_| Bad)? != EOF {
        return Err(Bad);
    }
    Ok(element)
}

// StrictJsonParser.parse succeeds: a strict read of one value (an empty
// document is JsonNull), and nothing after it.
pub fn is_strict_json(text: &str) -> bool {
    let mut r = Reader::new(text, true);
    let element = match r.peek() {
        Err(Eof) => return true,
        Err(_) => return false,
        Ok(_) => match r.read(0) {
            Ok(e) => e,
            Err(_) => return false,
        },
    };
    element == Je::Null || r.peek() == Ok(EOF)
}

// JsonWriter.string (not HTML-safe).
pub fn write_string(out: &mut String, text: &str) {
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            c => out.push(c),
        }
    }
    out.push('"');
}

// GsonHelper.toStableString: compact, object keys in natural order.
pub fn to_stable_string(e: &Je) -> String {
    let mut out = String::new();
    write_stable(&mut out, e);
    out
}

fn write_stable(out: &mut String, e: &Je) {
    match e {
        Je::Null => out.push_str("null"),
        Je::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Je::Num(n) => {
            if n == "NaN" || n == "Infinity" || n == "-Infinity" {
                refuse("a non-finite number written as JSON (JsonWriter throws)");
            }
            out.push_str(n)
        }
        Je::Str(s) => write_string(out, s),
        Je::Arr(items) => {
            out.push('[');
            for (i, x) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_stable(out, x);
            }
            out.push(']');
        }
        Je::Obj(members) => {
            let mut sorted: Vec<&(String, Je)> = members.iter().collect();
            sorted.sort_by(|a, b| a.0.encode_utf16().cmp(b.0.encode_utf16()));
            out.push('{');
            for (i, (k, v)) in sorted.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_string(out, k);
                out.push(':');
                write_stable(out, v);
            }
            out.push('}');
        }
    }
}

// A decimal number's text as BigDecimal reads it: (negative, digits, scale).
fn big_decimal(text: &str) -> Option<(bool, Vec<u8>, i64)> {
    let bytes = text.as_bytes();
    let mut i = 0;
    let negative = match bytes.first() {
        Some(b'-') => {
            i += 1;
            true
        }
        Some(b'+') => {
            i += 1;
            false
        }
        _ => false,
    };
    let mut digits = Vec::new();
    let mut fraction = 0i64;
    let mut seen_point = false;
    let mut any = false;
    while i < bytes.len() {
        match bytes[i] {
            b'0'..=b'9' => {
                digits.push(bytes[i] - b'0');
                any = true;
                if seen_point {
                    fraction += 1;
                }
            }
            b'.' if !seen_point => seen_point = true,
            b'e' | b'E' => break,
            _ => return None,
        }
        i += 1;
    }
    if !any {
        return None;
    }
    let mut exponent = 0i64;
    if i < bytes.len() {
        let e: &str = &text[i + 1..];
        let (sign, body) = match e.as_bytes().first() {
            Some(b'-') => (-1i64, &e[1..]),
            Some(b'+') => (1, &e[1..]),
            _ => (1, e),
        };
        if body.is_empty() || !body.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let body = body.trim_start_matches('0');
        if body.len() > 10 {
            return None;
        }
        exponent = sign * body.parse::<i64>().unwrap_or(0);
        if exponent < i32::MIN as i64 || exponent > i32::MAX as i64 {
            return None;
        }
    }
    Some((negative, digits, fraction - exponent))
}

// JsonOps.convertTo(NbtOps): a number is the narrowest integer tag it fits
// exactly (BigDecimal.longValueExact), else a float when that is exact, else
// a double; a null is EndTag (which a list or a compound throws on). Err
// where NumberLimits, BigDecimal or the tags throw.
pub fn json_to_nbt(e: &Je) -> R<Nbt> {
    Ok(match e {
        Je::Null => None,
        Je::Bool(b) => Some(Tag::Byte(*b as i8)),
        Je::Str(s) => Some(Tag::String(s.clone())),
        Je::Num(n) => Some(number_to_nbt(n)?),
        Je::Arr(items) => {
            let mut out = Vec::new();
            for x in items {
                match json_to_nbt(x)? {
                    Some(t) => out.push(t),
                    // EndTag in a ListTag throws.
                    None => return Err(Bad),
                }
            }
            Some(Tag::List(out))
        }
        Je::Obj(members) => {
            let mut c = Compound::new();
            for (k, v) in members {
                match json_to_nbt(v)? {
                    Some(t) => c.put(k, t),
                    // EndTag in a CompoundTag throws.
                    None => return Err(Bad),
                }
            }
            Some(Tag::Compound(c))
        }
    })
}

fn number_to_nbt(text: &str) -> R<Tag> {
    // NumberLimits.parseBigDecimal.
    if text.len() > 10000 {
        return Err(Bad);
    }
    let Some((negative, digits, scale)) = big_decimal(text) else { return Err(Bad) };
    if scale.abs() >= 10000 {
        return Err(Bad);
    }
    // longValueExact: no nonzero fraction, and within a long.
    let exact: Option<i64> = {
        let mut digits = digits.clone();
        let mut ok = true;
        if scale > 0 {
            let cut = digits.len().saturating_sub(scale as usize);
            if digits[cut..].iter().any(|d| *d != 0) {
                ok = false;
            }
            digits.truncate(cut);
        } else {
            for _ in 0..(-scale) {
                digits.push(0);
                if digits.len() > 40 {
                    break;
                }
            }
        }
        if !ok {
            None
        } else {
            let mut v: i128 = 0;
            let mut fits = true;
            for d in digits.iter().skip_while(|d| **d == 0) {
                v = v * 10 + *d as i128;
                if v > i64::MAX as i128 + 1 {
                    fits = false;
                    break;
                }
            }
            let v = if negative { -v } else { v };
            (fits && v >= i64::MIN as i128 && v <= i64::MAX as i128).then_some(v as i64)
        }
    };
    Ok(match exact {
        Some(l) => {
            if l as i8 as i64 == l {
                Tag::Byte(l as i8)
            } else if l as i16 as i64 == l {
                Tag::Short(l as i16)
            } else if l as i32 as i64 == l {
                Tag::Int(l as i32)
            } else {
                Tag::Long(l)
            }
        }
        None => {
            // BigDecimal.doubleValue: the correctly rounded double.
            let d: f64 = text.parse().unwrap_or_else(|_| refuse("a JSON number Rust does not read"));
            if (d as f32) as f64 == d {
                Tag::Float(d as f32)
            } else {
                Tag::Double(d)
            }
        }
    })
}

// NbtOps.convertTo(JsonOps): numbers as the boxed number's toString.
pub fn nbt_to_json(t: &Nbt) -> Je {
    match t {
        None => Je::Null,
        Some(t) => match t {
            Tag::Byte(v) => Je::Num(v.to_string()),
            Tag::Short(v) => Je::Num(v.to_string()),
            Tag::Int(v) => Je::Num(v.to_string()),
            Tag::Long(v) => Je::Num(v.to_string()),
            Tag::Float(v) => Je::Num(crate::jfmt::float_to_string(*v)),
            Tag::Double(v) => Je::Num(crate::jfmt::double_to_string(*v)),
            Tag::String(s) => Je::Str(s.clone()),
            Tag::ByteArray(a) => Je::Arr(a.iter().map(|v| Je::Num(v.to_string())).collect()),
            Tag::IntArray(a) => Je::Arr(a.iter().map(|v| Je::Num(v.to_string())).collect()),
            Tag::LongArray(a) => Je::Arr(a.iter().map(|v| Je::Num(v.to_string())).collect()),
            Tag::List(items) => Je::Arr(items.iter().map(|x| nbt_to_json(&Some(x.clone()))).collect()),
            Tag::Compound(c) => Je::Obj(c.0.iter().map(|(k, v)| (k.clone(), nbt_to_json(&Some(v.clone())))).collect()),
        },
    }
}
