// The renderer in-process: mcbench_render::Renderer (see_build's worlds,
// shots and cards, as the helper draws them: serve.rs and this module share
// store.rs) and the live view (mcbench_render::live), on one render thread
// that owns the device.
//
// The render thread does uploads and draws only. Meshing and lighting run
// on worker threads: a world's tiles on the scene's meshing threads
// (scene.rs tiles_in_order), a tick's patch and the tiles it answers on
// rayon's (world.rs apply); JPEG encoding on rayon's. Requests that change
// what is kept or draw shots (world, shots, drop) and the live world's show
// run in arrival order; the pane's inputs (size, camera, overlays, time of
// day, picks, takes) are applied as they arrive, also while a request runs:
// between two shots, and between the buffers of a scene being made, the
// render thread takes the pane's inputs and draws a live frame that is due,
// so the live view waits at most one shot's draw. The world shown is kept
// in the store like any other, pinned (store.rs Store::pinned).
//
// Patches stay in order: at most one batch is on rayon's threads at a time,
// and the next is sent only once the last one's tiles are written into the
// scene on the render thread; a batch is every patch waiting then, in
// arrival order. While a request holds the store, patches wait and a batch
// that comes back waits; both go on after it.
//
// Pacing (mcbench_render::live, How a frame reaches the screen): a frame is
// drawn when the view changed, no frame of the pane's is on the GPU, the
// pane is visible and its slot is empty. A drawn frame's rows are copied out
// of its ring slot into the slot and the waker is called. `take` empties
// the slot and wakes the render thread. An idle pane draws nothing.

pub mod camera;
pub mod entities;
pub mod night;
pub mod view;
pub mod world;

use crate::assets::{Assets, Paths};
use crate::gpu::{Choice, DeviceError, Draws, Failure, Gpu, Renderer as Shots, COLOR};
use crate::picture::{self, Font};
use crate::store::{self, Lit, Store};
use camera::{Camera, Lens};
use mcbench_render::live::{
    CameraInput, CameraState, Frame, Framing, LiveFacts, LivePane, LivePatch, LiveRenderer,
    Overlays, PaneSize, PaneStatus, Pick, Shown, TimeOfDay, ViewKey,
};
use mcbench_render::{
    Adapter, AdapterChoice, AdapterReport, Cell, Clip, Limits, Picture, PictureFormat, RenderError,
    WorldReport, WorldUpload,
};
use serde_json::Value;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use view::{InFlight, View};
use world::{Current, LiveWorld, Waiting};

// The sentence of LiveFacts::floor on a software adapter (the renderer's
// answers to the live view's seam, 6).
pub const FLOOR: &str = "No graphics hardware the app can use was found, so the processor draws the 3D view, at half resolution and without smoothed edges.";

// Today's bound on each step of a screenshot (server/shot.js:47,
// STAGE_TIMEOUT_MS): a request the render thread has not answered this long
// after its step began is answered `stalled` and the render thread is left
// behind (Draw::leave), as the server kills a helper that stops answering
// (shot.js:222-226).
pub const STAGE: Duration = Duration::from_secs(120);

// How often a waiting `shots` call passes the caller's cancel on to its job.
const CANCEL_POLL: Duration = Duration::from_millis(5);

// What a step that passed the bound answers: server/shot.js:224, word for
// word (`${STAGE_TIMEOUT_MS / 1000}`: JavaScript's and Rust's shortest
// forms of the seconds agree).
pub fn stalled(label: &str, bound: Duration) -> String {
    format!(
        "Screenshot {label} stalled for {}s; renderer reset, screenshot unavailable",
        bound.as_millis() as f64 / 1000.0
    )
}

// The pane's sentence once its render thread was left behind.
pub fn pane_stalled(bound: Duration) -> String {
    format!(
        "The 3D view stopped: the renderer did not answer a request for {} s.",
        bound.as_millis() as f64 / 1000.0
    )
}

