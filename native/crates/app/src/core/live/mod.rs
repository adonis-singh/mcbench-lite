//! The live world: one in-process engine world (`mcbench_engine::EngineWorld`,
//! on an engine thread of its own) holding the view the window asks for
//! (`Command::Live(Show)`, the only thing that places one), ticked by the
//! tick clock, clicked and walked in, and fed to the renderer
//! (`mcbench_render::Renderer::world`, `live_show`, `live_patch`). It is
//! today's page's live world (`web/live-world.mjs`) with the engine worker's
//! clock (`web/core-worker.mjs`) and the bridge's respawn
//! (`web/bluemap-bridge.js`), moved into the core.
//!
//! One task on the core's runtime owns it and takes its asks in the order
//! they were sent (`Core::send` hands them over before it returns), so a
//! click, a spawn or a player's input runs between two ticks in arrival
//! order. Each engine request is awaited; the renderer's calls return at
//! once (`world`, which keeps a placement's blueprint, runs on the blocking
//! pool and is awaited: `live_show` needs the world kept).
//!
//! The clock's rule is `core-worker.mjs` `run`'s: tick n is due n × 50 ms
//! after the world started running (Play, or a fresh placement while
//! playing, starts it at n = 0, due at once); a late timer is made up, one
//! tick at a time with due ticks before any ask; when a tick ends more than
//! 1000 ms after the next one was due the count starts again from now.
//! While running, `Update::Live` with the tick count is published at most
//! every 250 ms and the last count always (`live-world.mjs` `emitSoon`).
//!
//! A turn's change to the view on show is applied as the page applies it
//! (`live-world.mjs` `sync`): the commands from the version the world holds
//! (`mcbench_world::live_views`), with `edit` when they are not a fresh
//! placement and the view's render mask is unchanged (ticks continue),
//! else a placement (a fresh one at tick 0 spawns a standing player again
//! where it stood).

pub mod blocks;
pub mod clock;
mod views;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use indexmap::IndexMap;
use serde_json::Value;
use tokio::sync::{mpsc, oneshot};

use mcbench_engine::{Engine, EngineError, EngineWorld, Ticked};
use mcbench_render::live::{Framing, LivePatch, PlayerView, Shown, ViewKey};
use mcbench_render::{Clip, RenderError, Renderer, WorldUpload};
use mcbench_tools::settle::Settler;
use mcbench_tools::shots::SharedRenderer;
use mcbench_ui::{
    LiveCommand, LivePhase, LiveState, PaneHandle, PlayerState, Running, Update, ViewerInput,
};
use mcbench_world::export::{EngineExport, ExportTimes, world_zip};
use mcbench_world::facts::MinecraftFacts;
use mcbench_world::{Blueprint, PlaceCommands};

use super::Core;
use blocks::LiveBlocks;
pub use clock::{Clock, RuntimeClock};

/// The tick clock's step (`TICK_MS`).
pub const TICK: Duration = Duration::from_millis(50);
/// A world this far behind starts counting again (`LAG_MS`).
pub const LAG: Duration = Duration::from_millis(1000);
/// The running tick count is published at most this often
/// (`TICKS_DRAWN_MS`).
pub const TICKS_DRAWN: Duration = Duration::from_millis(250);
/// A turn's new revision is flown to in this long.
pub const FLY_MS: u32 = 700;
/// A player below this is respawned (`VOID_Y`).
pub const VOID_Y: f64 = -128.0;
/// The note of a respawn below [`VOID_Y`] (`web/bluemap-bridge.js` 881).
pub const FELL_OUT: &str = "Fell out of the world";
/// A command for a world when none is on show (`live-world.mjs`
/// `shownEngine`).
pub const NO_WORLD: &str = "There is no live world on show";

/// A standing player's eye height (`Avatar.POSES`, standing: 1.62f).
const EYE_HEIGHT: f64 = 1.62f32 as f64;

/// Starts the renderer from setup's assets: the app's drawing crate.
pub type RendererStart = Box<dyn Fn(&mcbench_render::Assets) -> Started + Send + Sync>;

/// A started renderer, or the sentence of why it did not start.
pub type Started = Result<Arc<dyn Renderer>, String>;

/// What the live world needs of setup: the in-process engine, Minecraft's
/// facts, the settles (the schematic's tick 0) and the block table's names
/// (a click's and a selection's).
pub struct LiveSources {
    /// The in-process engine.
    pub engine: Engine,
    /// Minecraft's facts (the import's).
    pub facts: Arc<MinecraftFacts>,
    /// The settles.
    pub settler: Arc<Settler>,
    /// Block id to Minecraft's name (`BlockStates::names`).
    pub names: Arc<IndexMap<String, String>>,
}

/// How the core's live world is made.
pub struct LiveParts {
    /// The tick clock's and the throttle's time.
    pub clock: Arc<dyn Clock>,
    /// The renderer, started once setup has its assets (the app's:
    /// [`super::renderer::draw`]); none only beside `given`, whose renderer
    /// it is.
    pub renderer: Option<RendererStart>,
    /// A test's sources and renderer (or the renderer's sentence), given
    /// at start instead of setup's.
    pub given: Option<(LiveSources, Started)>,
    /// The process's one renderer as the tools see it: given the live
    /// view's renderer (or its sentence) when it starts, so `see_build`
    /// draws with the same one, and closed when the live world stops.
    pub shared: Arc<SharedRenderer>,
    /// The machine's offset from UTC in seconds at a moment (seconds since
    /// 1970): a world export's zip dates are local time, as the page's.
    pub utc_offset: fn(i64) -> i32,
}

impl Default for LiveParts {
    fn default() -> LiveParts {
        LiveParts {
            clock: Arc::new(RuntimeClock::default()),
            renderer: Some(Box::new(|assets| {
                super::renderer::draw(assets)
                    .map(|draw| Arc::new(draw) as Arc<dyn Renderer>)
                    .map_err(render_text)
            })),
            given: None,
            shared: SharedRenderer::new(),
            utc_offset: local_offset,
        }
    }
}

/// The world on show as the engine saves it, and the spawn above the
/// build's middle (`live-world.mjs` `exportWorld`, before `worldZip`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorldSave {
    /// `Session::export` read.
    pub export: EngineExport,
    /// The spawn.
    pub spawn: [i32; 3],
}

