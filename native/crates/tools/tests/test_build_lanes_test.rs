//! `test/test-build-lanes.integration.test.js`: a test is a level of its
//! own, so its report is the same alone on one lane or beside others on
//! several. The same tests run alone, again, and all at once on the pool's
//! three lanes on the engine children, and their results and reports
//! compare equal but for what depends on the run.
//!
//! Needs Minecraft's tables, `MCBENCH_TABLES=<core/minecraft/1.21.10>`, and
//! the children's program, `MCBENCH_ENGINE_PROGRAM=<a built mcbench-lite>`
//! (it is ignored without them, and fails when they are set and missing).
//! Run it with `--release`.

mod common;

use std::path::PathBuf;
use std::sync::Arc;

use mcbench_engine::{BlockState, VerifyEngines};
use mcbench_tools::verify::{
    Budgets, Encoding, Limits, Minecraft, Placement, Settings, VerifierPool,
};
use mcbench_world::{BlockBox, World};
use serde_json::{Map, Value, json};
use tokio_util::sync::CancellationToken;

fn tables() -> PathBuf {
    let folder = PathBuf::from(
        std::env::var_os("MCBENCH_TABLES").expect("set MCBENCH_TABLES to Minecraft's tables"),
    );
    assert!(
        folder.join("blocks.txt").is_file(),
        "MCBENCH_TABLES names {}, which has no blocks.txt",
        folder.display()
    );
    folder
}

/// A lever into dust into a lamp, an observer ring that fires for good, and
/// a note block a sculk sensor hears, beside a sculk shrieker.
fn circuit() -> World {
    let state = |id: &str, props: &[(&str, &str)]| BlockState::new(id, props);
    let mut world = World::new();
    world
        .fill(
            BlockBox {
                min: [0, 0, 0],
                max: [20, 0, 12],
            },
            state("minecraft:stone", &[]),
        )
        .unwrap();
    world
        .set(
            [0, 1, 0],
            state(
                "minecraft:lever",
                &[("face", "floor"), ("facing", "north"), ("powered", "false")],
            ),
        )
        .unwrap();
    for x in 1..=4 {
        world
            .set(
                [x, 1, 0],
                state(
                    "minecraft:redstone_wire",
                    &[
                        ("north", "none"),
                        ("south", "none"),
                        ("east", "side"),
                        ("west", "side"),
                        ("power", "0"),
                    ],
                ),
            )
            .unwrap();
    }
    world
        .set(
            [5, 1, 0],
            state("minecraft:redstone_lamp", &[("lit", "false")]),
        )
        .unwrap();
    for (at, facing) in [
        ([10, 1, 4], "east"),
        ([11, 1, 4], "south"),
        ([11, 1, 5], "west"),
        ([10, 1, 5], "north"),
    ] {
        world
            .set(
                at,
                state(
                    "minecraft:observer",
                    &[("facing", facing), ("powered", "false")],
                ),
            )
            .unwrap();
    }
    world
        .set(
            [15, 1, 8],
            state(
                "minecraft:note_block",
                &[("instrument", "harp"), ("note", "0"), ("powered", "false")],
            ),
        )
        .unwrap();
    world
        .set(
            [16, 1, 8],
            state(
                "minecraft:sculk_sensor",
                &[
                    ("power", "0"),
                    ("sculk_sensor_phase", "inactive"),
                    ("waterlogged", "false"),
                ],
            ),
        )
        .unwrap();
    world
        .set(
            [17, 1, 8],
            state(
                "minecraft:sculk_shrieker",
                &[
                    ("can_summon", "false"),
                    ("shrieking", "false"),
                    ("waterlogged", "false"),
                ],
            ),
        )
        .unwrap();
    world.finalize();
    world
}

fn tests() -> Vec<Value> {
    let lamp = json!([[0, 1, 0], [3, 1, 0], [5, 1, 0]]);
    let mut with_note = lamp.as_array().unwrap().clone();
    with_note.push(json!([15, 1, 8]));
    vec![
        json!({ "probes": lamp, "actions": [{ "tick": 3, "type": "toggle", "at": [0, 1, 0] }], "samples": (0..21).map(|i| i * 5).collect::<Vec<i64>>() }),
        json!({ "probes": [[10, 1, 4], [11, 1, 4], [11, 1, 5], [10, 1, 5]], "samples": (0..100).map(|i| i * 2).collect::<Vec<i64>>() }),
        json!({ "probes": with_note, "actions": [{ "tick": 2, "type": "use", "at": [15, 1, 8] }, { "tick": 8, "type": "toggle", "at": [0, 1, 0] }], "samples": [0, 10, 20, 40] }),
        json!({ "probes": lamp, "actions": [{ "tick": 9, "type": "toggle", "at": [0, 1, 0] }, { "tick": 30, "type": "toggle", "at": [0, 1, 0] }], "samples": [0, 9, 10, 30, 31, 60], "settle_ticks": 7 }),
        json!({ "probes": lamp, "actions": [{ "tick": 1, "type": "toggle", "at": [0, 1, 0] }], "samples": [0, 5], "item_entities": true, "living_entities": true, "world_inventories": true }),
        json!({ "probes": [[10, 1, 4]], "samples": [0, 300], "settle_ticks": 100 }),
        json!({ "probes": [[3, 1, 0]], "actions": [{ "tick": 2, "type": "drop", "at": [3.5, 3, 0.5], "item": "minecraft:redstone", "count": 5, "velocity": [0, 0, 0] }], "samples": [0, 10, 40], "item_entities": true }),
        json!({ "probes": [[3, 1, 0], [15, 1, 8]], "actions": [{ "tick": 1, "type": "use", "at": [15, 1, 8] }, { "tick": 2, "type": "drop", "at": [3.5, 3, 0.5], "item": "minecraft:redstone", "count": 5, "velocity": [0, 0, 0] }], "samples": [0, 10, 40], "item_entities": true }),
    ]
}