// When the step a request waits on began, and its name in today's words
// (server/shot.js stage labels: `initialization` for a renderer made for
// the request, `world loading` for a world, `capture` for shots). The
// caller's bound runs from it; the render thread moves it on as it begins
// each step.
struct Stage(Mutex<(Instant, &'static str)>);

impl Stage {
    fn new(label: &'static str) -> Stage {
        Stage(Mutex::new((Instant::now(), label)))
    }
    fn begin(&self, label: &'static str) {
        *self.0.lock().unwrap() = (Instant::now(), label);
    }
    fn get(&self) -> (Instant, &'static str) {
        *self.0.lock().unwrap()
    }
}

// What a batch of patches gives back from rayon's threads: the shown
// world's lit world and blueprint, and what the batch made.
struct Returned {
    lit: Lit,
    current: Current,
    result: Result<world::Patched, String>,
}

enum Job {
    World(
        WorldUpload,
        Arc<Stage>,
        Sender<Result<WorldReport, RenderError>>,
    ),
    Shots {
        world: String,
        shots: Vec<store::Shot>,
        supersample: u32,
        quality: Option<u8>,
        // The job's own flag, which the waiting call sets from the caller's
        // (Draw::request): a job left behind never reads the caller's.
        cancel: Arc<AtomicBool>,
        stage: Arc<Stage>,
        reply: Sender<Result<Vec<Picture>, RenderError>>,
    },
    Drop(String, Arc<Stage>, Sender<Result<bool, RenderError>>),
    Show(Option<Shown>),
    Patch(LivePatch),
    Resize(PaneSize),
    Visible(bool),
    Camera(CameraInput),
    Overlays(Overlays),
    Time(TimeOfDay),
    Pick(u64, [f32; 2], Box<dyn FnOnce(Pick) + Send>),
    Taken,
    // A batch of patches back from rayon's threads, for the shown world of
    // that generation.
    Patched(u64, Box<Returned>),
    LoseDevice,
    // The camera's terrain height at x, z (Draw::terrain).
    Terrain(f64, f64, Sender<Option<f64>>),
    // What the renderer holds, by part (Draw::accounting).
    Account(Sender<Accounting>),
    // The render thread blocked for this long (Draw::hold).
    Hold(Duration),
    Quit,
}

impl Job {
    fn ordered(&self) -> bool {
        matches!(
            self,
            Job::World(..) | Job::Shots { .. } | Job::Drop(..) | Job::Show(..)
        )
    }

    // A request's step marker and the name of its step.
    fn stage(&self) -> Option<(&Stage, &'static str)> {
        match self {
            Job::World(_, stage, _) => Some((stage, "world loading")),
            Job::Shots { stage, .. } => Some((stage, "capture")),
            Job::Drop(_, stage, _) => Some((stage, "drop")),
            _ => None,
        }
    }

    // A request answered with `error` instead of being run.
    fn refuse(self, error: RenderError) {
        match self {
            Job::World(_, _, reply) => drop(reply.send(Err(error))),
            Job::Shots { reply, .. } => drop(reply.send(Err(error))),
            Job::Drop(_, _, reply) => drop(reply.send(Err(error))),
            _ => unreachable!("only requests are refused"),
        }
    }
}

// What the pane measured, for the timings and the tests.
#[derive(Clone, Debug, Default)]
pub struct Stats {
    // Frames delivered.
    pub frames: u64,
    // Per frame: from the draw's start to its rows mapped, and the copy of
    // the rows out of the ring.
    pub draw_ms: Vec<f64>,
    pub readback_ms: Vec<f64>,
    // Per pick: from its start to its answer.
    pub pick_ms: Vec<f64>,
    // Per batch of patches: the tiles World::patch answered and were meshed
    // again, and the patches in the batch.
    pub meshed: Vec<Vec<(i32, i32)>>,
    pub batches: Vec<usize>,
    // Per batch: World::patch and the meshing (rayon's threads), and the
    // writing of the tiles' quads (the render thread).
    pub apply_ms: Vec<f64>,
    pub mesh_ms: Vec<f64>,
    pub install_ms: Vec<f64>,
    // Times the device was made again after a loss.
    pub recreated: u32,
    // The last 256 frames' seq and the positions their tick-list entities
    // were drawn at.
    pub entities: VecDeque<(u64, Vec<[f64; 3]>)>,
}

// What the renderer holds, by part, in bytes (Draw::accounting): the
// store's own count (store.rs Store::counted, as a request's end counts
// it) and what it is made of, and the parts outside the store.
#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct Accounting {
    // How the store counts the GPU's memory (store.rs Counting).
    pub counting: String,
    // Store::counted with the live view's share, as a request's end counts.
    pub counted: u64,
    // The kept worlds' blueprints and entity lists.
    pub worlds: u64,
    // The scenes: on the CPU (tiles' blocks, blended quads, the translucent
    // order, the lit world) and their quads on the GPU.
    pub scenes_cpu: u64,
    pub scenes_gpu: u64,
    // The lit worlds alone (part of scenes_cpu).
    pub lit: u64,
    // The live view's readback ring (mapped, in the process's memory) and
    // its targets (on the GPU).
    pub ring: u64,
    pub targets: u64,
    // The atlases on the GPU, and their mip levels still held on the CPU.
    pub atlas_gpu: u64,
    pub atlas_cpu: u64,
    // The entity capture loaded (its atlas on the CPU counted above).
    pub entity_capture: bool,
    // see_build's picture targets kept.
    pub picture_targets: u64,
    // The device's allocator's reserved blocks (Vulkan, Direct3D 12, Metal).
    pub device: Option<u64>,
}

pub struct Pane {
    tx: Sender<Job>,
    slot: Mutex<Option<Frame>>,
    status: Mutex<PaneStatus>,
    camera: Mutex<CameraState>,
    facts: LiveFacts,
    waker: Mutex<Option<Box<dyn Fn() + Send + Sync>>>,
    stats: Mutex<Stats>,
    // Its render thread was left behind (Draw::leave): the pane's calls
    // and the thread's writes to it do nothing.
    left: AtomicBool,
}

impl Pane {
    fn new(tx: Sender<Job>, camera: CameraState, facts: LiveFacts) -> Pane {
        Pane {
            tx,
            slot: Mutex::new(None),
            status: Mutex::new(PaneStatus::Drawing),
            camera: Mutex::new(camera),
            facts,
            waker: Mutex::new(None),
            stats: Mutex::new(Stats::default()),
            left: AtomicBool::new(false),
        }
    }

    fn wake(&self) {
        if let Some(wake) = &*self.waker.lock().unwrap() {
            wake();
        }
    }

    // The renderer gone, a pane's calls do nothing.
    fn send(&self, job: Job) {
        if !self.left.load(Ordering::SeqCst) {
            let _ = self.tx.send(job);
        }
    }

    // The render thread's word on the pane, unless it was left behind.
    fn set_status(&self, status: PaneStatus) {
        let mut held = self.status.lock().unwrap();
        if !self.left.load(Ordering::SeqCst) {
            *held = status;
        }
        drop(held);
        self.wake();
    }

    fn deliver(&self, frame: Frame) {
        let mut slot = self.slot.lock().unwrap();
        if !self.left.load(Ordering::SeqCst) {
            *slot = Some(frame);
        }
    }

    // The render thread left behind: the pane stopped with `sentence` for
    // good, whatever the thread does if it ever goes on.
    fn leave(&self, sentence: &str) {
        let mut status = self.status.lock().unwrap();
        if !self.left.swap(true, Ordering::SeqCst) {
            *status = PaneStatus::Failed(sentence.to_string());
        }
        drop(status);
        *self.slot.lock().unwrap() = None;
        self.wake();
    }

    pub fn stats(&self) -> Stats {
        self.stats.lock().unwrap().clone()
    }
}

impl LivePane for Pane {
    fn set_waker(&self, wake: Box<dyn Fn() + Send + Sync>) {
        *self.waker.lock().unwrap() = Some(wake);
    }
    fn resize(&self, size: PaneSize) {
        self.send(Job::Resize(size));
    }
    fn visible(&self, visible: bool) {
        self.send(Job::Visible(visible));
    }
    fn camera(&self, input: CameraInput) {
        self.send(Job::Camera(input));
    }
    fn overlays(&self, overlays: Overlays) {
        self.send(Job::Overlays(overlays));
    }
    fn time_of_day(&self, time: TimeOfDay) {
        self.send(Job::Time(time));
    }
    fn pick(&self, seq: u64, at: [f32; 2], answer: Box<dyn FnOnce(Pick) + Send>) {
        self.send(Job::Pick(seq, at, answer));
    }
    fn take(&self) -> Option<Frame> {
        let frame = self.slot.lock().unwrap().take();
        if frame.is_some() {
            self.send(Job::Taken);
        }
        frame
    }
    fn status(&self) -> PaneStatus {
        self.status.lock().unwrap().clone()
    }
    fn camera_state(&self) -> CameraState {
        *self.camera.lock().unwrap()
    }
    fn facts(&self) -> LiveFacts {
        self.facts.clone()
    }
}

// The renderer the core draws with: its calls are jobs for the render
// thread, answered in order.
//
// The render thread is today's helper process. A request it does not
// answer within the bound (STAGE) is answered `stalled` and the thread is
// left behind: the Draw lets go of it and the next request starts a fresh
// one, which makes its device, assets and store from nothing and holds no
// world (server/shot.js stage and leave, then currentRun's startRun). A
// thread cannot be killed: one left behind is told to quit, ends at once if
// it ever goes on (its replies go nowhere, its pane is stopped, it reads no
// caller's memory: a `shots` job has its own cancel flag), and holds its
// device and memory until then.
pub struct Draw {
    report: AdapterReport,
    pane: Arc<Pane>,
    // What a fresh render thread is made from.
    start: Start,
    // The render thread; none after one was left behind, until the next
    // request starts another.
    current: Mutex<Option<Running>>,
    // Render threads started, for their ids.
    started: AtomicU64,
    shut: AtomicBool,
}

struct Start {
    paths: Paths,
    choice: Choice,
    memory_limit: u64,
    bound: Duration,
}

struct Running {
    id: u64,
    tx: Sender<Job>,
    // Joined at `shutdown`.
    thread: std::thread::JoinHandle<()>,
}

// What `world`, `shots` and `drop_world` answer after `shutdown`.
pub const SHUT_DOWN: &str = "The renderer was shut down";

struct Ready {
    assets: Arc<Assets>,
    gpu: Gpu,
    renderer: Shots,
    font: Arc<Font>,
    night: Arc<night::Night>,
}

// A delivered frame's camera, kept for picks: its seq and world, its lens,
// the pane's size it was drawn for and the yaw a click uses.
#[derive(Clone)]
struct KeptFrame {
    seq: u64,
    view: ViewKey,
    lens: Lens,
    pane: [u32; 2],
    yaw: f32,
}

// A frame on the GPU and what its Frame will carry.
struct Drawing {
    gpu: InFlight,
    kept: KeptFrame,
    revision: u64,
    anchors: Vec<Option<[f32; 2]>>,
    camera: CameraState,
    time: TimeOfDay,
    entities: Vec<[f64; 3]>,
}

// A frame's entities: the tick lists' solid quads on the GPU, the quads
// drawn behind the blocks, and where the tick lists' entities are drawn.
type Placed = (
    Option<crate::gpu::Batches>,
    Vec<crate::scene::Instance>,
    Vec<[f64; 3]>,
);

// The tick lists' entities of the shown world, once a patch has carried
// some: where they glide, and the light they read on the way.
struct Gliding {
    tracker: entities::Tracker,
    lights: entities::Cells,
}

struct Live {
    pane: Arc<Pane>,
    view: View,
    camera: Camera,
    shown: Option<LiveWorld>,
    generation: u64,
    size: Option<PaneSize>,
    visible: bool,
    time: TimeOfDay,
    overlays: Overlays,
    changed: bool,
    drawing: Option<Drawing>,
    seq: u64,
    kept: VecDeque<KeptFrame>,
    patches: Vec<Waiting>,
    patching: bool,
    // Batches that came back while a request held the store.
    held: Vec<(u64, Box<Returned>)>,
    gliding: Option<Gliding>,
    failed: bool,
    software: bool,
    tx: Sender<Job>,
    // A device lost during a request, recovered once it has answered.
    lost: Option<String>,
    // The renderer stopped (a second lost device): nothing shows again.
    dead: bool,
}

struct Thread {
    rx: Receiver<Job>,
    choice: Choice,
    paths: Paths,
    memory_limit: u64,
    r: Ready,
    store: Store,
    queue: VecDeque<Job>,
    live: Live,
    recreated: bool,
    // The second loss's sentence (or the renderer's own failure): the live
    // view stops for good, and the next request makes the device and the
    // store again from nothing (Thread::remake).
    dead: Option<String>,
    quit: bool,
}

// What a turn of the live view may do with the store: draw from it, or
// also write patches into the shown world's scene.
enum Access<'a> {
    Read(&'a Store),
    Write(&'a mut Store),
}

impl Access<'_> {
    fn store(&self) -> &Store {
        match self {
            Access::Read(s) => s,
            Access::Write(s) => s,
        }
    }
}

fn failure(f: Failure) -> RenderError {
    match f {
        Failure::Device(e) => RenderError::DeviceLost(format!("The graphics device was lost: {e}")),
        Failure::Cancelled => RenderError::Cancelled,
        Failure::Other(e) if e.starts_with("No world named") => RenderError::UnknownWorld(e),
        Failure::Other(e) => RenderError::Failed(e),
    }
}

fn adapter_report(gpu: &Gpu, store: &Store) -> AdapterReport {
    let a = gpu.report();
    let limits = gpu.device.limits();
    AdapterReport {
        version: crate::VERSION.to_string(),
        adapter: Adapter {
            name: a.name,
            backend: a.backend,
            device_type: a.device_type,
            driver: a.driver,
            driver_info: a.driver_info,
            software: a.software,
        },
        limits: Limits {
            max_texture_side: limits.max_texture_dimension_2d,
            max_buffer_bytes: limits.max_buffer_size,
            max_picture_side: limits.max_texture_dimension_2d,
            worlds_kept: store.limits.worlds as u32,
            scenes_kept: store.limits.scenes as u32,
            memory_limit: store.limits.bytes,
        },
    }
}

fn paths_of(assets: &mcbench_render::Assets) -> Paths {
    Paths {
        pack: assets.pack.clone(),
        textures: assets.textures.clone(),
        atlas: assets.atlas.clone(),
        atlas_map: assets.atlas_map.clone(),
        block_table: assets.block_table.clone(),
        render_layers: assets.render_layers.clone(),
        entities: assets.entities.clone(),
        client_jar: assets.client_jar.clone(),
        night: Some(assets.night.clone()),
        software_vulkan: assets.software_vulkan.clone(),
    }
}

// What a fresh helper makes at `hello` (serve.rs Worker::hello), with the
// live view's night: the device on `choice`'s adapter, the assets, the
// pipelines and the font. Any failure is the start's own sentence, as the
// helper answers it (serve.rs answer: `error` is Gpu::open's or the
// assets' sentence as it is). The wait for the GPU at its end is given
// apart: a device lost while starting is a lost device, which the helper
// answers `deviceLost` and the server answers by starting another.
fn make(paths: &Paths, choice: &Choice) -> Result<(Ready, Result<(), DeviceError>), String> {
    let gpu = Gpu::open(choice)?;
    let mut assets = Assets::load(paths)?;
    let night_path = paths.night.as_ref().ok_or(
        "The live view draws Minecraft's night from night.json, and the renderer was given none",
    )?;
    let night = night::read(night_path, &paths.client_jar)?;
    let renderer = Shots::new(&gpu, &assets.atlas, COLOR);
    // The atlas's pixels are the GPU's from here (serve.rs lets them go
    // the same way).
    assets.atlas.levels = Vec::new();
    let waited = gpu.wait();
    let font = Arc::new(Font::new()?);
    Ok((
        Ready {
            assets: Arc::new(assets),
            gpu,
            renderer,
            font,
            night: Arc::new(night),
        },
        waited,
    ))
}

// A render thread started: it makes its device and assets (make) and
// answers its jobs until told to quit. `stopped`: its live view never
// draws (a thread started after one was left behind: the window's pane was
// the first thread's). A start that has not answered within the bound is
// answered `stalled` and left behind: it ends once it is made, unseen.
fn spawn(
    start: &Start,
    id: u64,
    stopped: bool,
) -> Result<(Running, AdapterReport, Arc<Pane>), RenderError> {
    let (tx, rx) = mpsc::channel::<Job>();
    let (ready_tx, ready_rx) = mpsc::channel::<Result<(AdapterReport, Arc<Pane>), RenderError>>();
    let thread_tx = tx.clone();
    let (paths, choice, memory_limit) = (
        start.paths.clone(),
        start.choice.clone(),
        start.memory_limit,
    );
    let thread = std::thread::Builder::new()
        .name("mcbench-render".into())
        .spawn(move || {
            let (r, waited) = match make(&paths, &choice) {
                Ok(made) => made,
                Err(e) => {
                    let _ = ready_tx.send(Err(RenderError::NoAdapter(e)));
                    return;
                }
            };
            let software = r.gpu.report().software;
            let mut store = Store::new(store::Limits::new(Some(memory_limit), software));
            let report = adapter_report(&r.gpu, &store);
            let facts = LiveFacts {
                floor: software.then(|| FLOOR.to_string()),
            };
            let camera = Camera::new();
            let pane = Arc::new(Pane::new(thread_tx.clone(), camera.state(1.0), facts));
            let view = View::new(&r.gpu, &r.renderer, software, &r.night);
            store.in_process(&r.gpu);
            let live = Live {
                pane: pane.clone(),
                view,
                camera,
                shown: None,
                generation: 0,
                size: None,
                visible: true,
                time: TimeOfDay::Day,
                overlays: Overlays::default(),
                changed: false,
                drawing: None,
                seq: 0,
                kept: VecDeque::new(),
                patches: Vec::new(),
                patching: false,
                held: Vec::new(),
                gliding: None,
                failed: stopped,
                software,
                tx: thread_tx,
                // A device lost while starting is made again at once, as
                // any lost device (Thread::turn).
                lost: waited.err().map(|e| e.0),
                dead: stopped,
            };
            let mut t = Thread {
                rx,
                choice,
                paths,
                memory_limit,
                r,
                store,
                queue: VecDeque::new(),
                live,
                recreated: false,
                dead: None,
                quit: false,
            };
            // A start answered `stalled` meanwhile: nobody waits for this one.
            if ready_tx.send(Ok((report, pane))).is_ok() {
                t.run();
            }
            t.end();
        })
        .map_err(|e| RenderError::Failed(format!("The render thread could not start: {e}")))?;
    let (report, pane) = match ready_rx.recv_timeout(start.bound) {
        Ok(ready) => ready?,
        Err(mpsc::RecvTimeoutError::Timeout) => {
            return Err(RenderError::Stalled(stalled("initialization", start.bound)))
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            return Err(RenderError::Failed(
                "The render thread stopped while starting".into(),
            ))
        }
    };
    Ok((Running { id, tx, thread }, report, pane))
}

impl Draw {
    // The render thread started on `choice`'s adapter with the assets of
    // `paths`, keeping at most `memory_limit` bytes between requests, each
    // step of a request bounded by `bound` (the app's: STAGE).
    pub fn open(
        paths: Paths,
        choice: Choice,
        memory_limit: u64,
        bound: Duration,
    ) -> Result<Draw, RenderError> {
        let start = Start {
            paths,
            choice,
            memory_limit,
            bound,
        };
        let (running, report, pane) = spawn(&start, 0, false)?;
        Ok(Draw {
            report,
            pane,
            start,
            current: Mutex::new(Some(running)),
            started: AtomicU64::new(1),
            shut: AtomicBool::new(false),
        })
    }

    // What the pane measured (tests and timings).
    pub fn stats(&self) -> Stats {
        self.pane.stats()
    }

    // The render thread's channel.
    fn tx(&self) -> Sender<Job> {
        self.current
            .lock()
            .unwrap()
            .as_ref()
            .expect("the render thread runs")
            .tx
            .clone()
    }

    // For tests: the device destroyed, as a driver reset loses it.
    pub fn lose_device(&self) {
        self.tx()
            .send(Job::LoseDevice)
            .expect("the render thread runs");
    }

    // For tests: the render thread blocked for `time`, as a driver call
    // that hangs blocks it.
    pub fn hold(&self, time: Duration) {
        self.tx()
            .send(Job::Hold(time))
            .expect("the render thread runs");
    }

    // The height the map camera's target follows at x, z of the shown world
    // (BlueMap's terrainHeightAt with the bridge's floor; tests and tools).
    pub fn terrain(&self, x: f64, z: f64) -> Option<f64> {
        self.ask(|reply| Job::Terrain(x, z, reply))
            .expect("the renderer runs")
    }

    // What the renderer holds, by part (tests and tools).
    pub fn accounting(&self) -> Accounting {
        self.ask(Job::Account).expect("the renderer runs")
    }

    // Whether a render thread runs (tests: none after `shutdown`).
    pub fn running(&self) -> bool {
        self.current
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|r| !r.thread.is_finished())
    }

    // A question for the render thread (tests and tools), unbounded; after
    // `shutdown`, its sentence.
    fn ask<T>(&self, job: impl FnOnce(Sender<T>) -> Job) -> Result<T, RenderError> {
        if self.shut.load(Ordering::SeqCst) {
            return Err(RenderError::Failed(SHUT_DOWN.into()));
        }
        let (tx, rx) = mpsc::channel();
        self.tx().send(job(tx)).expect("the render thread runs");
        Ok(rx.recv().expect("the render thread answers"))
    }

    // A request (world, shots, drop) answered by the render thread, each
    // of its steps bounded (Stage); after `shutdown`, its sentence. With no
    // render thread (one was left behind) a fresh one is started first, its
    // start bounded as today's `initialization`. `cancel`, the caller's
    // flag, is passed on to the job's own while the call waits.
    fn request<T>(
        &self,
        label: &'static str,
        cancel: Option<&AtomicBool>,
        job: impl FnOnce(Arc<Stage>, Arc<AtomicBool>, Sender<Result<T, RenderError>>) -> Job,
    ) -> Result<T, RenderError> {
        if self.shut.load(Ordering::SeqCst) {
            return Err(RenderError::Failed(SHUT_DOWN.into()));
        }
        let stage = Arc::new(Stage::new(label));
        let flag = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel();
        // The render thread, or a fresh one started without holding the
        // lock (live_show and live_patch return at once meanwhile); of two
        // started at once one is kept and the other told to quit.
        let (thread, id) = loop {
            if let Some(running) = &*self.current.lock().unwrap() {
                break (running.tx.clone(), running.id);
            }
            let id = self.started.fetch_add(1, Ordering::SeqCst);
            let (fresh, _, _) = spawn(&self.start, id, true)?;
            stage.begin(label);
            let mut current = self.current.lock().unwrap();
            match &*current {
                None => *current = Some(fresh),
                Some(_) => drop(fresh.tx.send(Job::Quit)),
            }
        };
        thread
            .send(job(stage.clone(), flag.clone(), tx))
            .expect("the render thread runs");
        loop {
            if cancel.is_some_and(|c| c.load(Ordering::SeqCst)) {
                flag.store(true, Ordering::SeqCst);
            }
            let (since, label) = stage.get();
            let left = (since + self.start.bound).saturating_duration_since(Instant::now());
            if left.is_zero() {
                self.leave(id);
                return Err(RenderError::Stalled(stalled(label, self.start.bound)));
            }
            let wait = if cancel.is_some() {
                left.min(CANCEL_POLL)
            } else {
                left
            };
            match rx.recv_timeout(wait) {
                Ok(answer) => return answer,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    panic!("the render thread answers")
                }
            }
        }
    }

    // The render thread `id` left behind, as the server kills a helper
    // whose step stalled (shot.js leave): told to quit (it ends at once if
    // it ever goes on), its handle let go, the window's pane stopped. The
    // next request starts a fresh one. Another caller that waited on it
    // and stalled too finds it gone.
    fn leave(&self, id: u64) {
        let left = {
            let mut current = self.current.lock().unwrap();
            match &*current {
                Some(running) if running.id == id => current.take(),
                _ => None,
            }
        };
        // Outside the lock: the pane's waker is the window's.
        if let Some(running) = left {
            let _ = running.tx.send(Job::Quit);
            self.pane.leave(&pane_stalled(self.start.bound));
        }
    }

    // A job for the render thread, dropped after `shutdown` and while no
    // render thread runs.
    fn send(&self, job: Job) {
        if self.shut.load(Ordering::SeqCst) {
            return;
        }
        if let Some(running) = &*self.current.lock().unwrap() {
            running.tx.send(job).expect("the render thread runs");
        }
    }

    // mcbench_render::Renderer::shutdown: the thread told to quit and
    // joined; it waits for the GPU and lets the device go as it ends
    // (Thread::end). A thread left behind is not waited for.
    // Whether the thread ended without a panic.
    fn stop(&self) -> bool {
        self.shut.store(true, Ordering::SeqCst);
        let running = self.current.lock().unwrap().take();
        running.is_none_or(|r| {
            let _ = r.tx.send(Job::Quit);
            r.thread.join().is_ok()
        })
    }
}

