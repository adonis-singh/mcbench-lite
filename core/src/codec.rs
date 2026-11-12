// The parts of DFU's codecs and of ValueInput/ValueOutput that saved data is
// read and written with, over NbtOps: how a number is read from a tag, what a
// list or a map is, a resource location. A reader returns Fail where the
// codec would give an error: the caller refuses (this port does not write
// DFU's error messages), so a Fail is never a quiet default.

use crate::nbt::{Compound, Tag};

#[derive(Debug, Clone)]
pub struct Fail(pub String);

pub type R<T> = Result<T, Fail>;

pub fn fail<T>(message: impl Into<String>) -> R<T> {
    Err(Fail(message.into()))
}

// DynamicOps.getMap for NbtOps: only a compound is a map.
pub fn map(tag: &Tag) -> R<&Compound> {
    match tag {
        Tag::Compound(c) => Ok(c),
        other => fail(format!("Not a map: {}", other.to_snbt())),
    }
}

// DynamicOps.getStream for NbtOps: a list or an array (whose elements are
// numbers).
pub fn items(tag: &Tag) -> R<Vec<Tag>> {
    Ok(match tag {
        Tag::List(l) => l.clone(),
        Tag::ByteArray(a) => a.iter().map(|v| Tag::Byte(*v)).collect(),
        Tag::IntArray(a) => a.iter().map(|v| Tag::Int(*v)).collect(),
        Tag::LongArray(a) => a.iter().map(|v| Tag::Long(*v)).collect(),
        other => return fail(format!("Not a list: {}", other.to_snbt())),
    })
}

pub fn string(tag: &Tag) -> R<&str> {
    match tag {
        Tag::String(s) => Ok(s),
        _ => fail("Not a string"),
    }
}

// Codec.INT: the boxed number's intValue().
pub fn int(tag: &Tag) -> R<i32> {
    tag.box_int().ok_or_else(|| Fail("Not a number".into()))
}

pub fn long(tag: &Tag) -> R<i64> {
    tag.box_long().ok_or_else(|| Fail("Not a number".into()))
}

pub fn float(tag: &Tag) -> R<f32> {
    tag.box_float().ok_or_else(|| Fail("Not a number".into()))
}

pub fn double(tag: &Tag) -> R<f64> {
    tag.box_double().ok_or_else(|| Fail("Not a number".into()))
}

// Codec.BYTE: the boxed number's byteValue().
pub fn byte(tag: &Tag) -> R<i8> {
    tag.box_byte().ok_or_else(|| Fail("Not a number".into()))
}

// Codec.BOOL: DynamicOps.getBooleanValue reads the number's byteValue().
pub fn boolean(tag: &Tag) -> R<bool> {
    Ok(byte(tag)? != 0)
}

// Codec.intRange(min, max).
pub fn int_range(tag: &Tag, min: i32, max: i32) -> R<i32> {
    let v = int(tag)?;
    if v < min || v > max {
        return fail(format!("Value {v} outside of range [{min}:{max}]"));
    }
    Ok(v)
}

// ExtraCodecs.UNSIGNED_BYTE: the byte, as 0 to 255.
pub fn unsigned_byte(tag: &Tag) -> R<i32> {
    Ok(byte(tag)? as u8 as i32)
}

// A tag as a field of a map: absent is None.
pub fn field<'a>(m: &'a Compound, key: &str) -> Option<&'a Tag> {
    m.get(key)
}

// ResourceLocation.tryParse, for the keys saved data names: a namespace
// (default minecraft) and a path of the characters a location may hold.
pub fn resource_location(text: &str) -> R<String> {
    let (namespace, path) = match text.find(':') {
        Some(0) => ("minecraft", &text[1..]),
        Some(at) => (&text[..at], &text[at + 1..]),
        None => ("minecraft", text),
    };
    let namespace_ok = namespace.chars().all(|c| matches!(c, 'a'..='z' | '0'..='9' | '_' | '.' | '-'));
    let path_ok = path.chars().all(|c| matches!(c, 'a'..='z' | '0'..='9' | '_' | '.' | '-' | '/'));
    if !namespace_ok || !path_ok {
        return fail(format!("Not a valid resource location: {text}"));
    }
    Ok(format!("{namespace}:{path}"))
}

// A resource location read from a string tag (ResourceLocation.CODEC).
pub fn location(tag: &Tag) -> R<String> {
    resource_location(string(tag)?)
}

