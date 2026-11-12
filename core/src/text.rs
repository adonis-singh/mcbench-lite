// ComponentSerialization.CODEC over NbtOps: a text component read from the tag
// a saved block entity or item holds it as, and written again as its codec
// writes it. A component's value is kept as that written tag (like every
// data component here): decoding checks and normalizes it, and two components
// are equal as Component.equals has them when their written tags are.
//
// What is modeled: the plain, translatable, keybind, score and selector
// contents, the siblings, the style (color, shadow color, the five flags,
// insertion, font) and the click and hover events whose decoding does not
// depend on anything but the tag. What is not (an nbt or object content, a
// click event that opens a URL, a hover event on an entity this registry
// lacks) is a Fail: the caller refuses. A tag Minecraft's codec rejects is a
// Fail too, since the caller does not write DFU's error messages.
//
// Component.equals could tell apart two components that write the same tag
// in three ways, which are refused: a hex color of a named color's value
// (TextColor.equals compares the value, the name is what is written), and a
// translatable argument that is a component writing as a plain string (an
// argument that is a string is another object).

use crate::codec::{self, fail, items, location, map, string, Fail, R};
use crate::nbt::{Compound, Tag};
use crate::registry::Registry;

// The deepest nesting of components decoded; deeper is a Fail.
const MAX_DEPTH: usize = 64;

// Where the decoding of one component goes: the registry, and whether any
// part of it needs resolving (a score or a selector).
struct Ctx<'a> {
    reg: &'a Registry,
    resolves: std::cell::Cell<bool>,
}

// ComponentSerialization.CODEC, decoded and written again.
pub fn component(reg: &Registry, value: &Tag) -> R<Tag> {
    let ctx = Ctx { reg, resolves: std::cell::Cell::new(false) };
    Ok(encode(&decode(&ctx, value, 0)?))
}

// A line of a sign, which SignBlockEntity.loadLine passes through
// ComponentUtils.updateForEntity as the block entity loads: a component with
// a score or a selector is resolved (to what the scoreboard and the entities
// give, which this port does not read), the others are written as they were.
pub fn sign_line(reg: &Registry, value: &Tag) -> R<Tag> {
    let ctx = Ctx { reg, resolves: std::cell::Cell::new(false) };
    let written = encode(&decode(&ctx, value, 0)?);
    if ctx.resolves.get() {
        return fail("a sign line with a score or a selector component (resolved as the sign loads)");
    }
    Ok(written)
}

