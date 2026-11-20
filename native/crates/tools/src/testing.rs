//! What the replays (`examples/replay.rs`, `mcbench-agent`'s whole-turn
//! replay) and the ported tests share: the app's services on a data folder
//! with the in-process engine (its settles and its verifier), the real
//! build child, and the one fake the replay's Node side uses too
//! (`tests/node/replay.js`): a sandbox that answers a recorded run and
//! reads a `view_image` file's own bytes in place of Pillow's normalising.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use mcbench_sandbox::{BashCommand, BashRun, Sandbox, SandboxError, Workspace};
use serde_json::{Map, Value, json};
use tokio_util::sync::CancellationToken;

use crate::verify::VerifierPool;
use crate::{Services, World};

/// The record a fake answers from: `{ exec }` as `tests/node/prepare.js`
/// writes it (`{ formatted, storage }` or `{ throw }`).
pub type Record = Arc<Mutex<Value>>;

/// The sandbox fake: `exec` answers the record's `exec` and notes the
/// command it was given; `view_image` answers the file's bytes.
pub struct RecordedSandbox {
    /// The current call's record.
    pub record: Record,
    /// The last command: `{ command, timeoutMs (clamped), network }`.
    pub exec: Mutex<Option<Value>>,
}

impl RecordedSandbox {
    /// A fake with an empty record.
    pub fn new() -> RecordedSandbox {
        RecordedSandbox {
            record: Arc::new(Mutex::new(json!({}))),
            exec: Mutex::new(None),
        }
    }
}

impl Default for RecordedSandbox {
    fn default() -> Self {
        RecordedSandbox::new()
    }
}

#[async_trait]
impl Sandbox for RecordedSandbox {
    async fn prepare(&self) -> Result<(), SandboxError> {
        Ok(())
    }

    async fn exec(
        &self,
        _: &Workspace,
        command: &BashCommand,
        _: &CancellationToken,
    ) -> Result<BashRun, SandboxError> {
        *self.exec.lock().unwrap() = Some(json!({
            "command": command.command,
            "timeoutMs": command.clamped(),
            "network": command.network,
        }));
        let record = self.record.lock().unwrap();
        let answer = &record["exec"];
        if let Some(message) = answer.get("throw") {
            return Err(SandboxError::Failed(
                message.as_str().unwrap_or("").to_string(),
            ));
        }
        Ok(BashRun {
            exit_code: 0,
            formatted: answer["formatted"].as_str().unwrap_or("").to_string(),
            storage: answer["storage"].as_str().map(str::to_string),
        })
    }

    async fn view_image(&self, workspace: &Workspace, path: &str) -> Result<Vec<u8>, SandboxError> {
        let file = mcbench_sandbox::work::resolve_work_path(workspace, path, false)
            .map_err(SandboxError::Failed)?;
        if !file.exists() {
            return Err(SandboxError::Failed(format!(
                "No such file in /work: {path}"
            )));
        }
        // `fs.readFileSync`'s words for a folder, as the Node fake reads it.
        if file.is_dir() {
            return Err(SandboxError::Failed(
                "EISDIR: illegal operation on a directory, read".into(),
            ));
        }
        std::fs::read(&file).map_err(|error| SandboxError::Failed(error.to_string()))
    }

    async fn remove_work_files(
        &self,
        workspace: &Workspace,
        paths: &[String],
    ) -> Result<(), SandboxError> {
        for path in paths {
            let _ = std::fs::remove_file(workspace.host.join(path));
        }
        Ok(())
    }

    async fn reset(&self, _: &str) {}

    async fn destroy(&self, _: &Workspace) -> Result<(), SandboxError> {
        Ok(())
    }

    async fn shutdown(&self) {}

    fn network_access(&self) -> bool {
        true
    }

    fn disable_network(&self) {}
}

/// What every test of a process shares ([`crate::engines::Engines`]).
pub type Shared = crate::engines::Engines;

/// [`crate::engines::Engines::start`] with the engine children of the
/// built `mcbench-lite` that `MCBENCH_ENGINE_PROGRAM` names and the build
/// child `MCBENCH_BUILD_CHILD` names (`mcbench_build::child_program`, which
/// says the app's files are incomplete when it is not there).
pub fn shared(tables: &Path) -> Result<Shared, String> {
    let engine = std::env::var_os("MCBENCH_ENGINE_PROGRAM")
        .map(PathBuf::from)
        .ok_or("set MCBENCH_ENGINE_PROGRAM to a built mcbench-lite (the engine children)")?;
    crate::engines::Engines::start(tables, engine, mcbench_build::child_program()?)
}

