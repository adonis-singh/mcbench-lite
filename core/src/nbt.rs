// NBT values, as text and from text: net.minecraft.nbt's tags, the
// StringTagVisitor that prints them (Tag.toString) and the SNBT grammar that
// reads them (TagParser, SnbtGrammar, run with NbtOps).
//
// The reader returns an error for text it does not read the way Minecraft
// does: text the grammar rejects (whose error message this port does not
// write), and the parts of the grammar this port does not model (the built-in
// operations bool() and uuid(), \N{name} escapes). Text it reads, it reads
// as the grammar does.

use crate::jfmt::{double_to_string, float_to_string};

#[derive(Clone, Debug, PartialEq)]
pub enum Tag {
    Byte(i8),
    Short(i16),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    String(String),
    ByteArray(Vec<i8>),
    IntArray(Vec<i32>),
    LongArray(Vec<i64>),
    List(Vec<Tag>),
    Compound(Compound),
}

// CompoundTag: a map from names to tags. Names print in sorted order
// (StringTagVisitor), so the order kept here only matters to equality, which
// ignores it.
#[derive(Clone, Debug, Default)]
pub struct Compound(pub Vec<(String, Tag)>);

impl PartialEq for Compound {
    fn eq(&self, other: &Compound) -> bool {
        self.0.len() == other.0.len() && self.0.iter().all(|(key, value)| other.get(key) == Some(value))
    }
}

impl Compound {
    pub fn new() -> Compound {
        Compound(Vec::new())
    }