// Codec.string(min, max): the length in UTF-16 units.
pub fn string_length(tag: &Tag, min: usize, max: usize) -> R<String> {
    let s = string(tag)?;
    let length = s.encode_utf16().count();
    if length < min || length > max {
        return fail(format!("String length {length} outside of range [{min}:{max}]"));
    }
    Ok(s.to_string())
}

// A field of a record that is required.
pub fn required<'a>(m: &'a Compound, key: &str) -> R<&'a Tag> {
    m.get(key).ok_or_else(|| Fail(format!("No key {key} in MapLike[{}]", m)))
}

// ValueInput over a compound (TagValueInput): the getters that report a
// problem for a field of the wrong type. A problem is a Fail here, since the
// caller refuses where Minecraft reports one.
pub struct Input<'a>(pub &'a Compound);

impl<'a> Input<'a> {
    pub fn get(&self, key: &str) -> Option<&'a Tag> {
        self.0.get(key)
    }

    // getNumericTag: the field if it is a number, none if absent.
    fn numeric(&self, key: &str) -> R<Option<&'a Tag>> {
        match self.0.get(key) {
            None => Ok(None),
            Some(t) if t.is_numeric() => Ok(Some(t)),
            Some(t) => fail(format!("Expected field '{key}' to contain number, but got {}", t.type_name())),
        }
    }

    pub fn bool_or(&self, key: &str, default: bool) -> R<bool> {
        Ok(self.numeric(key)?.map_or(default, |t| t.byte_value().unwrap() != 0))
    }

    pub fn byte_or(&self, key: &str, default: i8) -> R<i8> {
        Ok(self.numeric(key)?.map_or(default, |t| t.byte_value().unwrap()))
    }

    pub fn short_or(&self, key: &str, default: i16) -> R<i16> {
        Ok(self.numeric(key)?.map_or(default, |t| t.short_value().unwrap()))
    }

    pub fn int_or(&self, key: &str, default: i32) -> R<i32> {
        Ok(self.numeric(key)?.map_or(default, |t| t.int_value().unwrap()))
    }

    pub fn int(&self, key: &str) -> R<Option<i32>> {
        Ok(self.numeric(key)?.map(|t| t.int_value().unwrap()))
    }

    pub fn long_or(&self, key: &str, default: i64) -> R<i64> {
        Ok(self.numeric(key)?.map_or(default, |t| t.long_value().unwrap()))
    }

    pub fn long(&self, key: &str) -> R<Option<i64>> {
        Ok(self.numeric(key)?.map(|t| t.long_value().unwrap()))
    }

    pub fn float_or(&self, key: &str, default: f32) -> R<f32> {
        Ok(self.numeric(key)?.map_or(default, |t| t.float_value().unwrap()))
    }

    pub fn double_or(&self, key: &str, default: f64) -> R<f64> {
        Ok(self.numeric(key)?.map_or(default, |t| t.double_value().unwrap()))
    }

    // getString / getOptionalTypedTag(STRING).
    pub fn string(&self, key: &str) -> R<Option<&'a str>> {
        match self.0.get(key) {
            None => Ok(None),
            Some(Tag::String(s)) => Ok(Some(s)),
            Some(t) => fail(format!("Expected field '{key}' to contain value of type STRING, but got {}", t.type_name())),
        }
    }

    pub fn int_array(&self, key: &str) -> R<Option<&'a Vec<i32>>> {
        match self.0.get(key) {
            None => Ok(None),
            Some(Tag::IntArray(a)) => Ok(Some(a)),
            Some(t) => fail(format!("Expected field '{key}' to contain value of type INT[], but got {}", t.type_name())),
        }
    }

    // listOrEmpty / childrenListOrEmpty: the elements of a list field.
    pub fn list_or_empty(&self, key: &str) -> R<&'a [Tag]> {
        match self.0.get(key) {
            None => Ok(&[]),
            Some(Tag::List(l)) => Ok(l),
            Some(t) => fail(format!("Expected field '{key}' to contain value of type LIST, but got {}", t.type_name())),
        }
    }

    // read(key, codec): the field decoded, none if absent.
    pub fn read<T>(&self, key: &str, decode: impl FnOnce(&'a Tag) -> R<T>) -> R<Option<T>> {
        match self.0.get(key) {
            None => Ok(None),
            Some(tag) => Ok(Some(decode(tag)?)),
        }
    }
}
