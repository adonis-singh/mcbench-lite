//! The world export (`mcbench_world::export`, a port of
//! `web/world-export.mjs`): its save read back by this crate's region and
//! zip readers, and the differential against the page's module.
//!
//! Without environment variables the tests here build their worlds' chunks
//! themselves and need nothing. The differential needs Node and the page's
//! module, `MCBENCH_NODE_EXPORT=<checkout whose web/world-export.mjs Node
//! runs>`; its worlds made by the engine need Minecraft's tables as well,
//! `MCBENCH_TABLES=<core/minecraft/1.21.10>`, and its stored worlds
//! `MCBENCH_EXPORT_WORLDS=<folder of world folders, each a manifest.json and
//! its chunks/>`. Those tests are ignored without them and fail when they
//! are set and something is missing. The cases are written to
//! `MCBENCH_EXPORT_OUT` when set, else under the target directory. Run them
//! with `--release --nocapture`: they print what they compared.

mod common;

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Once;

use indexmap::IndexMap;
use mcbench_world::export::{
    self, EngineExport, ExportChunk, ExportTimes, LocalTime, SavedWorld, world_zip,
};
use mcbench_world::facts::MinecraftFacts;
use mcbench_world::region::{self, Nbt};
use mcbench_world::{BlockState, World, WorldManifest, dimension, zip as zip_reader};

/// The moment every save here is made at: 2026-11-17 06:36:58.123 UTC (12:06:58 in Kolkata).
const NOW_MS: i64 = 1_794_897_418_123;
/// The differential's time zone, and its offset at `NOW_MS` (no daylight
/// saving there, so the offset is the same all year).
const TZ: &str = "Asia/Kolkata";
const TZ_OFFSET: i32 = 5 * 3600 + 30 * 60;

// --- worlds whose chunks the test writes ---

/// A block table of the states these tests use (the facts' format): air is
/// air, the rest are not.
const TABLE: &str = "block\tminecraft:air\tnet.minecraft.world.level.block.AirBlock\t-\t0\t1\t0\t0.0\t0.0\t0.0\t-\t-\t0.0\n\
state\t0\t-\t1\n\
block\tminecraft:stone\tnet.minecraft.world.level.block.Block\t-\t0\t1\t1\t0.6\t1.0\t1.0\t-\t-\t6.0\n\
state\t1\t-\n\
block\tminecraft:glass\tnet.minecraft.world.level.block.TransparentBlock\t-\t0\t1\t2\t0.6\t1.0\t1.0\t-\t-\t0.3\n\
state\t2\t-\n\
block\tminecraft:chest\tnet.minecraft.world.level.block.ChestBlock\t-\t0\t2\t3\t0.6\t1.0\t1.0\t-\t-\t2.5\n\
property\tfacing\tnorth,south\n\
state\t3\t0\n\
state\t4\t1\n\
block\tminecraft:barrel\tnet.minecraft.world.level.block.BarrelBlock\t-\t0\t1\t5\t0.6\t1.0\t1.0\t-\t-\t2.5\n\
state\t5\t-\n\
alone\t0,0,0,0,0,0\n\
held\t";

fn test_facts() -> MinecraftFacts {
    MinecraftFacts::new(TABLE).expect("the test's block table")
}

/// A palette entry: `minecraft:chest[facing=south]` is a Name and its
/// Properties.
fn palette_entry(state: &str) -> Nbt {
    let mut entry = IndexMap::new();
    let (name, properties) = match state.split_once('[') {
        Some((name, rest)) => (name, Some(rest.trim_end_matches(']'))),
        None => (state, None),
    };
    entry.insert("Name".into(), Nbt::String(name.into()));
    if let Some(properties) = properties {
        entry.insert(
            "Properties".into(),
            Nbt::Compound(
                properties
                    .split(',')
                    .map(|pair| {
                        let (key, value) = pair.split_once('=').expect("key=value");
                        (key.to_string(), Nbt::String(value.to_string()))
                    })
                    .collect(),
            ),
        );
    }
    Nbt::Compound(entry)
}

fn compound(entries: Vec<(&str, Nbt)>) -> Nbt {
    Nbt::Compound(
        entries
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect(),
    )
}