    pub fn get(&self, key: &str) -> Option<&Tag> {
        self.0.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    pub fn contains(&self, key: &str) -> bool {
        self.get(key).is_some()
    }

    // CompoundTag.put: a tag replaces the one with this name.
    pub fn put(&mut self, key: &str, value: Tag) {
        match self.0.iter_mut().find(|(k, _)| k == key) {
            Some(entry) => entry.1 = value,
            None => self.0.push((key.to_string(), value)),
        }
    }

    pub fn remove(&mut self, key: &str) -> Option<Tag> {
        let at = self.0.iter().position(|(k, _)| k == key)?;
        Some(self.0.remove(at).1)
    }

    pub fn put_int(&mut self, key: &str, value: i32) {
        self.put(key, Tag::Int(value));
    }

    pub fn put_byte(&mut self, key: &str, value: i8) {
        self.put(key, Tag::Byte(value));
    }

    pub fn put_boolean(&mut self, key: &str, value: bool) {
        self.put(key, Tag::Byte(value as i8));
    }

    pub fn put_string(&mut self, key: &str, value: &str) {
        self.put(key, Tag::String(value.to_string()));
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    // CompoundTag.merge: each tag of the other replaces the one of its name
    // (a compound merges into a compound).
    pub fn merge(&mut self, other: &Compound) {
        for (key, value) in &other.0 {
            match (self.get(key).cloned(), value) {
                (Some(Tag::Compound(mut mine)), Tag::Compound(theirs)) => {
                    mine.merge(theirs);
                    self.put(key, Tag::Compound(mine));
                }
                _ => self.put(key, value.clone()),
            }
        }
    }

    // The keys in String.compareTo order (UTF-16 code units).
    pub fn sorted(&self) -> Vec<&(String, Tag)> {
        let mut entries: Vec<&(String, Tag)> = self.0.iter().collect();
        entries.sort_by(|a, b| a.0.encode_utf16().cmp(b.0.encode_utf16()));
        entries
    }
}

// Mth.floor.
fn floor_i32(v: f64) -> i32 {
    let i = v as i32;
    if v < i as f64 {
        i - 1
    } else {
        i
    }
}

impl Tag {
    pub fn compound(entries: Vec<(&str, Tag)>) -> Tag {
        Tag::Compound(Compound(entries.into_iter().map(|(k, v)| (k.to_string(), v)).collect()))
    }

    pub fn string(s: &str) -> Tag {
        Tag::String(s.to_string())
    }

    // DoubleTag.valueOf and FloatTag.valueOf: a zero is the positive zero.
    pub fn double(v: f64) -> Tag {
        Tag::Double(if v == 0.0 { 0.0 } else { v })
    }

    pub fn float(v: f32) -> Tag {
        Tag::Float(if v == 0.0 { 0.0 } else { v })
    }

    pub fn is_numeric(&self) -> bool {
        matches!(self, Tag::Byte(_) | Tag::Short(_) | Tag::Int(_) | Tag::Long(_) | Tag::Float(_) | Tag::Double(_))
    }

    // TagType.getName.
    pub fn type_name(&self) -> &'static str {
        match self {
            Tag::Byte(_) => "BYTE",
            Tag::Short(_) => "SHORT",
            Tag::Int(_) => "INT",
            Tag::Long(_) => "LONG",
            Tag::Float(_) => "FLOAT",
            Tag::Double(_) => "DOUBLE",
            Tag::String(_) => "STRING",
            Tag::ByteArray(_) => "BYTE[]",
            Tag::IntArray(_) => "INT[]",
            Tag::LongArray(_) => "LONG[]",
            Tag::List(_) => "LIST",
            Tag::Compound(_) => "COMPOUND",
        }
    }

    // NumericTag.byteValue and the others, which the ValueInput getters
    // (getIntOr and the like) read a number with.
    pub fn byte_value(&self) -> Option<i8> {
        Some(match self {
            Tag::Byte(v) => *v,
            Tag::Short(v) => *v as i8,
            Tag::Int(v) => *v as i8,
            Tag::Long(v) => *v as i8,
            Tag::Float(v) => (floor_i32(*v as f64) & 0xFF) as u8 as i8,
            Tag::Double(v) => (floor_i32(*v) & 0xFF) as u8 as i8,
            _ => return None,
        })
    }

    pub fn short_value(&self) -> Option<i16> {
        Some(match self {
            Tag::Byte(v) => *v as i16,
            Tag::Short(v) => *v,
            Tag::Int(v) => *v as i16,
            Tag::Long(v) => *v as i16,
            Tag::Float(v) => (floor_i32(*v as f64) & 65535) as u16 as i16,
            Tag::Double(v) => (floor_i32(*v) & 65535) as u16 as i16,
            _ => return None,
        })
    }

    pub fn int_value(&self) -> Option<i32> {
        Some(match self {
            Tag::Byte(v) => *v as i32,
            Tag::Short(v) => *v as i32,
            Tag::Int(v) => *v,
            Tag::Long(v) => *v as i32,
            Tag::Float(v) => floor_i32(*v as f64),
            Tag::Double(v) => floor_i32(*v),
            _ => return None,
        })
    }

    pub fn long_value(&self) -> Option<i64> {
        Some(match self {
            Tag::Byte(v) => *v as i64,
            Tag::Short(v) => *v as i64,
            Tag::Int(v) => *v as i64,
            Tag::Long(v) => *v,
            Tag::Float(v) => *v as i64,
            Tag::Double(v) => v.floor() as i64,
            _ => return None,
        })
    }

    pub fn float_value(&self) -> Option<f32> {
        Some(match self {
            Tag::Byte(v) => *v as f32,
            Tag::Short(v) => *v as f32,
            Tag::Int(v) => *v as f32,
            Tag::Long(v) => *v as f32,
            Tag::Float(v) => *v,
            Tag::Double(v) => *v as f32,
            _ => return None,
        })
    }

    pub fn double_value(&self) -> Option<f64> {
        Some(match self {
            Tag::Byte(v) => *v as f64,
            Tag::Short(v) => *v as f64,
            Tag::Int(v) => *v as f64,
            Tag::Long(v) => *v as f64,
            Tag::Float(v) => *v as f64,
            Tag::Double(v) => *v,
            _ => return None,
        })
    }

    // NumericTag.box(), the Number a DynamicOps reads, with its intValue()
    // (Java's narrowing conversions, not the tag's own).
    pub fn box_int(&self) -> Option<i32> {
        Some(match self {
            Tag::Byte(v) => *v as i32,
            Tag::Short(v) => *v as i32,
            Tag::Int(v) => *v,
            Tag::Long(v) => *v as i32,
            Tag::Float(v) => *v as i32,
            Tag::Double(v) => *v as i32,
            _ => return None,
        })
    }

    // Number.longValue() of the boxed number.
    pub fn box_long(&self) -> Option<i64> {
        Some(match self {
            Tag::Byte(v) => *v as i64,
            Tag::Short(v) => *v as i64,
            Tag::Int(v) => *v as i64,
            Tag::Long(v) => *v,
            Tag::Float(v) => *v as i64,
            Tag::Double(v) => *v as i64,
            _ => return None,
        })
    }

    // Number.byteValue() of the boxed number: (byte) intValue() for the
    // floating types and Integer, (byte) value for the others.
    pub fn box_byte(&self) -> Option<i8> {
        Some(match self {
            Tag::Byte(v) => *v,
            Tag::Short(v) => *v as i8,
            Tag::Int(v) => *v as i8,
            Tag::Long(v) => *v as i8,
            Tag::Float(v) => (*v as i32) as i8,
            Tag::Double(v) => (*v as i32) as i8,
            _ => return None,
        })
    }

    pub fn box_float(&self) -> Option<f32> {
        self.float_value()
    }

    pub fn box_double(&self) -> Option<f64> {
        self.double_value()
    }

    pub fn as_compound(&self) -> Option<&Compound> {
        match self {
            Tag::Compound(c) => Some(c),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Tag::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_list(&self) -> Option<&Vec<Tag>> {
        match self {
            Tag::List(l) => Some(l),
            _ => None,
        }
    }

    // Tag.toString (StringTagVisitor).
    pub fn to_snbt(&self) -> String {
        let mut out = String::new();
        self.write_snbt(&mut out);
        out
    }

    // McbenchEngine.snbt, how the engine's state dump writes a block entity's
    // data: a compound's keys sorted and written as they are (never quoted),
    // a list's elements in the same way, any other tag as its toString.
    pub fn write_engine_snbt(&self, out: &mut String) {
        match self {
            Tag::Compound(c) => {
                out.push('{');
                for (i, (key, value)) in c.sorted().into_iter().enumerate() {
                    if i != 0 {
                        out.push(',');
                    }
                    out.push_str(key);
                    out.push(':');
                    value.write_engine_snbt(out);
                }
                out.push('}');
            }
            Tag::List(list) => {
                out.push('[');
                for (i, value) in list.iter().enumerate() {
                    if i != 0 {
                        out.push(',');
                    }
                    value.write_engine_snbt(out);
                }
                out.push(']');
            }
            other => other.write_snbt(out),
        }
    }

    // The tag's id in the binary format (Tag.getId).
    pub fn id(&self) -> u8 {
        match self {
            Tag::Byte(_) => 1,
            Tag::Short(_) => 2,
            Tag::Int(_) => 3,
            Tag::Long(_) => 4,
            Tag::Float(_) => 5,
            Tag::Double(_) => 6,
            Tag::ByteArray(_) => 7,
            Tag::String(_) => 8,
            Tag::List(_) => 9,
            Tag::Compound(_) => 10,
            Tag::IntArray(_) => 11,
            Tag::LongArray(_) => 12,
        }
    }

    // Tag.write: the payload as the binary format (what NbtIo writes, big
    // endian), without the id and the name that come before it in a compound.
    pub fn write_binary(&self, out: &mut Vec<u8>) {
        match self {
            Tag::Byte(v) => out.push(*v as u8),
            Tag::Short(v) => out.extend_from_slice(&v.to_be_bytes()),
            Tag::Int(v) => out.extend_from_slice(&v.to_be_bytes()),
            Tag::Long(v) => out.extend_from_slice(&v.to_be_bytes()),
            Tag::Float(v) => out.extend_from_slice(&v.to_bits().to_be_bytes()),
            Tag::Double(v) => out.extend_from_slice(&v.to_bits().to_be_bytes()),
            Tag::ByteArray(a) => {
                out.extend_from_slice(&(a.len() as i32).to_be_bytes());
                out.extend(a.iter().map(|&b| b as u8));
            }
            Tag::String(s) => write_utf(s, out),
            Tag::List(items) => {
                // An empty list has the end tag's id.
                out.push(items.first().map_or(0, Tag::id));
                out.extend_from_slice(&(items.len() as i32).to_be_bytes());
                for item in items {
                    assert!(item.id() == items[0].id(), "a list of mixed tags");
                    item.write_binary(out);
                }
            }
            Tag::Compound(c) => {
                for (key, value) in &c.0 {
                    out.push(value.id());
                    write_utf(key, out);
                    value.write_binary(out);
                }
                out.push(0);
            }
            Tag::IntArray(a) => {
                out.extend_from_slice(&(a.len() as i32).to_be_bytes());
                for v in a {
                    out.extend_from_slice(&v.to_be_bytes());
                }
            }
            Tag::LongArray(a) => {
                out.extend_from_slice(&(a.len() as i32).to_be_bytes());
                for v in a {
                    out.extend_from_slice(&v.to_be_bytes());
                }
            }
        }
    }

    pub fn write_snbt(&self, out: &mut String) {
        match self {
            Tag::String(s) => quote_and_escape(s, out),
            Tag::Byte(v) => {
                out.push_str(&v.to_string());
                out.push('b');
            }
            Tag::Short(v) => {
                out.push_str(&v.to_string());
                out.push('s');
            }
            Tag::Int(v) => out.push_str(&v.to_string()),
            Tag::Long(v) => {
                out.push_str(&v.to_string());
                out.push('L');
            }
            Tag::Float(v) => {
                out.push_str(&float_to_string(*v));
                out.push('f');
            }
            Tag::Double(v) => {
                out.push_str(&double_to_string(*v));
                out.push('d');
            }
            Tag::ByteArray(a) => {
                out.push_str("[B;");
                for (i, v) in a.iter().enumerate() {
                    if i != 0 {
                        out.push(',');
                    }
                    out.push_str(&v.to_string());
                    out.push('B');
                }
                out.push(']');
            }
            Tag::IntArray(a) => {
                out.push_str("[I;");
                for (i, v) in a.iter().enumerate() {
                    if i != 0 {
                        out.push(',');
                    }
                    out.push_str(&v.to_string());
                }
                out.push(']');
            }
            Tag::LongArray(a) => {
                out.push_str("[L;");
                for (i, v) in a.iter().enumerate() {
                    if i != 0 {
                        out.push(',');
                    }
                    out.push_str(&v.to_string());
                    out.push('L');
                }
                out.push(']');
            }
            Tag::List(l) => {
                out.push('[');
                for (i, v) in l.iter().enumerate() {
                    if i != 0 {
                        out.push(',');
                    }
                    v.write_snbt(out);
                }
                out.push(']');
            }
            Tag::Compound(c) => {
                out.push('{');
                for (i, (key, value)) in c.sorted().into_iter().enumerate() {
                    if i != 0 {
                        out.push(',');
                    }
                    if unquoted_key(key) {
                        out.push_str(key);
                    } else {
                        quote_and_escape(key, out);
                    }
                    out.push(':');
                    value.write_snbt(out);
                }
                out.push('}');
            }
        }
    }
}

impl std::fmt::Display for Tag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.to_snbt())
    }
}