impl Drop for Draw {
    fn drop(&mut self) {
        self.stop();
    }
}

impl LiveRenderer for Draw {
    fn live(&self) -> Arc<dyn LivePane> {
        self.pane.clone()
    }
    fn live_show(&self, shown: Option<Shown>) {
        self.send(Job::Show(shown));
    }
    fn live_patch(&self, patch: LivePatch) {
        self.send(Job::Patch(patch));
    }
}

impl mcbench_render::Renderer for Draw {
    fn start(
        assets: &mcbench_render::Assets,
        adapter: AdapterChoice,
        memory_limit: u64,
    ) -> Result<Draw, RenderError> {
        let paths = paths_of(assets);
        let choice = Choice {
            software: adapter == AdapterChoice::Software,
            software_vulkan: paths.software_vulkan.clone(),
        };
        Draw::open(paths, choice, memory_limit, STAGE)
    }

    fn report(&self) -> &AdapterReport {
        &self.report
    }

    fn world(&self, upload: WorldUpload) -> Result<WorldReport, RenderError> {
        self.request("world loading", None, |stage, _, reply| {
            Job::World(upload, stage, reply)
        })
    }

    fn shots(
        &self,
        world: &str,
        shots: &[mcbench_render::Shot],
        supersample: u32,
        format: PictureFormat,
        cancel: &AtomicBool,
    ) -> Result<Vec<Picture>, RenderError> {
        // The seam's shots carry the helper's fields and JSON names: read as
        // the helper reads a shots request's `shots` field, with its
        // sentence (serve.rs take: `shots: <serde's reason>`). A view named
        // after a property of Object.prototype gives today's camera of NaN
        // numbers (server/shot.js:367-370), written null, and answers so.
        let shots = serde_json::to_value(shots)
            .and_then(serde_json::from_value::<Vec<store::Shot>>)
            .map_err(|e| RenderError::Failed(format!("shots: {e}")))?;
        let quality = match format {
            PictureFormat::Jpeg(q) if (1..=100).contains(&q) => Some(q),
            PictureFormat::Jpeg(_) => {
                return Err(RenderError::Failed(
                    "format.quality must be 1 to 100".into(),
                ))
            }
            PictureFormat::Png => None,
        };
        self.request("capture", Some(cancel), |stage, cancel, reply| Job::Shots {
            world: world.to_string(),
            shots,
            supersample,
            quality,
            cancel,
            stage,
            reply,
        })
    }