/// Chunks in the shape the engine saves them (sections of block states in
/// palettes, block entities), of `blocks` at Minecraft's heights, in the
/// order of their first block.
fn chunks_of(blocks: &[([i32; 3], &str)]) -> Vec<ExportChunk> {
    type Sections<'a> = BTreeMap<i32, BTreeMap<usize, &'a str>>;
    let mut chunks: IndexMap<(i32, i32), (Sections, Vec<Nbt>)> = IndexMap::new();
    for &([x, y, z], state) in blocks {
        let (sections, entities) = chunks.entry((x >> 4, z >> 4)).or_default();
        let index = (((y & 15) << 8) | ((z & 15) << 4) | (x & 15)) as usize;
        sections.entry(y >> 4).or_default().insert(index, state);
        let name = state.split('[').next().expect("a name");
        if name == "minecraft:chest" || name == "minecraft:barrel" {
            entities.push(compound(vec![
                ("id", Nbt::String(name.into())),
                ("x", Nbt::Int(x)),
                ("y", Nbt::Int(y)),
                ("z", Nbt::Int(z)),
                ("keepPacked", Nbt::Byte(0)),
                ("Items", Nbt::List(Vec::new())),
            ]));
        }
    }
    chunks
        .into_iter()
        .map(|((cx, cz), (sections, entities))| {
            let sections = sections
                .into_iter()
                .map(|(sy, cells)| {
                    let mut palette = vec!["minecraft:air"];
                    if cells.len() == 4096 {
                        palette.clear();
                    }
                    let mut indices = vec![0u16; 4096];
                    for (&at, &state) in &cells {
                        let i = palette.iter().position(|p| *p == state).unwrap_or_else(|| {
                            palette.push(state);
                            palette.len() - 1
                        });
                        indices[at] = i as u16;
                    }
                    let mut states = vec![(
                        "palette",
                        Nbt::List(palette.iter().map(|s| palette_entry(s)).collect()),
                    )];
                    if palette.len() > 1 {
                        let bits = (64 - (palette.len() as u64 - 1).leading_zeros()).max(4);
                        let per_long = (64 / bits) as usize;
                        let mut data = Vec::new();
                        for group in indices.chunks(per_long) {
                            let long = group
                                .iter()
                                .enumerate()
                                .fold(0u64, |v, (k, &i)| v | (i as u64) << (k as u32 * bits));
                            data.extend_from_slice(&long.to_be_bytes());
                        }
                        states.push(("data", Nbt::LongArray(data)));
                    }
                    compound(vec![
                        ("Y", Nbt::Byte(sy as i8)),
                        ("block_states", compound(states)),
                        (
                            "biomes",
                            compound(vec![(
                                "palette",
                                Nbt::List(vec![Nbt::String(dimension::BIOME.into())]),
                            )]),
                        ),
                    ])
                })
                .collect();
            let chunk = compound(vec![
                ("DataVersion", Nbt::Int(export::DATA_VERSION)),
                ("xPos", Nbt::Int(cx)),
                ("zPos", Nbt::Int(cz)),
                ("yPos", Nbt::Int(-4)),
                ("Status", Nbt::String("minecraft:full".into())),
                ("LastUpdate", Nbt::Long(0)),
                ("sections", Nbt::List(sections)),
                ("block_entities", Nbt::List(entities)),
            ]);
            let Nbt::Compound(root) = chunk else {
                unreachable!()
            };
            ExportChunk {
                x: cx,
                z: cz,
                nbt: export::nbt_file(&root).expect("a chunk's NBT"),
            }
        })
        .collect()
}

/// A world with blocks at negative and positive coordinates in six regions,
/// block entities, a section of one state and builds at the bottom and top
/// of the height range.
fn mixed_blocks() -> Vec<([i32; 3], String)> {
    let mut blocks = vec![
        ([0, -64, 0], "minecraft:stone".to_string()),
        ([-1, -64, -1], "minecraft:glass".into()),
        ([5, 959, -7], "minecraft:stone".into()),
        ([-600, 959, 700], "minecraft:chest[facing=south]".into()),
        ([600, 100, -700], "minecraft:barrel".into()),
        ([-513, 0, 511], "minecraft:chest[facing=north]".into()),
        ([1023, 64, 1024], "minecraft:glass".into()),
    ];
    for i in 0..4096 {
        blocks.push((
            [32 + (i & 15), 16 + (i >> 8), 32 + ((i >> 4) & 15)],
            "minecraft:stone".into(),
        ));
    }
    blocks
}

fn synthetic(blocks: &[([i32; 3], String)], game_time: i64) -> EngineExport {
    let refs: Vec<([i32; 3], &str)> = blocks.iter().map(|(p, s)| (*p, s.as_str())).collect();
    EngineExport {
        game_time,
        chunks: chunks_of(&refs),
    }
}

/// The blocks that are not air in chunks, as `readBlockStates` names them
/// (a copy of its loop over chunks given directly).
fn blocks_of(chunks: &[Nbt], facts: &MinecraftFacts) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for chunk in chunks {
        let x = chunk.get("xPos").and_then(Nbt::as_i64).expect("xPos");
        let z = chunk.get("zPos").and_then(Nbt::as_i64).expect("zPos");
        for section in chunk.get("sections").and_then(Nbt::as_list).unwrap_or(&[]) {
            // The engine saves the light of sections outside the world as
            // sections without blocks.
            let Some(palette) = section
                .get("block_states")
                .and_then(|s| s.get("palette"))
                .and_then(Nbt::as_list)
            else {
                continue;
            };
            let indices = region::section_indices(section).expect("indices");
            let y = section.get("Y").and_then(Nbt::as_i64).expect("Y");
            for i in 0..4096usize {
                let entry = &palette[indices.as_ref().map_or(0, |v| v[i] as usize)];
                let name = entry.get("Name").and_then(Nbt::as_str).expect("Name");
                if !facts.is_air(name).expect("a known block") {
                    out.insert(
                        format!(
                            "{},{},{}",
                            x * 16 + (i & 15) as i64,
                            y * 16 + (i >> 8) as i64,
                            z * 16 + ((i >> 4) & 15) as i64
                        ),
                        region::state_name(entry).expect("a state"),
                    );
                }
            }
        }
    }
    out
}

fn source_chunks(export: &EngineExport) -> Vec<Nbt> {
    export
        .chunks
        .iter()
        .map(|c| region::read_nbt(&c.nbt).expect("the source's NBT"))
        .collect()
}

fn scratch(name: &str) -> PathBuf {
    let folder = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("world-export")
        .join(name);
    if folder.exists() {
        std::fs::remove_dir_all(&folder).expect("remove an old scratch folder");
    }
    std::fs::create_dir_all(&folder).expect("make a scratch folder");
    folder
}