impl std::fmt::Display for Compound {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&Tag::Compound(self.clone()).to_snbt())
    }
}

// StringTagVisitor.UNQUOTED_KEY_MATCH ([A-Za-z._]+[A-Za-z0-9._+-]*), and
// not "true" or "false" in any case.
fn unquoted_key(key: &str) -> bool {
    if key.eq_ignore_ascii_case("true") || key.eq_ignore_ascii_case("false") {
        return false;
    }
    let mut chars = key.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '.' || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '+' | '-'))
}

// SnbtGrammar.escapeControlCharacters.
fn escape_control(c: char) -> Option<String> {
    match c {
        '\u{8}' => Some("b".into()),
        '\t' => Some("t".into()),
        '\n' => Some("n".into()),
        '\u{c}' => Some("f".into()),
        '\r' => Some("r".into()),
        c if (c as u32) < 32 => Some(format!("x{:02X}", c as u32)),
        _ => None,
    }
}

// StringTag.quoteAndEscape.
// DataOutput.writeUTF: a length and the modified UTF-8 of the string (each
// UTF-16 unit on its own, a NUL as two bytes).
pub fn write_utf(text: &str, out: &mut Vec<u8>) {
    let mut bytes = Vec::with_capacity(text.len());
    for unit in text.encode_utf16() {
        match unit {
            1..=0x7f => bytes.push(unit as u8),
            0 | 0x80..=0x7ff => bytes.extend_from_slice(&[0xc0 | (unit >> 6) as u8, 0x80 | (unit & 0x3f) as u8]),
            _ => bytes.extend_from_slice(&[0xe0 | (unit >> 12) as u8, 0x80 | ((unit >> 6) & 0x3f) as u8, 0x80 | (unit & 0x3f) as u8]),
        }
    }
    assert!(bytes.len() <= 65535, "a string too long to write");
    out.extend_from_slice(&(bytes.len() as u16).to_be_bytes());
    out.extend_from_slice(&bytes);
}

