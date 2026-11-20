//! The golden cases, the runner and the result form the tests share.

use std::collections::HashMap;
use std::future::Future;
use std::io::Read;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, Mutex, OnceLock};

use async_trait::async_trait;
use mcbench_build::{BuildFailure, BuildRequest, EncodedHost, EncodedOutcome, V8Runner};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;

/// "At once" for a run that fails or is killed where it would otherwise
/// run on (a 16 GB fill, a process sleeping a minute): a bound a starved
/// machine meets.
pub const AT_ONCE: std::time::Duration = std::time::Duration::from_secs(10);

/// One test: a name and its body.
pub struct Test {
    pub name: String,
    pub run: Box<dyn Fn() -> Pin<Box<dyn Future<Output = ()>>>>,
}

/// A test named `name` running `body`.
pub fn test<F, Fut>(name: &str, body: F) -> Test
where
    F: Fn() -> Fut + 'static,
    Fut: Future<Output = ()> + 'static,
{
    Test {
        name: name.to_string(),
        run: Box::new(move || Box::pin(body())),
    }
}

pub fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// The child: the `mcbench-build-child` that `MCBENCH_BUILD_CHILD` names.
pub fn program() -> PathBuf {
    std::fs::canonicalize(mcbench_build::child_program().unwrap()).unwrap()
}

/// This test binary, the probe and the forged children (see main.rs).
pub fn test_binary() -> PathBuf {
    std::fs::canonicalize(std::env::current_exe().unwrap()).unwrap()
}

/// The runner every test shares: the child under the platform sandbox.
pub fn runner() -> &'static V8Runner {
    static RUNNER: OnceLock<V8Runner> = OnceLock::new();
    RUNNER.get_or_init(|| V8Runner::new(program(), mcbench_build::sandbox::platform().unwrap()))
}

pub fn sha(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// A golden case (`harness/golden.js`).
pub struct Case {
    pub id: String,
    pub test: Option<String>,
    pub registry: String,
    pub source: String,
    pub world: String,
    pub settled: Option<(u32, Option<String>)>,
    pub read_file: Option<Value>,
    pub expected: Value,
}

fn golden_dir() -> PathBuf {
    crate_dir().join("tests/golden")
}

/// The golden lines as serde_json reads them: Node's `\udXXX` escapes of
/// lone surrogates (adv-lone-surrogate's log) as `\ufffd`, as the parent
/// reads the child's JSON.
fn lone_surrogates(line: &str) -> String {
    let re = regex::Regex::new(r"\\\\|\\u[dD][89abAB][0-9a-fA-F]{2}\\u[dD][c-fC-F][0-9a-fA-F]{2}|\\u[dD][89a-fA-F][0-9a-fA-F]{2}").unwrap();
    re.replace_all(line, |c: &regex::Captures| {
        let m = &c[0];
        if m == "\\\\" || m.len() == 12 {
            m.to_string()
        } else {
            "\\ufffd".to_string()
        }
    })
    .into_owned()
}

pub fn golden() -> &'static [Case] {
    static CASES: OnceLock<Vec<Case>> = OnceLock::new();
    CASES.get_or_init(|| {
        let text = std::fs::read_to_string(golden_dir().join("cases.jsonl")).unwrap();
        text.lines()
            .map(|line| {
                let o: Value = serde_json::from_str(&lone_surrogates(line)).unwrap();
                Case {
                    id: o["id"].as_str().unwrap().into(),
                    test: o["test"].as_str().map(String::from),
                    registry: o["registry"].as_str().unwrap().into(),
                    source: o["source"].as_str().unwrap().into(),
                    world: o["world"].as_str().unwrap().into(),
                    settled: o["settled"].as_object().map(|s| {
                        (
                            s["ticks"].as_u64().unwrap() as u32,
                            s["world"].as_str().map(String::from),
                        )
                    }),
                    read_file: Some(o["readFile"].clone()).filter(|v| !v.is_null()),
                    expected: o["expected"].clone(),
                }
            })
            .collect()
    })
}

/// The golden cases one of today's tests made, in order.
pub fn cases_of(prefix: &str, test: &str) -> Vec<&'static Case> {
    let cases: Vec<_> = golden()
        .iter()
        .filter(|case| case.id.starts_with(prefix) && case.test.as_deref() == Some(test))
        .collect();
    assert!(!cases.is_empty(), "no golden case of {prefix} {test:?}");
    cases
}

