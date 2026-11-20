//! Every path the drawing crate opens in `Assets.client_jar` is in the
//! client-jar manifest, and so in `client-entries.zip`. `native/draw`
//! (native-stage2, `assets.rs` `entity_atlas`) opens, for each texture of the
//! entity capture's atlas (`mcbench-entities.json` `atlas.rects`: every
//! entity and block-entity texture the capture draws, all 36 entity kinds
//! and the block entities), `assets/<ns>/textures/<path>.png` and
//! `<that>.mcmeta` (the block textures come from `atlas.png`). The set is derived from the capture here, not by
//! instrumenting a run. A missing PNG is an error naming it there; a missing
//! `.mcmeta` is read as "not animated", so the manifest must hold each
//! `.mcmeta` the jar has for those textures: checked against the whole jar.
//! Every `Assets::load` also reads the overworld's dimension type
//! (`assets.rs` `overworld`: `data/minecraft/dimension_type/overworld.json`,
//! the height its translucent order is laid over), and fails without it.
//! The live view's night (`native/draw/src/live/night.rs` `read`) opens the
//! texture each sky draw at 18000 samples (`night.json`
//! `skyDraws["18000"][].samplers.Sampler0`, the sun's and the moon's), and
//! fails naming it when it is missing: each must be listed.
//!
//! Reads `MCBENCH_DIFF_DATA` (the shipped files) and
//! `MCBENCH_DIFF_CLIENT_JAR` (Mojang's whole client jar); fails when either
//! is set and its file is missing.

#[path = "../../agent/tests/common/fixture.rs"]
mod fixture;

use std::collections::BTreeSet;
use std::path::PathBuf;

use mcbench_setup::Data;

