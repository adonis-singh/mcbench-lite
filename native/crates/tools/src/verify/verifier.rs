//! The engine verifier (`EngineVerifier`, its `Experiments` base) and the
//! pool of lanes it runs on (`VerifierPool`).
//!
//! A lane runs one experiment at a time, in the order they were given
//! (today's promise chain, here a chain of one-shot channels); the pool
//! has as many lanes as the machine's memory holds (`RESERVED_BYTES`,
//! `LANE_BYTES`, `MAX_LANES`) and gives each experiment to the lane with
//! the fewest waiting or running, the first of them on a tie.
//!
//! An experiment runs in an engine child (`mcbench_engine::VerifyEngines`).
//! One cancelled, shut down or past its wall-clock budget while the engine
//! runs it is answered at once, with today's answer, and its child is
//! stopped (`SIGTERM` to its process group), so its lane is free at once
//! and the lane's next experiment starts on a fresh child, as today's next
//! experiment started the lane's engine process again.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use mcbench_engine::{BlockState, EngineError, VerifyReport, VerifyRequest};
use mcbench_world::facts::{LastPass, MinecraftFacts};
use mcbench_world::{Palette, World};
use serde_json::{Map, Value, json};
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;

use super::error::{ABORTED, ENGINE_STOPPED, SHUTTING_DOWN, VerifyError, verification_failure};
use super::js;
use super::request::{Experiment, ItemTags, Settings, check_budget, validate_request};
use super::system::random_uuid;
/// `sweepRuns`: the engine children's folders a process that ended mid-run
/// left ([`mcbench_engine::sweep_children`]).
pub use mcbench_engine::sweep_children as sweep_runs;

/// `MAX_INLINE_BYTES`: a call's result is at most this long inline, and a
/// report under half of it comes back inline with its test.
pub const MAX_INLINE_BYTES: usize = 20_000;
/// `RESERVED_BYTES`: memory kept for the rest of the machine.
pub const RESERVED_BYTES: u64 = 5 << 30;
/// `LANE_BYTES`: an engine at its largest (2.5 GiB).
pub const LANE_BYTES: u64 = 5 << 29;
/// `MAX_LANES`.
pub const MAX_LANES: usize = 3;

/// The note of a finished experiment (`EngineVerifier`'s).
pub const NOTE: &str = "Observed in an isolated Minecraft world, run by an exact port of the pinned Paper server's code. Experiment completion is not a design pass. Initial is before inputs; observations are after inputs at each sample tick.";

/// What the verifier needs of the engine: `VERIFY` and the data fixer's
/// palette, and its engines started and stopped with the verifier.
/// [`mcbench_engine::VerifyEngines`] is it; tests give a fake, as today's
/// tests give `EngineVerifier` a fake engine.
#[async_trait]
pub trait VerifyEngine: Send + Sync + 'static {
    /// Runs one experiment (`EngineVerifier`'s `engine.verify`). When
    /// `cancel` fires the engine running it is stopped and this returns at
    /// once (with [`EngineError::Cancelled`]); the verifier answers the
    /// experiment itself. `progress` is given each new `progress.json`.
    async fn verify(
        &self,
        request: &VerifyRequest,
        progress: &mut (dyn for<'t> FnMut(&'t str) + Send),
        cancel: &CancellationToken,
    ) -> Result<VerifyReport, EngineError>;

    /// Starts `engines` engines, so the first experiments do not wait for
    /// one to boot (`VerifierPool.warm`).
    fn warm(&self, engines: usize) -> Result<(), String>;

    /// Stops the engines no experiment runs on (`EngineVerifier.stop`'s
    /// `engine.stop()`).
    async fn stop(&self);

    /// `sweepRuns`: removes the engine children's folders that a process
    /// which ended mid-run left and that last changed over `age_ms` ago.
    /// The folders are the engine's, so the engine knows where they are.
    /// It blocks on the file system.
    fn sweep(&self, age_ms: f64);

    /// `fixedPalette`: the states the engine places for `entries` written
    /// at `data_version`.
    async fn fixed_palette(
        &self,
        entries: &[BlockState],
        data_version: i64,
    ) -> Result<Vec<BlockState>, String>;
}

