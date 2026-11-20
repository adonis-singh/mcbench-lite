//! The fuzz of the differential harness: `tests/node/fuzz.js` writes random
//! operation sequences from one seed with what today's `World` returned for
//! each (values and error messages); this test runs the same operations on
//! this crate's `World` and compares every result and the final manifest
//! and chunk files.
//!
//! `MCBENCH_NODE_DIFF=<checkout with node_modules>` turns it on;
//! `MCBENCH_FUZZ_SEED` (default 1) and `MCBENCH_FUZZ_SEQUENCES` (default
//! 2000) choose the run. Run it with `--release --nocapture`.

mod common;

use common::{Report, attempt, json, sha};
use mcbench_world::{BlockBox, World, blueprint, transfer};
use serde_json::{Value, json};

fn pos(value: &Value) -> [i32; 3] {
    let list = value.as_array().expect("a position");
    [0, 1, 2].map(|a| list[a].as_i64().expect("an integer") as i32)
}

fn pairs(value: &Value) -> Vec<(String, String)> {
    value
        .as_array()
        .expect("pairs")
        .iter()
        .map(|p| {
            (
                p[0].as_str().expect("a key").to_string(),
                p[1].as_str().expect("a value").to_string(),
            )
        })
        .collect()
}

fn boxes(value: &Value) -> Vec<BlockBox> {
    value
        .as_array()
        .expect("boxes")
        .iter()
        .map(|b| BlockBox {
            min: pos(&b[0]),
            max: pos(&b[1]),
        })
        .collect()
}

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

fn entry_json(entry: Option<&mcbench_world::BlockState>) -> Value {
    entry.map_or(Value::Null, |e| {
        let states: serde_json::Map<String, Value> = e
            .properties
            .iter()
            .map(|(k, v)| (k.clone(), json!(v)))
            .collect();
        json!({ "id": e.id, "states": states })
    })
}

fn bounds_json(bounds: Option<BlockBox>) -> Value {
    bounds.map_or(Value::Null, |b| json!({ "min": b.min, "max": b.max }))
}