pub fn case(id: &str) -> &'static Case {
    golden()
        .iter()
        .find(|case| case.id == id)
        .unwrap_or_else(|| panic!("no golden case {id}"))
}

pub fn world_bytes(name: &str) -> Vec<u8> {
    let file = std::fs::File::open(golden_dir().join(format!("worlds/{name}.bin.gz"))).unwrap();
    let mut bytes = Vec::new();
    flate2::read::GzDecoder::new(file)
        .read_to_end(&mut bytes)
        .unwrap();
    bytes
}

pub fn registry(name: &str) -> Value {
    serde_json::from_slice(&std::fs::read(golden_dir().join(format!("registries/{name}"))).unwrap())
        .unwrap()
}

/// `encodeWorld(new World())`.
pub const EMPTY_WORLD: [u8; 10] = [2, 0, 0, 0, b'[', b']', 0, 0, 0, 0];

/// A host answering `mc.readFile` from recorded answers and `mc.settled`
/// with recorded bytes, counting what it was asked.
pub struct Host {
    pub answers: Option<Value>,
    pub settled: Option<Vec<u8>>,
    pub reads: Arc<Mutex<Vec<String>>>,
    pub settled_calls: Arc<Mutex<u32>>,
}

#[async_trait]
impl EncodedHost for Host {
    async fn read_file(&mut self, path: &str) -> Result<String, String> {
        self.reads.lock().unwrap().push(path.to_string());
        let answers = self.answers.as_ref().expect("readFile is offered");
        let Some(answer) = answers.get(path) else {
            return Err(format!("mc.readFile: {path}: no such file in /work"));
        };
        let text = answer["text"].as_str().unwrap().to_string();
        if answer["ok"].as_bool().unwrap() {
            Ok(text)
        } else {
            Err(text)
        }
    }

    async fn settled_world(&mut self) -> Result<Vec<u8>, String> {
        *self.settled_calls.lock().unwrap() += 1;
        Ok(self.settled.clone().unwrap_or_else(|| EMPTY_WORLD.to_vec()))
    }
}

/// What a run of a case left, and what its host was asked.
pub struct Ran {
    pub result: Result<EncodedOutcome, BuildFailure>,
    pub reads: Vec<String>,
    pub settled_calls: u32,
}

impl Ran {
    /// The comparable form `harness/node-run.js` writes.
    pub fn value(&self) -> Value {
        value_of(&self.result)
    }

    pub fn outcome(&self) -> &EncodedOutcome {
        self.result
            .as_ref()
            .unwrap_or_else(|failure| panic!("the program failed: {failure:?}"))
    }

    pub fn failure(&self) -> &BuildFailure {
        match &self.result {
            Err(failure) => failure,
            Ok(outcome) => panic!("the program finished: {outcome:?}"),
        }
    }
}

const MAX_ERROR_CHARS: usize = 8000;

pub fn value_of(result: &Result<EncodedOutcome, BuildFailure>) -> Value {
    match result {
        Ok(outcome) => json!({
            "ok": true,
            "changes": sha(&outcome.changes),
            "logs": outcome.logs,
            "files": outcome.files.iter().map(|file| json!({
                "path": file.path,
                "length": file.text.encode_utf16().count(),
                "sha": sha(file.text.as_bytes()),
            })).collect::<Vec<_>>(),
            "touched": outcome.touched,
        }),
        Err(failure) => {
            let error = if failure.message.encode_utf16().count() > MAX_ERROR_CHARS {
                let mut units = 0;
                let cut: String = failure
                    .message
                    .chars()
                    .take_while(|c| {
                        units += c.len_utf16();
                        units <= MAX_ERROR_CHARS
                    })
                    .collect();
                format!("{cut}\n[error message cut at {MAX_ERROR_CHARS} characters]")
            } else {
                failure.message.clone()
            };
            json!({
                "ok": false,
                "error": error,
                "logs": if failure.logs.is_empty() { Value::Null } else { json!(failure.logs) },
            })
        }
    }
}