#[async_trait]
impl VerifyEngine for mcbench_engine::VerifyEngines {
    async fn verify(
        &self,
        request: &VerifyRequest,
        progress: &mut (dyn for<'t> FnMut(&'t str) + Send),
        cancel: &CancellationToken,
    ) -> Result<VerifyReport, EngineError> {
        let forward = move |text: &str| progress(text);
        mcbench_engine::VerifyEngines::verify(self, request, forward, cancel).await
    }

    fn warm(&self, engines: usize) -> Result<(), String> {
        mcbench_engine::VerifyEngines::warm(self, engines).map_err(|error| error.to_string())
    }

    async fn stop(&self) {
        mcbench_engine::VerifyEngines::stop(self).await
    }

    fn sweep(&self, age_ms: f64) {
        sweep_runs(&std::env::temp_dir(), age_ms);
    }

    async fn fixed_palette(
        &self,
        entries: &[BlockState],
        data_version: i64,
    ) -> Result<Vec<BlockState>, String> {
        self.program().fixed_palette(entries, data_version).await
    }
}

/// What the import's last pass is for a world whose blocks the data fixer
/// brought forward: by the world's palette index, the last-pass flag of
/// the state the engine places for it (`placedPalette`, `lastPass(placed)`).
struct Placed(Vec<u8>);

impl LastPass for Placed {
    fn last_pass(&self, _: &Palette) -> Result<Vec<u8>, String> {
        Ok(self.0.clone())
    }
}

/// What a verifier is made of, shared by its lanes.
pub struct Minecraft {
    /// The engine.
    pub engine: Arc<dyn VerifyEngine>,
    /// Minecraft's table, for the import's order ([`MinecraftFacts`]; a
    /// test's stand-in, as today's `fake-facts.js`).
    pub facts: Arc<dyn LastPass + Send + Sync>,
    /// Minecraft's item tags a request is checked against.
    pub tags: Arc<ItemTags>,
    /// Browser y + this is Minecraft y (`Y_OFFSET`).
    pub y_offset: i32,
}

impl Minecraft {
    /// The verifier's Minecraft on `engine`, with the table in
    /// `tables/blocks.txt` (`core/minecraft/<version>/`, or the shipped
    /// data) and the y offset of this process's environment.
    pub fn new(engine: Arc<dyn VerifyEngine>, tables: &Path) -> Result<Minecraft, String> {
        let file = tables.join("blocks.txt");
        let text = std::fs::read_to_string(&file)
            .map_err(|error| format!("{}: {error}", file.display()))?;
        Ok(Minecraft {
            engine,
            facts: Arc::new(MinecraftFacts::new(&text)?),
            tags: Arc::new(ItemTags::from_table(&text)?),
            y_offset: mcbench_world::dimension::y_offset(),
        })
    }
}

/// One lane: an engine verifier (`EngineVerifier`).
struct Lane {
    /// Where its reports are archived (`<root>/reports`).
    root: PathBuf,
    /// Experiments given to it that have not ended (`pending`).
    pending: AtomicUsize,
    /// The end of the last experiment given to it (`this.queue`).
    tail: Mutex<Option<oneshot::Receiver<()>>>,
}

/// The verifier pool (`VerifierPool`).
pub struct VerifierPool {
    root: PathBuf,
    minecraft: Arc<Minecraft>,
    memory: Box<dyn Fn() -> Result<u64, String> + Send + Sync>,
    lanes: Mutex<Vec<Arc<Lane>>>,
    sized: AtomicBool,
    /// The settings an experiment of a chat is queued with (`budgets`, the
    /// placement mode the benchmark's workers choose, the app's report
    /// encoding).
    pub settings: Mutex<Settings>,
    shutdown: CancellationToken,
    swept: AtomicBool,
}

/// An experiment's events and answer, as its lane sends them.
pub(crate) struct Channels {
    /// Its progress updates, with its index in the call.
    pub events: mpsc::UnboundedSender<(usize, Map<String, Value>)>,
    /// Its answer, with its index.
    pub answers: mpsc::UnboundedSender<(usize, Answer)>,
}

/// What an experiment answers: `run`'s result, or why there is none.
pub(crate) type Answer = Result<Map<String, Value>, VerifyError>;

