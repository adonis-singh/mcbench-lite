//! A Minecraft world save of the chunks the engine holds
//! (`web/world-export.mjs`): a zip of one world folder holding `level.dat`,
//! the dimension data pack and `region/r.x.z.mca`, openable in Minecraft
//! 1.21.10. The chunks are the engine's own saved chunks
//! (`mcbench_core::session::Session::export`); entities are not saved.
//!
//! Every file, field, record and order is the page's. The compressed bytes
//! (each chunk's zlib stream, `level.dat`'s gzip) are flate2's deflate at
//! zlib's default level, which inflates to the same bytes as the page's
//! (Node's and Chromium's zlib) but is not the same deflate stream; the zlib
//! and gzip headers and trailers are the ones zlib writes
//! (`tests/world_export_test.rs` holds all of it to the page's module).
//!
//! Nothing here reads the clock: the times a save records are arguments
//! ([`ExportTimes`]).

use std::io::Write;

use indexmap::IndexMap;

use crate::dimension::{BIOME, dimension_pack};
use crate::region::Nbt;

/// The data version `level.dat` names (`DATA_VERSION`).
pub const DATA_VERSION: i32 = 4556;
/// The game version `level.dat` names (`MC_VERSION`).
pub const MC_VERSION: &str = "1.21.10";
/// A region file's sector (`SECTOR`).
const SECTOR: usize = 4096;
/// zlib's default level, which `CompressionStream` uses.
const LEVEL: u32 = 6;
/// The OS byte zlib's gzip header carries for the platform it was built for
/// (`OS_CODE` in zlib's `zutil.h`): Node on macOS writes 19, on Windows 10,
/// elsewhere 3.
#[cfg(target_os = "macos")]
const GZIP_OS: u8 = 19;
#[cfg(windows)]
const GZIP_OS: u8 = 10;
#[cfg(not(any(target_os = "macos", windows)))]
const GZIP_OS: u8 = 3;

/// One chunk of the engine's export: its position and its NBT, uncompressed,
/// as `NbtIo.write` writes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExportChunk {
    /// The chunk's x.
    pub x: i32,
    /// The chunk's z.
    pub z: i32,
    /// The root compound, nameless.
    pub nbt: Vec<u8>,
}

/// What the engine's export holds (`CoreWorld.export`): the game time and
/// every chunk that holds a block, in the engine's order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EngineExport {
    /// The level's game time.
    pub game_time: i64,
    /// The chunks.
    pub chunks: Vec<ExportChunk>,
}

impl EngineExport {
    /// Reads `Session::export`'s bytes as `web/core-world.mjs` `export`
    /// does: the game time (i64), the number of chunks (i32), then each
    /// chunk's x, z and length (i32) and its NBT, big endian. Bytes that end
    /// early, or that go on after the last chunk, are refused.
    pub fn read(bytes: &[u8]) -> Result<EngineExport, String> {
        let mut at = 0usize;
        let mut take = |n: usize| -> Result<&[u8], String> {
            let part = bytes
                .get(at..at + n)
                .ok_or("The engine's export ends early")?;
            at += n;
            Ok(part)
        };
        let int = |b: &[u8]| i32::from_be_bytes(b.try_into().expect("four bytes"));
        let game_time = i64::from_be_bytes(take(8)?.try_into().expect("eight bytes"));
        let count = int(take(4)?);
        let mut chunks = Vec::with_capacity(count.max(0) as usize);
        for _ in 0..count {
            let x = int(take(4)?);
            let z = int(take(4)?);
            let length = int(take(4)?);
            let length = usize::try_from(length)
                .map_err(|_| format!("The engine's export gives chunk {x}, {z} length {length}"))?;
            chunks.push(ExportChunk {
                x,
                z,
                nbt: take(length)?.to_vec(),
            });
        }
        if at != bytes.len() {
            return Err(format!(
                "The engine's export goes on {} bytes past its last chunk",
                bytes.len() - at
            ));
        }
        Ok(EngineExport { game_time, chunks })
    }
}