// A decoded component: the literal text when its contents are PlainTextContents,
// the fields its contents write, the style's fields, and the siblings.
struct Comp {
    literal: Option<String>,
    contents: Vec<(&'static str, Tag)>,
    style: Vec<(&'static str, Tag)>,
    extra: Vec<Comp>,
}

// The Either<Either<String, List<Component>>, Component> of createCodec.
fn decode(ctx: &Ctx, value: &Tag, depth: usize) -> R<Comp> {
    if depth > MAX_DEPTH {
        return fail("a text component nested deeper than this port reads");
    }
    match value {
        Tag::String(text) => Ok(Comp { literal: Some(text.clone()), contents: vec![("text", value.clone())], style: Vec::new(), extra: Vec::new() }),
        Tag::List(_) | Tag::ByteArray(_) | Tag::IntArray(_) | Tag::LongArray(_) => {
            // ExtraCodecs.nonEmptyList(codec.listOf()), then createFromList:
            // the first component, the others appended to its siblings.
            let elements = items(value)?;
            if elements.is_empty() {
                return fail("List must have contents");
            }
            let mut parts = elements.iter().map(|element| decode(ctx, element, depth + 1)).collect::<R<Vec<Comp>>>()?.into_iter();
            let mut first = parts.next().unwrap();
            first.extra.extend(parts);
            Ok(first)
        }
        Tag::Compound(m) => decode_map(ctx, m, depth),
        _ => fail("Not a map"),
    }
}

// The record codec: the contents (the legacy matcher), the extra field, the style.
fn decode_map(ctx: &Ctx, m: &Compound, depth: usize) -> R<Comp> {
    let (literal, contents) = decode_contents(ctx, m, depth)?;
    let extra = match m.get("extra") {
        None => Vec::new(),
        Some(tag) => {
            let elements = items(tag)?;
            if elements.is_empty() {
                return fail("List must have contents");
            }
            elements.iter().map(|element| decode(ctx, element, depth + 1)).collect::<R<Vec<Comp>>>()?
        }
    };
    let style = decode_style(ctx, m, depth)?;
    Ok(Comp { literal, contents, style, extra })
}

fn encode(comp: &Comp) -> Tag {
    // Component.tryCollapseToString: plain text with no siblings and no style.
    if let Some(text) = &comp.literal {
        if comp.style.is_empty() && comp.extra.is_empty() {
            return Tag::String(text.clone());
        }
    }
    let mut out = Compound::new();
    for (key, value) in comp.contents.iter().chain(comp.style.iter()) {
        out.put(key, value.clone());
    }
    if !comp.extra.is_empty() {
        out.put("extra", Tag::List(comp.extra.iter().map(encode).collect()));
    }
    Tag::Compound(out)
}

const CONTENTS: [&str; 7] = ["text", "translatable", "keybind", "score", "selector", "nbt", "object"];

type Contents = (Option<String>, Vec<(&'static str, Tag)>);

// StrictEither: a map with a type field is read by the codec of that type; one
// without is read by the first contents codec that reads it (FuzzyCodec).
fn decode_contents(ctx: &Ctx, m: &Compound, depth: usize) -> R<Contents> {
    if let Some(kind) = m.get("type") {
        let kind = string(kind)?;
        return match CONTENTS.iter().find(|&&name| name == kind) {
            Some(&name) => contents_of(ctx, name, m, depth)?.ok_or_else(|| Fail(format!("no usable {name} contents"))),
            None => fail(format!("Unknown element id: {kind}")),
        };
    }
    for name in CONTENTS {
        if let Some(contents) = contents_of(ctx, name, m, depth)? {
            return Ok(contents);
        }
    }
    fail("No matching codec found")
}

// The contents codec of one type read from the map: None where it does not read it.
// The nbt and object contents are not modeled, and a map they could read
// that no earlier codec reads is a Fail.
fn contents_of(ctx: &Ctx, name: &str, m: &Compound, depth: usize) -> R<Option<Contents>> {
    Ok(match name {
        "text" => match m.get("text") {
            Some(Tag::String(text)) => Some((Some(text.clone()), vec![("text", Tag::String(text.clone()))])),
            _ => None,
        },
        "translatable" => translatable(ctx, m, depth)?,
        "keybind" => match m.get("keybind") {
            Some(Tag::String(key)) => Some((None, vec![("keybind", Tag::String(key.clone()))])),
            _ => None,
        },
        "score" => match m.get("score") {
            Some(Tag::Compound(score)) => match (score.get("name"), score.get("objective")) {
                (Some(Tag::String(name)), Some(Tag::String(objective))) => Some((None, {
                    ctx.resolves.set(true);
                    vec![("score", Tag::compound(vec![("name", Tag::String(name.clone())), ("objective", Tag::String(objective.clone()))]))]
                })),
                _ => None,
            },
            _ => None,
        },
        "selector" => match m.get("selector") {
            Some(Tag::String(selector)) => {
                if !selector_parses(selector) {
                    return if selector_unknown(selector) { fail("a selector this port does not parse") } else { Ok(None) };
                }
                ctx.resolves.set(true);
                let mut fields = vec![("selector", Tag::String(selector.clone()))];
                if let Some(separator) = m.get("separator") {
                    fields.push(("separator", encode(&decode(ctx, separator, depth + 1)?)));
                }
                Some((None, fields))
            }
            _ => None,
        },
        "nbt" | "object" => {
            if ["nbt", "object", "sprite", "player"].iter().any(|key| m.contains(key)) {
                return fail("an nbt or object text component");
            }
            None
        }
        other => unreachable!("no contents {other}"),
    })
}

// SelectorParser.parse for the selectors this port knows it reads: a target
// selector without arguments, or a player name of letters, digits and the
// characters _ . - up to 16.
fn selector_parses(selector: &str) -> bool {
    let bytes = selector.as_bytes();
    if bytes.len() == 2 && bytes[0] == b'@' {
        return matches!(bytes[1], b'a' | b'e' | b'p' | b'r' | b's' | b'n');
    }
    (1..=16).contains(&bytes.len()) && bytes.iter().all(|&b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-'))
}

// Whether the parser might still read a selector this port does not: anything
// that is not clearly rejected (an empty one, or a character no selector holds).
fn selector_unknown(selector: &str) -> bool {
    !selector.is_empty()
}

// TranslatableContents.MAP_CODEC: the key, a fallback that is none if it is
// no string, and the arguments (strings, numbers and components).
fn translatable(ctx: &Ctx, m: &Compound, depth: usize) -> R<Option<Contents>> {
    let Some(Tag::String(key)) = m.get("translate") else { return Ok(None) };
    let mut fields = vec![("translate", Tag::String(key.clone()))];
    if let Some(Tag::String(fallback)) = m.get("fallback") {
        fields.push(("fallback", Tag::String(fallback.clone())));
    }
    if let Some(with) = m.get("with") {
        // (A field that is no list makes the contents fail to read.)
        let Ok(elements) = items(with) else { return Ok(None) };
        let mut args = Vec::new();
        for element in &elements {
            match element {
                Tag::Byte(_) | Tag::Short(_) | Tag::Int(_) | Tag::Long(_) | Tag::String(_) => args.push(element.clone()),
                Tag::Float(v) if v.is_finite() => args.push(element.clone()),
                Tag::Double(v) if v.is_finite() => args.push(element.clone()),
                Tag::Float(_) | Tag::Double(_) => return fail("a translatable argument that is not a finite number"),
                Tag::List(_) | Tag::Compound(_) => {
                    let written = encode(&decode(ctx, element, depth + 1)?);
                    if matches!(written, Tag::String(_)) {
                        return fail("a translatable argument that is a component writing as a string");
                    }
                    args.push(written);
                }
                _ => return Ok(None),
            }
        }
        if !args.is_empty() {
            fields.push(("with", Tag::List(args)));
        }
    }
    Ok(Some((None, fields)))
}

// Style.Serializer.MAP_CODEC. Every field is optional and, if present, must
// read (a value that does not is an error).
fn decode_style(ctx: &Ctx, m: &Compound, depth: usize) -> R<Vec<(&'static str, Tag)>> {
    let mut style = Vec::new();
    if let Some(color) = m.get("color") {
        style.push(("color", Tag::String(text_color(string(color)?)?)));
    }
    if let Some(color) = m.get("shadow_color") {
        style.push(("shadow_color", Tag::Int(argb_color(color)?)));
    }
    for flag in ["bold", "italic", "underlined", "strikethrough", "obfuscated"] {
        if let Some(tag) = m.get(flag) {
            style.push((flag, Tag::Byte(codec::boolean(tag)? as i8)));
        }
    }
    if let Some(event) = m.get("click_event") {
        style.push(("click_event", click_event(map(event)?)?));
    }
    if let Some(event) = m.get("hover_event") {
        style.push(("hover_event", hover_event(ctx, map(event)?, depth)?));
    }
    if let Some(insertion) = m.get("insertion") {
        style.push(("insertion", Tag::String(string(insertion)?.to_string())));
    }
    if let Some(font) = m.get("font") {
        style.push(("font", Tag::String(location(font)?)));
    }
    Ok(style)
}

const NAMED_COLORS: [(&str, i32); 16] = [
    ("black", 0x000000),
    ("dark_blue", 0x0000AA),
    ("dark_green", 0x00AA00),
    ("dark_aqua", 0x00AAAA),
    ("dark_red", 0xAA0000),
    ("dark_purple", 0xAA00AA),
    ("gold", 0xFFAA00),
    ("gray", 0xAAAAAA),
    ("dark_gray", 0x555555),
    ("blue", 0x5555FF),
    ("green", 0x55FF55),
    ("aqua", 0x55FFFF),
    ("red", 0xFF5555),
    ("light_purple", 0xFF55FF),
    ("yellow", 0xFFFF55),
    ("white", 0xFFFFFF),
];

// TextColor.CODEC: a color name, or # and up to six hex digits (written with
// six, the digits upper case).
fn text_color(text: &str) -> R<String> {
    if let Some(hex) = text.strip_prefix('#') {
        if hex.is_empty() || hex.len() > 8 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return fail(format!("Invalid color value: {text}"));
        }
        let value = u64::from_str_radix(hex, 16).unwrap();
        if value > 0xFFFFFF {
            return fail(format!("Color value out of range: {text}"));
        }
        if NAMED_COLORS.iter().any(|&(_, named)| named as u64 == value) {
            return fail("a hex color of a named color's value (TextColor.equals compares the value)");
        }
        return Ok(format!("#{value:06X}"));
    }
    match NAMED_COLORS.iter().find(|&&(name, _)| name == text) {
        Some(&(name, _)) => Ok(name.to_string()),
        None => fail(format!("Invalid color name: {text}")),
    }
}

// ExtraCodecs.ARGB_COLOR_CODEC: an int, or a list of four floats (red, green,
// blue, alpha) that ARGB.colorFromFloat packs.
fn argb_color(tag: &Tag) -> R<i32> {
    if let Ok(color) = codec::int(tag) {
        return Ok(color);
    }
    let elements = items(tag)?;
    if elements.len() != 4 {
        return fail("Input is not a list of 4 elements");
    }
    let floats = elements.iter().map(codec::float).collect::<R<Vec<f32>>>()?;
    Ok(color_from_float(floats[3], floats[0], floats[1], floats[2]))
}

// ARGB.colorFromFloat(alpha, red, green, blue): each channel is Mth.floor(f * 255)
// masked to a byte.
pub fn color_from_float(alpha: f32, red: f32, green: f32, blue: f32) -> i32 {
    let channel = |value: f32| crate::mth::floor_float(value * 255.0f32);
    (channel(alpha) & 255) << 24 | (channel(red) & 255) << 16 | (channel(green) & 255) << 8 | (channel(blue) & 255)
}

// StringUtil.isAllowedChatCharacter for each character of ExtraCodecs.CHAT_STRING.
fn chat_string(tag: &Tag) -> R<String> {
    let text = string(tag)?;
    for unit in text.encode_utf16() {
        if unit == 167 || unit < 32 || unit == 127 {
            return fail("Disallowed chat character");
        }
    }
    Ok(text.to_string())
}

// ClickEvent.CODEC: the action selects the codec of the rest.
fn click_event(m: &Compound) -> R<Tag> {
    let action = string(codec::required(m, "action")?)?;
    let mut out = Compound::new();
    match action {
        "run_command" | "suggest_command" => out.put("command", Tag::String(chat_string(codec::required(m, "command")?)?)),
        "change_page" => {
            let page = codec::int(codec::required(m, "page")?)?;
            if page < 1 {
                return fail(format!("Value must be positive: {page}"));
            }
            out.put("page", Tag::Int(page));
        }
        "copy_to_clipboard" => out.put("value", Tag::String(string(codec::required(m, "value")?)?.to_string())),
        "open_url" => return fail("a click event that opens a URL (the codec keeps the URIs it has read)"),
        other => return fail(format!("the click event {other}")),
    }
    out.put("action", Tag::string(action));
    Ok(Tag::Compound(out))
}

// HoverEvent.CODEC.
fn hover_event(ctx: &Ctx, m: &Compound, depth: usize) -> R<Tag> {
    let action = string(codec::required(m, "action")?)?;
    let mut out = Compound::new();
    match action {
        "show_text" => out.put("value", encode(&decode(ctx, codec::required(m, "value")?, depth + 1)?)),
        "show_item" => {
            // ItemStack.MAP_CODEC beside the action.
            let stack = crate::components::decode_stack(ctx.reg, &Tag::Compound(m.clone()))?;
            for (key, value) in crate::components::stack_fields(ctx.reg, &stack)?.0 {
                out.put(&key, value);
            }
        }
        "show_entity" => {
            let id = location(codec::required(m, "id")?)?;
            if !ctx.reg.entity_types.contains_key(&id) {
                return fail(format!("Unknown registry key in ResourceKey[minecraft:root / minecraft:entity_type]: {id}"));
            }
            out.put("id", Tag::String(id));
            out.put("uuid", Tag::IntArray(uuid(codec::required(m, "uuid")?)?));
            if let Some(name) = m.get("name") {
                out.put("name", encode(&decode(ctx, name, depth + 1)?));
            }
        }
        other => return fail(format!("Unknown element name:{other}")),
    }
    out.put("action", Tag::string(action));
    Ok(Tag::Compound(out))
}

// UUIDUtil.CODEC: four ints (an int stream), or the text UUID.fromString reads.
fn uuid(tag: &Tag) -> R<Vec<i32>> {
    if let Ok(elements) = items(tag) {
        let ints = elements.iter().map(codec::int).collect::<R<Vec<i32>>>()?;
        if ints.len() != 4 {
            return fail("Input is not a list of 4 elements");
        }
        return Ok(ints);
    }
    let text = string(tag)?;
    // UUID.fromString: five hex numbers between four dashes (each up to
    // sixteen digits here; Long.parseLong also takes a sign).
    let parts: Vec<&str> = text.split('-').collect();
    if text.len() > 36 || parts.len() != 5 || parts.iter().any(|part| part.is_empty() || part.len() > 16 || !part.bytes().all(|b| b.is_ascii_hexdigit())) {
        return fail(format!("Invalid UUID {text}"));
    }
    let hex = |part: &str| u64::from_str_radix(part, 16).unwrap();
    let most = ((hex(parts[0]) & 0xFFFF_FFFF) << 32) | ((hex(parts[1]) & 0xFFFF) << 16) | (hex(parts[2]) & 0xFFFF);
    let least = ((hex(parts[3]) & 0xFFFF) << 48) | (hex(parts[4]) & 0xFFFF_FFFF_FFFF);
    Ok(vec![(most >> 32) as i32, most as i32, (least >> 32) as i32, least as i32])
}

// The length of a component's JSON (JsonOps, what GsonHelper.encodesLongerThan
// counts): exact for a plain string, else an upper bound.
pub enum JsonLength {
    Exact(usize),
    AtMost(usize),
}

pub fn json_length(component: &Tag) -> JsonLength {
    match component {
        Tag::String(text) => JsonLength::Exact(2 + escaped_length(text)),
        other => JsonLength::AtMost(bound(other)),
    }
}

// Gson's escapes: a quote, a backslash and the common control characters two
// characters, the other control characters and the line and paragraph
// separators six.
fn escaped_length(text: &str) -> usize {
    text.encode_utf16()
        .map(|unit| match unit {
            0x22 | 0x5C | 0x08 | 0x0C | 0x0A | 0x0D | 0x09 => 2,
            0..=0x1F | 0x2028 | 0x2029 => 6,
            _ => 1,
        })
        .sum()
}

// A bound on the JSON of a tag: strings at their escaped length, a number
// within 32 characters.
fn bound(tag: &Tag) -> usize {
    match tag {
        Tag::String(text) => 2 + escaped_length(text),
        Tag::List(list) => 2 + list.iter().map(|t| bound(t) + 1).sum::<usize>(),
        Tag::IntArray(list) => 2 + list.len() * 33,
        Tag::ByteArray(list) => 2 + list.len() * 33,
        Tag::LongArray(list) => 2 + list.len() * 33,
        Tag::Compound(c) => 2 + c.0.iter().map(|(key, value)| 3 + escaped_length(key) + bound(value) + 1).sum::<usize>(),
        _ => 32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_and_hex_colors() {
        assert_eq!(text_color("red").unwrap(), "red");
        assert_eq!(text_color("#12345").unwrap(), "#012345");
        assert!(text_color("#FF5555").is_err());
        assert!(text_color("#1234567").is_err());
        assert!(text_color("RED").is_err());
    }

    #[test]
    fn packed_floats() {
        let list = |v: [f32; 4]| Tag::List(v.iter().map(|&f| Tag::Float(f)).collect());
        assert_eq!(argb_color(&list([1.0, 0.5, 0.0, 1.0])).unwrap(), -33024);
        assert_eq!(argb_color(&list([1.0, 2.0, 3.0, 4.0])).unwrap(), -50331907);
    }
}
