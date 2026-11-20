//! The Rust side of the differential harness: every stored world of a data
//! folder clone through this crate, compared with what `tests/node/diff.js`
//! made of it through today's server modules.
//!
//! `MCBENCH_DIFF_DATA=<data folder clone>` and `MCBENCH_NODE_DIFF=<checkout
//! with node_modules>` turn it on (it is ignored without them, and fails
//! when they are set and something is missing). It runs `node diff.js` into
//! `MCBENCH_DIFF_OUT` (a scratch folder; a folder that already holds
//! `node.json` is read as it is, so a rerun of the Rust side does not rerun
//! Node) or into a folder under the target directory. Run it with
//! `--release --nocapture`: it prints a table of what it compared.

mod common;

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use common::{Report, attempt, json, sha};
use mcbench_world::bench::BenchComposer;
use mcbench_world::block_states::BlockStates;
use mcbench_world::facts::MinecraftFacts;
use mcbench_world::live_views::LiveViews;
use mcbench_world::selection::{describe_selection, read_selection};
use mcbench_world::{BlockBox, World, WorldManifest, region, transfer, zip};
use serde_json::{Value, json};

struct Data {
    root: PathBuf,
}

impl Data {
    fn load(&self, owner: &str, manifest: &str) -> Result<World, String> {
        let folder = self.root.join("worlds").join(owner).join("chunks");
        World::from_manifest(&WorldManifest(manifest.to_string()), &mut |hash: &str| {
            std::fs::read(folder.join(format!("{hash}.json"))).map_err(|e| format!("{hash}: {e}"))
        })
    }
}

/// `manifestOf`: the manifest's hash, the files written (name and content
/// hash, in order) and how many.
fn manifest_of(world: &World) -> Result<Value, String> {
    let mut files = Vec::new();
    let manifest = world.persist(&mut |hash: &str, bytes: &[u8]| {
        files.push(format!("{hash} {}", sha(bytes)));
        Ok(())
    })?;
    Ok(
        json!({ "manifest": sha(&manifest.0), "files": sha(files.join("\n")), "written": files.len() }),
    )
}

fn bounds_json(bounds: Option<BlockBox>) -> Value {
    bounds.map_or(Value::Null, |b| json!({ "min": b.min, "max": b.max }))
}

fn palette_json(world: &World) -> String {
    let manifest = world
        .persist(&mut |_: &str, _: &[u8]| Ok(()))
        .expect("a manifest");
    let text = manifest.0;
    let start = text.find(",\"palette\":").expect("a palette") + ",\"palette\":".len();
    let end = text.find(",\"chunks\":").expect("chunks");
    text[start..end].to_string()
}

fn commands(
    world: &World,
    previous: Option<&World>,
    y: i32,
    facts: &MinecraftFacts,
) -> Result<Value, String> {
    world
        .place_commands(previous, y, None, facts)
        .map(|c| json!(sha(c.0)))
}

/// The child's edits (`edit` in diff.js).
fn edit(world: &mut World) -> Vec<Value> {
    let b = world.bounds().unwrap_or(BlockBox {
        min: [0; 3],
        max: [0; 3],
    });
    let none = |r: Result<(), String>| attempt(r.map(|_| Value::Null));
    let state = |id: &str, props: &[(&str, &str)]| mcbench_world::BlockState::new(id, props);
    vec![
        none(world.set(
            [b.max[0] + 1, b.min[1], b.min[2]],
            state("minecraft:gold_block", &[]),
        )),
        none(world.set(b.min, state("minecraft:air", &[]))),
        none(world.fill(
            BlockBox {
                min: [b.min[0], b.max[1] + 1, b.min[2]],
                max: [b.min[0] + 20, b.max[1] + 3, b.min[2] + 20],
            },
            state(
                "minecraft:oak_stairs",
                &[("half", "top"), ("facing", "east")],
            ),
        )),
        none(world.fill(
            BlockBox {
                min: b.min,
                max: [b.min[0] + 2, b.min[1] + 2, b.min[2] + 2],
            },
            state("minecraft:air", &[]),
        )),
    ]
}

