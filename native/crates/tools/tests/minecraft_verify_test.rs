//! `test/minecraft-verify.test.js`, the cases whose subject is the engine
//! verifier: the checks of a call and of its experiments, the queue, its
//! cancellation and progress, the budgets, the archive and the pool. The
//! Paper verifier's own cases (its asynchronous chunk preparation, an
//! import cancelled inside Paper, its kept runtime and the runtime's exit,
//! the plugin's compiler) are not here: the native app has no Paper
//! runtime (see `mcbench_tools::verify`'s doc).

mod common;

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use common::*;

/// Waits until `done`, failing after a minute (a hang, not a slow machine).
async fn until(what: &str, done: impl Fn() -> bool) {
    let started = std::time::Instant::now();
    while !done() {
        assert!(started.elapsed() < Duration::from_secs(60), "{what}");
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
}
use mcbench_engine::BlockState;
use mcbench_tools::verify::{
    Placement, Settings, VerifierPool, VerifyError, call_tests, validate_request,
};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

fn validate(args: &Value) -> Result<mcbench_tools::verify::Experiment, VerifyError> {
    validate_request(&world(), args, 64, &settings(budgets()), &tags())
}

fn validate_with(
    args: &Value,
    budgets: mcbench_tools::verify::Limits,
) -> Result<mcbench_tools::verify::Experiment, VerifyError> {
    validate_request(&world(), args, 64, &settings(budgets), &tags())
}

fn strict(args: &Value) -> Result<mcbench_tools::verify::Experiment, VerifyError> {
    let settings = Settings {
        placement: Placement::Strict,
        ..settings(budgets())
    };
    validate_request(&world(), args, 64, &settings, &tags())
}

fn error(result: Result<mcbench_tools::verify::Experiment, VerifyError>) -> String {
    result.expect_err("an error").to_string()
}

fn budget_of(result: Result<mcbench_tools::verify::Experiment, VerifyError>) -> Value {
    Value::Object(
        result
            .expect_err("an error")
            .budget_object()
            .expect("a budget error")
            .clone(),
    )
}

#[test]
fn sample_normalization_preserves_caller_input_and_handles_ordered_and_unordered_duplicates() {
    for input in [
        json!([5, 5, 5]),
        json!([0, 1, 1, 2, 9]),
        json!([9, 2, 0, 9, 1, 2]),
    ] {
        let original = input.clone();
        let args = json!({ "probes": [[0, 0, 0]], "samples": input });
        let result = validate(&args).unwrap();
        let mut expected: Vec<i64> = original
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_i64().unwrap())
            .collect();
        expected.sort();
        expected.dedup();
        assert_eq!(result.request["samples"], json!(expected));
        assert_eq!(args["samples"], original);
    }
}

#[test]
fn observation_density_and_input_counts_trade_against_one_work_budget_with_no_old_per_field_ceilings()
 {
    let probes: Vec<Value> = (0..4096).map(|i| json!([i % 64, 0, i / 64])).collect();
    let samples: Vec<i64> = (0..129).map(|i| i * 10).collect();
    let actions: Vec<Value> = (0..64)
        .map(|i| json!({ "tick": i, "type": "toggle", "at": [0, 0, 0] }))
        .collect();
    let result = validate(
        &json!({ "probes": probes, "samples": samples, "actions": actions, "settle_ticks": 300 }),
    )
    .unwrap();
    assert_eq!(result.cost()["operations"], json!(4096 * 130 + 64));
    assert_eq!(result.cost()["simulationTicks"], json!(1580));
    for (probes, samples) in [(100, 9), (2, 499)] {
        let request = json!({ "probes": vec![json!([0, 0, 0]); probes], "samples": (0..samples).collect::<Vec<i64>>() });
        assert_eq!(
            validate_with(&request, with("operations", 1000.0))
                .unwrap()
                .cost()["operations"],
            json!(1000)
        );
        let mut more = request.clone();
        more["actions"] = json!([{ "tick": 0, "type": "press", "at": [0, 0, 0] }]);
        assert_eq!(
            budget_of(validate_with(&more, with("operations", 1000.0))),
            json!({ "resource": "operations", "requested": 1001, "allowed": 1000, "setting": "MCBENCH_VERIFY_MAX_OPERATIONS" })
        );
    }
}

