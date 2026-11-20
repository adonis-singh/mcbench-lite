//! The differential against today's server, for every block of the table:
//! the icons (`server/block-icon.js`), the tints (`block-tints.js`), the
//! facts (`minecraft-facts.js`) and the block registry (`assets.js`
//! getBlockRegistry: misode/mcmeta's `blocks.json`, shipped), from what
//! `tests/node/dump.js` printed of today's; and the shipped registry
//! against the engine's table: the same blocks, properties, values and
//! defaults.
//!
//! The data is never committed, so the test reads environment variables
//! and fails when one is set and a file is missing: `MCBENCH_DIFF_DATA`
//! (the shipped files, by their bundle names), `MCBENCH_DIFF_TEXTURES` (the
//! bake's folder) and `MCBENCH_DIFF_NODE` (dump.js's output for the same
//! Minecraft).

#[path = "../../agent/tests/common/fixture.rs"]
mod fixture;

use std::collections::BTreeMap;
use std::path::PathBuf;

use mcbench_setup::Data;
use mcbench_setup::bake::decode_png;
use mcbench_setup::bundle::sha256;
use mcbench_setup::icon::Icons;
use mcbench_setup::registry::block_registry;
use mcbench_setup::tints::BlockTints;
use mcbench_world::block_states::BlockStates;
use mcbench_world::facts::MinecraftFacts;
use serde_json::Value;

fn var(name: &str) -> PathBuf {
    let path = PathBuf::from(
        std::env::var(name).unwrap_or_else(|_| panic!("{name} names a folder or file")),
    );
    assert!(path.exists(), "{name}: {} does not exist", path.display());
    path
}

/// The folder `MCBENCH_DIFF_DATA` names, refused when it holds a key file.
fn shipped() -> PathBuf {
    let folder = var("MCBENCH_DIFF_DATA");
    fixture::guard(&folder);
    folder
}