/// An experiment given to a lane.
pub(crate) struct Submitted {
    /// Whether its lane started it (`started`).
    pub started: Arc<AtomicBool>,
}

impl VerifierPool {
    /// A pool archiving under `root` (`<root>/reports`, and
    /// `<root>/lanes/<n>/reports` for the lanes past the first), sized by
    /// `memory` (the machine's total memory: [`super::system::total_memory`])
    /// the first time an experiment runs.
    pub fn new(
        root: PathBuf,
        minecraft: Minecraft,
        settings: Settings,
        memory: Box<dyn Fn() -> Result<u64, String> + Send + Sync>,
    ) -> VerifierPool {
        VerifierPool {
            lanes: Mutex::new(vec![Arc::new(Lane::new(root.clone()))]),
            root,
            minecraft: Arc::new(minecraft),
            memory,
            sized: AtomicBool::new(false),
            settings: Mutex::new(settings),
            shutdown: CancellationToken::new(),
            swept: AtomicBool::new(false),
        }
    }

    /// A pool of exactly one lane (`createVerifier`'s `EngineVerifier`, the
    /// benchmark's grader).
    pub fn single(root: PathBuf, minecraft: Minecraft, settings: Settings) -> VerifierPool {
        let pool = VerifierPool::new(root, minecraft, settings, Box::new(|| Ok(0)));
        pool.sized.store(true, Ordering::SeqCst);
        pool
    }

    /// The lanes' roots, in order.
    pub fn lane_roots(&self) -> Vec<PathBuf> {
        self.lanes
            .lock()
            .unwrap()
            .iter()
            .map(|lane| lane.root.clone())
            .collect()
    }

    /// Adds the lanes the machine's memory holds, once (`size`); a reading
    /// that failed is read again by the next experiment.
    pub fn size(&self) -> Result<(), String> {
        if self.sized.load(Ordering::SeqCst) {
            return Ok(());
        }
        let bytes = (self.memory)()?;
        let count =
            MAX_LANES.min(1.max((bytes.saturating_sub(RESERVED_BYTES) / LANE_BYTES) as usize));
        let mut lanes = self.lanes.lock().unwrap();
        if !self.sized.swap(true, Ordering::SeqCst) {
            for index in lanes.len()..count {
                lanes.push(Arc::new(Lane::new(
                    self.root.join("lanes").join(index.to_string()),
                )));
            }
        }
        Ok(())
    }

    /// Sizes the pool and starts an engine for each lane (`warm`), so the
    /// first tests do not wait for one to boot.
    pub fn warm(&self) -> Result<(), String> {
        self.size()?;
        let lanes = self.lanes.lock().unwrap().len();
        self.minecraft.engine.warm(lanes)
    }

    /// Runs one experiment of `world` (`run(world, args, { signal,
    /// onUpdate, reportPath })`): the result `verifier.run` resolves with.
    pub async fn run(
        &self,
        world: &World,
        args: &Value,
        settings: &Settings,
        cancel: &CancellationToken,
        progress: &mut (dyn FnMut(Map<String, Value>) + Send),
        report_path: Option<String>,
    ) -> Result<Map<String, Value>, VerifyError> {
        let (events, mut events_out) = mpsc::unbounded_channel();
        let (answers, mut answers_out) = mpsc::unbounded_channel();
        let submitted = self.submit(
            world,
            args,
            settings,
            cancel,
            report_path,
            0,
            Channels { events, answers },
        )?;
        loop {
            tokio::select! {
                biased;
                Some((_, update)) = events_out.recv() => progress(update),
                Some((_, answer)) = answers_out.recv() => {
                    while let Ok((_, update)) = events_out.try_recv() {
                        progress(update);
                    }
                    return answer;
                }
                _ = cancel.cancelled(), if !submitted.started.load(Ordering::SeqCst) => {
                    return Err(VerifyError::message(ABORTED));
                }
                _ = self.shutdown.cancelled(), if !submitted.started.load(Ordering::SeqCst) => {
                    return Err(VerifyError::message(SHUTTING_DOWN));
                }
            }
        }
    }

