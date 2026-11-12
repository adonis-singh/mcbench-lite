// A chunk as Mojang's SerializableChunkData saves it (copyOf(level, chunk).write()
// of a LevelChunk in McbenchLevel), written as the SNBT its CompoundTag
// prints: the sections' block states and biomes as PalettedContainer packs
// them, Starlight's light, the heightmaps, block entities and scheduled
// ticks.

// SharedConstants.getCurrentVersion().dataVersion().version().
pub const DATA_VERSION: i64 = 4556;

use crate::dir::Pos;
use crate::level::Level;
use crate::light::dims::{MAX_LIGHT, MIN_LIGHT, MIN_SECTION, SECTIONS};
use crate::registry::State;
use crate::world::Section;

// An NBT tag, written the way StringTagVisitor writes it.
pub enum Tag {
    Byte(i8),
    Int(i32),
    Long(i64),
    Str(String),
    List(Vec<Tag>),
    Compound(Vec<(String, Tag)>),
    ByteArray(Vec<u8>),
    LongArray(Vec<u64>),
    // A tag already written as SNBT.
    Raw(String),
}

impl Tag {
    pub fn snbt(&self) -> String {
        let mut out = String::new();
        self.write(&mut out);
        out
    }

    // The tag's id in the binary format.
    fn id(&self) -> u8 {
        match self {
            Tag::Byte(_) => 1,
            Tag::Int(_) => 3,
            Tag::Long(_) => 4,
            Tag::Str(_) => 8,
            Tag::List(_) => 9,
            Tag::Compound(_) => 10,
            Tag::ByteArray(_) => 7,
            Tag::LongArray(_) => 12,
            Tag::Raw(text) => crate::nbt::parse(text).unwrap_or_else(|_| panic!("saved data this port wrote is not SNBT: {text}")).id(),
        }
    }

    // The payload as NbtIo writes it: what a region file's chunk holds, with
    // the saved data of block entities (SNBT text above) read back into tags.
    fn write_binary(&self, out: &mut Vec<u8>) {
        match self {
            Tag::Byte(v) => out.push(*v as u8),
            Tag::Int(v) => out.extend_from_slice(&v.to_be_bytes()),
            Tag::Long(v) => out.extend_from_slice(&v.to_be_bytes()),
            Tag::Str(v) => crate::nbt::write_utf(v, out),
            Tag::List(items) => {
                out.push(items.first().map_or(0, Tag::id));
                out.extend_from_slice(&(items.len() as i32).to_be_bytes());
                for item in items {
                    item.write_binary(out);
                }
            }
            Tag::Compound(entries) => {
                let mut sorted: Vec<&(String, Tag)> = entries.iter().collect();
                sorted.sort_by(|a, b| a.0.encode_utf16().cmp(b.0.encode_utf16()));
                for (key, value) in sorted {
                    out.push(value.id());
                    crate::nbt::write_utf(key, out);
                    value.write_binary(out);
                }
                out.push(0);
            }
            Tag::ByteArray(bytes) => {
                out.extend_from_slice(&(bytes.len() as i32).to_be_bytes());
                out.extend_from_slice(bytes);
            }
            Tag::LongArray(longs) => {
                out.extend_from_slice(&(longs.len() as i32).to_be_bytes());
                for long in longs {
                    out.extend_from_slice(&long.to_be_bytes());
                }
            }
            Tag::Raw(text) => crate::nbt::parse(text).unwrap_or_else(|_| panic!("saved data this port wrote is not SNBT: {text}")).write_binary(out),
        }
    }

    fn write(&self, out: &mut String) {
        use std::fmt::Write;
        match self {
            Tag::Byte(v) => write!(out, "{v}b").unwrap(),
            Tag::Int(v) => write!(out, "{v}").unwrap(),
            Tag::Long(v) => write!(out, "{v}L").unwrap(),
            Tag::Str(v) => quote_and_escape(v, out),
            Tag::List(items) => {
                out.push('[');
                for (i, item) in items.iter().enumerate() {
                    if i != 0 {
                        out.push(',');
                    }
                    item.write(out);
                }
                out.push(']');
            }
            Tag::Compound(entries) => {
                let mut sorted: Vec<&(String, Tag)> = entries.iter().collect();
                sorted.sort_by(|a, b| a.0.encode_utf16().cmp(b.0.encode_utf16()));
                out.push('{');
                for (i, (key, value)) in sorted.iter().map(|e| (&e.0, &e.1)).enumerate() {
                    if i != 0 {
                        out.push(',');
                    }
                    handle_key_escape(key, out);
                    out.push(':');
                    value.write(out);
                }
                out.push('}');
            }
            Tag::Raw(text) => out.push_str(text),
            Tag::ByteArray(bytes) => {
                out.push_str("[B;");
                for (i, &b) in bytes.iter().enumerate() {
                    if i != 0 {
                        out.push(',');
                    }
                    write!(out, "{}B", b as i8).unwrap();
                }
                out.push(']');
            }
            Tag::LongArray(longs) => {
                out.push_str("[L;");
                for (i, &l) in longs.iter().enumerate() {
                    if i != 0 {
                        out.push(',');
                    }
                    write!(out, "{}L", l as i64).unwrap();
                }
                out.push(']');
            }
        }
    }
}