/// The blocks a save's region folder holds, read by `region`'s reader after
/// `zip`'s reader unpacks it.
fn read_back(zip_file: &Path, folder: &str, facts: &MinecraftFacts) -> BTreeMap<String, String> {
    let out = zip_file.with_extension("unpacked");
    if out.exists() {
        std::fs::remove_dir_all(&out).expect("remove an old unpacked save");
    }
    zip_reader::extract_zip(zip_file, &out).expect("the save unpacks");
    let states = region::read_block_states(&out.join(folder).join("region"), facts)
        .expect("the regions read");
    std::fs::remove_dir_all(&out).expect("remove the unpacked save");
    states.into_iter().collect()
}

fn save(export: &EngineExport, name: &str, spawn: [i32; 3]) -> SavedWorld {
    world_zip(name, export, spawn, &ExportTimes::at(NOW_MS, TZ_OFFSET)).expect("a save")
}

// --- tests that need nothing ---

#[test]
fn the_times_of_a_moment_are_its_milliseconds_seconds_and_local_time() {
    let times = ExportTimes::at(NOW_MS, TZ_OFFSET);
    assert_eq!(times.last_played_ms, NOW_MS);
    assert_eq!(times.region_seconds, 1_794_897_418);
    assert_eq!(
        times.zip_time,
        LocalTime {
            year: 2026,
            month: 11,
            day: 17,
            hour: 12,
            minute: 6,
            second: 58
        }
    );
    let local = |ms, offset| ExportTimes::at(ms, offset).zip_time;
    // 2024-02-29 23:59:59 UTC, a day later at +01:00.
    assert_eq!(
        local(1_709_251_199_000, 3600),
        LocalTime {
            year: 2024,
            month: 3,
            day: 1,
            hour: 0,
            minute: 59,
            second: 59
        }
    );
    // Before 1970 at -08:00: 1969-12-31 15:59:59.
    assert_eq!(
        local(-1, -8 * 3600),
        LocalTime {
            year: 1969,
            month: 12,
            day: 31,
            hour: 15,
            minute: 59,
            second: 59
        }
    );
    assert_eq!(ExportTimes::at(-1, 0).region_seconds, u32::MAX);
}

#[test]
fn the_folder_is_the_name_made_a_file_name_and_a_name_of_dots_is_world() {
    let cases = [
        ("Castle", "Castle"),
        ("a/b\\c:d*e?f\"g<h>i|j", "a_b_c_d_e_f_g_h_i_j"),
        ("  tab\tand\nline  ", "tab_and_line"),
        ("\u{a0}\u{2003}x\u{feff}\u{3000}", "x"),
        // U+0085 is not JavaScript white space, and is not a control
        // character below U+0020.
        ("\u{85}x", "\u{85}x"),
        ("", "world"),
        (" .. ", "world"),
        ("...", "world"),
        ("..a", "..a"),
        ("\u{1}", "_"),
    ];
    for (name, folder) in cases {
        assert_eq!(export::folder_name(name), folder, "{name:?}");
    }
}

#[test]
fn the_engine_s_export_bytes_are_read_as_the_page_reads_them_and_short_or_long_bytes_are_refused() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&(-5i64).to_be_bytes());
    bytes.extend_from_slice(&2i32.to_be_bytes());
    for (x, z, nbt) in [(-1i32, 3i32, &b"ab"[..]), (40, -40, &b""[..])] {
        bytes.extend_from_slice(&x.to_be_bytes());
        bytes.extend_from_slice(&z.to_be_bytes());
        bytes.extend_from_slice(&(nbt.len() as i32).to_be_bytes());
        bytes.extend_from_slice(nbt);
    }
    let read = EngineExport::read(&bytes).unwrap();
    assert_eq!(read.game_time, -5);
    assert_eq!(
        read.chunks,
        vec![
            ExportChunk {
                x: -1,
                z: 3,
                nbt: b"ab".to_vec()
            },
            ExportChunk {
                x: 40,
                z: -40,
                nbt: Vec::new()
            }
        ]
    );
    assert_eq!(
        EngineExport::read(&bytes[..bytes.len() - 13]).unwrap_err(),
        "The engine's export ends early"
    );
    let mut long = bytes.clone();
    long.push(0);
    assert_eq!(
        EngineExport::read(&long).unwrap_err(),
        "The engine's export goes on 1 bytes past its last chunk"
    );
}