fn node_output() -> Option<(PathBuf, PathBuf)> {
    let data = common::folder("MCBENCH_DIFF_DATA")?;
    common::fixture::guard(&data);
    let checkout = common::folder("MCBENCH_NODE_DIFF")
        .expect("MCBENCH_NODE_DIFF names the checkout whose server/ and node_modules Node runs");
    let out = std::env::var_os("MCBENCH_DIFF_OUT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("world-diff"));
    if !out.join("node.json").is_file() {
        common::node(&checkout, "diff.js", &[&data, &common::blocks_txt(), &out]);
    }
    Some((data, out))
}

#[test]
#[ignore = "needs MCBENCH_DIFF_DATA (a data folder clone) and MCBENCH_NODE_DIFF (a checkout with node_modules)"]
fn every_stored_world_is_what_node_makes_of_it() {
    let Some((root, out)) = node_output() else {
        panic!("set MCBENCH_DIFF_DATA and MCBENCH_NODE_DIFF to run the differential harness");
    };
    let node = json(&out.join("node.json"));
    let data = Data { root: root.clone() };
    let facts = Arc::new(
        MinecraftFacts::new(&std::fs::read_to_string(common::blocks_txt()).expect("blocks.txt"))
            .expect("the engine's table"),
    );
    let tables = &node["tables"];
    let states = BlockStates::new(
        &std::fs::read_to_string(root.join(tables["table"].as_str().expect("a table")))
            .expect("the block table"),
        &std::fs::read_to_string(root.join(tables["layers"].as_str().expect("layers")))
            .expect("the render layers"),
    )
    .expect("the block states");
    let mut report = Report::default();
    report.check(
        "block states: page bytes",
        "",
        &tables["bytes"],
        &json!(sha(states.bytes())),
    );
    let names: Vec<(&String, &String)> = states.names.iter().collect();
    report.check(
        "block states: names",
        "",
        &tables["names"],
        &json!(sha(serde_json::to_string(&names).expect("JSON"))),
    );

    let worlds = node["worlds"].as_array().expect("worlds");
    let manifest_text = |entry: &Value| {
        std::fs::read_to_string(out.join(entry["file"].as_str().expect("a file")))
            .expect("a manifest")
    };
    for entry in worlds {
        let owner = entry["owner"].as_str().expect("an owner");
        let what = format!("world {} ({owner})", entry["n"]);
        let text = manifest_text(entry);
        let mut world = data.load(owner, &text).expect("a stored world loads");
        let read = json!({ "size": world.size(), "chunkCount": world.chunk_count(), "paletteLength": world.palette().len(), "palette": sha(palette_json(&world)) });
        let node_read = json!({ "size": entry["size"], "chunkCount": entry["chunkCount"], "paletteLength": entry["paletteLength"], "palette": entry["palette"] });
        report.check("read: size, chunks, palette", &what, &node_read, &read);
        let restored = world
            .persist(&mut |_: &str, _: &[u8]| Err("a loaded chunk was written".into()))
            .map(|m| m.0 == text);
        report.check(
            "write: stored manifest written again unchanged",
            &what,
            &entry["restored"],
            &json!(restored == Ok(true)),
        );
        let rewritten = world
            .persist(&mut |_: &str, _: &[u8]| Err("a loaded chunk was written".into()))
            .map(|m| json!(sha(m.0)));
        report.check(
            "write: stored world's manifest written again",
            &what,
            &entry["rewritten"],
            &attempt(rewritten),
        );
        let cold = transfer::encode_world(&world);
        report.check(
            "transfer: encode, cold",
            &what,
            &entry["encodeCold"],
            &json!(sha(&cold)),
        );
        report.check(
            "read: bounds",
            &what,
            &entry["bounds"],
            &bounds_json(world.bounds()),
        );
        report.check(
            "transfer: encode, bounds known",
            &what,
            &entry["encodeWarm"],
            &json!(sha(transfer::encode_world(&world))),
        );
        report.check(
            "read: render LOD",
            &what,
            &entry["lod"],
            &world.render_lod().map_or(
                Value::Null,
                |(axis, scale)| json!({ "axis": axis, "scale": scale }),
            ),
        );
        let blueprint = world.blueprint(64).expect("a blueprint");
        report.check(
            "blueprint: bytes",
            &what,
            &entry["blueprint"],
            &json!(sha(&blueprint.0)),
        );
        let from = World::from_blueprint(&blueprint, 64, None).and_then(|w| manifest_of(&w));
        report.check(
            "blueprint: world of the blueprint, manifest and files",
            &what,
            &entry["blueprintWorld"],
            &attempt(from),
        );
        let fresh = transfer::decode_world(&transfer::encode_world(&world)).expect("decodes");
        report.check(
            "write: unstored world persisted, manifest and files",
            &what,
            &entry["fresh"],
            &manifest_of(&fresh).expect("persists"),
        );
        report.check(
            "place commands: fresh, y 64",
            &what,
            &entry["commands"],
            &attempt(commands(&world, None, 64, &facts)),
        );
        report.check(
            "place commands: fresh, y 0",
            &what,
            &entry["commands0"],
            &attempt(commands(&world, None, 0, &facts)),
        );
        let ids = states
            .of_palette(world.palette())
            .map(|p| json!(sha(serde_json::to_string(&p.ids).expect("JSON"))));
        report.check(
            "block states: palette state ids",
            &what,
            &entry["paletteStates"],
            &attempt(ids),
        );
        let sent = transfer::encode_world(&world);
        let mut child = transfer::decode_world(&sent).expect("decodes");
        let palette_start = child.palette().len();
        report.check(
            "transfer: child's edits",
            &what,
            &entry["edits"],
            &json!(edit(&mut child)),
        );
        let changes = transfer::encode_changes(&child, palette_start);
        report.check(
            "transfer: change set",
            &what,
            &entry["changes"]["sha"],
            &json!(sha(&changes)),
        );
        let node_changes =
            std::fs::read(out.join(entry["changes"]["file"].as_str().expect("a file")))
                .expect("Node's change set");
        world.finalize();
        let mut clone = world.clone_world().expect("a finalized world clones");
        let applied = transfer::apply_changes(
            &mut clone,
            &node_changes,
            world.palette().len(),
            &mut transfer::take_as_sent,
            64,
        );
        let applied = match applied {
            Err(error) => json!({ "error": error }),
            Ok(()) => {
                let mut value = manifest_of(&clone).expect("persists");
                let diff = clone.diff(&world).expect("a direct clone");
                let object = value.as_object_mut().expect("an object");
                object.insert("diff".into(), json!({ "added": diff.added, "removed": diff.removed, "changed": diff.changed }));
                object.insert("size".into(), json!(clone.size()));
                object.insert("bounds".into(), bounds_json(clone.bounds()));
                value
            }
        };
        report.check(
            "transfer: Node's change set applied, manifest, files, diff",
            &what,
            &entry["applied"],
            &applied,
        );
    }

    // Live views and incremental commands along each chat's message worlds.
    for view in node["views"].as_array().expect("views") {
        let owner = view["owner"].as_str().expect("an owner");
        let current: Rc<RefCell<Option<Arc<World>>>> = Rc::new(RefCell::new(None));
        let source = current.clone();
        let mut clock = 0.0;
        let mut live = LiveViews::new(
            move |_: &str| Ok((source.borrow().clone().expect("a world"), Value::Null)),
            64,
            Box::new(move || {
                clock += 1000.0;
                clock
            }),
            facts.clone(),
        );
        let key = format!("chat-{owner}");
        let mut since = None;
        let mut previous: Option<Arc<World>> = None;
        for step in view["steps"].as_array().expect("steps") {
            let n = step["n"].as_u64().expect("a world number") as usize;
            let mut world = data.load(owner, &manifest_text(&worlds[n])).expect("loads");
            world.finalize();
            let world = Arc::new(world);
            *current.borrow_mut() = Some(world.clone());
            let what = format!("chat {owner} world {n}");
            report.check(
                "live views: describe",
                &what,
                &step["describe"],
                &attempt(live.describe(&key)),
            );
            let imports = live.imports(&key, since).map(|i| {
                since = Some(i.version);
                json!({ "version": i.version, "fresh": i.fresh, "commands": sha(&i.commands), "blockCount": i.block_count })
            });
            report.check(
                "live views: imports",
                &what,
                &step["imports"],
                &attempt(imports),
            );
            if let Some(previous) = &previous {
                report.check(
                    "place commands: incremental",
                    &what,
                    &step["incremental"],
                    &attempt(commands(&world, Some(previous), 64, &facts)),
                );
            }
            previous = Some(world);
        }
    }

    // Bench compositions.
    for bench in node["benches"].as_array().expect("benches") {
        let ids: Vec<String> = bench["members"]
            .as_array()
            .expect("members")
            .iter()
            .map(|m| m.as_str().expect("an id").to_string())
            .collect();
        let current: Vec<World> = ids
            .iter()
            .map(|id| {
                let row = worlds
                    .iter()
                    .find(|w| w["owner"] == id.as_str() && w["kinds"].get("conversation").is_some())
                    .expect("a member's world");
                let mut world = data.load(id, &manifest_text(row)).expect("loads");
                world.finalize();
                world
            })
            .collect();
        let first = bench["first"].as_u64().map(|n| {
            let mut world = data
                .load(&ids[0], &manifest_text(&worlds[n as usize]))
                .expect("loads");
            world.finalize();
            world
        });
        let entries = |swap: bool| -> Vec<(String, &World)> {
            ids.iter()
                .zip(&current)
                .enumerate()
                .map(|(i, (id, w))| {
                    (
                        id.clone(),
                        if i == 0 && swap {
                            first.as_ref().unwrap_or(w)
                        } else {
                            w
                        },
                    )
                })
                .collect()
        };
        let mut composer = BenchComposer::new();
        for (round, swap) in [false, true, false].into_iter().enumerate() {
            let result = composer.compose(&entries(swap)).map(|(world, layout)| {
                let mut value = json!({ "layout": layout, "size": world.size(), "revision": world.revision(), "encode": sha(transfer::encode_world(&world)) });
                let manifest = manifest_of(&world).expect("persists");
                value.as_object_mut().expect("an object").extend(manifest.as_object().expect("an object").clone());
                value
            });
            report.check(
                "bench: composed world and layout",
                &format!("bench {} step {round}", bench["id"]),
                &bench["results"][round],
                &attempt(result),
            );
        }
    }

    // Selections.
    let mut chat_worlds = std::collections::HashMap::new();
    for selection in node["selections"].as_array().expect("selections") {
        let owner = selection["owner"].as_str().expect("an owner");
        let world = chat_worlds.entry(owner.to_string()).or_insert_with(|| {
            let row = worlds
                .iter()
                .find(|w| w["owner"] == owner && w["kinds"].get("conversation").is_some())
                .expect("the chat's world");
            data.load(owner, &manifest_text(row)).expect("loads")
        });
        let (min, max) = match selection.get("part") {
            Some(part) => {
                let part = if part.is_null() { &Value::Null } else { part };
                let read = read_selection(part)
                    .map(|s| serde_json::to_value(s).expect("JSON"))
                    .map_err(|e| e.0);
                report.check("selection: read", owner, &selection["read"], &attempt(read));
                (
                    part.get("min").cloned().unwrap_or(Value::Null),
                    part.get("max").cloned().unwrap_or(Value::Null),
                )
            }
            None => (selection["min"].clone(), selection["max"].clone()),
        };
        let described = describe_selection(world, &min, &max, &states.names)
            .map(|d| serde_json::to_value(d).expect("JSON"))
            .map_err(|e| e.0);
        report.check(
            "selection: describe",
            &format!("{owner} {min} {max}"),
            &selection["describe"],
            &attempt(described),
        );
    }

    // Region folders and archives.
    for entry in node["regions"].as_array().expect("regions") {
        let folder = root.join(entry["folder"].as_str().expect("a folder"));
        let read = region::read_block_states(&folder, &facts);
        let rust = match read {
            Ok(states) => {
                json!({ "count": states.len(), "states": sha(states.iter().map(|(k, v)| format!("{k} {v}")).collect::<Vec<_>>().join("\n")) })
            }
            Err(error) => json!({ "error": error }),
        };
        let node_value = match entry.get("error") {
            Some(error) => json!({ "error": error }),
            None => json!({ "count": entry["count"], "states": entry["states"] }),
        };
        report.check(
            "region: blocks of a region folder",
            &folder.display().to_string(),
            &node_value,
            &rust,
        );
    }
    for entry in node["archives"].as_array().expect("archives") {
        let file = root.join(entry["file"].as_str().expect("a file"));
        let small = |name: &str| {
            [".json", ".txt", ".mcmeta", ".properties", ".MF", ".yml"]
                .iter()
                .any(|e| name.ends_with(e))
        };
        let rust = zip::zip_names(&file).and_then(|names| {
            let read = zip::read_zip_entries(&file, small)?;
            Ok(json!({
                "names": sha(names.join("\n")), "count": names.len(),
                "entries": sha(read.iter().map(|(n, b)| format!("{n} {}", sha(b))).collect::<Vec<_>>().join("\n")), "read": read.len(),
            }))
        });
        let rust = rust.unwrap_or_else(|error| json!({ "error": error }));
        let node_value = match entry.get("error") {
            Some(error) => json!({ "error": error }),
            None => {
                json!({ "names": entry["names"], "count": entry["count"], "entries": entry["entries"], "read": entry["read"] })
            }
        };
        report.check(
            "zip: names and entries of an archive",
            &file.display().to_string(),
            &node_value,
            &rust,
        );
    }

    // The data pack and localeCompare.
    for (file, text) in mcbench_world::dimension::dimension_pack() {
        let node_text = node["pack"]
            .as_array()
            .expect("pack")
            .iter()
            .find(|p| p[0] == format!("world/{file}").as_str())
            .expect("the file")[1]
            .clone();
        report.check("dimension: data pack", file, &node_text, &json!(text));
    }
    let mut words: Vec<String> = node["collation"]["words"]
        .as_array()
        .expect("words")
        .iter()
        .map(|w| w.as_str().expect("a word").to_string())
        .collect();
    words.sort_by(|a, b| mcbench_world::js::locale_cmp(a, b).expect("printable ASCII"));
    report.check(
        "js: localeCompare order",
        "3000 words",
        &node["collation"]["sorted"],
        &json!(words),
    );
    report.finish(&format!(
        "differential harness on {}",
        Path::new(&root).display()
    ));
}