enum Ask {
    Live(LiveCommand),
    Viewer(ViewerInput),
    ExportWorld {
        view: ViewKey,
        name: String,
        to: PathBuf,
    },
    Changed,
    Ready(Arc<LiveSources>, Option<Started>),
    /// What setup made could not be put to use: no world can be shown.
    Unprepared(String),
    Save(oneshot::Sender<Result<WorldSave, String>>),
    Stop,
}

type Envelope = (Ask, Option<oneshot::Sender<()>>);

#[derive(Default)]
struct Handles {
    asks: Option<mpsc::UnboundedSender<Envelope>>,
    sources: Option<Arc<LiveSources>>,
    renderer: Option<RendererStart>,
    renderer_asked: bool,
}

/// The live world's handle in the core (see the module's doc).
#[derive(Default)]
pub struct Live(Mutex<Handles>);

/// Whether `command` is the live world's: it is handed over in order.
pub fn is_live(command: &mcbench_ui::Command) -> bool {
    matches!(
        command,
        mcbench_ui::Command::Live(_)
            | mcbench_ui::Command::Viewer(_)
            | mcbench_ui::Command::ExportWorld { .. }
    )
}

impl Live {
    /// Starts the live world's task on the core's runtime.
    pub fn start(&self, core: &Core, parts: LiveParts) {
        assert!(
            parts.renderer.is_some() != parts.given.is_some(),
            "the live world is given a renderer's start or a test's renderer"
        );
        let (asks, taken) = mpsc::unbounded_channel();
        {
            let mut handles = self.0.lock().expect("the live world");
            handles.asks = Some(asks);
            handles.renderer = parts.renderer;
        }
        let actor = Actor::new(core.clone(), parts.clock, parts.utc_offset, parts.shared);
        core.0.handle.spawn(actor.run(taken));
        if let Some((sources, renderer)) = parts.given {
            let sources = Arc::new(sources);
            let mut handles = self.0.lock().expect("the live world");
            handles.sources = Some(Arc::clone(&sources));
            handles.renderer_asked = true;
            drop(handles);
            self.enqueue(Ask::Ready(sources, Some(renderer)), None);
        }
    }

    fn enqueue(&self, ask: Ask, done: Option<oneshot::Sender<()>>) {
        if let Some(asks) = &self.0.lock().expect("the live world").asks {
            let _ = asks.send((ask, done));
        }
    }

    /// Hands a live command over now, in the order sent (`Core::send`).
    pub fn hand_over(&self, command: mcbench_ui::Command) {
        self.enqueue(Self::ask_of(command), None);
    }

    /// Hands a live command over and resolves once it has ended
    /// (`Core::command`).
    pub async fn run(&self, command: mcbench_ui::Command) {
        let (done, ended) = oneshot::channel();
        self.enqueue(Self::ask_of(command), Some(done));
        let _ = ended.await;
    }

    fn ask_of(command: mcbench_ui::Command) -> Ask {
        match command {
            mcbench_ui::Command::Live(live) => Ask::Live(live),
            mcbench_ui::Command::Viewer(input) => Ask::Viewer(input),
            mcbench_ui::Command::ExportWorld { view, name, to } => {
                Ask::ExportWorld { view, name, to }
            }
            other => panic!("{other:?} is not the live world's"),
        }
    }

    /// What setup gave the live world, once it has.
    pub fn sources(&self) -> Option<Arc<LiveSources>> {
        self.0.lock().expect("the live world").sources.clone()
    }

    /// A turn (or a branch, a revert) changed `chat`'s world: the view on
    /// show is brought to its latest version.
    pub fn changed(&self, _core: &Core, _chat: &str) {
        self.enqueue(Ask::Changed, None);
    }

    /// Setup has the shipped data: the engine, the facts, the settles and
    /// the block table's names go to the live world at once, and then the
    /// renderer is started from the assets, once, on the blocking pool (a
    /// thread of its own while it opens the adapter and reads the assets,
    /// seconds on a cold machine; never a runtime worker or the window's
    /// thread). The live world takes it, or its sentence, as a second
    /// `Ready`; a `Show` before it waits in `Preparing`.
    pub fn ready(&self, core: &Core) {
        let Some(app) = core.0.app.clone() else {
            return;
        };
        let (Some(prepared), Some(engine)) = (app.prepared(), app.live_engine()) else {
            return;
        };
        let start = {
            let mut handles = self.0.lock().expect("the live world");
            if handles.renderer_asked {
                None
            } else {
                handles.renderer_asked = true;
                Some(
                    handles
                        .renderer
                        .take()
                        .expect("a renderer's start where no test gave one"),
                )
            }
        };
        let core = core.clone();
        core.0.handle.clone().spawn(async move {
            let assets = mcbench_setup::start::assets(&prepared.data, &prepared.textures);
            let made = tokio::task::spawn_blocking(move || {
                let names = mcbench_world::block_states::BlockStates::new(
                    &prepared.data.text("block-table.tsv")?,
                    &prepared.data.text("render-layers.tsv")?,
                )?
                .names;
                Ok::<_, String>(LiveSources {
                    engine,
                    facts: Arc::clone(&prepared.engines.facts),
                    settler: Arc::clone(&prepared.engines.settler),
                    names: Arc::new(names),
                })
            })
            .await
            .expect("the live world's sources do not panic");
            let sources = match made {
                Ok(sources) => Arc::new(sources),
                Err(error) => {
                    core.publish(Update::Error(error));
                    return;
                }
            };
            core.0.live.0.lock().expect("the live world").sources = Some(Arc::clone(&sources));
            core.0
                .live
                .enqueue(Ask::Ready(Arc::clone(&sources), None), None);
            if let Some(start) = start {
                let started = tokio::task::spawn_blocking(move || start(&assets))
                    .await
                    .expect("the renderer's start does not panic");
                // Once, to the app's log: what draws the 3D frames.
                match &started {
                    Ok(renderer) => {
                        eprintln!("{}", super::renderer::adapter_line(renderer.as_ref()))
                    }
                    Err(error) => eprintln!("[renderer] {error}"),
                }
                core.0
                    .live
                    .enqueue(Ask::Ready(sources, Some(started)), None);
            }
        });
    }