#[test]
fn a_save_holds_level_dat_the_data_pack_and_the_regions_in_that_order_read_back_to_its_blocks() {
    let facts = test_facts();
    let blocks = mixed_blocks();
    let export = synthetic(&blocks, 123_456_789_012);
    let expected: BTreeMap<String, String> = blocks
        .iter()
        .map(|([x, y, z], s)| (format!("{x},{y},{z}"), s.clone()))
        .collect();
    assert_eq!(blocks_of(&source_chunks(&export), &facts), expected);
    let saved = save(&export, "My/World", [1, 72, -3]);
    assert_eq!(saved.folder, "My_World");
    let folder = scratch("mixed");
    let file = folder.join("save.zip");
    std::fs::write(&file, &saved.bytes).unwrap();

    let names = zip_reader::zip_names(&file).unwrap();
    assert_eq!(
        names,
        [
            "My_World/level.dat",
            "My_World/datapacks/mcbench/pack.mcmeta",
            "My_World/datapacks/mcbench/data/minecraft/dimension_type/overworld.json",
            "My_World/region/r.0.0.mca",
            "My_World/region/r.-1.-1.mca",
            "My_World/region/r.0.-1.mca",
            "My_World/region/r.-2.1.mca",
            "My_World/region/r.1.-2.mca",
            "My_World/region/r.-2.0.mca",
            "My_World/region/r.1.2.mca",
        ]
    );
    for (path, text) in dimension::dimension_pack() {
        assert_eq!(
            zip_reader::read_zip_entry(&file, &format!("My_World/{path}")).unwrap(),
            text.as_bytes()
        );
    }
    assert_eq!(read_back(&file, "My_World", &facts), expected);

    // The chunks read back are the chunks given.
    let unpacked = folder.join("unpacked");
    zip_reader::extract_zip(&file, &unpacked).unwrap();
    let mut read: Vec<Nbt> = region::read_chunks(&unpacked.join("My_World/region")).unwrap();
    let mut given = source_chunks(&export);
    let key = |c: &Nbt| {
        (
            c.get("xPos").unwrap().as_i64(),
            c.get("zPos").unwrap().as_i64(),
        )
    };
    read.sort_by_key(key);
    given.sort_by_key(key);
    assert_eq!(read, given);

    // level.dat: the page's fields, the name as given.
    let mut level = Vec::new();
    flate2::read::GzDecoder::new(&std::fs::read(unpacked.join("My_World/level.dat")).unwrap()[..])
        .read_to_end(&mut level)
        .unwrap();
    let level = region::read_nbt(&level).unwrap();
    let data = level.get("Data").unwrap();
    assert_eq!(data.get("LevelName"), Some(&Nbt::String("My/World".into())));
    assert_eq!(data.get("Time"), Some(&Nbt::Long(123_456_789_012)));
    assert_eq!(data.get("LastPlayed"), Some(&Nbt::Long(NOW_MS)));
    assert_eq!(
        data.get("spawn").and_then(|s| s.get("pos")),
        Some(&Nbt::IntArray(vec![1, 72, -3]))
    );
    std::fs::remove_dir_all(&folder).unwrap();
}

#[test]
fn every_chunk_of_a_region_is_in_the_sectors_after_the_one_before_with_the_save_s_timestamp() {
    let blocks = mixed_blocks();
    let export = synthetic(&blocks, 0);
    let files = export::region_files(&export.chunks, 1_794_897_418).unwrap();
    for (name, file) in &files {
        assert_eq!(file.len() % 4096, 0, "{name}");
        let mut next = 2;
        let mut slots: Vec<(u32, usize)> = (0..1024)
            .filter_map(|i| {
                let location = u32::from_be_bytes(file[i * 4..i * 4 + 4].try_into().unwrap());
                (location != 0).then_some((location, i))
            })
            .collect();
        slots.sort();
        for (location, i) in slots {
            let (sector, count) = ((location >> 8) as usize, (location & 255) as usize);
            assert_eq!(sector, next, "{name} slot {i}");
            let stamp =
                u32::from_be_bytes(file[4096 + i * 4..4096 + i * 4 + 4].try_into().unwrap());
            assert_eq!(stamp, 1_794_897_418);
            let length =
                i32::from_be_bytes(file[sector * 4096..sector * 4096 + 4].try_into().unwrap())
                    as usize;
            assert_eq!(count, (length + 4).div_ceil(4096));
            assert_eq!(file[sector * 4096 + 4], 2);
            assert!(
                file[sector * 4096 + 4 + length..(sector + count) * 4096]
                    .iter()
                    .all(|&b| b == 0)
            );
            next += count;
        }
        assert_eq!(next * 4096, file.len());
    }
}

#[test]
fn a_chunk_of_more_than_255_sectors_is_refused_as_the_page_refuses_it() {
    let mut seed = 7u32;
    let noise: Vec<u8> = (0..1_100_000)
        .map(|_| {
            seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            (seed >> 24) as u8
        })
        .collect();
    let chunk = ExportChunk {
        x: -3,
        z: 70,
        nbt: noise,
    };
    assert_eq!(
        export::region_files(&[chunk], 0).unwrap_err(),
        "Chunk -3, 70 is too large for a region file"
    );
}

#[test]
fn the_zip_and_gzip_wrappers_are_zlib_s() {
    let data = b"mcbench mcbench mcbench";
    let zlib = export::zlib(data);
    assert_eq!(zlib[..2], [0x78, 0x9c]);
    let mut out = Vec::new();
    flate2::read::ZlibDecoder::new(&zlib[..])
        .read_to_end(&mut out)
        .unwrap();
    assert_eq!(out, data);
    let gzip = export::gzip(data);
    let os = if cfg!(target_os = "macos") {
        19
    } else if cfg!(windows) {
        10
    } else {
        3
    };
    assert_eq!(gzip[..10], [0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, os]);
}

// --- the differential with the page's module ---

/// One save to make through both: its export and how the page is asked.
struct Case {
    label: String,
    name: String,
    export: EngineExport,
    spawn: [i32; 3],
    /// The blocks the source world holds where they can be named without
    /// the engine (`"x,y,z"` to the block's id), checked against the save
    /// read back.
    world: Option<BTreeMap<String, String>>,
}

/// A zip entry: its local header, name, data, central directory record
/// and where its local header is.
type Record = (Vec<u8>, String, Vec<u8>, Vec<u8>, usize);

