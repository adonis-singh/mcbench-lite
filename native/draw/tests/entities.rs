// Every entity kind of the bake's capture, drawn by the Rust port
// (src/entities.rs) and by the page's own module (web/entity-draw.mjs, run
// in Node at test time on the same capture, entities, light and block
// quads): the same quads (corners, uv, texture, colour, overlay, layer,
// light) and the same frames. Needs Minecraft's baked data
// (MCBENCH_DRAW_DATA) and Node.

mod common;

use mcbench_draw::assets::{material_names, read, Paths};
use mcbench_draw::entities::{self, BlockQuad, Capture, View};
use mcbench_renderer::pack::Pack;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

fn value_of(text: &str) -> Value {
    match text {
        "true" => json!(true),
        "false" => json!(false),
        _ => text.parse::<i64>().map(|n| json!(n)).or_else(|_| text.parse::<f64>().map(|n| json!(n))).unwrap_or_else(|_| json!(text)),
    }
}

// For each kind: entities of its first, a middle and its last variant, at
// positions with fractions, turned, with what each kind's look reads.
fn entities_for(capture: &Value) -> Vec<Value> {
    let items: Vec<&String> = capture["items"].as_object().unwrap().keys().collect();
    let mut out = Vec::new();
    let mut id = 11i64;
    for (kind_name, kind) in capture["kinds"].as_object().unwrap() {
        let by: Vec<&str> = kind["by"].as_array().map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
        let keys: Vec<&String> = kind["variants"].as_object().map(|v| v.keys().collect()).unwrap_or_default();
        let picks: BTreeSet<usize> = if keys.is_empty() { [0].into() } else { [0, keys.len() / 2, keys.len() - 1].into() };
        for pick in picks {
            id += 7;
            let mut e = json!({
                "id": id, "type": kind_name,
                "pos": [3.25 + id as f64 * 0.5, 64.0 + (id % 5) as f64 * 0.375, -7.75 + id as f64 * 0.125],
                "yaw": (id * 37 % 360) as f64 - 180.0, "pitch": (id % 30) as f64 - 15.0,
                "width": 0.98, "height": 0.7, "ticks": 37 + id,
            });
            if kind_name == "minecraft:item" {
                e["item"] = json!(items[(id as usize * 13) % items.len()]);
                e["count"] = json!(1 + id % 40);
                out.push(e);
                continue;
            }
            if kind.get("icons").is_some() {
                let icons = kind["icons"].as_array().unwrap();
                let at = (pick * (icons.len() - 1)) / keys.len().max(1);
                e["value"] = json!(icons[at][0].as_f64().unwrap() as i64 + 1);
            } else if let Some(key) = keys.get(pick) {
                for (name, text) in by.iter().zip(key.split(',')) {
                    e[*name] = value_of(text);
                }
            }
            let variant = keys.get(pick).map(|k| &kind["variants"][k.as_str()]);
            if let Some(base) = variant.and_then(|v| v["base"].as_object()) {
                if base.contains_key("offset") {
                    e["offset"] = json!(6 + id % 4);
                }
                if base.keys().any(|k| k.contains('.')) {
                    e["pose"] = json!([[-12.5, 30.0, 0.0], [0.0, 5.0, 0.0], [-10.0, 0.0, -10.0], [-15.0, 0.0, 10.0], [-1.0, 0.0, -1.0], [1.0, 0.0, 1.0]]);
                }
            }
            if e.get("block").is_none() {
                e["block"] = json!(["minecraft:sand", "minecraft:chest[facing=east]", "minecraft:tnt"][(id % 3) as usize]);
            }
            if e.get("item").is_none() {
                e["item"] = json!(items[(id as usize * 7) % items.len()]);
            }
            if let Some(table) = kind["equipment"].as_object() {
                let mut equipment = serde_json::Map::new();
                for (item, slots) in table.iter().step_by(5).take(4) {
                    if let Some(slot) = slots.as_object().and_then(|s| s.keys().next()) {
                        equipment.insert(slot.clone(), json!(item));
                    }
                }
                equipment.insert("mainhand".into(), json!(items[(id as usize * 3) % items.len()]));
                e["equipment"] = Value::Object(equipment);
            }
            out.push(e);
        }
    }
    out
}

struct TestView<'a> {
    pack: &'a Pack,
    names: &'a [String],
}

fn light(x: i32, y: i32, z: i32) -> [i32; 2] {
    [(x * 7 + y * 13 + z * 5).rem_euclid(16), (x * 3 + y * 5 + z * 11).rem_euclid(16)]
}

impl View for TestView<'_> {
    fn light(&self, [x, y, z]: [i32; 3]) -> [i32; 2] {
        light(x, y, z)
    }
    fn block_quads(&self, state: &str) -> Result<Vec<BlockQuad>, String> {
        entities::lone_quads(self.pack, self.names, state)
    }
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-6 * (1.0 + a.abs().max(b.abs()))
}