/// A moment as JavaScript's `Date` getters read it in the local time zone
/// (`getFullYear`, `getMonth() + 1`, `getDate`, `getHours`, `getMinutes`,
/// `getSeconds`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LocalTime {
    /// The year.
    pub year: i32,
    /// 1 to 12.
    pub month: u32,
    /// 1 to 31.
    pub day: u32,
    /// 0 to 23.
    pub hour: u32,
    /// 0 to 59.
    pub minute: u32,
    /// 0 to 59.
    pub second: u32,
}

/// The times a save records, which the page reads from the clock.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExportTimes {
    /// `level.dat`'s `LastPlayed`: milliseconds since 1970 (`Date.now()`).
    pub last_played_ms: i64,
    /// Every chunk's timestamp in the region files: seconds since 1970
    /// (`Math.floor(Date.now() / 1000)`, written as an unsigned 32-bit
    /// number).
    pub region_seconds: u32,
    /// The zip entries' modification time (`new Date()`, local time).
    pub zip_time: LocalTime,
}

impl ExportTimes {
    /// The times of one moment, `now_ms` milliseconds since 1970, in a time
    /// zone `utc_offset_seconds` ahead of UTC at that moment.
    pub fn at(now_ms: i64, utc_offset_seconds: i32) -> ExportTimes {
        let local = now_ms + utc_offset_seconds as i64 * 1000;
        let days = local.div_euclid(86_400_000);
        let of_day = local.rem_euclid(86_400_000) / 1000;
        // Days to a civil date (Howard Hinnant's days_from_civil, inverted).
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z.rem_euclid(146_097);
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = doy - (153 * mp + 2) / 5 + 1;
        let month = if mp < 10 { mp + 3 } else { mp - 9 };
        let year = yoe + era * 400 + i64::from(month <= 2);
        ExportTimes {
            last_played_ms: now_ms,
            region_seconds: now_ms.div_euclid(1000) as u32,
            zip_time: LocalTime {
                year: year as i32,
                month: month as u32,
                day: day as u32,
                hour: (of_day / 3600) as u32,
                minute: (of_day / 60 % 60) as u32,
                second: (of_day % 60) as u32,
            },
        }
    }
}

/// A save: the folder the zip holds and the zip's bytes (`worldZip`'s
/// `{ folder, bytes }`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SavedWorld {
    /// The world folder's name inside the zip.
    pub folder: String,
    /// The zip.
    pub bytes: Vec<u8>,
}

fn tag_id(value: &Nbt) -> u8 {
    match value {
        Nbt::Byte(_) => 1,
        Nbt::Short(_) => 2,
        Nbt::Int(_) => 3,
        Nbt::Long(_) => 4,
        Nbt::Float(_) => 5,
        Nbt::Double(_) => 6,
        Nbt::ByteArray(_) => 7,
        Nbt::String(_) => 8,
        Nbt::List(_) => 9,
        Nbt::Compound(_) => 10,
        Nbt::IntArray(_) => 11,
        Nbt::LongArray(_) => 12,
    }
}

fn write_text(text: &str, out: &mut Vec<u8>) -> Result<(), String> {
    let length = u16::try_from(text.len()).map_err(|_| {
        format!(
            "An NBT string of {} bytes is longer than NBT holds",
            text.len()
        )
    })?;
    out.extend_from_slice(&length.to_be_bytes());
    out.extend_from_slice(text.as_bytes());
    Ok(())
}

fn count(n: usize) -> Result<[u8; 4], String> {
    i32::try_from(n)
        .map(i32::to_be_bytes)
        .map_err(|_| format!("An NBT list or array of {n} items is longer than NBT holds"))
}