    /// Whether the experiment of a call that is not started yet is answered
    /// now: its cancel or the pool's shutdown fired.
    pub(crate) fn aborted_reason(&self, cancel: &CancellationToken) -> Option<&'static str> {
        if self.shutdown.is_cancelled() {
            Some(SHUTTING_DOWN)
        } else if cancel.is_cancelled() {
            Some(ABORTED)
        } else {
            None
        }
    }

    /// The pool's shutdown.
    pub(crate) fn shutdown(&self) -> &CancellationToken {
        &self.shutdown
    }

    /// `pool.run` up to the experiment's start: sizes the pool, picks the
    /// lane, validates and snapshots (`Experiments.run`), and queues it.
    /// Errors before it is queued are its answer.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn submit(
        &self,
        world: &World,
        args: &Value,
        settings: &Settings,
        cancel: &CancellationToken,
        report_path: Option<String>,
        index: usize,
        channels: Channels,
    ) -> Result<Submitted, VerifyError> {
        self.size().map_err(VerifyError::Message)?;
        let lane = {
            let lanes = self.lanes.lock().unwrap();
            let mut best = &lanes[0];
            for candidate in lanes.iter() {
                if candidate.pending.load(Ordering::SeqCst) < best.pending.load(Ordering::SeqCst) {
                    best = candidate;
                }
            }
            best.clone()
        };
        if let Some(reason) = self.aborted_reason(cancel) {
            return Err(VerifyError::message(reason));
        }
        let experiment = validate_request(
            world,
            args,
            self.minecraft.y_offset,
            settings,
            &self.minecraft.tags,
        )?;
        let snapshot = world.clone();
        let _ = channels.events.send((
            index,
            update("queued", "Waiting for the Minecraft verifier"),
        ));
        let started = Arc::new(AtomicBool::new(false));
        let (done, finished) = oneshot::channel();
        let previous = lane.tail.lock().unwrap().replace(finished);
        lane.pending.fetch_add(1, Ordering::SeqCst);
        let run = Run {
            minecraft: self.minecraft.clone(),
            lane: lane.clone(),
            snapshot,
            experiment,
            encoding: settings.encoding,
            cancel: cancel.clone(),
            shutdown: self.shutdown.clone(),
            report_path,
            index,
            channels,
            sweep: !self.swept.swap(true, Ordering::SeqCst),
        };
        let started_flag = started.clone();
        tokio::spawn(async move {
            if let Some(previous) = previous {
                let _ = previous.await;
            }
            let aborted = if run.shutdown.is_cancelled() {
                Some(SHUTTING_DOWN)
            } else if run.cancel.is_cancelled() {
                Some(ABORTED)
            } else {
                None
            };
            match aborted {
                Some(reason) => {
                    let _ = run
                        .channels
                        .answers
                        .send((run.index, Err(VerifyError::message(reason))));
                }
                None => {
                    started_flag.store(true, Ordering::SeqCst);
                    run.execute().await;
                }
            }
            lane.pending.fetch_sub(1, Ordering::SeqCst);
            let _ = done.send(());
        });
        Ok(Submitted { started })
    }

    /// Ends the verifier (`stop`): no experiment starts after it; one that
    /// waits is answered with the shutdown, one that runs as a cancel would
    /// answer it (its engine is stopped); returns once every lane's last
    /// experiment has ended and the engines have exited.
    pub async fn stop(&self) {
        self.shutdown.cancel();
        let tails: Vec<oneshot::Receiver<()>> = self
            .lanes
            .lock()
            .unwrap()
            .iter()
            .filter_map(|lane| lane.tail.lock().unwrap().take())
            .collect();
        for tail in tails {
            let _ = tail.await;
        }
        self.minecraft.engine.stop().await;
    }
}

impl Lane {
    fn new(root: PathBuf) -> Lane {
        Lane {
            root,
            pending: AtomicUsize::new(0),
            tail: Mutex::new(None),
        }
    }
}

/// A progress update `{phase, message}`.
fn update(phase: &str, message: &str) -> Map<String, Value> {
    let mut map = Map::new();
    map.insert("phase".into(), Value::from(phase));
    map.insert("message".into(), Value::from(message));
    map
}

