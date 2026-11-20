//! The verifier's differential harness: the same `test_build` calls and
//! grader experiments through today's Node verifier on the released engine
//! program (`tests/node/verify-node.js`) and through `mcbench_tools::verify`
//! on the app's engine children, compared by `tests/node/verify-compare.js`:
//! the result JSON a model reads and every report file, byte for byte but
//! for the run-dependent fields that script lists.
//!
//! `MCBENCH_VERIFY_DIFF=<out folder>` turns it on, with `MCBENCH_TABLES`
//! (Minecraft's tables), `MCBENCH_ENGINE_PROGRAM` (a built `mcbench-lite`,
//! the children's program) and `node` on the PATH; it is ignored without them
//! and fails when they are set and something is missing. The out folder
//! holds `cases.jsonl` and `node/`, or they are made first: the cases by
//! `verify-cases.js` from `MCBENCH_NODE_DIFF` (a checkout with
//! `node_modules` and Minecraft's tables), `MCBENCH_DIFF_DATA` (a data
//! folder clone) and `MCBENCH_CORPUS` (the community builds), the Node
//! side by `verify-node.js` with `MCBENCH_TOOLS_DIR` (the released engine).
//! Run it with `--release --nocapture`.

#[path = "../../agent/tests/common/fixture.rs"]
mod fixture;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use mcbench_engine::{EngineProgram, VerifyEngines};
use mcbench_tools::verify::{
    Budgets, Encoding, Limits, Minecraft, Placement, Settings, VerifierPool, VerifyError,
    WorkFolder, execute_test_build, failure, total_memory,
};
use mcbench_world::{World, WorldManifest};
use serde_json::{Map, Value};
use tokio_util::sync::CancellationToken;

fn folder(name: &str) -> Option<PathBuf> {
    let value = std::env::var_os(name)?;
    let folder = PathBuf::from(value);
    assert!(
        folder.is_dir(),
        "{name} names {}, which is not a folder",
        folder.display()
    );
    Some(folder)
}

fn script(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/node")
        .join(name)
}

fn node(args: &[&std::ffi::OsStr]) -> bool {
    Command::new("node")
        .arg("--max-old-space-size=8000")
        .args(args)
        .status()
        .unwrap_or_else(|e| panic!("cannot run node: {e}"))
        .success()
}

