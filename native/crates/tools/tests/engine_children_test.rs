//! What the verifier's engine children are for (`native/README.md`,
//! Decisions of stage three: where `test_build` and settles run): a test of
//! a dense, long-running world cancelled mid-run, or past its wall-clock
//! budget, is answered at once with today's answer, its child is gone at once
//! (each "at once" a bound a starved machine meets, [`AT_ONCE`]), its lane is
//! free at once, and the lane's next test runs on a new child. An in-process
//! `VERIFY` held its lane until its ticks ended.
//!
//! Needs Minecraft's tables (`MCBENCH_TABLES`) and the children's program
//! (`MCBENCH_ENGINE_PROGRAM`, a built `mcbench-lite`); ignored without them,
//! failing when they are set and missing. Run it with `--release`.

mod common;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use common::{alive, engine_program, scratch, settings, with};
use mcbench_engine::{BlockState, VerifyEngines};
use mcbench_tools::verify::{ENGINE_STOPPED, Limits, Minecraft, VerifierPool};
use mcbench_world::World;
use serde_json::{Map, Value, json};
use tokio_util::sync::CancellationToken;

/// "At once": against the rest of a 70,000-tick run of 1,024 clocks, or a
/// child that never exits by itself, a bound a starved machine meets.
const AT_ONCE: Duration = Duration::from_secs(10);

fn tables() -> PathBuf {
    let folder = PathBuf::from(
        std::env::var_os("MCBENCH_TABLES").expect("set MCBENCH_TABLES to Minecraft's tables"),
    );
    assert!(
        folder.join("blocks.txt").is_file(),
        "no blocks.txt in {}",
        folder.display()
    );
    folder
}

/// 1,024 observer clocks on stone, each firing every tick for good.
fn clocks() -> World {
    let mut world = World::new();
    for i in 0..1024 {
        let (x, z) = (4 * (i % 32), 2 * (i / 32));
        for (dx, facing) in [(0, "east"), (1, "west")] {
            world
                .set([x + dx, 0, z], BlockState::new("minecraft:stone", &[]))
                .unwrap();
            world
                .set(
                    [x + dx, 1, z],
                    BlockState::new(
                        "minecraft:observer",
                        &[("facing", facing), ("powered", "false")],
                    ),
                )
                .unwrap();
        }
    }
    world.finalize();
    world
}

fn stone() -> World {
    common::world_at([0, 0, 0])
}

/// A pool of one lane on children of the program, and the children.
fn lane(budgets: Limits) -> (Arc<VerifierPool>, Arc<VerifyEngines>) {
    let tables = tables();
    let engines = Arc::new(VerifyEngines::new(engine_program(&tables)));
    let children_root = scratch("children");
    let pool = Arc::new(VerifierPool::single(
        children_root.path_buf(),
        Minecraft::new(engines.clone(), &tables).unwrap(),
        settings(budgets),
    ));
    (pool, engines)
}

/// The long test of [`clocks`]: 70,000 ticks.
fn long() -> Value {
    json!({ "probes": [[0, 1, 0]], "samples": [0, 70000] })
}

/// Runs the long test on `pool`, its updates kept; resolves to its answer.
fn run_long(
    pool: &Arc<VerifierPool>,
    budgets: Limits,
    cancel: &CancellationToken,
    updates: &Arc<Mutex<Vec<Map<String, Value>>>>,
) -> tokio::task::JoinHandle<Result<Map<String, Value>, mcbench_tools::verify::VerifyError>> {
    let (pool, cancel, updates) = (pool.clone(), cancel.clone(), updates.clone());
    tokio::spawn(async move {
        pool.run(
            &clocks(),
            &long(),
            &settings(budgets),
            &cancel,
            &mut |update| updates.lock().unwrap().push(update),
            None,
        )
        .await
    })
}