struct Zip {
    /// Per entry: the local header, its name, its data, the central
    /// directory's record, where the local header is.
    entries: Vec<Record>,
    end: Vec<u8>,
}

fn le16(b: &[u8], at: usize) -> usize {
    u16::from_le_bytes([b[at], b[at + 1]]) as usize
}

fn le32(b: &[u8], at: usize) -> usize {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap()) as usize
}

/// A zip of stored entries without extra fields or comments, as the page
/// writes them, read record by record; every record must tile the file.
fn parse_zip(bytes: &[u8]) -> Zip {
    let end = bytes[bytes.len() - 22..].to_vec();
    assert_eq!(le32(&end, 0), 0x06054b50);
    let count = le16(&end, 10);
    let (size, offset) = (le32(&end, 12), le32(&end, 16));
    assert_eq!(offset + size + 22, bytes.len());
    let mut entries = Vec::new();
    let (mut at, mut local_at) = (offset, 0);
    for _ in 0..count {
        let central = bytes[at..at + 46].to_vec();
        assert_eq!(le32(&central, 0), 0x02014b50);
        let name_length = le16(&central, 28);
        let name = String::from_utf8(bytes[at + 46..at + 46 + name_length].to_vec()).unwrap();
        assert_eq!(le32(&central, 42), local_at);
        let local = bytes[local_at..local_at + 30].to_vec();
        assert_eq!(le32(&local, 0), 0x04034b50);
        assert_eq!(le16(&local, 26), name_length);
        assert_eq!(
            &bytes[local_at + 30..local_at + 30 + name_length],
            name.as_bytes()
        );
        let data_at = local_at + 30 + name_length;
        let data = bytes[data_at..data_at + le32(&local, 18)].to_vec();
        let mut crc = flate2::Crc::new();
        crc.update(&data);
        assert_eq!(le32(&local, 14), crc.sum() as usize, "{name}'s CRC");
        assert_eq!(le32(&local, 22), data.len());
        assert_eq!(local[14..26], central[16..28]);
        let next = data_at + data.len();
        entries.push((local, name, data, central, local_at));
        local_at = next;
        at += 46 + name_length;
    }
    assert_eq!(local_at, offset);
    assert_eq!(at, offset + size);
    Zip { entries, end }
}

fn inflate_zlib(stream: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    flate2::read::ZlibDecoder::new(stream)
        .read_to_end(&mut out)
        .expect("a zlib stream");
    out
}

fn gunzip(file: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    flate2::read::GzDecoder::new(file)
        .read_to_end(&mut out)
        .expect("a gzip file");
    out
}

/// A region file's slots in the order of their sectors: index, first
/// sector, sector count and the stored zlib stream.
fn region_slots(file: &[u8]) -> Vec<(usize, usize, usize, Vec<u8>)> {
    let mut slots: Vec<(usize, usize, usize, Vec<u8>)> = (0..1024)
        .filter_map(|i| {
            let location = u32::from_be_bytes(file[i * 4..i * 4 + 4].try_into().unwrap()) as usize;
            if location == 0 {
                return None;
            }
            let (sector, count) = (location >> 8, location & 255);
            let at = sector * 4096;
            let length = i32::from_be_bytes(file[at..at + 4].try_into().unwrap()) as usize;
            assert_eq!(file[at + 4], 2, "slot {i} is zlib");
            Some((i, sector, count, file[at + 5..at + 4 + length].to_vec()))
        })
        .collect();
    slots.sort_by_key(|s| s.1);
    slots
}

#[derive(Default)]
struct Tally {
    zip_bytes: [usize; 2],
    chunks: usize,
    chunks_same_deflate: usize,
    chunks_same_sectors: usize,
    regions_same_bytes: usize,
    regions: usize,
    level_same_deflate: bool,
    zip_same_bytes: bool,
}