/// The experiment's progress as `test_build` reports it (`progressUpdate`).
fn progress_update(progress: &Value) -> Map<String, Value> {
    let mut map = progress.as_object().cloned().unwrap_or_default();
    let number = |name: &str| match progress.get(name) {
        Some(Value::Number(n)) => js::text(n.as_f64().expect("a number")),
        Some(Value::String(s)) => s.clone(),
        Some(other) => mcbench_json::stringify(other),
        None => "undefined".into(),
    };
    let (tick, total) = (number("tick"), number("totalTicks"));
    let message = match progress.get("phase").and_then(Value::as_str) {
        Some("activating") => format!("Waiting for Minecraft chunks to tick: {tick}/{total}"),
        Some("settling") => format!("Settling Minecraft: {tick}/{total} ticks"),
        _ => format!("Observing Minecraft: {tick}/{total} ticks"),
    };
    map.insert("message".into(), Value::from(message));
    map
}

/// One experiment on its lane (`EngineVerifier.execute`).
struct Run {
    minecraft: Arc<Minecraft>,
    lane: Arc<Lane>,
    snapshot: World,
    experiment: Experiment,
    encoding: super::request::Encoding,
    cancel: CancellationToken,
    shutdown: CancellationToken,
    report_path: Option<String>,
    index: usize,
    channels: Channels,
    sweep: bool,
}

/// How an experiment's wait for something ended early.
enum Stop {
    /// The chat's cancel or the shutdown: its reason.
    Aborted(&'static str),
    /// Past its wall-clock budget.
    Wall,
}

impl Run {
    fn send(&self, update: Map<String, Value>) {
        let _ = self.channels.events.send((self.index, update));
    }

    fn answer(&self, answer: Result<Map<String, Value>, VerifyError>) {
        let _ = self.channels.answers.send((self.index, answer));
    }

    /// Resolves when the experiment must stop waiting.
    async fn stopped(&self, deadline: tokio::time::Instant) -> Stop {
        tokio::select! {
            _ = self.shutdown.cancelled() => Stop::Aborted(SHUTTING_DOWN),
            _ = self.cancel.cancelled() => Stop::Aborted(ABORTED),
            _ = tokio::time::sleep_until(deadline) => Stop::Wall,
        }
    }

    fn aborted(&self, deadline: tokio::time::Instant) -> Option<Stop> {
        if self.shutdown.is_cancelled() {
            Some(Stop::Aborted(SHUTTING_DOWN))
        } else if self.cancel.is_cancelled() {
            Some(Stop::Aborted(ABORTED))
        } else if tokio::time::Instant::now() >= deadline {
            Some(Stop::Wall)
        } else {
            None
        }
    }

