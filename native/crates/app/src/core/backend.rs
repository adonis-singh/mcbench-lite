//! What a turn reaches outside the store: its provider, its tools and the
//! chat's sandbox ([`Backend`]); the app's own ([`AppBackend`]): the
//! providers of the data folder, the tools over the engines, the build
//! child, the platform's sandbox ([`PlatformSandbox`], made when setup's
//! shell step has its files) and the store's budget, made once setup has
//! the shipped data; and setup itself ([`run_setup`]).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use async_trait::async_trait;
use serde_json::{Map, Value};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

use mcbench_engine::Engine;
use mcbench_providers::Provider;
use mcbench_sandbox::{Sandbox, Workspace};
use mcbench_setup::{Data, Steps, Textures};
use mcbench_store::Store;
use mcbench_tools::engines::{Engines, registry};
use mcbench_tools::{Chat, Progress, Services, Tool, ToolError, ToolOutcome, Tools};

use super::providers::Providers;
use mcbench_headless::shell::PlatformSandbox;
use mcbench_tools::shots::SharedRenderer;

/// What a turn of the core reaches besides the store.
#[async_trait]
pub trait Backend: Send + Sync + 'static {
    /// `resolveProvider(key)`'s provider, or its refusal.
    fn provider(&self, key: &str) -> Result<Arc<dyn Provider>, String>;
    /// The tools a turn dispatches to.
    fn tools(&self) -> Tools;
    /// `sandbox.syncAttachments(chat)`.
    fn sync_attachments(&self, chat: &str) -> Result<(), String>;
    /// `sandbox.reset(chat)`.
    async fn reset_sandbox(&self, chat: &str) -> Result<(), String>;
    /// `sandbox.networkAccess()`.
    fn network_access(&self) -> bool;
    /// Ends every box, engine child, build child and CLI it started.
    async fn shutdown(&self);
}

/// The tool names `Tools::new` holds, in its order.
const TOOL_NAMES: [&str; 6] = [
    "build",
    "test_build",
    "see_build",
    "bash",
    "view_image",
    "prompt_library",
];

/// What setup gave the tools: the engines and the tools over them.
pub struct Prepared {
    /// The shipped data.
    pub data: Data,
    /// The baked textures.
    pub textures: Textures,
    /// The engine children, the settles and the build runner.
    pub engines: Engines,
    /// The tools' services.
    pub services: Arc<Services>,
    /// The tools (`see_build` draws with the live view's renderer).
    pub tools: Tools,
    /// Every block's icon (`mcbench_setup::icon`), made from the bake's
    /// atlas when first asked for.
    pub icons: mcbench_setup::icon::Icons,
}

/// The icons' maker over the shipped data and the bake
/// (`server/block-icon.js`): the atlas and its map, the particle texture
/// of each block, the block table's states and the tints.
fn icons(data: &Data, textures: &Textures) -> Result<mcbench_setup::icon::Icons, String> {
    let read = |name: &str| {
        let file = textures.file(name);
        std::fs::read(&file).map_err(|e| format!("{}: {e}", file.display()))
    };
    let states = mcbench_world::block_states::BlockStates::new(
        &data.text("block-table.tsv")?,
        &data.text("render-layers.tsv")?,
    )?;
    let tints = mcbench_setup::tints::BlockTints::new(&data.text("tints.tsv")?, &states)?;
    mcbench_setup::icon::Icons::new(
        &read("atlas.png")?,
        &read("atlas_map.json")?,
        &data.manifest()?.particles,
        states,
        tints,
    )
}

/// Where setup stands for the tools: nothing yet, ready, or failed with
/// its error.
pub type Readiness = Option<Result<Arc<Prepared>, String>>;

/// A tool that waits for setup, then runs as the prepared tools run it: a
/// turn started before setup is done waits for it, as today's first build
/// waits for the engine's install.
struct Deferred {
    name: &'static str,
    ready: watch::Receiver<Readiness>,
}

