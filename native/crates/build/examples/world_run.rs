//! The world-level differential, Rust side (`harness/world-run.js` is the
//! Node side and stores every case's worlds first): each case's stored
//! world loaded with [`World::from_manifest`], run through
//! [`BuildRunner::run`] with the platform sandbox, and a changed world
//! finalized and persisted with [`World::persist`]; `changes` names the
//! persisted world as `world-run.js` does.
//!
//! `cargo run --release --example world_run -- <mcbench-build-child> <corpus> <store> <out.jsonl> [id-substring]`

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use mcbench_build::{BuildHost, BuildRequest, BuildRunner, V8Runner, World};
use mcbench_world::WorldManifest;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;

const MAX_ERROR_CHARS: usize = 8000;

fn sha(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn load(dir: &Path, name: &str) -> World {
    let manifest =
        WorldManifest(std::fs::read_to_string(dir.join(format!("{name}.json"))).unwrap());
    World::from_manifest(&manifest, &mut |hash: &str| {
        std::fs::read(dir.join("chunks").join(format!("{hash}.json"))).map_err(|e| e.to_string())
    })
    .unwrap()
}

/// `path.resolve(root, rel)`, lexically.
fn resolve(root: &Path, rel: &str) -> PathBuf {
    let mut out = PathBuf::new();
    for part in root.join(rel).components() {
        match part {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            other => out.push(other),
        }
    }
    out
}

struct Host {
    meta: Value,
    read_root: Option<PathBuf>,
    settled: Option<World>,
}

#[async_trait]
impl BuildHost for Host {
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
                .map_err(|error| {
                    let code = match error.raw_os_error() {
                        Some(libc::ENOENT) => "ENOENT".to_string(),
                        Some(libc::EISDIR) => "EISDIR".to_string(),
                        Some(libc::ENOTDIR) => "ENOTDIR".to_string(),
                        _ => error.to_string(),
                    };
                    format!("mc.readFile: {rel}: {code}")
                });
        }
        let Some(answer) = self.meta["readFile"].get(rel) else {
            return Err(format!("mc.readFile: {rel}: no such file in /work"));
        };
        let text = answer["text"].as_str().unwrap().to_string();
        if answer["ok"].as_bool().unwrap() {
            Ok(text)
        } else {
            Err(text)
        }
    }

    async fn settled_world(&mut self) -> Result<World, String> {
        Ok(self
            .settled
            .as_ref()
            .expect("a settled case")
            .clone_world()
            .unwrap())
    }
}

fn cut(text: &str) -> String {
    if text.encode_utf16().count() <= MAX_ERROR_CHARS {
        return text.to_string();
    }
    let mut units = 0;
    let kept: String = text
        .chars()
        .take_while(|c| {
            units += c.len_utf16();
            units <= MAX_ERROR_CHARS
        })
        .collect();
    format!("{kept}\n[error message cut at {MAX_ERROR_CHARS} characters]")
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [program, corpus, store, out_file, rest @ ..] = args.as_slice() else {
        panic!(
            "usage: world_run <mcbench-build-child> <corpus> <store> <out.jsonl> [id-substring]"
        );
    };
    let filter = rest.first().cloned().unwrap_or_default();
    let (corpus, store) = (Path::new(corpus), Path::new(store));
    let runner = V8Runner::new(
        std::fs::canonicalize(program).unwrap(),
        mcbench_build::sandbox::platform().unwrap(),
    );
    let mut ids: Vec<String> = std::fs::read_dir(corpus.join("cases"))
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .filter(|id| id.contains(&filter))
        .collect();
    ids.sort();
    let mut registries: HashMap<String, Value> = HashMap::new();
    let mut lines = String::new();
    for (done, id) in ids.iter().enumerate() {
        let case_dir = corpus.join("cases").join(id);
        let meta: Value =
            serde_json::from_slice(&std::fs::read(case_dir.join("case.json")).unwrap()).unwrap();
        let registry_name = meta["registry"].as_str().unwrap().to_string();
        let registry = registries
            .entry(registry_name.clone())
            .or_insert_with(|| {
                serde_json::from_slice(
                    &std::fs::read(corpus.join("registries").join(&registry_name)).unwrap(),
                )
                .unwrap()
            })
            .clone();
        let dir = store.join(id);
        let before = load(&dir, "manifest");
        let settled = meta["settled"].is_object().then(|| {
            if meta["settled"]["world"].is_string() {
                load(&dir, "settled")
            } else {
                World::new()
            }
        });
        let request = BuildRequest {
            source: String::from_utf8(std::fs::read(case_dir.join("source.js")).unwrap()).unwrap(),
            registry,
            read_file: !meta["readFile"].is_null() || meta.get("readFileDir").is_some(),
            settled_ticks: meta["settled"]["ticks"].as_u64().map(|t| t as u32),
        };
        let read_root = meta.get("readFileDir").and_then(Value::as_str).map(|dir| {
            let moved = std::env::var("READFILE_ROOT").unwrap_or_else(|_| "=".into());
            let (from, to) = moved.split_once('=').unwrap();
            PathBuf::from(if from.is_empty() {
                dir.to_string()
            } else {
                dir.replacen(from, to, 1)
            })
        });
        let mut host = Host {
            meta: meta.clone(),
            read_root,
            settled,
        };
        tokio::time::sleep(Duration::from_millis(100)).await;
        let started = Instant::now();
        let result = match runner
            .run(&request, &before, &mut host, &CancellationToken::new())
            .await
        {
            Ok(mut outcome) => {
                outcome.world.finalize();
                let mut written: HashMap<String, Vec<u8>> = HashMap::new();
                let manifest = outcome
                    .world
                    .persist(&mut |hash: &str, bytes: &[u8]| {
                        written.insert(hash.to_string(), bytes.to_vec());
                        Ok(())
                    })
                    .unwrap();
                let parsed: Value = serde_json::from_str(&manifest.0).unwrap();
                let chunks: Vec<Vec<u8>> = parsed["chunks"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|entry| {
                        let hash = entry[1].as_str().unwrap();
                        written.get(hash).cloned().unwrap_or_else(|| {
                            std::fs::read(dir.join("chunks").join(format!("{hash}.json"))).unwrap()
                        })
                    })
                    .collect();
                json!({
                    "ok": true,
                    "changes": format!("{}:{}", sha(manifest.0.as_bytes()), sha(&chunks.join(&b'\n'))),
                    "logs": outcome.logs,
                    "files": outcome.files.iter().map(|f| json!({ "path": f.path, "length": f.text.encode_utf16().count(), "sha": sha(f.text.as_bytes()) })).collect::<Vec<_>>(),
                    "touched": outcome.touched,
                })
            }
            Err(failure) => json!({
                "ok": false,
                "error": cut(&failure.message),
                "logs": if failure.logs.is_empty() { Value::Null } else { json!(failure.logs) },
            }),
        };
        let ms = started.elapsed().as_secs_f64() * 1e3;
        let mut line = json!({ "id": id });
        line.as_object_mut()
            .unwrap()
            .extend(result.as_object().unwrap().clone());
        line["unstable"] = json!(false);
        line["ms"] = json!([ms]);
        lines.push_str(&line.to_string());
        lines.push('\n');
        if (done + 1) % 50 == 0 {
            eprintln!("rust: {}/{}", done + 1, ids.len());
        }
    }
    std::fs::write(out_file, lines).unwrap();
}