    /// Setup ended and what it made could not be put to use (`error`, the
    /// backend's sentence): the pane is told so (`Update::LivePane(Err)`)
    /// and every view fails with it, where it would otherwise wait in
    /// `Preparing` for a renderer that is never started.
    pub fn unprepared(&self, error: String) {
        self.enqueue(Ask::Unprepared(error), None);
    }

    /// The world on show as a Minecraft save holds it.
    pub async fn save(&self) -> Result<WorldSave, String> {
        let (reply, answer) = oneshot::channel();
        self.enqueue(Ask::Save(reply), None);
        answer
            .await
            .map_err(|_| "The live world has stopped".to_string())?
    }

    /// Ends the live world's task and its engine thread.
    pub async fn stop(&self) {
        let (done, ended) = oneshot::channel();
        self.enqueue(Ask::Stop, Some(done));
        let _ = ended.await;
    }
}

/// The renderer's sentence.
pub(crate) fn render_text(error: RenderError) -> String {
    super::renderer::sentence(error)
}

/// The blueprint of a world with no blocks: air in its palette, no
/// sections (`live-patch.mjs` `emptyBlueprint`).
fn empty_blueprint() -> Vec<u8> {
    let air = b"minecraft:air";
    let mut bytes = Vec::with_capacity(4 + 2 + air.len() + 4 + 24 + 4);
    bytes.extend(1i32.to_be_bytes());
    bytes.extend((air.len() as u16).to_be_bytes());
    bytes.extend(air);
    bytes.extend([0; 4 + 24 + 4]);
    bytes
}

/// `Math.round`.
fn js_round(value: f64) -> f64 {
    (value + 0.5).floor()
}

/// `besideBuild({ min, max }, target, rotation)`: where player mode starts
/// the player (`near`, feet) and what it looks at (`look`).
pub fn beside_build(
    min: [i32; 3],
    max: [i32; 3],
    target: [f64; 3],
    rotation: f64,
) -> ([f64; 3], [f64; 3]) {
    let heading = [rotation.sin(), -rotation.cos()];
    let low = [min[0] as f64, min[2] as f64];
    let high = [(max[0] + 1) as f64, (max[2] + 1) as f64];
    let seen: [f64; 2] = [0, 1].map(|axis| target[axis * 2].max(low[axis]).min(high[axis]));
    let exit = |direction: [f64; 2]| {
        [0, 1]
            .map(|axis| {
                if direction[axis] > 0.0 {
                    (high[axis] - seen[axis]) / direction[axis]
                } else if direction[axis] < 0.0 {
                    (low[axis] - seen[axis]) / direction[axis]
                } else {
                    f64::INFINITY
                }
            })
            .into_iter()
            .fold(f64::INFINITY, f64::min)
    };
    let back = exit(heading.map(|value| -value));
    let ahead = exit(heading);
    let along = |distance: f64| [0, 1].map(|axis| seen[axis] + heading[axis] * distance);
    let edge = along(-back - 0.5);
    let middle = along((ahead - back) / 2.0);
    (
        [edge[0], min[1] as f64, edge[1]],
        [middle[0], (min[1] + max[1] + 1) as f64 / 2.0, middle[1]],
    )
}

/// What the player holds: the keys, the taps since the last tick, the look.
#[derive(Clone, Copy, Debug, Default)]
struct Held {
    keys: u32,
    taps: u32,
    yaw: f32,
    pitch: f32,
}

#[derive(Clone, Copy)]
struct Standing {
    /// Its latest view: the spawn's, then each tick's.
    view: [f64; 28],
    held: Held,
    /// Input was given since the spawn (a fresh placement's respawn gives
    /// it back, `placeAfresh`).
    input_given: bool,
    /// Where the last spawn stood it, and whether it flew (the bridge's
    /// `player.spawn`, where a fall respawns it).
    spawn: ([f64; 3], bool),
    /// The look a respawn takes (the bridge's `player.yaw`, `pitch`).
    look: (f32, f32),
}

/// The view on show and the world held for it (`live-world.mjs` `reset`).
struct ViewState {
    phase: LivePhase,
    /// The version a sync failed on: not tried again until another
    /// version or a restart.
    failure: Option<u64>,
    restarting: bool,
    running: Running,
    /// The version the engine's world was placed from.
    held: Option<u64>,
    /// The version shown.
    shown_version: Option<u64>,
    /// The descriptor of what is shown (`base.view`), none before a base.
    descriptor: Option<Value>,
    /// The blocks of what is shown; none for an empty world.
    blocks: Option<LiveBlocks>,
    ticks: u64,
    /// The running clock: when it started and the next tick's number.
    clock: Option<(Duration, u64)>,
    spawning: bool,
    player: Option<Standing>,
    respawning: bool,
}

impl ViewState {
    fn new(phase: LivePhase) -> ViewState {
        ViewState {
            phase,
            failure: None,
            restarting: false,
            running: Running::Playing,
            held: None,
            shown_version: None,
            descriptor: None,
            blocks: None,
            ticks: 0,
            clock: None,
            spawning: false,
            player: None,
            respawning: false,
        }
    }

    /// A world with blocks is on show (the page's `shownEngine`).
    fn on_show(&self) -> bool {
        self.blocks.is_some()
    }
}

fn bounds_of(descriptor: &Value) -> Option<[[i32; 3]; 2]> {
    let corner = |v: &Value| -> Option<[i32; 3]> {
        let list = v.as_array()?;
        Some([0, 1, 2].map(|i| list[i].as_i64().expect("a corner's integer") as i32))
    };
    let bounds = descriptor.get("bounds")?;
    Some([corner(&bounds["min"])?, corner(&bounds["max"])?])
}

fn raised(bounds: [[i32; 3]; 2], y_offset: i32) -> [[i32; 3]; 2] {
    let [mut min, mut max] = bounds;
    min[1] += y_offset;
    max[1] += y_offset;
    [min, max]
}

struct Actor {
    core: Core,
    clock: Arc<dyn Clock>,
    utc_offset: fn(i64) -> i32,
    y_offset: i32,
    sources: Option<Arc<LiveSources>>,
    renderer: Option<Started>,
    shared: Arc<SharedRenderer>,
    views: Option<Arc<Mutex<views::Views>>>,
    world: Option<Arc<EngineWorld>>,
    key: Option<ViewKey>,
    view: ViewState,
    /// The live world the renderer keeps (`live:<view>`).
    uploaded: Option<String>,
    emitted: Option<Duration>,
    trailing: Option<Duration>,
    recheck: Option<Duration>,
    sync_error: Option<String>,
}

