//! The core: the implementation of `mcbench_ui::Core` the window talks to,
//! which replaces `server/index.js` (its routes and NDJSON streams) with
//! commands handed over in-process and updates published to the window.
//!
//! [`Core`] owns, for one data folder: the tokio runtime (multi-thread,
//! [`threads::workers`] workers, every thread named `mcbench-core`), the
//! one-instance lock (today's server's `server.pid`, so the app and today's
//! localhost app refuse each other a data folder), the store, the providers ([`providers::Providers`]),
//! the turn lifecycle (`mcbench_agent::lifecycle::Turns`, with this
//! module's [`CoreServices`] as its services), setup (started at once in
//! the background, each change of its steps published) and, once setup has
//! the shipped data, the tools over the engines and the build child and the
//! live world of the view the window shows (`live/`). `see_build` and the live view's
//! frames are drawn by ONE renderer, [`renderer`], started once.
//!
//! `Core::send` returns at once; each command runs as a task on the
//! runtime ([`Core::command`], `commands.rs`, which states the update each
//! command ends in) and everything it gives reaches the window as
//! `Update`s through `Window::publish`: a turn's events through one sink
//! per turn, in the order `turns.rs` states (the join of a running turn
//! placed between two of its events), the transcript when a chat is opened,
//! when its turn starts and when it ends, the chat list whenever it changes
//! (coalesced, as each change today writes the whole list to every
//! watcher), setup's steps on each change, a refused send as
//! `Update::SendRefused`, other refusals as `Update::Error`, each change of
//! a sign-in or a CLI install as `Update::SignIn` or `Update::Install` (the
//! core watches them, `routes/logins.rs`; nothing polls). After attach
//! the core publishes the lists, the providers and the favourites. The
//! window's preferences are `preferences.json` ([`preferences`]). A panic
//! on any thread of the runtime becomes ONE `Update::Fatal` with today's
//! fatal line, after which the core takes no command and stops everything
//! it started ([`threads`]).
//!
//! The routes that have no `Command` yet are typed methods on [`Core`],
//! grouped by area in `routes/`: each is an `async fn` returning its typed
//! result or today's refusal (`Refusal`: the status and the sentence the
//! route answers), so the interface can add a `Command` per feature that
//! calls one. `native/crates/app/ROUTES.md` maps every route of
//! `server/index.js` to its function.

pub mod backend;
mod bench;
mod commands;
pub use mcbench_headless::providers;
pub mod live;
pub mod preferences;
pub mod renderer;
pub mod routes;
pub mod settings;
pub mod share;
pub mod threads;
mod turns;
pub mod views;
mod wsl;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::Duration;

use async_trait::async_trait;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

use mcbench_agent::TurnSink;
use mcbench_agent::context;
use mcbench_agent::contract::Placement;
use mcbench_agent::lifecycle::{Refusal, Services, TurnConfig, Turns};
use mcbench_providers::Provider;
use mcbench_providers::login::Login;
use mcbench_setup::{InstanceLock, Step, Steps};
use mcbench_store::json::Json;
use mcbench_store::{AttachmentFile, Host, Limits, SettingsFile, Store};
use mcbench_tools::Tools;
use mcbench_ui::{Command, Preferences, Update, Window};

use backend::{AppBackend, Backend, DataSource};
use mcbench_headless::shell::PlatformSandbox;
use providers::Providers;

/// How long a fatal failure's shutdown may take before the core gives up
/// on it (`FATAL_SHUTDOWN_MS`).
pub const FATAL_SHUTDOWN: Duration = Duration::from_millis(10_000);

/// The purge runs at least this often (`PURGE_EVERY_MS`).
pub const PURGE_EVERY: Duration = Duration::from_millis(24 * 60 * 60 * 1000);

/// A refusal with its status.
pub fn refusal(status: u16, message: impl Into<String>) -> Refusal {
    Refusal {
        status,
        message: message.into(),
    }
}