/// Runs `source` on `world` with `registry`, as `runProgram` would.
pub async fn run(
    source: &str,
    registry: &Value,
    world: &[u8],
    answers: Option<Value>,
    settled: Option<(u32, Option<Vec<u8>>)>,
) -> Ran {
    let request = BuildRequest {
        source: source.to_string(),
        registry: registry.clone(),
        read_file: answers.is_some(),
        settled_ticks: settled.as_ref().map(|(ticks, _)| *ticks),
    };
    let reads = Arc::new(Mutex::new(Vec::new()));
    let settled_calls = Arc::new(Mutex::new(0));
    let mut host = Host {
        answers,
        settled: settled.and_then(|(_, world)| world),
        reads: reads.clone(),
        settled_calls: settled_calls.clone(),
    };
    let result = runner()
        .run_encoded(&request, world, &mut host, &CancellationToken::new())
        .await;
    Ran {
        result,
        reads: reads.lock().unwrap().clone(),
        settled_calls: *settled_calls.lock().unwrap(),
    }
}

pub async fn run_case(case: &Case) -> Ran {
    run(
        &case.source,
        &registry(&case.registry),
        &world_bytes(&case.world),
        case.read_file.clone(),
        case.settled
            .as_ref()
            .map(|(ticks, world)| (*ticks, world.as_ref().map(|name| world_bytes(name)))),
    )
    .await
}

/// `harness/allow-list.json`.
fn allow_list() -> &'static HashMap<String, Value> {
    static LIST: OnceLock<HashMap<String, Value>> = OnceLock::new();
    LIST.get_or_init(|| {
        let text = std::fs::read_to_string(crate_dir().join("harness/allow-list.json")).unwrap();
        let list: serde_json::Map<String, Value> = serde_json::from_str(&text).unwrap();
        list.into_iter().collect()
    })
}

/// The result this platform's child must give for `case`: Node's, or the
/// one the allow-list pins (per platform when the platforms differ).
pub fn wanted(case: &Case) -> Value {
    match allow_list().get(&case.id) {
        None => case.expected.clone(),
        Some(entry) => pinned(&entry["rust"]).clone(),
    }
}

/// An allow-list pin: one result for every platform, or one per platform.
/// Windows has none of its own: its child is the Linux program inside the
/// app's WSL2 distro, so it is held to Linux's.
fn pinned(rust: &Value) -> &Value {
    if rust.get("ok").is_some() {
        rust
    } else {
        rust.get(std::env::consts::OS).unwrap_or(&rust["linux"])
    }
}

/// Whether `got` is what this platform's child must give for `case`: an
/// unpinned allow-list case (the recursion depth) is one integer above
/// 10,000, every other case is [`wanted`].
pub fn accepted(case: &Case, got: &Value) -> bool {
    if allow_list()
        .get(&case.id)
        .is_some_and(|entry| entry["unpinned"] == true)
    {
        return got["ok"] == true
            && got["logs"].as_array().is_some_and(|logs| {
                logs.len() == 1
                    && logs[0]
                        .as_str()
                        .unwrap()
                        .parse::<u32>()
                        .is_ok_and(|n| n > 10_000)
            });
    }
    *got == wanted(case)
}

/// Runs `case` and requires [`accepted`].
pub async fn check(case: &Case) -> Ran {
    let ran = run_case(case).await;
    let got = ran.value();
    if accepted(case, &got) {
        return ran;
    }
    panic!(
        "{}: the child's result is not the wanted one\n  got:  {}\n  want: {}\n  node: {}",
        case.id,
        short(&got),
        short(&wanted(case)),
        short(&case.expected)
    );
}

pub fn short(value: &Value) -> String {
    let text = value.to_string();
    if text.len() > 1500 {
        format!("{}...", &text[..text.floor_char_boundary(1500)])
    } else {
        text
    }
}

/// The golden cases run one at a time, after the others: the four that run
/// into the 5 s limit by design (a busy loop each, whose result is the
/// timeout however loaded the machine is) and the one other case over
/// 150 ms in a release run on adipc and the Mac (adv-cpu-2e8, 156 to
/// 196 ms; 2026-11-17).
const HEAVY: [&str; 5] = [
    "adv-loop",
    "adv-loop-catch",
    "adv-loop-finally",
    "adv-regex-backtrack",
    "adv-cpu-2e8",
];

/// The golden cases no always-run test runs, because their own time is a
/// large share of the 5 s limit: `adv-cpu-objects` (1.0 to 1.4 s idle,
/// 3.6 s on adipc with every core busy) and `adv-cpu-mc-set` (0.36 to
/// 0.40 s). The differential harness runs them (`examples/corpus.rs`);
/// `adv-math-bits` (0.6 s) has its own test, [`math_test`], alone.
const NOT_ALWAYS_RUN: [&str; 3] = ["adv-cpu-objects", "adv-cpu-mc-set", "adv-math-bits"];