async fn until(clock: &dyn Clock, at: Option<Duration>) {
    match at {
        Some(at) => clock.until(at).await,
        None => std::future::pending().await,
    }
}

impl Actor {
    fn new(
        core: Core,
        clock: Arc<dyn Clock>,
        utc_offset: fn(i64) -> i32,
        shared: Arc<SharedRenderer>,
    ) -> Actor {
        Actor {
            core,
            clock,
            utc_offset,
            y_offset: mcbench_world::dimension::y_offset(),
            sources: None,
            renderer: None,
            shared,
            views: None,
            world: None,
            key: None,
            view: ViewState::new(LivePhase::Idle),
            uploaded: None,
            emitted: None,
            trailing: None,
            recheck: None,
            sync_error: None,
        }
    }

    async fn run(mut self, mut asks: mpsc::UnboundedReceiver<Envelope>) {
        loop {
            let wake = [self.tick_due(), self.trailing, self.recheck]
                .into_iter()
                .flatten()
                .min();
            tokio::select! {
                biased;
                _ = until(&*self.clock, wake) => self.timers().await,
                envelope = asks.recv() => match envelope {
                    None => {
                        self.end().await;
                        return;
                    }
                    Some((Ask::Stop, done)) => {
                        self.end().await;
                        if let Some(done) = done {
                            let _ = done.send(());
                        }
                        return;
                    }
                    Some((ask, done)) => {
                        self.handle(ask).await;
                        if let Some(done) = done {
                            let _ = done.send(());
                        }
                    }
                },
            }
        }
    }

    async fn end(&mut self) {
        self.view.clock = None;
        if let Some(world) = self.world.take() {
            world.stop().await;
        }
        // The tools let the renderer go with the live world.
        self.shared.close();
    }

    fn tick_due(&self) -> Option<Duration> {
        let (start, n) = self.view.clock?;
        (self.view.running == Running::Playing && self.view.on_show())
            .then(|| start + Duration::from_millis(50 * n))
    }

    /// What is due: a tick first, then the throttled count, then a bench
    /// view's second look.
    async fn timers(&mut self) {
        let now = self.clock.now();
        if self.tick_due().is_some_and(|due| due <= now) {
            let (start, n) = self.view.clock.expect("a tick is due");
            if self.world_tick().await && self.view.clock.is_some() {
                let now = self.clock.now();
                let next = start + Duration::from_millis(50 * (n + 1));
                self.view.clock = Some(if now > next + LAG {
                    (now, 0)
                } else {
                    (start, n + 1)
                });
            }
            return;
        }
        if self.trailing.is_some_and(|at| at <= now) {
            self.trailing = None;
            self.emit();
        }
        if self.recheck.is_some_and(|at| at <= now) {
            self.recheck = None;
            self.sync().await;
            self.emit();
        }
    }

    fn publish(&self, update: Update) {
        self.core.publish(update);
    }

    fn snapshot(&self) -> LiveState {
        let descriptor = self.view.descriptor.as_ref();
        LiveState {
            view: self.key.clone(),
            revision: self.view.shown_version,
            phase: self.view.phase.clone(),
            running: self.view.on_show().then(|| self.view.running.clone()),
            tick: self.view.ticks,
            player: if self.view.spawning {
                PlayerState::Spawning
            } else if self.view.player.is_some() {
                PlayerState::Standing
            } else {
                PlayerState::None
            },
            bounds: descriptor
                .and_then(bounds_of)
                .map(|b| raised(b, self.y_offset)),
            y_offset: self.y_offset,
            blocks: descriptor.map_or(0, |d| {
                d["blockCount"].as_u64().expect("a view's block count")
            }),
            layout: descriptor
                .and_then(|d| d["layout"].as_array())
                .map(|layout| {
                    layout
                        .iter()
                        .map(|placed| {
                            let corner = |v: &Value| {
                                [0, 1, 2].map(|i| v[i].as_i64().expect("a corner") as i32)
                            };
                            (
                                placed["id"].as_str().expect("a member's id").to_string(),
                                raised(
                                    [corner(&placed["min"]), corner(&placed["max"])],
                                    self.y_offset,
                                ),
                            )
                        })
                        .collect()
                })
                .unwrap_or_default(),
        }
    }

    fn emit(&mut self) {
        self.emitted = Some(self.clock.now());
        self.publish(Update::Live(self.snapshot()));
    }

    /// `emitSoon`: now, or once [`TICKS_DRAWN`] has passed since the last.
    fn emit_soon(&mut self) {
        let now = self.clock.now();
        match self.emitted {
            Some(at) if now < at + TICKS_DRAWN => {
                if self.trailing.is_none() {
                    self.trailing = Some(at + TICKS_DRAWN);
                }
            }
            _ => self.emit(),
        }
    }

    async fn handle(&mut self, ask: Ask) {
        match ask {
            Ask::Live(LiveCommand::Show { view, afresh }) => self.show(view, afresh).await,
            Ask::Live(LiveCommand::Play) => self.run_to(Running::Playing),
            Ask::Live(LiveCommand::Pause) => self.run_to(Running::Paused),
            Ask::Live(LiveCommand::Step) => self.step().await,
            Ask::Live(LiveCommand::Restart) => {
                self.view.restarting = true;
                // Asked after a sync failed, it tries that version again.
                self.view.failure = None;
                if let LivePhase::Failed { .. } = self.view.phase {
                    self.view.phase = if self.view.descriptor.is_some() {
                        LivePhase::Shown
                    } else {
                        LivePhase::Preparing
                    };
                }
                self.sync().await;
                self.emit();
            }
            Ask::Viewer(input) => self.viewer(input).await,
            Ask::ExportWorld { view, name, to } => {
                let exported = self.export_world(&view, name, to).await;
                self.publish(Update::Exported(exported));
            }
            Ask::Changed => {
                if self.key.is_some() {
                    self.sync().await;
                    if self.key.as_ref().is_some_and(|k| k.0.starts_with("bench:")) {
                        // A bench view is composed at most every 250 ms
                        // (`LOOK_MS`): one more look after that.
                        self.recheck = Some(
                            self.clock.now()
                                + Duration::from_millis(mcbench_world::live_views::LOOK_MS as u64),
                        );
                    }
                    self.emit();
                }
            }
            Ask::Unprepared(error) => {
                self.publish(Update::LivePane(Err(error.clone())));
                self.renderer = Some(Err(error));
                if self.key.is_some() {
                    self.sync().await;
                    self.emit();
                }
            }
            Ask::Ready(sources, renderer) => {
                if self.views.is_none() {
                    self.views = Some(Arc::new(Mutex::new(views::views(
                        Arc::clone(self.core.store()),
                        Arc::clone(&self.clock),
                        Arc::clone(&sources.facts),
                    ))));
                }
                self.sources = Some(sources);
                if let Some(renderer) = renderer {
                    self.publish(Update::LivePane(match &renderer {
                        Ok(renderer) => Ok(PaneHandle(renderer.live())),
                        Err(error) => Err(error.clone()),
                    }));
                    // see_build draws with the same renderer.
                    self.shared.set(renderer.clone());
                    self.renderer = Some(renderer);
                }
                if self.key.is_some() {
                    self.sync().await;
                    self.emit();
                }
            }
            Ask::Save(reply) => {
                let _ = reply.send(self.save().await);
            }
            Ask::Stop => unreachable!("handled by run"),
        }
    }