/// What a core is made of.
pub struct Parts {
    /// The data folder (`mcbench_setup::data_folder`).
    pub data: PathBuf,
    /// The app's own binary: the engine children (`headless engine`), the
    /// CLIs' tool bridge (`tool-bridge`) and the sandbox's helper
    /// (`sandbox-agent`). The build child is its own program,
    /// `mcbench-build-child` (`mcbench_build::child_program`).
    pub program: PathBuf,
    /// The clock, ids and the disk (`mcbench_store::SystemHost`).
    pub host: Arc<dyn Host>,
    /// The shipped data (`Data::shipped` in a release build).
    pub shipped: DataSource,
    /// The client setup downloads with (`mcbench_setup::fetch::client`).
    pub client: reqwest::Client,
    /// A test's stand-in for the app's providers, tools and sandbox; `None`
    /// is the app's own ([`AppBackend`]).
    pub backend: Option<Arc<dyn Backend>>,
    /// A test's stand-in for a CLI install's download and unpacking; `None`
    /// is the pinned download (`mcbench_providers::harness::install::install`).
    pub installer: Option<mcbench_providers::harness::install::Installer>,
    /// A test's stand-in for sharing's options and BlueMap's jar; `None`
    /// is the app's own ([`share::options`]: the share folder of the data
    /// folder, git and gh as processes, the app's export and viewer, the
    /// pinned jar).
    pub share: Option<share::Stand>,
    /// The live world's clock and renderer ([`live::LiveParts`]; its
    /// `Default` is the app's: the runtime's clock, no drawing crate).
    pub live: live::LiveParts,
}

/// The window once attached, and the updates published before it was.
type WindowSlot = (Option<Arc<dyn Window>>, Vec<Update>);

pub(crate) struct Inner {
    pub(crate) handle: tokio::runtime::Handle,
    runtime: Mutex<Option<tokio::runtime::Runtime>>,
    core_id: u64,
    window: Mutex<WindowSlot>,
    pub(crate) stopped: AtomicBool,
    stopping: CancellationToken,
    pub(crate) store: Arc<Store>,
    pub(crate) turns: Turns,
    pub(crate) providers: Arc<Providers>,
    pub(crate) backend: Arc<dyn Backend>,
    pub(crate) app: Option<Arc<AppBackend>>,
    pub(crate) steps: Arc<Steps>,
    setup: Mutex<Option<SetupRun>>,
    pub(crate) sign_ins: mcbench_providers::login::SignIns,
    pub(crate) installs: mcbench_providers::harness::install::CliInstaller,
    pub(crate) versions: settings::Versions,
    /// How many changes of each login's sign-in were published: a command
    /// whose call published none publishes the state itself (`logins.rs`).
    pub(crate) sign_ins_told: Mutex<std::collections::HashMap<Login, u64>>,
    /// The same for each CLI's install.
    pub(crate) installs_told: Mutex<std::collections::HashMap<String, u64>>,
    list_changed: Arc<Notify>,
    pub(crate) live: live::Live,
    started: tokio::sync::watch::Sender<bool>,
    /// The archive was read (`Command::ReadArchive`) since the core
    /// started: a change of it publishes it again.
    archive_read: AtomicBool,
    /// What the core keeps of the turns the window sees (`turns.rs`).
    turn_views: Mutex<turns::TurnViews>,
    /// Held while a list is read and published, and while a turn's end is
    /// published (`turns.rs`).
    list_order: Mutex<()>,
    /// The benches whose details are published on change, with the
    /// details last published (`bench.rs`).
    benches_shown: Mutex<bench::Shown>,
    /// The prompt requests under way (`Command::GeneratePrompt`), by the
    /// window's number.
    generating: Mutex<std::collections::HashMap<u64, CancellationToken>>,
    /// Told each time a prompt request ends.
    prompt_ended: tokio::sync::watch::Sender<()>,
    /// The data folder's one-instance lock (`server.pid`), let go at the
    /// end of [`Core::shutdown`], as today's `stop` ends with
    /// `releaseDataFolder()`: the app's process exits without dropping the
    /// core, so a lock released only by a drop stayed behind after every
    /// quit, for the next start to judge by whether its pid was alive.
    lock: Mutex<Option<InstanceLock>>,
    /// The shared builds and the publishes under way ([`share`]).
    pub(crate) sharing: share::Sharing,
}

