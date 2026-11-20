//! The Rust side of the differential harness (`harness/node-run.js` is the
//! Node side): every corpus case through [`V8Runner::run_encoded`] with the
//! platform sandbox and `<mcbench-build-child>` as the child's program, one build
//! at a time with a 100 ms pause between builds, as `node-run.js` runs them.
//! Writes the same JSON lines (`harness/compare.js` classifies them).
//!
//! `cargo run --release --example corpus -- <mcbench-build-child> <corpus> <out.jsonl> [repeat] [id-substring]`
//!
//! `READFILE_ROOT=<from>=<to>` moves the recorded `/work` folders, as for
//! `node-run.js`.

use std::path::{Component, Path, PathBuf};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use mcbench_build::{BuildRequest, EncodedHost, V8Runner};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;

const MAX_ERROR_CHARS: usize = 8000;
/// `encodeWorld(new World())`: an empty palette and no chunks.
const EMPTY_WORLD: [u8; 10] = [2, 0, 0, 0, b'[', b']', 0, 0, 0, 0];

fn sha(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// `path.resolve(root, rel)`: lexical, as Node resolves.
fn resolve(root: &Path, rel: &str) -> PathBuf {
    let mut out = PathBuf::new();
    for part in root.join(rel).components() {
        match part {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other),
        }
    }
    out
}

/// Node's `error.code` of an I/O error.
fn code(error: &std::io::Error) -> String {
    let codes = [
        (libc::ENOENT, "ENOENT"),
        (libc::EISDIR, "EISDIR"),
        (libc::ENOTDIR, "ENOTDIR"),
        (libc::EACCES, "EACCES"),
        (libc::EPERM, "EPERM"),
        (libc::ELOOP, "ELOOP"),
        (libc::ENAMETOOLONG, "ENAMETOOLONG"),
    ];
    let raw = error.raw_os_error();
    codes
        .iter()
        .find(|(errno, _)| Some(*errno) == raw)
        .map(|(_, name)| name.to_string())
        .unwrap_or_else(|| format!("{error}"))
}

struct Case {
    id: String,
    meta: Value,
    source: String,
    registry: Value,
    world: Vec<u8>,
    settled: Option<Vec<u8>>,
}

struct Host<'a> {
    case: &'a Case,
    read_root: Option<PathBuf>,
}

#[async_trait]
impl EncodedHost for Host<'_> {
    async fn read_file(&mut self, rel: &str) -> Result<String, String> {
        if let Some(root) = &self.read_root {
            let file = resolve(root, rel.strip_prefix("/work/").unwrap_or(rel));
            if !file
                .to_str()
                .unwrap()
                .starts_with(&format!("{}/", root.display()))
            {
                return Err(format!("mc.readFile: {rel} is outside /work"));
            }
            return std::fs::read(&file)
                .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
                .map_err(|error| format!("mc.readFile: {rel}: {}", code(&error)));
        }
        let Some(answer) = self.case.meta["readFile"].get(rel) else {
            return Err(format!("mc.readFile: {rel}: no such file in /work"));
        };
        let text = answer["text"].as_str().unwrap().to_string();
        if answer["ok"].as_bool().unwrap() {
            Ok(text)
        } else {
            Err(text)
        }
    }

    async fn settled_world(&mut self) -> Result<Vec<u8>, String> {
        Ok(self
            .case
            .settled
            .clone()
            .unwrap_or_else(|| EMPTY_WORLD.to_vec()))
    }
}

fn load(corpus: &Path, id: &str) -> Case {
    let dir = corpus.join("cases").join(id);
    let meta: Value =
        serde_json::from_slice(&std::fs::read(dir.join("case.json")).unwrap()).unwrap();
    let registry_file = corpus
        .join("registries")
        .join(meta["registry"].as_str().unwrap());
    let registry = serde_json::from_slice(&std::fs::read(registry_file).unwrap()).unwrap();
    let settled = meta["settled"]["world"]
        .as_str()
        .map(|file| std::fs::read(dir.join(file)).unwrap());
    Case {
        id: id.to_string(),
        source: String::from_utf8(std::fs::read(dir.join("source.js")).unwrap()).unwrap(),
        world: std::fs::read(dir.join("world.bin")).unwrap(),
        registry,
        settled,
        meta,
    }
}