    async fn show(&mut self, view: Option<ViewKey>, afresh: bool) {
        match view {
            None => {
                if self.key.is_some() {
                    self.leave_view().await;
                    self.key = None;
                    self.view = ViewState::new(LivePhase::Idle);
                }
                self.emit();
            }
            Some(key) if self.key.as_ref() == Some(&key) => {
                if afresh {
                    self.view.restarting = true;
                    self.view.failure = None;
                }
                self.sync().await;
                self.emit();
            }
            Some(key) => {
                if self.key.is_some() {
                    self.leave_view().await;
                }
                self.key = Some(key);
                self.view = ViewState::new(LivePhase::Preparing);
                self.sync_error = None;
                self.emit();
                self.sync().await;
                self.emit();
            }
        }
    }

    /// The view on show is let go: nothing drawn, the clock stopped, its
    /// player gone with no note, the renderer's copy freed. The engine's
    /// world stays for the next placement.
    async fn leave_view(&mut self) {
        self.view.clock = None;
        if let Some(Ok(renderer)) = &self.renderer {
            renderer.live_show(None);
            if let Some(name) = self.uploaded.take() {
                let renderer = Arc::clone(renderer);
                let dropped = tokio::task::spawn_blocking(move || renderer.drop_world(&name))
                    .await
                    .expect("the renderer's drop does not panic");
                if let Err(error) = dropped {
                    self.publish(Update::Error(render_text(error)));
                }
            }
        }
    }

    fn engine_world(&mut self) -> Result<Arc<EngineWorld>, String> {
        if let Some(world) = &self.world
            && world.alive()
        {
            return Ok(Arc::clone(world));
        }
        let sources = self
            .sources
            .as_ref()
            .expect("the live world has its sources");
        let world = Arc::new(EngineWorld::new(sources.engine).map_err(|e| e.to_string())?);
        self.world = Some(Arc::clone(&world));
        Ok(world)
    }

    /// The world stops (`core-worker.mjs` `stop`, `live-world.mjs`
    /// `stopped`): the first reason stands.
    fn stop(&mut self, message: &str) {
        self.view.clock = None;
        if !matches!(self.view.running, Running::Stopped(_)) {
            self.view.running = Running::Stopped(format!("The live world stopped: {message}"));
        }
    }

    /// A request that failed: a world it spent stops (`core-worker.mjs`
    /// `stops`). Answers the failure's sentence.
    async fn failed(&mut self, error: EngineError) -> String {
        let message = error.to_string();
        let spent = match &self.world {
            Some(world) => world.spent().await.unwrap_or(true),
            None => false,
        };
        if spent {
            self.stop(&message);
        }
        message
    }

    /// Brings the world to the view's latest version (`live-world.mjs`
    /// `sync`, with `behind` and the import).
    async fn sync(&mut self) {
        let Some(key) = self.key.clone() else { return };
        let Some(renderer) = &self.renderer else {
            return;
        };
        // A renderer that did not start, or a backend that setup could not
        // prepare, fails the view whether or not the sources came.
        let renderer = match renderer {
            Ok(renderer) => Arc::clone(renderer),
            Err(error) => {
                self.view.phase = LivePhase::Failed {
                    error: error.clone(),
                    nothing_shown: self.view.descriptor.is_none(),
                };
                return;
            }
        };
        if self.sources.is_none() {
            return;
        }
        let views = Arc::clone(self.views.as_ref().expect("views with the sources"));
        let (failure, restarting, shown, held) = (
            self.view.failure,
            self.view.restarting,
            self.view.shown_version,
            self.view.held,
        );
        let since = if restarting { None } else { held };
        let name = key.0.clone();
        let looked = tokio::task::spawn_blocking(move || {
            let mut views = views.lock().expect("the views");
            let described = views.describe(&name).map_err(|e| (e, None))?;
            let version = described["version"].as_u64().expect("a view's version");
            if Some(version) == failure || (!restarting && Some(version) == shown) {
                return Ok((described, None));
            }
            let imports = views
                .imports(&name, since)
                .map_err(|e| (e, Some(version)))?;
            Ok((described, Some(imports)))
        })
        .await
        .expect("a view's look does not panic");
        let (described, imports) = match looked {
            Ok(looked) => looked,
            Err((error, version)) => {
                self.view.failure = version;
                self.view.phase = LivePhase::Failed {
                    error,
                    nothing_shown: self.view.descriptor.is_none(),
                };
                return;
            }
        };
        let sync_error = described["syncError"].as_str().map(str::to_string);
        if sync_error.is_some() && sync_error != self.sync_error {
            self.publish(Update::Error(sync_error.clone().expect("checked")));
        }
        self.sync_error = sync_error;
        let Some(imports) = imports else { return };
        let version = imports.version;
        if let Err(error) = self.apply(&key, renderer, described, imports).await {
            self.view.failure = Some(version);
            self.view.phase = LivePhase::Failed {
                error,
                nothing_shown: self.view.descriptor.is_none(),
            };
        }
    }