    async fn execute(self) {
        // Today's `EngineVerifier.execute` names its run folder
        // `runs/<crypto.randomUUID()>`; the engine child keeps a folder of its
        // own, and the id is drawn here so that a process's ids come in
        // today's order (a deterministic source, `system::set_uuid_source`,
        // counts them in the whole-turn replay).
        let _run = random_uuid();
        let started_at = Instant::now();
        let wall = self
            .experiment
            .budgets
            .get("wallMs")
            .unwrap_or(f64::INFINITY);
        let deadline =
            tokio::time::Instant::now() + Duration::from_secs_f64((wall / 1000.0).min(1e9));
        let reached: Arc<Mutex<Option<Value>>> = Arc::new(Mutex::new(None));
        let fail = |stop: Stop, reached: &Arc<Mutex<Option<Value>>>| match stop {
            Stop::Aborted(reason) => VerifyError::message(reason),
            Stop::Wall => VerifyError::wall(
                started_at.elapsed().as_millis() as f64,
                wall,
                reached.lock().unwrap().as_ref(),
            ),
        };
        if let Some(stop) = self.aborted(deadline) {
            return self.answer(Err(fail(stop, &reached)));
        }
        let mut timings = Map::new();
        let mut phase_start = Instant::now();
        let mut phase = |name: &str, timings: &mut Map<String, Value>| {
            timings.insert(
                name.into(),
                Value::from(phase_start.elapsed().as_millis() as u64),
            );
            phase_start = Instant::now();
        };
        if self.sweep {
            let age = 2.0 * wall;
            let engine = self.minecraft.engine.clone();
            tokio::task::spawn_blocking(move || engine.sweep(age))
                .await
                .expect("the sweep does not panic");
        }
        self.send(update("preparing", "Preparing Minecraft"));
        phase("startupMs", &mut timings);
        if let Some(stop) = self.aborted(deadline) {
            return self.answer(Err(fail(stop, &reached)));
        }
        // The commands, ordered by the palette the engine places.
        let prepared = tokio::select! {
            prepared = self.prepare() => prepared,
            stop = self.stopped(deadline) => return self.answer(Err(fail(stop, &reached))),
        };
        let request = match prepared {
            Ok(request) => request,
            Err(error) => return self.answer(Err(error)),
        };
        self.send(update(
            "running",
            "Running Minecraft inputs and observations",
        ));
        let engine = self.minecraft.engine.clone();
        let events = self.channels.events.clone();
        let index = self.index;
        let seen = reached.clone();
        let mut progress = move |text: &str| {
            let Ok(value) = mcbench_json::parse(text) else {
                return;
            };
            *seen.lock().unwrap() = Some(value.clone());
            let _ = events.send((index, progress_update(&value)));
        };
        // Fired when the experiment must stop: its engine is stopped.
        let stop_engine = CancellationToken::new();
        let verify = engine.verify(&request, &mut progress, &stop_engine);
        tokio::pin!(verify);
        let outcome = tokio::select! {
            outcome = &mut verify => outcome,
            stop = self.stopped(deadline) => {
                stop_engine.cancel();
                // The engine process is stopped and the experiment fails
                // with its exit, worded as today's SIGTERM; a timeout is a
                // wall budget error.
                self.answer(Err(match stop {
                    Stop::Aborted(_) => VerifyError::message(ENGINE_STOPPED),
                    Stop::Wall => fail(Stop::Wall, &reached),
                }));
                // Returns at once: the engine was stopped.
                let _ = verify.await;
                return;
            }
        };
        let answer = match outcome {
            Err(error) => Err(verification_failure(&error.to_string())),
            Ok(report) => {
                self.finish(report, started_at, &mut timings, &mut phase)
                    .await
            }
        };
        self.answer(answer);
    }

    /// The request and commands of the experiment (`transport`,
    /// `importCommands` with `placedPalette`).
    async fn prepare(&self) -> Result<VerifyRequest, VerifyError> {
        let version = self
            .experiment
            .block_data_version
            .unwrap_or(mcbench_engine::DATA_VERSION);
        let palette = self.snapshot.palette().clone();
        let last = if version >= mcbench_engine::DATA_VERSION {
            None
        } else {
            let entries: Vec<BlockState> = palette.entries().cloned().collect();
            let fixed = self
                .minecraft
                .engine
                .fixed_palette(&entries, version)
                .await
                .map_err(VerifyError::Message)?;
            let placed = Palette::new();
            let mut at = vec![0usize];
            for state in &fixed {
                at.push(
                    placed
                        .intern(&state.id, &state.properties)
                        .map_err(VerifyError::Message)? as usize,
                );
            }
            let last = self
                .minecraft
                .facts
                .last_pass(&placed)
                .map_err(VerifyError::Message)?;
            Some(Placed(
                at.iter()
                    .enumerate()
                    .map(|(i, &j)| if i == 0 { 0 } else { last[j] })
                    .collect(),
            ))
        };
        let world = self.snapshot.clone();
        let facts = self.minecraft.facts.clone();
        let y_offset = self.minecraft.y_offset;
        let request = self.experiment.transport(self.encoding);
        let commands = tokio::task::spawn_blocking(move || match &last {
            Some(placed) => world.place_commands(None, y_offset, None, placed),
            None => world.place_commands(None, y_offset, None, facts.as_ref()),
        })
        .await
        .expect("the import's commands do not panic")
        .map_err(VerifyError::Message)?;
        Ok(VerifyRequest { request, commands })
    }

