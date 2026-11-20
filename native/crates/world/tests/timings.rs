//! The Rust side of this crate's timings (`tests/node/timings.js` is
//! today's): the same operations on a chat's stored world, once after one
//! untimed run, printed as one JSON line of milliseconds. Needs
//! `MCBENCH_DIFF_DATA=<data folder clone>`, `MCBENCH_TIMING_CHAT=<chat id>`
//! and `MCBENCH_TIMING_MANIFEST=<file>` (the chat's `world_manifest_json`,
//! which `timings.js` writes); run with `--release --nocapture`, alternating
//! with the Node side.

mod common;

use std::time::Instant;

use mcbench_world::{BlockBox, BlockState, World, WorldManifest, transfer};

#[test]
#[ignore = "needs MCBENCH_DIFF_DATA, MCBENCH_TIMING_CHAT and MCBENCH_TIMING_MANIFEST"]
fn timings_of_a_stored_world() {
    let data = common::folder("MCBENCH_DIFF_DATA").expect("set MCBENCH_DIFF_DATA");
    common::fixture::guard(&data);
    let chat = std::env::var("MCBENCH_TIMING_CHAT").expect("set MCBENCH_TIMING_CHAT");
    let file = std::env::var("MCBENCH_TIMING_MANIFEST").expect("set MCBENCH_TIMING_MANIFEST");
    let manifest =
        WorldManifest(std::fs::read_to_string(&file).unwrap_or_else(|e| panic!("{file}: {e}")));
    let folder = data.join("worlds").join(&chat).join("chunks");
    let load = || {
        World::from_manifest(&manifest, &mut |hash: &str| {
            std::fs::read(folder.join(format!("{hash}.json"))).map_err(|e| e.to_string())
        })
        .unwrap()
    };
    let mut out = serde_json::Map::new();
    type Kept = Box<dyn std::any::Any>;
    fn timed<'a>(
        out: &mut serde_json::Map<String, serde_json::Value>,
        name: &str,
        run: &mut dyn FnMut() -> Box<dyn FnOnce() -> Kept + 'a>,
    ) {
        for round in 0..2 {
            let go = run();
            let started = Instant::now();
            let kept = go();
            let elapsed = started.elapsed();
            // What the operation made is freed after the clock stops, as
            // Node's collector frees it outside the timed span.
            drop(kept);
            if round == 1 {
                out.insert(
                    name.into(),
                    serde_json::json!(elapsed.as_secs_f64() * 1000.0),
                );
            }
        }
    }
    let mut base = load();
    base.finalize();
    let bounds = base.bounds().unwrap();
    let at = bounds.max;
    let base = &base;
    let gold = || BlockState::new("minecraft:gold_block", &[]);
    timed(&mut out, "load", &mut || {
        Box::new(|| Box::new(std::hint::black_box(load())))
    });
    timed(&mut out, "clone and one set", &mut || {
        Box::new(move || {
            let mut clone = base.clone_world().unwrap();
            clone.set(at, gold()).unwrap();
            Box::new(clone)
        })
    });
    timed(&mut out, "diff", &mut || {
        let mut clone = base.clone_world().unwrap();
        clone.set(at, gold()).unwrap();
        Box::new(move || Box::new((std::hint::black_box(clone.diff(base).unwrap()), clone)))
    });
    timed(&mut out, "persist unchanged", &mut || {
        Box::new(move || {
            Box::new(std::hint::black_box(
                base.persist(&mut |_: &str, _: &[u8]| Ok(())).unwrap(),
            ))
        })
    });
    timed(&mut out, "persist one chunk changed", &mut || {
        let mut clone = base.clone_world().unwrap();
        clone.set(at, gold()).unwrap();
        Box::new(move || {
            let manifest =
                std::hint::black_box(clone.persist(&mut |_: &str, _: &[u8]| Ok(())).unwrap());
            Box::new((manifest, clone))
        })
    });
    timed(&mut out, "blueprint", &mut || {
        Box::new(move || Box::new(std::hint::black_box(base.blueprint(64).unwrap())))
    });
    timed(&mut out, "transfer: encode", &mut || {
        Box::new(move || Box::new(std::hint::black_box(transfer::encode_world(base))))
    });
    let sent = transfer::encode_world(base);
    let sent = &sent;
    timed(&mut out, "transfer: decode", &mut || {
        Box::new(move || Box::new(std::hint::black_box(transfer::decode_world(sent).unwrap())))
    });
    timed(&mut out, "transfer: to the child and back", &mut || {
        Box::new(move || {
            let start = base.palette().len();
            let child = transfer::decode_world(&transfer::encode_world(base)).unwrap();
            let changes = transfer::encode_changes(&child, child.palette().len());
            let mut clone = base.clone_world().unwrap();
            transfer::apply_changes(&mut clone, &changes, start, &mut transfer::take_as_sent, 64)
                .unwrap();
            Box::new((child, changes, clone))
        })
    });
    timed(&mut out, "fill 3M blocks", &mut || {
        Box::new(|| {
            let mut world = World::new();
            world
                .fill(
                    BlockBox {
                        min: [0, 0, 0],
                        max: [299, 199, 49],
                    },
                    BlockState::new("minecraft:stone", &[]),
                )
                .unwrap();
            Box::new(world)
        })
    });
    println!(
        "{}",
        serde_json::json!({ "side": "rust", "chat": chat, "blocks": base.size(), "ms": out })
    );
}