    fn montage(
        &self,
        size: [u32; 2],
        cells: &[Cell],
        jpegs: &[&[u8]],
        quality: u8,
    ) -> Result<Vec<u8>, RenderError> {
        // No GPU: drawn on the caller's thread.
        let cells: Vec<picture::Cell> = cells
            .iter()
            .map(|c| picture::Cell {
                x: c.x,
                y: c.y,
                width: c.width,
                height: c.height,
            })
            .collect();
        if !(1..=100).contains(&quality) {
            return Err(RenderError::Failed("quality must be 1 to 100".into()));
        }
        picture::montage(jpegs, &cells, size, quality).map_err(RenderError::Failed)
    }

    fn drop_world(&self, world: &str) -> Result<bool, RenderError> {
        self.request("drop", None, |stage, _, reply| {
            Job::Drop(world.to_string(), stage, reply)
        })
    }

    fn shutdown(&self) {
        assert!(
            self.stop(),
            "the render thread panicked before the shutdown"
        );
    }
}

impl Thread {
    fn run(&mut self) {
        // A device lost while starting is made again before any job.
        self.turn();
        while !self.quit {
            if let Some(job) = self.queue.pop_front() {
                self.ordered(job);
                self.turn();
                continue;
            }
            let job = if let Some(drawing) = &self.live.drawing {
                match self.rx.try_recv() {
                    Ok(job) => Some(job),
                    Err(TryRecvError::Empty) => {
                        // The frame's GPU work waited for a millisecond at a time.
                        let _ = self.r.gpu.device.poll(wgpu::PollType::Wait {
                            submission_index: Some(drawing.gpu.submission.clone()),
                            timeout: Some(std::time::Duration::from_millis(1)),
                        });
                        None
                    }
                    Err(TryRecvError::Disconnected) => return,
                }
            } else {
                match self.rx.recv() {
                    Ok(job) => Some(job),
                    Err(_) => return,
                }
            };
            if let Some(job) = job {
                dispatch(
                    &mut self.live,
                    &self.r,
                    &mut self.queue,
                    &mut self.quit,
                    &mut Access::Write(&mut self.store),
                    job,
                );
            }
            self.turn();
        }
    }

    // The thread's end: no frame left on the GPU, its work waited for, then
    // the device let go with everything else the thread owns, before the
    // process exits (mcbench_render::Renderer::shutdown).
    fn end(mut self) {
        self.live.drop_drawing();
        let _ = self.r.gpu.wait();
    }

    // Every input waiting taken, the pane pumped; a lost device made again.
    fn turn(&mut self) {
        turn(
            &mut self.live,
            &self.rx,
            &self.r,
            &mut self.queue,
            &mut self.quit,
            &mut Access::Write(&mut self.store),
        );
        if self.dead.is_some() {
            return;
        }
        let health = self.r.gpu.health.lock().unwrap();
        let (lost, error) = (health.lost.clone(), health.error.clone());
        drop(health);
        if let Some(reason) = lost.or(self.live.lost.take()) {
            self.recover(&reason);
            // The shown world drawn again on the new device.
            turn(
                &mut self.live,
                &self.rx,
                &self.r,
                &mut self.queue,
                &mut self.quit,
                &mut Access::Write(&mut self.store),
            );
        } else if let Some(error) = error {
            // A wgpu validation error is a bug of the renderer.
            let sentence = format!("The renderer failed: {error}");
            self.live.dead = true;
            self.live.fail(&sentence);
            self.dead = Some(sentence);
        }
    }

    // The shown world brought up to date in the store before a request
    // reads it: every patch received applied (a batch on rayon's threads
    // waited for), and its blueprint with them written back.
    fn settle_shown(&mut self) {
        loop {
            self.live.install_held(&self.r, &mut self.store);
            if !self.live.patching && !self.live.patches.is_empty() {
                self.live.send_batch(&self.r, &mut self.store);
            }
            if !self.live.patching {
                break;
            }
            match self.rx.recv() {
                Ok(job) => dispatch(
                    &mut self.live,
                    &self.r,
                    &mut self.queue,
                    &mut self.quit,
                    &mut Access::Write(&mut self.store),
                    job,
                ),
                Err(_) => return,
            }
        }
        if let Some(shown) = &mut self.live.shown {
            if shown.stale {
                let current = shown
                    .current
                    .as_ref()
                    .expect("the blueprint is home once no batch is out");
                self.store
                    .worlds
                    .get_mut(&shown.name)
                    .expect("the shown world is kept")
                    .blueprint = current.bytes();
                shown.stale = false;
            }
        }
    }