pub fn quote_and_escape(text: &str, out: &mut String) {
    let start = out.len();
    out.push(' ');
    let mut quote = '\0';
    for c in text.chars() {
        if c == '\\' {
            out.push_str("\\\\");
        } else if c != '"' && c != '\'' {
            match escape_control(c) {
                Some(escape) => {
                    out.push('\\');
                    out.push_str(&escape);
                }
                None => out.push(c),
            }
        } else {
            if quote == '\0' {
                quote = if c == '"' { '\'' } else { '"' };
            }
            if quote == c {
                out.push('\\');
            }
            out.push(c);
        }
    }
    if quote == '\0' {
        quote = '"';
    }
    out.replace_range(start..start + 1, &quote.to_string());
    out.push(quote);
}

// --- Reading ---

// Why text was not read: the grammar rejects it, or it uses what this port
// does not model.
#[derive(Debug, Clone, PartialEq)]
pub enum ParseError {
    Syntax,
    Unsupported(&'static str),
}

// Character.isWhitespace.
fn is_whitespace(c: u16) -> bool {
    matches!(c, 0x09..=0x0D | 0x1C..=0x1F | 0x20 | 0x1680 | 0x2000..=0x2006 | 0x2008..=0x200A | 0x2028 | 0x2029 | 0x205F | 0x3000)
}

struct Reader {
    // The text as UTF-16, as Java's StringReader indexes it.
    text: Vec<u16>,
    at: usize,
    depth: i32,
}

type Parsed<T> = Result<Option<T>, ParseError>;

#[derive(Clone, Copy, PartialEq)]
enum Suffix {
    Float,
    Double,
    Byte,
    Short,
    Int,
    Long,
}

#[derive(Clone, Copy, PartialEq)]
enum Signedness {
    Signed,
    Unsigned,
}

struct IntegerLiteral {
    minus: bool,
    radix: u32,
    // The digits, with their underscores.
    digits: String,
    signed: Option<Signedness>,
    suffix: Option<Suffix>,
}

impl Reader {
    fn peek(&self) -> Option<u16> {
        self.text.get(self.at).copied()
    }

    fn skip_whitespace(&mut self) {
        while self.at < self.text.len() && is_whitespace(self.text[self.at]) {
            self.at += 1;
        }
    }

    // StringReaderTerms.character(s): whitespace, then one of the characters.
    fn character(&mut self, accepted: &[char]) -> bool {
        self.skip_whitespace();
        match self.peek() {
            Some(c) if accepted.iter().any(|a| *a as u32 == c as u32) => {
                self.at += 1;
                true
            }
            _ => false,
        }
    }

    // A positive lookahead for characters: nothing consumed.
    fn looks_at(&mut self, accepted: &[char]) -> bool {
        let mark = self.at;
        let found = self.character(accepted);
        self.at = mark;
        found
    }

    // The NUMBER_LOOKEAHEAD: whitespace, then a character that can start a number.
    fn looks_at_number(&mut self) -> bool {
        let mark = self.at;
        self.skip_whitespace();
        let found = matches!(self.peek(), Some(c) if matches!(char::from_u32(c as u32), Some('+' | '-' | '.' | '0'..='9')));
        self.at = mark;
        found
    }

    // NumberRunParseRule: whitespace, then a run of the digits (and
    // underscores), which may not start or end with an underscore.
    fn numeral(&mut self, accepted: fn(u8) -> bool) -> Option<String> {
        self.skip_whitespace();
        let start = self.at;
        let mut end = start;
        while end < self.text.len() && self.text[end] < 128 && accepted(self.text[end] as u8) {
            end += 1;
        }
        if end == start || self.text[start] == b'_' as u16 || self.text[end - 1] == b'_' as u16 {
            return None;
        }
        self.at = end;
        Some(self.text[start..end].iter().map(|c| *c as u8 as char).collect())
    }

    fn decimal(&mut self) -> Option<String> {
        self.numeral(|c| c.is_ascii_digit() || c == b'_')
    }

    fn hex(&mut self) -> Option<String> {
        self.numeral(|c| c.is_ascii_hexdigit() || c == b'_')
    }

    fn binary(&mut self) -> Option<String> {
        self.numeral(|c| c == b'0' || c == b'1' || c == b'_')
    }

    fn sign(&mut self) -> Option<bool> {
        if self.character(&['+']) {
            Some(false)
        } else if self.character(&['-']) {
            Some(true)
        } else {
            None
        }
    }

