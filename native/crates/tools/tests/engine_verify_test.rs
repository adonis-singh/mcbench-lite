//! `test/engine-verify.test.js`, the cases that are the engine verifier's
//! (`mcbench-engine`'s `tests/engine.rs` has the engine's): a test and its
//! archived report, failures, a cancel while the engine runs, the wall
//! budget with the ticks it reached, an unported reach, a test cancelled
//! while its engine child starts (today: while its engine program is
//! built), the sweep of the children's folders a crash left, and the pool's
//! memory reading.

mod common;

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use common::*;
use mcbench_engine::{EngineError, EngineProgram, VerifyEngines};
use mcbench_tools::verify::{Minecraft, VerifierPool, sweep_runs};
use serde_json::json;
use tokio_util::sync::CancellationToken;

fn stone() -> mcbench_world::World {
    world_at([0, 0, 0])
}

/// "At once" for an answer that would otherwise never come: a bound a
/// starved machine meets.
const AT_ONCE: Duration = Duration::from_secs(10);

/// Waits until `done`, failing after a minute (a hang, not a slow machine).
async fn until(what: &str, done: impl Fn() -> bool) {
    let started = Instant::now();
    while !done() {
        assert!(started.elapsed() < Duration::from_secs(60), "{what}");
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
}

#[tokio::test]
async fn the_engine_runs_a_test_and_its_report_comes_back_archived() {
    let engine = FakeEngine::new(|request| Box::pin(async move { Ok(complete(&request)) }));
    let (pool, _root) = verifier(engine.clone(), budgets());
    let mut phases = Vec::new();
    let result = pool
        .run(
            &stone(),
            &json!({ "probes": [[0, 0, 0]] }),
            &settings(budgets()),
            &CancellationToken::new(),
            &mut |u| phases.push(u["phase"].as_str().unwrap().to_string()),
            None,
        )
        .await
        .unwrap();
    assert_eq!(result["ok"], json!(true));
    assert!(std::path::Path::new(result["reportFile"].as_str().unwrap()).is_file());
    let (request, commands) = engine.verified.lock().unwrap()[0].clone();
    assert!(
        commands.starts_with("setblock 0 64 0 minecraft:stone"),
        "{commands}"
    );
    assert_eq!(request["placementMode"], "updates");
    assert!(phases.iter().any(|p| p == "running"));
    let keys: Vec<&String> = result["timings"].as_object().unwrap().keys().collect();
    assert_eq!(keys, ["startupMs", "simulationMs", "archiveMs"]);
}

#[tokio::test]
async fn a_verifiers_first_test_has_the_engine_sweep_its_leftover_folders_once() {
    let engine = FakeEngine::new(|request| Box::pin(async move { Ok(complete(&request)) }));
    let (pool, _root) = verifier(engine.clone(), with("wallMs", 60_000.0));
    for _ in 0..2 {
        pool.run(
            &stone(),
            &json!({ "probes": [[0, 0, 0]] }),
            &settings(with("wallMs", 60_000.0)),
            &CancellationToken::new(),
            &mut |_| {},
            None,
        )
        .await
        .unwrap();
    }
    // Twice the wall budget, as today's `sweepRuns(runs, 2 * wallMs)`.
    assert_eq!(*engine.swept.lock().unwrap(), [120_000.0]);
}

#[tokio::test]
async fn a_failed_experiment_incomplete_observations_and_an_engine_fault_are_errors() {
    let run = |engine: Arc<FakeEngine>| async move {
        let (pool, _root) = verifier(engine, budgets());
        pool.run(
            &stone(),
            &json!({ "probes": [[0, 0, 0]] }),
            &settings(budgets()),
            &CancellationToken::new(),
            &mut |_| {},
            None,
        )
        .await
        .unwrap_err()
        .to_string()
    };
    let failed = FakeEngine::new(|_| {
        Box::pin(async move {
            Ok(mcbench_engine::VerifyReport {
                result: r#"{"ok":false,"error":"java.io.IOException: No block entity at [0,0,0]"}"#
                    .into(),
                files: Vec::new(),
            })
        })
    });
    assert_eq!(
        run(failed).await,
        "Minecraft verification failed: java.io.IOException: No block entity at [0,0,0]"
    );
    let incomplete = FakeEngine::new(|request| {
        Box::pin(async move {
            Ok(complete_with(
                &request,
                r#"{"initial":{},"observations":[]}"#,
                |r| {
                    r.insert("sampleCount".into(), json!(0));
                },
            ))
        })
    });
    assert_eq!(
        run(incomplete).await,
        "Minecraft verification returned incomplete observations"
    );
    let fault = FakeEngine::new(|_| {
        Box::pin(async move { Err(EngineError::Failed("java.lang.NullPointerException".into())) })
    });
    assert_eq!(
        run(fault).await,
        "Minecraft verification failed: java.lang.NullPointerException"
    );
    let rejected = FakeEngine::new(|_| {
        Box::pin(async move {
            Err(EngineError::Failed(
                "java.lang.IllegalStateException: No block entity at [0, 0, 0] <- x".into(),
            ))
        })
    });
    assert_eq!(
        run(rejected).await,
        "Minecraft verification could not start: MCBENCH_ERROR No block entity at [0, 0, 0]\n"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancelling_a_running_test_stops_its_engine_and_its_lane_is_free_at_once() {
    let gate = Arc::new(tokio::sync::Semaphore::new(0));
    let held = gate.clone();
    let engine = FakeEngine::new(move |request| {
        let gate = held.clone();
        Box::pin(async move {
            let _ = gate.acquire().await.unwrap();
            Ok(complete(&request))
        })
    });
    let (pool, _root) = verifier(engine.clone(), budgets());
    let pool = Arc::new(pool);
    let cancel = CancellationToken::new();
    let running = {
        let (pool, cancel) = (pool.clone(), cancel.clone());
        tokio::spawn(async move {
            pool.run(
                &stone(),
                &json!({ "probes": [[0, 0, 0]] }),
                &settings(budgets()),
                &cancel,
                &mut |_| {},
                None,
            )
            .await
        })
    };
    until("the test reaches its engine", || {
        !engine.verified.lock().unwrap().is_empty()
    })
    .await;
    let cancelled = Instant::now();
    cancel.cancel();
    let refused = running.await.unwrap().unwrap_err();
    // The engine never answers (its gate stays shut): the cancel's answer
    // came from the cancel, and a bound a starved machine meets says it
    // came at once.
    assert!(cancelled.elapsed() < AT_ONCE);
    assert_eq!(refused.to_string(), "The engine stopped (SIGTERM): ");
    engine.until_stopped(1).await;
    assert_eq!(engine.stopped.load(Ordering::SeqCst), 1);
    // The next test runs at once: the cancelled one's engine was stopped.
    let next = {
        let pool = pool.clone();
        tokio::spawn(async move {
            pool.run(
                &stone(),
                &json!({ "probes": [[0, 0, 0]] }),
                &settings(budgets()),
                &CancellationToken::new(),
                &mut |_| {},
                None,
            )
            .await
        })
    };
    while engine.verified.lock().unwrap().len() < 2 {
        assert!(cancelled.elapsed() < AT_ONCE, "the lane is still taken");
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
    gate.add_permits(1);
    assert!(next.await.unwrap().is_ok());
    assert_eq!(engine.finished.load(Ordering::SeqCst), 2);
}

/// The wall budget of the test that outlasts it: the budget runs from the
/// experiment's start and the ticks it reached are known only once its
/// engine runs, so the budget is one a starved machine reaches the engine
/// inside.
const WALL_MS: f64 = 5000.0;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_test_that_outlasts_the_wall_budget_fails_plainly_with_the_ticks_it_reached() {
    let gate = Arc::new(tokio::sync::Semaphore::new(0));
    let held = gate.clone();
    let engine = FakeEngine::with_progress(
        move |request| {
            let gate = held.clone();
            Box::pin(async move {
                let _ = gate.acquire().await.unwrap();
                Ok(complete(&request))
            })
        },
        |_| vec![r#"{"phase":"running","tick":3200,"totalTicks":9000}"#.to_string()],
    );
    let (pool, _root) = verifier(engine.clone(), with("wallMs", WALL_MS));
    let refused = pool
        .run(
            &stone(),
            &json!({ "probes": [[0, 0, 0]] }),
            &settings(with("wallMs", WALL_MS)),
            &CancellationToken::new(),
            &mut |_| {},
            None,
        )
        .await
        .unwrap_err();
    engine.until_stopped(1).await;
    assert_eq!(
        engine.stopped.load(Ordering::SeqCst),
        1,
        "the engine was stopped once"
    );
    assert_eq!(
        refused.to_string(),
        "The test was too dense to finish within its 5-second wall-clock budget (MCBENCH_VERIFY_MAX_WALL_MS) and was stopped: it reached tick 3200 of 9000 of its timeline, after its settling ticks. Shorten it (fewer ticks, a smaller area, less active circuitry) or split it into several tests; this is a limit of the harness, not a result about the build."
    );
    let budget = refused.budget_object().unwrap();
    assert_eq!(budget["resource"], "wallMs");
    assert!(budget["requested"].as_f64().unwrap() >= WALL_MS);
    assert_eq!(budget["allowed"], json!(5000));
    assert_eq!(budget["setting"], "MCBENCH_VERIFY_MAX_WALL_MS");
    assert_eq!(budget["phase"], "running");
    assert_eq!(budget["ticksReached"], json!(3200));
    assert_eq!(budget["totalTicks"], json!(9000));
    let failure = mcbench_tools::verify::failure(&refused, false);
    assert_eq!(failure["code"], "VERIFICATION_BUDGET_EXCEEDED");
}

#[tokio::test]
async fn a_test_that_reaches_what_the_engine_does_not_model_fails_with_the_reason_and_nothing_runs_it_again()
 {
    let engine = FakeEngine::new(|_| {
        Box::pin(async move {
            Err(EngineError::Failed(
                "unported: LiquidBlock.getDrops (minecraft:water[level=1])".into(),
            ))
        })
    });
    let (pool, _root) = verifier(engine.clone(), budgets());
    let refused = pool
        .run(
            &stone(),
            &json!({ "probes": [[0, 0, 0]] }),
            &settings(budgets()),
            &CancellationToken::new(),
            &mut |_| {},
            None,
        )
        .await;
    assert_eq!(
        refused.unwrap_err().to_string(),
        "Minecraft verification failed: unported: LiquidBlock.getDrops (minecraft:water[level=1])"
    );
    assert_eq!(engine.verified.lock().unwrap().len(), 1);
}

// A test cancelled while its engine program was being built returned at
// once and nothing spawned. A child is started, not built: a test cancelled
// while its child is starting (it never says it is ready) returns at once,
// and the child is gone.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_test_cancelled_while_its_engine_starts_returns_at_once_and_the_engine_is_gone() {
    let folder = scratch("starting");
    #[cfg(unix)]
    let program = {
        let program = folder.join("never-ready.sh");
        write_program(&program, "#!/bin/sh\nexec sleep 600\n");
        program
    };
    #[cfg(windows)]
    let program = {
        let program = folder.join("never-ready.cmd");
        std::fs::write(
            &program,
            "@powershell -NoProfile -NonInteractive -Command Start-Sleep 600\r\n",
        )
        .unwrap();
        program
    };
    let engines = Arc::new(VerifyEngines::new(EngineProgram::new(
        &program,
        folder.path_buf(),
    )));
    let starting_root = scratch("starting-root");
    let pool = Arc::new(VerifierPool::single(
        starting_root.path_buf(),
        Minecraft {
            engine: engines.clone(),
            facts: Arc::new(FakeFacts),
            tags: Arc::new(tags()),
            y_offset: 64,
        },
        settings(budgets()),
    ));
    let cancel = CancellationToken::new();
    let running = {
        let (pool, cancel) = (pool.clone(), cancel.clone());
        tokio::spawn(async move {
            pool.run(
                &stone(),
                &json!({ "probes": [[0, 0, 0]] }),
                &settings(budgets()),
                &cancel,
                &mut |_| {},
                None,
            )
            .await
        })
    };
    until("the engine child is started", || !engines.pids().is_empty()).await;
    let pid = engines.pids()[0];
    tokio::time::sleep(Duration::from_millis(20)).await;
    let cancelled = Instant::now();
    cancel.cancel();
    let refused = running.await.unwrap().unwrap_err();
    // The child never says it is ready: the answer came from the cancel.
    assert!(cancelled.elapsed() < AT_ONCE);
    assert_eq!(refused.to_string(), "The engine stopped (SIGTERM): ");
    while alive(pid) {
        assert!(
            cancelled.elapsed() < AT_ONCE,
            "the engine {pid} is still running"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(engines.pids().is_empty());
    pool.stop().await;
}

#[test]
fn run_folders_a_crash_left_are_swept_and_ones_in_use_are_kept() {
    let runs = scratch("runs");
    let own = format!("mcbench-engine-child-{}-0", std::process::id());
    // Process ids no process has (above every system's pid limit).
    let gone = "mcbench-engine-child-2147483000-0";
    let young = "mcbench-engine-child-2147483001-0";
    // Process 1 is always running.
    let running = "mcbench-engine-child-1-0";
    // Last changed long ago: an hour back, far older than the sweep's age
    // however slowly this runs. The young one is made now.
    let long_ago = std::time::SystemTime::now() - Duration::from_secs(3600);
    for name in [own.as_str(), gone, running, "unrelated"] {
        let folder = runs.join(name);
        std::fs::create_dir_all(&folder).unwrap();
        // A folder's times are set through a handle that may write them:
        // read-only on Unix; on Windows a folder opens only with
        // FILE_FLAG_BACKUP_SEMANTICS (0x0200_0000), and its times are
        // written with FILE_WRITE_ATTRIBUTES (0x0100).
        #[cfg(unix)]
        let handle = std::fs::File::open(&folder).unwrap();
        #[cfg(windows)]
        let handle = {
            use std::os::windows::fs::OpenOptionsExt;
            std::fs::OpenOptions::new()
                .access_mode(0x0100)
                .custom_flags(0x0200_0000)
                .open(&folder)
                .unwrap()
        };
        handle.set_modified(long_ago).unwrap();
    }
    std::fs::create_dir_all(runs.join(young)).unwrap();
    sweep_runs(&runs, 600_000.0);
    let mut left: Vec<String> = std::fs::read_dir(&runs)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    left.sort();
    let mut kept = vec![own.clone(), young.to_string(), "unrelated".to_string()];
    // On Windows a running child's folder cannot be removed: the rule there
    // is the age alone.
    if cfg!(unix) {
        kept.push(running.to_string());
    }
    kept.sort();
    assert_eq!(left, kept);
}

#[tokio::test]
async fn a_verifier_pool_whose_memory_reading_failed_reads_it_again_for_the_next_test() {
    let reads = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counted = reads.clone();
    let memory_root = scratch("memory");
    let pool = VerifierPool::new(
        memory_root.path_buf(),
        minecraft(FakeEngine::new(|request| {
            Box::pin(async move { Ok(complete(&request)) })
        })),
        settings(budgets()),
        Box::new(move || {
            if counted.fetch_add(1, Ordering::SeqCst) == 0 {
                Err("vm_stat failed".to_string())
            } else {
                Ok(16 << 30)
            }
        }),
    );
    let (world, args, settings, never) = (
        stone(),
        json!({ "probes": [[0, 0, 0]] }),
        settings(budgets()),
        CancellationToken::new(),
    );
    let first = pool
        .run(&world, &args, &settings, &never, &mut |_| {}, None)
        .await;
    assert_eq!(first.unwrap_err().to_string(), "vm_stat failed");
    assert!(
        pool.run(&world, &args, &settings, &never, &mut |_| {}, None)
            .await
            .is_ok()
    );
    assert_eq!(reads.load(Ordering::SeqCst), 2);
}