#[test]
fn every_entity_kind_draws_the_quads_the_page_draws() {
    let Some(data) = common::data("every_entity_kind_draws_the_quads_the_page_draws") else { return };
    let paths = Paths::of_data(&data).unwrap();
    let pack = Pack::read(&read(&paths.pack).unwrap()).unwrap();
    let names = material_names(&read(&paths.textures).unwrap()).unwrap();
    let json: Value = serde_json::from_slice(&read(&paths.entities).unwrap()).unwrap();
    let list = entities_for(&json);
    let capture = Capture::new(json.clone()).unwrap();
    let view = TestView { pack: &pack, names: &names };
    // The block quads the page's blockQuads would answer (the same mesher).
    let mut states: BTreeSet<String> = list.iter().filter_map(|e| e["block"].as_str().map(str::to_string)).collect();
    for kind in json["kinds"].as_object().unwrap().values() {
        for variant in kind["variants"].as_object().map(|v| v.values().collect::<Vec<_>>()).unwrap_or_default() {
            for draw in variant["draws"].as_array().unwrap() {
                if let Some(b) = draw["block"].as_str().filter(|b| *b != "entity") {
                    states.insert(b.to_string());
                }
            }
        }
    }
    let lone: BTreeMap<String, Value> = states
        .iter()
        .map(|s| {
            let quads = entities::lone_quads(&pack, &names, s).unwrap();
            (s.clone(), json!(quads.iter().map(|q| json!({ "corners": q.corners, "uv": q.uv, "texture": q.texture, "color": q.color, "layer": "block" })).collect::<Vec<_>>()))
        })
        .collect();
    let dir = std::env::temp_dir().join(format!("mcbench-draw-entities-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("entities.json"), serde_json::to_vec(&list).unwrap()).unwrap();
    std::fs::write(dir.join("lone.json"), serde_json::to_vec(&lone).unwrap()).unwrap();
    let module = common::crate_dir().join("../../web/entity-draw.mjs");
    let output = std::process::Command::new("node")
        .arg(common::crate_dir().join("tests/entity-draw-page.mjs"))
        .arg(&module)
        .arg(&paths.entities)
        .arg(dir.join("entities.json"))
        .arg(dir.join("lone.json"))
        .output()
        .expect("Node runs the page's module");
    std::fs::remove_dir_all(&dir).unwrap();
    assert!(output.status.success(), "node: {}", String::from_utf8_lossy(&output.stderr));
    let page: Vec<Value> = serde_json::from_slice(&output.stdout).unwrap();
    let turn = mcbench_draw::entities::multiply(&mcbench_draw::camera::rotate_y(-0.7), &mcbench_draw::camera::rotate_x(-0.4));
    let mut kinds_checked = BTreeMap::new();
    for (entity, expected) in list.iter().zip(&page) {
        let ty = entity["type"].as_str().unwrap();
        let ours = entities::geometry(&capture, entity, &view);
        if let Some(error) = expected.get("error") {
            panic!("the page's module fails on {ty} ({error}); ours: {:?}", ours.err());
        }
        let ours = ours.unwrap_or_else(|e| panic!("{ty}: {e}"));
        let theirs = expected["quads"].as_array().unwrap();
        assert_eq!(ours.len(), theirs.len(), "{ty}: quads");
        assert!(!ours.is_empty(), "{ty} draws no quads");
        for (i, (q, t)) in ours.iter().zip(theirs).enumerate() {
            let corners: Vec<f64> = q.corners.iter().flatten().copied().collect();
            let numbers = |v: &Value| v.as_array().unwrap().iter().map(|n| n.as_f64().unwrap()).collect::<Vec<f64>>();
            for (field, a, b) in [("corners", corners, numbers(&t["corners"])), ("uv", q.uv.to_vec(), numbers(&t["uv"])), ("color", q.color.to_vec(), numbers(&t["color"])), ("overlay", q.overlay.to_vec(), numbers(&t["overlay"])), ("light", q.light.to_vec(), numbers(&t["light"]))] {
                assert_eq!(a.len(), b.len(), "{ty} quad {i} {field}");
                assert!(a.iter().zip(&b).all(|(x, y)| close(*x, *y)), "{ty} quad {i} {field}: {a:?} vs {b:?}");
            }
            assert_eq!(q.texture, t["texture"].as_str().unwrap_or(""), "{ty} quad {i} texture");
            assert_eq!(q.layer, t["layer"].as_str().unwrap_or(""), "{ty} quad {i} layer");
        }
        let place = entities::Place::of(entity).unwrap();
        for (name, camera) in [("frame", None), ("facing", Some(&turn))] {
            let m = entities::frame(&capture, entity, &place, camera).unwrap();
            let e: Vec<f64> = expected[name].as_array().unwrap().iter().map(|n| n.as_f64().unwrap()).collect();
            assert!(m.iter().zip(&e).all(|(a, b)| close(*a, *b)), "{ty} {name}: {m:?} vs {e:?}");
        }
        *kinds_checked.entry(ty.to_string()).or_insert(0) += ours.len();
    }
    let kinds = json["kinds"].as_object().unwrap().len();
    assert_eq!(kinds_checked.len(), kinds, "every kind checked");
    eprintln!("entity kinds checked against the page's module: {kinds_checked:?}");
}