/// What a result holds that depends on the experiment, not the run.
fn comparable(mut result: Map<String, Value>) -> Value {
    for key in ["wallMs", "timings", "metrics", "reportBytes"] {
        result.shift_remove(key);
    }
    let file = result.shift_remove("reportFile").unwrap();
    let mut report: Map<String, Value> =
        serde_json::from_slice(&std::fs::read(file.as_str().unwrap()).unwrap()).unwrap();
    report.shift_remove("metrics");
    result.insert("report".into(), Value::Object(report));
    Value::Object(result)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs MCBENCH_TABLES (Minecraft's tables) and MCBENCH_ENGINE_PROGRAM (a built mcbench-lite)"]
async fn tests_run_side_by_side_on_the_pools_lanes_report_what_they_report_one_at_a_time() {
    let tables = tables();
    let engine = Arc::new(VerifyEngines::new(common::engine_program(&tables)));
    let root = std::env::temp_dir().join(format!("mcbench-lanes-{}", std::process::id()));
    let settings = Settings {
        budgets: Limits::from(Budgets::DEFAULT),
        placement: Placement::Updates,
        encoding: Encoding::Full,
    };
    let pool = Arc::new(VerifierPool::new(
        root.clone(),
        Minecraft::new(engine.clone(), &tables).unwrap(),
        settings.clone(),
        Box::new(|| Ok(16 << 30)),
    ));
    pool.warm().unwrap();
    assert_eq!(pool.lane_roots().len(), 3);
    let world = Arc::new(circuit());
    let run = |test: Value| {
        let (pool, world, settings) = (pool.clone(), world.clone(), settings.clone());
        async move {
            pool.run(
                &world,
                &test,
                &settings,
                &CancellationToken::new(),
                &mut |_| {},
                None,
            )
            .await
            .unwrap()
        }
    };
    let mut alone = Vec::new();
    for test in tests() {
        alone.push(comparable(run(test).await));
    }
    let mut again = Vec::new();
    for test in tests() {
        again.push(comparable(run(test).await));
    }
    // All at once, given to the pool in order (as Promise.all gives them).
    let t = tests();
    let (a, b, c, d, e, f, g, h) = tokio::join!(
        biased;
        run(t[0].clone()), run(t[1].clone()), run(t[2].clone()), run(t[3].clone()),
        run(t[4].clone()), run(t[5].clone()), run(t[6].clone()), run(t[7].clone()),
    );
    let mut together = Vec::new();
    let mut lanes = std::collections::HashMap::new();
    for result in [a, b, c, d, e, f, g, h] {
        let lane = PathBuf::from(result["reportFile"].as_str().unwrap())
            .parent()
            .unwrap()
            .to_path_buf();
        *lanes.entry(lane).or_insert(0) += 1;
        together.push(comparable(result));
    }
    let mut counts: Vec<i32> = lanes.into_values().collect();
    counts.sort();
    assert_eq!(counts, [2, 3, 3], "the tests spread over the lanes");
    for (i, result) in together.iter().enumerate() {
        assert!(
            result["report"]["engine"]
                .as_str()
                .unwrap()
                .starts_with("mcbench-core"),
            "every test ran on the Rust engine"
        );
        assert_eq!(again[i], alone[i], "test {i} twice alone");
        assert_eq!(*result, alone[i], "test {i} beside the others");
    }
    assert!(
        together[6]["report"]["inputs"][0]["entityUUID"].is_string(),
        "a dropped item has a UUID, the same on every run"
    );
    // One child per lane, warm from the pool's warm-up to its stop.
    let pids = engine.pids();
    assert_eq!((pids.len(), engine.started()), (3, 3));
    pool.stop().await;
    for pid in pids {
        assert!(
            !common::alive(pid),
            "the engine child {pid} outlived the pool"
        );
    }
    let _ = std::fs::remove_dir_all(&root);
}