/// Compares the save Rust made with the page's, everything the format fixes
/// byte for byte and the compressed data after decompression, and returns
/// what was the same.
fn compare(case: &Case, rust: &SavedWorld, node: &[u8], node_folder: &str) -> Tally {
    let label = &case.label;
    let times = ExportTimes::at(NOW_MS, TZ_OFFSET);
    assert_eq!(rust.folder, node_folder, "{label}: the folder");
    let (r, n) = (parse_zip(&rust.bytes), parse_zip(node));
    let mut tally = Tally {
        zip_bytes: [rust.bytes.len(), node.len()],
        zip_same_bytes: rust.bytes == node,
        ..Tally::default()
    };
    let names = |z: &Zip| z.entries.iter().map(|e| e.1.clone()).collect::<Vec<_>>();
    assert_eq!(names(&r), names(&n), "{label}: the entries and their order");

    // The container: the page's zip of the page's files is this zip writer's
    // byte for byte.
    let node_files: IndexMap<String, Vec<u8>> = n
        .entries
        .iter()
        .map(|e| (e.1.clone(), e.2.clone()))
        .collect();
    assert!(
        export::zip(&node_files, &times.zip_time).unwrap() == node,
        "{label}: the zip of the page's files is not the page's zip"
    );
    // Every record field but the sizes, CRCs and offsets the compressed
    // data moves is the same byte for byte; those are checked above to be
    // right for each zip's own data.
    let blank = |record: &[u8], ranges: &[(usize, usize)]| {
        let mut out = record.to_vec();
        for &(start, end) in ranges {
            out[start..end].fill(0);
        }
        out
    };
    for (re, ne) in r.entries.iter().zip(&n.entries) {
        assert_eq!(
            blank(&re.0, &[(14, 26)]),
            blank(&ne.0, &[(14, 26)]),
            "{label}: {}'s local header",
            ne.1
        );
        assert_eq!(
            blank(&re.3, &[(16, 28), (42, 46)]),
            blank(&ne.3, &[(16, 28), (42, 46)]),
            "{label}: {}'s directory record",
            ne.1
        );
    }
    assert_eq!(
        blank(&r.end, &[(16, 20)]),
        blank(&n.end, &[(16, 20)]),
        "{label}: the end record"
    );

    for (re, ne) in r.entries.iter().zip(&n.entries) {
        let name = &ne.1;
        let file = name.strip_prefix(&format!("{node_folder}/")).unwrap();
        if file == "level.dat" {
            // gzip: header and trailer (CRC-32, length) byte for byte, the
            // NBT after gunzip.
            assert_eq!(re.2[..10], ne.2[..10], "{label}: level.dat's gzip header");
            assert_eq!(
                re.2[re.2.len() - 8..],
                ne.2[ne.2.len() - 8..],
                "{label}: level.dat's gzip trailer"
            );
            assert_eq!(gunzip(&re.2), gunzip(&ne.2), "{label}: level.dat");
            tally.level_same_deflate = re.2 == ne.2;
        } else if let Some(region) = file.strip_prefix("region/") {
            tally.regions += 1;
            tally.regions_same_bytes += usize::from(re.2 == ne.2);
            assert_eq!(
                re.2[4096..8192],
                ne.2[4096..8192],
                "{label}: {region}'s timestamps"
            );
            let (rs, ns) = (region_slots(&re.2), region_slots(&ne.2));
            let indices =
                |s: &[(usize, usize, usize, Vec<u8>)]| s.iter().map(|s| s.0).collect::<Vec<_>>();
            assert_eq!(
                indices(&rs),
                indices(&ns),
                "{label}: {region}'s slots in sector order"
            );
            for (a, b) in rs.iter().zip(&ns) {
                tally.chunks += 1;
                assert_eq!(
                    a.3[..2],
                    b.3[..2],
                    "{label}: {region} slot {}'s zlib header",
                    b.0
                );
                assert_eq!(
                    a.3[a.3.len() - 4..],
                    b.3[b.3.len() - 4..],
                    "{label}: {region} slot {}'s Adler-32",
                    b.0
                );
                assert_eq!(
                    inflate_zlib(&a.3),
                    inflate_zlib(&b.3),
                    "{label}: {region} slot {}'s NBT",
                    b.0
                );
                tally.chunks_same_deflate += usize::from(a.3 == b.3);
                tally.chunks_same_sectors += usize::from(a.2 == b.2);
            }
            // The layout: the page's compressed chunks in this region writer
            // are the page's region file byte for byte.
            let node_chunks: Vec<(i32, i32, Vec<u8>)> = ns
                .iter()
                .map(|s| ((s.0 & 31) as i32, (s.0 >> 5) as i32, s.3.clone()))
                .collect();
            assert!(
                export::region_file(&node_chunks, times.region_seconds).unwrap() == ne.2,
                "{label}: the region file of the page's chunks is not the page's {region}"
            );
        } else {
            assert_eq!(re.0, ne.0, "{label}: {name}'s local header");
            assert_eq!(re.2, ne.2, "{label}: {name}");
        }
    }
    tally
}

fn out_folder(label: &str) -> PathBuf {
    match std::env::var_os("MCBENCH_EXPORT_OUT") {
        Some(folder) => {
            let folder = PathBuf::from(folder).join(label);
            if folder.exists() {
                std::fs::remove_dir_all(&folder).unwrap();
            }
            std::fs::create_dir_all(&folder).unwrap();
            folder
        }
        None => scratch(label),
    }
}