    // A lost device made again once from the renderer's own copies: the
    // assets read again from their files, every kept world's scene let go
    // (meshed again from its blueprint by the next shots), the shown world
    // meshed again from its lit world with every patch applied. A second
    // loss stops the live view with its sentence, and the next request
    // makes the renderer again from nothing (remake).
    fn recover(&mut self, reason: &str) {
        let sentence = format!("The graphics device was lost: {reason}");
        if self.recreated {
            self.live.dead = true;
            self.live.fail(&sentence);
            self.dead = Some(sentence);
            return;
        }
        self.recreated = true;
        self.settle_shown();
        let shown = self.live.shown.as_ref().map(|s| s.name.clone());
        let old = shown
            .as_ref()
            .and_then(|name| self.store.scenes.remove(&(name.clone(), None)));
        self.store.scenes.clear();
        let made = (|| -> Result<(), Failure> {
            let gpu = Gpu::open(&self.choice)?;
            let mut assets = Assets::load(&self.paths)?;
            let renderer = Shots::new(&gpu, &assets.atlas, COLOR);
            assets.atlas.levels = Vec::new();
            self.live.drawing = None;
            self.live.kept.clear();
            self.live.view = View::new(&gpu, &renderer, self.live.software, &self.r.night);
            if let (Some(name), Some(mut old)) = (shown, old) {
                let lit = old.lit.take().expect("the shown world keeps its lit world");
                let kept = &self.store.worlds[&name];
                let mut scene =
                    world::remesh(&old, &lit, &kept.entities, kept.y_offset, &assets, &gpu)?;
                drop(old);
                scene.lit = Some(lit);
                if !scene.entities.is_empty() || self.live.gliding.is_some() {
                    renderer.entity_atlas(&gpu, &assets.entity()?.atlas);
                }
                self.store.scenes.insert((name, None), scene);
            }
            gpu.wait()?;
            self.r.gpu = gpu;
            self.r.renderer = renderer;
            self.r.assets = Arc::new(assets);
            Ok(())
        })();
        match made {
            Ok(()) => {
                self.live.changed = true;
                self.live.pane.stats.lock().unwrap().recreated += 1;
            }
            Err(e) => {
                let e = match e {
                    Failure::Device(e) | Failure::Other(e) => e,
                    Failure::Cancelled => unreachable!("nothing cancels a recreate"),
                };
                let sentence = format!("{sentence}, and making it again failed: {e}");
                self.live.dead = true;
                self.live.fail(&sentence);
                self.dead = Some(sentence);
            }
        }
    }

    // The renderer made again from nothing, as a fresh helper's `hello`
    // makes it: the device, the assets and pipelines (make), an empty
    // store (a fresh helper holds no world). The live view stays stopped:
    // the window shows the second loss's sentence (the renderer's answers
    // to the live view's seam, 4). A start that fails answers its sentence
    // and the renderer stays stopped, for the next request to try again.
    fn remake(&mut self) -> Result<(), RenderError> {
        let (r, waited) = make(&self.paths, &self.choice).map_err(RenderError::NoAdapter)?;
        if let Err(e) = waited {
            return Err(RenderError::DeviceLost(format!(
                "The graphics device was lost: {}",
                e.0
            )));
        }
        // No batch of patches out; the shown world let go with the store.
        self.settle_shown();
        self.live.unshow(&mut self.store);
        self.live.held.clear();
        self.live.kept.clear();
        self.live.lost = None;
        let software = r.gpu.report().software;
        self.live.software = software;
        self.live.view = View::new(&r.gpu, &r.renderer, software, &r.night);
        self.store = Store::new(store::Limits::new(Some(self.memory_limit), software));
        self.store.in_process(&r.gpu);
        self.r = r;
        self.recreated = false;
        self.dead = None;
        Ok(())
    }

    fn ordered(&mut self, job: Job) {
        // A request after the renderer stopped (a second lost device) is
        // answered by a renderer made again from nothing, as the server
        // answers the call after a helper's exit with a fresh helper
        // (shot.js leave, then currentRun's startRun and its `hello`, the
        // step `initialization`).
        if let Some((stage, label)) = job.stage() {
            if self.dead.is_some() {
                stage.begin("initialization");
                if let Err(e) = self.remake() {
                    job.refuse(e);
                    return;
                }
            }
            stage.begin(label);
        }
        let shown = self.live.shown.as_ref().map(|s| s.name.clone());
        let touches = |name: &str| shown.as_deref() == Some(name);
        let reads_shown = match &job {
            Job::World(upload, ..) => {
                touches(&upload.name) || upload.base.as_ref().is_some_and(|(b, _)| touches(b))
            }
            Job::Shots { world, .. } => touches(world),
            Job::Show(_) => true,
            _ => false,
        };
        if reads_shown {
            self.settle_shown();
        }
        let replaced = match &job {
            Job::World(upload, ..) if touches(&upload.name) => Some(upload.name.clone()),
            _ => None,
        };
        {
            let Thread {
                live,
                rx,
                r,
                queue,
                quit,
                store,
                ..
            } = self;
            let mut between =
                |store: &Store| turn(live, rx, r, queue, quit, &mut Access::Read(store));
            match job {
                Job::World(upload, _, reply) => {
                    let result = (|| -> Result<(WorldReport, u64), Failure> {
                        let entities: Vec<Value> = match &upload.entities {
                            Some(bytes) => serde_json::from_slice(bytes)
                                .map_err(|e| format!("The entities are not a JSON list: {e}"))?,
                            None => Vec::new(),
                        };
                        let (base, keep_base) = match upload.base {
                            Some((base, keep)) => (Some(base), keep),
                            None => (None, false),
                        };
                        let made = store::keep_world(
                            store,
                            &r.assets,
                            &r.gpu,
                            &r.renderer,
                            store::Upload {
                                name: upload.name,
                                y_offset: upload.y_offset,
                                blueprint: upload.blueprint,
                                entities,
                                base,
                                keep_base,
                            },
                            &mut between,
                        )?;
                        let report = WorldReport {
                            quads: made.quads as u64,
                            entity_quads: made.entity_quads as u64,
                            tiles: made.tiles as u64,
                            blocks: made.blocks,
                            bounds: Clip {
                                min: made.bounds.0,
                                max: made.bounds.1,
                            },
                            bytes: made.bytes,
                            missing_textures: made.missing,
                            tiles_meshed: made.meshed.map(|n| n as u64),
                        };
                        Ok((report, made.used))
                    })();
                    match result {
                        Ok((report, used)) => {
                            let _ = reply.send(Ok(report));
                            // The new scene stays for the shots that follow.
                            // What it leaves let go by the store's counts;
                            // freed memory given back once no request waits
                            // (a device error is answered by the health check
                            // after the job).
                            let room = store::Room {
                                gpu: &r.gpu,
                                renderer: &r.renderer,
                                extra: match store.counting {
                                    store::Counting::Readbacks => {
                                        live.view.held_bytes(&r.gpu, false)
                                    }
                                    store::Counting::Device => 0,
                                    store::Counting::Resources => {
                                        live.view.held_bytes(&r.gpu, true)
                                    }
                                },
                            };
                            store.shed(used, false, &room);
                            if queue.is_empty() {
                                let _ = store.release(&room);
                            }
                        }
                        Err(e) => {
                            if let Failure::Device(reason) = &e {
                                live.lost = Some(reason.clone());
                            }
                            let _ = reply.send(Err(failure(e)));
                        }
                    }
                }
                Job::Shots {
                    world,
                    shots,
                    supersample,
                    quality,
                    cancel,
                    reply,
                    ..
                } => {
                    let cancelled = || cancel.load(Ordering::SeqCst);
                    match store::draw_shots(
                        store,
                        &r.assets,
                        &r.gpu,
                        &r.renderer,
                        &r.font,
                        &world,
                        &shots,
                        supersample,
                        quality,
                        &cancelled,
                        &mut between,
                    ) {
                        Ok((_, pictures, used)) => {
                            let pictures = shots
                                .iter()
                                .zip(pictures)
                                .map(|(s, bytes)| Picture {
                                    width: s.width,
                                    height: s.height,
                                    bytes,
                                })
                                .collect();
                            let _ = reply.send(Ok(pictures));
                            // What it leaves let go by the store's counts;
                            // freed memory given back once no request waits
                            // (a device error is answered by the health check
                            // after the job).
                            let room = store::Room {
                                gpu: &r.gpu,
                                renderer: &r.renderer,
                                extra: match store.counting {
                                    store::Counting::Readbacks => {
                                        live.view.held_bytes(&r.gpu, false)
                                    }
                                    store::Counting::Device => 0,
                                    store::Counting::Resources => {
                                        live.view.held_bytes(&r.gpu, true)
                                    }
                                },
                            };
                            store.shed(used, true, &room);
                            if queue.is_empty() {
                                let _ = store.release(&room);
                            }
                        }
                        Err(e) => {
                            if let Failure::Device(reason) = &e {
                                live.lost = Some(reason.clone());
                            }
                            let _ = reply.send(Err(failure(e)));
                        }
                    }
                }
                Job::Drop(name, _, reply) => {
                    if store.pinned.as_deref() == Some(name.as_str()) {
                        let _ = reply.send(Err(RenderError::Failed(format!(
                            "The world {name} is shown in the live view and is kept while it is"
                        ))));
                        return;
                    }
                    let dropped = store.drop_world(&name);
                    if let Err(e) = r.gpu.wait() {
                        live.lost = Some(e.0);
                    }
                    let _ = reply.send(Ok(dropped));
                }
                Job::Show(shown) => live.show(store, r, shown),
                Job::Patch(patch) => live.patch(patch),
                _ => unreachable!("only ordered jobs are queued"),
            }
        }
        // A new revision of the shown world under its name: its blueprint
        // read again; a shown world the request let go is shown no more.
        if let Some(shown) = &mut self.live.shown {
            match self.store.worlds.get(&shown.name) {
                Some(kept) if replaced.as_deref() == Some(shown.name.as_str()) => {
                    match Current::read(&kept.blueprint) {
                        Ok(current) => {
                            shown.current = Some(current);
                            shown.stale = false;
                            self.live.changed = true;
                        }
                        Err(e) => self.live.fail(&e),
                    }
                }
                Some(_) => {}
                None => {
                    self.live.shown = None;
                    self.store.pinned = None;
                }
            }
        }
    }
}

