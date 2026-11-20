//! The tints by state id, and Minecraft's client's tints.
//!
//! `the_tints_are_read_by_state_id` and
//! `tints_of_other_states_than_the_block_tables_are_refused` port
//! `test/block-tints.test.js`, over states written for the test (the
//! helpers `test/helpers/block-tints.js` `tintsText` and
//! `test/helpers/block-table.js` `testBlockStates`, ported below).
//!
//! The ignored tests port `test/block-tints-minecraft.test.js` against
//! Minecraft's real files, which are never committed:
//! `the_tints_are_minecrafts_clients_in_the_worlds_biome` reads
//! `MCBENCH_DIFF_DATA` (a folder of the shipped `block-table.tsv`,
//! `render-layers.tsv`, `tints.tsv`) and `MCBENCH_DIFF_CLIENT_JAR`
//! (Mojang's whole client jar);
//! `the_live_viewers_pack_tints_every_state_minecrafts_client_tints` reads
//! `MCBENCH_DIFF_DATA` (`tints.tsv`, `mcbench-pack.bin`);
//! `a_blocks_icon_is_its_particles_colour_and_a_name_of_no_block_has_none`
//! reads `MCBENCH_DIFF_DATA` (`block-table.tsv`, `render-layers.tsv`,
//! `tints.tsv`, `client-jar.json`) and `MCBENCH_DIFF_TEXTURES` (the bake's
//! `atlas.png`, `atlas_map.json`). Each fails when its variable is unset or
//! a file is missing.

#[path = "../../agent/tests/common/fixture.rs"]
mod fixture;

use std::collections::HashMap;
use std::io::Read;
use std::path::PathBuf;

use mcbench_renderer::pack::Pack;
use mcbench_setup::bake::decode_png;
use mcbench_setup::icon::{Icons, icon};
use mcbench_setup::jar::{self, Entry, Manifest};
use mcbench_setup::tints::BlockTints;
use mcbench_world::block_states::BlockStates;
use serde_json::Value;

/// `testBlockStates(states)` of states named alone (every other field its
/// default), air first: `blockTable` and `renderLayers` with no options.
fn test_block_states(names: &[&str]) -> BlockStates {
    let all: Vec<&str> = std::iter::once("minecraft:air")
        .chain(names.iter().copied())
        .collect();
    let mut lines = Vec::new();
    let mut block = "";
    for (i, name) in all.iter().enumerate() {
        let id = name.split('[').next().expect("a name");
        if id != block {
            lines.push(format!("default\t{id}\t{name}\tTest {id}"));
        }
        block = id;
        let model = u8::from(i != 0);
        lines.push(format!(
            "{name}\t0\t0\t0\t0,0,0,0,0,0\t0\t{model}\t0,0\t0,0,0,0,0,0\t0"
        ));
    }
    lines.push("facehides\t2\t0101".into());
    lines.push("faceoccludes\t2\t0111".into());
    lines.push("skiprendering\t1\t1\t0".into());
    let table = format!("{}\n", lines.join("\n"));
    let mut layers = vec!["layer\tOPAQUE\t0".to_string(), "layer\tBLENDED\t1".into()];
    layers.extend(all.iter().map(|name| format!("{name}\t0\t0\t-")));
    BlockStates::new(&table, &format!("{}\n", layers.join("\n"))).expect("the test table reads")
}

/// One state's colours in `tintsText`'s `colors`.
struct Colors {
    world: [u32; 2],
    single: u32,
    particle: u32,
    faces: &'static str,
}

const UNTINTED: Colors = Colors {
    world: [0xffffff, 0xffffff],
    single: 0xffffff,
    particle: 0xffffff,
    faces: "-",
};