struct SetupRun {
    data: Arc<DataSource>,
    client: reqwest::Client,
    running: bool,
}

/// The core of one data folder (see the module's doc).
#[derive(Clone)]
pub struct Core(pub(crate) Arc<Inner>);

/// The lifecycle's services: the backend's provider, tools and sandbox,
/// the title model from the settings, the chat list published on each
/// change, and the window's sink for a turn no request started.
pub struct CoreServices {
    backend: Arc<dyn Backend>,
    settings: Arc<SettingsFile>,
    list_changed: Arc<Notify>,
    core: Arc<OnceLock<Weak<Inner>>>,
}

#[async_trait]
impl Services for CoreServices {
    fn provider(&self, key: &str) -> Result<Arc<dyn Provider>, String> {
        self.backend.provider(key)
    }
    fn tools(&self) -> Tools {
        self.backend.tools()
    }
    fn title_model(&self) -> Option<(String, String, Option<String>)> {
        let settings = self.settings.get();
        let title = settings.title_model();
        let text = |key: &str| title.get(key).and_then(Json::as_str).map(str::to_string);
        Some((text("provider")?, text("model")?, text("effort")))
    }
    fn sync_attachments(&self, chat: &str) -> Result<(), String> {
        self.backend.sync_attachments(chat)
    }
    async fn reset_sandbox(&self, chat: &str) -> Result<(), String> {
        self.backend.reset_sandbox(chat).await
    }
    fn conversations_changed(&self) {
        self.list_changed.notify_one();
    }
    fn turn_sink(&self, chat: &str) -> Option<Box<dyn TurnSink>> {
        Some(Box::new(turns::TurnUpdates {
            core: self.core.get().expect("the core is made").clone(),
            chat: chat.to_string(),
            join: turns::Join::Said,
        }))
    }
}

