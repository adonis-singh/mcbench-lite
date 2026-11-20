//! `test/verifier-capabilities.test.js`, the two cases that are the
//! verifier's: saved tick queues are refused before queuing outside strict
//! placement, and forwarded unchanged in it. The other three are the
//! `test_build` schema's and guidance's, which are `mcbench-agent`'s
//! contract (generated from `verification-contract.js`).

mod common;

use common::*;
use mcbench_tools::verify::{Placement, Settings};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn unsupported_saved_queues_fail_before_queuing_or_starting_minecraft_including_empty_queues()
{
    let engine = FakeEngine::new(|request| Box::pin(async move { Ok(complete(&request)) }));
    let (pool, _root) = verifier(engine.clone(), budgets());
    let stone = world_at([0, 0, 0]);
    let mut progress: Vec<Value> = Vec::new();
    for entries in [
        json!([]),
        json!([{ "kind": "block", "id": "minecraft:stone", "at": [0, 0, 0], "delay": 1, "priority": 0, "sub_tick": "1" }]),
    ] {
        let request = json!({ "probes": [[0, 0, 0]], "scheduled_ticks": { "bounds": [[0, 0, 0], [1, 1, 1]], "data_version": 4189, "entries": entries } });
        let before = request.clone();
        let refused = pool
            .run(
                &stone,
                &request,
                &settings(budgets()),
                &CancellationToken::new(),
                &mut |u| progress.push(Value::Object(u)),
                None,
            )
            .await
            .unwrap_err()
            .to_string();
        assert!(
            refused.contains("Omit scheduled_ticks") && refused.contains("not started"),
            "{refused}"
        );
        assert_eq!(
            request, before,
            "reject incompatible submissions without rewriting them"
        );
    }
    assert!(engine.verified.lock().unwrap().is_empty());
    assert!(progress.is_empty());
    pool.run(
        &stone,
        &json!({ "probes": [[0, 0, 0]] }),
        &settings(budgets()),
        &CancellationToken::new(),
        &mut |_| {},
        None,
    )
    .await
    .unwrap();
    assert_eq!(
        engine.verified.lock().unwrap().len(),
        1,
        "a corrected request still runs in the original mode"
    );
    assert_eq!(engine.requests()[0]["placementMode"], "updates");
}

#[tokio::test]
async fn strict_placement_still_forwards_authoritative_empty_and_nonempty_queues_unchanged() {
    let engine = FakeEngine::new(|request| Box::pin(async move { Ok(complete(&request)) }));
    let (pool, _root) = verifier(engine.clone(), budgets());
    let strict = Settings {
        placement: Placement::Strict,
        ..settings(budgets())
    };
    let stone = world_at([0, 0, 0]);
    for entries in [
        json!([]),
        json!([{ "kind": "block", "id": "minecraft:stone", "at": [0, 0, 0], "delay": 7, "priority": -1, "sub_tick": "9007199254740993" }]),
    ] {
        let saved =
            json!({ "bounds": [[0, 0, 0], [1, 1, 1]], "data_version": 4189, "entries": entries });
        pool.run(
            &stone,
            &json!({ "probes": [[0, 0, 0]], "scheduled_ticks": saved }),
            &strict,
            &CancellationToken::new(),
            &mut |_| {},
            None,
        )
        .await
        .unwrap();
        let request = engine.requests().pop().unwrap();
        assert_eq!(request["placementMode"], "strict");
        assert_eq!(request["scheduled_ticks"], saved);
    }
}