fn text(path: PathBuf) -> String {
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

#[test]
#[ignore = "needs the shipped files, the bake and Node's dump: set MCBENCH_DIFF_DATA, MCBENCH_DIFF_TEXTURES and MCBENCH_DIFF_NODE"]
fn icons_tints_facts_and_the_registry_of_every_block_are_todays() {
    let data = Data::at(&shipped());
    let textures = var("MCBENCH_DIFF_TEXTURES");
    let node = text(var("MCBENCH_DIFF_NODE"));
    let table = data.text("block-table.tsv").unwrap();
    let layers = data.text("render-layers.tsv").unwrap();
    let states = BlockStates::new(&table, &layers).unwrap();
    let tints = BlockTints::new(&data.text("tints.tsv").unwrap(), &states).unwrap();
    let facts = MinecraftFacts::new(&data.text("blocks.txt").unwrap()).unwrap();
    let manifest = data.manifest().unwrap();
    let icons = Icons::new(
        &std::fs::read(textures.join("atlas.png")).unwrap(),
        &std::fs::read(textures.join("atlas_map.json")).unwrap(),
        &manifest.particles,
        BlockStates::new(&table, &layers).unwrap(),
        BlockTints::new(&data.text("tints.tsv").unwrap(), &states).unwrap(),
    )
    .unwrap();
    let lines: Vec<Value> = node
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let (last, blocks) = lines.split_last().unwrap();
    let mut counts: BTreeMap<&str, (usize, Vec<String>)> = BTreeMap::new();
    let mut check = |what: &'static str, same: bool, id: &str| {
        let c = counts.entry(what).or_default();
        if same {
            c.0 += 1;
        } else {
            c.1.push(id.to_string());
        }
    };
    assert_eq!(
        blocks.len(),
        states.defaults.len(),
        "a line for every block of the table"
    );
    for line in blocks {
        let id = line["id"].as_str().unwrap();
        let name = id.strip_prefix("minecraft:").unwrap();
        // The icon: the same pixels, or a failure where today's fails.
        let ours = icons.block_icon(name);
        let same = match (&line["icon"], &ours) {
            (Value::Object(o), Err(_)) if o.contains_key("error") => true,
            (Value::Object(o), Ok(Some(png))) if o.contains_key("rgba") => {
                let image = decode_png(png).unwrap();
                o["width"] == image.width && o["rgba"] == sha256(&image.rgba).as_str()
            }
            _ => false,
        };
        check("icon", same, id);
        let state = states.id_of(id, &[]).unwrap() as usize;
        check(
            "tint of its particles",
            line["tintParticle"] == tints.particle[state],
            id,
        );
        let world: Vec<u32> =
            tints.world[state * tints.indices..(state + 1) * tints.indices].to_vec();
        check(
            "tints of its faces",
            line["tintWorld"] == serde_json::json!(world),
            id,
        );
        check(
            "class",
            line["className"] == facts.class_name(id).unwrap(),
            id,
        );
        let supertypes: Vec<&str> = line["supertypes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s.as_str().unwrap())
            .collect();
        check(
            "supertypes",
            supertypes.iter().all(|s| facts.is(id, s).unwrap())
                && !facts.is(id, "NoSuchBlock").unwrap(),
            id,
        );
        let defaults: Vec<(String, String)> = line["defaults"]
            .as_object()
            .unwrap()
            .iter()
            .map(|(k, v)| (k.clone(), v.as_str().unwrap().to_string()))
            .collect();
        check("defaults", defaults == facts.defaults(id).unwrap(), id);
        check("air", line["air"] == facts.is_air(id).unwrap(), id);
        check(
            "default state",
            line["stateId"] == facts.state_id(id, &[]).unwrap().unwrap(),
            id,
        );
        let held: Vec<Vec<i32>> = facts
            .held_by(id, &[])
            .unwrap()
            .iter()
            .map(|s| s.to_vec())
            .collect();
        check("held by", line["heldBy"] == serde_json::json!(held), id);
    }
    let tags = last["itemTags"].as_object().unwrap();
    for (tag, members) in tags {
        let members: Vec<&str> = members
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m.as_str().unwrap())
            .collect();
        check("item tag", members == facts.item_tag(tag).unwrap(), tag);
    }
    check(
        "survives, by state",
        last["survives"] == sha256(&facts.survives).as_str(),
        "all states",
    );
    check(
        "held, by state",
        last["held"] == sha256(&facts.held).as_str(),
        "all states",
    );
    check(
        "the tints' renderer bytes",
        last["tintBytes"] == sha256(&tints.bytes()).as_str(),
        "all states",
    );
    let registry = block_registry(&data).unwrap();
    check(
        "the block registry's sha256",
        last["registry"] == sha256(registry.as_bytes()).as_str(),
        "blocks.json",
    );
    // The shipped registry against the engine's table, property order aside.
    let parsed: serde_json::Map<String, Value> = serde_json::from_str(&registry).unwrap();
    check(
        "the registry's blocks are the table's",
        parsed.len() == facts.blocks().count(),
        "blocks.json",
    );
    let mut reordered = 0;
    for (id, properties) in facts.blocks() {
        let entry = &parsed[id.strip_prefix("minecraft:").unwrap()];
        let (values, defaults) = (entry[0].as_object().unwrap(), entry[1].as_object().unwrap());
        let same = values.len() == properties.len()
            && defaults.len() == properties.len()
            && properties.iter().all(|(name, list, default)| {
                values.get(*name) == Some(&serde_json::json!(list))
                    && defaults.get(*name) == Some(&serde_json::json!(default))
            });
        check(
            "the registry's properties, values and defaults are the table's",
            same,
            id,
        );
        reordered += usize::from(
            values
                .keys()
                .map(String::as_str)
                .ne(properties.iter().map(|(n, _, _)| *n)),
        );
    }
    println!("{reordered} blocks list their properties in another order than the table");
    let report: Vec<String> = counts
        .iter()
        .map(|(what, (same, different))| {
            format!(
                "{what}: {same} same, {} different {:?}",
                different.len(),
                &different[..different.len().min(5)]
            )
        })
        .collect();
    println!("{}", report.join("\n"));
    assert!(
        counts.values().all(|(_, different)| different.is_empty()),
        "{}",
        report.join("\n")
    );
}