fn load(world: &Value) -> World {
    let path = |name: &str| PathBuf::from(world[name].as_str().expect("a path"));
    let (manifest, chunks) = (path("manifest"), path("chunks"));
    let text = std::fs::read_to_string(&manifest).expect("a manifest");
    World::from_manifest(&WorldManifest(text), &mut |hash: &str| {
        std::fs::read(chunks.join(format!("{hash}.json"))).map_err(|e| format!("{hash}: {e}"))
    })
    .expect("a world")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs MCBENCH_VERIFY_DIFF (an out folder), MCBENCH_TABLES, MCBENCH_ENGINE_PROGRAM and node; see the file's doc"]
async fn every_case_answers_what_node_answers() {
    let out = folder("MCBENCH_VERIFY_DIFF").expect("set MCBENCH_VERIFY_DIFF to the out folder");
    let tables = folder("MCBENCH_TABLES").expect("set MCBENCH_TABLES to Minecraft's tables");
    if !out.join("cases.jsonl").is_file() {
        let checkout = folder("MCBENCH_NODE_DIFF").expect("MCBENCH_NODE_DIFF names the checkout");
        let data =
            folder("MCBENCH_DIFF_DATA").expect("MCBENCH_DIFF_DATA names a data folder clone");
        fixture::guard(&data);
        folder("MCBENCH_CORPUS").expect("MCBENCH_CORPUS names the community builds");
        assert!(node(&[
            script("verify-cases.js").as_os_str(),
            checkout.as_os_str(),
            data.as_os_str(),
            out.as_os_str()
        ]));
    }
    if !out.join("node").is_dir() {
        let checkout = folder("MCBENCH_NODE_DIFF").expect("MCBENCH_NODE_DIFF names the checkout");
        folder("MCBENCH_TOOLS_DIR")
            .expect("MCBENCH_TOOLS_DIR names the tools with the released engine");
        assert!(node(&[
            script("verify-node.js").as_os_str(),
            checkout.as_os_str(),
            out.as_os_str()
        ]));
    }
    let program = PathBuf::from(
        std::env::var_os("MCBENCH_ENGINE_PROGRAM")
            .expect("set MCBENCH_ENGINE_PROGRAM to a built mcbench-lite (the engine children)"),
    );
    assert!(program.is_file(), "{} is not a file", program.display());
    let engine = Arc::new(VerifyEngines::new(EngineProgram::new(program, &tables)));
    let all = std::time::Instant::now();
    let scratch = out.join("rust-scratch");
    let _ = std::fs::remove_dir_all(&scratch);
    let budgets = Limits::from(Budgets::from_env().expect("budgets"));
    let defaults = Settings {
        budgets: budgets.clone(),
        placement: Placement::Updates,
        encoding: Encoding::Full,
    };
    let pool = VerifierPool::new(
        scratch.join("verifier"),
        Minecraft::new(engine.clone(), &tables).expect("Minecraft"),
        defaults.clone(),
        Box::new(total_memory),
    );
    let grader = VerifierPool::single(
        scratch.join("grader"),
        Minecraft::new(engine.clone(), &tables).expect("Minecraft"),
        defaults.clone(),
    );
    let side = out.join("rust");
    std::fs::create_dir_all(&side).unwrap();
    let cases = std::fs::read_to_string(out.join("cases.jsonl")).expect("cases.jsonl");
    let never = CancellationToken::new();
    for (n, line) in cases.lines().enumerate() {
        let case = mcbench_json::parse(line).expect("a case");
        let files = side.join(n.to_string());
        let _ = std::fs::remove_dir_all(&files);
        std::fs::create_dir_all(&files).unwrap();
        let world = load(&case["world"]);
        let started = std::time::Instant::now();
        let placement = Placement::parse(case["placementMode"].as_str().unwrap()).unwrap();
        let text = if case["kind"] == "call" {
            pool.settings.lock().unwrap().placement = placement;
            let work = scratch.join("work").join(n.to_string());
            let args = case["args"]
                .as_object()
                .expect("a call's arguments are an object");
            let result = execute_test_build(
                &pool,
                &world,
                args,
                &never,
                &mut |_| {},
                &WorkFolder(work.clone()),
            )
            .await;
            if let Some(Value::Array(tests)) = result.get("tests") {
                for (i, test) in tests.iter().enumerate() {
                    if let Some(report) = test.get("report").and_then(Value::as_str) {
                        let from = work.join(report.strip_prefix("/work/").unwrap());
                        std::fs::copy(&from, files.join(format!("{i}.json"))).unwrap();
                    }
                }
            }
            let _ = std::fs::remove_dir_all(&work);
            mcbench_json::stringify(&Value::Object(result))
        } else {
            let mut settings = defaults.clone();
            settings.placement = placement;
            settings.encoding = Encoding::parse(case["reportEncoding"].as_str().unwrap()).unwrap();
            if let Some(budgets) = case.get("budgets") {
                settings.budgets = Limits::from_json(budgets).unwrap();
            }
            let report = files.join("0.json").to_string_lossy().into_owned();
            let mut reply = Map::new();
            match grader
                .run(
                    &world,
                    &case["test"],
                    &settings,
                    &never,
                    &mut |_| {},
                    Some(report),
                )
                .await
            {
                Ok(result) => {
                    reply.insert("ok".into(), Value::Bool(true));
                    reply.insert("result".into(), Value::Object(result));
                }
                Err(error) => {
                    reply.insert("ok".into(), Value::Bool(false));
                    reply.insert("error".into(), Value::from(error.to_string()));
                    if let VerifyError::Budget { .. } = error {
                        let shown = failure(&error, false);
                        reply.insert("code".into(), shown["code"].clone());
                        reply.insert("budget".into(), shown["budget"].clone());
                    }
                }
            }
            mcbench_json::stringify(&Value::Object(reply))
        };
        std::fs::write(side.join(format!("{n}.json")), text).unwrap();
        println!(
            "{n} {} {} ms",
            case["name"].as_str().unwrap(),
            started.elapsed().as_millis()
        );
    }
    println!(
        "{} cases in {} ms on {} engine children",
        cases.lines().count(),
        all.elapsed().as_millis(),
        engine.started()
    );
    pool.stop().await;
    grader.stop().await;
    let _ = std::fs::remove_dir_all(&scratch);
    assert!(
        node(&[
            script("verify-compare.js").as_os_str(),
            out.as_os_str(),
            "rust".as_ref(),
            "--json".as_ref(),
            out.join("compare-rust.json").as_os_str()
        ]),
        "the Rust verifier answered differently from Node: see the table above"
    );
}