/// `tintsText(states, colors)`: two tint indices, a state not named
/// untinted.
fn tints_text(states: &BlockStates, colors: &[(&str, Colors)]) -> String {
    let mut names: Vec<(&String, &u16)> = states.ids.iter().collect();
    names.sort_by_key(|(_, id)| **id);
    let mut lines = vec!["tints\t2".to_string()];
    for (name, _) in names {
        let c = colors
            .iter()
            .find(|(n, _)| *n == name.as_str())
            .map_or(&UNTINTED, |(_, c)| c);
        lines.push(format!(
            "{name}\t{:06x},{:06x}\t{:06x}\t{:06x}\t{}",
            c.world[0], c.world[1], c.single, c.particle, c.faces
        ));
    }
    format!("{}\n", lines.join("\n"))
}

fn props(list: &[(&str, &str)]) -> Vec<(String, String)> {
    list.iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

fn states() -> BlockStates {
    test_block_states(&[
        "minecraft:grass_block[snowy=false]",
        "minecraft:grass_block[snowy=true]",
        "minecraft:stone",
    ])
}

#[test]
fn the_tints_are_read_by_state_id() {
    let states = states();
    let text = tints_text(
        &states,
        &[
            (
                "minecraft:grass_block[snowy=false]",
                Colors {
                    world: [0x91bd59, 0x91bd59],
                    single: 0x7cbd6b,
                    particle: 0xffffff,
                    faces: "0",
                },
            ),
            (
                "minecraft:grass_block[snowy=true]",
                Colors {
                    world: [0x91bd59, 0x123456],
                    particle: 0x91bd59,
                    ..UNTINTED
                },
            ),
        ],
    );
    let tints = BlockTints::new(&text, &states).unwrap();
    assert_eq!(tints.indices, 2);
    let grass = states
        .id_of("minecraft:grass_block", &props(&[("snowy", "true")]))
        .unwrap() as usize;
    assert_eq!(tints.world[grass * 2..grass * 2 + 2], [0x91bd59, 0x123456]);
    assert_eq!(tints.particle[grass], 0x91bd59);
    let snowless = states
        .id_of("minecraft:grass_block", &props(&[("snowy", "false")]))
        .unwrap() as usize;
    assert_eq!(tints.particle[snowless], 0xffffff);
    let stone = states.id_of("minecraft:stone", &[]).unwrap() as usize;
    assert_eq!(tints.world[stone * 2..stone * 2 + 2], [0xffffff, 0xffffff]);

    // The page's bytes: states, indices, then the colours by state and index.
    let bytes = tints.bytes();
    let words: Vec<u32> = bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|w| u32::from_le_bytes(*w))
        .collect();
    assert_eq!(words[..2], [states.count as u32, 2]);
    assert_eq!(words[2..], tints.world[..]);
}

#[test]
fn tints_of_other_states_than_the_block_tables_are_refused() {
    let states = states();
    let text = tints_text(&states, &[]);
    let mut lines: Vec<&str> = text.split('\n').collect();
    let short = lines[..lines.len() - 2].join("\n");
    let error = BlockTints::new(&short, &states).unwrap_err();
    assert!(
        error.contains("The tints have 3 states, the block table 4"),
        "{error}"
    );
    lines.swap(2, 3);
    let error = BlockTints::new(&lines.join("\n"), &states).unwrap_err();
    assert!(
        error.contains("The tints name state 1 minecraft:grass_block[snowy=true]"),
        "{error}"
    );
}

// Minecraft's real files.

/// The folder or file a variable names.
fn env_path(variable: &str) -> PathBuf {
    PathBuf::from(std::env::var_os(variable).unwrap_or_else(|| panic!("{variable} must be set")))
}