/// Writes a value's payload (`writeNbt`): strings as UTF-8, a list's
/// element tag from its first item (end for an empty list).
pub fn write_nbt(value: &Nbt, out: &mut Vec<u8>) -> Result<(), String> {
    match value {
        Nbt::Byte(v) => out.push(*v as u8),
        Nbt::Short(v) => out.extend_from_slice(&v.to_be_bytes()),
        Nbt::Int(v) => out.extend_from_slice(&v.to_be_bytes()),
        Nbt::Long(v) => out.extend_from_slice(&v.to_be_bytes()),
        Nbt::Float(v) => out.extend_from_slice(&v.to_be_bytes()),
        Nbt::Double(v) => out.extend_from_slice(&v.to_be_bytes()),
        Nbt::ByteArray(bytes) => {
            out.extend_from_slice(&count(bytes.len())?);
            out.extend_from_slice(bytes);
        }
        Nbt::String(text) => write_text(text, out)?,
        Nbt::List(items) => {
            let element = items.first().map_or(0, tag_id);
            if items.iter().any(|item| tag_id(item) != element) {
                return Err("An NBT list holds items of more than one tag".into());
            }
            out.push(element);
            out.extend_from_slice(&count(items.len())?);
            for item in items {
                write_nbt(item, out)?;
            }
        }
        Nbt::Compound(map) => {
            for (name, child) in map {
                out.push(tag_id(child));
                write_text(name, out)?;
                write_nbt(child, out)?;
            }
            out.push(0);
        }
        Nbt::IntArray(values) => {
            out.extend_from_slice(&count(values.len())?);
            for v in values {
                out.extend_from_slice(&v.to_be_bytes());
            }
        }
        Nbt::LongArray(bytes) => {
            if bytes.len() % 8 != 0 {
                return Err(format!(
                    "An NBT long array of {} bytes is not whole longs",
                    bytes.len()
                ));
            }
            out.extend_from_slice(&count(bytes.len() / 8)?);
            out.extend_from_slice(bytes);
        }
    }
    Ok(())
}

/// A file's root compound, nameless (`nbtFile`, `NbtIo.write`).
pub fn nbt_file(compound: &IndexMap<String, Nbt>) -> Result<Vec<u8>, String> {
    let mut out = vec![10, 0, 0];
    for (name, child) in compound {
        out.push(tag_id(child));
        write_text(name, &mut out)?;
        write_nbt(child, &mut out)?;
    }
    out.push(0);
    Ok(out)
}

fn compound<const N: usize>(entries: [(&str, Nbt); N]) -> Nbt {
    Nbt::Compound(
        entries
            .into_iter()
            .map(|(name, value)| (name.to_string(), value))
            .collect(),
    )
}

fn text(value: &str) -> Nbt {
    Nbt::String(value.to_string())
}

fn flag(on: bool) -> Nbt {
    Nbt::Byte(i8::from(on))
}

