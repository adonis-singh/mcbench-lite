//! The Rust side of the speed table's `build` rows (`harness/bench.js` is
//! the Node side and writes the worlds this reads): the same programs and
//! worlds through [`V8Runner::run_encoded`] with the platform sandbox, a
//! 100 ms pause before each, `passes` passes. `coldStartMs` is the spawn of
//! the sandboxed child to its result with no spare process. `peak` is the
//! peak resident memory (`wait4`'s `ru_maxrss`) of one child, today's
//! (`node-command.json`) and this one, for a one-block program on an empty
//! world and on the 2.1M-block world. Prints JSON.
//!
//! `cargo run --release --example bench -- <mcbench-build-child> <blocks.json> <dir> [passes]`

use std::path::Path;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use mcbench_build::{BuildRequest, EncodedHost, V8Runner};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

const EMPTY_WORLD: [u8; 10] = [2, 0, 0, 0, b'[', b']', 0, 0, 0, 0];
const ONE_BLOCK: &str = r#"module.exports = (mc) => mc.set([0, 60, 0], "gold_block")"#;
const REPLACE: &str = r#"module.exports = mc => mc.replace("stone", "oak_planks")"#;
const OUTLINE: &str =
    r#"module.exports = mc => mc.fill([0,0,0], [299,199,49], "stone", "outline")"#;

struct Host(String);

#[async_trait]
impl EncodedHost for Host {
    async fn read_file(&mut self, _: &str) -> Result<String, String> {
        Ok(self.0.clone())
    }
    async fn settled_world(&mut self) -> Result<Vec<u8>, String> {
        unreachable!("no program here reads mc.settled")
    }
}

/// The child's confined command on this platform, for [`once`]'s `wait4`
/// (the runner starts the same command through the sandbox's `spawn`).
fn confined(program: &Path) -> std::process::Command {
    #[cfg(target_os = "macos")]
    return mcbench_build::sandbox::macos::command(program).unwrap();
    #[cfg(not(target_os = "macos"))]
    return mcbench_build::sandbox::linux::command(program).unwrap();
}

fn input(source: &str, registry: &Value, world: &[u8]) -> Vec<u8> {
    let request = serde_json::to_vec(
        &json!({ "source": source, "registry": registry, "readFile": false, "settledTicks": null }),
    )
    .unwrap();
    let mut bytes = (request.len() as u32).to_le_bytes().to_vec();
    bytes.extend_from_slice(&request);
    bytes.extend_from_slice(&(world.len() as u32).to_le_bytes());
    bytes.extend_from_slice(world);
    bytes
}

#[cfg(not(unix))]
fn once(_: std::process::Command, _: &[u8]) -> (f64, u64) {
    unimplemented!("the peak memory comes from wait4, which Windows does not have")
}

