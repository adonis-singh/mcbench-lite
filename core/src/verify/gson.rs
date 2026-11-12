// JSON as Gson holds and writes it, for the verifier's request and report:
// JsonElement trees whose objects keep their members' order and whose
// numbers keep the text they were read as (LazilyParsedNumber), and a
// JsonWriter that writes compact JSON the way McbenchExperiment's report
// does (Gson's escaping with and without its HTML-safe mode, null members
// left out of an element that is written).

#[derive(Clone, Debug, PartialEq)]
pub enum J {
    Null,
    Bool(bool),
    // A number as it is written: the source text of a parsed one, Java's
    // toString of one the experiment makes.
    Num(String),
    Str(String),
    Arr(Vec<J>),
    Obj(Vec<(String, J)>),
}

impl J {
    pub fn int(v: i64) -> J {
        J::Num(v.to_string())
    }

    pub fn double(v: f64) -> J {
        J::Num(crate::jfmt::double_to_string(v))
    }

    pub fn string(v: impl Into<String>) -> J {
        J::Str(v.into())
    }

    pub fn obj() -> J {
        J::Obj(Vec::new())
    }

    // JsonObject.add: replaces the value of a member it has, in place.
    pub fn put(&mut self, key: &str, value: J) {
        let J::Obj(members) = self else { panic!("not an object") };
        match members.iter_mut().find(|(k, _)| k == key) {
            Some(member) => member.1 = value,
            None => members.push((key.to_string(), value)),
        }
    }

    pub fn push(&mut self, value: J) {
        let J::Arr(items) = self else { panic!("not an array") };
        items.push(value);
    }

