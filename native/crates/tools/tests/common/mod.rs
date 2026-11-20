//! What the verifier's tests share: today's `test/helpers/fake-facts.js`
//! stand-ins (the import's last pass, the lectern books), a fake engine that
//! answers each experiment as `McbenchExperiment` writes its files (as
//! `test/engine-verify.test.js`'s `fakeEngine`), worlds and pools.

#![allow(dead_code)]

use std::collections::HashSet;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use mcbench_engine::{BlockState, EngineError, VerifyReport, VerifyRequest};
use mcbench_tools::verify::{
    Budgets, Encoding, ItemTags, Limits, Minecraft, Placement, Settings, VerifierPool, VerifyEngine,
};
use mcbench_world::facts::LastPass;
use mcbench_world::{Palette, World};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

/// `fake-facts.js` `lastPass`: the supported blocks are of the last pass.
pub struct FakeFacts;

impl LastPass for FakeFacts {
    fn last_pass(&self, palette: &Palette) -> Result<Vec<u8>, String> {
        const SUPPORTED: [&str; 7] = [
            "minecraft:torch",
            "minecraft:wall_torch",
            "minecraft:redstone_wire",
            "minecraft:sand",
            "minecraft:water",
            "minecraft:lever",
            "minecraft:rail",
        ];
        Ok((0..palette.len())
            .map(|i| {
                u8::from(
                    palette
                        .get(i)
                        .is_some_and(|e| SUPPORTED.contains(&e.id.as_str())),
                )
            })
            .collect())
    }
}

/// `fake-facts.js` `itemTags`.
pub fn tags() -> ItemTags {
    ItemTags {
        lectern_books: HashSet::from([
            "minecraft:written_book".to_string(),
            "minecraft:writable_book".to_string(),
        ]),
    }
}

/// A world of one stone block at `at`.
pub fn world_at(at: [i32; 3]) -> World {
    let mut world = World::new();
    world
        .set(at, BlockState::new("minecraft:stone", &[]))
        .unwrap();
    world.finalize();
    world
}

/// `test/minecraft-verify.test.js`'s world: stone at -1, 0, 0.
pub fn world() -> World {
    world_at([-1, 0, 0])
}

/// `BUDGETS` as the tests read it (no variable set).
pub fn budgets() -> Limits {
    Limits::from(Budgets::DEFAULT)
}

/// `budgets` with `resource` set to `value`.
pub fn with(resource: &str, value: f64) -> Limits {
    let mut limits = budgets();
    for (name, v) in &mut limits.0 {
        if name == resource {
            *v = value;
        }
    }
    limits
}

/// The app's settings with `budgets`.
pub fn settings(budgets: Limits) -> Settings {
    Settings {
        budgets,
        placement: Placement::Updates,
        encoding: Encoding::Full,
    }
}

/// The answer of a fake engine to one experiment.
pub type Answer = Pin<Box<dyn Future<Output = Result<VerifyReport, EngineError>> + Send>>;

/// The progress texts a fake engine gives an experiment first.
pub type Progress = Box<dyn Fn(&Value) -> Vec<String> + Send + Sync>;

/// An engine that answers each experiment with `answer` (its request read
/// back as JSON), after giving `progress`' texts.
pub struct FakeEngine {
    pub answer: Box<dyn Fn(Value) -> Answer + Send + Sync>,
    pub progress: Progress,
    pub verified: Mutex<Vec<(Value, String)>>,
    pub finished: AtomicUsize,
    /// How many times its engine was stopped (a cancel or the verifier's stop).
    pub stopped: AtomicUsize,
    /// The ages (ms) its sweeps of leftover folders were asked for.
    pub swept: Mutex<Vec<f64>>,
    /// Told at each stop ([`FakeEngine::until_stopped`]).
    pub stops: tokio::sync::Notify,
}

impl FakeEngine {
    pub fn new(answer: impl Fn(Value) -> Answer + Send + Sync + 'static) -> Arc<FakeEngine> {
        Arc::new(FakeEngine {
            answer: Box::new(answer),
            progress: Box::new(|_| Vec::new()),
            verified: Mutex::new(Vec::new()),
            finished: AtomicUsize::new(0),
            stopped: AtomicUsize::new(0),
            swept: Mutex::new(Vec::new()),
            stops: tokio::sync::Notify::new(),
        })
    }

