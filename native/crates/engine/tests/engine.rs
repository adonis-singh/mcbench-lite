//! Today's engine tests on the engine as the app runs it: `engine.test.js`'s
//! placements and experiments on a child, `engines.test.js`,
//! `core-survive.test.js` on the live world, `settle-ticks-minecraft.test.js`,
//! `import-data-version.test.js` and the engine half of
//! `engine-verify.test.js` on children. Which of their cases are not here,
//! and why, is in the crate doc (Behaviour defined by).
//!
//! Every test needs Minecraft's tables (`MCBENCH_TABLES` at build time);
//! those on children also need `MCBENCH_ENGINE_PROGRAM`, and those whose
//! fixtures today's Node modules make `MCBENCH_DIFF_NODE`.

mod common;

use common::{engine, node, program, scratch};
use mcbench_engine::{
    BlockState, ChildEngine, EngineError, PlaceCommands, VerifyEngines, VerifyReport, VerifyRequest,
};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

const COMMANDS: &str = "fill 0 64 0 5 64 5 minecraft:stone\nsetblock 1 65 1 minecraft:lever[face=floor,facing=north,powered=false]\nsetblock 2 65 1 minecraft:redstone_wire\nsetblock 3 65 1 minecraft:redstone_lamp\n";

/// The VERIFY requests `tests/node/verify-fixtures.js` makes with today's
/// verifier, once per process.
fn fixtures() -> PathBuf {
    static FOLDER: OnceLock<PathBuf> = OnceLock::new();
    FOLDER
        .get_or_init(|| {
            let folder = scratch("verify-fixtures");
            node("verify-fixtures.js", &[folder.to_str().unwrap()]);
            folder
        })
        .clone()
}

fn fixture(name: &str) -> VerifyRequest {
    let folder = fixtures().join(name);
    VerifyRequest {
        request: std::fs::read_to_string(folder.join("request.json")).unwrap(),
        commands: PlaceCommands(std::fs::read_to_string(folder.join("commands.txt")).unwrap()),
    }
}

/// The verifier's children, of this test's own (a child belongs to the
/// runtime it was started on, and each test has a runtime of its own).
fn engines() -> Arc<VerifyEngines> {
    Arc::new(VerifyEngines::new(program()))
}

/// One experiment on a child of its own.
async fn verify(name: &str) -> Result<VerifyReport, EngineError> {
    engines()
        .verify(&fixture(name), |_| {}, &CancellationToken::new())
        .await
}

/// A child, ready.
async fn child() -> ChildEngine {
    let child = program().spawn().unwrap();
    child.ready(&CancellationToken::new()).await.unwrap();
    child
}

fn file<'a>(report: &'a VerifyReport, name: &str) -> &'a [u8] {
    &report
        .files
        .iter()
        .find(|(file, _)| file == name)
        .unwrap_or_else(|| panic!("no {name}"))
        .1
}

// engine.test.js: "the engine is built for the server and runs placements
// and experiments". The child is the app's own binary, nothing is built: a
// placement on the world the last one left, an experiment and its failure
// on the same child, and the ticks with and without light.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[cfg_attr(
    not(all(mcbench_tables, mcbench_program, mcbench_node)),
    ignore = "needs Minecraft's tables, the children's program and Node: set MCBENCH_TABLES, MCBENCH_ENGINE_PROGRAM and MCBENCH_DIFF_NODE at build time"
)]
async fn the_engine_runs_placements_and_experiments() {
    let never = CancellationToken::new();
    let child = child().await;
    let blueprint = child
        .place(&PlaceCommands(COMMANDS.into()), false, &never)
        .await
        .unwrap();
    assert!(!blueprint.0.is_empty());
    let report = child
        .verify(&fixture("stone"), |_| {}, &never)
        .await
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&report.result).unwrap()["ok"],
        true
    );
    let failed = child
        .verify(&fixture("resin"), |_| {}, &never)
        .await
        .unwrap_err();
    assert!(
        failed.to_string().starts_with(
            "The engine failed: unported: MultifaceBlock.updateShape (minecraft:resin_clump"
        ),
        "{failed}"
    );
    // The ticks light what they changed unless the caller reads only blocks after them.
    let stock = child.tick(100, true, &never).await.unwrap();
    let dark = child.tick(100, false, &never).await.unwrap();
    assert!(!stock.changes.is_empty() && !dark.changes.is_empty());
    let pid = child.pid();
    child.stop().await;
    assert!(common::gone_within(pid, common::AT_ONCE).await);
}