fn var(name: &str) -> PathBuf {
    let path = PathBuf::from(
        std::env::var(name).unwrap_or_else(|_| panic!("{name} names a file or folder")),
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

#[test]
#[ignore = "needs the shipped files and the client jar: set MCBENCH_DIFF_DATA and MCBENCH_DIFF_CLIENT_JAR"]
fn every_path_the_drawing_crate_opens_in_the_client_entries_is_in_the_manifest() {
    let data = Data::at(&shipped());
    let jar = std::fs::read(var("MCBENCH_DIFF_CLIENT_JAR")).unwrap();
    let in_jar: BTreeSet<String> = mcbench_setup::jar::entries(&jar)
        .unwrap()
        .into_iter()
        .map(|e| e.name)
        .collect();
    let manifest = data.manifest().unwrap();
    let listed: BTreeSet<&str> = manifest.entries.iter().map(|e| e.name.as_str()).collect();
    let capture: serde_json::Value =
        serde_json::from_slice(&data.read("mcbench-entities.json").unwrap()).unwrap();
    let rects = capture["atlas"]["rects"].as_object().unwrap();
    let mut missing = Vec::new();
    let mut animated = 0;
    for name in rects.keys() {
        let (space, path) = name.split_once(':').unwrap();
        let png = format!("assets/{space}/textures/{path}.png");
        let meta = format!("{png}.mcmeta");
        if !listed.contains(png.as_str()) {
            missing.push(png.clone());
        }
        if in_jar.contains(&meta) {
            animated += 1;
            if !listed.contains(meta.as_str()) {
                missing.push(meta);
            }
        }
    }
    let overworld = "data/minecraft/dimension_type/overworld.json";
    assert!(
        in_jar.contains(overworld),
        "{overworld} is not in the client jar"
    );
    if !listed.contains(overworld) {
        missing.push(overworld.to_string());
    }
    assert_eq!(manifest.entity_atlas.len(), rects.len());
    assert!(
        missing.is_empty(),
        "{} paths the drawing crate opens are not in the manifest: {:?}",
        missing.len(),
        &missing[..missing.len().min(10)]
    );
    println!(
        "{} entity textures, {animated} with an animation file, all in the manifest",
        rects.len()
    );
}

#[test]
#[ignore = "needs the shipped files and the client jar: set MCBENCH_DIFF_DATA and MCBENCH_DIFF_CLIENT_JAR"]
fn every_texture_the_live_views_night_opens_is_in_the_manifest() {
    let data = Data::at(&shipped());
    let jar = std::fs::read(var("MCBENCH_DIFF_CLIENT_JAR")).unwrap();
    let in_jar: BTreeSet<String> = mcbench_setup::jar::entries(&jar)
        .unwrap()
        .into_iter()
        .map(|e| e.name)
        .collect();
    let manifest = data.manifest().unwrap();
    let listed: BTreeSet<&str> = manifest.entries.iter().map(|e| e.name.as_str()).collect();
    let night: serde_json::Value =
        serde_json::from_slice(&data.read("night.json").unwrap()).unwrap();
    // As night::read opens them: each sky draw at 18000 with a Sampler0.
    let opened: BTreeSet<&str> = night["skyDraws"]["18000"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|draw| draw["samplers"]["Sampler0"].as_str())
        .collect();
    assert!(
        !opened.is_empty(),
        "night.json's sky pass at 18000 samples no texture"
    );
    for path in &opened {
        assert!(in_jar.contains(*path), "{path} is not in the client jar");
        assert!(
            listed.contains(path),
            "{path}, which the live view's night opens, is not in the manifest"
        );
    }
    println!(
        "{} night textures, all in the manifest: {opened:?}",
        opened.len()
    );
}

// The bake's own atlas layout, twice from the same entries: the same atlas
// and map, byte for byte; their sha256s are printed to compare machines.
#[test]
#[ignore = "needs the shipped files and the client jar: set MCBENCH_DIFF_DATA and MCBENCH_DIFF_CLIENT_JAR"]
fn the_own_atlas_is_the_same_bytes_every_time() {
    let data = Data::at(&shipped());
    let jar = std::fs::read(var("MCBENCH_DIFF_CLIENT_JAR")).unwrap();
    let manifest = data.manifest().unwrap();
    let bake = || {
        let bytes = manifest
            .entries
            .iter()
            .map(|e| {
                (
                    e.name.clone(),
                    e.unpack(mcbench_setup::jar::stored(&jar, e)).unwrap(),
                )
            })
            .collect();
        let files = mcbench_setup::bake::Files::decode(bytes, 2).unwrap();
        let baked = mcbench_setup::bake::bake(&manifest, &files).unwrap();
        (baked.atlas_png, baked.atlas_map)
    };
    let (png, map) = bake();
    assert_eq!(bake(), (png.clone(), map.clone()));
    println!(
        "atlas.png {} bytes sha256 {}; atlas_map.json {} bytes sha256 {}",
        png.len(),
        mcbench_setup::bundle::sha256(&png),
        map.len(),
        mcbench_setup::bundle::sha256(&map)
    );
}

// The entries zip is complete: for every texture the manifest lists, its
// animation file is listed if and only if the client jar has one (the
// drawing crate reads a missing `.mcmeta` as "not animated"), and the
// manifest lists no animation file without its texture.
#[test]
#[ignore = "needs the shipped files and the client jar: set MCBENCH_DIFF_DATA and MCBENCH_DIFF_CLIENT_JAR"]
fn every_listed_texture_has_its_animation_file_listed_exactly_when_the_jar_has_one() {
    let data = Data::at(&shipped());
    let jar = std::fs::read(var("MCBENCH_DIFF_CLIENT_JAR")).unwrap();
    let in_jar: BTreeSet<String> = mcbench_setup::jar::entries(&jar)
        .unwrap()
        .into_iter()
        .map(|e| e.name)
        .collect();
    let manifest = data.manifest().unwrap();
    let listed: BTreeSet<&str> = manifest.entries.iter().map(|e| e.name.as_str()).collect();
    let textures: Vec<&str> = listed
        .iter()
        .copied()
        .filter(|n| n.ends_with(".png"))
        .collect();
    let (mut with, mut without, mut wrong) = (0, 0, Vec::new());
    for png in &textures {
        let meta = format!("{png}.mcmeta");
        match (in_jar.contains(&meta), listed.contains(meta.as_str())) {
            (true, true) => with += 1,
            (false, false) => without += 1,
            (true, false) => wrong.push(format!("{meta}: the jar has it, the manifest does not")),
            (false, true) => wrong.push(format!("{meta}: the manifest lists it, the jar has none")),
        }
    }
    for meta in listed.iter().filter(|n| n.ends_with(".mcmeta")) {
        if !listed.contains(meta.trim_end_matches(".mcmeta")) {
            wrong.push(format!("{meta} is listed without its texture"));
        }
    }
    println!(
        "{} textures listed: {with} with an animation file, {without} without",
        textures.len()
    );
    assert!(
        wrong.is_empty(),
        "{} mismatches: {:?}",
        wrong.len(),
        &wrong[..wrong.len().min(10)]
    );
}