/// Spawn to the first byte of the answer, and the peak resident memory in KiB
/// once the child exits.
#[cfg(unix)]
#[allow(clippy::zombie_processes, reason = "wait4 reaps the child")]
fn once(mut command: std::process::Command, input: &[u8]) -> (f64, u64) {
    use std::io::{Read, Write};
    let start = Instant::now();
    let mut child = command
        .env_clear()
        .current_dir("/")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let input = input.to_vec();
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(&input);
    });
    // A child that does not finish within 30 s is killed (and, under
    // bubblewrap's --die-with-parent, its program with it), so a hung
    // measurement never leaves processes behind (2026-11-17: one did).
    let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let pid = child.id() as libc::pid_t;
    let watchdog = {
        let done = done.clone();
        std::thread::spawn(move || {
            let deadline = Instant::now() + std::time::Duration::from_secs(30);
            while !done.load(std::sync::atomic::Ordering::Relaxed) {
                if Instant::now() > deadline {
                    // SAFETY: the pid is our child, not yet waited for.
                    unsafe { libc::kill(pid, libc::SIGKILL) };
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        })
    };
    let mut stdout = child.stdout.take().unwrap();
    let mut first = [0u8; 1];
    stdout
        .read_exact(&mut first)
        .expect("the child answers within 30 s");
    let ms = start.elapsed().as_secs_f64() * 1e3;
    let mut rest = Vec::new();
    stdout.read_to_end(&mut rest).unwrap();
    writer.join().unwrap();
    let mut status = 0;
    // SAFETY: an all-zero rusage is valid, and wait4 fills it for our own
    // child's pid, which has not been waited for.
    let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
    // The watchdog stops before the child is reaped, so it never signals a
    // reused pid.
    done.store(true, std::sync::atomic::Ordering::Relaxed);
    watchdog.join().unwrap();
    let waited = unsafe { libc::wait4(pid, &mut status, 0, &mut usage) };
    assert_eq!(waited, pid);
    let max = usage.ru_maxrss as u64;
    // macOS reports bytes, Linux KiB.
    let kib = if cfg!(target_os = "macos") {
        max / 1024
    } else {
        max
    };
    (ms, kib)
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [program, blocks, dir, rest @ ..] = args.as_slice() else {
        panic!("usage: bench <mcbench-build-child> <blocks.json> <dir> [passes]");
    };
    let passes: usize = rest.first().map_or(5, |p| p.parse().unwrap());
    let dir = Path::new(dir);
    let program = std::fs::canonicalize(program).unwrap();
    let registry: Value = serde_json::from_slice(&std::fs::read(blocks).unwrap()).unwrap();
    let small = json!({ "stone": [{}, {}], "oak_planks": [{}, {}] });
    let terrain = std::fs::read(dir.join("terrain.bin")).unwrap();
    let village_world = std::fs::read(dir.join("village.bin")).unwrap();
    let stone = std::fs::read(dir.join("stone3m.bin")).unwrap();
    let heightmap = std::fs::read_to_string(dir.join("heightmap.json")).unwrap();
    let bench = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../scripts/benchmark-build.js"),
    )
    .unwrap();
    let village_src = {
        let from = bench.find("const VILLAGE = `").unwrap() + "const VILLAGE = `".len();
        let to = from + bench[from..].find("`;").unwrap();
        bench[from..to].replace("${SIZE}", "256")
    };
    let runner = V8Runner::new(program.clone(), mcbench_build::sandbox::platform().unwrap());
    let mut samples: serde_json::Map<String, Value> = serde_json::Map::new();
    let mut push = |name: &str, ms: f64| {
        samples
            .entry(name)
            .or_insert_with(|| json!([]))
            .as_array_mut()
            .unwrap()
            .push(json!(ms));
    };
    let rows: [(&str, &str, &Value, &[u8]); 5] = [
        ("villageMs", &village_src, &registry, &terrain),
        (
            "oneBlockOnLargeWorldMs",
            ONE_BLOCK,
            &registry,
            &village_world,
        ),
        ("emptyWorldMs", ONE_BLOCK, &registry, &EMPTY_WORLD),
        ("replace3mMs", REPLACE, &small, &stone),
        ("outline3mMs", OUTLINE, &small, &EMPTY_WORLD),
    ];
    for _ in 0..passes {
        for (name, source, reg, world) in rows {
            tokio::time::sleep(Duration::from_millis(100)).await;
            let request = BuildRequest {
                source: source.to_string(),
                registry: reg.clone(),
                read_file: true,
                settled_ticks: None,
            };
            let start = Instant::now();
            let outcome = runner
                .run_encoded(
                    &request,
                    world,
                    &mut Host(heightmap.clone()),
                    &CancellationToken::new(),
                )
                .await
                .unwrap();
            push(name, start.elapsed().as_secs_f64() * 1e3);
            if name == "villageMs" {
                assert_eq!(outcome.logs, ["village 6480 4"]);
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
        let (ms, _) = once(
            confined(&program),
            &input(ONE_BLOCK, &registry, &EMPTY_WORLD),
        );
        push("coldStartMs", ms);
    }
    let node: Vec<String> =
        serde_json::from_slice(&std::fs::read(dir.join("node-command.json")).unwrap()).unwrap();
    let mut peak = serde_json::Map::new();
    for (label, world) in [
        ("emptyWorld", &EMPTY_WORLD[..]),
        ("largeWorld", &village_world[..]),
    ] {
        let bytes = input(ONE_BLOCK, &registry, world);
        let mut node_command = std::process::Command::new(&node[0]);
        node_command.args(&node[1..]);
        let (_, node_kib) = once(node_command, &bytes);
        let (_, rust_kib) = once(confined(&program), &bytes);
        peak.insert(
            label.into(),
            json!({ "nodeKiB": node_kib, "rustKiB": rust_kib }),
        );
    }
    let median = |values: &Vec<Value>| {
        let mut v: Vec<f64> = values.iter().map(|x| x.as_f64().unwrap()).collect();
        v.sort_by(f64::total_cmp);
        v[v.len() / 2]
    };
    let out: serde_json::Map<String, Value> = samples
        .iter()
        .map(|(name, values)| {
            (
                name.clone(),
                json!({ "median": median(values.as_array().unwrap()), "samples": values }),
            )
        })
        .chain([("peak".to_string(), Value::Object(peak))])
        .collect();
    println!("{}", Value::Object(out));
}