/// `level.dat`'s root compound (`levelData`): a void overworld (one layer
/// of air) in creative mode, the engine's game rules (no weather, no
/// daylight cycle, no random ticks, no mobs, no fire spread) and the clock
/// where the engine left it.
pub fn level_data(
    name: &str,
    game_time: i64,
    spawn: [i32; 3],
    last_played_ms: i64,
) -> IndexMap<String, Nbt> {
    let rules = [
        ("doDaylightCycle", "false"),
        ("doWeatherCycle", "false"),
        ("doMobSpawning", "false"),
        ("doFireTick", "false"),
        ("randomTickSpeed", "0"),
        ("keepInventory", "true"),
        ("spawnRadius", "0"),
        ("commandBlocksEnabled", "true"),
        ("announceAdvancements", "false"),
    ];
    let flat = compound([
        ("type", text("minecraft:flat")),
        (
            "settings",
            compound([
                (
                    "layers",
                    Nbt::List(vec![compound([
                        ("block", text("minecraft:air")),
                        ("height", Nbt::Int(1)),
                    ])]),
                ),
                ("features", flag(false)),
                ("lakes", flag(false)),
                ("biome", text(BIOME)),
            ]),
        ),
    ]);
    let noise = |dimension: &str, settings: &str, biome_source: Nbt| {
        compound([
            ("type", text(dimension)),
            (
                "generator",
                compound([
                    ("type", text("minecraft:noise")),
                    ("settings", text(settings)),
                    ("biome_source", biome_source),
                ]),
            ),
        ])
    };
    let data = compound([
        ("DataVersion", Nbt::Int(DATA_VERSION)),
        ("version", Nbt::Int(19133)),
        (
            "Version",
            compound([
                ("Id", Nbt::Int(DATA_VERSION)),
                ("Name", text(MC_VERSION)),
                ("Series", text("main")),
                ("Snapshot", flag(false)),
            ]),
        ),
        ("LevelName", text(name)),
        ("GameType", Nbt::Int(1)),
        ("Difficulty", Nbt::Byte(0)),
        ("DifficultyLocked", flag(false)),
        ("hardcore", flag(false)),
        ("allowCommands", flag(true)),
        ("initialized", flag(true)),
        ("Time", Nbt::Long(game_time)),
        ("DayTime", Nbt::Long(6000)),
        ("LastPlayed", Nbt::Long(last_played_ms)),
        ("raining", flag(false)),
        ("rainTime", Nbt::Int(0)),
        ("thundering", flag(false)),
        ("thunderTime", Nbt::Int(0)),
        ("clearWeatherTime", Nbt::Int(0)),
        (
            "spawn",
            compound([
                ("pos", Nbt::IntArray(spawn.to_vec())),
                ("yaw", Nbt::Float(0.0)),
                ("pitch", Nbt::Float(0.0)),
                ("dimension", text("minecraft:overworld")),
            ]),
        ),
        (
            "GameRules",
            Nbt::Compound(
                rules
                    .iter()
                    .map(|(key, value)| (key.to_string(), text(value)))
                    .collect(),
            ),
        ),
        (
            "DragonFight",
            compound([
                ("DragonKilled", flag(false)),
                ("PreviouslyKilled", flag(false)),
                ("NeedsStateScanning", flag(true)),
            ]),
        ),
        ("CustomBossEvents", compound([])),
        ("ScheduledEvents", Nbt::List(Vec::new())),
        ("WanderingTraderSpawnDelay", Nbt::Int(0)),
        ("WanderingTraderSpawnChance", Nbt::Int(0)),
        (
            "DataPacks",
            compound([
                (
                    "Enabled",
                    Nbt::List(vec![text("vanilla"), text("file/mcbench")]),
                ),
                (
                    "Disabled",
                    Nbt::List(vec![
                        text("minecart_improvements"),
                        text("redstone_experiments"),
                        text("trade_rebalance"),
                    ]),
                ),
            ]),
        ),
        (
            "WorldGenSettings",
            compound([
                ("seed", Nbt::Long(0)),
                ("generate_features", flag(false)),
                ("bonus_chest", flag(false)),
                (
                    "dimensions",
                    compound([
                        (
                            "minecraft:overworld",
                            compound([("type", text("minecraft:overworld")), ("generator", flat)]),
                        ),
                        (
                            "minecraft:the_nether",
                            noise(
                                "minecraft:the_nether",
                                "minecraft:nether",
                                compound([
                                    ("type", text("minecraft:multi_noise")),
                                    ("preset", text("minecraft:nether")),
                                ]),
                            ),
                        ),
                        (
                            "minecraft:the_end",
                            noise(
                                "minecraft:the_end",
                                "minecraft:end",
                                compound([("type", text("minecraft:the_end"))]),
                            ),
                        ),
                    ]),
                ),
            ]),
        ),
    ]);
    [("Data".to_string(), data)].into_iter().collect()
}

/// zlib's Adler-32 of `bytes`.
fn adler32(bytes: &[u8]) -> u32 {
    const MOD: u32 = 65_521;
    let (mut a, mut b) = (1u32, 0u32);
    // 5552 bytes is the most that cannot overflow `b` before the modulo.
    for block in bytes.chunks(5552) {
        for &byte in block {
            a += byte as u32;
            b += a;
        }
        a %= MOD;
        b %= MOD;
    }
    (b << 16) | a
}