impl Core {
    /// Makes the core of `parts.data`: takes the folder's lock, opens the
    /// store and the settings, makes the providers, the sandbox and the
    /// lifecycle, and starts at once, in the background: the turns the last
    /// run left are ended and every bench carries on, the archive's purge
    /// runs and is scheduled, the chat list is published, and setup begins.
    pub fn start(parts: Parts) -> Result<Core, String> {
        let (runtime, watch) =
            threads::runtime().map_err(|e| format!("The core's runtime: {e}"))?;
        let lock = InstanceLock::take(&parts.data)?;
        let settings = Arc::new(
            SettingsFile::open(SettingsFile::path(&parts.data, |name| {
                std::env::var(name).ok()
            }))
            .map_err(|e| e.message)?,
        );
        let tools_dir = mcbench_setup::tools_folder(&parts.data)?;
        let providers = Arc::new(Providers::new(
            parts.data.clone(),
            Arc::clone(&settings),
            tools_dir.clone(),
            parts.program.clone(),
            Default::default(),
        ));
        let removed: Arc<OnceLock<Weak<Inner>>> = Arc::new(OnceLock::new());
        let on_removed = Arc::clone(&removed);
        let store = Arc::new(
            Store::open(
                &mcbench_store::DataFolder {
                    root: parts.data.clone(),
                },
                mcbench_store::Options {
                    host: parts.host,
                    limits: Limits::from_env(|name| std::env::var(name).ok())?,
                    chat_removed: Box::new(move |id| {
                        if let Some(inner) = on_removed.get().and_then(Weak::upgrade) {
                            let (providers, id) = (Arc::clone(&inner.providers), id.to_string());
                            inner
                                .handle
                                .spawn(async move { providers.chat_removed(&id).await });
                        }
                    }),
                },
            )
            .map_err(|e| e.message)?,
        );
        let (backend, app): (Arc<dyn Backend>, Option<Arc<AppBackend>>) = match parts.backend {
            Some(given) => (given, None),
            None => {
                let sandbox = Arc::new(PlatformSandbox::new(
                    parts.data.join("sandbox"),
                    &parts.program,
                    Arc::new(mcbench_tools::budget::StoreBudget(Arc::clone(
                        store.storage(),
                    ))),
                ));
                let app = Arc::new(AppBackend::new(
                    Arc::clone(&store),
                    Arc::clone(&providers),
                    sandbox,
                    parts.program.clone(),
                    Arc::clone(&parts.live.shared),
                ));
                (Arc::clone(&app) as Arc<dyn Backend>, Some(app))
            }
        };
        let stopping = CancellationToken::new();
        let sharing = share::Sharing::new(share::options(
            &parts.data,
            app.clone(),
            parts.client.clone(),
            stopping.clone(),
            parts.share,
        ));
        let list_changed = Arc::new(Notify::new());
        let network_access = backend.network_access();
        let services_core: Arc<OnceLock<Weak<Inner>>> = Arc::new(OnceLock::new());
        let turns = Turns::new(
            Arc::clone(&store),
            Arc::new(CoreServices {
                backend: Arc::clone(&backend),
                settings: Arc::clone(&settings),
                list_changed: Arc::clone(&list_changed),
                core: Arc::clone(&services_core),
            }),
            TurnConfig {
                placement: Placement::Updates,
                network_access,
                budgets: mcbench_tools::verify::Budgets::from_env()?,
                max_context_bytes: context::max_bytes_from(
                    std::env::var("MCBENCH_CONTEXT_MAX_BYTES").ok().as_deref(),
                )?,
                keep_exchanges: context::KEEP_EXCHANGES,
                work: Arc::new(mcbench_tools::WorkFolders {
                    root: parts.data.join("sandbox"),
                }),
            },
        );
        let steps_core: Arc<OnceLock<Weak<Inner>>> = Arc::new(OnceLock::new());
        let notify_core = Arc::clone(&steps_core);
        let steps = Arc::new(Steps::new(
            mcbench_setup::steps::native_steps(),
            Box::new(move |list: &[Step]| {
                if let Some(inner) = notify_core.get().and_then(Weak::upgrade) {
                    Core(inner).publish(Update::Setup(list.to_vec()));
                }
            }),
        ));
        let sign_ins = mcbench_providers::login::SignIns::new(Arc::new(providers.logins()));
        let versions_cleared: Arc<OnceLock<Weak<Inner>>> = Arc::new(OnceLock::new());
        let on_installed = Arc::clone(&versions_cleared);
        let on_installed: Arc<dyn Fn(&str) + Send + Sync> = Arc::new(move |_| {
            if let Some(inner) = on_installed.get().and_then(Weak::upgrade) {
                inner.versions.clear();
            }
        });
        let installs = match parts.installer {
            None => mcbench_providers::harness::install::CliInstaller::new(
                tools_dir,
                providers.lookup().clone(),
                on_installed,
            ),
            Some(installer) => mcbench_providers::harness::install::CliInstaller::with_installer(
                tools_dir,
                mcbench_providers::harness::install::platform(),
                providers.lookup().clone(),
                installer,
                on_installed,
            ),
        };
        let inner = Arc::new(Inner {
            handle: runtime.handle().clone(),
            runtime: Mutex::new(Some(runtime)),
            core_id: watch.core,
            window: Mutex::new((None, Vec::new())),
            stopped: AtomicBool::new(false),
            stopping,
            store,
            turns,
            providers,
            backend,
            app,
            steps,
            setup: Mutex::new(Some(SetupRun {
                data: Arc::new(parts.shipped),
                client: parts.client,
                running: false,
            })),
            sign_ins,
            installs,
            versions: settings::Versions::default(),
            sign_ins_told: Mutex::default(),
            installs_told: Mutex::default(),
            list_changed,
            live: live::Live::default(),
            started: tokio::sync::watch::channel(false).0,
            archive_read: AtomicBool::new(false),
            turn_views: Mutex::new(turns::TurnViews::default()),
            list_order: Mutex::new(()),
            benches_shown: Mutex::default(),
            generating: Mutex::default(),
            prompt_ended: tokio::sync::watch::channel(()).0,
            lock: Mutex::new(Some(lock)),
            sharing,
        });
        for cell in [&removed, &steps_core, &versions_cleared, &services_core] {
            let _ = cell.set(Arc::downgrade(&inner));
        }
        let core = Core(inner);
        core.0.live.start(&core, parts.live);
        core.watch_sign_ins_and_installs();
        core.watch_fatal(watch.fatal);
        core.publish_lists();
        core.begin();
        Ok(core)
    }

