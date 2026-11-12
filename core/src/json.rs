// A JSON reader for the data this engine reads: loot tables and verifier
// requests.

use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<Json>),
    Object(BTreeMap<String, Json>),
}

impl Json {
    pub fn parse(text: &str) -> Json {
        let mut reader = Reader { bytes: text.as_bytes(), at: 0 };
        let value = reader.value();
        reader.space();
        assert_eq!(reader.at, reader.bytes.len(), "trailing JSON");
        value
    }

    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Object(map) => map.get(key),
            _ => None,
        }
    }

    pub fn str(&self) -> &str {
        match self {
            Json::String(s) => s,
            other => panic!("not a string: {other:?}"),
        }
    }

    pub fn num(&self) -> f64 {
        match self {
            Json::Number(n) => *n,
            other => panic!("not a number: {other:?}"),
        }
    }

    pub fn bool(&self) -> bool {
        match self {
            Json::Bool(b) => *b,
            other => panic!("not a boolean: {other:?}"),
        }
    }

    pub fn array(&self) -> &[Json] {
        match self {
            Json::Array(a) => a,
            other => panic!("not an array: {other:?}"),
        }
    }

    pub fn object(&self) -> &BTreeMap<String, Json> {
        match self {
            Json::Object(o) => o,
            other => panic!("not an object: {other:?}"),
        }
    }
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

    fn expect(&mut self, b: u8) {
        self.space();
        assert_eq!(self.bytes[self.at], b, "expected {} at {}", b as char, self.at);
        self.at += 1;
    }

    fn value(&mut self) -> Json {
        self.space();
        match self.bytes[self.at] {
            b'{' => {
                self.at += 1;
                let mut map = BTreeMap::new();
                self.space();
                if self.bytes[self.at] == b'}' {
                    self.at += 1;
                    return Json::Object(map);
                }
                loop {
                    self.space();
                    let key = self.string();
                    self.expect(b':');
                    let value = self.value();
                    map.insert(key, value);
                    self.space();
                    self.at += 1;
                    match self.bytes[self.at - 1] {
                        b',' => continue,
                        b'}' => return Json::Object(map),
                        other => panic!("bad JSON object at {}: {}", self.at, other as char),
                    }
                }
            }
            b'[' => {
                self.at += 1;
                let mut list = Vec::new();
                self.space();
                if self.bytes[self.at] == b']' {
                    self.at += 1;
                    return Json::Array(list);
                }
                loop {
                    list.push(self.value());
                    self.space();
                    self.at += 1;
                    match self.bytes[self.at - 1] {
                        b',' => continue,
                        b']' => return Json::Array(list),
                        other => panic!("bad JSON array at {}: {}", self.at, other as char),
                    }
                }
            }
            b'"' => Json::String(self.string()),
            b't' => self.word("true", Json::Bool(true)),
            b'f' => self.word("false", Json::Bool(false)),
            b'n' => self.word("null", Json::Null),
            _ => {
                let start = self.at;
                while self.at < self.bytes.len() && matches!(self.bytes[self.at], b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9') {
                    self.at += 1;
                }
                Json::Number(std::str::from_utf8(&self.bytes[start..self.at]).unwrap().parse().unwrap_or_else(|_| panic!("bad JSON number at {start}")))
            }
        }
    }

    fn word(&mut self, word: &str, value: Json) -> Json {
        assert!(self.bytes[self.at..].starts_with(word.as_bytes()), "bad JSON at {}", self.at);
        self.at += word.len();
        value
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
                            let code = u32::from_str_radix(std::str::from_utf8(&self.bytes[self.at..self.at + 4]).unwrap(), 16).unwrap();
                            self.at += 4;
                            let mut buf = [0; 4];
                            out.extend_from_slice(char::from_u32(code).expect("surrogate escapes are not read").encode_utf8(&mut buf).as_bytes());
                        }
                        other => panic!("bad JSON escape {}", other as char),
                    }
                }
                _ => out.push(b),
            }
        }
    }
}