/// A block registry as `blocks.json` has it (`name: [properties, defaults]`),
/// made from the engine's table, for tests without the app's assets.
pub fn registry_from_table(tables: &Path) -> Result<Value, String> {
    let facts = mcbench_world::facts::MinecraftFacts::new(
        &std::fs::read_to_string(tables.join("blocks.txt")).map_err(|e| e.to_string())?,
    )?;
    let mut registry = Map::new();
    for (id, block) in facts.blocks() {
        let name = id.strip_prefix("minecraft:").unwrap_or(id);
        if name == "air" {
            continue;
        }
        let mut properties = Map::new();
        let mut defaults = Map::new();
        for (property, values, default) in block {
            properties.insert(property.to_string(), json!(values));
            defaults.insert(property.to_string(), json!(default));
        }
        registry.insert(name.to_string(), json!([properties, defaults]));
    }
    Ok(Value::Object(registry))
}

/// The services of one data folder over `shared`, with `limits`, a sandbox,
/// a verifier and a registry.
pub fn services(
    shared: &Shared,
    data: &Path,
    limits: mcbench_store::Limits,
    sandbox: Arc<dyn Sandbox>,
    verifier: Arc<VerifierPool>,
    registry: Arc<Value>,
) -> Result<Arc<Services>, String> {
    let folder = mcbench_store::DataFolder {
        root: data.to_path_buf(),
    };
    let host: Arc<dyn mcbench_store::Host> = Arc::new(mcbench_store::SystemHost);
    let store = Arc::new(
        mcbench_store::Store::open(
            &folder,
            mcbench_store::Options {
                host: host.clone(),
                limits,
                chat_removed: Box::new(|_| {}),
            },
        )
        .map_err(|e| e.message)?,
    );
    Ok(services_on(shared, store, sandbox, verifier, registry))
}

/// The services over an open `store`, with a renderer not yet started
/// (`services.renderer.set` starts it).
pub fn services_on(
    shared: &Shared,
    store: Arc<mcbench_store::Store>,
    sandbox: Arc<dyn Sandbox>,
    verifier: Arc<VerifierPool>,
    registry: Arc<Value>,
) -> Arc<Services> {
    Services::on(
        shared,
        store,
        sandbox,
        verifier,
        registry,
        crate::shots::SharedRenderer::new(),
    )
}

pub use crate::engines::registry_of;

/// The replay's services: data folder `data` with the operator's limits
/// from the environment, the recorded sandbox, the engine verifier and the
/// data folder's own `blocks.json`.
pub struct Harness {
    /// The services.
    pub services: Arc<Services>,
    /// The fake's record.
    pub record: Record,
    /// The sandbox fake.
    pub sandbox: Arc<RecordedSandbox>,
}

/// Builds a [`Harness`] (inside a tokio runtime).
pub fn harness(data: &Path, tables: &Path) -> Result<Harness, String> {
    let shared = shared(tables)?;
    let sandbox = Arc::new(RecordedSandbox::new());
    let record = sandbox.record.clone();
    let limits = mcbench_store::Limits::from_env(|name| std::env::var(name).ok())?;
    let verifier = shared.verifier(data.join("minecraft-verify"))?;
    let services = services(
        &shared,
        data,
        limits,
        sandbox.clone(),
        verifier,
        Arc::new(registry_of(data)?),
    )?;
    Ok(Harness {
        services,
        record,
        sandbox,
    })
}

/// A world read from a manifest and a folder of chunk files.
pub fn world_of(manifest: &str, chunks: &Path) -> Result<World, String> {
    World::from_manifest(
        &mcbench_world::WorldManifest(manifest.to_string()),
        &mut |hash: &str| {
            std::fs::read(chunks.join(format!("{hash}.json")))
                .map_err(|e| format!("chunk {hash}: {e}"))
        },
    )
}

/// `JSON.stringify(world.serializeManifest(...))` without writing a file.
pub fn manifest_of(world: &World) -> String {
    world
        .persist(&mut |_: &str, _: &[u8]| Ok(()))
        .expect("nothing to write fails")
        .0
}