// engine.test.js: "a dead engine fails requests with its own stderr, and a
// restart starts with fresh output". A world has no process: work given to
// it that panics outside the engine's requests ends it with the panic's
// words, every later call says it was stopped, and a new world starts
// fresh. A refusal inside a request does not end the world.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[cfg_attr(
    not(mcbench_tables),
    ignore = "needs Minecraft's tables: set MCBENCH_TABLES at build time"
)]
async fn a_world_whose_work_panicked_fails_its_requests_and_a_new_world_starts_fresh() {
    let world = engine().world().unwrap();
    world
        .place(&PlaceCommands(COMMANDS.into()), true)
        .await
        .unwrap();
    let died = world
        .call(|_| -> () { panic!("first world ran out of memory") })
        .await
        .unwrap_err();
    assert_eq!(
        died.to_string(),
        "The engine stopped: mcbench-core failed: first world ran out of memory"
    );
    assert!(!world.alive());
    assert_eq!(
        world.tick(1, true).await.err().unwrap().to_string(),
        "The engine was stopped"
    );
    let again = engine().world().unwrap();
    assert_eq!(again.state().await.unwrap().lines().next(), Some("time 0"));
    again.stop().await;
}

// engines.test.js: "a verifier lane runs on the Rust engine alone": its
// engine is the app's own program (engineProgram). Here: the verifier's
// children are of the program given, and an experiment runs on one.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[cfg_attr(
    not(all(mcbench_tables, mcbench_program, mcbench_node)),
    ignore = "needs Minecraft's tables, the children's program and Node: set MCBENCH_TABLES, MCBENCH_ENGINE_PROGRAM and MCBENCH_DIFF_NODE at build time"
)]
async fn a_verifier_runs_on_the_engine_alone() {
    assert_eq!(engines().program().binary(), program().binary());
    let report = verify("stone").await.unwrap();
    let result: Value = serde_json::from_str(&report.result).unwrap();
    assert_eq!(result["ok"], true);
    assert_eq!(result["probeCount"], 1);
}