/// `String.prototype.slice(0, n)` on UTF-16 units.
fn js_cut(text: &str) -> String {
    let mut units = 0;
    let mut out = String::new();
    for c in text.chars() {
        units += c.len_utf16();
        if units > MAX_ERROR_CHARS {
            break;
        }
        out.push(c);
    }
    format!("{out}\n[error message cut at {MAX_ERROR_CHARS} characters]")
}

async fn run_case(runner: &V8Runner, case: &Case) -> Value {
    let request = BuildRequest {
        source: case.source.clone(),
        registry: case.registry.clone(),
        read_file: !case.meta["readFile"].is_null() || case.meta.get("readFileDir").is_some(),
        settled_ticks: case.meta["settled"]["ticks"].as_u64().map(|t| t as u32),
    };
    let read_root = case
        .meta
        .get("readFileDir")
        .and_then(Value::as_str)
        .map(|dir| {
            let moved = std::env::var("READFILE_ROOT").unwrap_or_else(|_| "=".into());
            let (from, to) = moved.split_once('=').unwrap();
            PathBuf::from(if from.is_empty() {
                dir.to_string()
            } else {
                dir.replacen(from, to, 1)
            })
        });
    let mut host = Host { case, read_root };
    match runner
        .run_encoded(&request, &case.world, &mut host, &CancellationToken::new())
        .await
    {
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
        Err(failure) => json!({
            "ok": false,
            "error": if failure.message.encode_utf16().count() > MAX_ERROR_CHARS { js_cut(&failure.message) } else { failure.message },
            "logs": if failure.logs.is_empty() { Value::Null } else { json!(failure.logs) },
        }),
    }
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [program, corpus, out_file, rest @ ..] = args.as_slice() else {
        panic!("usage: corpus <mcbench-build-child> <corpus> <out.jsonl> [repeat] [id-substring]");
    };
    let repeat: usize = rest.first().map_or(1, |r| r.parse().unwrap());
    let filter = rest.get(1).cloned().unwrap_or_default();
    let corpus = Path::new(corpus);
    let program = std::fs::canonicalize(program).unwrap();
    let runner = V8Runner::new(program, mcbench_build::sandbox::platform().unwrap());
    let mut ids: Vec<String> = std::fs::read_dir(corpus.join("cases"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .filter(|id| id.contains(&filter))
        .collect();
    // Node's default sort: UTF-16 code units, the same as bytes for ASCII ids.
    ids.sort();
    let mut lines = String::new();
    for (done, id) in ids.iter().enumerate() {
        let case = load(corpus, id);
        let mut first: Option<Value> = None;
        let mut unstable = false;
        let mut runs = Vec::new();
        for _ in 0..repeat {
            tokio::time::sleep(Duration::from_millis(100)).await;
            let started = Instant::now();
            let result = run_case(&runner, &case).await;
            runs.push(started.elapsed().as_secs_f64() * 1e3);
            match &first {
                None => first = Some(result),
                Some(first) => unstable |= *first != result,
            }
        }
        let mut line = json!({ "id": case.id });
        line.as_object_mut()
            .unwrap()
            .extend(first.unwrap().as_object().unwrap().clone());
        line["unstable"] = json!(unstable);
        line["ms"] = json!(runs);
        lines.push_str(&serde_json::to_string(&line).unwrap());
        lines.push('\n');
        if (done + 1) % 50 == 0 {
            eprintln!("rust: {}/{}", done + 1, ids.len());
        }
    }
    std::fs::write(out_file, lines).unwrap();
}