    pub fn get(&self, key: &str) -> Option<&J> {
        match self {
            J::Obj(members) => members.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn has(&self, key: &str) -> bool {
        self.get(key).is_some()
    }

    pub fn req(&self, key: &str) -> &J {
        self.get(key).unwrap_or_else(|| panic!("the request has no {key}"))
    }

    pub fn arr(&self) -> &[J] {
        match self {
            J::Arr(items) => items,
            other => panic!("not an array: {other:?}"),
        }
    }

    pub fn str(&self) -> &str {
        match self {
            J::Str(s) => s,
            other => panic!("not a string: {other:?}"),
        }
    }

    pub fn bool(&self) -> bool {
        match self {
            J::Bool(b) => *b,
            other => panic!("not a boolean: {other:?}"),
        }
    }

    pub fn is_empty_arr(&self) -> bool {
        matches!(self, J::Arr(items) if items.is_empty())
    }

    // JsonElement.getAsDouble.
    pub fn double_value(&self) -> f64 {
        match self {
            J::Num(text) => text.parse().unwrap_or_else(|_| panic!("bad number {text}")),
            other => panic!("not a number: {other:?}"),
        }
    }

    // JsonElement.getAsLong (a number written with a fraction is cut).
    pub fn long_value(&self) -> i64 {
        match self {
            J::Num(text) => text.parse::<i64>().unwrap_or_else(|_| text.parse::<f64>().unwrap_or_else(|_| panic!("bad number {text}")) as i64),
            other => panic!("not a number: {other:?}"),
        }
    }

    // JsonElement.getAsInt (LazilyParsedNumber.intValue: Integer.parseInt,
    // then BigDecimal.intValue, which cuts the fraction and wraps).
    pub fn int_value(&self) -> i32 {
        self.long_value() as i32
    }

    // Gson.toJson(element): compact, HTML-safe, null members left out.
    pub fn to_json(&self) -> String {
        let mut out = Vec::new();
        write_element(&mut out, self, true);
        String::from_utf8(out).unwrap()
    }
}

pub fn parse(text: &str) -> J {
    let mut reader = Reader { bytes: text.as_bytes(), at: 0 };
    let value = reader.value();
    reader.space();
    assert_eq!(reader.at, reader.bytes.len(), "trailing JSON");
    value
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn space(&mut self) {
        while self.at < self.bytes.len() && matches!(self.bytes[self.at], b' ' | b'\n' | b'\r' | b'\t') {
            self.at += 1;
        }
    }

    fn value(&mut self) -> J {
        self.space();
        match self.bytes[self.at] {
            b'{' => {
                self.at += 1;
                let mut object = J::obj();
                self.space();
                if self.bytes[self.at] == b'}' {
                    self.at += 1;
                    return object;
                }
                loop {
                    self.space();
                    let key = self.string();
                    self.space();
                    assert_eq!(self.bytes[self.at], b':', "expected : at {}", self.at);
                    self.at += 1;
                    let value = self.value();
                    object.put(&key, value);
                    self.space();
                    self.at += 1;
                    match self.bytes[self.at - 1] {
                        b',' => continue,
                        b'}' => return object,
                        other => panic!("bad JSON object at {}: {}", self.at, other as char),
                    }
                }
            }
            b'[' => {
                self.at += 1;
                let mut items = Vec::new();
                self.space();
                if self.bytes[self.at] == b']' {
                    self.at += 1;
                    return J::Arr(items);
                }
                loop {
                    items.push(self.value());
                    self.space();
                    self.at += 1;
                    match self.bytes[self.at - 1] {
                        b',' => continue,
                        b']' => return J::Arr(items),
                        other => panic!("bad JSON array at {}: {}", self.at, other as char),
                    }
                }
            }
            b'"' => J::Str(self.string()),
            b't' => self.word("true", J::Bool(true)),
            b'f' => self.word("false", J::Bool(false)),
            b'n' => self.word("null", J::Null),
            _ => {
                let start = self.at;
                while self.at < self.bytes.len() && matches!(self.bytes[self.at], b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9') {
                    self.at += 1;
                }
                assert!(self.at > start, "bad JSON at {start}");
                J::Num(std::str::from_utf8(&self.bytes[start..self.at]).unwrap().to_string())
            }
        }
    }

    fn word(&mut self, word: &str, value: J) -> J {
        assert!(self.bytes[self.at..].starts_with(word.as_bytes()), "bad JSON at {}", self.at);
        self.at += word.len();
        value
    }

    fn hex4(&mut self) -> u32 {
        let code = u32::from_str_radix(std::str::from_utf8(&self.bytes[self.at..self.at + 4]).unwrap(), 16).unwrap();
        self.at += 4;
        code
    }

    fn string(&mut self) -> String {
        assert_eq!(self.bytes[self.at], b'"', "expected a string at {}", self.at);
        self.at += 1;
        let mut out = Vec::new();
        loop {
            let b = self.bytes[self.at];
            self.at += 1;
            match b {
                b'"' => return String::from_utf8(out).unwrap(),
                b'\\' => {
                    let e = self.bytes[self.at];
                    self.at += 1;
                    match e {
                        b'"' => out.push(b'"'),
                        b'\\' => out.push(b'\\'),
                        b'/' => out.push(b'/'),
                        b'b' => out.push(8),
                        b'f' => out.push(12),
                        b'n' => out.push(b'\n'),
                        b'r' => out.push(b'\r'),
                        b't' => out.push(b'\t'),
                        b'u' => {
                            let mut code = self.hex4();
                            if (0xD800..0xDC00).contains(&code) {
                                assert!(self.bytes[self.at] == b'\\' && self.bytes[self.at + 1] == b'u', "lone surrogate");
                                self.at += 2;
                                let low = self.hex4();
                                assert!((0xDC00..0xE000).contains(&low), "lone surrogate");
                                code = 0x10000 + ((code - 0xD800) << 10) + (low - 0xDC00);
                            }
                            let mut buf = [0; 4];
                            out.extend_from_slice(char::from_u32(code).expect("lone surrogate").encode_utf8(&mut buf).as_bytes());
                        }
                        other => panic!("bad JSON escape {}", other as char),
                    }
                }
                _ => out.push(b),
            }
        }
    }
}

// JsonWriter.string: quotes and escapes; `html` is Gson's HTML-safe mode
// (< > & = ' as \u escapes), which Gson.toJson turns on for what it writes
// and a JsonWriter made by hand does not have.
pub fn write_string(out: &mut Vec<u8>, text: &str, html: bool) {
    out.push(b'"');
    let bytes = text.as_bytes();
    // Runs of bytes that need no escape are copied whole. A byte that may
    // need one: control characters, quotes, backslashes, the HTML-safe set
    // when asked, and 0xE2, which starts U+2028 and U+2029 (any other
    // character starting with it is copied as it is).
    let mut run = 0;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        let special = b < 0x20 || b == b'"' || b == b'\\' || b == 0xE2 || (html && matches!(b, b'<' | b'>' | b'&' | b'=' | b'\''));
        if !special {
            i += 1;
            continue;
        }
        let escape: &[u8] = match b {
            b'"' => b"\\\"",
            b'\\' => b"\\\\",
            b'\t' => b"\\t",
            0x08 => b"\\b",
            b'\n' => b"\\n",
            b'\r' => b"\\r",
            0x0C => b"\\f",
            0xE2 => match bytes.get(i..i + 3) {
                Some([0xE2, 0x80, 0xA8]) => b"\\u2028",
                Some([0xE2, 0x80, 0xA9]) => b"\\u2029",
                _ => {
                    i += 1;
                    continue;
                }
            },
            _ => {
                out.extend_from_slice(&bytes[run..i]);
                out.extend_from_slice(b"\\u00");
                out.push(b"0123456789abcdef"[(b >> 4) as usize]);
                out.push(b"0123456789abcdef"[(b & 15) as usize]);
                i += 1;
                run = i;
                continue;
            }
        };
        out.extend_from_slice(&bytes[run..i]);
        out.extend_from_slice(escape);
        i += if b == 0xE2 { 3 } else { 1 };
        run = i;
    }
    out.extend_from_slice(&bytes[run..]);
    out.push(b'"');
}

// Gson.toJson(JsonElement, JsonWriter) without serializeNulls: an object
// member that is null is not written.
pub fn write_element(out: &mut Vec<u8>, value: &J, html: bool) {
    match value {
        J::Null => out.extend_from_slice(b"null"),
        J::Bool(b) => out.extend_from_slice(if *b { b"true" } else { b"false" }),
        J::Num(text) => out.extend_from_slice(text.as_bytes()),
        J::Str(text) => write_string(out, text, html),
        J::Arr(items) => {
            out.push(b'[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(b',');
                }
                write_element(out, item, html);
            }
            out.push(b']');
        }
        J::Obj(members) => {
            out.push(b'{');
            let mut first = true;
            for (key, member) in members {
                if *member == J::Null {
                    continue;
                }
                if !first {
                    out.push(b',');
                }
                first = false;
                write_string(out, key, html);
                out.push(b':');
                write_element(out, member, html);
            }
            out.push(b'}');
        }
    }
}