/// Every case through the page's module and through Rust, compared, and the
/// saves of both read back by this crate's readers to the source's blocks.
fn differential(group: &str, cases: Vec<Case>, facts: &MinecraftFacts) {
    let checkout = common::folder("MCBENCH_NODE_EXPORT")
        .expect("set MCBENCH_NODE_EXPORT to a checkout whose web/world-export.mjs Node runs");
    assert!(
        checkout.join("web/world-export.mjs").is_file(),
        "{} has no web/world-export.mjs",
        checkout.display()
    );
    let folder = out_folder(group);
    for case in &cases {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&case.export.game_time.to_be_bytes());
        bytes.extend_from_slice(&(case.export.chunks.len() as i32).to_be_bytes());
        for c in &case.export.chunks {
            bytes.extend_from_slice(&c.x.to_be_bytes());
            bytes.extend_from_slice(&c.z.to_be_bytes());
            bytes.extend_from_slice(&(c.nbt.len() as i32).to_be_bytes());
            bytes.extend_from_slice(&c.nbt);
        }
        assert_eq!(EngineExport::read(&bytes).unwrap(), case.export);
        std::fs::write(folder.join(format!("{}.export", case.label)), bytes).unwrap();
        std::fs::write(
            folder.join(format!("{}.json", case.label)),
            serde_json::json!({ "name": case.name, "spawn": case.spawn, "now": NOW_MS })
                .to_string(),
        )
        .unwrap();
    }
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/node/export.mjs");
    let status = std::process::Command::new("node")
        .arg(&script)
        .arg(&checkout)
        .arg(&folder)
        .env("TZ", TZ)
        .status()
        .expect("node runs");
    assert!(status.success(), "{} failed: {status}", script.display());

    println!(
        "| {group}: case | chunks | zip bytes (Rust / page) | zip identical | regions identical | chunk deflate identical | same sector counts | level.dat deflate identical | blocks read back (Rust = page = source) |"
    );
    println!("|---|---|---|---|---|---|---|---|---|");
    for case in &cases {
        let rust = save(&case.export, &case.name, case.spawn);
        let node = std::fs::read(folder.join(format!("{}.node.zip", case.label))).unwrap();
        let node_folder = common::json(&folder.join(format!("{}.node.json", case.label)))["folder"]
            .as_str()
            .unwrap()
            .to_string();
        let tally = compare(case, &rust, &node, &node_folder);
        let rust_file = folder.join(format!("{}.rust.zip", case.label));
        std::fs::write(&rust_file, &rust.bytes).unwrap();
        let source = blocks_of(&source_chunks(&case.export), facts);
        let from_rust = read_back(&rust_file, &rust.folder, facts);
        let from_node = read_back(
            &folder.join(format!("{}.node.zip", case.label)),
            &node_folder,
            facts,
        );
        assert!(
            from_rust == source,
            "{}: the Rust save reads back to other blocks",
            case.label
        );
        assert!(
            from_node == source,
            "{}: the page's save reads back to other blocks",
            case.label
        );
        if let Some(world) = &case.world {
            let ids: BTreeMap<String, String> = source
                .iter()
                .map(|(k, v)| (k.clone(), v.split('[').next().unwrap().to_string()))
                .collect();
            assert!(
                &ids == world,
                "{}: the save's blocks are not the source world's",
                case.label
            );
        }
        println!(
            "| {} | {} | {} / {} | {} | {}/{} | {}/{} | {}/{} | {} | {} |",
            case.label,
            tally.chunks,
            tally.zip_bytes[0],
            tally.zip_bytes[1],
            tally.zip_same_bytes,
            tally.regions_same_bytes,
            tally.regions,
            tally.chunks_same_deflate,
            tally.chunks,
            tally.chunks_same_sectors,
            tally.chunks,
            tally.level_same_deflate,
            source.len()
        );
        for file in ["export", "rust.zip", "node.zip"] {
            std::fs::remove_file(folder.join(format!("{}.{file}", case.label))).unwrap();
        }
    }
}

#[test]
#[ignore = "needs Node and the page's module: set MCBENCH_NODE_EXPORT to a checkout"]
fn the_page_s_module_and_rust_make_the_same_saves_of_chunks_written_here() {
    let one: Vec<([i32; 3], String)> = vec![([0, 64, 0], "minecraft:stone".into())];
    let four: Vec<([i32; 3], String)> = [
        [-1, 0, -1],
        [0, 0, 0],
        [-1, 0, 0],
        [0, 0, -1],
        [-512, 63, 511],
    ]
    .iter()
    .map(|p| (*p, "minecraft:glass".to_string()))
    .collect();
    let cases = vec![
        ("empty", "Empty", Vec::new(), [0, 72, 0]),
        ("one", "One block", one, [0, 72, 0]),
        ("four", "Four regions", four, [0, 72, 0]),
        ("mixed", " My/World: ünïcødé ", mixed_blocks(), [-3, 967, 4]),
    ]
    .into_iter()
    .map(|(label, name, blocks, spawn)| Case {
        label: label.into(),
        name: name.into(),
        export: synthetic(&blocks, 987_654_321),
        spawn,
        world: None,
    })
    .collect();
    differential("written", cases, &test_facts());
}

// --- worlds the engine makes ---

static TABLES: Once = Once::new();

fn engine_facts() -> MinecraftFacts {
    let blocks = common::blocks_txt();
    let folder = blocks.parent().unwrap().to_path_buf();
    TABLES.call_once(|| mcbench_core::tables::set(mcbench_core::tables::read(&folder)));
    MinecraftFacts::new(&std::fs::read_to_string(blocks).unwrap()).unwrap()
}

/// The engine's export of each world, placed as the live view places it
/// (`placeCommands` at Minecraft's y offset 64), on an engine-sized thread.
fn engine_exports(worlds: &[World], facts: &MinecraftFacts) -> Vec<EngineExport> {
    let commands: Vec<String> = worlds
        .iter()
        .map(|w| w.place_commands(None, 64, None, facts).expect("commands").0)
        .collect();
    std::thread::Builder::new()
        .stack_size(1 << 30)
        .spawn(move || {
            let (registry, blocks) = mcbench_core::boot();
            commands
                .iter()
                .map(|c| {
                    let mut session = mcbench_core::session::Session::new(registry, blocks);
                    // The engine refuses an import of no blocks: an empty world is a
                    // new level with nothing placed.
                    if !c.is_empty() {
                        session.place(c, true).expect("the engine places the world");
                    }
                    EngineExport::read(&session.export()).expect("the engine's export")
                })
                .collect()
        })
        .unwrap()
        .join()
        .expect("the engine thread")
}

/// `"x,y,z"` (Minecraft's y) to the id of every block of `world`.
fn world_ids(world: &World) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    world.for_each(None, |[x, y, z], state| {
        out.insert(format!("{x},{},{z}", y + 64), state.id.clone());
    });
    out
}