async fn timed(case: &'static Case) -> (&'static Case, Value, std::time::Duration) {
    let started = std::time::Instant::now();
    let got = run_case(case).await.value();
    (case, got, started.elapsed())
}

/// Every golden case not claimed by a ported or dedicated test: the light
/// ones four at a time, then [`HEAVY`] one at a time. Prints the slowest
/// case's time against the 5 s limit.
pub fn corpus_tests() -> Vec<Test> {
    vec![test(
        "corpus: every recorded and adversarial case gives Node 26.13.1's result or its pinned allow-list result",
        || async {
            let cases: Vec<&'static Case> = golden()
                .iter()
                .filter(|case| {
                    !case.id.starts_with("recmcdsl-")
                        && !case.id.starts_with("recbuildsandbox-")
                        && !NOT_ALWAYS_RUN.contains(&case.id.as_str())
                })
                .collect();
            let semaphore = Arc::new(tokio::sync::Semaphore::new(4));
            let mut tasks = Vec::new();
            for &case in cases
                .iter()
                .filter(|case| !HEAVY.contains(&case.id.as_str()))
            {
                let semaphore = semaphore.clone();
                tasks.push(tokio::spawn(async move {
                    let _permit = semaphore.acquire().await.unwrap();
                    timed(case).await
                }));
            }
            let mut results = Vec::new();
            for task in tasks {
                results.push(task.await.unwrap());
            }
            for &case in cases
                .iter()
                .filter(|case| HEAVY.contains(&case.id.as_str()))
            {
                results.push(timed(case).await);
            }
            let total = results.len();
            let mut wrong = Vec::new();
            for (case, got, _) in &results {
                if !accepted(case, got) {
                    wrong.push(format!(
                        "{}\n  got:  {}\n  want: {}",
                        case.id,
                        short(got),
                        short(&wanted(case))
                    ));
                }
            }
            let slowest = results
                .iter()
                .filter(|(case, _, _)| !HEAVY[..4].contains(&case.id.as_str()))
                .max_by_key(|(_, _, took)| *took)
                .unwrap();
            println!(
                "    {total} cases; the slowest that does not run into the limit: {} in {} ms of 5,000",
                slowest.0.id,
                slowest.2.as_millis()
            );
            if !wrong.is_empty() {
                panic!(
                    "{} of {total} cases differ:\n{}",
                    wrong.len(),
                    wrong.join("\n")
                );
            }
        },
    )]
}

/// The folder whose `server/` the golden files name: two cases
/// (adv-capture-stack-trace, adv-prepare-stack-trace) log Node's stack, which
/// names that folder and is cut at 300 characters. They were recorded
/// (`harness/golden.js`) from a tree in a scratch folder under a home
/// directory, and that path was rewritten after recording to this neutral
/// one of the same length (50 characters), so every cut falls where it fell.
const RECORDED_ROOT: &str = "/srv/build/mcbench-golden-recording-folder-xx/tree";

/// The files of `server/` that `runProgram` loads: `build-sandbox.js` and
/// the child's five sources.
const SERVER_FILES: [&str; 6] = [
    "build-sandbox.js",
    "build-child.js",
    "mc-dsl.js",
    "world.js",
    "minecraft-dimension.js",
    "build-transfer.js",
];

