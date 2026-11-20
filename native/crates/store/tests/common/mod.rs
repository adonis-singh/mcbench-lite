//! What the store's tests share: a deterministic host (the same ids, clock
//! and random numbers as `tests/node/det.js`), and opening a store.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};

use mcbench_store::{DataFolder, Host, Limits, Options, Store, SystemHost};

/// `det.js`'s machine: `Date.now()` is what the test sets, ids count up,
/// `randomInt` counts up, background measurements do not run.
pub struct DetHost {
    pub now: AtomicI64,
    ids: AtomicU64,
    ints: AtomicU64,
}

impl DetHost {
    pub fn new(now: i64) -> Arc<DetHost> {
        Arc::new(DetHost {
            now: AtomicI64::new(now),
            ids: AtomicU64::new(0),
            ints: AtomicU64::new(0),
        })
    }

    pub fn set_now(&self, ms: i64) {
        self.now.store(ms, Ordering::SeqCst);
    }
}

impl Host for DetHost {
    fn now_ms(&self) -> i64 {
        self.now.load(Ordering::SeqCst)
    }
    fn uuid(&self) -> String {
        let n = self.ids.fetch_add(1, Ordering::SeqCst) + 1;
        format!("{n:08x}-0000-4000-8000-{n:012x}")
    }
    fn random_int(&self, below: u32) -> u32 {
        let n = self.ints.fetch_add(1, Ordering::SeqCst) + 1;
        ((n * 7919) % u64::from(below)) as u32
    }
    fn free_bytes(&self, folder: &Path) -> Result<u64, String> {
        SystemHost.free_bytes(folder)
    }
    fn background(&self, _job: Box<dyn FnOnce() + Send>) {}
}

/// A host whose disk has `free` bytes, otherwise the system's.
pub struct SmallDisk(pub std::sync::Mutex<Option<u64>>);

impl Host for SmallDisk {
    fn now_ms(&self) -> i64 {
        SystemHost.now_ms()
    }
    fn uuid(&self) -> String {
        SystemHost.uuid()
    }
    fn random_int(&self, below: u32) -> u32 {
        SystemHost.random_int(below)
    }
    fn free_bytes(&self, folder: &Path) -> Result<u64, String> {
        match *self.0.lock().unwrap() {
            Some(free) => Ok(free),
            None => SystemHost.free_bytes(folder),
        }
    }
    fn background(&self, job: Box<dyn FnOnce() + Send>) {
        std::thread::spawn(job);
    }
}

/// Removes `harness/<id>`, what `harness.destroy` does on disk.
pub fn harness_removed(root: PathBuf) -> Box<dyn Fn(&str) + Send + Sync> {
    Box::new(move |id| {
        let folder = root.join("harness").join(id);
        if folder.exists() {
            std::fs::remove_dir_all(folder).unwrap();
        }
    })
}

pub fn open(
    root: &Path,
    host: Arc<dyn Host>,
    limits: Limits,
) -> Result<Store, mcbench_store::Error> {
    Store::open(
        &DataFolder {
            root: root.to_path_buf(),
        },
        Options {
            host,
            limits,
            chat_removed: harness_removed(root.to_path_buf()),
        },
    )
}

pub fn limits(chat_max_bytes: u64) -> Limits {
    Limits {
        chat_max_bytes,
        disk_reserve_bytes: 2 * 1024 * 1024 * 1024,
        reports_kept: 100,
    }
}

/// Blocks for a test's world, made into a `mcbench_world::World` by
/// [`TestWorld::into_chat`] (a new world, each block set, finalized).
#[derive(Clone, Debug, Default)]
pub struct TestWorld {
    pub blocks: std::collections::BTreeMap<(i32, i32, i32), String>,
}

impl TestWorld {
    pub fn with(&self, x: i32, y: i32, z: i32, id: &str) -> TestWorld {
        let mut next = self.clone();
        next.blocks.insert((x, y, z), id.to_string());
        next
    }

    pub fn into_chat(self) -> mcbench_world::World {
        let mut world = mcbench_world::World::new();
        for ((x, y, z), id) in self.blocks {
            world
                .set([x, y, z], mcbench_world::BlockState::new(&id, &[]))
                .unwrap();
        }
        world.finalize();
        world
    }
}

pub fn sha(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let digest = <sha2::Sha256 as sha2::Digest>::digest(bytes);
    digest.iter().fold(String::new(), |mut out, b| {
        write!(out, "{b:02x}").unwrap();
        out
    })
}

/// The block at `(x, y, z)` of a world (the folder and chat are those its
/// chunk files were read from).
pub fn block_at(
    _root: &Path,
    _chat: &str,
    world: &mcbench_world::World,
    x: i32,
    y: i32,
    z: i32,
) -> Option<String> {
    world.get([x, y, z]).map(|state| state.id.clone())
}

/// A fresh scratch folder under the system's temporary folder.
pub fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "mcbench-store-{name}-{}-{}",
        std::process::id(),
        SystemHost.uuid()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
