//! What today's JavaScript does with the values it reads, where a provider
//! module depends on it: truthiness, `String(value)`, `String.prototype`'s
//! UTF-16 slicing and `trim`, `Date.prototype.toISOString` and
//! `Date.parse` of ISO 8601 text.

use serde_json::Value;

/// JavaScript's truthiness of a JSON value (`undefined` is `None`).
pub fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|x| x != 0.0 && !x.is_nan()),
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(_)) | Some(Value::Object(_)) => true,
    }
}

/// `String(value)` for a JSON value: an array joins its elements' strings
/// with commas (null as empty), an object is `[object Object]`.
pub fn to_string(value: &Value) -> String {
    match value {
        Value::Null => "null".into(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => mcbench_json::number_to_string(n.as_f64().expect("a JSON number")),
        Value::String(s) => s.clone(),
        Value::Array(items) => items
            .iter()
            .map(|item| match item {
                Value::Null => String::new(),
                other => to_string(other),
            })
            .collect::<Vec<_>>()
            .join(","),
        Value::Object(_) => "[object Object]".into(),
    }
}

/// `value?.[key]` on a JSON value.
pub fn get<'a>(value: Option<&'a Value>, key: &str) -> Option<&'a Value> {
    value.and_then(|v| v.as_object()).and_then(|o| o.get(key))
}

/// `a ?? b`: `a` unless it is null or undefined.
pub fn coalesce<'a>(a: Option<&'a Value>, b: Option<&'a Value>) -> Option<&'a Value> {
    match a {
        None | Some(Value::Null) => b,
        some => some,
    }
}

/// `text.slice(0, units)` in UTF-16 code units. JavaScript can cut a
/// surrogate pair there and keep half of it, which a Rust string cannot
/// hold: the half becomes U+FFFD (the one place this differs from Node).
pub fn slice_utf16(text: &str, units: usize) -> String {
    let mut out = String::new();
    let mut count = 0;
    for c in text.chars() {
        let width = c.len_utf16();
        if count + width > units {
            if count < units {
                out.push('\u{FFFD}');
            }
            break;
        }
        out.push(c);
        count += width;
    }
    out
}

/// The length of `text` in UTF-16 code units.
pub fn len_utf16(text: &str) -> usize {
    text.chars().map(char::len_utf16).sum()
}

/// Whether `c` is whitespace or a line terminator to `String.prototype.trim`.
pub fn is_js_whitespace(c: char) -> bool {
    matches!(
        c,
        '\u{9}' | '\u{A}' | '\u{B}' | '\u{C}' | '\u{D}' | ' ' | '\u{A0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200A}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202F}'
                | '\u{205F}'
                | '\u{3000}'
                | '\u{FEFF}'
    )
}

/// `String.prototype.trim`.
pub fn trim(text: &str) -> &str {
    text.trim_matches(is_js_whitespace)
}

/// Days since 1970-01-01 of a proleptic Gregorian date.
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The date of a day count since 1970-01-01.
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// `new Date(ms).toISOString()` for a time from 0 to 9999 years after 1970.
pub fn iso_string(ms: i64) -> String {
    let days = ms.div_euclid(86_400_000);
    let rest = ms.rem_euclid(86_400_000);
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{:03}Z",
        rest / 3_600_000,
        rest / 60_000 % 60,
        rest / 1000 % 60,
        rest % 1000
    )
}

/// Milliseconds since 1970 now (`Date.now()`).
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("the clock is after 1970")
        .as_millis() as i64
}