// core-survive.test.js: the engine's SURVIVE against Minecraft's canSurvive
// as the engine's table holds it (McbenchEngine.alone): every state of the
// table placed by the import alone, four blocks from the next, and once more
// with a block of stone where the table says a full block holds it. Every
// state the placement keeps is named exactly when Minecraft says it cannot
// survive there.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[cfg_attr(
    not(all(mcbench_tables, mcbench_node)),
    ignore = "needs Minecraft's tables and Node: set MCBENCH_TABLES and MCBENCH_DIFF_NODE at build time"
)]
async fn the_engine_names_the_blocks_that_cannot_survive_where_they_are_for_every_state_alone_and_held()
 {
    const STEPS: [[i32; 3]; 6] = [
        [0, -1, 0],
        [0, 1, 0],
        [0, 0, -1],
        [0, 0, 1],
        [-1, 0, 0],
        [1, 0, 0],
    ];
    let facts: Value = serde_json::from_str(&node(
        "survive-facts.js",
        &[common::tables().join("blocks.txt").to_str().unwrap()],
    ))
    .unwrap();
    let names: Vec<String> = facts["names"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n.as_str().unwrap().to_string())
        .collect();
    let survives: Vec<bool> = facts["survives"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_bool().unwrap())
        .collect();
    let held: Vec<u64> = facts["held"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_u64().unwrap())
        .collect();
    let folder = scratch("survive");
    let world = engine().world().unwrap();
    let block_of = |state: usize| names[state].split('[').next().unwrap().to_string();
    // Places `scenes` (state, at, stone) afresh; answers the scenes kept with
    // their state's block and the positions SURVIVE names. A block the port
    // refuses to place is left out, and the rest placed again.
    let survive = async |scenes: Vec<(usize, [i32; 3], Option<[i32; 3]>)>| {
        let mut refused = std::collections::HashSet::new();
        loop {
            let kept: Vec<_> = scenes
                .iter()
                .filter(|(state, _, _)| !refused.contains(&block_of(*state)))
                .cloned()
                .collect();
            let mut commands = String::new();
            for (state, at, stone) in &kept {
                if let Some(stone) = stone {
                    commands.push_str(&format!(
                        "setblock {} {} {} minecraft:stone\n",
                        stone[0], stone[1], stone[2]
                    ));
                }
                commands.push_str(&format!(
                    "setblock {} {} {} {}\n",
                    at[0], at[1], at[2], names[*state]
                ));
            }
            let blueprint = match world.place(&PlaceCommands(commands), true).await {
                Ok(blueprint) => blueprint,
                Err(EngineError::Failed(reason)) => {
                    let block = reason
                        .find("unported: ")
                        .and_then(|_| reason.find("(minecraft:"))
                        .map(|at| {
                            reason[at + 1..]
                                .chars()
                                .take_while(|c| {
                                    c.is_ascii_lowercase()
                                        || c.is_ascii_digit()
                                        || *c == '_'
                                        || *c == ':'
                                })
                                .collect::<String>()
                        })
                        .unwrap_or_else(|| panic!("placement failed: {reason}"));
                    refused.insert(block);
                    continue;
                }
                Err(other) => panic!("{other}"),
            };
            let named: std::collections::HashSet<String> = world
                .unsupported()
                .await
                .unwrap()
                .lines()
                .map(String::from)
                .collect();
            let file = folder.join("blueprint.bin");
            std::fs::write(&file, &blueprint.0).unwrap();
            let positions = folder.join("positions.txt");
            std::fs::write(
                &positions,
                kept.iter()
                    .map(|(_, at, _)| format!("[{},{},{}]\n", at[0], at[1], at[2]))
                    .collect::<String>(),
            )
            .unwrap();
            let ids = node_with_input("blueprint-ids.js", &[file.to_str().unwrap()], &positions);
            let kept: Vec<_> = kept
                .into_iter()
                .zip(ids.lines())
                .filter(|((state, _, _), id)| *id == block_of(*state))
                .map(|(scene, _)| scene)
                .collect();
            return (kept, named);
        }
    };
    let at = |i: usize| [4 * (i % 256) as i32, 64, 4 * (i / 256) as i32];
    let (alone_kept, alone_named) = survive(
        (0..names.len())
            .map(|state| (state, at(state), None))
            .collect(),
    )
    .await;
    let wrong: Vec<_> = alone_kept
        .iter()
        .filter(|(state, pos, _)| {
            alone_named.contains(&format!("{} {} {}", pos[0], pos[1], pos[2])) == survives[*state]
        })
        .map(|(state, _, _)| names[*state].clone())
        .collect();
    assert!(
        wrong.is_empty(),
        "{} states alone: {:?}",
        wrong.len(),
        &wrong[..wrong.len().min(20)]
    );
    let holdable: Vec<usize> = (0..names.len()).filter(|&state| held[state] != 0).collect();
    let (held_kept, held_named) = survive(
        holdable
            .iter()
            .enumerate()
            .map(|(i, &state)| {
                let pos = at(i);
                let step = STEPS[held[state].trailing_zeros() as usize];
                (
                    state,
                    pos,
                    Some([pos[0] + step[0], pos[1] + step[1], pos[2] + step[2]]),
                )
            })
            .collect(),
    )
    .await;
    let named: Vec<_> = held_kept
        .iter()
        .filter(|(_, pos, _)| held_named.contains(&format!("{} {} {}", pos[0], pos[1], pos[2])))
        .map(|(state, _, _)| names[*state].clone())
        .collect();
    assert!(
        named.is_empty(),
        "{} held states: {:?}",
        named.len(),
        &named[..named.len().min(20)]
    );
    assert!(
        alone_kept.len() > 25000 && held_kept.len() > 4000,
        "{} states alone, {} held",
        alone_kept.len(),
        held_kept.len()
    );
    assert!(
        alone_named.len() > 3000,
        "{} named alone",
        alone_named.len()
    );
    world.stop().await;
    std::fs::remove_dir_all(&folder).unwrap();
}