/// With `MCBENCH_BUILD_GOLDEN_NODE=<node>` (Node 26.13.1): the golden files
/// are that Node's results. The cases are written back as a corpus,
/// `harness/node-run.js` runs them through today's `runProgram`, and every
/// result must be the golden one. So that the result does not depend on
/// where the tree is, Node runs a copy of `node-run.js` and the server's
/// files laid out as in the tree under `/tmp`, in a folder whose real path
/// is as long as [`RECORDED_ROOT`], and that path (or the part of it a
/// log's cut at 300 characters keeps) reads as [`RECORDED_ROOT`] in its
/// results. Without the variable the
/// test is not run (it needs Node).
pub fn golden_node_test() -> Option<Test> {
    let Some(node) = std::env::var_os("MCBENCH_BUILD_GOLDEN_NODE") else {
        println!(
            "ignored: golden: the golden files are Node's results (needs MCBENCH_BUILD_GOLDEN_NODE=<node>)"
        );
        return None;
    };
    Some(test(
        "golden: the golden files are Node 26.13.1's results",
        move || {
            let node = node.clone();
            async move {
                let version = std::process::Command::new(&node)
                    .arg("--version")
                    .output()
                    .unwrap();
                assert_eq!(
                    String::from_utf8_lossy(&version.stdout).trim(),
                    "v26.13.1",
                    "the golden files are Node 26.13.1's"
                );
                let corpus = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("golden-corpus");
                let _ = std::fs::remove_dir_all(&corpus);
                std::fs::create_dir_all(corpus.join("registries")).unwrap();
                for case in golden() {
                    let dir = corpus.join("cases").join(&case.id);
                    std::fs::create_dir_all(&dir).unwrap();
                    std::fs::write(dir.join("source.js"), &case.source).unwrap();
                    std::fs::write(dir.join("world.bin"), world_bytes(&case.world)).unwrap();
                    let settled = case.settled.as_ref().map(|(ticks, world)| {
                        if let Some(name) = world {
                            std::fs::write(dir.join("settled.bin"), world_bytes(name)).unwrap();
                        }
                        json!({ "ticks": ticks, "world": world.as_ref().map(|_| "settled.bin") })
                    });
                    let meta = json!({ "id": case.id, "registry": case.registry, "readFile": case.read_file, "settled": settled });
                    std::fs::write(dir.join("case.json"), meta.to_string()).unwrap();
                    std::fs::copy(
                        golden_dir().join("registries").join(&case.registry),
                        corpus.join("registries").join(&case.registry),
                    )
                    .unwrap();
                }
                let out = corpus.join("node.jsonl");
                let tmp = std::fs::canonicalize("/tmp").unwrap();
                let base = format!("{}/mcbench-golden-{}-", tmp.display(), std::process::id());
                assert!(
                    base.len() < RECORDED_ROOT.len(),
                    "{base} is longer than the recorded {RECORDED_ROOT}"
                );
                let root = format!("{base}{}", "x".repeat(RECORDED_ROOT.len() - base.len()));
                let _ = std::fs::remove_dir_all(&root);
                std::fs::create_dir_all(format!("{root}/server")).unwrap();
                let server = crate_dir().join("../../../server");
                for file in SERVER_FILES {
                    std::fs::copy(server.join(file), format!("{root}/server/{file}")).unwrap();
                }
                let harness = format!("{root}/native/crates/build/harness");
                std::fs::create_dir_all(&harness).unwrap();
                std::fs::copy(
                    crate_dir().join("harness/node-run.js"),
                    format!("{harness}/node-run.js"),
                )
                .unwrap();
                let status = std::process::Command::new(&node)
                    .arg(format!("{harness}/node-run.js"))
                    .arg(&corpus)
                    .arg(&out)
                    .env("W", &root)
                    .status()
                    .unwrap();
                std::fs::remove_dir_all(&root).unwrap();
                assert!(status.success());
                // The folder reads as the recorded one, also where a log's cut
                // at 300 characters falls inside it.
                let mut text = std::fs::read_to_string(&out)
                    .unwrap()
                    .replace(&root, RECORDED_ROOT);
                for cut in 1..root.len() {
                    text = text.replace(
                        &format!("{}...(truncated)", &root[..cut]),
                        &format!("{}...(truncated)", &RECORDED_ROOT[..cut]),
                    );
                }
                let mut wrong = Vec::new();
                for line in text.lines() {
                    let o: Value = serde_json::from_str(&lone_surrogates(line)).unwrap();
                    let got = if o["ok"].as_bool().unwrap() {
                        json!({ "ok": true, "changes": o["changes"], "logs": o["logs"], "files": o["files"], "touched": o["touched"] })
                    } else {
                        json!({ "ok": false, "error": o["error"], "logs": o["logs"] })
                    };
                    let case = case(o["id"].as_str().unwrap());
                    if got != case.expected {
                        wrong.push(format!(
                            "{}\n  node:   {}\n  golden: {}",
                            case.id,
                            short(&got),
                            short(&case.expected)
                        ));
                    }
                }
                assert!(
                    wrong.is_empty(),
                    "{} cases differ from the golden files:\n{}",
                    wrong.len(),
                    wrong.join("\n")
                );
            }
        },
    ))
}