/// `Date.parse(text)` for the ISO 8601 forms ECMAScript defines
/// (`YYYY-MM-DD`, `YYYY-MM-DDTHH:mm`, `:ss`, `.sss` with any number of
/// fraction digits as V8 reads them, and `Z` or `±HH:mm`). A date alone and a
/// time with a zone are UTC; a time without a zone is local time to
/// JavaScript, and every other text goes to V8's legacy parser: both are
/// `None` here, which callers treat as an unparsable date (NaN). The
/// providers send `created_at` and the Codex CLI `last_refresh` with `Z`.
pub fn parse_iso(text: &str) -> Option<i64> {
    let b = text.as_bytes();
    let digits = |from: usize, n: usize| -> Option<i64> {
        let part = b.get(from..from + n)?;
        part.iter().all(u8::is_ascii_digit).then(|| {
            part.iter()
                .fold(0i64, |acc, d| acc * 10 + i64::from(d - b'0'))
        })
    };
    let year = digits(0, 4)?;
    if b.get(4) != Some(&b'-') {
        return None;
    }
    let month = digits(5, 2)?;
    if b.get(7) != Some(&b'-') {
        return None;
    }
    let day = digits(8, 2)?;
    if !(1..=12).contains(&month) || day < 1 || day > days_in_month(year, month) {
        return None;
    }
    let date = days_from_civil(year, month, day) * 86_400_000;
    if b.len() == 10 {
        return Some(date);
    }
    if b.get(10) != Some(&b'T') {
        return None;
    }
    let hour = digits(11, 2)?;
    if b.get(13) != Some(&b':') {
        return None;
    }
    let minute = digits(14, 2)?;
    let mut at = 16;
    let mut second = 0;
    let mut millis = 0;
    if b.get(at) == Some(&b':') {
        second = digits(at + 1, 2)?;
        at += 3;
        if b.get(at) == Some(&b'.') {
            let start = at + 1;
            let mut end = start;
            while b.get(end).is_some_and(u8::is_ascii_digit) {
                end += 1;
            }
            if end == start {
                return None;
            }
            let fraction = &text[start..end];
            millis = format!("{fraction:0<3}")[..3].parse::<i64>().ok()?;
            at = end;
        }
    }
    if hour > 24
        || minute > 59
        || second > 59
        || (hour == 24 && (minute, second, millis) != (0, 0, 0))
    {
        return None;
    }
    let offset = match b.get(at) {
        Some(b'Z') if b.len() == at + 1 => 0,
        Some(sign @ (b'+' | b'-')) if b.len() == at + 6 && b[at + 3] == b':' => {
            let h = digits(at + 1, 2)?;
            let m = digits(at + 4, 2)?;
            if h > 23 || m > 59 {
                return None;
            }
            let minutes = h * 60 + m;
            if *sign == b'+' { minutes } else { -minutes }
        }
        _ => return None,
    };
    Some(date + ((hour * 60 + minute - offset) * 60 + second) * 1000 + millis)
}

fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        2 if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_strings_round_trip() {
        assert_eq!(iso_string(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(iso_string(1_763_337_600_123), "2025-11-17T00:00:00.123Z");
        assert_eq!(
            parse_iso("2025-11-17T00:00:00.123Z"),
            Some(1_763_337_600_123)
        );
        assert_eq!(
            parse_iso("2025-11-17T00:00:00.123456789Z"),
            Some(1_763_337_600_123)
        );
        assert_eq!(parse_iso("2026-09-01T00:00:00Z"), Some(1_788_220_800_000));
        assert_eq!(
            parse_iso("2026-09-01T02:00:00+02:00"),
            Some(1_788_220_800_000)
        );
        assert_eq!(parse_iso("2026-09-01"), Some(1_788_220_800_000));
        assert_eq!(parse_iso("not a date"), None);
        assert_eq!(parse_iso("2026-02-30"), None);
    }

    #[test]
    fn utf16_slices_and_trims() {
        assert_eq!(slice_utf16("ab🧱c", 3), "ab\u{FFFD}");
        assert_eq!(slice_utf16("ab🧱c", 4), "ab🧱");
        assert_eq!(trim("\u{FEFF} x \u{3000}"), "x");
        assert_eq!(
            to_string(&serde_json::json!([1, null, "a", [2, 3]])),
            "1,,a,2,3"
        );
    }
}