fn node_with_input(script: &str, args: &[&str], input: &std::path::Path) -> String {
    let node =
        std::env::var("MCBENCH_DIFF_NODE").expect("MCBENCH_DIFF_NODE names the node program");
    let output = std::process::Command::new(node)
        .arg(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/node")
                .join(script),
        )
        .args(args)
        .stdin(std::fs::File::open(input).unwrap())
        .current_dir(common::repository())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "node {script}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

// settle-ticks-minecraft.test.js: the default settle against Minecraft's
// fluids (the engine's table: each flowing fluid's tick delay and drop-off
// in the overworld, and the amount of each fluid state): the steps of the
// longest spread across flat ground, from a source's amount down to 1, at
// the slowest tick delay. The default is today's (server/minecraft-dimension.js).
#[test]
#[cfg_attr(
    not(all(mcbench_tables, mcbench_node)),
    ignore = "needs Minecraft's tables and Node: set MCBENCH_TABLES and MCBENCH_DIFF_NODE at build time"
)]
fn the_default_settle_is_the_longest_fluid_spread_on_flat_ground_at_the_slowest_fluids_tick_delay()
{
    let table = std::fs::read_to_string(common::tables().join("blocks.txt")).unwrap();
    let lines: Vec<Vec<&str>> = table
        .lines()
        .map(|line| line.split('\t').collect())
        .collect();
    let rules: Vec<(&str, i64, i64)> = lines
        .iter()
        .filter(|line| line[0] == "fluidrule")
        .map(|line| (line[1], line[2].parse().unwrap(), line[3].parse().unwrap()))
        .collect();
    let source = lines
        .iter()
        .filter(|line| line[0] == "fluid" && line.get(6) == Some(&"1"))
        .map(|line| line[5].parse::<i64>().unwrap())
        .max()
        .unwrap();
    let mut keys: Vec<&str> = rules.iter().map(|(key, _, _)| *key).collect();
    keys.sort();
    assert_eq!(
        keys,
        [
            "minecraft:flowing_lava",
            "minecraft:flowing_water",
            "minecraft:lava",
            "minecraft:water"
        ]
    );
    let steps = rules
        .iter()
        .map(|(_, _, drop_off)| (source - 1) / drop_off)
        .max()
        .unwrap();
    let slowest = rules.iter().map(|(_, delay, _)| *delay).max().unwrap();
    assert_eq!([source, steps, slowest], [8, 7, 30]);
    let facts: Value = serde_json::from_str(&node(
        "survive-facts.js",
        &[common::tables().join("blocks.txt").to_str().unwrap()],
    ))
    .unwrap();
    assert_eq!(
        facts["defaultSettleTicks"].as_i64().unwrap(),
        steps * slowest
    );
}