// StringTagVisitor.handleKeyEscape: UNQUOTED_KEY_MATCH is [A-Za-z._]+[A-Za-z0-9._+-]*.
fn handle_key_escape(key: &str, out: &mut String) {
    let mut chars = key.chars();
    let first_ok = chars.next().map_or(false, |c| c.is_ascii_alphabetic() || c == '.' || c == '_');
    let rest_ok = key.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '+' | '-'));
    if first_ok && rest_ok && !key.eq_ignore_ascii_case("true") && !key.eq_ignore_ascii_case("false") {
        out.push_str(key);
    } else {
        quote_and_escape(key, out);
    }
}

// StringTag.quoteAndEscape.
fn quote_and_escape(text: &str, out: &mut String) {
    let start = out.len();
    out.push(' ');
    let mut quote = '\0';
    for c in text.chars() {
        if c == '\\' {
            out.push_str("\\\\");
        } else if c != '"' && c != '\'' {
            match c {
                '\u{8}' => out.push_str("\\b"),
                '\u{c}' => out.push_str("\\f"),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                _ => out.push(c),
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

// The top-level entries of a compound's SNBT, as (key, value text).
fn split_compound(snbt: &str) -> Vec<(String, String)> {
    let bytes = snbt.as_bytes();
    assert!(bytes[0] == b'{' && bytes[bytes.len() - 1] == b'}', "not a compound: {snbt}");
    let inner = &snbt[1..snbt.len() - 1];
    let mut entries = Vec::new();
    if inner.is_empty() {
        return entries;
    }
    let (mut depth, mut quote, mut escaped, mut start) = (0i32, None::<char>, false, 0usize);
    let mut parts = Vec::new();
    for (i, c) in inner.char_indices() {
        if let Some(q) = quote {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            '"' | '\'' => quote = Some(c),
            '{' | '[' => depth += 1,
            '}' | ']' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(&inner[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&inner[start..]);
    for part in parts {
        let colon = part.find(':').expect("a key and a value");
        let key = &part[..colon];
        assert!(!key.contains('"') && !key.contains('\''), "a quoted key in {snbt}");
        entries.push((key.to_string(), part[colon + 1..].to_string()));
    }
    entries
}

// Mth.ceillog2.
fn ceil_log2(value: usize) -> u32 {
    if value <= 1 {
        0
    } else {
        usize::BITS - (value - 1).leading_zeros()
    }
}

// PalettedContainer.pack of a section's block states, as the codec writes it
// ({palette, data}).
fn block_states(level: &Level, section: Option<&Section>) -> Tag {
    let mut palette: Vec<State> = Vec::new();
    let mut ints = [0u32; 4096];
    let mut last: Option<(State, u32)> = None;
    for (i, slot) in ints.iter_mut().enumerate() {
        let state = section.map_or(0, |s| s.blocks[i]);
        let id = match last {
            Some((s, id)) if s == state => id,
            _ => {
                let id = palette.iter().position(|&p| p == state).unwrap_or_else(|| {
                    palette.push(state);
                    palette.len() - 1
                }) as u32;
                last = Some((state, id));
                id
            }
        };
        *slot = id;
    }
    let bits = ceil_log2(palette.len());
    // Strategy.createForBlockStates: 1 to 4 bits are four, the rest as they are.
    let storage_bits = match bits {
        0 => 0,
        1..=4 => 4,
        n => n,
    };
    let mut entries = vec![(
        "palette".to_string(),
        Tag::List(
            palette
                .iter()
                .map(|&state| {
                    let mut entry = vec![("Name".to_string(), Tag::Str(level.reg.blocks[level.block(state) as usize].key.clone()))];
                    let values = level.reg.values(state);
                    if !values.is_empty() {
                        entry.push(("Properties".to_string(), Tag::Compound(values.into_iter().map(|(k, v)| (k.to_string(), Tag::Str(v.to_string()))).collect())));
                    }
                    Tag::Compound(entry)
                })
                .collect(),
        ),
    )];
    if storage_bits != 0 {
        entries.push(("data".to_string(), Tag::LongArray(pack_bits(&ints, storage_bits))));
    }
    Tag::Compound(entries)
}

// new SimpleBitStorage(bits, ints.length, ints).getRaw().
fn pack_bits(ints: &[u32], bits: u32) -> Vec<u64> {
    let per_long = (64 / bits) as usize;
    let mask = (1u64 << bits) - 1;
    ints.chunks(per_long).map(|group| group.iter().enumerate().fold(0u64, |long, (i, &v)| long | (v as u64 & mask) << (i as u32 * bits))).collect()
}

impl Level {
    // SerializableChunkData.copyOf(level, chunk).write().toString() of the
    // chunk at x, z, once the light engine has lit its changes.
    pub fn chunk_snbt(&mut self, x: i32, z: i32) -> String {
        self.chunk_tag(x, z).snbt()
    }

    // The same chunk as a region file stores it: the root compound in
    // NbtIo's binary format, nameless (NbtIo.write).
    pub fn chunk_nbt(&mut self, x: i32, z: i32) -> Vec<u8> {
        let tag = self.chunk_tag(x, z);
        let mut out = vec![10, 0, 0];
        tag.write_binary(&mut out);
        out
    }

    // Every chunk that holds a block, as a save's regions would: the game
    // time (i64), the number of chunks (i32), then each chunk's x and z (i32)
    // and the length (i32) and bytes of chunk_nbt. Big endian.
    pub fn export_chunks(&mut self) -> Vec<u8> {
        let mut keys: Vec<i64> = self.world.chunk_keys().collect();
        keys.sort();
        let mut chunks = Vec::new();
        for key in keys {
            let chunk = self.world.chunk(key);
            if chunk.is_empty() {
                continue;
            }
            chunks.push((chunk.x, chunk.z));
        }
        let mut out = Vec::new();
        out.extend_from_slice(&self.game_time.to_be_bytes());
        out.extend_from_slice(&(chunks.len() as i32).to_be_bytes());
        for (x, z) in chunks {
            let nbt = self.chunk_nbt(x, z);
            out.extend_from_slice(&x.to_be_bytes());
            out.extend_from_slice(&z.to_be_bytes());
            out.extend_from_slice(&(nbt.len() as i32).to_be_bytes());
            out.extend_from_slice(&nbt);
        }
        out
    }

    fn chunk_tag(&mut self, x: i32, z: i32) -> Tag {
        self.chunk(x, z);
        self.flush_light();
        let key = crate::dir::chunk_key(x, z);
        let light = self.light.chunk_light(key);
        let mut sections = Vec::new();
        for light_section in MIN_LIGHT..=MAX_LIGHT {
            let light_index = (light_section - MIN_LIGHT) as usize;
            let block_index = light_section - MIN_SECTION;
            let in_world = block_index >= 0 && (block_index as usize) < SECTIONS;
            let block_nibble = light.layer(false)[light_index].save_state();
            let sky_nibble = light.layer(true)[light_index].save_state();
            if !in_world && block_nibble.is_none() && sky_nibble.is_none() {
                continue;
            }
            let mut section = Vec::new();
            if in_world {
                let world_section = self.world.chunk(key).sections[block_index as usize].as_ref();
                section.push(("block_states".to_string(), block_states(self, world_section)));
                // The biomes: a section made empty is all plains.
                section.push((
                    "biomes".to_string(),
                    Tag::Compound(vec![("palette".to_string(), Tag::List(vec![Tag::Str("minecraft:plains".to_string())]))]),
                ));
            }
            if let Some(save) = &block_nibble {
                if let Some(data) = &save.data {
                    section.push(("BlockLight".to_string(), Tag::ByteArray(data.to_vec())));
                }
                if save.state > 0 {
                    section.push(("starlight.blocklight_state".to_string(), Tag::Int(save.state as i32)));
                }
            }
            if let Some(save) = &sky_nibble {
                if let Some(data) = &save.data {
                    section.push(("SkyLight".to_string(), Tag::ByteArray(data.to_vec())));
                }
                if save.state > 0 {
                    section.push(("starlight.skylight_state".to_string(), Tag::Int(save.state as i32)));
                }
            }
            if !section.is_empty() {
                section.push(("Y".to_string(), Tag::Byte(light_section as i8)));
                sections.push(Tag::Compound(section));
            }
        }
        let light_correct = light.light_correct;

        // Block entities, in the order the chunk's HashSet of their positions
        // iterates, each as saveWithFullMetadata with keepPacked.
        let mut block_entities = Vec::new();
        for pos in self.block_entity_order(key) {
            let (kind, data) = self.block_entity_saved(pos);
            let mut entries: Vec<(String, Tag)> = split_compound(&data).into_iter().map(|(k, v)| (k, Tag::Raw(v))).collect();
            for (name, value) in [("id", Tag::Str(kind.to_string())), ("x", Tag::Int(pos.x())), ("y", Tag::Int(pos.y())), ("z", Tag::Int(pos.z())), ("keepPacked", Tag::Byte(0))] {
                assert!(!entries.iter().any(|(k, _)| k == name), "block entity data holds {name}");
                entries.push((name.to_string(), value));
            }
            block_entities.push(Tag::Compound(entries));
        }

        // Scheduled ticks: the queue in the order of its heap.
        let block_ticks: Vec<Tag> = self
            .block_ticks
            .pack(key)
            .into_iter()
            .map(|tick| {
                Tag::Compound(vec![
                    ("i".to_string(), Tag::Str(self.reg.blocks[tick.kind as usize].key.clone())),
                    ("x".to_string(), Tag::Int(tick.pos.x())),
                    ("y".to_string(), Tag::Int(tick.pos.y())),
                    ("z".to_string(), Tag::Int(tick.pos.z())),
                    ("t".to_string(), Tag::Int((tick.trigger - self.game_time) as i32)),
                    ("p".to_string(), Tag::Int(tick.priority)),
                ])
            })
            .collect();

        let fluid_ticks: Vec<Tag> = self
            .fluid_ticks
            .pack(key)
            .into_iter()
            .map(|tick| {
                Tag::Compound(vec![
                    ("i".to_string(), Tag::Str(tick.kind.key().to_string())),
                    ("x".to_string(), Tag::Int(tick.pos.x())),
                    ("y".to_string(), Tag::Int(tick.pos.y())),
                    ("z".to_string(), Tag::Int(tick.pos.z())),
                    ("t".to_string(), Tag::Int((tick.trigger - self.game_time) as i32)),
                    ("p".to_string(), Tag::Int(tick.priority)),
                ])
            })
            .collect();

        let heightmaps: Vec<(String, Tag)> = self.world.chunk(key).heightmaps.iter().map(|map| (map.kind.key().to_string(), Tag::LongArray(map.raw().to_vec()))).collect();

        let mut root = vec![
            ("DataVersion".to_string(), Tag::Int(DATA_VERSION as i32)),
            ("xPos".to_string(), Tag::Int(x)),
            ("yPos".to_string(), Tag::Int(MIN_SECTION)),
            ("zPos".to_string(), Tag::Int(z)),
            ("LastUpdate".to_string(), Tag::Long(self.game_time)),
            ("InhabitedTime".to_string(), Tag::Long(0)),
            ("Status".to_string(), Tag::Str("minecraft:full".to_string())),
            ("sections".to_string(), Tag::List(sections)),
            ("block_entities".to_string(), Tag::List(block_entities)),
            ("block_ticks".to_string(), Tag::List(block_ticks)),
            ("fluid_ticks".to_string(), Tag::List(fluid_ticks)),
            ("PostProcessing".to_string(), Tag::List((0..SECTIONS).map(|_| Tag::List(Vec::new())).collect())),
            ("Heightmaps".to_string(), Tag::Compound(heightmaps)),
            (
                "structures".to_string(),
                Tag::Compound(vec![("starts".to_string(), Tag::Compound(Vec::new())), ("References".to_string(), Tag::Compound(Vec::new()))]),
            ),
        ];
        if light_correct {
            // isLightOn is written true, then false with the light version.
            root.push(("isLightOn".to_string(), Tag::Byte(0)));
            root.push(("starlight.light_version".to_string(), Tag::Int(10)));
        }
        Tag::Compound(root)
    }

    // chunk.getBlockEntitiesPos(): a HashSet of the positions added in the
    // order the chunk's block entity map iterates.
    fn block_entity_order(&self, key: i64) -> Vec<Pos> {
        let Some(keys) = self.block_entity_keys.get(&key) else { return Vec::new() };
        let mut set = crate::jmap::JavaPosSet::new();
        for pos in keys.iter() {
            set.insert(pos);
        }
        set.to_vec()
    }
}
