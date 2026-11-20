//! `test/agent-verification.test.js`, the cases whose subject is
//! `executeTestBuild`: a call's checks, its tests side by side and in order,
//! failures and budgets as a model reads them, cancellation, the reports in
//! `/work/verification/` and the inline limit. Today's verifier fakes are
//! here a fake engine whose report the test chooses (the result is the
//! engine verifier's shape around it). Not here: a dropped completion's
//! retry and the turn loop (the agent loop's, `mcbench-agent`), and the
//! build tool's revisions (`build`'s).

mod common;

use std::sync::Arc;
use std::time::Duration;

use common::*;
use mcbench_engine::EngineError;
use mcbench_tools::verify::{VerifierPool, WorkFolder, execute_test_build};
use serde_json::{Map, Value, json};
use tokio_util::sync::CancellationToken;

/// An engine whose report is `report(request)`.
fn reporting(report: impl Fn(&Value) -> Value + Send + Sync + 'static) -> Arc<FakeEngine> {
    let report = Arc::new(report);
    FakeEngine::new(move |request| {
        let report = report.clone();
        Box::pin(async move {
            let text = mcbench_json::stringify(&report(&request));
            Ok(complete_with(&request, &text, |_| {}))
        })
    })
}

async fn call(pool: &VerifierPool, args: Value, work: &WorkFolder) -> Map<String, Value> {
    execute_test_build(
        pool,
        &world_at([0, 0, 0]),
        args.as_object().unwrap(),
        &CancellationToken::new(),
        &mut |_| {},
        work,
    )
    .await
}

fn read(work: &WorkFolder, report: &Value) -> Value {
    let name = report.as_str().unwrap().strip_prefix("/work/").unwrap();
    serde_json::from_slice(&std::fs::read(work.0.join(name)).unwrap()).unwrap()
}

#[tokio::test]
async fn all_model_types_get_native_verification_and_large_observations_remain_available_in_the_workspace()
 {
    let probes: Vec<Value> = (0..200).map(|x| json!([x, 1, 0])).collect();
    let blocks: Vec<Value> = probes
        .iter()
        .map(|_| json!({ "block": "minecraft:redstone_lamp[lit=false]", "signal": 0 }))
        .collect();
    let report = json!({ "ok": true, "engine": "Paper fixture", "probes": probes, "initial": { "tick": 0, "blocks": blocks }, "observations": [{ "tick": 20, "blocks": blocks }], "inputs": [] });
    let expected = report.clone();
    let (pool, _root) = verifier(reporting(move |_| report.clone()), budgets());
    let work_dir = scratch("work");
    let work = WorkFolder(work_dir.path_buf());
    let result = call(
        &pool,
        json!({ "tests": [{ "probes": [[0, 0, 0]] }] }),
        &work,
    )
    .await;
    assert_eq!(result["ok"], json!(true));
    let test = &result["tests"][0];
    assert_eq!(test["observationsInFile"], json!(true));
    assert!(test.get("observations").is_none());
    assert_eq!(read(&work, &test["report"]), expected);
    assert!(!result.contains_key("passed"));
    assert!(
        test["report"]
            .as_str()
            .unwrap()
            .starts_with("/work/verification/")
    );
}

#[tokio::test]
async fn engine_errors_are_delivered_to_the_model_as_verification_failure() {
    let (pool, _root) = verifier(
        FakeEngine::new(|_| {
            Box::pin(async move { Err(EngineError::Stopped("Docker unavailable".into())) })
        }),
        budgets(),
    );
    let result = call(
        &pool,
        json!({ "tests": [{ "probes": [[0, 0, 0]] }] }),
        &WorkFolder(scratch("work").path_buf()),
    )
    .await;
    assert_eq!(result["ok"], json!(false));
    assert!(
        result["error"]
            .as_str()
            .unwrap()
            .starts_with("test 1: Docker unavailable")
    );
    assert_eq!(result["tests"][0]["ok"], json!(false));
    assert!(result["tests"][0].get("report").is_none());
    assert_eq!(
        Value::Object(result),
        json!({ "ok": false, "error": "test 1: Docker unavailable", "tests": [{ "ok": false, "error": "Docker unavailable", "note": "No completed Minecraft verification is available from this call." }] })
    );
}