#[test]
fn operator_configuration_is_validated_and_time_request_budgets_are_explicit() {
    // readBudgets' half is budgets.rs's unit test.
    assert_eq!(
        budget_of(validate(
            &json!({ "probes": [[0, 0, 0]], "samples": [72000 - 3999], "settle_ticks": 4000 })
        ))["resource"],
        "simulationTicks"
    );
    assert_eq!(
        budget_of(validate_with(
            &json!({ "probes": [[0, 0, 0]] }),
            with("requestBytes", 1.0)
        ))["resource"],
        "requestBytes"
    );
}

#[test]
fn a_call_holds_a_list_of_tests_within_the_tests_budget() {
    let call = json!({ "tests": [{ "probes": [[0, 0, 0]] }, { "probes": [[1, 0, 0]] }] });
    let call = call.as_object().unwrap();
    assert_eq!(
        call_tests(call, &budgets()).unwrap(),
        call["tests"].as_array().unwrap().as_slice()
    );
    assert!(call_tests(call, &with("tests", 2.0)).is_ok());
    let refused = call_tests(call, &with("tests", 1.0)).unwrap_err();
    assert_eq!(
        Value::Object(refused.budget_object().unwrap().clone()),
        json!({ "resource": "tests", "requested": 2, "allowed": 1, "setting": "MCBENCH_VERIFY_MAX_TESTS" })
    );
    for args in [
        json!({}),
        json!({ "tests": [] }),
        json!({ "tests": "x" }),
        json!({ "probes": [[0, 0, 0]] }),
        json!({ "tests": [{}], "samples": [0] }),
    ] {
        assert_eq!(
            call_tests(args.as_object().unwrap(), &budgets())
                .unwrap_err()
                .to_string(),
            "test_build requires {tests: [one experiment, ...]}"
        );
    }
}

#[test]
fn verification_validates_untrusted_input_tick_limits_height_and_simulation_coverage_before_boot() {
    let normalized = validate(&json!({ "probes": [[-1, 0, 0]], "samples": [20, 0, 20] })).unwrap();
    assert_eq!(normalized.request["samples"], json!([0, 20]));
    assert_eq!(normalized.request["yOffset"], json!(64));
    assert!(
        normalized.request["columns"]
            .as_array()
            .unwrap()
            .contains(&json!([-2, -1]))
    );
    assert!(
        error(validate(
            &json!({ "probes": [[0, 0, 0]], "command": "setblock" })
        ))
        .contains("Unknown")
    );
    assert!(error(validate(&json!({ "probes": [[0, "0\nstop", 0]] }))).contains("coordinate"));
    assert!(error(validate(&json!({ "probes": [[0, 896, 0]] }))).contains("coordinate"));
    assert!(
        error(validate(
            &json!({ "probes": [[0, 0, 0]], "samples": [72000] })
        ))
        .contains("simulationTicks")
    );
    assert!(error(validate(&json!({ "probes": [[0, 0, 0]], "samples": [0], "actions": [{ "tick": 1, "type": "press", "at": [0, 0, 0] }] }))).contains("action tick"));
    assert_eq!(
        error(validate(
            &json!({ "probes": [[0, 0, 0]], "actions": [{ "tick": 0, "type": "setblock", "at": [0, 0, 0] }] })
        )),
        "Malformed input action"
    );
    assert!(
        error(validate(
            &json!({ "probes": [[0, 0, 0]], "settle_ticks": 0 })
        ))
        .contains("settle_ticks")
    );
    // A use may say the yaw the player faces; no other input takes one.
    assert_eq!(
        validate(&json!({ "probes": [[0, 0, 0]], "actions": [{ "tick": 0, "type": "use", "at": [0, 0, 0], "yaw": -44.5 }, { "tick": 0, "type": "use", "at": [0, 0, 0] }] })).unwrap().request["actions"],
        json!([{ "tick": 0, "type": "use", "at": [0, 0, 0], "yaw": -44.5 }, { "tick": 0, "type": "use", "at": [0, 0, 0] }])
    );
    assert_eq!(
        error(validate(
            &json!({ "probes": [[0, 0, 0]], "actions": [{ "tick": 0, "type": "use", "at": [0, 0, 0], "yaw": "90" }] })
        )),
        "yaw must be a finite number of degrees"
    );
    assert_eq!(
        error(validate(
            &json!({ "probes": [[0, 0, 0]], "actions": [{ "tick": 0, "type": "toggle", "at": [0, 0, 0], "yaw": 90 }] })
        )),
        "Malformed input action"
    );
    let mut scattered = mcbench_world::World::new();
    for x in (-4096..=4096).step_by(48) {
        for z in (-4096..=4096).step_by(48) {
            scattered
                .set([x, 0, z], BlockState::new("minecraft:stone", &[]))
                .unwrap();
        }
    }
    scattered.finalize();
    let refused = validate_request(
        &scattered,
        &json!({ "probes": [[0, 0, 0]] }),
        64,
        &settings(budgets()),
        &tags(),
    );
    assert!(error(refused).contains("columns"));
}