// The pane's inputs and the live world's patches taken as they arrive;
// requests queued in order.
fn dispatch(
    live: &mut Live,
    r: &Ready,
    queue: &mut VecDeque<Job>,
    quit: &mut bool,
    access: &mut Access,
    job: Job,
) {
    match job {
        // A patch waits behind a show queued before it (it is the shown
        // world's after that show), else it waits for the next batch.
        Job::Patch(patch) => {
            if queue.iter().any(|j| matches!(j, Job::Show(..))) {
                queue.push_back(Job::Patch(patch));
            } else {
                live.patch(patch);
            }
        }
        job if job.ordered() => queue.push_back(job),
        Job::Resize(size) => {
            live.size = Some(size);
            live.camera.resize([size.width, size.height]);
            live.changed = true;
        }
        Job::Visible(visible) => {
            live.visible = visible;
            live.changed |= visible;
        }
        Job::Camera(input) => {
            let now = crate::now_ms();
            let store = access.store();
            let hit = match &input {
                CameraInput::DoubleClick { seq, at } => {
                    live.pick_point(r, store, *seq, *at).map(|(point, _)| point)
                }
                _ => None,
            };
            let lost = std::cell::RefCell::new(None);
            let Live {
                camera,
                view,
                shown,
                ..
            } = live;
            let terrain = |x: f64, z: f64| terrain(view, r, store, shown.as_ref(), &lost, x, z);
            camera.input(&input, now, hit, &terrain);
            if let Some(e) = lost.into_inner() {
                live.lost = Some(e);
            }
            live.changed = true;
        }
        Job::Overlays(overlays) => {
            live.overlays = overlays;
            live.changed = true;
        }
        Job::Time(time) => {
            live.time = time;
            live.changed = true;
        }
        Job::Pick(seq, at, answer) => {
            let t = crate::now_ms();
            let pick = live.pick(r, access.store(), seq, at);
            live.pane
                .stats
                .lock()
                .unwrap()
                .pick_ms
                .push(crate::now_ms() - t);
            answer(pick);
        }
        Job::Taken => {}
        Job::Patched(generation, returned) => {
            live.patching = false;
            live.held.push((generation, returned));
            if let Access::Write(store) = access {
                live.install_held(r, store);
            }
        }
        Job::LoseDevice => r.gpu.device.destroy(),
        Job::Terrain(x, z, reply) => {
            let lost = std::cell::RefCell::new(None);
            let _ = reply.send(terrain(
                &live.view,
                r,
                access.store(),
                live.shown.as_ref(),
                &lost,
                x,
                z,
            ));
            if let Some(e) = lost.into_inner() {
                live.lost = Some(e);
            }
        }
        Job::Account(reply) => {
            let _ = reply.send(account(live, r, access.store()));
        }
        Job::Hold(time) => std::thread::sleep(time),
        Job::Quit => *quit = true,
        _ => unreachable!("every job is matched"),
    }
}

fn turn(
    live: &mut Live,
    rx: &Receiver<Job>,
    r: &Ready,
    queue: &mut VecDeque<Job>,
    quit: &mut bool,
    access: &mut Access,
) {
    while let Ok(job) = rx.try_recv() {
        dispatch(live, r, queue, quit, access, job);
    }
    if let Access::Write(store) = access {
        live.install_held(r, store);
        if !live.patching && !live.patches.is_empty() {
            live.send_batch(r, store);
        }
    }
    if let Err(e) = live.pump(r, access.store()) {
        live.lost = Some(e.0);
    }
}

fn account(live: &Live, r: &Ready, store: &Store) -> Accounting {
    let all = live.view.held_bytes(&r.gpu, true);
    let ring = live.view.held_bytes(&r.gpu, false);
    let room = store::Room {
        gpu: &r.gpu,
        renderer: &r.renderer,
        extra: match store.counting {
            store::Counting::Readbacks => ring,
            store::Counting::Device => 0,
            store::Counting::Resources => all,
        },
    };
    let atlas_cpu =
        |a: &crate::assets::Atlas| a.levels.iter().map(|l| l.capacity() as u64).sum::<u64>();
    let entity = r.assets.entity_assets();
    Accounting {
        counting: format!("{:?}", store.counting),
        counted: store.counted(&room),
        worlds: store.worlds.values().map(store::Kept::bytes).sum(),
        scenes_cpu: store.scenes.values().map(|s| s.cpu_bytes()).sum(),
        scenes_gpu: store.scenes.values().map(|s| s.gpu_bytes()).sum(),
        lit: store
            .scenes
            .values()
            .filter_map(|s| s.lit.as_ref())
            .map(|l| store::lit_bytes(&l.world))
            .sum(),
        ring,
        targets: all - ring,
        atlas_gpu: r.renderer.atlas_bytes(),
        atlas_cpu: atlas_cpu(&r.assets.atlas) + entity.map_or(0, |e| atlas_cpu(&e.atlas)),
        entity_capture: entity.is_some(),
        picture_targets: r.renderer.kept_targets_bytes(&r.gpu, true),
        device: r.gpu.memory(),
    }
}

// BlueMap's map.terrainHeightAt over the shown world (View::height: a ray
// down against the drawn tile under x, z), with the bridge's floor within
// the build's footprint: at least its middle height less 3
// (web/bluemap-view.mjs:127-136). Minecraft's coordinates.
fn terrain(
    view: &View,
    r: &Ready,
    store: &Store,
    shown: Option<&LiveWorld>,
    lost: &std::cell::RefCell<Option<String>>,
    x: f64,
    z: f64,
) -> Option<f64> {
    let shown = shown?;
    let scene = store.scenes.get(&(shown.name.clone(), None))?;
    let y = store.worlds[&shown.name].y_offset;
    let at = (
        mcbench_renderer::mesh::cell(x.floor() as i32),
        mcbench_renderer::mesh::cell(z.floor() as i32),
    );
    let mut ranges: Vec<(&wgpu::Buffer, u32, u32)> = Vec::new();
    if let Some(&(b, first, n)) = scene.arena.ranges.get(&at) {
        ranges.push((&scene.arena.batches.buffers[b].0, first, n));
    }
    if let Some(t) = &scene.translucent {
        ranges.extend(t.tile_runs(at));
    }
    let height = if ranges.is_empty() {
        None
    } else {
        match view.height(&r.gpu, &r.renderer, &ranges, y, x, z) {
            Ok(h) => h,
            Err(e) => {
                *lost.borrow_mut() = Some(e.0);
                None
            }
        }
    };
    let b = shown.bounds;
    if x < b.min[0] as f64
        || x > b.max[0] as f64 + 1.0
        || z < b.min[2] as f64
        || z > b.max[2] as f64 + 1.0
    {
        return height;
    }
    Some(
        height
            .unwrap_or(0.0)
            .max((b.min[1] + b.max[1] + 1) as f64 / 2.0 - 3.0),
    )
}

impl Live {
    // A frame on the GPU given up: its ring slot's mapping cancelled, so the
    // slot can be drawn into again.
    fn drop_drawing(&mut self) {
        if let Some(d) = self.drawing.take() {
            self.view.release(&d.gpu);
        }
    }

    fn fail(&mut self, sentence: &str) {
        self.failed = true;
        self.drop_drawing();
        self.pane
            .set_status(PaneStatus::Failed(sentence.to_string()));
    }

    // The frame size for a pane: half its width and height on a software
    // adapter.
    fn frame_size(&self, pane: PaneSize) -> [u32; 2] {
        if self.software {
            [(pane.width / 2).max(1), (pane.height / 2).max(1)]
        } else {
            [pane.width.max(1), pane.height.max(1)]
        }
    }