    // integer_suffix.
    fn integer_suffix(&mut self) -> (Option<Signedness>, Option<Suffix>) {
        let mark = self.at;
        let kinds = |s: &mut Reader| -> Option<Suffix> {
            if s.character(&['b', 'B']) {
                Some(Suffix::Byte)
            } else if s.character(&['s', 'S']) {
                Some(Suffix::Short)
            } else if s.character(&['i', 'I']) {
                Some(Suffix::Int)
            } else if s.character(&['l', 'L']) {
                Some(Suffix::Long)
            } else {
                None
            }
        };
        if self.character(&['u', 'U']) {
            if let Some(kind) = kinds(self) {
                return (Some(Signedness::Unsigned), Some(kind));
            }
            self.at = mark;
        } else if self.character(&['s', 'S']) {
            if let Some(kind) = kinds(self) {
                return (Some(Signedness::Signed), Some(kind));
            }
            self.at = mark;
        }
        // The alternative's bare forms: a b, s, i or l.
        if let Some(kind) = kinds(self) {
            return (None, Some(kind));
        }
        self.at = mark;
        (None, None)
    }

    // integer_literal: sign, a number (with the leading-zero rules), and an
    // optional suffix. None is the rule failing.
    fn integer_literal(&mut self) -> Option<IntegerLiteral> {
        let start = self.at;
        let minus = self.sign().unwrap_or(false);
        let body = self.integer_body();
        let Some((radix, digits)) = body else {
            self.at = start;
            return None;
        };
        let (signed, suffix) = self.integer_suffix();
        Some(IntegerLiteral { minus, radix, digits, signed, suffix })
    }

    // Alternative(Sequence('0', cut, Alternative(...)), decimal_numeral).
    fn integer_body(&mut self) -> Option<(u32, String)> {
        let mark = self.at;
        if self.character(&['0']) {
            // The cut: if what follows the 0 does not parse, the rule fails.
            let after_zero = self.at;
            if self.character(&['x', 'X']) {
                // Sequence('x', cut, hex_numeral): the cut ends the choice.
                return self.hex().map(|d| (16, d));
            }
            self.at = after_zero;
            let mark_b = self.at;
            if self.character(&['b', 'B']) {
                if let Some(d) = self.binary() {
                    return Some((2, d));
                }
            }
            self.at = mark_b;
            if self.decimal().is_some() {
                // Sequence(decimal_numeral, cut, fail(leading zero)).
                return None;
            }
            self.at = after_zero;
            return Some((10, "0".into()));
        }
        self.at = mark;
        self.decimal().map(|d| (10, d))
    }

    // IntegerLiteral.create: the tag for a suffix (the literal's own, or int).
    fn make_integer(literal: &IntegerLiteral, suffix: Suffix) -> Option<Tag> {
        let signed = match literal.signed {
            Some(s) => s == Signedness::Signed,
            None => literal.radix == 10,
        };
        if !signed && literal.minus {
            return None;
        }
        let mut digits: String = literal.digits.chars().filter(|c| *c != '_').collect();
        if literal.minus {
            digits.insert(0, '-');
        }
        let radix = literal.radix;
        if signed {
            Some(match suffix {
                Suffix::Byte => Tag::Byte(i8::from_str_radix(&digits, radix).ok()?),
                Suffix::Short => Tag::Short(i16::from_str_radix(&digits, radix).ok()?),
                Suffix::Int => Tag::Int(i32::from_str_radix(&digits, radix).ok()?),
                Suffix::Long => Tag::Long(i64::from_str_radix(&digits, radix).ok()?),
                _ => return None,
            })
        } else {
            Some(match suffix {
                Suffix::Byte => Tag::Byte(u8::from_str_radix(&digits, radix).ok()? as i8),
                Suffix::Short => Tag::Short(u16::from_str_radix(&digits, radix).ok()? as i16),
                Suffix::Int => Tag::Int(u32::from_str_radix(&digits, radix).ok()? as i32),
                Suffix::Long => Tag::Long(u64::from_str_radix(&digits, radix).ok()? as i64),
                _ => return None,
            })
        }
    }

    fn float_suffix(&mut self) -> Option<Suffix> {
        if self.character(&['f', 'F']) {
            Some(Suffix::Float)
        } else if self.character(&['d', 'D']) {
            Some(Suffix::Double)
        } else {
            None
        }
    }

    // float_exponent_part: e or E, an optional sign, a decimal numeral.
    fn float_exponent(&mut self) -> Option<(bool, String)> {
        let mark = self.at;
        if self.character(&['e', 'E']) {
            let minus = self.sign().unwrap_or(false);
            if let Some(digits) = self.decimal() {
                return Some((minus, digits));
            }
        }
        self.at = mark;
        None
    }