// import-data-version.test.js: blocks renamed since older versions, the
// data fixer's own cases (chain is iron_chain since 1.21.9, grass
// short_grass since 1.20.3, grass_path dirt_path since 1.17), as the engine
// brings a palette forward (in a child of its own, `--datafix`); a palette
// of the pinned version or a later one stays as written.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[cfg_attr(
    not(all(mcbench_tables, mcbench_program)),
    ignore = "needs Minecraft's tables and the children's program: set MCBENCH_TABLES and MCBENCH_ENGINE_PROGRAM at build time"
)]
async fn a_palette_written_at_an_older_version_is_brought_forward_by_the_data_fixer_and_a_current_one_is_kept()
 {
    let state = |id: &str, properties: &[(&str, &str)]| BlockState {
        id: id.into(),
        properties: properties
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
    };
    let cases: [(i64, Vec<(BlockState, &str)>); 4] = [
        (
            2566,
            vec![
                (state("minecraft:grass_path", &[]), "minecraft:dirt_path"),
                (state("minecraft:grass", &[]), "minecraft:short_grass"),
                (
                    state("minecraft:chain", &[("axis", "x")]),
                    "minecraft:iron_chain",
                ),
            ],
        ),
        (
            3218,
            vec![
                (state("minecraft:grass", &[]), "minecraft:short_grass"),
                (
                    state("minecraft:chain", &[("axis", "x")]),
                    "minecraft:iron_chain",
                ),
            ],
        ),
        (
            3465,
            vec![
                (state("minecraft:grass", &[]), "minecraft:short_grass"),
                (
                    state(
                        "minecraft:chain",
                        &[("axis", "y"), ("waterlogged", "false")],
                    ),
                    "minecraft:iron_chain",
                ),
            ],
        ),
        (
            4189,
            vec![
                (state("minecraft:short_grass", &[]), "minecraft:short_grass"),
                (
                    state("minecraft:chain", &[("axis", "z")]),
                    "minecraft:iron_chain",
                ),
            ],
        ),
    ];
    for (version, blocks) in cases {
        let mut entries = vec![state("minecraft:dirt", &[])];
        entries.extend(blocks.iter().map(|(state, _)| state.clone()));
        let fixed = program().fixed_palette(&entries, version).await.unwrap();
        assert_eq!(fixed[0].id, "minecraft:dirt");
        for ((written, now), fixed) in blocks.iter().zip(&fixed[1..]) {
            assert_eq!(fixed.id, *now, "{} at {version}", written.id);
        }
    }
    let current = vec![state("minecraft:iron_chain", &[("axis", "x")])];
    for version in [
        mcbench_engine::DATA_VERSION,
        mcbench_engine::DATA_VERSION + 1,
    ] {
        let kept = program().fixed_palette(&current, version).await.unwrap();
        assert_eq!(
            (kept[0].id.as_str(), kept[0].properties.clone()),
            ("minecraft:iron_chain", current[0].properties.clone())
        );
    }
}

// import-data-version.test.js: the verifier imports each world written at
// an older version and reads its blocks back: every renamed block is there
// as the fixer names it, and the grass on its dirt (the requests are
// today's verifier's).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[cfg_attr(
    not(all(mcbench_tables, mcbench_program, mcbench_node)),
    ignore = "needs Minecraft's tables, the children's program and Node: set MCBENCH_TABLES, MCBENCH_ENGINE_PROGRAM and MCBENCH_DIFF_NODE at build time"
)]
async fn the_verifier_imports_worlds_written_at_older_versions_and_reads_the_fixed_blocks_back() {
    for (version, now) in [
        (
            2566,
            vec![
                "minecraft:dirt_path",
                "minecraft:short_grass",
                "minecraft:iron_chain",
            ],
        ),
        (3218, vec!["minecraft:short_grass", "minecraft:iron_chain"]),
        (3465, vec!["minecraft:short_grass", "minecraft:iron_chain"]),
        (4189, vec!["minecraft:short_grass", "minecraft:iron_chain"]),
    ] {
        let report = verify(&format!("version-{version}")).await.unwrap();
        let report: Value = serde_json::from_slice(file(&report, "report.json")).unwrap();
        let observations = report["observations"].as_array().unwrap();
        assert!(!observations.is_empty());
        for observation in observations {
            let blocks: Vec<&str> = observation["blocks"]
                .as_array()
                .unwrap()
                .iter()
                .map(|block| block["block"].as_str().unwrap().split('[').next().unwrap())
                .collect();
            assert_eq!(blocks, now, "{version} at tick {}", observation["tick"]);
        }
    }
}