fn read(path: PathBuf) -> Vec<u8> {
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn data(name: &str) -> Vec<u8> {
    let folder = env_path("MCBENCH_DIFF_DATA");
    fixture::guard(&folder);
    read(folder.join(name))
}

fn data_text(name: &str) -> String {
    String::from_utf8(data(name)).unwrap_or_else(|_| panic!("{name} is not UTF-8"))
}

/// Mojang's client jar, read entry by entry.
struct ClientJar {
    bytes: Vec<u8>,
    entries: HashMap<String, Entry>,
}

impl ClientJar {
    fn open() -> ClientJar {
        let bytes = read(env_path("MCBENCH_DIFF_CLIENT_JAR"));
        let entries = jar::entries(&bytes)
            .unwrap()
            .into_iter()
            .map(|e| (e.name.clone(), e))
            .collect();
        ClientJar { bytes, entries }
    }

    fn entry(&self, name: &str) -> Vec<u8> {
        let entry = self
            .entries
            .get(name)
            .unwrap_or_else(|| panic!("The client jar has no {name}"));
        let stored = jar::stored(&self.bytes, entry);
        match entry.method {
            0 => stored.to_vec(),
            8 => {
                let mut out = Vec::with_capacity(entry.size as usize);
                flate2::read::DeflateDecoder::new(stored)
                    .read_to_end(&mut out)
                    .unwrap_or_else(|e| panic!("{name} does not inflate: {e}"));
                out
            }
            other => panic!("{name} has compression method {other}"),
        }
    }

    fn json(&self, name: &str) -> Value {
        serde_json::from_slice(&self.entry(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
    }

    /// Minecraft's colour map lookup (ColorMapColorUtil.get), its climate's
    /// floats widened to doubles as Biome does.
    fn colormap(&self, name: &str, temperature: f64, downfall: f64) -> u32 {
        let map =
            decode_png(&self.entry(&format!("assets/minecraft/textures/colormap/{name}.png")))
                .unwrap();
        let t = (temperature as f32 as f64).clamp(0.0, 1.0);
        let d = (downfall as f32 as f64).clamp(0.0, 1.0) * t;
        let at = (((((1.0 - d) * 255.0).trunc() as usize) << 8)
            | ((1.0 - t) * 255.0).trunc() as usize)
            * 4;
        let p = &map.rgba[at..at + 3];
        (p[0] as u32) << 16 | (p[1] as u32) << 8 | p[2] as u32
    }

    /// An item definition's first tint source's constant, as 0xRRGGBB.
    fn item_constant(&self, item: &str) -> u32 {
        let value = &self.json(&format!("assets/minecraft/items/{item}.json"))["model"]["tints"][0]
            ["value"];
        (value.as_i64().expect("a tint constant") & 0xffffff) as u32
    }
}

/// RedStoneWireBlock's colour of a power, in its floats
/// (ARGB.colorFromFloat).
fn dust(power: u32) -> u32 {
    let f = (power as f64 / 15.0) as f32 as f64;
    let fround = |v: f64| v as f32 as f64;
    let clamp = |v: f64| v.clamp(0.0, 1.0);
    let channels = [
        fround(fround(f * 0.6) + if f > 0.0 { 0.4 } else { 0.3 }),
        clamp(fround(fround(f * f * 0.7) - 0.5)),
        clamp(fround(fround(f * f * 0.6) - 0.7)),
    ];
    channels.iter().fold(0, |color, c| {
        (color << 8) | fround(c * 255.0).floor() as u32
    })
}

/// One state's line of the tints file.
#[derive(Debug, PartialEq)]
struct Line {
    world: Vec<u32>,
    single: u32,
    particle: u32,
    faces: String,
}

fn hex(text: &str) -> u32 {
    u32::from_str_radix(text, 16).unwrap()
}

#[test]
#[ignore = "needs MCBENCH_DIFF_DATA and MCBENCH_DIFF_CLIENT_JAR"]
fn the_tints_are_minecrafts_clients_in_the_worlds_biome() {
    let jar = ClientJar::open();
    let tints_file = data_text("tints.tsv");
    let states = BlockStates::new(
        &data_text("block-table.tsv"),
        &data_text("render-layers.tsv"),
    )
    .unwrap();
    let tints = BlockTints::new(&tints_file, &states).unwrap();
    let lines: Vec<Vec<&str>> = tints_file
        .split('\n')
        .filter(|l| !l.is_empty())
        .skip(1)
        .map(|l| l.split('\t').collect())
        .collect();
    let of = |block: &str, properties: &[(&str, &str)]| {
        let id = states.id_of(block, &props(properties)).unwrap() as usize;
        let line = &lines[id];
        Line {
            world: line[1].split(',').map(hex).collect(),
            single: hex(line[2]),
            particle: hex(line[3]),
            faces: line[4].to_string(),
        }
    };
    assert_eq!(tints.indices, 2, "block models use tint indices 0 and 1");

    let biome = jar.json("data/minecraft/worldgen/biome/plains.json");
    for key in ["grass_color", "foliage_color", "grass_color_modifier"] {
        assert!(
            biome["effects"].get(key).is_none_or(Value::is_null),
            "the biome has no grass or foliage override or modifier"
        );
    }
    let climate = |v: &Value| {
        (
            v["temperature"].as_f64().unwrap(),
            v["downfall"].as_f64().unwrap(),
        )
    };
    let (temperature, downfall) = climate(&biome);
    let grass = jar.colormap("grass", temperature, downfall);
    let foliage = jar.colormap("foliage", temperature, downfall);
    let dry_foliage = jar.colormap("dry_foliage", temperature, downfall);
    // The colour with no level: the grass item's tint source, grass at 0.5, 1.
    let grass_item = jar.json("assets/minecraft/items/grass_block.json");
    let (t, d) = climate(&grass_item["model"]["tints"][0]);
    let default_grass = jar.colormap("grass", t, d);
    assert_eq!(
        [grass, foliage, default_grass],
        [0x91bd59, 0x77ab2f, 0x7cbd6b],
        "plains' grass and foliage, and the default grass, as the wiki gives them"
    );

    // Grass blocks: top and side overlay tinted alike (index 0), their
    // particles their dirt, untinted.
    let model = jar.json("assets/minecraft/models/block/grass_block.json");
    let mut tinted: Vec<&str> = model["elements"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|element| element["faces"].as_object().unwrap().iter())
        .filter(|(_, face)| face["tintindex"].as_i64() == Some(0))
        .map(|(side, _)| side.as_str())
        .collect();
    tinted.sort_unstable();
    assert_eq!(
        tinted,
        ["east", "north", "south", "up", "west"],
        "its top and the overlay of each side are of tint index 0"
    );
    for snowy in ["false", "true"] {
        assert_eq!(
            of("minecraft:grass_block", &[("snowy", snowy)]),
            Line {
                world: vec![grass, grass],
                single: default_grass,
                particle: 0xffffff,
                faces: "0".into()
            }
        );
    }
    for block in [
        "minecraft:short_grass",
        "minecraft:fern",
        "minecraft:bush",
        "minecraft:potted_fern",
    ] {
        let l = of(block, &[]);
        assert_eq!(
            (l.world, l.single, l.particle),
            (vec![grass, grass], default_grass, grass),
            "{block}"
        );
    }
    for half in ["lower", "upper"] {
        for block in ["minecraft:tall_grass", "minecraft:large_fern"] {
            assert_eq!(
                of(block, &[("half", half)]).world,
                [grass, grass],
                "{block} {half}"
            );
        }
    }
    // Sugar cane: grass in the world, untinted with no level.
    let l = of("minecraft:sugar_cane", &[]);
    assert_eq!((l.world, l.single), (vec![grass, grass], 0xffffff));
    // Petals: the stems (index 1) grass, the flowers (index 0) untinted.
    let l = of("minecraft:pink_petals", &[]);
    assert_eq!((l.world, l.faces.as_str()), (vec![0xffffff, grass], "1"));

    // Leaves: the biome's foliage in the world, the default (the oak leaves
    // item's constant) with no level; spruce and birch their own constants.
    for block in [
        "minecraft:oak_leaves",
        "minecraft:jungle_leaves",
        "minecraft:acacia_leaves",
        "minecraft:dark_oak_leaves",
        "minecraft:mangrove_leaves",
        "minecraft:vine",
    ] {
        let l = of(block, &[]);
        assert_eq!(
            (l.world, l.single, l.particle),
            (
                vec![foliage, foliage],
                jar.item_constant("oak_leaves"),
                foliage
            ),
            "{block}"
        );
    }
    for leaves in ["spruce_leaves", "birch_leaves"] {
        let l = of(&format!("minecraft:{leaves}"), &[]);
        let constant = jar.item_constant(leaves);
        assert_eq!((l.world, l.single), (vec![constant, constant], constant));
    }
    // Cherry and pale oak leaves have tinted faces Minecraft leaves white.
    for block in [
        "minecraft:cherry_leaves",
        "minecraft:pale_oak_leaves",
        "minecraft:bamboo",
    ] {
        let l = of(block, &[]);
        assert_eq!(
            (l.world, l.single, l.faces.as_str()),
            (vec![0xffffff, 0xffffff], 0xffffff, "0"),
            "{block}"
        );
    }
    assert_eq!(
        of("minecraft:leaf_litter", &[]).world,
        [dry_foliage, dry_foliage],
        "leaf litter takes the dry foliage map"
    );

    // Lily pads: their colour in a level; with none, the lily pad item's.
    let l = of("minecraft:lily_pad", &[]);
    assert_eq!(
        (l.world, l.single),
        (vec![0x208030, 0x208030], jar.item_constant("lily_pad"))
    );

    // Water, its cauldron and bubble columns: the biome's water colour; with
    // no level, none.
    let water = biome["effects"]["water_color"].as_u64().unwrap() as u32;
    for (block, properties) in [
        ("minecraft:water", ("level", "0")),
        ("minecraft:water", ("level", "7")),
        ("minecraft:water_cauldron", ("level", "3")),
        ("minecraft:bubble_column", ("drag", "true")),
    ] {
        let l = of(block, &[properties]);
        assert_eq!(
            (l.world, l.single),
            (vec![water, water], 0xffffff),
            "{block}"
        );
    }

    // Redstone dust by power, by RedStoneWireBlock's own formula.
    for power in [0, 7, 15] {
        let l = of("minecraft:redstone_wire", &[("power", &power.to_string())]);
        assert_eq!(
            (l.world, l.single),
            (vec![dust(power), dust(power)], dust(power)),
            "dust of power {power}"
        );
    }
    assert_eq!([0, 7, 15].map(dust), [0x4c0000, 0xad0000, 0xff3200]);

    // Stems by age (ARGB.color(age * 32, 255 - age * 8, age * 4)); attached
    // ones the colour of the oldest.
    for block in ["minecraft:melon_stem", "minecraft:pumpkin_stem"] {
        for age in 0..=7u32 {
            let color = (age * 32) << 16 | (255 - age * 8) << 8 | (age * 4);
            assert_eq!(
                of(block, &[("age", &age.to_string())]).world,
                [color, color],
                "{block} {age}"
            );
        }
    }
    for block in [
        "minecraft:attached_melon_stem",
        "minecraft:attached_pumpkin_stem",
    ] {
        assert_eq!(
            of(block, &[("facing", "north")]).world,
            of("minecraft:melon_stem", &[("age", "7")]).world,
            "{block}"
        );
    }

    // Untinted blocks.
    for block in [
        "minecraft:stone",
        "minecraft:glass",
        "minecraft:oak_planks",
        "minecraft:dirt",
        "minecraft:lava",
        "minecraft:cauldron",
        "minecraft:torch",
    ] {
        assert_eq!(
            of(block, &[]),
            Line {
                world: vec![0xffffff, 0xffffff],
                single: 0xffffff,
                particle: 0xffffff,
                faces: "-".into()
            },
            "{block}"
        );
    }
}

/// The baked pack's colour of every state, by state name: the pack read by
/// the mesher's own reader (`mcbench_renderer::pack::Pack::read`, pack
/// format 7), each state's tint as 8-bit channels rounded as the pack's
/// floats were made from them.
fn pack_tints(bytes: &[u8]) -> HashMap<String, u32> {
    let pack = Pack::read(bytes).unwrap();
    (0..pack.states.len() as u32)
        .map(|state| {
            let rgb = pack.tints[pack.states[state as usize].tint as usize];
            let color = rgb.iter().fold(0, |color, channel| {
                (color << 8) | (*channel as f64 * 255.0 + 0.5).floor() as u32
            });
            (pack.name(state), color)
        })
        .collect()
}

#[test]
#[ignore = "needs MCBENCH_DIFF_DATA"]
fn the_live_viewers_pack_tints_every_state_minecrafts_client_tints() {
    let tints_file = data_text("tints.tsv");
    let pack = pack_tints(&data("mcbench-pack.bin"));
    let mut compared = 0;
    for line in tints_file.split('\n').filter(|l| !l.is_empty()).skip(1) {
        let fields: Vec<&str> = line.split('\t').collect();
        let (name, world, faces) = (fields[0], fields[1], fields[4]);
        let colors: Vec<&str> = world.split(',').collect();
        if faces == "-" && colors.iter().all(|c| *c == "ffffff") {
            continue;
        }
        let index = if faces == "-" {
            0
        } else {
            faces.split(',').next().unwrap().parse().unwrap()
        };
        let color = hex(colors[index]);
        let packed = *pack
            .get(name)
            .unwrap_or_else(|| panic!("the pack has no {name}"));
        assert_eq!(format!("{packed:x}"), format!("{color:x}"), "{name}");
        compared += 1;
    }
    assert!(compared > 1700, "{compared} tinted states");
    assert_eq!(
        pack["minecraft:redstone_wire[east=none,north=none,power=7,south=none,west=none]"],
        dust(7)
    );
}

#[test]
#[ignore = "needs MCBENCH_DIFF_DATA and MCBENCH_DIFF_TEXTURES"]
fn a_blocks_icon_is_its_particles_colour_and_a_name_of_no_block_has_none() {
    let textures = env_path("MCBENCH_DIFF_TEXTURES");
    let atlas_png = read(textures.join("atlas.png"));
    let atlas_map = read(textures.join("atlas_map.json"));
    let manifest: Manifest = serde_json::from_slice(&data("client-jar.json")).unwrap();
    let load = || {
        let states = BlockStates::new(
            &data_text("block-table.tsv"),
            &data_text("render-layers.tsv"),
        )
        .unwrap();
        let tints = BlockTints::new(&data_text("tints.tsv"), &states).unwrap();
        (states, tints)
    };
    let (states, tints) = load();
    let (icon_states, icon_tints) = load();
    let icons = Icons::new(
        &atlas_png,
        &atlas_map,
        &manifest.particles,
        icon_states,
        icon_tints,
    )
    .unwrap();
    let atlas = decode_png(&atlas_png).unwrap();
    // An icon is cut by its box's first three numbers, [x, y, size].
    let boxes: HashMap<String, Vec<u32>> = serde_json::from_slice(&atlas_map).unwrap();
    let particles: HashMap<&str, &str> = manifest
        .particles
        .iter()
        .map(|(b, t)| (b.as_str(), t.as_str()))
        .collect();
    for name in ["oak_leaves", "grass_block", "redstone_wire", "stone"] {
        let id = format!("minecraft:{name}");
        let color = tints.particle[states.id_of(&id, &[]).unwrap() as usize];
        let b = &boxes[particles[id.as_str()]];
        let at = [b[0], b[1], b[2]];
        let made = icons.block_icon(name).unwrap().expect("a block's icon");
        assert_eq!(
            decode_png(&made).unwrap().rgba,
            decode_png(&icon(&atlas, at, color).unwrap()).unwrap().rgba,
            "{name}"
        );
    }
    assert_eq!(
        tints.particle[states.id_of("minecraft:grass_block", &[]).unwrap() as usize],
        0xffffff,
        "a grass block breaks into dirt, untinted"
    );
    assert_eq!(
        icons.block_icon("glow_item_frame").unwrap(),
        None,
        "item frames have block definitions and are no blocks"
    );
    assert_eq!(icons.block_icon("not_a_block").unwrap(), None);
}