    // float_literal: the four forms, then the value (createFloat). None is
    // the rule failing, including a value that is not finite.
    fn float_literal(&mut self) -> Option<Tag> {
        let start = self.at;
        let minus = self.sign().unwrap_or(false);
        let mut whole: Option<String> = None;
        let mut fraction: Option<String> = None;
        let mut exponent: Option<(bool, String)> = None;
        let mut suffix: Option<Suffix> = None;
        let mut matched = false;
        // Alternative 1: whole '.' cut [fraction] [exponent] [suffix].
        let mark = self.at;
        if let Some(w) = self.decimal() {
            if self.character(&['.']) {
                whole = Some(w);
                fraction = self.decimal();
                exponent = self.float_exponent();
                suffix = self.float_suffix();
                matched = true;
            }
        }
        if !matched {
            self.at = mark;
            // Alternative 2: '.' cut fraction [exponent] [suffix]. After the
            // cut a failure ends the rule.
            if self.character(&['.']) {
                match self.decimal() {
                    Some(f) => {
                        fraction = Some(f);
                        exponent = self.float_exponent();
                        suffix = self.float_suffix();
                        matched = true;
                    }
                    None => {
                        self.at = start;
                        return None;
                    }
                }
            }
        }
        if !matched {
            self.at = mark;
            // Alternative 3: whole exponent cut [suffix].
            if let Some(w) = self.decimal() {
                if let Some(e) = self.float_exponent() {
                    whole = Some(w);
                    exponent = Some(e);
                    suffix = self.float_suffix();
                    matched = true;
                }
            }
        }
        if !matched {
            self.at = mark;
            // Alternative 4: whole [exponent] suffix.
            if let Some(w) = self.decimal() {
                let exp = self.float_exponent();
                if let Some(s) = self.float_suffix() {
                    whole = Some(w);
                    exponent = exp;
                    suffix = Some(s);
                    matched = true;
                }
            }
        }
        if !matched {
            self.at = start;
            return None;
        }
        let mut text = String::new();
        if minus {
            text.push('-');
        }
        if let Some(w) = &whole {
            text.extend(w.chars().filter(|c| *c != '_'));
        }
        if let Some(f) = &fraction {
            text.push('.');
            text.extend(f.chars().filter(|c| *c != '_'));
        }
        if let Some((exp_minus, digits)) = &exponent {
            text.push('e');
            if *exp_minus {
                text.push('-');
            }
            text.extend(digits.chars().filter(|c| *c != '_'));
        }
        // Rust reads what Java's parseDouble reads here ("1.", ".5", "1e5"),
        // each correctly rounded.
        let value = match suffix {
            None | Some(Suffix::Double) => text.parse::<f64>().ok().filter(|v| v.is_finite()).map(Tag::double),
            Some(Suffix::Float) => text.parse::<f32>().ok().filter(|v| v.is_finite()).map(Tag::float),
            Some(_) => None,
        };
        if value.is_none() {
            self.at = start;
        }
        value
    }

    // quoted_string_literal.
    fn quoted_string(&mut self) -> Parsed<String> {
        self.skip_whitespace();
        let quote = match self.peek() {
            Some(0x22) => 0x22u16,
            Some(0x27) => 0x27u16,
            _ => return Ok(None),
        };
        let mark = self.at;
        self.at += 1;
        let mut chunks: Vec<u16> = Vec::new();
        loop {
            match self.peek() {
                None => {
                    self.at = mark;
                    return Ok(None);
                }
                Some(c) if c == quote => {
                    self.at += 1;
                    break;
                }
                Some(0x5C) => {
                    self.at += 1;
                    match self.escape_sequence()? {
                        Some(units) => chunks.extend(units),
                        None => {
                            self.at = mark;
                            return Ok(None);
                        }
                    }
                }
                Some(c) => {
                    // A plain chunk, or the other quote character.
                    chunks.push(c);
                    self.at += 1;
                }
            }
        }
        // A lone surrogate (from an escape) is a char this port cannot hold.
        match String::from_utf16(&chunks) {
            Ok(s) => Ok(Some(s)),
            Err(_) => Err(ParseError::Unsupported("a string with an unpaired surrogate")),
        }
    }

    fn hex_digits(&mut self, count: usize) -> Option<u32> {
        if self.at + count > self.text.len() {
            return None;
        }
        let mut value: u32 = 0;
        for i in 0..count {
            let c = self.text[self.at + i];
            let digit = char::from_u32(c as u32).and_then(|c| c.to_digit(16))?;
            value = (value << 4) | digit;
        }
        self.at += count;
        Some(value)
    }

    // string_escape_sequence, after the backslash. Some(units) is the text it
    // stands for; None is the rule failing.
    fn escape_sequence(&mut self) -> Result<Option<Vec<u16>>, ParseError> {
        let Some(c) = self.peek() else { return Ok(None) };
        self.at += 1;
        let one = |c: char| Ok(Some(vec![c as u16]));
        match char::from_u32(c as u32) {
            Some('b') => one('\u{8}'),
            Some('s') => one(' '),
            Some('t') => one('\t'),
            Some('n') => one('\n'),
            Some('f') => one('\u{c}'),
            Some('r') => one('\r'),
            Some('\\') => one('\\'),
            Some('\'') => one('\''),
            Some('"') => one('"'),
            Some(kind @ ('x' | 'u' | 'U')) => {
                let digits = match kind {
                    'x' => 2,
                    'u' => 4,
                    _ => 8,
                };
                let Some(code) = self.hex_digits(digits) else { return Ok(None) };
                match char::from_u32(code) {
                    Some(ch) => {
                        let mut buf = [0u16; 2];
                        Ok(Some(ch.encode_utf16(&mut buf).to_vec()))
                    }
                    // A surrogate code point is one UTF-16 unit in Java.
                    None if (0xD800..=0xDFFF).contains(&code) => Ok(Some(vec![code as u16])),
                    // Character.isValidCodePoint is false: the rule fails.
                    None => Ok(None),
                }
            }
            Some('N') => Err(ParseError::Unsupported("a \\N{name} escape")),
            _ => Ok(None),
        }
    }

    // unquoted_string: whitespace, then at least one of 0-9 A-Z a-z _ - . +
    // (StringReader.readUnquotedString).
    fn unquoted_string(&mut self) -> Option<String> {
        self.skip_whitespace();
        let start = self.at;
        let mut end = start;
        while end < self.text.len() {
            let c = self.text[end];
            let allowed = c < 128 && matches!(c as u8, b'0'..=b'9' | b'A'..=b'Z' | b'a'..=b'z' | b'_' | b'-' | b'.' | b'+');
            if !allowed {
                break;
            }
            end += 1;
        }
        if end == start {
            return None;
        }
        self.at = end;
        Some(self.text[start..end].iter().map(|c| *c as u8 as char).collect())
    }