    /// The runtime's handle, for the window's side to run a route method on.
    pub fn handle(&self) -> &tokio::runtime::Handle {
        &self.0.handle
    }

    /// The store.
    pub fn store(&self) -> &Arc<Store> {
        &self.0.store
    }

    /// The turn lifecycle.
    pub fn turns(&self) -> &Turns {
        &self.0.turns
    }

    /// The tools a turn dispatches to (each waits for setup).
    pub fn tools(&self) -> Tools {
        self.0.backend.tools()
    }

    /// The providers.
    pub fn providers(&self) -> &Arc<Providers> {
        &self.0.providers
    }

    /// Whether the core stopped (a fatal failure or [`Core::shutdown`]).
    pub fn stopped(&self) -> bool {
        self.0.stopped.load(Ordering::SeqCst)
    }

    /// Hands `update` to the window, or keeps it until one is attached.
    pub fn publish(&self, update: Update) {
        let window = {
            let mut window = self.0.window.lock().expect("the window");
            match &window.0 {
                Some(window) => Arc::clone(window),
                None => {
                    window.1.push(update);
                    return;
                }
            }
        };
        window.publish(update);
    }

    /// Runs `job` with the core on the blocking pool (the store is blocking).
    pub(crate) async fn blocking<T: Send + 'static>(
        &self,
        job: impl FnOnce(&Core) -> T + Send + 'static,
    ) -> T {
        let core = self.clone();
        tokio::task::spawn_blocking(move || job(&core))
            .await
            .expect("a store job does not panic")
    }

    /// What starts with the core, in today's order: the archive's purge
    /// (`index.js` at load), the lifecycle's start (`startHttpServer`:
    /// unfinished turns ended, then every bench advanced), the first chat
    /// list, then the purge's schedule; setup beside them.
    fn begin(&self) {
        let core = self.clone();
        self.0.handle.spawn(async move {
            let mut wait = core.purge().await;
            match core.0.turns.start().await {
                Ok(ended) => {
                    for id in ended {
                        eprintln!("[startup] ended the turn the server stopped in {id}");
                    }
                }
                Err(error) => core.publish(Update::Error(error.message)),
            }
            core.0.list_changed.notify_one();
            core.0.started.send_replace(true);
            loop {
                tokio::select! {
                    _ = tokio::time::sleep(wait) => {}
                    _ = core.0.stopping.cancelled() => return,
                }
                wait = core.purge().await;
            }
        });
        self.start_setup();
    }

    /// Resolves once the core's start is done (the purge, the unfinished
    /// turns ended, the benches advanced): the first chat list follows.
    pub async fn started(&self) {
        let _ = self.0.started.subscribe().wait_for(|done| *done).await;
    }

    /// One purge (`clearExpiredArchive`), and how long until the next one
    /// (`untilNextPurge`): when the next archived item is due, at most a
    /// day; a purge that fails is tried again the next day.
    async fn purge(&self) -> Duration {
        {
            let purged = self
                .blocking(|core| -> Result<Option<i64>, mcbench_store::Error> {
                    let store = &core.0.store;
                    store.clear_expired_archive(store.host().now_ms())?;
                    store.next_archive_expiry(store.host().now_ms())
                })
                .await;
            match purged {
                Ok(next) => {
                    self.0.list_changed.notify_one();
                    let now = self.0.store.host().now_ms();
                    next.map_or(PURGE_EVERY, |at| {
                        PURGE_EVERY.min(Duration::from_millis((at - now).max(0) as u64))
                    })
                }
                Err(error) => {
                    eprintln!("[archive] purge failed: {error}");
                    PURGE_EVERY
                }
            }
        }
    }

    /// Starts setup unless it runs (`setup.start(prepare)`, and
    /// `setup.retry()`).
    pub(crate) fn start_setup(&self) {
        let Some(app) = self.0.app.clone() else {
            return;
        };
        let run = {
            let mut setup = self.0.setup.lock().expect("setup");
            let Some(run) = setup.as_mut() else { return };
            if run.running {
                return;
            }
            run.running = true;
            (Arc::clone(&run.data), run.client.clone())
        };
        app.retrying();
        let core = self.clone();
        self.0.handle.spawn(async move {
            let (data, client) = run;
            let folder = core.0.store.folder().root.clone();
            let shell = core.shell_work(&app, &client);
            let made = backend::run_setup(
                &core.0.steps,
                &folder,
                &data,
                &client,
                shell,
                &core.0.stopping,
            )
            .await;
            match made {
                Ok((data, textures)) => {
                    app.prepare(data, textures);
                    let unprepared = match &*app.readiness().borrow() {
                        Some(Err(error)) => Some(error.clone()),
                        _ => None,
                    };
                    match unprepared {
                        None => core.0.live.ready(&core),
                        // Setup's steps are done and what they made cannot
                        // be put to use (no build child beside the program,
                        // say): said where a log and a person read it, and
                        // the pane fails with it instead of waiting.
                        Some(error) => {
                            eprintln!("[setup] {error}");
                            core.publish(Update::Error(error.clone()));
                            core.0.live.unprepared(error);
                        }
                    }
                }
                Err(error) => {
                    eprintln!("[setup] {error}");
                    app.failed(error);
                }
            }
            if let Some(run) = core.0.setup.lock().expect("setup").as_mut() {
                run.running = false;
            }
        });
    }

    /// The shell step's work ([`PlatformSandbox::shell_step`] of the files
    /// `mcbench_setup::shell::files` names), as setup runs it.
    fn shell_work(&self, app: &Arc<AppBackend>, client: &reqwest::Client) -> mcbench_setup::Shell {
        Arc::clone(app.sandbox()).shell_step(
            mcbench_setup::shell::files(
                Arc::clone(&self.0.steps),
                self.0.store.folder().root.clone(),
                client.clone(),
                self.0.stopping.clone(),
            ),
            Arc::clone(self.0.providers.shell()),
        )
    }

    /// The shell step alone, run again in the background (after WSL was
    /// installed with no restart needed): the other steps are untouched.
    pub(crate) fn run_shell_step(&self, app: &Arc<AppBackend>) {
        let client = {
            let setup = self.0.setup.lock().expect("setup");
            setup
                .as_ref()
                .expect("the core keeps its setup run while it lives")
                .client
                .clone()
        };
        let shell = self.shell_work(app, &client);
        let steps = Arc::clone(&self.0.steps);
        self.0.handle.spawn(async move {
            if let Err(error) = steps.track("shell", shell).await {
                eprintln!("[setup] {error}");
            }
        });
    }

    /// The chat list, published whenever it changed (coalesced).
    fn publish_lists(&self) {
        let core = self.clone();
        self.0.handle.spawn(async move {
            loop {
                tokio::select! {
                    _ = core.0.list_changed.notified() => {}
                    _ = core.0.stopping.cancelled() => return,
                }
                core.publish_list_now().await;
            }
        });
    }

    /// Says the chat list changed.
    pub(crate) fn list_changed(&self) {
        self.0.list_changed.notify_one();
    }

    /// The fatal watcher: the first panic on the runtime is published once
    /// as `Update::Fatal`, the core takes no more commands, and everything
    /// it started stops within [`FATAL_SHUTDOWN`].
    fn watch_fatal(&self, mut fatal: tokio::sync::watch::Receiver<Option<String>>) {
        let core = Arc::downgrade(&self.0);
        self.0.handle.spawn(async move {
            let Ok(line) = fatal.wait_for(Option::is_some).await.map(|l| l.clone()) else {
                return;
            };
            let Some(inner) = core.upgrade() else { return };
            let core = Core(inner);
            if core.0.stopped.swap(true, Ordering::SeqCst) {
                return;
            }
            let line = line.expect("waited for Some");
            eprintln!("{line}");
            core.publish(Update::Fatal(line));
            if tokio::time::timeout(FATAL_SHUTDOWN, core.stop_everything())
                .await
                .is_err()
            {
                eprintln!("[fatal] shutdown failed: it did not end within its deadline");
                core.0.backend.shutdown().await;
                core.0.live.stop().await;
            }
        });
    }

    /// `shutdown`'s work: no turn starts, every prompt request is stopped
    /// and waited for (each ends in `Generated::Cancelled`), every turn is
    /// stopped and waited for, every publish is ended and waited for (each
    /// ends in `PublishStep::Failed`), then the live world, the sign-ins,
    /// the engines, the build child, the boxes and the CLIs stop.
    async fn stop_everything(&self) {
        self.0.stopping.cancel();
        self.prompt_requests_ended().await;
        self.0.sharing.ended().await;
        self.0.sign_ins.shutdown();
        self.0.turns.shutdown().await;
        self.0.live.stop().await;
        self.0.backend.shutdown().await;
    }

    /// Stops the core as the app quits: everything it started ends, then
    /// its runtime, then the data folder's lock is let go. Called once,
    /// from the window's thread.
    pub fn shutdown(&self) {
        if !self.0.stopped.swap(true, Ordering::SeqCst) {
            let core = self.clone();
            self.0
                .handle
                .block_on(async move { core.stop_everything().await });
        }
        threads::forget(self.0.core_id);
        if let Some(runtime) = self.0.runtime.lock().expect("the runtime").take() {
            runtime.shutdown_timeout(Duration::from_secs(1));
        }
        drop(self.0.lock.lock().expect("the data folder's lock").take());
    }
}