/// `live-world.mjs` `exportWorld`'s spawn: above the middle of the build.
fn spawn_of(world: &World) -> [i32; 3] {
    let Some(b) = world.bounds() else {
        return [0, 72, 0];
    };
    let middle = |a: i32, b: i32| ((a + b) as f64 / 2.0 + 0.5).floor() as i32;
    [
        middle(b.min[0], b.max[0]),
        (b.max[1] + 64 + 8).min(959),
        middle(b.min[2], b.max[2]),
    ]
}

fn engine_cases(named: Vec<(&str, &str, World)>, facts: &MinecraftFacts) -> Vec<Case> {
    let worlds: Vec<World> = named.iter().map(|n| n.2.reference()).collect();
    let exports = engine_exports(&worlds, facts);
    named
        .into_iter()
        .zip(exports)
        .map(|((label, name, world), export)| Case {
            label: label.into(),
            name: name.into(),
            spawn: spawn_of(&world),
            world: Some(world_ids(&world)),
            export,
        })
        .collect()
}

#[test]
#[ignore = "needs Node, the page's module and Minecraft's tables: set MCBENCH_NODE_EXPORT and MCBENCH_TABLES"]
fn the_page_s_module_and_rust_make_the_same_saves_of_the_engine_s_chunks() {
    let facts = engine_facts();
    let state = |id: &str, properties: &[(&str, &str)]| BlockState::new(id, properties);
    let mut one = World::new();
    one.set([0, 0, 0], state("minecraft:stone", &[])).unwrap();
    let mut four = World::new();
    four.fill(
        mcbench_world::BlockBox {
            min: [-40, 0, -40],
            max: [40, 0, 40],
        },
        state("minecraft:smooth_stone", &[]),
    )
    .unwrap();
    // Negative and positive coordinates, six regions, block entities and
    // builds at the bottom (Minecraft -64) and top (959) of the height range.
    let mut mixed = World::new();
    mixed
        .fill(
            mcbench_world::BlockBox {
                min: [-20, -128, -20],
                max: [20, -127, 20],
            },
            state("minecraft:stone", &[]),
        )
        .unwrap();
    mixed
        .fill(
            mcbench_world::BlockBox {
                min: [500, 893, -530],
                max: [530, 895, -500],
            },
            state("minecraft:oak_planks", &[]),
        )
        .unwrap();
    let entities = [
        (
            [-600, 0, 700],
            state("minecraft:chest", &[("facing", "south")]),
        ),
        (
            [601, 10, -700],
            state("minecraft:barrel", &[("facing", "up")]),
        ),
        (
            [-513, 895, 511],
            state("minecraft:furnace", &[("facing", "east")]),
        ),
        (
            [3, -128, -3],
            state("minecraft:hopper", &[("facing", "down")]),
        ),
        (
            [1023, 64, 1024],
            state("minecraft:oak_sign", &[("rotation", "4")]),
        ),
    ];
    for (pos, s) in entities {
        mixed.set(pos, s).unwrap();
    }
    mixed
        .set([1, -126, 1], state("minecraft:redstone_wire", &[]))
        .unwrap();
    mixed
        .set([2, -126, 1], state("minecraft:lever", &[("face", "floor")]))
        .unwrap();
    let cases = engine_cases(
        vec![
            ("empty", "Empty", World::new()),
            ("one", "One block", one),
            ("four", "Four regions", four),
            ("mixed", "Mixed: top/bottom", mixed),
        ],
        &facts,
    );
    let entities: usize = cases
        .iter()
        .map(|c| {
            source_chunks(&c.export)
                .iter()
                .map(|n| {
                    n.get("block_entities")
                        .and_then(Nbt::as_list)
                        .map_or(0, |l| l.len())
                })
                .sum::<usize>()
        })
        .sum();
    assert!(
        entities >= 5,
        "the engine's chunks hold {entities} block entities"
    );
    differential("engine", cases, &facts);
}

#[test]
#[ignore = "needs Node, the page's module, Minecraft's tables and stored worlds: set MCBENCH_NODE_EXPORT, MCBENCH_TABLES and MCBENCH_EXPORT_WORLDS"]
fn the_page_s_module_and_rust_make_the_same_saves_of_stored_worlds() {
    let facts = engine_facts();
    let root = common::folder("MCBENCH_EXPORT_WORLDS").expect("set MCBENCH_EXPORT_WORLDS");
    let mut folders: Vec<PathBuf> = std::fs::read_dir(&root)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.join("manifest.json").is_file())
        .collect();
    folders.sort();
    assert!(
        !folders.is_empty(),
        "{} holds no world folder",
        root.display()
    );
    let mut cases = Vec::new();
    for folder in folders {
        let manifest = std::fs::read_to_string(folder.join("manifest.json")).unwrap();
        let chunks = folder.join("chunks");
        let world = World::from_manifest(
            &WorldManifest(manifest.trim_end().to_string()),
            &mut |hash: &str| {
                std::fs::read(chunks.join(format!("{hash}.json")))
                    .map_err(|e| format!("{hash}: {e}"))
            },
        )
        .expect("a stored world");
        let label = folder.file_name().unwrap().to_string_lossy().into_owned();
        println!("{label}: {} blocks", world.size());
        cases.extend(engine_cases(
            vec![(&label, "A stored world", world)],
            &facts,
        ));
    }
    differential("stored", cases, &facts);
}