    // A literal: the grammar's `literal` rule.
    fn literal(&mut self) -> Parsed<Tag> {
        let start = self.at;
        if self.looks_at_number() {
            // Alternative(float_literal, integer_literal) after the lookahead.
            if let Some(tag) = self.float_literal() {
                return Ok(Some(tag));
            }
            self.at = start;
            if let Some(literal) = self.integer_literal() {
                let suffix = literal.suffix.unwrap_or(Suffix::Int);
                if let Some(tag) = Self::make_integer(&literal, suffix) {
                    return Ok(Some(tag));
                }
            }
            self.at = start;
        }
        if self.looks_at(&['"', '\'']) {
            // The cut: a string that does not parse ends the rule.
            return Ok(self.quoted_string()?.map(Tag::String));
        }
        if self.looks_at(&['{']) {
            return self.map_literal();
        }
        if self.looks_at(&['[']) {
            return self.list_literal();
        }
        // unquoted_string_or_builtin.
        let Some(string) = self.unquoted_string() else {
            self.at = start;
            return Ok(None);
        };
        let first = string.as_bytes()[0];
        if matches!(first, b'+' | b'-' | b'.' | b'0'..=b'9') {
            self.at = start;
            return Ok(None);
        }
        if self.looks_at(&['(']) {
            return Err(ParseError::Unsupported("a built-in operation (bool, uuid)"));
        }
        if string.eq_ignore_ascii_case("true") {
            Ok(Some(Tag::Byte(1)))
        } else if string.eq_ignore_ascii_case("false") {
            Ok(Some(Tag::Byte(0)))
        } else {
            Ok(Some(Tag::String(string)))
        }
    }

    fn enter(&mut self) -> Result<(), ParseError> {
        self.depth += 1;
        if self.depth > 512 {
            return Err(ParseError::Unsupported("SNBT nested deeper than 512"));
        }
        Ok(())
    }

    // map_literal: '{' [entries] '}', with a trailing separator allowed.
    fn map_literal(&mut self) -> Parsed<Tag> {
        let start = self.at;
        if !self.character(&['{']) {
            return Ok(None);
        }
        self.enter()?;
        let mut compound = Compound::new();
        let mut first = true;
        loop {
            let mark = self.at;
            if !first && !self.character(&[',']) {
                self.at = mark;
                break;
            }
            let after_separator = self.at;
            let Some((key, value)) = self.map_entry()? else {
                self.at = after_separator;
                break;
            };
            compound.put(&key, value);
            first = false;
        }
        self.depth -= 1;
        if !self.character(&['}']) {
            self.at = start;
            return Ok(None);
        }
        Ok(Some(Tag::Compound(compound)))
    }

    fn map_entry(&mut self) -> Parsed<(String, Tag)> {
        let start = self.at;
        let key = match self.quoted_string()? {
            Some(key) => key,
            None => match self.unquoted_string() {
                Some(key) => key,
                None => {
                    self.at = start;
                    return Ok(None);
                }
            },
        };
        if !self.character(&[':']) {
            self.at = start;
            return Ok(None);
        }
        let Some(value) = self.literal()? else {
            self.at = start;
            return Ok(None);
        };
        if key.is_empty() {
            self.at = start;
            return Ok(None);
        }
        Ok(Some((key, value)))
    }

    // list_literal: '[' (array_prefix ';' integers | literals) ']'.
    fn list_literal(&mut self) -> Parsed<Tag> {
        let start = self.at;
        if !self.character(&['[']) {
            return Ok(None);
        }
        self.enter()?;
        let after_bracket = self.at;
        let mut result: Option<Tag> = None;
        if self.looks_at(&['B', 'L', 'I']) {
            let prefix = {
                self.skip_whitespace();
                let c = self.peek().unwrap() as u8 as char;
                self.at += 1;
                c
            };
            if self.character(&[';']) {
                if let Some(literals) = self.integer_entries() {
                    if self.character(&[']']) {
                        result = self.array_of(prefix, &literals)?;
                        if result.is_none() {
                            // The array rule failed on an element: the whole
                            // alternative did, and the list form is tried.
                            self.at = after_bracket;
                        }
                    } else {
                        self.at = after_bracket;
                    }
                } else {
                    self.at = after_bracket;
                }
            } else {
                self.at = after_bracket;
            }
        }
        if let Some(tag) = result {
            self.depth -= 1;
            return Ok(Some(tag));
        }
        // list_entries.
        let mut items: Vec<Tag> = Vec::new();
        let mut first = true;
        loop {
            let mark = self.at;
            if !first && !self.character(&[',']) {
                self.at = mark;
                break;
            }
            let after_separator = self.at;
            let Some(value) = self.literal()? else {
                self.at = after_separator;
                break;
            };
            items.push(value);
            first = false;
        }
        self.depth -= 1;
        if !self.character(&[']']) {
            self.at = start;
            return Ok(None);
        }
        Ok(Some(Tag::List(items)))
    }

    // int_array_entries: integer literals separated by commas, with a trailing
    // separator allowed.
    fn integer_entries(&mut self) -> Option<Vec<IntegerLiteral>> {
        let mut items = Vec::new();
        let mut first = true;
        loop {
            let mark = self.at;
            if !first && !self.character(&[',']) {
                self.at = mark;
                break;
            }
            let after_separator = self.at;
            let Some(literal) = self.integer_literal() else {
                self.at = after_separator;
                break;
            };
            items.push(literal);
            first = false;
        }
        Some(items)
    }