    pub fn with_progress(
        answer: impl Fn(Value) -> Answer + Send + Sync + 'static,
        progress: impl Fn(&Value) -> Vec<String> + Send + Sync + 'static,
    ) -> Arc<FakeEngine> {
        Arc::new(FakeEngine {
            answer: Box::new(answer),
            progress: Box::new(progress),
            verified: Mutex::new(Vec::new()),
            finished: AtomicUsize::new(0),
            stopped: AtomicUsize::new(0),
            swept: Mutex::new(Vec::new()),
            stops: tokio::sync::Notify::new(),
        })
    }

    /// Waits until the engine was stopped `times` times. The verifier
    /// answers an experiment that is cancelled or past its wall budget at
    /// once and stops its engine right after, on the experiment's own task
    /// (`Run::execute`), so a test that has the answer may be a moment ahead
    /// of the stop.
    pub async fn until_stopped(&self, times: usize) {
        let wait = async {
            loop {
                let told = self.stops.notified();
                tokio::pin!(told);
                told.as_mut().enable();
                if self.stopped.load(Ordering::SeqCst) >= times {
                    return;
                }
                told.await;
            }
        };
        tokio::time::timeout(std::time::Duration::from_secs(30), wait)
            .await
            .expect("the engine is stopped within 30 seconds of the answer");
    }

    pub fn requests(&self) -> Vec<Value> {
        self.verified
            .lock()
            .unwrap()
            .iter()
            .map(|(r, _)| r.clone())
            .collect()
    }
}

#[async_trait]
impl VerifyEngine for FakeEngine {
    async fn verify(
        &self,
        request: &VerifyRequest,
        progress: &mut (dyn for<'t> FnMut(&'t str) + Send),
        cancel: &CancellationToken,
    ) -> Result<VerifyReport, EngineError> {
        let parsed = mcbench_json::parse(&request.request).unwrap();
        self.verified
            .lock()
            .unwrap()
            .push((parsed.clone(), request.commands.0.clone()));
        for text in (self.progress)(&parsed) {
            progress(&text);
        }
        // A cancel stops the engine, and the experiment fails with its exit
        // (as today's fake engine waits for its stop and throws).
        let answer = tokio::select! {
            answer = (self.answer)(parsed) => answer,
            _ = cancel.cancelled() => {
                self.stopped.fetch_add(1, Ordering::SeqCst);
                self.stops.notify_waiters();
                Err(EngineError::Stopped("The engine stopped (SIGTERM): ".into()))
            }
        };
        self.finished.fetch_add(1, Ordering::SeqCst);
        answer
    }

    fn warm(&self, _engines: usize) -> Result<(), String> {
        Ok(())
    }

    async fn stop(&self) {
        self.stopped.fetch_add(1, Ordering::SeqCst);
        self.stops.notify_waiters();
    }

    fn sweep(&self, age_ms: f64) {
        self.swept.lock().unwrap().push(age_ms);
    }

    async fn fixed_palette(
        &self,
        entries: &[BlockState],
        _data_version: i64,
    ) -> Result<Vec<BlockState>, String> {
        Ok(entries.to_vec())
    }
}

/// The samples of a request (its `samples`, or its `sampleSequence`).
pub fn samples(request: &Value) -> Vec<i64> {
    match request.get("sampleSequence") {
        Some(s) => {
            let (start, step, count) = (
                s["start"].as_i64().unwrap(),
                s["step"].as_i64().unwrap(),
                s["count"].as_i64().unwrap(),
            );
            (0..count).map(|i| start + i * step).collect()
        }
        None => request["samples"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t.as_i64().unwrap())
            .collect(),
    }
}

/// The files a completed experiment leaves (`complete(request, folder)`),
/// with `report` as report.json and `result` changed by `change`.
pub fn complete_with(
    request: &Value,
    report: &str,
    change: impl FnOnce(&mut serde_json::Map<String, Value>),
) -> VerifyReport {
    let count = samples(request).len() as i64;
    let probes = request["probes"].as_array().unwrap().len() as i64;
    let mut result = json!({
        "ok": true, "elapsedTicks": request["duration"], "sampleCount": count, "probeCount": probes,
        "inputCount": request["actions"].as_array().unwrap().len(), "observationCount": probes * (count + 1),
        "reportBytes": report.len(),
    })
    .as_object()
    .unwrap()
    .clone();
    change(&mut result);
    VerifyReport {
        result: mcbench_json::stringify(&Value::Object(result)),
        files: vec![("report.json".into(), report.as_bytes().to_vec())],
    }
}