// The report's JsonWriter (compact), which tracks where commas go.
pub struct Writer {
    pub out: Vec<u8>,
    // Whether each open container has a member yet.
    stack: Vec<bool>,
    // The name given and not written yet: JsonWriter writes a name with
    // its value, so a name whose value never comes is never written.
    deferred: String,
    has_deferred: bool,
}

// An integer as Long.toString writes it.
pub fn push_int(out: &mut Vec<u8>, value: i64) {
    let mut digits = [0u8; 20];
    let mut at = digits.len();
    let mut rest = value.unsigned_abs();
    loop {
        at -= 1;
        digits[at] = b'0' + (rest % 10) as u8;
        rest /= 10;
        if rest == 0 {
            break;
        }
    }
    if value < 0 {
        out.push(b'-');
    }
    out.extend_from_slice(&digits[at..]);
}

impl Writer {
    pub fn new() -> Writer {
        Writer { out: Vec::new(), stack: Vec::new(), deferred: String::new(), has_deferred: false }
    }

    fn before_value(&mut self) {
        if self.has_deferred {
            self.has_deferred = false;
            let has = self.stack.last_mut().expect("a name in an object");
            if *has {
                self.out.push(b',');
            }
            *has = true;
            write_string(&mut self.out, &self.deferred, false);
            self.out.push(b':');
        } else if let Some(has) = self.stack.last_mut() {
            if *has {
                self.out.push(b',');
            }
            *has = true;
        }
    }

    pub fn begin_object(&mut self) -> &mut Writer {
        self.before_value();
        self.out.push(b'{');
        self.stack.push(false);
        self
    }

    pub fn end_object(&mut self) -> &mut Writer {
        self.stack.pop();
        self.out.push(b'}');
        self
    }

    pub fn begin_array(&mut self) -> &mut Writer {
        self.before_value();
        self.out.push(b'[');
        self.stack.push(false);
        self
    }

    pub fn end_array(&mut self) -> &mut Writer {
        self.stack.pop();
        self.out.push(b']');
        self
    }

    // JsonWriter.name, on a writer that is not HTML-safe.
    pub fn name(&mut self, name: &str) -> &mut Writer {
        assert!(!self.has_deferred, "Already wrote a name, expecting a value");
        self.deferred.clear();
        self.deferred.push_str(name);
        self.has_deferred = true;
        self
    }

    // JsonWriter.value(String), not HTML-safe.
    pub fn value_str(&mut self, text: &str) -> &mut Writer {
        self.before_value();
        write_string(&mut self.out, text, false);
        self
    }

    pub fn value_int(&mut self, value: i64) -> &mut Writer {
        self.before_value();
        push_int(&mut self.out, value);
        self
    }