    async fn apply(
        &mut self,
        key: &ViewKey,
        renderer: Arc<dyn Renderer>,
        described: Value,
        imports: mcbench_world::live_views::Imports,
    ) -> Result<(), String> {
        let fresh = imports.fresh;
        // A world placed afresh starts over: one that stopped plays again.
        if fresh && matches!(self.view.running, Running::Stopped(_)) {
            self.view.running = Running::Playing;
        }
        let moved =
            self.view.descriptor.as_ref().map(|d| &d["bounds"]) != Some(&described["bounds"]);
        let first = self.view.descriptor.is_none();
        if imports.block_count == 0 {
            // `showEmpty`: nothing to place and nothing to tick.
            self.view.clock = None;
            self.view.restarting = false;
            self.view.held = None;
            self.view.shown_version = Some(imports.version);
            self.view.ticks = 0;
            self.upload(
                key,
                &renderer,
                empty_blueprint(),
                &described,
                imports.version,
                first,
                moved,
            )
            .await?;
            self.view.descriptor = Some(described);
            self.view.blocks = None;
            self.view.phase = LivePhase::Shown;
            return Ok(());
        }
        let commands = PlaceCommands(imports.commands);
        let world = self.engine_world()?;
        let same_mask = self
            .view
            .descriptor
            .as_ref()
            .is_some_and(|d| d["mask"] == described["mask"]);
        if !fresh && self.view.on_show() && same_mask {
            if !commands.0.is_empty() {
                let ticked = match world.edit(&commands).await {
                    Ok(ticked) => ticked,
                    Err(error) => return Err(self.failed(error).await),
                };
                self.view.descriptor = Some(described);
                self.view.shown_version = Some(imports.version);
                self.receive(ticked, None, moved.then_some(Framing::Fly(FLY_MS)))?;
            }
        } else if fresh || !commands.0.is_empty() {
            if fresh {
                self.view.held = None;
                self.view.restarting = false;
            }
            let placed = if fresh {
                self.place_afresh(&world, &commands).await
            } else {
                match world.place(&commands, false).await {
                    Ok(placed) => Ok(placed),
                    Err(error) => Err(self.failed(error).await),
                }
            }?;
            let inputs = match world.base_inputs().await {
                Ok(inputs) => inputs,
                Err(error) => return Err(self.failed(error).await),
            };
            let blocks = LiveBlocks::load(&placed.0, &inputs)?;
            self.upload(
                key,
                &renderer,
                placed.0,
                &described,
                imports.version,
                first,
                moved,
            )
            .await?;
            self.view.descriptor = Some(described);
            self.view.blocks = Some(blocks);
        }
        if fresh {
            self.view.ticks = 0;
        }
        self.view.held = Some(imports.version);
        self.view.shown_version = Some(imports.version);
        // A world placed afresh runs (a paused one stays paused).
        if fresh && self.view.running == Running::Playing {
            self.view.clock = Some((self.clock.now(), 0));
        }
        self.view.phase = LivePhase::Shown;
        Ok(())
    }

    /// `placeAfresh`: a fresh placement holds no player, so a standing one
    /// is spawned again where its latest view has it, with the keys and look
    /// it last held; a placement or respawn that fails lets it go.
    async fn place_afresh(
        &mut self,
        world: &EngineWorld,
        commands: &PlaceCommands,
    ) -> Result<Blueprint, String> {
        let standing = self.view.player.take();
        self.view.respawning = false;
        let placed = match world.place(commands, true).await {
            Ok(placed) => placed,
            Err(error) => {
                let message = error.to_string();
                self.stop(&message);
                return Err(message);
            }
        };
        if let Some(standing) = standing {
            let v = standing.view;
            self.spawn([v[0], v[1], v[2]], v[9] as f32, v[10] as f32, v[17] == 1.0)
                .await?;
            if standing.input_given {
                let player = self.view.player.as_mut().expect("spawned");
                player.held = Held {
                    taps: 0,
                    ..standing.held
                };
                player.input_given = true;
            }
        }
        Ok(placed)
    }

    /// Keeps a placement's blueprint in the renderer and shows it.
    #[allow(clippy::too_many_arguments)]
    async fn upload(
        &mut self,
        key: &ViewKey,
        renderer: &Arc<dyn Renderer>,
        blueprint: Vec<u8>,
        described: &Value,
        version: u64,
        first: bool,
        moved: bool,
    ) -> Result<(), String> {
        let name = format!("live:{}", key.0);
        let base = (self.uploaded.as_deref() == Some(name.as_str())).then(|| (name.clone(), false));
        let upload = WorldUpload {
            name: name.clone(),
            y_offset: self.y_offset,
            blueprint,
            entities: None,
            base,
        };
        let kept = Arc::clone(renderer);
        tokio::task::spawn_blocking(move || kept.world(upload))
            .await
            .expect("the renderer's world does not panic")
            .map_err(render_text)?;
        self.uploaded = Some(name.clone());
        let [min, max] = bounds_of(described)
            .map(|b| raised(b, self.y_offset))
            .unwrap_or([[0, self.y_offset, 0]; 2]);
        renderer.live_show(Some(Shown {
            world: name,
            view: key.clone(),
            revision: version,
            bounds: Clip { min, max },
            framing: if first {
                Framing::AtOnce
            } else if moved {
                Framing::Fly(FLY_MS)
            } else {
                Framing::Keep
            },
        }));
        Ok(())
    }

    /// A tick's or an edit's change, to the blocks and the renderer.
    fn receive(
        &mut self,
        ticked: Ticked,
        view: Option<[f64; 28]>,
        framing: Option<Framing>,
    ) -> Result<(), String> {
        let blocks = self.view.blocks.as_mut().expect("a world on show");
        blocks.apply(&ticked.changes, &ticked.inputs)?;
        if let Some(Ok(renderer)) = &self.renderer {
            renderer.live_patch(LivePatch {
                view: self.key.clone().expect("a view on show"),
                revision: self.view.shown_version.expect("a version on show"),
                tick: self.view.ticks,
                changes: ticked.changes,
                entities: ticked.entities,
                player: view.map(|v| PlayerView::decode(&v).expect("the engine's player view")),
                framing,
            });
        }
        Ok(())
    }