impl mcbench_ui::Core for Core {
    /// `preferences.json` of the data folder ([`preferences`]); one that
    /// cannot be read is published as `Update::Error` (kept until the window
    /// is attached), and the defaults are given.
    fn preferences(&self) -> Preferences {
        let (preferences, error) =
            preferences::read(&preferences::file(&self.0.store.folder().root));
        if let Some(error) = error {
            self.publish(Update::Error(error));
        }
        preferences
    }

    /// The window: the updates kept until now, then the lists, the
    /// providers and the favourites once the core's start is done.
    fn attach(&self, window: Arc<dyn Window>) {
        let pending = {
            let mut slot = self.0.window.lock().expect("the window");
            slot.0 = Some(Arc::clone(&window));
            std::mem::take(&mut slot.1)
        };
        for update in pending {
            window.publish(update);
        }
        let core = self.clone();
        self.0.handle.spawn(async move {
            core.started().await;
            core.publish_list_now().await;
            core.publish_providers().await;
            core.publish_favorites().await;
        });
    }

    fn send(&self, command: Command) {
        if self.stopped() {
            return;
        }
        // A prompt request's token exists from the moment it is sent, so a
        // cancel sent after it finds it (`bench.rs`).
        if let Command::GeneratePrompt { request, .. } = &command {
            self.generate_token(*request);
        }
        // The live world's commands are handed over now, so they reach it in
        // the order they were sent (`live.rs`).
        if live::is_live(&command) {
            self.0.live.hand_over(command);
            return;
        }
        let core = self.clone();
        self.0
            .handle
            .spawn(async move { core.command(command).await });
    }

    fn image_file(&self, address: &str) -> Result<PathBuf, String> {
        Core::image_file(self, address)
            .map(|(path, _)| path)
            .map_err(|refused| refused.message)
    }

    fn attachment_file(&self, address: &str) -> Result<AttachmentFile, String> {
        Core::attachment_file(self, address).map_err(|refused| refused.message)
    }

    /// `GET /api/blocks/<id>/icon.png`: the icon's PNG, cut from the bake's
    /// atlas and tinted, kept by the maker after its first making; today's
    /// 404 sentence for a name the block table lacks, and the tools' own
    /// sentence before setup has the data.
    fn block_icon(&self, id: &str) -> Result<Vec<u8>, String> {
        let prepared = self
            .0
            .app
            .as_ref()
            .and_then(|app| app.prepared())
            .ok_or("Minecraft's data is not ready yet")?;
        prepared
            .icons
            .block_icon(id)?
            .ok_or_else(|| "Unknown block".to_string())
    }

    /// The app quits: [`Core::shutdown`] (today's server `stop`).
    fn shutdown(&self) {
        Core::shutdown(self)
    }
}