/// The hashes of docs/native-app.md, `Math` results: the embedded V8's, the
/// same on macOS arm64 and Linux x64 (and required of Windows).
pub const MATH_HASHES: [(&str, &str); 15] = [
    ("sin", "e9e860bf"),
    ("cos", "40780711"),
    ("tan", "35370bdb"),
    ("asin", "8958913f"),
    ("atan", "2d10d850"),
    ("sinh", "cdce13c5"),
    ("cosh", "ef5980fe"),
    ("asinh", "4dc2cbe6"),
    ("acosh", "200aeaed"),
    ("exp", "c368d568"),
    ("expm1", "63be4303"),
    ("log", "80620689"),
    ("log1p", "4ceced83"),
    ("log10", "ceeb285e"),
    ("atan2", "c491a5d1"),
];

/// The three-platform `Math` test (`math-fixed.js`, inputs of fixed bits):
/// every function in `same` of `tests/golden/math-fixed.json` gives those
/// hashes on every platform; the others (pow and `**`, which call the
/// platform's libm) give this platform's (Windows is Linux's: its child is
/// the Linux program, on the Linux C library, inside the app's WSL2
/// distro). The hash compared is the one with every NaN
/// as one pattern: V8 gives a NaN result's sign and payload per platform
/// and, for asin, per run (0x7ff4… or 0x7ffc…, with the tier that ran the
/// call), so the all-bits hash is not deterministic and not compared.
pub fn math_fixed_test() -> Test {
    test(
        "math: fixed-bit inputs give the same values on every platform but pow and ** as pinned per platform",
        || async {
            let pins: Value =
                serde_json::from_str(include_str!("../golden/math-fixed.json")).unwrap();
            let platform = pins["platform"]
                .get(std::env::consts::OS)
                .unwrap_or(&pins["platform"]["linux"]);
            let ran = run(
                include_str!("math-fixed.js"),
                &json!({ "stone": [{}, {}] }),
                &EMPTY_WORLD,
                None,
                None,
            )
            .await;
            let logs = &ran.outcome().logs;
            let expected =
                pins["same"].as_object().unwrap().len() + platform.as_object().unwrap().len();
            assert_eq!(logs.len(), expected, "{logs:?}");
            for line in logs {
                let [name, _all_bits, canonical] = line.split(' ').collect::<Vec<_>>()[..] else {
                    panic!("{line:?} is not a name and two hashes");
                };
                let want = pins["same"]
                    .get(name)
                    .or_else(|| platform.get(name))
                    .unwrap_or_else(|| panic!("{name} is not pinned"));
                assert_eq!(canonical, want.as_str().unwrap(), "{name}");
            }
        },
    )
}

/// The `Math` bits probe (`adv-math-bits`): the plan's hashes on every
/// platform but asin's and acos's. The probe hashes all bits of every
/// result, and asin's and acos's NaN results (inputs past ±1) come back as
/// 0x7ff4000000000000 or 0x7ffc000000000000 depending on which tier ran the
/// call, which changes from run to run under load (asin in Node 26.13.1 as
/// in the child: 11 and 4 of 40 runs on adipc with every core busy,
/// 2026-11-17; acos 4f17ce45 for its pinned 6255d23d once in 30 runs on
/// adipc at load 30, 2026-11-17); their values are held by
/// [`math_fixed_test`]. It runs alone, and prints its time against
/// the 5 s limit.
pub fn math_test() -> Test {
    test(
        "math: adv-math-bits gives the plan's hashes on every platform",
        || async {
            let case = case("adv-math-bits");
            let started = std::time::Instant::now();
            let ran = run_case(case).await;
            println!(
                "    adv-math-bits ran in {} ms of 5,000",
                started.elapsed().as_millis()
            );
            let logs = &ran.outcome().logs;
            for (name, hash) in MATH_HASHES.iter().filter(|(name, _)| *name != "asin") {
                assert!(
                    logs.contains(&format!("{name} {hash}")),
                    "{name} is not {hash}: {logs:?}"
                );
            }
            let pinned = pinned(&allow_list()["adv-math-bits"]["rust"]);
            let tiered = |line: &Value| {
                let line = line.as_str().unwrap();
                line.starts_with("asin ") || line.starts_with("acos ")
            };
            let untiered = |logs: &Value| -> Vec<Value> {
                logs.as_array()
                    .unwrap()
                    .iter()
                    .filter(|line| !tiered(line))
                    .cloned()
                    .collect()
            };
            assert_eq!(untiered(&ran.value()["logs"]), untiered(&pinned["logs"]));
        },
    )
}
