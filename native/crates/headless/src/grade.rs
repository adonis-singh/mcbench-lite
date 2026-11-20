//! `mcbench-lite headless grade <tables folder> <verifier root>`:
//! `test_build`'s verifier for the benchmark's graders (`PROTOCOL.md`,
//! family `grade`). It replaces the benchmark's Paper adapter's
//! `createVerifier` (an `EngineVerifier` of `server/minecraft-verify.js`)
//! and its `verifier.run(world, request, { reportPath })`: one request
//! line is one experiment, its reply is what `run` resolved with, and the
//! report file is written where `run` wrote it.
//!
//! The experiments run in engine children of this binary
//! (`mcbench_engine::VerifyEngines`), as `createVerifier`'s ran in its
//! engine process.
//!
//! Like `createVerifier`'s verifier, the process has one lane: its
//! experiments run one at a time, in the order they arrive. The budgets
//! are this process's `MCBENCH_VERIFY_MAX_*` unless a request gives its
//! own (a grader that sets `verifier.budgets`).

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use mcbench_engine::{EngineProgram, VerifyEngines};
use mcbench_tools::verify::{
    Budgets, Encoding, Limits, Minecraft, Placement, Settings, VerifierPool, VerifyError, failure,
};
use mcbench_world::{World, WorldManifest};
use serde_json::{Map, Value};
use tokio_util::sync::CancellationToken;

/// The fields a request may hold.
const FIELDS: [&str; 7] = [
    "id",
    "world",
    "test",
    "placementMode",
    "reportEncoding",
    "budgets",
    "reportPath",
];

/// One request, read.
struct Grade {
    id: Value,
    world: World,
    test: Value,
    settings: Settings,
    report_path: Option<String>,
}

/// Ends the process: the stream can no longer be trusted.
fn refuse(message: &str) -> ! {
    eprintln!("mcbench-lite headless grade: {message}");
    std::process::exit(2)
}

/// Serves `grade` on stdin and stdout until stdin ends and every
/// experiment has answered, then exits 0.
pub(crate) fn run(arguments: &[String]) -> ! {
    let [tables, root] = arguments else {
        refuse("usage: mcbench-lite headless grade <tables folder> <verifier root>")
    };
    let budgets = match Budgets::from_env() {
        Ok(budgets) => Limits::from(budgets),
        Err(message) => {
            eprintln!("mcbench-lite headless grade: {message}");
            std::process::exit(1)
        }
    };
    // The experiments run in engine children of this binary.
    let binary = std::env::current_exe().unwrap_or_else(|error| {
        eprintln!("mcbench-lite headless grade: the program's own path: {error}");
        std::process::exit(1)
    });
    let engine = VerifyEngines::new(EngineProgram::new(binary, tables));
    let minecraft = Minecraft::new(Arc::new(engine), Path::new(tables)).unwrap_or_else(|message| {
        eprintln!("mcbench-lite headless grade: {message}");
        std::process::exit(1)
    });
    let defaults = Settings {
        budgets,
        placement: Placement::Updates,
        encoding: Encoding::Full,
    };
    let pool = Arc::new(VerifierPool::single(
        PathBuf::from(root),
        minecraft,
        defaults.clone(),
    ));
    // Time for the budgets, I/O for the engine children's pipes.
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a runtime");
    let out = Arc::new(Mutex::new(std::io::stdout()));
    let (lines, mut incoming) = tokio::sync::mpsc::unbounded_channel::<String>();
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines() {
            let line = line.unwrap_or_else(|error| refuse(&format!("stdin: {error}")));
            if lines.send(line).is_err() {
                return;
            }
        }
    });
    runtime.block_on(async move {
        let mut running = tokio::task::JoinSet::new();
        while let Some(line) = incoming.recv().await {
            let grade = read(&line, &defaults);
            let pool = pool.clone();
            let out = out.clone();
            running.spawn(async move {
                let reply = answer(&pool, grade).await;
                let mut out = out.lock().unwrap();
                writeln!(out, "{reply}").expect("stdout");
                out.flush().expect("stdout");
            });
        }
        while running.join_next().await.is_some() {}
        // The engine children exit before the process does.
        pool.stop().await;
    });
    std::process::exit(0)
}

/// A request line, or the end of the process.
fn read(line: &str, defaults: &Settings) -> Grade {
    let value =
        mcbench_json::parse(line).unwrap_or_else(|error| refuse(&format!("not JSON: {error}")));
    let Value::Object(request) = value else {
        refuse("a request is a JSON object")
    };
    for key in request.keys() {
        if !FIELDS.contains(&key.as_str()) {
            refuse(&format!("a grade request takes no field {key}"));
        }
    }
    let field = |name: &str| {
        request
            .get(name)
            .unwrap_or_else(|| refuse(&format!("a grade request needs {name}")))
    };
    let id = field("id").clone();
    if !(id.is_number() || id.is_string()) {
        refuse("id is a number or a string");
    }
    let world = field("world");
    let text = |name: &str| {
        world
            .get(name)
            .and_then(Value::as_str)
            .unwrap_or_else(|| refuse(&format!("world.{name} is a path")))
    };
    let (manifest, chunks) = (text("manifest"), PathBuf::from(text("chunks")));
    let manifest = std::fs::read_to_string(manifest)
        .unwrap_or_else(|error| refuse(&format!("{manifest}: {error}")));
    let world = World::from_manifest(&WorldManifest(manifest), &mut |hash: &str| {
        std::fs::read(chunks.join(format!("{hash}.json"))).map_err(|e| format!("{hash}: {e}"))
    })
    .unwrap_or_else(|error| refuse(&format!("world: {error}")));
    let mut settings = defaults.clone();
    if let Some(mode) = request.get("placementMode") {
        settings.placement = mode
            .as_str()
            .ok_or_else(|| "placementMode is a string".to_string())
            .and_then(Placement::parse)
            .unwrap_or_else(|error| refuse(&error));
    }
    if let Some(encoding) = request.get("reportEncoding") {
        settings.encoding = encoding
            .as_str()
            .ok_or_else(|| "reportEncoding is a string".to_string())
            .and_then(Encoding::parse)
            .unwrap_or_else(|error| refuse(&error));
    }
    if let Some(budgets) = request.get("budgets") {
        settings.budgets = Limits::from_json(budgets).unwrap_or_else(|error| refuse(&error));
    }
    let report_path = request.get("reportPath").map(|path| {
        path.as_str()
            .unwrap_or_else(|| refuse("reportPath is a path"))
            .to_string()
    });
    Grade {
        id,
        world,
        test: field("test").clone(),
        settings,
        report_path,
    }
}

/// The reply line of one experiment.
async fn answer(pool: &VerifierPool, grade: Grade) -> String {
    let never = CancellationToken::new();
    let result = pool
        .run(
            &grade.world,
            &grade.test,
            &grade.settings,
            &never,
            &mut |_| {},
            grade.report_path,
        )
        .await;
    let mut reply = Map::new();
    reply.insert("id".into(), grade.id);
    match result {
        Ok(result) => {
            reply.insert("ok".into(), Value::Bool(true));
            reply.insert("result".into(), Value::Object(result));
        }
        Err(error) => {
            let shown = failure(&error, false);
            reply.insert("ok".into(), Value::Bool(false));
            reply.insert("error".into(), Value::from(error.to_string()));
            if let VerifyError::Budget { .. } = error {
                reply.insert("code".into(), shown["code"].clone());
                reply.insert("budget".into(), shown["budget"].clone());
            }
        }
    }
    mcbench_json::stringify(&Value::Object(reply))
}