    // The shown world unpinned: it stays kept, its scene with every patch
    // (the caller settled it: no batch is out and its blueprint is written
    // back).
    fn unshow(&mut self, store: &mut Store) {
        self.drop_drawing();
        self.patches.clear();
        self.gliding = None;
        if self.shown.take().is_some() {
            store.pinned = None;
        }
        self.generation += 1;
    }

    fn show(&mut self, store: &mut Store, r: &Ready, shown: Option<Shown>) {
        self.unshow(store);
        if self.dead {
            return;
        }
        let Some(shown) = shown else {
            self.changed = false;
            return;
        };
        let made = (|| -> Result<LiveWorld, String> {
            let name = &shown.world;
            let kept = store
                .worlds
                .get(name)
                .ok_or_else(|| format!("No world named {name} is kept"))?;
            let key = (name.clone(), None);
            // Its whole scene with its lit world (one whose lit world went to
            // a newer scene is made again).
            if !store.scenes.get(&key).is_some_and(|s| s.lit.is_some()) {
                let (scene, _) = store::make_scene(&r.assets, &r.gpu, kept, None, &mut |_| {})
                    .map_err(|e| match e {
                        Failure::Device(e) | Failure::Other(e) => e,
                        Failure::Cancelled => unreachable!("nothing cancels a scene"),
                    })?;
                store.put_scene(key.clone(), scene);
            }
            let scene = &store.scenes[&key];
            // The renderer's answers, 2: a world that does not fit beside the
            // drawing targets is not drawn.
            let size = self.size.map_or([1, 1], |s| self.frame_size(s));
            let gpu = match store.counting {
                store::Counting::Readbacks => 0,
                _ => scene.gpu_bytes(),
            };
            let needs = scene.cpu_bytes() + gpu + self.view.bytes_at(&r.gpu, size);
            let limit = store.limits.bytes;
            if needs > limit {
                let mb = |b: u64| (b as f64 / 1e6).round() as u64;
                return Err(format!("This world needs about {} MB to draw and the renderer's limit on this computer is {} MB", mb(needs), mb(limit)));
            }
            if !scene.entities.is_empty() {
                r.renderer.entity_atlas(&r.gpu, &r.assets.entity()?.atlas);
            }
            let current = Current::read(&store.worlds[name].blueprint)?;
            store.pinned = Some(name.clone());
            Ok(LiveWorld {
                name: name.clone(),
                view: shown.view.clone(),
                revision: shown.revision,
                tick: 0,
                bounds: shown.bounds,
                current: Some(current),
                stale: false,
            })
        })();
        match made {
            Ok(world) => {
                self.shown = Some(world);
                if self.failed {
                    self.failed = false;
                    self.pane.set_status(PaneStatus::Drawing);
                }
                self.frame(shown.framing, shown.bounds);
                self.changed = true;
            }
            Err(sentence) => self.fail(&sentence),
        }
    }

    fn frame(&mut self, framing: Framing, bounds: Clip) {
        let now = crate::now_ms();
        match framing {
            Framing::AtOnce => self.camera.fly(bounds.min, bounds.max, 0.0, now),
            Framing::Fly(ms) => self.camera.fly(bounds.min, bounds.max, ms as f64, now),
            Framing::Keep => {}
        }
    }

    fn patch(&mut self, patch: LivePatch) {
        let Some(shown) = &mut self.shown else { return };
        if self.failed || patch.view != shown.view {
            return;
        }
        // The box a framing fits and the terrain's floor follow the blocks.
        match world::patch_bounds(&patch.changes) {
            Ok((min, max)) => shown.bounds = Clip { min, max },
            Err(e) => {
                self.fail(&format!(
                    "The live world could not take a tick's changes: {e}"
                ));
                return;
            }
        }
        let now = crate::now_ms();
        if let Some(view) = patch.player {
            self.camera.player(view, patch.tick, now);
            self.changed = true;
        }
        if let Some(framing) = patch.framing {
            let bounds = shown.bounds;
            self.frame(framing, bounds);
            self.changed = true;
        }
        self.patches
            .push((patch.revision, patch.tick, patch.changes, patch.entities));
    }

    // Every patch waiting sent to rayon's threads as one batch, with the
    // shown world's lit world and blueprint.
    fn send_batch(&mut self, r: &Ready, store: &mut Store) {
        if self.failed {
            return;
        }
        let Some(shown) = &mut self.shown else { return };
        let Some(scene) = store.scenes.get_mut(&(shown.name.clone(), None)) else {
            return;
        };
        let (Some(mut lit), Some(mut current)) = (scene.lit.take(), shown.current.take()) else {
            unreachable!("the shown world's lit world and blueprint are home between batches")
        };
        let batch = std::mem::take(&mut self.patches);
        self.pane.stats.lock().unwrap().batches.push(batch.len());
        self.patching = true;
        shown.stale = true;
        let (assets, tx, generation, y) = (
            r.assets.clone(),
            self.tx.clone(),
            self.generation,
            store.worlds[&shown.name].y_offset,
        );
        rayon::spawn(move || {
            let result = world::apply(&mut lit, &mut current, &assets, y, &batch);
            let _ = tx.send(Job::Patched(
                generation,
                Box::new(Returned {
                    lit,
                    current,
                    result,
                }),
            ));
        });
    }

    // The batches back from rayon's threads written into the shown world's
    // scene, with the lit world and blueprint they took, in order.
    fn install_held(&mut self, r: &Ready, store: &mut Store) {
        for (generation, returned) in std::mem::take(&mut self.held) {
            assert_eq!(
                generation, self.generation,
                "a batch is back before the world it was sent for is let go"
            );
            let Returned {
                lit,
                current,
                result,
            } = *returned;
            let shown = self.shown.as_mut().expect("a batch's world is shown");
            let y = store.worlds[&shown.name].y_offset;
            let scene = store
                .scenes
                .get_mut(&(shown.name.clone(), None))
                .expect("the shown world's scene is kept");
            shown.current = Some(current);
            match result {
                Ok(patched) if !self.failed => {
                    let meshed: Vec<(i32, i32)> =
                        patched.meshed.iter().map(|(at, _)| *at).collect();
                    let (apply_ms, mesh_ms) = (patched.apply_ms, patched.mesh_ms);
                    let (revision, tick) = (patched.revision, patched.tick);
                    let entities = patched.entities.clone();
                    let t = crate::now_ms();
                    if let Err(e) = world::install(scene, &r.gpu, &r.assets, y, patched) {
                        scene.lit = Some(lit);
                        self.lost = Some(e.0);
                        continue;
                    }
                    shown.revision = revision;
                    shown.tick = tick;
                    let install_ms = crate::now_ms() - t;
                    // The tick's entities, lit by the world after it.
                    if !entities.is_empty() {
                        if let Err(e) = self.glide(r, &lit, y, &entities) {
                            scene.lit = Some(lit);
                            self.fail(&format!(
                                "The live view could not draw the tick's entities: {e}"
                            ));
                            continue;
                        }
                    }
                    scene.lit = Some(lit);
                    let mut stats = self.pane.stats.lock().unwrap();
                    stats.meshed.push(meshed);
                    stats.apply_ms.push(apply_ms);
                    stats.mesh_ms.push(mesh_ms);
                    stats.install_ms.push(install_ms);
                    drop(stats);
                    self.changed = true;
                }
                Ok(_) => scene.lit = Some(lit),
                Err(sentence) => {
                    scene.lit = Some(lit);
                    self.fail(&sentence);
                }
            }
        }
    }

    // A tick's entities (the engine's list, Minecraft's heights) set to
    // glide to, and their light taken from the world after the tick.
    fn glide(&mut self, r: &Ready, lit: &Lit, y: i32, bytes: &[u8]) -> Result<(), String> {
        let list: Vec<Value> = serde_json::from_slice(bytes)
            .map_err(|e| format!("The entities are not a JSON list: {e}"))?;
        // At the page's heights, as the kept world's (server/shot-world.js:199).
        let list: Vec<Value> = list
            .into_iter()
            .map(|mut e| {
                if let Some(v) = e["pos"][1].as_f64() {
                    e["pos"][1] = serde_json::json!(v - y as f64);
                }
                e
            })
            .collect();
        let gliding = self.gliding.get_or_insert_with(|| Gliding {
            tracker: entities::Tracker::default(),
            lights: Default::default(),
        });
        gliding.tracker.set(list, crate::now_ms())?;
        let capture = &r.assets.entity()?.capture;
        let draw = crate::scene::Draw {
            pack: &r.assets.pack,
            facts: &r.assets.facts,
            y_offset: y,
        };
        gliding.lights = entities::lights(capture, &gliding.tracker, &lit.world, &draw)?;
        r.renderer.entity_atlas(&r.gpu, &r.assets.entity()?.atlas);
        Ok(())
    }