#[test]
#[ignore = "needs MCBENCH_NODE_DIFF (a checkout with node_modules)"]
fn random_operations_return_what_node_returns() {
    let Some(checkout) = common::folder("MCBENCH_NODE_DIFF") else {
        panic!("set MCBENCH_NODE_DIFF to run the fuzz");
    };
    let seed = std::env::var("MCBENCH_FUZZ_SEED").unwrap_or_else(|_| "1".into());
    let count = std::env::var("MCBENCH_FUZZ_SEQUENCES").unwrap_or_else(|_| "2000".into());
    let out = std::env::var_os("MCBENCH_DIFF_OUT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("world-diff")
        });
    std::fs::create_dir_all(&out).expect("the output folder");
    let file = out.join(format!("fuzz-{seed}-{count}.json"));
    if !file.is_file() {
        common::node(
            &checkout,
            "fuzz.js",
            &[
                &file,
                std::path::Path::new(&seed),
                std::path::Path::new(&count),
            ],
        );
    }
    let fuzz = json(&file);
    let mut report = Report::default();
    let mut operations = 0;
    for (index, sequence) in fuzz["sequences"]
        .as_array()
        .expect("sequences")
        .iter()
        .enumerate()
    {
        let mut world = World::new();
        let mut origin: Option<World> = None;
        let mut palette_start = 1;
        for (step, op) in sequence["ops"].as_array().expect("ops").iter().enumerate() {
            operations += 1;
            let what = format!(
                "sequence {index} step {step} {}",
                serde_json::to_string(op)
                    .expect("JSON")
                    .chars()
                    .take(300)
                    .collect::<String>()
            );
            let name = op["op"].as_str().expect("an op");
            let none = |r: Result<(), String>| r.map(|_| Value::Null);
            let result: Result<Value, String> = match name {
                "set" => none(world.set_block(
                    pos(&op["at"]),
                    op["id"].as_str().expect("an id"),
                    &pairs(&op["states"]),
                )),
                "fill" => none(world.fill_boxes(
                    &[BlockBox {
                        min: pos(&op["min"]),
                        max: pos(&op["max"]),
                    }],
                    op["id"].as_str().expect("an id"),
                    &pairs(&op["states"]),
                    &[],
                )),
                "fillBoxes" => none(world.fill_boxes(
                    &boxes(&op["parts"]),
                    op["id"].as_str().expect("an id"),
                    &pairs(&op["states"]),
                    &boxes(&op["clear"]),
                )),
                "get" => {
                    let at = pos(&op["at"]);
                    Ok(json!({ "entry": entry_json(world.get(at)), "has": world.has(at) }))
                }
                "finalize" => {
                    world.finalize();
                    Ok(Value::Null)
                }
                "clone" => world.clone_world().map(|next| {
                    origin = Some(std::mem::replace(&mut world, next));
                    palette_start = world.palette().len();
                    Value::Null
                }),
                "diff" => world.diff(origin.as_ref().expect("an origin")).map(
                    |d| json!({ "added": d.added, "removed": d.removed, "changed": d.changed }),
                ),
                "look" => Ok(json!({
                    "size": world.size(),
                    "bounds": bounds_json(world.bounds()),
                    "chunks": world.chunks().map(|(key, chunk)| json!([key, chunk.count(), chunk.is_dense()])).collect::<Vec<_>>(),
                    "changed": world.changed_chunks().collect::<Vec<_>>(),
                    "owned": world.owned_chunks().collect::<Vec<_>>(),
                    "near": world.near_changed_chunks().into_iter().collect::<Vec<_>>(),
                    "palette": world.palette().len(),
                    "lod": world.render_lod().map_or(Value::Null, |(axis, scale)| json!({ "axis": axis, "scale": scale })),
                })),
                "replace" => {
                    let target = op["target"].as_str().expect("a target").to_string();
                    let area = (!op["box"].is_null()).then(|| BlockBox {
                        min: pos(&op["box"]["min"]),
                        max: pos(&op["box"]["max"]),
                    });
                    world
                        .replace(
                            op["id"].as_str().expect("an id"),
                            |entry| {
                                Ok(mcbench_world::BlockState {
                                    id: target.clone(),
                                    properties: entry.properties.clone(),
                                })
                            },
                            area,
                        )
                        .map(|n| json!(n))
                }
                "clear" => {
                    world.clear();
                    Ok(Value::Null)
                }
                "encode" => Ok(json!({
                    "world": sha(transfer::encode_world(&world)),
                    "changes": sha(transfer::encode_changes(&world, palette_start.min(world.palette().len()))),
                })),
                "blueprint" => {
                    blueprint::world_blueprint(&world, op["y"].as_i64().expect("a y") as i32)
                        .map(|b| json!(sha(b)))
                }
                "manifest" => manifest_of(&world),
                "reference" => {
                    world = world.reference();
                    origin = None;
                    Ok(Value::Null)
                }
                other => panic!("unknown op {other}"),
            };
            report.check(
                &format!("fuzz: {name}"),
                &what,
                &op["result"],
                &attempt(result),
            );
        }
        report.check(
            "fuzz: final manifest and chunk files",
            &format!("sequence {index}"),
            &sequence["final"],
            &attempt(manifest_of(&world)),
        );
        let entries: Vec<Value> = world
            .palette()
            .entries()
            .map(|e| {
                json!([
                    e.id,
                    e.properties
                        .iter()
                        .map(|(k, v)| json!([k, v]))
                        .collect::<Vec<_>>()
                ])
            })
            .collect();
        report.check(
            "fuzz: final palette",
            &format!("sequence {index}"),
            &sequence["entries"],
            &json!(entries),
        );
    }
    println!("{operations} operations");
    report.finish(&format!("fuzz, seed {seed}, {count} sequences"));
}