    /// One tick of the world, with its player if it has one
    /// (`core-worker.mjs` `worldTick` and `frame`); a failed tick stops the
    /// world. True when it ticked.
    async fn world_tick(&mut self) -> bool {
        let world = Arc::clone(self.world.as_ref().expect("a world on show"));
        let ticked = match self.view.player.as_mut() {
            Some(player) => {
                let held = &mut player.held;
                let keys = held.keys | held.taps;
                held.taps = 0;
                let (yaw, pitch) = (held.yaw, held.pitch);
                world
                    .player_tick(keys, yaw, pitch)
                    .await
                    .map(|(ticked, view)| (ticked, Some(view)))
            }
            None => world.tick(1, true).await.map(|ticked| (ticked, None)),
        };
        let (ticked, view) = match ticked {
            Ok(ticked) => ticked,
            Err(error) => {
                self.stop(&error.to_string());
                self.emit();
                return false;
            }
        };
        self.view.ticks += 1;
        if let Err(error) = self.receive(ticked, view, None) {
            self.stop(&error);
            self.emit();
            return false;
        }
        if let Some(view) = view {
            self.player_ticked(view).await;
        }
        self.emit_soon();
        true
    }

    /// A player tick's view: kept, and a fall below [`VOID_Y`] respawned
    /// where the player last spawned (`bluemap-bridge.js` `playerTicked`).
    async fn player_ticked(&mut self, view: [f64; 28]) {
        let Some(player) = self.view.player.as_mut() else {
            return;
        };
        player.view = view;
        if view[1] < VOID_Y && !self.view.respawning {
            self.view.respawning = true;
            let ((at, flying), (yaw, pitch)) = (player.spawn, player.look);
            self.publish(Update::PlayerNote(FELL_OUT.into()));
            if let Err(error) = self.spawn(at, yaw, pitch, flying).await {
                self.publish(Update::PlayerNote(error));
            }
        } else if view[1] >= VOID_Y {
            self.view.respawning = false;
        }
    }

    /// Puts the creative player in the world, holding nothing (the worker's
    /// `player` request with `spawn`).
    async fn spawn(
        &mut self,
        at: [f64; 3],
        yaw: f32,
        pitch: f32,
        flying: bool,
    ) -> Result<(), String> {
        let world = self.engine_world()?;
        let answered = world
            .call(move |world| {
                world.spawn_player(at, yaw, pitch, flying)?;
                world.player_view()
            })
            .await
            .and_then(|view| view);
        let view = match answered {
            Ok(view) => view,
            Err(error) => return Err(self.failed(error).await),
        };
        self.view.player = Some(Standing {
            view,
            held: Held {
                keys: 0,
                taps: 0,
                yaw,
                pitch,
            },
            input_given: false,
            spawn: ([view[0], view[1], view[2]], view[17] == 1.0),
            look: (view[9] as f32, view[10] as f32),
        });
        Ok(())
    }

    fn run_to(&mut self, running: Running) {
        if !self.view.on_show() {
            self.publish(Update::Error(NO_WORLD.into()));
            self.emit();
            return;
        }
        if self.view.running != running {
            if matches!(self.view.running, Running::Stopped(_)) {
                let to = if running == Running::Playing {
                    "playing"
                } else {
                    "paused"
                };
                self.publish(Update::Error(format!(
                    "The live world cannot go from stopped to {to}"
                )));
                self.emit();
                return;
            }
            self.view.running = running;
        }
        self.view.clock = (self.view.running == Running::Playing).then(|| (self.clock.now(), 0));
        self.emit();
    }

    async fn step(&mut self) {
        let refused = if !self.view.on_show() {
            Some(NO_WORLD)
        } else {
            match self.view.running {
                Running::Stopped(_) => Some("The live world cannot step while stopped"),
                Running::Playing => Some("Pause the live world to step it"),
                Running::Paused => None,
            }
        };
        match refused {
            Some(refused) => self.publish(Update::Error(refused.into())),
            None => {
                self.world_tick().await;
            }
        }
        self.emit();
    }

    async fn viewer(&mut self, input: ViewerInput) {
        match input {
            ViewerInput::Click {
                view, block, yaw, ..
            } => self.click(view, block, yaw).await,
            ViewerInput::EnterPlayer {
                view,
                target,
                rotation,
                ..
            } => {
                if !self.player_view_on_show(&view) {
                    return;
                }
                let [min, max] = raised(
                    bounds_of(self.view.descriptor.as_ref().expect("on show"))
                        .expect("a world with blocks has bounds"),
                    self.y_offset,
                );
                let (near, look) = beside_build(min, max, target, rotation);
                self.begin_spawn();
                let spot = match self.engine_world() {
                    Ok(world) => match world.standing_spot(min, max, near).await {
                        Ok(spot) => Ok(spot),
                        Err(error) => Err(self.failed(error).await),
                    },
                    Err(error) => Err(error),
                };
                let spawned = match spot {
                    Ok(spot) => {
                        let [x, y, z] = spot.unwrap_or(near);
                        let (dx, dy, dz) = (look[0] - x, look[1] - (y + EYE_HEIGHT), look[2] - z);
                        let yaw = (-dx).atan2(dz) * 180.0 / std::f64::consts::PI;
                        let pitch = -dy.atan2(dx.hypot(dz)) * 180.0 / std::f64::consts::PI;
                        self.spawn([x, y, z], yaw as f32, pitch as f32, spot.is_none())
                            .await
                    }
                    Err(error) => Err(error),
                };
                self.end_spawn(spawned);
            }
            ViewerInput::SpawnPlayer {
                view,
                at,
                yaw,
                pitch,
                flying,
            } => {
                if !self.player_view_on_show(&view) {
                    return;
                }
                self.begin_spawn();
                let spawned = self.spawn(at, yaw, pitch, flying).await;
                self.end_spawn(spawned);
            }
            ViewerInput::PlayerInput {
                keys,
                taps,
                yaw,
                pitch,
            } => {
                if let Some(player) = self.view.player.as_mut() {
                    player.held = Held {
                        keys,
                        taps: player.held.taps | taps,
                        yaw,
                        pitch,
                    };
                    player.input_given = true;
                    player.look = (yaw, pitch);
                }
            }
            ViewerInput::LeavePlayer => {
                if self.view.player.take().is_some()
                    && let Some(world) = self.world.clone()
                    && !world.spent().await.unwrap_or(true)
                    && let Err(error) = world.remove_player().await
                {
                    let message = self.failed(error).await;
                    self.publish(Update::Error(message));
                }
                self.view.respawning = false;
                self.emit();
            }
        }
    }