    // A frame delivered when its rows are mapped; a frame drawn when one is
    // due.
    fn pump(&mut self, r: &Ready, store: &Store) -> Result<(), crate::gpu::DeviceError> {
        let _ = r.gpu.device.poll(wgpu::PollType::Poll);
        if self
            .drawing
            .as_ref()
            .is_some_and(|d| d.gpu.mapped.load(Ordering::SeqCst))
        {
            let d = self.drawing.take().unwrap();
            let mapped = crate::now_ms();
            let bgra = self.view.read(&d.gpu)?;
            let read = crate::now_ms();
            let frame = Frame::new(
                d.kept.seq,
                d.kept.view.clone(),
                d.revision,
                d.gpu.size,
                d.anchors,
                d.camera,
                d.time,
                bgra,
            );
            self.pane.deliver(frame);
            *self.pane.camera.lock().unwrap() = d.camera;
            {
                let mut stats = self.pane.stats.lock().unwrap();
                stats.frames += 1;
                stats.draw_ms.push(mapped - d.gpu.started);
                stats.readback_ms.push(read - mapped);
                stats.entities.push_back((d.kept.seq, d.entities));
                while stats.entities.len() > 256 {
                    stats.entities.pop_front();
                }
            }
            self.kept.push_back(d.kept);
            while self.kept.len() > 3 {
                self.kept.pop_front();
            }
            self.pane.wake();
        }
        let now = crate::now_ms();
        let gliding = self.gliding.as_ref().is_some_and(|g| g.tracker.moving(now));
        let due = (self.changed || gliding || self.camera.moving(now))
            && self.visible
            && !self.failed
            && self.drawing.is_none()
            && self.size.is_some()
            && self
                .shown
                .as_ref()
                .is_some_and(|s| store.scenes.contains_key(&(s.name.clone(), None)))
            && self.pane.slot.lock().unwrap().is_none();
        if due {
            self.draw(r, store, now);
        }
        Ok(())
    }

    fn draw(&mut self, r: &Ready, store: &Store, now: f64) {
        let pane = self.size.expect("a size before a draw");
        let size = self.frame_size(pane);
        {
            let lost = std::cell::RefCell::new(None);
            let Live {
                camera,
                view,
                shown,
                ..
            } = self;
            let terrain = |x: f64, z: f64| terrain(view, r, store, shown.as_ref(), &lost, x, z);
            camera.update(now, &terrain);
            if let Some(e) = lost.into_inner() {
                self.lost = Some(e);
            }
        }
        let aspect = pane.width.max(1) as f64 / pane.height.max(1) as f64;
        let lens = self.camera.lens(aspect);
        let state = self.camera.state(aspect);
        let shown = self.shown.as_ref().expect("a shown world before a draw");
        let scene = &store.scenes[&(shown.name.clone(), None)];
        let y = store.worlds[&shown.name].y_offset;
        // see_build's draws (store.rs prepare): the translucent quads ordered
        // and the entities behind for this eye (the page's heights). The tick
        // lists' entities, once a patch carried some, glide where they are.
        let eye = [lens.eye[0], lens.eye[1] - y as f64, lens.eye[2]];
        let forward = [-lens.view[2], -lens.view[6], -lens.view[10]];
        let (yaw, pitch) = (forward[0].atan2(-forward[2]), (-forward[1]).asin());
        let made = (|| -> Result<Placed, String> {
            match &self.gliding {
                Some(g) => {
                    let list = g.tracker.sample(now);
                    let positions = list
                        .iter()
                        .map(|e| [0, 1, 2].map(|i| e["pos"][i].as_f64().unwrap_or(f64::NAN)))
                        .collect();
                    if list.is_empty() {
                        return Ok((None, Vec::new(), positions));
                    }
                    let capture = &r.assets.entity()?.capture;
                    let view = entities::Lights {
                        cells: g.lights.clone(),
                        pack: &r.assets.pack,
                        names: &r.assets.names,
                        lone: Default::default(),
                    };
                    let sorted = crate::entities::build(capture, &list, &view)?;
                    let solid = store::entity_instances(&sorted.solid, &r.assets)?;
                    let behind = store::entity_instances(
                        &crate::entities::behind(capture, &list, &sorted, yaw, pitch, eye)?,
                        &r.assets,
                    )?;
                    Ok((
                        (!solid.is_empty())
                            .then(|| crate::gpu::Batches::upload(&r.gpu, &solid, "entities")),
                        behind,
                        positions,
                    ))
                }
                None if scene.entities.is_empty() => Ok((None, Vec::new(), Vec::new())),
                None => {
                    let capture = &r.assets.entity()?.capture;
                    let behind = store::entity_instances(
                        &crate::entities::behind(
                            capture,
                            &scene.entities,
                            &scene.sorted,
                            yaw,
                            pitch,
                            eye,
                        )?,
                        &r.assets,
                    )?;
                    Ok((None, behind, Vec::new()))
                }
            }
        })();
        let (solid, behind, positions) = match made {
            Ok(m) => m,
            Err(e) => {
                self.fail(&format!("The live view could not draw its entities: {e}"));
                return;
            }
        };
        let shown = self.shown.as_ref().expect("a shown world before a draw");
        let draws = Draws {
            blocks: scene.block_batches(),
            translucent: scene.translucent.as_ref().map(|t| t.ordered(&r.gpu, eye)),
            solid: if self.gliding.is_some() {
                solid.as_ref()
            } else {
                scene.solid.as_ref()
            },
            behind: (!behind.is_empty())
                .then(|| crate::gpu::Batches::upload(&r.gpu, &behind, "behind")),
        };
        let overlays = view::overlay_geometry(&self.overlays, y);
        let anchors = view::project(&lens, &self.overlays.anchors, size);
        self.seq += 1;
        let kept = KeptFrame {
            seq: self.seq,
            view: shown.view.clone(),
            lens,
            pane: [pane.width.max(1), pane.height.max(1)],
            yaw: self.camera.yaw(),
        };
        let gpu = self.view.draw(
            &r.gpu,
            &r.renderer,
            &draws,
            &lens,
            y,
            scene.bounds,
            size,
            &overlays,
            self.time,
        );
        self.drawing = Some(Drawing {
            gpu,
            kept,
            revision: shown.revision,
            anchors,
            camera: state,
            time: self.time,
            entities: positions,
        });
        self.changed = false;
    }

    // The block and face under `at` in frame `seq`, cast with that frame's
    // camera against the shown world's opaque and cutout quads: the hit
    // quad's own block, exact at any coordinate.
    fn pick_point(
        &mut self,
        r: &Ready,
        store: &Store,
        seq: u64,
        at: [f32; 2],
    ) -> Option<([f64; 3], [i8; 3])> {
        let k = self.kept.iter().find(|k| k.seq == seq)?.clone();
        let shown = self.shown.as_ref()?;
        let scene = store.scenes.get(&(shown.name.clone(), None))?;
        let y = store.worlds[&shown.name].y_offset;
        let ndc = [
            at[0] as f64 / k.pane[0] as f64 * 2.0 - 1.0,
            1.0 - at[1] as f64 / k.pane[1] as f64 * 2.0,
        ];
        let hit = match self.view.pick(
            &r.gpu,
            &r.renderer,
            &scene.arena.batches,
            &k.lens,
            y,
            scene.bounds,
            ndc,
            [k.pane[0] as f64, k.pane[1] as f64],
        ) {
            Ok(hit) => hit?,
            Err(e) => {
                self.lost = Some(e.0);
                return None;
            }
        };
        let (buffer, quad, axis) = hit;
        let (&tile, &(_, first, _)) = scene
            .arena
            .ranges
            .iter()
            .find(|(_, &(b, first, n))| b == buffer && quad >= first && quad < first + n)?;
        let packed = scene.geometry[&tile].blocks[(quad - first) as usize];
        let b = crate::scene::unpack_block(packed, tile);
        let n: [i8; 3] = match axis {
            1 => [1, 0, 0],
            2 => [-1, 0, 0],
            3 => [0, 1, 0],
            4 => [0, -1, 0],
            5 => [0, 0, 1],
            _ => [0, 0, -1],
        };
        Some((
            [
                b[0] as f64 + 0.5,
                (b[1] + y) as f64 + 0.5,
                b[2] as f64 + 0.5,
            ],
            n,
        ))
    }

    // A pick's answer (the renderer's answers, 5): the block hit and its
    // face; a seq older than the three frames kept, or the sky, a miss.
    fn pick(&mut self, r: &Ready, store: &Store, seq: u64, at: [f32; 2]) -> Pick {
        let kept = self.kept.iter().find(|k| k.seq == seq).cloned();
        let view = kept
            .as_ref()
            .map(|k| k.view.clone())
            .or_else(|| self.shown.as_ref().map(|s| s.view.clone()))
            .unwrap_or(ViewKey(String::new()));
        let yaw = kept.as_ref().map_or_else(|| self.camera.yaw(), |k| k.yaw);
        let (block, face) = match self.pick_point(r, store, seq, at) {
            Some((centre, face)) => (Some(centre.map(|v| v.floor() as i32)), Some(face)),
            None => (None, None),
        };
        Pick {
            seq,
            view,
            block,
            face,
            yaw,
        }
    }
}