#[tokio::test]
async fn the_model_receives_actionable_budget_failures_without_a_design_failure_verdict() {
    let (pool, _root) = verifier(reporting(|_| json!({})), with("operations", 1000.0));
    pool.settings.lock().unwrap().budgets = with("operations", 1000.0);
    let probes = vec![json!([0, 0, 0]); 1001];
    let result = call(
        &pool,
        json!({ "tests": [{ "probes": probes, "samples": [0] }] }),
        &WorkFolder(scratch("work").path_buf()),
    )
    .await;
    // 1001 probes x 2 observations: past 1000 operations.
    let test = &result["tests"][0];
    assert_eq!(test["code"], "VERIFICATION_BUDGET_EXCEEDED");
    assert_eq!(test["budget"]["setting"], "MCBENCH_VERIFY_MAX_OPERATIONS");
    assert!(test.get("report").is_none());
    assert!(!result.contains_key("passed"));
    let (pool, _root) = verifier(
        FakeEngine::new(|_| panic!("no test starts when the call has too many")),
        budgets(),
    );
    let tests = vec![json!({}); 9];
    let too_many = call(
        &pool,
        json!({ "tests": tests }),
        &WorkFolder(scratch("work").path_buf()),
    )
    .await;
    assert_eq!(too_many["code"], "VERIFICATION_BUDGET_EXCEEDED");
    assert_eq!(
        too_many["budget"],
        json!({ "resource": "tests", "requested": 9, "allowed": 8, "setting": "MCBENCH_VERIFY_MAX_TESTS" })
    );
    assert_eq!(
        mcbench_json::stringify(&Value::Object(too_many)),
        r#"{"ok":false,"code":"VERIFICATION_BUDGET_EXCEEDED","budget":{"resource":"tests","requested":9,"allowed":8,"setting":"MCBENCH_VERIFY_MAX_TESTS"},"error":"Minecraft test needs 9 tests; configured budget is 8 (MCBENCH_VERIFY_MAX_TESTS)","note":"No completed Minecraft verification is available from this call."}"#
    );
}

#[tokio::test]
async fn output_is_bounded_in_bytes_even_for_few_large_states_or_a_large_probe_list_and_the_full_report_is_readable()
 {
    for (count, block) in [
        (
            64usize,
            format!("minecraft:fixture[state={}]", "長".repeat(100)),
        ),
        (2048, "minecraft:air".to_string()),
    ] {
        let probes: Vec<Value> = (0..count)
            .map(|i| json!([4096, 895, 4096 - i as i64]))
            .collect();
        let blocks: Vec<Value> = probes
            .iter()
            .map(|_| json!({ "block": block, "signal": 0 }))
            .collect();
        let report = json!({ "ok": true, "probes": probes, "initial": { "tick": 0, "blocks": blocks }, "observations": [{ "tick": 0, "blocks": blocks }], "inputs": [] });
        let expected = report.clone();
        let (pool, _root) = verifier(reporting(move |_| report.clone()), budgets());
        let work_dir = scratch("work");
        let work = WorkFolder(work_dir.path_buf());
        let result = call(&pool, json!({ "tests": [{ "probes": probes }] }), &work).await;
        assert!(mcbench_json::stringify(&Value::Object(result.clone())).len() <= 20_000);
        let test = &result["tests"][0];
        assert_eq!(test["probeCount"], json!(count));
        assert_eq!(test["observationsInFile"], json!(true));
        if count == 2048 {
            assert_eq!(test["probesInFile"], json!(true));
        }
        assert_eq!(read(&work, &test["report"]), expected);
    }
}