/// `bytes` as a zlib stream (`CompressionStream('deflate')`): zlib's header
/// for its default level (78 9c), the deflate data, the Adler-32.
pub fn zlib(bytes: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x9c];
    let mut encoder = flate2::write::DeflateEncoder::new(&mut out, flate2::Compression::new(LEVEL));
    encoder
        .write_all(bytes)
        .expect("writing to memory does not fail");
    encoder.finish().expect("writing to memory does not fail");
    out.extend_from_slice(&adler32(bytes).to_be_bytes());
    out
}

/// `bytes` as a gzip file (`CompressionStream('gzip')`): zlib's header (no
/// name, no time, the platform's OS byte), the deflate data, the CRC-32 and
/// the length.
pub fn gzip(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = flate2::GzBuilder::new()
        .operating_system(GZIP_OS)
        .write(Vec::new(), flate2::Compression::new(LEVEL));
    encoder
        .write_all(bytes)
        .expect("writing to memory does not fail");
    encoder.finish().expect("writing to memory does not fail")
}

/// One region file of chunks already compressed (`regionFiles`' loop over a
/// region's members): `(x, z, zlib stream)` in the order given, each in the
/// sectors after the one before, padded to whole sectors, every chunk's
/// timestamp `timestamp`.
pub fn region_file(chunks: &[(i32, i32, Vec<u8>)], timestamp: u32) -> Result<Vec<u8>, String> {
    let mut out = vec![0u8; 2 * SECTOR];
    let mut sector = 2usize;
    for (x, z, compressed) in chunks {
        let length = compressed.len() + 1;
        let sectors = (length + 4).div_ceil(SECTOR);
        if sectors > 255 {
            return Err(format!("Chunk {x}, {z} is too large for a region file"));
        }
        let index = ((x & 31) + (z & 31) * 32) as usize;
        let location = ((sector as u32) << 8) | sectors as u32;
        out[index * 4..index * 4 + 4].copy_from_slice(&location.to_be_bytes());
        out[SECTOR + index * 4..SECTOR + index * 4 + 4].copy_from_slice(&timestamp.to_be_bytes());
        let start = out.len();
        out.extend_from_slice(&(length as i32).to_be_bytes());
        out.push(2); // zlib
        out.extend_from_slice(compressed);
        out.resize(start + sectors * SECTOR, 0);
        sector += sectors;
    }
    Ok(out)
}

/// The files of a region folder, by name in the order of each region's
/// first chunk (`regionFiles`): `r.<x>.<z>.mca` for chunks `x >> 5`,
/// `z >> 5`.
pub fn region_files(
    chunks: &[ExportChunk],
    timestamp: u32,
) -> Result<IndexMap<String, Vec<u8>>, String> {
    let mut regions: IndexMap<String, Vec<&ExportChunk>> = IndexMap::new();
    for chunk in chunks {
        regions
            .entry(format!("r.{}.{}.mca", chunk.x >> 5, chunk.z >> 5))
            .or_default()
            .push(chunk);
    }
    regions
        .into_iter()
        .map(|(name, members)| {
            let compressed: Vec<(i32, i32, Vec<u8>)> = members
                .iter()
                .map(|chunk| (chunk.x, chunk.z, zlib(&chunk.nbt)))
                .collect();
            Ok((name, region_file(&compressed, timestamp)?))
        })
        .collect()
}

fn le16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn le32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