/// A verifier whose engine answers with an air report, holding each
/// experiment until `gate` opens when one is given.
fn air(gate: Option<Arc<tokio::sync::Semaphore>>) -> Arc<FakeEngine> {
    FakeEngine::new(move |request| {
        let gate = gate.clone();
        Box::pin(async move {
            if let Some(gate) = gate {
                let _ = gate.acquire().await.unwrap();
            }
            let report = air_report(&request);
            Ok(complete_with(&request, &report, |_| {}))
        })
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn queued_verification_snapshots_revisions_and_returns_observed_failure_without_repairing_or_grading()
 {
    let gate = Arc::new(tokio::sync::Semaphore::new(0));
    let engine = air(Some(gate.clone()));
    let (pool, _root) = verifier(engine.clone(), budgets());
    let pool = Arc::new(pool);
    let mut build = world();
    let revision = build.revision();
    let args = json!({ "probes": [[-1, 0, 0]], "samples": [0], "actions": [{ "tick": 0, "type": "press", "at": [-1, 0, 0] }] });
    // The chat's world shares its chunks with the one the test was given; the
    // chat's world changes while the test waits.
    let given = build.clone();
    let pending = {
        let pool = pool.clone();
        tokio::spawn(async move {
            pool.run(
                &given,
                &args,
                &settings(budgets()),
                &CancellationToken::new(),
                &mut |_| {},
                None,
            )
            .await
        })
    };
    until("the test waits behind its engine", || {
        !engine.verified.lock().unwrap().is_empty()
    })
    .await;
    build
        .set([-1, 0, 0], BlockState::new("minecraft:gold_block", &[]))
        .unwrap();
    build.finalize();
    gate.add_permits(1);
    let result = pending.await.unwrap().unwrap();
    assert_eq!(result["worldRevision"], json!(revision));
    assert!(
        engine.verified.lock().unwrap()[0]
            .1
            .contains("minecraft:stone")
    );
    assert_eq!(result["ok"], json!(true));
    assert_eq!(result["inputs"][0]["applied"], json!(false));
    assert_eq!(
        result["observations"][0]["blocks"][0]["block"],
        json!("minecraft:air")
    );
    assert!(!result.contains_key("passed"));
}

#[tokio::test]
async fn incomplete_engine_observations_never_become_successful_verification() {
    let engine = FakeEngine::new(|request| {
        Box::pin(async move {
            let report = air_report(&request);
            Ok(complete_with(&request, &report, |r| {
                r.insert("sampleCount".into(), json!(0));
            }))
        })
    });
    let (pool, _root) = verifier(engine, budgets());
    let refused = pool
        .run(
            &world(),
            &json!({ "probes": [[0, 0, 0]] }),
            &settings(budgets()),
            &CancellationToken::new(),
            &mut |_| {},
            None,
        )
        .await;
    assert_eq!(
        refused.unwrap_err().to_string(),
        "Minecraft verification returned incomplete observations"
    );
}

#[tokio::test]
async fn cancelled_queued_tests_do_not_start_a_server() {
    let engine = air(None);
    let (pool, _root) = verifier(engine.clone(), budgets());
    let cancelled = CancellationToken::new();
    cancelled.cancel();
    let refused = pool
        .run(
            &world(),
            &json!({ "probes": [[0, 0, 0]] }),
            &settings(budgets()),
            &cancelled,
            &mut |_| {},
            None,
        )
        .await;
    assert_eq!(
        refused.unwrap_err().to_string(),
        "This operation was aborted"
    );
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(engine.verified.lock().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancelling_behind_an_active_test_returns_promptly_without_stopping_it_or_bypassing_cleanup()
 {
    let gate = Arc::new(tokio::sync::Semaphore::new(0));
    let engine = air(Some(gate.clone()));
    let (pool, _root) = verifier(engine.clone(), budgets());
    let pool = Arc::new(pool);
    let settings = settings(budgets());
    let first = {
        let (pool, settings) = (pool.clone(), settings.clone());
        tokio::spawn(async move {
            pool.run(
                &world(),
                &json!({ "probes": [[0, 0, 0]] }),
                &settings,
                &CancellationToken::new(),
                &mut |_| {},
                None,
            )
            .await
        })
    };
    until("the first test reaches its engine", || {
        !engine.verified.lock().unwrap().is_empty()
    })
    .await;
    let cancel = CancellationToken::new();
    let queued = {
        let (pool, settings, cancel) = (pool.clone(), settings.clone(), cancel.clone());
        tokio::spawn(async move {
            pool.run(
                &world(),
                &json!({ "probes": [[0, 0, 0]] }),
                &settings,
                &cancel,
                &mut |_| {},
                None,
            )
            .await
        })
    };
    tokio::time::sleep(Duration::from_millis(20)).await;
    cancel.cancel();
    // Its answer would otherwise wait for the first test, which waits for
    // its gate: "promptly" is a bound a starved machine meets.
    let refused = tokio::time::timeout(Duration::from_secs(10), queued)
        .await
        .expect("the queued cancel returns promptly")
        .unwrap();
    assert_eq!(
        refused.unwrap_err().to_string(),
        "This operation was aborted"
    );
    assert_eq!(
        engine.finished.load(Ordering::SeqCst),
        0,
        "a queued cancellation does not stop another test"
    );
    let next = {
        let (pool, settings) = (pool.clone(), settings.clone());
        tokio::spawn(async move {
            pool.run(
                &world(),
                &json!({ "probes": [[0, 0, 0]] }),
                &settings,
                &CancellationToken::new(),
                &mut |_| {},
                None,
            )
            .await
        })
    };
    gate.add_permits(2);
    assert!(first.await.unwrap().is_ok());
    assert!(next.await.unwrap().is_ok());
    assert_eq!(
        engine.verified.lock().unwrap().len(),
        2,
        "the cancelled test never reached the engine"
    );
}

#[tokio::test]
async fn progress_follows_the_execution_lifecycle_and_shutdown_rejects_future_calls() {
    let engine = FakeEngine::with_progress(
        |request| {
            Box::pin(async move {
                let report = air_report(&request);
                Ok(complete_with(&request, &report, |_| {}))
            })
        },
        |request| {
            vec![
                mcbench_json::stringify(
                    &json!({ "phase": "settling", "tick": 20, "totalTicks": 40 }),
                ),
                mcbench_json::stringify(
                    &json!({ "phase": "running", "tick": 100, "totalTicks": request["duration"] }),
                ),
            ]
        },
    );
    let (pool, _root) = verifier(engine.clone(), budgets());
    let mut events = Vec::new();
    pool.run(
        &world(),
        &json!({ "probes": [[0, 0, 0]] }),
        &settings(budgets()),
        &CancellationToken::new(),
        &mut |e| events.push(e),
        None,
    )
    .await
    .unwrap();
    let phases: Vec<&str> = events
        .iter()
        .map(|e| e["phase"].as_str().unwrap())
        .collect();
    assert_eq!(
        phases,
        [
            "queued",
            "preparing",
            "running",
            "settling",
            "running",
            "running"
        ]
    );
    let messages: Vec<&str> = events
        .iter()
        .map(|e| e["message"].as_str().unwrap())
        .collect();
    assert_eq!(
        messages,
        [
            "Waiting for the Minecraft verifier",
            "Preparing Minecraft",
            "Running Minecraft inputs and observations",
            "Settling Minecraft: 20/40 ticks",
            "Observing Minecraft: 100/100 ticks",
            "Minecraft observations complete",
        ]
    );
    assert_eq!(events[3], json!({ "phase": "settling", "tick": 20, "totalTicks": 40, "message": "Settling Minecraft: 20/40 ticks" }).as_object().unwrap().clone());
    pool.stop().await;
    let refused = pool
        .run(
            &world(),
            &json!({ "probes": [[0, 0, 0]] }),
            &settings(budgets()),
            &CancellationToken::new(),
            &mut |_| {},
            None,
        )
        .await;
    assert_eq!(
        refused.unwrap_err().to_string(),
        "Minecraft verification is shutting down"
    );
    assert_eq!(engine.verified.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn runtime_and_report_budget_exhaustion_are_harness_errors_with_cleanup() {
    // A run cut off by its wall budget; a report over budget from a finished run.
    let slow = FakeEngine::new(|request| {
        Box::pin(async move {
            // Far past its budget, however slowly the machine runs.
            tokio::time::sleep(Duration::from_secs(60)).await;
            Ok(complete(&request))
        })
    });
    let (pool, _root) = verifier(slow.clone(), with("wallMs", 5.0));
    let refused = pool
        .run(
            &world(),
            &json!({ "probes": [[0, 0, 0]] }),
            &settings(with("wallMs", 5.0)),
            &CancellationToken::new(),
            &mut |_| {},
            None,
        )
        .await;
    assert_eq!(
        refused.unwrap_err().budget_object().unwrap()["resource"],
        "wallMs"
    );
    let (pool, root) = verifier(air(None), with("reportBytes", 1.0));
    let refused = pool
        .run(
            &world(),
            &json!({ "probes": [[0, 0, 0]] }),
            &settings(with("reportBytes", 1.0)),
            &CancellationToken::new(),
            &mut |_| {},
            None,
        )
        .await;
    assert_eq!(
        refused.unwrap_err().budget_object().unwrap()["resource"],
        "reportBytes"
    );
    assert!(
        !root.join("reports").exists(),
        "a report over budget is not archived"
    );
}

#[tokio::test]
async fn archived_reports_survive_later_tests_and_large_reports_are_not_inlined() {
    let (pool, _root) = verifier(air(None), budgets());
    let probes: Vec<Value> = (0..4096).map(|i| json!([i % 64, 0, i / 64])).collect();
    let never = CancellationToken::new();
    let result = pool
        .run(
            &world(),
            &json!({ "probes": probes, "samples": [0, 1] }),
            &settings(budgets()),
            &never,
            &mut |_| {},
            None,
        )
        .await
        .unwrap();
    assert_eq!(result["observationsInFile"], json!(true));
    assert!(!result.contains_key("observations"));
    let file = result["reportFile"].as_str().unwrap().to_string();
    let bytes = std::fs::read(&file).unwrap();
    pool.run(
        &world(),
        &json!({ "probes": [[0, 0, 0]] }),
        &settings(budgets()),
        &never,
        &mut |_| {},
        None,
    )
    .await
    .unwrap();
    assert_eq!(std::fs::read(&file).unwrap(), bytes);
    let report: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        report["observations"][1]["blocks"]
            .as_array()
            .unwrap()
            .len(),
        4096
    );
}

#[tokio::test]
async fn placement_mode_is_frozen_with_each_queued_experiment_and_invalid_modes_are_rejected() {
    assert_eq!(
        Placement::parse("approximate").unwrap_err(),
        "Unsupported placement mode"
    );
    let engine = air(None);
    let (pool, _root) = verifier(engine.clone(), budgets());
    let never = CancellationToken::new();
    let strict = Settings {
        placement: Placement::Strict,
        ..settings(budgets())
    };
    pool.run(
        &world(),
        &json!({ "probes": [[0, 0, 0]], "samples": [0] }),
        &strict,
        &never,
        &mut |_| {},
        None,
    )
    .await
    .unwrap();
    assert_eq!(engine.requests().last().unwrap()["placementMode"], "strict");
    pool.run(
        &world(),
        &json!({ "probes": [[0, 0, 0]], "samples": [0] }),
        &settings(budgets()),
        &never,
        &mut |_| {},
        None,
    )
    .await
    .unwrap();
    assert_eq!(
        engine.requests().last().unwrap()["placementMode"],
        "updates"
    );
}

#[test]
fn dynamic_inventory_and_dropped_item_observation_are_opt_in_and_validated() {
    let args = json!({ "probes": [[0, 0, 0]], "inventory_probes": [[0, 0, 0]], "samples": [0], "inventory_missing": "report", "item_entities": true });
    let result = validate(&args).unwrap();
    assert_eq!(result.request["inventory_missing"], "report");
    assert_eq!(result.request["item_entities"], true);
    let mut bad = args.clone();
    bad["inventory_missing"] = json!("empty");
    assert!(error(validate(&bad)).contains("inventory_missing"));
    let mut bad = args.clone();
    bad["item_entities"] = json!("yes");
    assert!(error(validate(&bad)).contains("item_entities"));
    let baseline = validate(&json!({ "probes": [[0, 0, 0]], "samples": [0] })).unwrap();
    assert!(
        !baseline.request.contains_key("inventory_missing")
            && !baseline.request.contains_key("item_entities")
    );
}

#[test]
fn entity_imports_validate_fractional_coordinates_and_include_their_chunk_margin() {
    let entity = json!({ "at": [96.125, 2.5, -96.75], "data_version": 4189, "snbt": "{}" });
    let v = validate(&json!({ "probes": [[0, 0, 0]], "entities": [entity] })).unwrap();
    assert_eq!(v.request["entities"], json!([entity]));
    assert!(
        v.request["columns"]
            .as_array()
            .unwrap()
            .contains(&json!([7, -8]))
    );
    for at in [
        json!([null, 0, 0]),
        json!([0, 896, 0]),
        json!([4096, 0, 0]),
        json!(["1", 0, 0]),
        json!([0, 0]),
    ] {
        let mut e = entity.clone();
        e["at"] = at;
        assert!(validate(&json!({ "probes": [[0, 0, 0]], "entities": [e] })).is_err());
    }
    let mut e = entity.clone();
    e["command"] = json!("summon");
    assert_eq!(
        error(validate(&json!({ "probes": [[0, 0, 0]], "entities": [e] }))),
        "entities require at, data_version and nonempty snbt"
    );
}

#[test]
fn native_drop_supply_validates_all_fields_and_keeps_fractional_inlet_coordinates() {
    let action = json!({ "tick": 0, "type": "drop", "at": [6.5, 2, 2.12], "velocity": [-1, 0, 0], "item": "minecraft:iron_ingot", "count": 1 });
    let validated = &validate(&json!({ "probes": [[0, 0, 0]], "actions": [action] }))
        .unwrap()
        .request["actions"][0];
    for key in ["tick", "type", "at", "velocity", "item", "count"] {
        assert_eq!(validated[key], action[key], "{key}");
    }
    for bad in [
        json!({ "velocity": [0, 0, null] }),
        json!({ "velocity": [11, 0, 0] }),
        json!({ "count": 0 }),
        json!({ "slot": 0 }),
        json!({ "at": [1, 2] }),
        json!({ "item": "stone" }),
    ] {
        let mut a = action.clone();
        for (k, v) in bad.as_object().unwrap() {
            a[k] = v.clone();
        }
        assert!(validate(&json!({ "probes": [[0, 0, 0]], "actions": [a] })).is_err());
    }
    assert!(error(validate(&json!({ "probes": [[0, 0, 0]], "actions": [{ "tick": 0, "type": "toggle", "at": [0.5, 0, 0] }] }))).contains("coordinate"));
}

fn with_fields(base: &Value, change: Value) -> Value {
    let mut out = base.clone();
    for (k, v) in change.as_object().unwrap() {
        out[k] = v.clone();
    }
    out
}

#[test]
fn page_inputs_are_explicit_zero_based_native_lectern_selections() {
    let action = json!({ "tick": 0, "type": "page", "at": [0, 0, 0], "page": 14 });
    assert_eq!(
        validate(&json!({ "probes": [[0, 0, 0]], "actions": [action] }))
            .unwrap()
            .request["actions"],
        json!([action])
    );
    for bad in [
        json!({ "page": -1 }),
        json!({ "page": 1.5 }),
        json!({ "slot": 1 }),
        json!({ "page": 2147483647 }),
        json!({ "at": [0.5, 0, 0] }),
    ] {
        assert!(
            validate(&json!({ "probes": [[0, 0, 0]], "actions": [with_fields(&action, bad)] }))
                .is_err()
        );
    }
}

#[test]
fn book_inputs_require_explicit_boolean_presence_and_integer_lectern_coordinates() {
    let action = json!({ "tick": 0, "type": "book", "at": [0, 0, 0], "present": false });
    assert_eq!(
        validate(&json!({ "probes": [[0, 0, 0]], "actions": [action] }))
            .unwrap()
            .request["actions"],
        json!([action])
    );
    for bad in [
        json!({ "present": 0 }),
        json!({ "present": "false" }),
        json!({ "present": null }),
        json!({ "page": 1 }),
        json!({ "at": [0.5, 0, 0] }),
    ] {
        assert!(
            validate(&json!({ "probes": [[0, 0, 0]], "actions": [with_fields(&action, bad)] }))
                .is_err()
        );
    }
}

#[test]
fn saved_native_queues_retain_signed_order_exactly_and_reject_malformed_entries() {
    let e = json!({ "kind": "fluid", "id": "minecraft:water", "at": [0, 0, 0], "delay": 5, "priority": 0, "sub_tick": "9007199254740993" });
    let saved = json!({ "bounds": [[0, 0, 0], [2, 2, 2]], "data_version": 4189, "entries": [e] });
    let request = json!({ "probes": [[0, 0, 0]], "scheduled_ticks": saved });
    assert_eq!(strict(&request).unwrap().request["scheduled_ticks"], saved);
    for bad in [
        json!({ "priority": 4 }),
        json!({ "sub_tick": 1 }),
        json!({ "sub_tick": "9223372036854775808" }),
        json!({ "delay": 1.5 }),
        json!({ "at": [3, 0, 0] }),
        json!({ "kind": "entity" }),
        json!({ "id": "fake" }),
        json!({ "extra": true }),
    ] {
        let changed = with_fields(&saved, json!({ "entries": [with_fields(&e, bad)] }));
        assert!(strict(&json!({ "probes": [[0, 0, 0]], "scheduled_ticks": changed })).is_err());
    }
    assert!(error(strict(&json!({ "probes": [[0, 0, 0]], "scheduled_ticks": with_fields(&saved, json!({ "entries": [e, e] })) }))).contains("Duplicate"));
    assert!(error(strict(&json!({ "probes": [[0, 0, 0]], "scheduled_ticks": with_fields(&saved, json!({ "bounds": [[2, 2, 2], [0, 0, 0]] })) }))).contains("Reversed"));
}

#[test]
fn world_inventory_census_is_explicit_and_boolean() {
    assert_eq!(
        validate(&json!({ "probes": [[0, 0, 0]], "world_inventories": true }))
            .unwrap()
            .request["world_inventories"],
        true
    );
    assert!(
        error(validate(
            &json!({ "probes": [[0, 0, 0]], "world_inventories": 1 })
        ))
        .contains("boolean")
    );
}

#[test]
fn complete_item_input_is_accepted_only_on_supply_actions_and_retains_its_data_version() {
    let action = json!({ "tick": 0, "type": "insert", "at": [0, 0, 0], "slot": 0, "item": "minecraft:enchanted_book", "count": 1, "stack": { "data_version": 3465, "snbt": "{id:\"minecraft:enchanted_book\",Count:1b}" } });
    let request =
        |actions: Value| json!({ "probes": [[0, 0, 0]], "samples": [0, 1], "actions": actions });
    assert_eq!(
        validate(&request(json!([action]))).unwrap().request["actions"][0]["stack"],
        action["stack"]
    );
    for stack in [
        json!(null),
        json!([]),
        json!({}),
        json!({ "data_version": -1, "snbt": "{}" }),
        json!({ "data_version": 4556, "snbt": "" }),
        json!({ "data_version": 4556, "snbt": "{}", "ignore": true }),
    ] {
        assert!(
            validate(&request(json!([with_fields(
                &action,
                json!({ "stack": stack })
            )])))
            .is_err()
        );
    }
    assert!(
        validate(&request(
            json!([{ "tick": 0, "type": "toggle", "at": [0, 0, 0], "stack": action["stack"] }])
        ))
        .is_err()
    );
    assert!(validate(&request(json!([{ "tick": 0, "type": "extract", "at": [0, 0, 0], "slot": 0, "count": 1, "stack": action["stack"] }]))).is_err());
}

#[test]
fn historical_block_version_is_explicit_and_cannot_silently_coerce_invalid_metadata() {
    let q = json!({ "probes": [[0, 0, 0]], "samples": [0], "block_data_version": 3218 });
    assert_eq!(
        validate(&q).unwrap().request["block_data_version"],
        json!(3218)
    );
    for v in [json!(-1), json!(1.5), json!("3218"), json!(null)] {
        assert!(validate(&with_fields(&q, json!({ "block_data_version": v }))).is_err());
    }
    assert!(
        !validate(&json!({ "probes": [[0, 0, 0]] }))
            .unwrap()
            .request
            .contains_key("block_data_version")
    );
}

#[test]
fn living_census_is_optional_and_accepts_only_a_boolean() {
    let args = json!({ "probes": [[0, 0, 0]], "samples": [0] });
    assert!(
        !validate(&args)
            .unwrap()
            .request
            .contains_key("living_entities")
    );
    for enabled in [false, true] {
        assert_eq!(
            validate(&with_fields(&args, json!({ "living_entities": enabled })))
                .unwrap()
                .request["living_entities"],
            json!(enabled)
        );
    }
    for invalid in [
        json!(0),
        json!(1),
        json!("true"),
        json!(null),
        json!([]),
        json!({}),
    ] {
        assert_eq!(
            error(validate(&with_fields(
                &args,
                json!({ "living_entities": invalid })
            ))),
            "living_entities must be a boolean"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_pool_runs_concurrent_tests_on_as_many_runtimes_as_memory_allows() {
    let gate = Arc::new(tokio::sync::Semaphore::new(0));
    let engine = air(Some(gate.clone()));
    let root = scratch("pool");
    let pool = Arc::new(VerifierPool::new(
        root.path_buf(),
        minecraft(engine.clone()),
        settings(budgets()),
        Box::new(|| Ok(16 << 30)),
    ));
    pool.settings.lock().unwrap().placement = Placement::Strict;
    let mut runs = Vec::new();
    for i in 0..4 {
        let pool = pool.clone();
        let settings = pool.settings.lock().unwrap().clone();
        runs.push(tokio::spawn(async move {
            pool.run(
                &world(),
                &json!({ "probes": [[0, 0, 0]] }),
                &settings,
                &CancellationToken::new(),
                &mut |_| {},
                None,
            )
            .await
        }));
        // Each of the first three has its lane before the next is sent.
        if i < 3 {
            until("the test takes a lane", || {
                engine.verified.lock().unwrap().len() == i + 1
            })
            .await;
        }
    }
    // Three lanes take one test each at once; the fourth waits.
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert_eq!(engine.verified.lock().unwrap().len(), 3);
    gate.add_permits(4);
    let mut lanes = Vec::new();
    for run in runs {
        let result = run.await.unwrap().unwrap();
        lanes.push(
            std::path::Path::new(result["reportFile"].as_str().unwrap())
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .to_path_buf(),
        );
    }
    assert_eq!(
        pool.lane_roots(),
        [root.path_buf(), root.join("lanes/1"), root.join("lanes/2")]
    );
    let mut first_three = lanes[..3].to_vec();
    first_three.sort();
    let mut expected = pool.lane_roots();
    expected.sort();
    assert_eq!(first_three, expected);
    assert!(
        engine
            .requests()
            .iter()
            .all(|r| r["placementMode"] == "strict")
    );
    let small_root = scratch("small");
    let small = VerifierPool::new(
        small_root.path_buf(),
        minecraft(air(None)),
        settings(budgets()),
        Box::new(|| Ok(8 << 30)),
    );
    small
        .run(
            &world(),
            &json!({ "probes": [[0, 0, 0]] }),
            &settings(budgets()),
            &CancellationToken::new(),
            &mut |_| {},
            None,
        )
        .await
        .unwrap();
    assert_eq!(small.lane_roots().len(), 1);
}