#[tokio::test]
async fn cancelled_tests_return_a_distinct_persisted_outcome() {
    let (pool, _root) = verifier(reporting(|_| json!({})), budgets());
    let cancel = CancellationToken::new();
    cancel.cancel();
    let result = execute_test_build(
        &pool,
        &world_at([0, 0, 0]),
        json!({ "tests": [{ "probes": [[0, 0, 0]] }] })
            .as_object()
            .unwrap(),
        &cancel,
        &mut |_| {},
        &WorkFolder(scratch("work").path_buf()),
    )
    .await;
    assert_eq!(
        Value::Object(result),
        json!({ "ok": false, "cancelled": true, "error": "test 1: This operation was aborted", "tests": [{ "ok": false, "cancelled": true, "error": "This operation was aborted", "note": "No completed Minecraft verification is available from this call." }] })
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_tests_of_a_call_run_side_by_side_each_on_the_same_blueprint_and_come_back_in_order_and_a_failing_test_does_not_stop_the_others()
 {
    let started = Arc::new(tokio::sync::Barrier::new(3));
    let barrier = started.clone();
    let engine = FakeEngine::new(move |request| {
        let barrier = barrier.clone();
        Box::pin(async move {
            // A test that waits for the others to start proves they run at once.
            barrier.wait().await;
            if request["probes"][0] == json!([0, 0, 0]) {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            let report = mcbench_json::stringify(
                &json!({ "ok": true, "engine": "fixture", "probes": request["probes"], "inputs": [], "initial": { "tick": 0, "blocks": [] }, "observations": [] }),
            );
            Ok(complete_with(&request, &report, |_| {}))
        })
    });
    let lanes_root = scratch("lanes");
    let pool = VerifierPool::new(
        lanes_root.path_buf(),
        minecraft(engine),
        settings(budgets()),
        Box::new(|| Ok(16 << 30)),
    );
    let work_dir = scratch("work");
    let work = WorkFolder(work_dir.path_buf());
    let mut updates = Vec::new();
    let far: Vec<Value> = (0..2000)
        .map(|i| json!([(i % 45) * 48 - 2048, 0, (i / 45) * 48 - 2048]))
        .collect();
    let tests = json!([{ "probes": [[0, 0, 0]] }, { "probes": far }, { "probes": [[2, 0, 0]] }, { "probes": [[1, 0, 0]] }]);
    let result = execute_test_build(
        &pool,
        &world_at([0, 0, 0]),
        json!({ "tests": tests }).as_object().unwrap(),
        &CancellationToken::new(),
        &mut |u| updates.push(u["message"].as_str().unwrap().to_string()),
        &work,
    )
    .await;
    assert_eq!(result["ok"], json!(false));
    let oks: Vec<&Value> = result["tests"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| &t["ok"])
        .collect();
    assert_eq!(
        oks,
        [&json!(true), &json!(false), &json!(true), &json!(true)]
    );
    assert_eq!(result["tests"][1]["code"], "VERIFICATION_BUDGET_EXCEEDED");
    assert!(
        result["error"]
            .as_str()
            .unwrap()
            .starts_with("test 2: Minecraft test needs "),
        "{}",
        result["error"]
    );
    let reports: std::collections::HashSet<&str> = result["tests"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|t| t["report"].as_str())
        .collect();
    assert_eq!(reports.len(), 3, "each test keeps a report file of its own");
    for (i, test) in result["tests"].as_array().unwrap().iter().enumerate() {
        if test["ok"] == json!(true) {
            assert_eq!(read(&work, &test["report"])["probes"], tests[i]["probes"]);
        }
    }
    let last = updates.last().unwrap();
    assert!(
        last.starts_with("Test 1: ")
            && last.contains("; Test 2: ")
            && last.contains("; Test 3: ")
            && last.contains("; Test 4: "),
        "{last}"
    );
}

#[tokio::test]
async fn a_call_of_many_large_tests_stays_within_the_inline_budget_and_keeps_every_report_in_a_file()
 {
    let probes: Vec<Value> = (0..200).map(|x| json!([x, 1, 0])).collect();
    let blocks: Vec<Value> = probes
        .iter()
        .map(|_| json!({ "block": "minecraft:redstone_lamp[lit=false]", "signal": 0 }))
        .collect();
    let report = json!({ "ok": true, "engine": "fixture", "probes": probes, "initial": { "tick": 0, "blocks": blocks }, "observations": [{ "tick": 20, "blocks": blocks }], "inputs": [], "preview": { "initial": blocks[..50] } });
    let (pool, _root) = verifier(reporting(move |_| report.clone()), budgets());
    let work_dir = scratch("work");
    let work = WorkFolder(work_dir.path_buf());
    let result = call(
        &pool,
        json!({ "tests": vec![json!({ "probes": [[0, 0, 0]] }); 8] }),
        &work,
    )
    .await;
    assert_eq!(result["ok"], json!(true));
    assert_eq!(result["tests"].as_array().unwrap().len(), 8);
    assert!(mcbench_json::stringify(&Value::Object(result.clone())).len() <= 20_000);
    for test in result["tests"].as_array().unwrap() {
        assert_eq!(
            read(&work, &test["report"])["observations"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }
}

#[tokio::test]
async fn a_call_must_hold_a_list_of_tests() {
    let (pool, _root) = verifier(FakeEngine::new(|_| panic!("nothing runs")), budgets());
    for args in [
        json!({}),
        json!({ "probes": [[0, 0, 0]] }),
        json!({ "tests": [] }),
        json!({ "tests": {} }),
        json!({ "tests": [{}], "probes": [[0, 0, 0]] }),
    ] {
        let result = call(&pool, args, &WorkFolder(scratch("work").path_buf())).await;
        assert_eq!(result["ok"], json!(false));
        assert_eq!(
            result["error"],
            "test_build requires {tests: [one experiment, ...]}"
        );
    }
}