#[async_trait]
impl Tool for Deferred {
    fn name(&self) -> &'static str {
        self.name
    }

    async fn call(
        &self,
        chat: &mut Chat,
        arguments: &Map<String, Value>,
        progress: &mut (dyn FnMut(Progress) + Send),
        cancel: &CancellationToken,
    ) -> Result<ToolOutcome, ToolError> {
        let mut ready = self.ready.clone();
        let prepared = tokio::select! {
            prepared = ready.wait_for(Option::is_some) => prepared
                .map_err(|_| ToolError("The app stopped before setup was done".into()))?
                .clone()
                .expect("waited for Some"),
            _ = cancel.cancelled() => return Ok(ToolOutcome::result(
                serde_json::json!({"ok": false, "cancelled": true, "error": "Cancelled by user"}),
            )),
        };
        let prepared = prepared.map_err(ToolError)?;
        prepared
            .tools
            .execute(chat, self.name, arguments, progress, cancel)
            .await
    }
}

/// The in-process engine: Minecraft's tables are given once per process.
fn engine(tables: &Path) -> Result<Engine, String> {
    static ENGINE: OnceLock<Result<Engine, String>> = OnceLock::new();
    ENGINE.get_or_init(|| Engine::start(tables)).clone()
}

/// The app's backend.
pub struct AppBackend {
    /// The providers.
    pub providers: Arc<Providers>,
    store: Arc<Store>,
    sandbox: Arc<PlatformSandbox>,
    program: PathBuf,
    ready: watch::Sender<Readiness>,
    live: Mutex<Option<Engine>>,
    renderer: Arc<SharedRenderer>,
}

impl AppBackend {
    /// The backend of `store` with `providers`, `sandbox` (made when
    /// setup's shell step has its files), `program` (the app's binary) as
    /// the engine children and the build child, and `renderer`, the live
    /// view's, which `see_build` draws with.
    pub fn new(
        store: Arc<Store>,
        providers: Arc<Providers>,
        sandbox: Arc<PlatformSandbox>,
        program: PathBuf,
        renderer: Arc<SharedRenderer>,
    ) -> AppBackend {
        AppBackend {
            providers,
            store,
            sandbox,
            program,
            ready: watch::channel(None).0,
            live: Mutex::new(None),
            renderer,
        }
    }

    /// The sandbox.
    pub fn sandbox(&self) -> &Arc<PlatformSandbox> {
        &self.sandbox
    }

    /// The renderer `see_build` and a share's shots draw with.
    pub fn renderer(&self) -> &Arc<SharedRenderer> {
        &self.renderer
    }

    /// The app's binary.
    pub fn program(&self) -> &Path {
        &self.program
    }

    /// Watches setup's outcome for the tools.
    pub fn readiness(&self) -> watch::Receiver<Readiness> {
        self.ready.subscribe()
    }

    /// The in-process engine, once setup gave it the tables.
    pub fn live_engine(&self) -> Option<Engine> {
        *self.live.lock().expect("the live engine")
    }

    /// What setup prepared, once it has.
    pub fn prepared(&self) -> Option<Arc<Prepared>> {
        match &*self.ready.borrow() {
            Some(Ok(prepared)) => Some(Arc::clone(prepared)),
            _ => None,
        }
    }

    /// Minecraft's facts, once setup has the tables.
    pub fn facts(&self) -> Option<Arc<mcbench_world::facts::MinecraftFacts>> {
        self.prepared().map(|p| Arc::clone(&p.engines.facts))
    }

