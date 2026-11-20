//! What the differential tests share: the environment they need, clones of
//! the fixture, Node runs and a store on a clone.
//!
//! `MCBENCH_DIFF_DATA=<schema-14 data folder>` (cloned, never opened),
//! `MCBENCH_NODE_DIFF=<checkout with server/ and node_modules/>`,
//! `MCBENCH_DIFF_SCRATCH=<folder for the clones>`; `MCBENCH_NODE` names the
//! node binary (default `node`). Unset, the tests are ignored; set, anything
//! missing fails them.

#![allow(dead_code)]

#[path = "fixture.rs"]
pub mod fixture;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use mcbench_store::{DataFolder, Limits, Options, Store, SystemHost};

pub struct Env {
    pub fixture: PathBuf,
    pub checkout: PathBuf,
    pub scratch: PathBuf,
    pub scripts: PathBuf,
}

pub fn env() -> Env {
    let var = |name: &str| std::env::var(name).unwrap_or_else(|_| panic!("{name} must be set"));
    let env = Env {
        fixture: PathBuf::from(var("MCBENCH_DIFF_DATA")),
        checkout: PathBuf::from(var("MCBENCH_NODE_DIFF")),
        scratch: PathBuf::from(var("MCBENCH_DIFF_SCRATCH")),
        scripts: Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("node"),
    };
    fixture::guard(&env.fixture);
    assert!(
        env.fixture.join("mcbench.sqlite3").exists(),
        "no database in {}",
        env.fixture.display()
    );
    assert!(
        env.checkout.join("server").join("agent.js").exists()
            && env.checkout.join("node_modules").exists(),
        "no server/agent.js and node_modules/ in {}",
        env.checkout.display()
    );
    std::fs::create_dir_all(&env.scratch).unwrap();
    env
}

/// A copy of `from` at `scratch/name` (copy-on-write where the disk can),
/// without `fixture::LEFT_OUT`.
pub fn clone(env: &Env, from: &Path, name: &str) -> PathBuf {
    let to = env.scratch.join(name);
    fixture::clone(from, &to);
    to
}

/// Runs `script` from `tests/node/` with `data` as the data folder.
pub fn node(env: &Env, data: &Path, script: &str, args: &[&str], vars: &[(&str, &str)]) {
    let mut command =
        Command::new(std::env::var("MCBENCH_NODE").unwrap_or_else(|_| "node".to_string()));
    command
        .arg("-r")
        .arg(env.scripts.join("det.js"))
        .arg(env.scripts.join(script))
        .args(args)
        .env("MCBENCH_NODE_DIFF", &env.checkout)
        .env("MCBENCH_DATA_DIR", data)
        .env("MCBENCH_SETTINGS_FILE", data.join("settings.json"))
        .env("MCBENCH_TOOLS_DIR", env.scratch.join("tools-empty"))
        .env_remove("MCBENCH_CONTEXT_MAX_BYTES");
    for (key, value) in vars {
        command.env(key, value);
    }
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "node {script}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// `det.js`'s machine (`mcbench-store`'s tests' `DetHost`): the clock is
/// fixed, ids and `randomInt` count up, background work does not run.
pub struct DetHost {
    pub now: std::sync::atomic::AtomicI64,
    ids: std::sync::atomic::AtomicU64,
    ints: std::sync::atomic::AtomicU64,
}

/// `det.js`'s `MCBENCH_DIFF_NOW` default.
pub const DET_NOW: i64 = 1794873600000;

impl DetHost {
    pub fn new(now: i64) -> Arc<DetHost> {
        Arc::new(DetHost {
            now: now.into(),
            ids: 0.into(),
            ints: 0.into(),
        })
    }
}

impl mcbench_store::Host for DetHost {
    fn now_ms(&self) -> i64 {
        self.now.load(std::sync::atomic::Ordering::SeqCst)
    }
    fn uuid(&self) -> String {
        let n = self.ids.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
        format!("{n:08x}-0000-4000-8000-{n:012x}")
    }
    fn random_int(&self, below: u32) -> u32 {
        let n = self.ints.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
        ((n * 7919) % u64::from(below)) as u32
    }
    fn free_bytes(&self, folder: &Path) -> Result<u64, String> {
        mcbench_store::Host::free_bytes(&SystemHost, folder)
    }
    fn background(&self, _job: Box<dyn FnOnce() + Send>) {}
}

pub fn open(data: &Path) -> Arc<Store> {
    open_with(data, Arc::new(SystemHost))
}

pub fn open_with(data: &Path, host: Arc<dyn mcbench_store::Host>) -> Arc<Store> {
    Arc::new(
        Store::open(
            &DataFolder {
                root: data.to_path_buf(),
            },
            Options {
                host,
                limits: Limits::from_env(|_| None).unwrap(),
                chat_removed: Box::new(|_| {}),
            },
        )
        .unwrap(),
    )
}

pub fn sha256(text: &str) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(text.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Every file under `dir`, relative, with its sha256, sorted.
pub fn files(dir: &Path) -> Vec<(String, String)> {
    let mut out = Vec::new();
    fn walk(root: &Path, at: &Path, out: &mut Vec<(String, String)>) {
        let Ok(entries) = std::fs::read_dir(at) else {
            return;
        };
        for entry in entries {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(root, &path, out);
            } else {
                let bytes = std::fs::read(&path).unwrap();
                use sha2::Digest;
                let sha: String = sha2::Sha256::digest(&bytes)
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect();
                out.push((path.strip_prefix(root).unwrap().display().to_string(), sha));
            }
        }
    }
    walk(dir, dir, &mut out);
    out.sort();
    out
}