// engine-verify.test.js: "the engine runs a test and its report comes back
// archived". The report comes back whole, and the run's own files in the
// child's folder go once it is done.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[cfg_attr(
    not(all(mcbench_tables, mcbench_program, mcbench_node)),
    ignore = "needs Minecraft's tables, the children's program and Node: set MCBENCH_TABLES, MCBENCH_ENGINE_PROGRAM and MCBENCH_DIFF_NODE at build time"
)]
async fn the_engine_runs_a_test_and_its_report_comes_back_and_its_files_go() {
    let request = fixture("stone");
    assert!(
        request
            .commands
            .0
            .starts_with("setblock 0 64 0 minecraft:stone")
    );
    let updates = Arc::new(Mutex::new(Vec::new()));
    let seen = updates.clone();
    let child = child().await;
    let report = child
        .verify(
            &request,
            move |text| seen.lock().unwrap().push(text.to_string()),
            &CancellationToken::new(),
        )
        .await
        .unwrap();
    let result: Value = serde_json::from_str(&report.result).unwrap();
    assert_eq!(result["ok"], true);
    assert_eq!(
        result["reportBytes"].as_u64().unwrap() as usize,
        file(&report, "report.json").len()
    );
    assert!(
        !updates.lock().unwrap().is_empty()
            || report.files.iter().any(|(name, _)| name == "progress.json")
    );
    // The run's folder goes before the answer comes.
    let left: Vec<_> = std::fs::read_dir(child.folder())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with("verify-"))
        .collect();
    assert!(left.is_empty(), "{left:?}");
    let folder = child.folder().to_path_buf();
    child.stop().await;
    assert!(!folder.exists(), "the child's folder goes with it");
}

// engine-verify.test.js: "a failed experiment, incomplete observations and
// an engine fault are errors". The engine half: an experiment that fails
// ends with result.json's ok false and the reason, and a fault of the
// engine is the request's failure.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[cfg_attr(
    not(all(mcbench_tables, mcbench_program, mcbench_node)),
    ignore = "needs Minecraft's tables, the children's program and Node: set MCBENCH_TABLES, MCBENCH_ENGINE_PROGRAM and MCBENCH_DIFF_NODE at build time"
)]
async fn a_failed_experiment_and_an_engine_fault_come_back_as_they_happened() {
    let failed = verify("missing-inventory").await;
    match failed {
        Ok(report) => {
            let result: Value = serde_json::from_str(&report.result).unwrap();
            assert_eq!(result["ok"], false);
            assert_eq!(
                result["error"],
                "java.io.IOException: Inventory probe is not a container: BlockPos{x=0, y=64, z=0}"
            );
        }
        Err(error) => panic!("an experiment that failed is a report, not {error}"),
    }
    let fault = verify("resin").await.unwrap_err();
    assert!(matches!(fault, EngineError::Failed(_)));
}

