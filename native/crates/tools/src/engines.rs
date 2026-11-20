//! What the tools of a process share whatever the data folder: the engine
//! children (`mcbench_engine::EngineProgram`: `<binary> headless engine
//! <tables>`) behind the settle pool (`SettlePool`, `pool_size`) with the
//! settles kept ([`crate::settle::Settler`]) and behind the verifier
//! (`VerifyEngines`), the build child (`mcbench_build::V8Runner` over
//! `mcbench_build::sandbox::platform`), Minecraft's facts and the
//! verifier's budgets; and the [`Services`] of a data folder over them.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use mcbench_engine::{EngineProgram, SettlePool, VerifyEngines, pool_size};
use serde_json::Value;

use crate::Services;
use crate::verify::{
    Budgets, Encoding, Limits, Minecraft, Placement, Settings, VerifierPool, total_memory,
};

/// The process's engines and build child (see the module's doc).
pub struct Engines {
    /// How an engine child starts.
    pub program: EngineProgram,
    /// The settles.
    pub settler: Arc<crate::settle::Settler>,
    /// The build runner.
    pub runner: Arc<dyn mcbench_build::BuildRunner>,
    /// Minecraft's facts.
    pub facts: Arc<mcbench_world::facts::MinecraftFacts>,
    /// The verifier's budgets (`BUDGETS`, `MCBENCH_VERIFY_MAX_*`).
    pub budgets: Budgets,
}

impl Engines {
    /// The engines of `engine` (a built `mcbench-lite`: the app's own
    /// binary) on Minecraft's tables at `tables`, and the build child
    /// `child` (`mcbench-build-child`, as `mcbench_build::child_program`
    /// names it), inside a tokio
    /// runtime (the V8 runner starts its first child at once).
    pub fn start(tables: &Path, engine: PathBuf, child: PathBuf) -> Result<Engines, String> {
        let program = EngineProgram::new(engine, tables);
        let budgets = Budgets::from_env()?;
        let env: std::collections::HashMap<String, String> = std::env::vars().collect();
        let pool = SettlePool::new(
            program.clone(),
            pool_size(&env)?,
            std::time::Duration::from_millis(budgets.wall_ms),
        );
        let facts = Arc::new(mcbench_world::facts::MinecraftFacts::new(
            &std::fs::read_to_string(tables.join("blocks.txt"))
                .map_err(|e| format!("{}: {e}", tables.join("blocks.txt").display()))?,
        )?);
        let settler = Arc::new(crate::settle::Settler::new(
            pool,
            facts.clone(),
            mcbench_world::dimension::y_offset(),
            budgets.simulation_ticks,
        ));
        let runner = Arc::new(mcbench_build::V8Runner::new(
            child,
            mcbench_build::sandbox::platform()?,
        ));
        Ok(Engines {
            program,
            settler,
            runner,
            facts,
            budgets,
        })
    }

    /// The app's engine verifier (`minecraftVerifier`: a `VerifierPool` of
    /// `EngineVerifier`s on engine children, `updates` placement, `full`
    /// reports) archiving under `root` (the data folder's
    /// `minecraft-verify`).
    pub fn verifier(&self, root: PathBuf) -> Result<Arc<VerifierPool>, String> {
        let engines = Arc::new(VerifyEngines::new(self.program.clone()));
        let minecraft = Minecraft::new(engines, self.program.tables())?;
        let settings = Settings {
            budgets: Limits::from(self.budgets),
            placement: Placement::Updates,
            encoding: Encoding::Full,
        };
        Ok(Arc::new(VerifierPool::new(
            root,
            minecraft,
            settings,
            Box::new(total_memory),
        )))
    }
}

/// The block registry build programs are checked against: the shipped
/// data's `blocks.json` in Minecraft's tables folder `tables` (today's asset,
/// byte for byte; what setup unpacks into `<data folder>/minecraft/<id>`),
/// or the data folder `data`'s `assets/1.21.10/blocks.json` of a folder
/// today's app made. The app and `headless turn` both read it here.
pub fn registry(tables: &Path, data: &Path) -> Result<Value, String> {
    let shipped = tables.join("blocks.json");
    if shipped.is_file() {
        return serde_json::from_str(
            &std::fs::read_to_string(&shipped)
                .map_err(|e| format!("{}: {e}", shipped.display()))?,
        )
        .map_err(|e| format!("{}: {e}", shipped.display()));
    }
    registry_of(data)
}

/// The data folder's `blocks.json` (the app's block registry asset,
/// `assets.js` `getBlockRegistry`).
pub fn registry_of(data: &Path) -> Result<Value, String> {
    let file = data.join("assets/1.21.10/blocks.json");
    serde_json::from_str(
        &std::fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?,
    )
    .map_err(|e| format!("{}: {e}", file.display()))
}

impl Services {
    /// The services of an open `store` over `engines`, with `sandbox`, the
    /// verifier, the block registry and the process's renderer.
    pub fn on(
        engines: &Engines,
        store: Arc<mcbench_store::Store>,
        sandbox: Arc<dyn mcbench_sandbox::Sandbox>,
        verifier: Arc<VerifierPool>,
        registry: Arc<Value>,
        renderer: Arc<crate::shots::SharedRenderer>,
    ) -> Arc<Services> {
        let prompts = Arc::new(mcbench_store::PromptLibrary::new(
            store.folder().prompts(),
            Arc::clone(store.host()),
        ));
        Arc::new(Services {
            store,
            prompts,
            settler: engines.settler.clone(),
            runner: engines.runner.clone(),
            sandbox,
            registry,
            facts: engines.facts.clone(),
            budgets: engines.budgets,
            verifier,
            renderer,
        })
    }
}