    pub fn value_bool(&mut self, value: bool) -> &mut Writer {
        self.before_value();
        self.out.extend_from_slice(if value { b"true" } else { b"false" });
        self
    }

    // JsonWriter.jsonValue("null").
    pub fn value_null(&mut self) -> &mut Writer {
        self.before_value();
        self.out.extend_from_slice(b"null");
        self
    }

    // gson.toJson(element, writer).
    pub fn value_element(&mut self, value: &J) -> &mut Writer {
        // A null written under a name would leave the name out, which
        // the report never asks for.
        assert!(*value != J::Null, "a null member written by name");
        self.before_value();
        write_element(&mut self.out, value, true);
        self
    }

    pub fn len(&self) -> usize {
        self.out.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The writer as it was written character by character.
    fn reference_write_string(out: &mut Vec<u8>, text: &str, html: bool) {
        out.push(b'"');
        for c in text.chars() {
            match c {
                '"' => out.extend_from_slice(b"\\\""),
                '\\' => out.extend_from_slice(b"\\\\"),
                '\t' => out.extend_from_slice(b"\\t"),
                '\u{8}' => out.extend_from_slice(b"\\b"),
                '\n' => out.extend_from_slice(b"\\n"),
                '\r' => out.extend_from_slice(b"\\r"),
                '\u{c}' => out.extend_from_slice(b"\\f"),
                '<' | '>' | '&' | '=' | '\'' if html => out.extend_from_slice(format!("\\u{:04x}", c as u32).as_bytes()),
                c if (c as u32) < 0x20 || c == '\u{2028}' || c == '\u{2029}' => out.extend_from_slice(format!("\\u{:04x}", c as u32).as_bytes()),
                c => {
                    let mut buf = [0; 4];
                    out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
                }
            }
        }
        out.push(b'"');
    }

    #[test]
    fn strings_are_written_as_the_character_by_character_writer_did() {
        let alphabet: Vec<char> = "ab \"\\\t\u{8}\n\r\u{c}\u{1}\u{1f}\u{7f}<>&='\u{e9}\u{20ac}\u{2026}\u{2028}\u{2029}\u{2027}\u{202a}\u{1f600}".chars().collect();
        let mut state = 12345u64;
        for _ in 0..20000 {
            let mut text = String::new();
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            for _ in 0..(state >> 60) {
                state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                text.push(alphabet[(state >> 33) as usize % alphabet.len()]);
            }
            for html in [false, true] {
                let (mut a, mut b) = (Vec::new(), Vec::new());
                write_string(&mut a, &text, html);
                reference_write_string(&mut b, &text, html);
                assert_eq!(a, b, "{text:?} html {html}");
            }
        }
    }

    #[test]
    fn numbers_keep_their_text_and_objects_their_order() {
        let value = parse(r#"{"b": 1.0, "a": [1e3, -0, "xé😀"], "b": 2}"#);
        assert_eq!(value.to_json(), "{\"b\":2,\"a\":[1e3,-0,\"x\u{e9}\u{1f600}\"]}");
    }

    #[test]
    fn gson_escapes_html_in_what_it_writes_and_not_in_a_hand_made_writer() {
        let value = J::string("lever[a=b,c='d'] <&>\n\u{2028}");
        assert_eq!(value.to_json(), "\"lever[a\\u003db,c\\u003d\\u0027d\\u0027] \\u003c\\u0026\\u003e\\n\\u2028\"");
        let mut writer = Writer::new();
        writer.value_str("lever[a=b,c='d'] <&>");
        assert_eq!(String::from_utf8(writer.out).unwrap(), "\"lever[a=b,c='d'] <&>\"");
    }

    #[test]
    fn null_members_are_left_out_and_null_elements_are_not() {
        let mut object = J::obj();
        object.put("a", J::Null);
        object.put("b", J::Arr(vec![J::Null, J::int(1)]));
        assert_eq!(object.to_json(), "{\"b\":[null,1]}");
    }

    #[test]
    fn integers_are_written_as_long_to_string_does() {
        for value in [0, 1, -1, 9, 10, 99, 100, -100, 123456789, i64::MAX, i64::MIN, i64::MIN + 1] {
            let mut out = Vec::new();
            push_int(&mut out, value);
            assert_eq!(String::from_utf8(out).unwrap(), value.to_string());
        }
    }

    #[test]
    fn a_name_is_written_with_its_value_only() {
        let mut writer = Writer::new();
        writer.begin_object().name("a").value_int(1).name("b");
        writer.end_object();
        assert_eq!(String::from_utf8(writer.out).unwrap(), "{\"a\":1}");
    }
}