// engine-verify.test.js: "cancelling a running test stops its engine": the
// call returns at once, the child is gone within a second, and the next
// experiment runs on a new child. And "a test that outlasts the wall budget
// fails plainly, with the ticks it reached": the engine half is the
// progress the caller is given while it runs, the tick it reached of its
// timeline.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[cfg_attr(
    not(all(mcbench_tables, mcbench_program, mcbench_node)),
    ignore = "needs Minecraft's tables, the children's program and Node: set MCBENCH_TABLES, MCBENCH_ENGINE_PROGRAM and MCBENCH_DIFF_NODE at build time"
)]
async fn cancelling_a_running_test_returns_at_once_with_the_progress_it_reached() {
    let cancel = CancellationToken::new();
    let cancel_seen = cancel.clone();
    let progress = Arc::new(Mutex::new(Vec::<String>::new()));
    let (seen, cancelling) = (progress.clone(), cancel.clone());
    let request = fixture("clock");
    let engines = engines();
    let running = tokio::spawn({
        let engines = engines.clone();
        async move {
            engines
                .verify(
                    &request,
                    move |text| {
                        seen.lock().unwrap().push(text.to_string());
                        if text.contains("\"running\"") {
                            cancelling.cancel();
                        }
                    },
                    &cancel,
                )
                .await
        }
    });
    // The child running it.
    let began = Instant::now();
    let pid = loop {
        if let Some(&pid) = engines.pids().first() {
            break pid;
        }
        assert!(
            began.elapsed() < Duration::from_secs(120),
            "no child runs it"
        );
        tokio::time::sleep(Duration::from_millis(1)).await;
    };
    while !cancel_seen.is_cancelled() {
        assert!(began.elapsed() < Duration::from_secs(120), "it never ran");
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
    let cancelled = Instant::now();
    let result = tokio::time::timeout(Duration::from_secs(120), running)
        .await
        .unwrap()
        .unwrap();
    assert!(cancelled.elapsed() < common::AT_ONCE);
    assert!(matches!(result, Err(EngineError::Cancelled)), "{result:?}");
    let last: Value = serde_json::from_str(progress.lock().unwrap().last().unwrap()).unwrap();
    assert_eq!(last["phase"], "running");
    assert_eq!(last["totalTicks"], 70000);
    assert!(last["tick"].as_i64().unwrap() < 70000);
    assert!(
        common::gone_within(pid, common::AT_ONCE).await,
        "the child {pid} still runs"
    );
    assert!(engines.pids().is_empty());
    // The next experiment runs on a new child.
    let next = engines
        .verify(&fixture("stone"), |_| {}, &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&next.result).unwrap()["ok"],
        true
    );
    assert_eq!(engines.started(), 2);
    let pids = engines.pids();
    engines.stop().await;
    for pid in pids {
        assert!(common::gone_within(pid, common::AT_ONCE).await);
    }
}

// engine-verify.test.js: "a test that reaches what the engine does not
// model fails with the reason, and nothing runs it again".
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[cfg_attr(
    not(all(mcbench_tables, mcbench_program, mcbench_node)),
    ignore = "needs Minecraft's tables, the children's program and Node: set MCBENCH_TABLES, MCBENCH_ENGINE_PROGRAM and MCBENCH_DIFF_NODE at build time"
)]
async fn a_test_that_reaches_what_the_engine_does_not_model_fails_with_the_reason() {
    let error = verify("resin").await.unwrap_err();
    assert_eq!(
        error,
        EngineError::Failed(
            error
                .to_string()
                .trim_start_matches("The engine failed: ")
                .to_string()
        )
    );
    assert!(
        error.to_string().starts_with(
            "The engine failed: unported: MultifaceBlock.updateShape (minecraft:resin_clump"
        ),
        "{error}"
    );
}

// engine-verify.test.js: "a test cancelled while its engine is still being
// built returns at once, and nothing spawns". Nothing is built: a test
// cancelled before it starts returns at once and starts no child (one
// cancelled while its child starts is `mcbench-tools`' case).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[cfg_attr(
    not(all(mcbench_tables, mcbench_program, mcbench_node)),
    ignore = "needs Minecraft's tables, the children's program and Node: set MCBENCH_TABLES, MCBENCH_ENGINE_PROGRAM and MCBENCH_DIFF_NODE at build time"
)]
async fn a_test_cancelled_before_it_starts_returns_at_once_and_runs_nothing() {
    let cancel = CancellationToken::new();
    cancel.cancel();
    let request = fixture("clock");
    let started = Instant::now();
    let ran = Arc::new(Mutex::new(false));
    let seen = ran.clone();
    let engines = engines();
    let result = engines
        .verify(&request, move |_| *seen.lock().unwrap() = true, &cancel)
        .await;
    assert!(matches!(result, Err(EngineError::Cancelled)));
    assert!(started.elapsed() < common::AT_ONCE);
    assert!(!*ran.lock().unwrap());
    assert_eq!(engines.started(), 0, "no child was started for it");
}