    // ArrayPrefix.create. None is an element the array's type does not take.
    fn array_of(&mut self, prefix: char, literals: &[IntegerLiteral]) -> Parsed<Tag> {
        // The element suffixes each array takes (the default first).
        let allowed: &[Suffix] = match prefix {
            'B' => &[Suffix::Byte],
            'I' => &[Suffix::Int, Suffix::Byte, Suffix::Short],
            _ => &[Suffix::Long, Suffix::Byte, Suffix::Short, Suffix::Int],
        };
        let mut numbers: Vec<Tag> = Vec::new();
        for literal in literals {
            let suffix = match literal.suffix {
                None => allowed[0],
                Some(s) if allowed.contains(&s) => s,
                Some(_) => return Ok(None),
            };
            match Self::make_integer(literal, suffix) {
                Some(tag) => numbers.push(tag),
                None => return Ok(None),
            }
        }
        let long_of = |t: &Tag| t.long_value().unwrap();
        Ok(Some(match prefix {
            'B' => Tag::ByteArray(numbers.iter().map(|t| long_of(t) as i8).collect()),
            'I' => Tag::IntArray(numbers.iter().map(|t| long_of(t) as i32).collect()),
            _ => Tag::LongArray(numbers.iter().map(long_of).collect()),
        }))
    }
}

// TagParser.parseCompoundFully: a compound and nothing after it.
pub fn parse_compound(text: &str) -> Result<Compound, ParseError> {
    match parse(text)? {
        Tag::Compound(c) => Ok(c),
        _ => Err(ParseError::Syntax),
    }
}

// TagParser.parseFully for any value.
pub fn parse(text: &str) -> Result<Tag, ParseError> {
    let mut reader = Reader { text: text.encode_utf16().collect(), at: 0, depth: 0 };
    let Some(tag) = reader.literal()? else { return Err(ParseError::Syntax) };
    reader.skip_whitespace();
    if reader.at < reader.text.len() {
        return Err(ParseError::Syntax);
    }
    Ok(tag)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binary_is_what_nbtio_writes() {
        // NbtIo.write of {a:1b,b:"\u0000é",c:[1,2],d:{}}.
        let tag = parse("{a:1b,b:\"\\x00é\",c:[1,2],d:{}}").unwrap();
        let mut out = Vec::new();
        tag.write_binary(&mut out);
        assert_eq!(
            out,
            [1, 0, 1, b'a', 1, 8, 0, 1, b'b', 0, 4, 0xc0, 0x80, 0xc3, 0xa9, 9, 0, 1, b'c', 3, 0, 0, 0, 2, 0, 0, 0, 1, 0, 0, 0, 2, 10, 0, 1, b'd', 0, 0]
        );
    }

    fn roundtrip(text: &str) -> String {
        parse(text).unwrap().to_snbt()
    }

    #[test]
    fn numbers_read_by_their_suffix() {
        assert_eq!(roundtrip("{a:1,b:2b,c:3s,d:4L,e:5.5f,f:6.0,g:7d,h:1e3,i:.5,j:1.,k:0x10,l:0b101,m:1_000}"), "{a:1,b:2b,c:3s,d:4L,e:5.5f,f:6.0d,g:7.0d,h:1000.0d,i:0.5d,j:1.0d,k:16,l:5,m:1000}");
        assert_eq!(roundtrip("[0xffub, 0xffffffff, 255ub, -1sb]"), "[-1b,-1,-1b,-1b]");
        assert!(parse("01").is_err());
        assert!(parse("1e999").is_err());
        assert!(parse("-0xff").is_err());
        assert_eq!(roundtrip("{a:0}"), "{a:0}");
        assert_eq!(roundtrip("{a:0b}"), "{a:0b}");
    }

    #[test]
    fn strings_and_keys_print_as_minecraft_prints_them() {
        assert_eq!(roundtrip("{a:\"x\", \"b c\":'it\"s', 'true':\"\\n\\x01\\u00e9\"}"), "{a:\"x\",\"b c\":'it\"s',\"true\":\"\\n\\x01é\"}");
        assert_eq!(roundtrip("{a:\"it's\",b:'a\\'b\"c'}"), "{a:\"it's\",b:\"a'b\\\"c\"}");
        assert_eq!(roundtrip("{a:abc,b:true,c:False,d:x.y-z}"), "{a:\"abc\",b:1b,c:0b,d:\"x.y-z\"}");
        assert!(parse("{a:-abc}").is_err());
        assert!(parse("{\"\":1}").is_err());
    }

    #[test]
    fn lists_and_arrays() {
        assert_eq!(roundtrip("{a:[1,2,],b:[B;1,2],c:[I;1,2b,3s],d:[L;1,2],e:[],f:[I;],g:[1,'x',{}]}"), "{a:[1,2],b:[B;1B,2B],c:[I;1,2,3],d:[L;1L,2L],e:[],f:[I;],g:[1,\"x\",{}]}");
        assert!(parse("[B;1L]").is_err());
        assert_eq!(roundtrip("[B]"), "[\"B\"]");
        assert_eq!(roundtrip("{ a : [ 1 , 2 ] , }"), "{a:[1,2]}");
    }

    #[test]
    fn trailing_data_and_nonsense_fail() {
        assert!(parse_compound("{a:1} x").is_err());
        assert!(parse_compound("[1]").is_err());
        assert!(parse("{a:1").is_err());
        assert!(parse("{a:bool(1)}").is_err());
    }
}
