//! The release side: the shipped files staged from an extractor run, the
//! client-jar manifest made from the whole jar, the archive, the lock.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use mcbench_setup::bundle::{self, FILES, Lock};
use mcbench_setup::icon::particle_texture;
use mcbench_setup::jar::{self, Entry, Manifest, texture_path};
use mcbench_world::block_states::BlockStates;
use serde_json::Value;
use sha2::Digest;

fn read(path: &Path) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))
}

fn json(path: &Path) -> Result<Value, String> {
    serde_json::from_slice(&read(path)?).map_err(|e| format!("{}: {e}", path.display()))
}

/// Where each shipped file is in an extractor's work folder (extract.sh).
fn source(work: &Path, name: &str) -> Result<PathBuf, String> {
    Ok(match name {
        "blocks.txt" | "loot.json" | "datafix.txt" | "night.json" => work.join("out").join(name),
        "block-table.tsv" => {
            let found: Vec<PathBuf> = std::fs::read_dir(work.join("out"))
                .map_err(|e| format!("{}: {e}", work.display()))?
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| {
                    p.file_name()
                        .is_some_and(|n| n.to_string_lossy().starts_with("block-table-"))
                })
                .collect();
            match found.as_slice() {
                [one] => one.clone(),
                _ => {
                    return Err(format!(
                        "{} holds {} block tables, not one",
                        work.join("out").display(),
                        found.len()
                    ));
                }
            }
        }
        "render-layers.tsv" | "tints.tsv" => work.join(name),
        "mcbench-pack.bin" | "mcbench-entities.json" | "settings.json" => {
            work.join("web").join(name)
        }
        "map-settings.json" => work.join("web/maps/live/settings.json"),
        other => return Err(format!("{other} has no source in the work folder")),
    })
}

/// The shipped files of the extractor's work folder `work`, by their
/// bundle names, into `folder`, with the client-jar manifest made from the
/// cache's jar.
pub fn stage(cache: &Path, work: &Path, folder: &Path) -> Result<(), String> {
    if folder.exists() {
        return Err(format!(
            "{} exists; the staged folder must be new",
            folder.display()
        ));
    }
    std::fs::create_dir_all(folder).map_err(|e| format!("{}: {e}", folder.display()))?;
    for name in FILES
        .iter()
        .filter(|n| !["client-jar.json", "blocks.json"].contains(n))
    {
        std::fs::copy(source(work, name)?, folder.join(name))
            .map_err(|e| format!("{name}: {e}"))?;
    }
    let report = read(&work.join("out/generated/reports/blocks.json"))?;
    std::fs::write(folder.join("blocks.json"), registry(&report)?)
        .map_err(|e| format!("blocks.json: {e}"))?;
    let manifest = manifest(cache, work, folder)?;
    let mut text = serde_json::to_string(&manifest).map_err(|e| e.to_string())?;
    text.push('\n');
    std::fs::write(folder.join("client-jar.json"), text)
        .map_err(|e| format!("client-jar.json: {e}"))?;
    let stored: u64 = manifest.entries.iter().map(|e| e.stored).sum();
    eprintln!(
        "client-jar.json: {} entries, {stored} bytes stored in the jar",
        manifest.entries.len()
    );
    Ok(())
}

/// The block registry today's app reads (`server/assets.js`), from Minecraft's
/// data generator's blocks report (`reports/blocks.json`), by misode/mcmeta's
/// rule (`main.py` at commit 3d63d61bcb7421a0d8ddda656bc851cc178067dd, "create
/// blocks and items report" and `create_summary`): by block id without
/// `minecraft:`, `[properties, the default state's properties]` (both `{}`
/// for a block without properties), each as the report orders it, the
/// blocks sorted by id, written as `json.dump(..., separators=(',', ':'))`
/// writes it (ASCII escapes, no spaces), then a newline.
pub fn registry(report: &[u8]) -> Result<String, String> {
    let report: serde_json::Map<String, Value> =
        serde_json::from_slice(report).map_err(|e| format!("the blocks report: {e}"))?;
    let mut blocks: Vec<(String, Value)> = Vec::new();
    for (key, data) in &report {
        let name = key.strip_prefix("minecraft:").unwrap_or(key).to_string();
        let entry = match data.get("properties") {
            Some(Value::Object(properties)) if !properties.is_empty() => {
                let default = data["states"]
                    .as_array()
                    .and_then(|states| {
                        states
                            .iter()
                            .find(|s| s.get("default") == Some(&Value::Bool(true)))
                    })
                    .and_then(|s| s.get("properties"))
                    .ok_or_else(|| {
                        format!("{key} has no default state's properties in the report")
                    })?;
                serde_json::json!([properties, default])
            }
            _ => serde_json::json!([{}, {}]),
        };
        blocks.push((name, entry));
    }
    // Python sorts str by code point; every id is ASCII.
    if let Some((name, _)) = blocks.iter().find(|(n, _)| !n.is_ascii()) {
        return Err(format!(
            "{name} is not ASCII, which this rule does not order"
        ));
    }
    blocks.sort_by(|a, b| a.0.cmp(&b.0));
    let map: serde_json::Map<String, Value> = blocks.into_iter().collect();
    let mut text = serde_json::to_string(&map).map_err(|e| e.to_string())?;
    if !text.is_ascii() {
        return Err("The registry holds text json.dump would escape".into());
    }
    text.push('\n');
    Ok(text)
}