/// A zip of `files` by name, every entry stored (`zip`): version 2.0, UTF-8
/// names, the local time `time` in DOS form, no extra fields, comments or
/// attributes. More than 65,535 entries, or entries or offsets past 4 GiB,
/// which a zip without its 64-bit extension cannot hold, are refused.
pub fn zip(files: &IndexMap<String, Vec<u8>>, time: &LocalTime) -> Result<Vec<u8>, String> {
    // `setUint16` keeps the low 16 bits, as `as u16` does.
    let dos_time = ((time.hour << 11) | (time.minute << 5) | (time.second >> 1)) as u16;
    let dos_date =
        (((time.year - 1980) << 9) | ((time.month as i32) << 5) | time.day as i32) as u16;
    let entries = u16::try_from(files.len())
        .map_err(|_| format!("A zip of {} files is more than a zip holds", files.len()))?;
    let too_large = || "The world is larger than a zip holds (4 GiB)".to_string();
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (name, bytes) in files {
        let offset = u32::try_from(out.len()).map_err(|_| too_large())?;
        let size = u32::try_from(bytes.len()).map_err(|_| too_large())?;
        let name_length = u16::try_from(name.len())
            .map_err(|_| format!("A zip entry's name of {} bytes is too long", name.len()))?;
        let mut crc = flate2::Crc::new();
        crc.update(bytes);
        let crc = crc.sum();
        le32(&mut out, 0x04034b50);
        le16(&mut out, 20);
        le16(&mut out, 0x0800); // UTF-8 names
        le16(&mut out, 0);
        le16(&mut out, dos_time);
        le16(&mut out, dos_date);
        le32(&mut out, crc);
        le32(&mut out, size);
        le32(&mut out, size);
        le16(&mut out, name_length);
        le16(&mut out, 0);
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(bytes);
        le32(&mut central, 0x02014b50);
        le16(&mut central, 20);
        le16(&mut central, 20);
        le16(&mut central, 0x0800);
        le16(&mut central, 0);
        le16(&mut central, dos_time);
        le16(&mut central, dos_date);
        le32(&mut central, crc);
        le32(&mut central, size);
        le32(&mut central, size);
        le16(&mut central, name_length);
        central.extend_from_slice(&[0; 12]);
        le32(&mut central, offset);
        central.extend_from_slice(name.as_bytes());
    }
    let directory_offset = u32::try_from(out.len()).map_err(|_| too_large())?;
    let directory_size = u32::try_from(central.len()).map_err(|_| too_large())?;
    out.extend_from_slice(&central);
    le32(&mut out, 0x06054b50);
    le16(&mut out, 0);
    le16(&mut out, 0);
    le16(&mut out, entries);
    le16(&mut out, entries);
    le32(&mut out, directory_size);
    le32(&mut out, directory_offset);
    le16(&mut out, 0);
    Ok(out)
}

/// Whether JavaScript's `String.prototype.trim` removes `c` (WhiteSpace and
/// LineTerminator).
fn js_space(c: char) -> bool {
    matches!(
        c,
        '\u{9}' | '\u{a}' | '\u{b}' | '\u{c}' | '\u{d}' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200a}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202f}'
                | '\u{205f}'
                | '\u{3000}'
                | '\u{feff}'
    )
}

/// The world folder's name for a save called `name` (`worldZip`): the
/// characters a file name cannot hold made `_`, trimmed; a name of dots
/// alone (or nothing) is `world`, as it would name the zip's parent folder.
pub fn folder_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| match c {
            '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\u{0}'..='\u{1f}' => '_',
            c => c,
        })
        .collect();
    let cleaned = cleaned.trim_matches(js_space);
    if cleaned.chars().all(|c| c == '.') {
        "world".to_string()
    } else {
        cleaned.to_string()
    }
}

/// The save of an engine export as a zip (`worldZip`): one folder holding
/// `level.dat`, the dimension data pack and `region/`, the files in that
/// order. `name` is the world's name in Minecraft (`LevelName`) and, made a
/// file name, the folder's; `spawn` is `[x, y, z]`.
pub fn world_zip(
    name: &str,
    export: &EngineExport,
    spawn: [i32; 3],
    times: &ExportTimes,
) -> Result<SavedWorld, String> {
    let folder = folder_name(name);
    let mut files = IndexMap::new();
    files.insert(
        format!("{folder}/level.dat"),
        gzip(&nbt_file(&level_data(
            name,
            export.game_time,
            spawn,
            times.last_played_ms,
        ))?),
    );
    for (file, text) in dimension_pack() {
        files.insert(format!("{folder}/{file}"), text.into_bytes());
    }
    for (file, bytes) in region_files(&export.chunks, times.region_seconds)? {
        files.insert(format!("{folder}/region/{file}"), bytes);
    }
    let bytes = zip(&files, &times.zip_time)?;
    Ok(SavedWorld { folder, bytes })
}