    /// A spawn's view is the one on show, with a world: else the note.
    fn player_view_on_show(&mut self, view: &ViewKey) -> bool {
        if self.key.as_ref() == Some(view) && self.view.on_show() {
            return true;
        }
        self.publish(Update::PlayerNote(NO_WORLD.into()));
        self.emit();
        false
    }

    fn begin_spawn(&mut self) {
        if self.view.player.is_none() {
            self.view.spawning = true;
            self.emit();
        }
    }

    fn end_spawn(&mut self, spawned: Result<(), String>) {
        self.view.spawning = false;
        if let Err(error) = spawned {
            self.publish(Update::PlayerNote(error));
        }
        self.emit();
    }

    async fn click(&mut self, view: ViewKey, block: [i32; 3], yaw: f32) {
        let clicked = |name: String, input: Option<&str>| Update::Clicked {
            view: view.clone(),
            block,
            name,
            input: input.map(str::to_string),
        };
        let (Some(blocks), true) = (&self.view.blocks, self.key.as_ref() == Some(&view)) else {
            self.publish(clicked(String::new(), None));
            return;
        };
        let (id, input) = blocks.at(block);
        let names = &self
            .sources
            .as_ref()
            .expect("a world on show has sources")
            .names;
        let Some(name) = names.get(id).cloned() else {
            self.publish(Update::Error(format!(
                "The block table names no block {id}"
            )));
            self.publish(clicked(String::new(), None));
            return;
        };
        if input.is_none() {
            self.publish(clicked(name, None));
            return;
        }
        let world = Arc::clone(self.world.as_ref().expect("a world on show"));
        match world.click(block, yaw).await {
            Ok(taken) => self.publish(clicked(name, taken)),
            Err(error) => {
                let message = self.failed(error).await;
                self.publish(Update::Error(message));
                self.publish(clicked(name, None));
                if matches!(self.view.running, Running::Stopped(_)) {
                    self.emit();
                }
            }
        }
    }

    /// `exportWorld`'s save: the engine's chunks, its game time, and the
    /// spawn above the build's middle.
    async fn save(&mut self) -> Result<WorldSave, String> {
        if !self.view.on_show() {
            return Err(NO_WORLD.into());
        }
        let [min, max] = bounds_of(self.view.descriptor.as_ref().expect("on show"))
            .expect("a world with blocks has bounds");
        let world = Arc::clone(self.world.as_ref().expect("a world on show"));
        let bytes = match world.export().await {
            Ok(bytes) => bytes,
            Err(error) => return Err(self.failed(error).await),
        };
        Ok(WorldSave {
            export: EngineExport::read(&bytes)?,
            spawn: [
                js_round((min[0] + max[0]) as f64 / 2.0) as i32,
                959.min(max[1] + self.y_offset + 8),
                js_round((min[2] + max[2]) as f64 / 2.0) as i32,
            ],
        })
    }

    /// `ExportWorld`: the save of the view on show as a Minecraft world's
    /// zip (`mcbench_world::export::world_zip`, the page's `worldZip`) at one
    /// moment in the machine's time zone, written beside `to` and renamed
    /// over it. An empty world, like no world, is `NO_WORLD`, as the page's
    /// `shownEngine` refuses it.
    async fn export_world(
        &mut self,
        view: &ViewKey,
        name: String,
        to: PathBuf,
    ) -> Result<PathBuf, String> {
        if self.key.as_ref() != Some(view) {
            return Err(NO_WORLD.into());
        }
        let save = self.save().await?;
        let now = self.core.store().host().now_ms();
        let times = ExportTimes::at(now, (self.utc_offset)(now.div_euclid(1000)));
        tokio::task::spawn_blocking(move || {
            let zip = world_zip(&name, &save.export, save.spawn, &times)?;
            write_beside(&to, &zip.bytes).map(|()| to)
        })
        .await
        .expect("a world's export does not panic")
    }
}

impl Core {
    /// The world on show as the engine saves it, with its spawn: what
    /// `ExportWorld` zips.
    pub async fn live_world_save(&self) -> Result<WorldSave, String> {
        self.0.live.save().await
    }
}

/// Writes `bytes` to `to` through a temporary file beside it renamed over
/// it, so a failed write leaves nothing under `to`'s name and no
/// temporary file.
pub fn write_beside(to: &std::path::Path, bytes: &[u8]) -> Result<(), String> {
    let folder = to
        .parent()
        .ok_or_else(|| format!("{} is not in a folder", to.display()))?;
    let name = to
        .file_name()
        .ok_or_else(|| format!("{} names no file", to.display()))?;
    let mut temporary = std::ffi::OsString::from(".");
    temporary.push(name);
    temporary.push(format!(".{}.part", std::process::id()));
    let temporary = folder.join(temporary);
    let written = std::fs::write(&temporary, bytes)
        .and_then(|()| std::fs::rename(&temporary, to))
        .map_err(|e| format!("{}: {e}", to.display()));
    if written.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    written
}

/// The machine's offset from UTC at `at` (seconds since 1970).
#[cfg(unix)]
pub fn local_offset(at: i64) -> i32 {
    let time: libc::time_t = at as libc::time_t;
    // SAFETY: localtime_r writes the broken-down time into `tm`, which is
    // ours, and reads only `time`.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    let done = unsafe { libc::localtime_r(&time, &mut tm) };
    assert!(!done.is_null(), "localtime_r cannot convert {at}");
    tm.tm_gmtoff as i32
}

/// The machine's offset from UTC now (Windows gives the zone's rule in
/// force now, which is the moment an export asks for).
#[cfg(windows)]
pub fn local_offset(_at: i64) -> i32 {
    use windows_sys::Win32::System::Time::{GetTimeZoneInformation, TIME_ZONE_INFORMATION};
    // SAFETY: GetTimeZoneInformation fills the structure, which is ours.
    let mut info: TIME_ZONE_INFORMATION = unsafe { std::mem::zeroed() };
    let id = unsafe { GetTimeZoneInformation(&mut info) };
    // 2 is TIME_ZONE_ID_DAYLIGHT: daylight time is in force now.
    let bias = info.Bias
        + if id == 2 {
            info.DaylightBias
        } else {
            info.StandardBias
        };
    -bias * 60
}