/// The overworld's dimension type in the client jar (native/draw/src/assets.rs
/// `overworld`).
const OVERWORLD: &str = "data/minecraft/dimension_type/overworld.json";

/// The client-jar paths `night.json` names: the textures its sky draws
/// sample (`skyDraws.<time>[].samplers.<sampler>`, the sun's and the
/// moon's), which the live view's night reads from the client entries
/// (`native/draw/src/live/night.rs`).
fn night_paths(night: &Value) -> Result<BTreeSet<String>, String> {
    let mut paths = BTreeSet::new();
    for (time, draws) in night["skyDraws"]
        .as_object()
        .ok_or("night.json has no skyDraws")?
    {
        for draw in draws
            .as_array()
            .ok_or_else(|| format!("night.json: skyDraws {time} is not a list"))?
        {
            for (sampler, path) in draw["samplers"]
                .as_object()
                .ok_or_else(|| format!("night.json: a sky draw at {time} has no samplers"))?
            {
                let path = path.as_str().ok_or_else(|| {
                    format!(
                        "night.json: the sampler {sampler} of a sky draw at {time} is not a path"
                    )
                })?;
                paths.insert(path.to_string());
            }
        }
    }
    Ok(paths)
}

/// The client-jar manifest: every entry the app fetches (each material's
/// texture, each entity atlas texture, each block's particle texture, with
/// their animation files; Minecraft's atlas definitions and the trims'
/// palettes and base textures; the overworld's dimension type; the
/// textures `night.json` names), with where it lies in the jar and the
/// sha256 of its bytes.
fn manifest(cache: &Path, work: &Path, staged: &Path) -> Result<Manifest, String> {
    let jar_bytes = read(&cache.join("client.jar"))?;
    let version = json(&cache.join("1.21.10.json"))?;
    let client = &version["downloads"]["client"];
    let sha1 = jar::hex(&sha1_of(&jar_bytes));
    if client["sha1"].as_str() != Some(sha1.as_str())
        || client["size"].as_u64() != Some(jar_bytes.len() as u64)
    {
        return Err(format!(
            "The cache's client jar is not the one {} names",
            cache.join("1.21.10.json").display()
        ));
    }
    let all = jar::entries(&jar_bytes)?;
    let by_name: HashMap<&str, &Entry> = all.iter().map(|e| (e.name.as_str(), e)).collect();
    let entry_bytes = |name: &str| -> Result<Option<Vec<u8>>, String> {
        match by_name.get(name) {
            None => Ok(None),
            Some(e) => {
                let mut e = (*e).clone();
                let stored = jar::stored(&jar_bytes, &e);
                // The sha256 is the manifest's to make: check size and CRC32 only.
                let bytes = {
                    let mut out = Vec::new();
                    if e.method == 8 {
                        use std::io::Read;
                        flate2::read::DeflateDecoder::new(stored)
                            .read_to_end(&mut out)
                            .map_err(|x| format!("{name}: {x}"))?;
                    } else {
                        out = stored.to_vec();
                    }
                    out
                };
                if bytes.len() as u64 != e.size || crc32fast::hash(&bytes) != e.crc32 {
                    return Err(format!("{name} in the jar does not match its CRC32"));
                }
                e.sha256 = jar::hex(&sha2::Sha256::digest(&bytes));
                Ok(Some(bytes))
            }
        }
    };
    let textures = json(&work.join("web/maps/live/textures.json"))?;
    let materials: Vec<String> = textures
        .as_array()
        .ok_or("textures.json is not a list")?
        .iter()
        .map(|t| {
            t["resourcePath"]
                .as_str()
                .map(str::to_string)
                .ok_or("A texture has no resourcePath")
        })
        .collect::<Result<_, _>>()?;
    let entities = json(&work.join("web/mcbench-entities.json"))?;
    let entity_atlas: Vec<String> = entities["atlas"]["rects"]
        .as_object()
        .ok_or("mcbench-entities.json has no atlas rects")?
        .keys()
        .cloned()
        .collect();
    // Each block's particle texture, from the jar's block states and models.
    let states = BlockStates::new(
        &String::from_utf8(read(&staged.join("block-table.tsv"))?)
            .map_err(|_| "block-table.tsv is not UTF-8")?,
        &String::from_utf8(read(&staged.join("render-layers.tsv"))?)
            .map_err(|_| "render-layers.tsv is not UTF-8")?,
    )?;
    let load = |name: String| -> Option<Value> {
        entry_bytes(&name)
            .ok()
            .flatten()
            .and_then(|b| serde_json::from_slice(&b).ok())
    };
    let definitions = |name: &str| load(format!("assets/minecraft/blockstates/{name}.json"));
    let models = |name: &str| load(format!("assets/minecraft/models/{name}.json"));
    let mut particles = Vec::new();
    for id in states.names.keys() {
        let name = id
            .strip_prefix("minecraft:")
            .ok_or_else(|| format!("The block {id} is not Minecraft's"))?;
        particles.push((id.clone(), particle_texture(name, &definitions, &models)?));
    }
    let mut wanted: BTreeSet<String> = BTreeSet::new();
    let want_texture = |id: &str, wanted: &mut BTreeSet<String>| {
        let path = texture_path(id);
        if by_name.contains_key(path.as_str()) {
            let meta = format!("{path}.mcmeta");
            if by_name.contains_key(meta.as_str()) {
                wanted.insert(meta);
            }
            wanted.insert(path);
        }
    };
    for id in materials
        .iter()
        .chain(&entity_atlas)
        .chain(particles.iter().map(|(_, t)| t))
    {
        want_texture(id, &mut wanted);
    }
    for e in all
        .iter()
        .filter(|e| e.name.starts_with("assets/minecraft/atlases/"))
    {
        wanted.insert(e.name.clone());
        let atlas: Value = serde_json::from_slice(&entry_bytes(&e.name)?.expect("listed"))
            .map_err(|x| format!("{}: {x}", e.name))?;
        for source in atlas["sources"]
            .as_array()
            .ok_or_else(|| format!("{} has no sources", e.name))?
        {
            if source["type"] == "minecraft:paletted_permutations" {
                let mut ids: Vec<&str> = source["textures"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .collect();
                ids.extend(source["palette_key"].as_str());
                ids.extend(
                    source["permutations"]
                        .as_object()
                        .into_iter()
                        .flat_map(|p| p.values())
                        .filter_map(Value::as_str),
                );
                for id in ids {
                    let path = texture_path(id);
                    if by_name.contains_key(path.as_str()) {
                        wanted.insert(path);
                    }
                }
            }
        }
    }
    // The one data file the drawing crate reads from the client entries:
    // the overworld's dimension type, the height its translucent order lays
    // Minecraft's octree of sections over (native/draw/src/assets.rs
    // `overworld`, read by every Assets::load).
    if !by_name.contains_key(OVERWORLD) {
        return Err(format!("The client jar has no {OVERWORLD}"));
    }
    wanted.insert(OVERWORLD.to_string());
    for path in night_paths(&json(&staged.join("night.json"))?)? {
        if !by_name.contains_key(path.as_str()) {
            return Err(format!(
                "night.json names {path}, which the client jar does not hold"
            ));
        }
        wanted.insert(path);
    }
    let mut entries = Vec::new();
    for name in &wanted {
        let mut e = by_name[name.as_str()].clone();
        e.sha256 = jar::hex(&sha2::Sha256::digest(entry_bytes(name)?.expect("listed")));
        entries.push(e);
    }
    Ok(Manifest {
        minecraft: version["id"]
            .as_str()
            .ok_or("The version file has no id")?
            .to_string(),
        url: client["url"]
            .as_str()
            .ok_or("The version file has no client address")?
            .to_string(),
        size: jar_bytes.len() as u64,
        sha1,
        entries,
        materials,
        entity_atlas,
        particles,
    })
}

fn sha1_of(bytes: &[u8]) -> Vec<u8> {
    use sha1::Digest as _;
    sha1::Sha1::digest(bytes).to_vec()
}

/// The archive of a staged folder's files in the bundle's order.
pub fn tar(folder: &Path, archive: &Path) -> Result<(), String> {
    let files: Vec<(&str, Vec<u8>)> = FILES
        .iter()
        .map(|n| Ok((*n, read(&folder.join(n))?)))
        .collect::<Result<_, String>>()?;
    let list: Vec<(&str, &[u8])> = files.iter().map(|(n, b)| (*n, b.as_slice())).collect();
    std::fs::write(archive, bundle::tar(&list)?).map_err(|e| format!("{}: {e}", archive.display()))
}

/// The lock file's text for a bundle and the staged folder it was made of.
pub fn lock(bundle_file: &Path, folder: &Path, zstd: &str, url: &str) -> Result<(), String> {
    let bytes = read(bundle_file)?;
    let files = FILES
        .iter()
        .map(|n| {
            let b = read(&folder.join(n))?;
            Ok((n.to_string(), bundle::sha256(&b), b.len() as u64))
        })
        .collect::<Result<_, String>>()?;
    let lock = Lock {
        minecraft: "1.21.10".into(),
        sha256: bundle::sha256(&bytes),
        size: bytes.len() as u64,
        url: url.into(),
        zstd: zstd.into(),
        files,
    };
    print!("{}", lock.text());
    Ok(())
}

/// The bundle against a lock file: its sha256 and size, then every file's
/// sha256; and, given a staged folder, every file against it byte for byte.
pub fn check(bundle_file: &Path, lock_file: &Path, folder: Option<PathBuf>) -> Result<(), String> {
    let lock = Lock::parse(
        &String::from_utf8(read(lock_file)?).map_err(|_| "The lock file is not UTF-8")?,
    )?;
    let bytes = read(bundle_file)?;
    let mut failed = Vec::new();
    // The files first, so a bundle compressed by another zstd still says
    // whether its contents are the lock's.
    let files = bundle::untar(&bundle::decompress(&bytes)?)?;
    for ((name, data), (lock_name, sha256, size)) in files.iter().zip(&lock.files) {
        let same =
            name == lock_name && bundle::sha256(data) == *sha256 && data.len() as u64 == *size;
        println!(
            "{} {name} {}",
            if same { "same" } else { "DIFFERENT" },
            data.len()
        );
        if !same {
            failed.push(name.clone());
        }
        if let Some(folder) = &folder
            && read(&folder.join(name))? != *data
        {
            println!("DIFFERENT {name} from {}", folder.display());
            failed.push(name.clone());
        }
    }
    match bundle::open(&bytes, &lock) {
        Ok(_) => println!("same bundle {} {}", lock.sha256, lock.size),
        Err(e) => {
            println!("DIFFERENT bundle: {e}");
            failed.push("the bundle".into());
        }
    }
    if failed.is_empty() {
        Ok(())
    } else {
        Err(format!("differs from the lock: {}", failed.join(", ")))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_night_paths_are_every_sampler_of_every_sky_draw() {
        let night = serde_json::json!({ "skyDraws": {
            "6000": [{ "samplers": {} }, { "samplers": { "Sampler0": "assets/minecraft/textures/environment/sun.png" } }],
            "18000": [{ "samplers": { "Sampler0": "assets/minecraft/textures/environment/moon_phases.png" } },
                      { "samplers": { "Sampler0": "assets/minecraft/textures/environment/sun.png" } }],
        }});
        assert_eq!(
            super::night_paths(&night)
                .unwrap()
                .into_iter()
                .collect::<Vec<_>>(),
            [
                "assets/minecraft/textures/environment/moon_phases.png",
                "assets/minecraft/textures/environment/sun.png"
            ]
        );
        assert!(super::night_paths(&serde_json::json!({ "skyDraws": { "18000": [{}] } })).is_err());
    }

    #[test]
    fn night_json_is_staged_from_the_extractors_out_folder() {
        let work = std::path::Path::new("work");
        assert_eq!(
            super::source(work, "night.json").unwrap(),
            work.join("out/night.json")
        );
    }

    #[test]
    fn the_registry_takes_the_reports_order_and_sorts_the_blocks() {
        let report = br#"{"minecraft:stone":{"states":[{"default":true,"id":1}]},"minecraft:chest":{"properties":{"type":["single","left"],"facing":["north","south"]},"states":[{"id":2,"properties":{"type":"single","facing":"south"}},{"default":true,"id":3,"properties":{"type":"single","facing":"north"}}]}}"#;
        assert_eq!(
            super::registry(report).unwrap(),
            "{\"chest\":[{\"type\":[\"single\",\"left\"],\"facing\":[\"north\",\"south\"]},{\"type\":\"single\",\"facing\":\"north\"}],\"stone\":[{},{}]}\n"
        );
    }
}