/// The child running the lane's test, once it runs and its progress has a
/// tick past 0.
async fn running_child(
    engines: &VerifyEngines,
    updates: &Arc<Mutex<Vec<Map<String, Value>>>>,
) -> u32 {
    let began = Instant::now();
    loop {
        let ticking = updates.lock().unwrap().iter().any(|u| {
            u.get("tick")
                .and_then(Value::as_f64)
                .is_some_and(|t| t > 0.0)
        });
        if ticking && let Some(&pid) = engines.pids().first() {
            return pid;
        }
        assert!(
            began.elapsed() < Duration::from_secs(60),
            "the test never ran"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

async fn gone_at_once(pid: u32) {
    let began = Instant::now();
    while alive(pid) {
        assert!(began.elapsed() < AT_ONCE, "the child {pid} still runs");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

/// The lane's next test runs at once, on a new child, and passes.
async fn next_test_runs_on_a_new_child(pool: &VerifierPool, engines: &VerifyEngines) {
    let began = Instant::now();
    let result = pool
        .run(
            &stone(),
            &json!({ "probes": [[0, 0, 0]] }),
            &settings(common::budgets()),
            &CancellationToken::new(),
            &mut |_| {},
            None,
        )
        .await
        .unwrap();
    assert_eq!(result["ok"], json!(true));
    assert!(
        began.elapsed() < AT_ONCE,
        "the lane was held: {:?}",
        began.elapsed()
    );
    assert_eq!(engines.started(), 2, "the next test ran on a new child");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "needs MCBENCH_TABLES (Minecraft's tables) and MCBENCH_ENGINE_PROGRAM (a built mcbench-lite)"]
async fn a_dense_test_cancelled_mid_run_answers_at_once_its_child_goes_and_its_lane_is_free() {
    let (pool, engines) = lane(common::budgets());
    let cancel = CancellationToken::new();
    let updates = Arc::new(Mutex::new(Vec::new()));
    let running = run_long(&pool, common::budgets(), &cancel, &updates);
    let pid = running_child(&engines, &updates).await;
    let cancelled = Instant::now();
    cancel.cancel();
    let answer = running.await.unwrap().unwrap_err();
    assert!(cancelled.elapsed() < AT_ONCE, "{:?}", cancelled.elapsed());
    assert_eq!(answer.to_string(), ENGINE_STOPPED);
    gone_at_once(pid).await;
    next_test_runs_on_a_new_child(&pool, &engines).await;
    let pids = engines.pids();
    pool.stop().await;
    for pid in pids {
        gone_at_once(pid).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "needs MCBENCH_TABLES (Minecraft's tables) and MCBENCH_ENGINE_PROGRAM (a built mcbench-lite)"]
async fn a_dense_test_past_its_wall_budget_answers_at_once_its_child_goes_and_its_lane_is_free() {
    let budgets = with("wallMs", 3000.0);
    let (pool, engines) = lane(budgets.clone());
    let updates = Arc::new(Mutex::new(Vec::new()));
    let began = Instant::now();
    let running = run_long(&pool, budgets, &CancellationToken::new(), &updates);
    let pid = running_child(&engines, &updates).await;
    let answer = running.await.unwrap().unwrap_err();
    let elapsed = began.elapsed();
    assert!(
        elapsed >= Duration::from_secs(3) && elapsed < Duration::from_secs(3) + AT_ONCE,
        "{elapsed:?}"
    );
    assert!(
        answer.to_string().starts_with(
            "The test was too dense to finish within its 3-second wall-clock budget (MCBENCH_VERIFY_MAX_WALL_MS) and was stopped: it "
        ),
        "{answer}"
    );
    let budget = answer.budget_object().unwrap();
    assert_eq!(budget["resource"], "wallMs");
    assert_eq!(budget["totalTicks"], json!(70000));
    assert!(budget["ticksReached"].as_f64().unwrap() < 70000.0);
    gone_at_once(pid).await;
    next_test_runs_on_a_new_child(&pool, &engines).await;
    let pids = engines.pids();
    pool.stop().await;
    for pid in pids {
        gone_at_once(pid).await;
    }
}