/// `complete(request, folder)` with an empty report.
pub fn complete(request: &Value) -> VerifyReport {
    complete_with(request, r#"{"initial":{},"observations":[]}"#, |_| {})
}

/// A report as the engine writes one: every probe air in each sample.
pub fn air_report(request: &Value) -> String {
    let blocks: Vec<Value> = request["probes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|_| json!({ "block": "minecraft:air", "signal": 0 }))
        .collect();
    let observations: Vec<Value> = samples(request)
        .iter()
        .map(|tick| json!({ "tick": tick, "blocks": blocks }))
        .collect();
    let inputs: Vec<Value> = request["actions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| {
            let mut a = a.as_object().unwrap().clone();
            a.insert("applied".into(), Value::Bool(false));
            a.insert("error".into(), Value::from("Missing input"));
            Value::Object(a)
        })
        .collect();
    mcbench_json::stringify(
        &json!({ "ok": true, "engine": "fixture", "elapsedTicks": request["duration"], "initial": { "tick": 0, "blocks": blocks }, "observations": observations, "inputs": inputs }),
    )
}

/// A scratch folder that is removed when it is dropped, also when the test
/// that made it fails (a panic unwinds through its owner).
#[derive(Debug)]
pub struct Scratch(PathBuf);

impl Scratch {
    /// The folder's path, owned.
    pub fn path_buf(&self) -> PathBuf {
        self.0.clone()
    }
}

impl std::ops::Deref for Scratch {
    type Target = std::path::Path;
    fn deref(&self) -> &std::path::Path {
        &self.0
    }
}

impl AsRef<std::path::Path> for Scratch {
    fn as_ref(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A scratch folder of its own, removed when dropped.
pub fn scratch(name: &str) -> Scratch {
    static N: AtomicUsize = AtomicUsize::new(0);
    let folder = std::env::temp_dir().join(format!(
        "mcbench-tools-verify-{name}-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(&folder).unwrap();
    Scratch(folder)
}

/// A verifier of one lane on `engine`.
pub fn verifier(engine: Arc<FakeEngine>, budgets: Limits) -> (VerifierPool, Scratch) {
    let root = scratch("root");
    let pool = VerifierPool::single(root.path_buf(), minecraft(engine), settings(budgets));
    (pool, root)
}

/// The verifier's Minecraft with `engine` and the stand-ins.
pub fn minecraft(engine: Arc<FakeEngine>) -> Minecraft {
    Minecraft {
        engine,
        facts: Arc::new(FakeFacts),
        tags: Arc::new(tags()),
        y_offset: 64,
    }
}

/// The engine children's program for the tables in `tables`:
/// `MCBENCH_ENGINE_PROGRAM` names a built `mcbench-lite`.
pub fn engine_program(tables: &std::path::Path) -> mcbench_engine::EngineProgram {
    let program = PathBuf::from(
        std::env::var_os("MCBENCH_ENGINE_PROGRAM")
            .expect("set MCBENCH_ENGINE_PROGRAM to a built mcbench-lite (the engine children)"),
    );
    assert!(
        program.is_file(),
        "MCBENCH_ENGINE_PROGRAM names {}, which is not a file",
        program.display()
    );
    mcbench_engine::EngineProgram::new(program, tables)
}

/// Writes `text` as the program `path`, executable, through a shell of its
/// own: this process never holds the file open for writing, so another
/// test's fork (which keeps this process's descriptors until it execs) can
/// never leave the program busy (`ETXTBSY`) when the app starts it.
#[cfg(unix)]
pub fn write_program(path: &std::path::Path, text: &str) {
    use std::io::Write as _;
    let mut shell = std::process::Command::new("/bin/sh")
        .args(["-c", "cat > \"$1\" && chmod 755 \"$1\"", "sh"])
        .arg(path)
        .stdin(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    shell
        .stdin
        .take()
        .unwrap()
        .write_all(text.as_bytes())
        .unwrap();
    assert!(
        shell.wait().unwrap().success(),
        "{} is written",
        path.display()
    );
}

/// Whether a process with this id runs (`kill -0`).
#[cfg(unix)]
pub fn alive(pid: u32) -> bool {
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap()
        .success()
}

/// Whether a process with this id runs: it opens and has no exit code yet.
#[cfg(windows)]
pub fn alive(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    // SAFETY: OpenProcess takes plain values and answers a handle or null;
    // the handle is read once and closed here.
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return false;
        }
        let mut code = 0u32;
        let read = GetExitCodeProcess(process, &mut code);
        CloseHandle(process);
        read != 0 && code == STILL_ACTIVE as u32
    }
}

/// A test experiment's arguments.
pub fn args(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}