    /// A finished `VERIFY` read back (`result.json`, `checkComplete`,
    /// `finishedResult`).
    async fn finish(
        &self,
        report: VerifyReport,
        started_at: Instant,
        timings: &mut Map<String, Value>,
        phase: &mut (dyn FnMut(&str, &mut Map<String, Value>) + Send),
    ) -> Result<Map<String, Value>, VerifyError> {
        let result = mcbench_json::parse(&report.result).map_err(VerifyError::Message)?;
        let field = |name: &str| result.get(name);
        if !js::truthy(field("ok")) {
            if js::truthy(field("budgetResource")) {
                let resource = shown(field("budgetResource"));
                return Err(VerifyError::budget(
                    &resource,
                    field("requested").and_then(Value::as_f64),
                    self.experiment.budgets.get(&resource),
                ));
            }
            return Err(VerifyError::message(format!(
                "Minecraft verification failed: {}",
                shown(field("error"))
            )));
        }
        let file = report
            .files
            .into_iter()
            .find(|(name, _)| name == "report.json")
            .map(|(_, bytes)| bytes);
        let number = |name: &str| field(name).and_then(Value::as_f64);
        let [probes, samples, actions] = self.experiment.counts.map(|n| n as f64);
        let complete = number("elapsedTicks") == Some(self.experiment.duration as f64)
            && number("sampleCount") == Some(samples)
            && number("probeCount") == Some(probes)
            && number("inputCount") == Some(actions)
            && number("observationCount") == Some(probes * (samples + 1.0))
            && file
                .as_ref()
                .is_some_and(|bytes| Some(bytes.len() as f64) == number("reportBytes"));
        let Some(file) = file.filter(|_| complete) else {
            return Err(VerifyError::message(
                "Minecraft verification returned incomplete observations",
            ));
        };
        phase("simulationMs", timings);
        let report_bytes = number("reportBytes").expect("checked complete");
        check_budget("reportBytes", report_bytes, &self.experiment.budgets)?;
        let archive = self.report_path.clone().unwrap_or_else(|| {
            self.lane
                .root
                .join("reports")
                .join(format!("{}.json", random_uuid()))
                .to_string_lossy()
                .into_owned()
        });
        let inline = report_bytes < (MAX_INLINE_BYTES / 2) as f64;
        let (file, parsed) = {
            let archive = archive.clone();
            tokio::task::spawn_blocking(move || -> Result<(Vec<u8>, Option<Value>), String> {
                let path = Path::new(&archive);
                if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
                    std::fs::create_dir_all(parent)
                        .map_err(|e| format!("{}: {e}", parent.display()))?;
                }
                std::fs::write(path, &file).map_err(|e| format!("{archive}: {e}"))?;
                let parsed = if inline {
                    let text =
                        std::str::from_utf8(&file).map_err(|e| format!("report.json: {e}"))?;
                    Some(mcbench_json::parse(text)?)
                } else {
                    None
                };
                Ok((file, parsed))
            })
            .await
            .expect("archiving a report does not panic")
            .map_err(VerifyError::Message)?
        };
        drop(file);
        let mut out = match parsed {
            Some(Value::Object(report)) => report,
            Some(_) => Map::new(),
            None => {
                let mut out = Map::new();
                for name in ["observationsInFile", "probesInFile", "inputsInFile"] {
                    out.insert(name.into(), Value::Bool(true));
                }
                out
            }
        };
        if let Value::Object(result) = result {
            for (key, value) in result {
                out.insert(key, value);
            }
        }
        out.insert(
            "worldRevision".into(),
            Value::from(self.snapshot.revision()),
        );
        out.insert("reportFile".into(), Value::from(archive));
        out.insert("cost".into(), self.experiment.cost().clone());
        out.insert("budgets".into(), self.experiment.budgets.to_json());
        out.insert(
            "wallMs".into(),
            Value::from(started_at.elapsed().as_millis() as u64),
        );
        out.insert("note".into(), Value::from(NOTE));
        phase("archiveMs", timings);
        let mut done = Map::new();
        done.insert("phase".into(), Value::from("running"));
        done.insert("tick".into(), out["elapsedTicks"].clone());
        done.insert("totalTicks".into(), Value::from(self.experiment.duration));
        done.insert(
            "message".into(),
            Value::from("Minecraft observations complete"),
        );
        self.send(done);
        out.insert("timings".into(), Value::Object(timings.clone()));
        Ok(out)
    }
}

/// `String(value)` of a JSON value read from the engine.
fn shown(value: Option<&Value>) -> String {
    match value {
        None => "undefined".into(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => js::text(n.as_f64().expect("a number")),
        Some(Value::Bool(b)) => b.to_string(),
        Some(Value::Null) => "null".into(),
        Some(Value::Array(_)) | Some(Value::Object(_)) => json!(value).to_string(),
    }
}