    /// Makes the tools from setup's data, on the runtime (the build runner
    /// starts its first child at once): the engines on the data's tables
    /// with `program` as the engine children and `mcbench-build-child`
    /// beside it as the build child (`mcbench_build::child_program`), the
    /// verifier archiving in `<data>/minecraft-verify`, the block registry
    /// of the shipped data, and the live view's renderer for `see_build`
    /// (a call before it has started waits for it).
    pub fn prepare(&self, data: Data, textures: Textures) {
        let made = (|| -> Result<Prepared, String> {
            let tables = data.folder().to_path_buf();
            let live = engine(&tables)?;
            *self.live.lock().expect("the live engine") = Some(live);
            let engines = Engines::start(
                &tables,
                self.program.clone(),
                mcbench_build::child_program()?,
            )?;
            let root = self.store.folder().root.clone();
            let verifier = engines.verifier(root.join("minecraft-verify"))?;
            let registry = Arc::new(registry(data.folder(), &root)?);
            let services = Services::on(
                &engines,
                Arc::clone(&self.store),
                Arc::clone(&self.sandbox) as Arc<dyn Sandbox>,
                verifier,
                registry,
                Arc::clone(&self.renderer),
            );
            let tools = Tools::new(Arc::clone(&services));
            let icons = icons(&data, &textures)?;
            Ok(Prepared {
                data,
                textures,
                engines,
                services,
                tools,
                icons,
            })
        })();
        self.ready.send_replace(Some(made.map(Arc::new)));
    }

    /// Setup failed: the tools answer with its error until a retry works.
    pub fn failed(&self, error: String) {
        self.ready.send_replace(Some(Err(error)));
    }

    /// Setup runs again (a retry): calls wait for it.
    pub fn retrying(&self) {
        self.ready.send_if_modified(|ready| {
            if matches!(ready, Some(Err(_))) {
                *ready = None;
                true
            } else {
                false
            }
        });
    }
}

#[async_trait]
impl Backend for AppBackend {
    fn provider(&self, key: &str) -> Result<Arc<dyn Provider>, String> {
        self.providers.provider(key)
    }

    fn tools(&self) -> Tools {
        Tools::of(
            TOOL_NAMES
                .iter()
                .map(|&name| {
                    Arc::new(Deferred {
                        name,
                        ready: self.ready.subscribe(),
                    }) as Arc<dyn Tool>
                })
                .collect(),
        )
    }

    fn sync_attachments(&self, chat: &str) -> Result<(), String> {
        let folder = self.store.folder();
        mcbench_sandbox::work::sync_attachments(
            &Workspace {
                chat: chat.to_string(),
                host: folder.root.join("sandbox").join(chat),
            },
            &folder.attachments(),
        )
    }

    async fn reset_sandbox(&self, chat: &str) -> Result<(), String> {
        self.sandbox.reset(chat).await;
        Ok(())
    }

    fn network_access(&self) -> bool {
        self.sandbox.network_access()
    }

    async fn shutdown(&self) {
        // The prepared tools go, so the build runner's spare child ends with
        // the last of them (it is killed on drop).
        let prepared = self
            .ready
            .send_replace(Some(Err("The app has stopped".into())));
        if let Some(Ok(prepared)) = prepared {
            prepared.services.verifier.stop().await;
            prepared.engines.settler.pool().stop().await;
        }
        self.sandbox.shutdown().await;
        self.providers.shutdown().await;
    }
}

/// Shipped data the core is given: made once, on the blocking pool.
pub type DataSource = Box<dyn Fn() -> Result<Data, String> + Send + Sync>;

/// Setup: `mcbench_setup::start` on the data folder with the shipped data,
/// the client and `shell`, the shell step's work
/// ([`PlatformSandbox::shell_step`]: the files `mcbench_setup::shell::files`
/// names, the platform's sandbox made of them and prepared); the shell goes
/// on in the background.
pub async fn run_setup(
    steps: &Arc<Steps>,
    folder: &Path,
    data: &Arc<DataSource>,
    client: &reqwest::Client,
    shell: mcbench_setup::Shell,
    cancel: &CancellationToken,
) -> Result<(Data, Textures), String> {
    let source = Arc::clone(data);
    let ready = mcbench_setup::start(
        Arc::clone(steps),
        folder.to_path_buf(),
        Box::new(move || source()),
        client.clone(),
        shell,
        cancel.clone(),
    )
    .await?;
    Ok((ready.data, ready.textures))
}
