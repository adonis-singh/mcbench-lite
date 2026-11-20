//! The settle pool, the port of `server/engine-pool.js` (`EnginePool`,
//! `poolSize`): worlds for work that needs a whole world to itself, one job
//! at a time per key (a conversation) in the order asked, jobs of different
//! keys at once on as many members as the pool has. [`Pool`] is the pool
//! over any [`Member`], as `EnginePool` is over whatever its `make` makes;
//! [`SettlePool`] is the pool of engine children ([`ChildEngine`]), and
//! [`settle_on`] the settle's requests (`server/shot-world.js`
//! `runSettle`).

use crate::{ChildEngine, EngineError, EngineProgram};
use mcbench_world::{Blueprint, PlaceCommands};
use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{oneshot, watch};
use tokio_util::sync::CancellationToken;

/// Memory one member is expected to hold: the engine with a large build and
/// the caller's copy of its answer (`engine-pool.js` `MEMBER_BYTES`).
pub const MEMBER_BYTES: u64 = 3 * 1024 * 1024 * 1024 / 2;
/// The most members a pool sized from the machine has.
pub const MAX_MEMBERS: usize = 8;

/// How many members the machine holds (`engine-pool.js` `poolSize`): one
/// core stays for the rest of the app, and each member needs
/// [`MEMBER_BYTES`]. `MCBENCH_SETTLE_ENGINES` in `env` sets it.
pub fn pool_size(env: &HashMap<String, String>) -> Result<usize, String> {
    if let Some(value) = env.get("MCBENCH_SETTLE_ENGINES") {
        // `Number(value)`, then `Number.isInteger(size) && size >= 1`.
        return match value.trim().parse::<f64>() {
            Ok(size) if size.fract() == 0.0 && (1.0..=u32::MAX as f64).contains(&size) => {
                Ok(size as usize)
            }
            _ => Err(format!(
                "MCBENCH_SETTLE_ENGINES must be an integer of at least 1, not {value}"
            )),
        };
    }
    let cores = std::thread::available_parallelism().map_or(1, |n| n.get());
    let memory = (available_memory()? / MEMBER_BYTES) as usize;
    Ok(1.max(MAX_MEMBERS.min(cores.saturating_sub(1)).min(memory)))
}

/// Memory that can be given to a program without swapping (`engine-pool.js`
/// `availableMemory`): on macOS the pages `vm_stat` says it could drop
/// (inactive, speculative, purgeable) beside the free ones; elsewhere what
/// Node's `os.freemem()` reads.
#[cfg(target_os = "macos")]
fn available_memory() -> Result<u64, String> {
    let output = mcbench_spawn::command("vm_stat")
        .output()
        .map_err(|error| format!("vm_stat: {error}"))?;
    let text = String::from_utf8_lossy(&output.stdout);
    let number_after = |label: &str| -> Result<u64, String> {
        let at = text
            .find(label)
            .ok_or_else(|| format!("vm_stat printed no {label}"))?;
        let digits: String = text[at + label.len()..]
            .trim_start()
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        digits
            .parse()
            .map_err(|_| format!("vm_stat printed no number after {label}"))
    };
    let pages = number_after("Pages free:")?
        + number_after("Pages inactive:")?
        + number_after("Pages speculative:")?
        + number_after("Pages purgeable:")?;
    Ok(pages * number_after("page size of")?)
}

/// Linux: `MemAvailable` of `/proc/meminfo`, as libuv's `uv_get_free_memory`
/// (Node's `os.freemem()`) reads it.
#[cfg(target_os = "linux")]
fn available_memory() -> Result<u64, String> {
    let text = std::fs::read_to_string("/proc/meminfo")
        .map_err(|error| format!("/proc/meminfo: {error}"))?;
    let line = text
        .lines()
        .find(|line| line.starts_with("MemAvailable:"))
        .ok_or("/proc/meminfo has no MemAvailable")?;
    let kib: u64 = line
        .trim_start_matches("MemAvailable:")
        .trim()
        .trim_end_matches("kB")
        .trim()
        .parse()
        .map_err(|_| format!("/proc/meminfo: {line}"))?;
    Ok(kib * 1024)
}

/// Windows: `ullAvailPhys` of `GlobalMemoryStatusEx`, as libuv reads it.
#[cfg(windows)]
fn available_memory() -> Result<u64, String> {
    #[repr(C)]
    struct MemoryStatusEx {
        length: u32,
        memory_load: u32,
        total_phys: u64,
        avail_phys: u64,
        total_page_file: u64,
        avail_page_file: u64,
        total_virtual: u64,
        avail_virtual: u64,
        avail_extended_virtual: u64,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GlobalMemoryStatusEx(buffer: *mut MemoryStatusEx) -> i32;
    }
    let mut status = MemoryStatusEx {
        length: std::mem::size_of::<MemoryStatusEx>() as u32,
        memory_load: 0,
        total_phys: 0,
        avail_phys: 0,
        total_page_file: 0,
        avail_page_file: 0,
        total_virtual: 0,
        avail_virtual: 0,
        avail_extended_virtual: 0,
    };
    // SAFETY: `status` is a MEMORYSTATUSEX with its length set, as the
    // function requires; it writes only into that struct.
    if unsafe { GlobalMemoryStatusEx(&mut status) } == 0 {
        return Err(format!(
            "GlobalMemoryStatusEx: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(status.avail_phys)
}

/// What a pool holds: something with a world (an engine child, or a test's
/// fake) that can die and be stopped.
pub trait Member: Send + Sync + 'static {
    /// False once its engine died: a free member that is not alive is
    /// retired, not handed out.
    fn alive(&self) -> bool;
    /// Ends it; resolves once its engine has exited.
    fn stop(&self) -> Pin<Box<dyn Future<Output = ()> + Send + '_>>;
}

impl Member for ChildEngine {
    fn alive(&self) -> bool {
        ChildEngine::alive(self)
    }

    fn stop(&self) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        Box::pin(ChildEngine::stop(self))
    }
}

/// A member as a job holds it: the member, its slot, and whether the job
/// dropped it (it is not used again).
pub struct Lease<M> {
    member: M,
    slot: usize,
    dropped: AtomicBool,
}

impl<M> Lease<M> {
    /// The member.
    pub fn member(&self) -> &M {
        &self.member
    }

    /// The slot it has (0, 1, ...), reused once the member that had it
    /// stopped.
    pub fn slot(&self) -> usize {
        self.slot
    }

    /// The job found the member broken (it did not start, or its engine
    /// stopped): it is stopped when the job ends and not used again.
    pub fn drop_member(&self) {
        self.dropped.store(true, Ordering::SeqCst);
    }
}

type Make<M> = dyn Fn(usize) -> Result<M, EngineError> + Send + Sync;
type Waiter<M> = oneshot::Sender<Result<Arc<Lease<M>>, EngineError>>;

struct State<M> {
    members: HashMap<usize, Arc<Lease<M>>>,
    closing: HashMap<usize, watch::Receiver<bool>>,
    free: Vec<Arc<Lease<M>>>,
    waiting: VecDeque<Waiter<M>>,
    tails: HashMap<String, (u64, oneshot::Receiver<()>)>,
    stopped: bool,
}

struct Shared<M> {
    size: usize,
    make: Box<Make<M>>,
    wall: Duration,
    state: Mutex<State<M>>,
    next_tail: AtomicU64,
}

/// `EnginePool`: members made by `make(slot)` as jobs wait for them, at most
/// `size` at once (a member stopping still counts), each running one job at
/// a time. Cloned handles share one pool. Its calls need the tokio runtime.
pub struct Pool<M: Member> {
    shared: Arc<Shared<M>>,
}

impl<M: Member> Clone for Pool<M> {
    fn clone(&self) -> Self {
        Pool {
            shared: self.shared.clone(),
        }
    }
}

fn stopped_pool() -> EngineError {
    EngineError::Stopped("The engine pool was stopped".into())
}

/// A JavaScript number as `${number}` writes it, for a duration in seconds.
fn js_seconds(wall: Duration) -> String {
    let ms = wall.as_millis() as f64;
    let seconds = ms / 1000.0;
    if seconds.fract() == 0.0 {
        format!("{}", seconds as i64)
    } else {
        format!("{seconds}")
    }
}

impl<M: Member> Pool<M> {
    /// A pool of at most `size` members, each made by `make(slot)`. `wall` is
    /// how long a job may hold its member.
    pub fn new(
        size: usize,
        wall: Duration,
        make: impl Fn(usize) -> Result<M, EngineError> + Send + Sync + 'static,
    ) -> Pool<M> {
        Pool {
            shared: Arc::new(Shared {
                size,
                make: Box::new(make),
                wall,
                state: Mutex::new(State {
                    members: HashMap::new(),
                    closing: HashMap::new(),
                    free: Vec::new(),
                    waiting: VecDeque::new(),
                    tails: HashMap::new(),
                    stopped: false,
                }),
                next_tail: AtomicU64::new(0),
            }),
        }
    }

    /// Runs `job` on a member of its own, after the key's earlier jobs (in
    /// the order `run` was called). A job still running the pool's wall
    /// budget after it got its member fails, and its member is dropped, so
    /// no build holds a world for good. The order is taken when `run` is
    /// called, not when its future is first polled.
    pub fn run<T, F, Fut>(
        &self,
        key: &str,
        job: F,
    ) -> impl Future<Output = Result<T, EngineError>> + Send + 'static + use<M, T, F, Fut>
    where
        T: Send + 'static,
        F: FnOnce(Arc<Lease<M>>) -> Fut + Send + 'static,
        Fut: Future<Output = Result<T, EngineError>> + Send + 'static,
    {
        let (previous, done, tail) = self.queue(key);
        let pool = self.clone();
        let key = key.to_string();
        // A key with no job before this one asks for its member at once, so
        // members go to jobs in the order run was called (EnginePool's
        // acquire runs in the microtask after the call).
        let first = previous.is_none().then(|| self.wait_for_member());
        async move {
            let member = match (previous, first) {
                (_, Some(first)) => first.await,
                (Some(previous), None) => {
                    let _ = previous.await;
                    pool.acquire().await
                }
                (None, None) => unreachable!(),
            };
            let result = match member {
                Err(error) => Err(error),
                Ok(lease) => {
                    let wall = pool.shared.wall;
                    let outcome = tokio::time::timeout(wall, job(lease.clone())).await;
                    let result = match outcome {
                        Ok(result) => result,
                        Err(_) => {
                            lease.drop_member();
                            Err(EngineError::Stopped(format!(
                                "The settle ran past its {}-second wall-clock budget (MCBENCH_VERIFY_MAX_WALL_MS) and was stopped: the build is too active to settle in time. Ask for fewer ticks or make the build less active.",
                                js_seconds(wall)
                            )))
                        }
                    };
                    pool.release(lease);
                    result
                }
            };
            pool.unqueue(&key, tail, done);
            result
        }
    }

    /// Runs `job` with no member (`run(key, job, { member: false })`),
    /// still after the key's earlier jobs.
    pub fn run_without_member<T, Fut>(
        &self,
        key: &str,
        job: Fut,
    ) -> impl Future<Output = Result<T, EngineError>> + Send + 'static + use<M, T, Fut>
    where
        T: Send + 'static,
        Fut: Future<Output = Result<T, EngineError>> + Send + 'static,
    {
        let (previous, done, tail) = self.queue(key);
        let pool = self.clone();
        let key = key.to_string();
        async move {
            if let Some(previous) = previous {
                let _ = previous.await;
            }
            let result = job.await;
            pool.unqueue(&key, tail, done);
            result
        }
    }

    /// Takes the key's tail (the job before) and makes this job the tail.
    fn queue(&self, key: &str) -> (Option<oneshot::Receiver<()>>, oneshot::Sender<()>, u64) {
        let (done, next) = oneshot::channel();
        let tail = self.shared.next_tail.fetch_add(1, Ordering::SeqCst);
        let previous = self
            .shared
            .state
            .lock()
            .unwrap()
            .tails
            .insert(key.to_string(), (tail, next))
            .map(|(_, previous)| previous);
        (previous, done, tail)
    }

    /// The job ended: the next of its key may start, and a key with no job
    /// after it is forgotten.
    fn unqueue(&self, key: &str, tail: u64, done: oneshot::Sender<()>) {
        let mut state = self.shared.state.lock().unwrap();
        if state.tails.get(key).is_some_and(|(id, _)| *id == tail) {
            state.tails.remove(key);
        }
        drop(state);
        let _ = done.send(());
    }

    /// A member of its own, first come first served.
    pub async fn acquire(&self) -> Result<Arc<Lease<M>>, EngineError> {
        self.wait_for_member().await
    }

    /// Joins the queue for a member now, and answers the future that waits
    /// for it.
    fn wait_for_member(
        &self,
    ) -> impl Future<Output = Result<Arc<Lease<M>>, EngineError>> + Send + 'static + use<M> {
        let (sender, receiver) = oneshot::channel();
        let stopped = {
            let mut state = self.shared.state.lock().unwrap();
            if !state.stopped {
                state.waiting.push_back(sender);
                self.serve(&mut state);
            }
            state.stopped
        };
        async move {
            if stopped {
                return Err(stopped_pool());
            }
            receiver.await.unwrap_or_else(|_| Err(stopped_pool()))
        }
    }

    /// Hands members to waiters, first come first served.
    fn serve(&self, state: &mut State<M>) {
        while !state.waiting.is_empty() && !state.stopped {
            let taken = match self.take(state) {
                Taken::None => return,
                Taken::Member(lease) => Ok(lease),
                Taken::Failed(error) => Err(error),
            };
            let waiter = state.waiting.pop_front().unwrap();
            if let Err(Ok(lease)) = waiter.send(taken) {
                // Its job is gone: the member is free again.
                state.free.push(lease);
            }
        }
    }

    /// A free member that is alive, or a new one if a slot is open.
    fn take(&self, state: &mut State<M>) -> Taken<M> {
        while let Some(lease) = state.free.pop() {
            if lease.member.alive() {
                return Taken::Member(lease);
            }
            self.retire(state, lease);
        }
        if state.members.len() + state.closing.len() >= self.shared.size {
            return Taken::None;
        }
        let mut slot = 0;
        while state.members.contains_key(&slot) || state.closing.contains_key(&slot) {
            slot += 1;
        }
        match (self.shared.make)(slot) {
            Err(error) => Taken::Failed(error),
            Ok(member) => {
                let lease = Arc::new(Lease {
                    member,
                    slot,
                    dropped: AtomicBool::new(false),
                });
                state.members.insert(slot, lease.clone());
                Taken::Member(lease)
            }
        }
    }

    /// Stops `lease`'s member; its slot opens once it has exited.
    fn retire(&self, state: &mut State<M>, lease: Arc<Lease<M>>) {
        state.members.remove(&lease.slot);
        let (exited, closed) = watch::channel(false);
        state.closing.insert(lease.slot, closed);
        let pool = self.clone();
        tokio::spawn(async move {
            lease.member.stop().await;
            let mut state = pool.shared.state.lock().unwrap();
            state.closing.remove(&lease.slot);
            pool.serve(&mut state);
            drop(state);
            let _ = exited.send(true);
        });
    }

    /// A member [`Pool::acquire`] gave back: a stopped pool already stopped
    /// it, a dropped one is stopped, any other is free again.
    pub fn release(&self, lease: Arc<Lease<M>>) {
        let mut state = self.shared.state.lock().unwrap();
        if state.stopped {
            return;
        }
        if lease.dropped.load(Ordering::SeqCst) {
            self.retire(&mut state, lease);
        } else {
            state.free.push(lease);
        }
        self.serve(&mut state);
    }

    /// Stops every member and fails every job still waiting for one; the
    /// pool takes no more. Resolves once every member has exited.
    pub async fn stop(&self) {
        let (members, closing) = {
            let mut state = self.shared.state.lock().unwrap();
            state.stopped = true;
            for waiter in state.waiting.drain(..) {
                let _ = waiter.send(Err(stopped_pool()));
            }
            state.free.clear();
            let members: Vec<_> = state.members.drain().map(|(_, lease)| lease).collect();
            let closing: Vec<_> = state.closing.values().cloned().collect();
            (members, closing)
        };
        for lease in &members {
            lease.member.stop().await;
        }
        for mut closed in closing {
            let _ = closed.wait_for(|exited| *exited).await;
        }
    }

    /// How many members the pool holds now, free or running a job, and
    /// those stopping.
    pub fn held(&self) -> usize {
        let state = self.shared.state.lock().unwrap();
        state.members.len() + state.closing.len()
    }
}

enum Taken<M> {
    None,
    Member(Arc<Lease<M>>),
    Failed(EngineError),
}

/// The settle pool: [`Pool`] of engine children (`shot-world.js`
/// `shotPool` with `makeMember`), each started when a job waits for it and
/// warm between jobs.
#[derive(Clone)]
pub struct SettlePool {
    pool: Pool<ChildEngine>,
    started: Arc<AtomicUsize>,
}

impl SettlePool {
    /// A pool of `members` children of `program` (`engine-pool.js`
    /// `poolSize`, see [`pool_size`]); a job may hold its child `wall` long
    /// (`MCBENCH_VERIFY_MAX_WALL_MS`, `verification-contract.js`
    /// `BUDGETS.wallMs`), after which the child is stopped. Its calls need
    /// the tokio runtime.
    pub fn new(program: EngineProgram, members: usize, wall: Duration) -> SettlePool {
        let started = Arc::new(AtomicUsize::new(0));
        let counted = started.clone();
        SettlePool {
            pool: Pool::new(members, wall, move |_slot| {
                counted.fetch_add(1, Ordering::SeqCst);
                program.spawn()
            }),
            started,
        }
    }

    /// How many children the pool has started since it was made (a member
    /// that died or was dropped is replaced by a new one).
    pub fn engines_started(&self) -> usize {
        self.started.load(Ordering::SeqCst)
    }

    /// The settle's engine work for a conversation (`runSettle` without
    /// what it makes of the blueprints): [`settle_on`] on a member after the
    /// conversation's earlier jobs, or, for a world of no blocks (`commands`
    /// empty: the engine refuses a placement of nothing), an empty settle
    /// for each tick with no member, still in the conversation's order. An
    /// empty settle has an empty blueprint, `[]` for its entities and no
    /// lines.
    pub fn settle(
        &self,
        conversation: &str,
        commands: PlaceCommands,
        ticks: Vec<i32>,
    ) -> impl Future<Output = Result<Vec<Settled>, EngineError>> + Send + 'static + use<> {
        let empty = commands.0.is_empty();
        let job: Pin<Box<dyn Future<Output = Result<Vec<Settled>, EngineError>> + Send>> = if empty
        {
            Box::pin(self.run_without_member(conversation, async move {
                Ok(ticks
                    .iter()
                    .map(|&tick| Settled {
                        ticks: tick,
                        blueprint: Blueprint(Vec::new()),
                        entities: b"[]".to_vec(),
                        unsupported: String::new(),
                    })
                    .collect())
            }))
        } else {
            Box::pin(self.run(conversation, move |lease| async move {
                settle_on(lease.member(), &commands, &ticks, &CancellationToken::new()).await
            }))
        };
        job
    }

    /// Runs `job` on a member's child once it is ready, after every earlier
    /// job of `key`. A member that did not start, or whose child ended
    /// under the job (it ran out of memory, or was killed), is dropped and
    /// replaced (`shot-world.js`); one whose request was refused is kept,
    /// as the refusal leaves a world that takes a fresh placement.
    pub fn run<T, F, Fut>(
        &self,
        key: &str,
        job: F,
    ) -> impl Future<Output = Result<T, EngineError>> + Send + 'static + use<T, F, Fut>
    where
        T: Send + 'static,
        F: FnOnce(Arc<Lease<ChildEngine>>) -> Fut + Send + 'static,
        Fut: Future<Output = Result<T, EngineError>> + Send + 'static,
    {
        self.pool
            .run(key, move |lease: Arc<Lease<ChildEngine>>| async move {
                if let Err(error) = lease.member().ready(&CancellationToken::new()).await {
                    lease.drop_member();
                    return Err(error);
                }
                let result = job(lease.clone()).await;
                if !lease.member().alive() {
                    lease.drop_member();
                }
                result
            })
    }

    /// Runs `job` with no child, still after the key's earlier jobs (a
    /// settle of an empty world).
    pub fn run_without_member<T, Fut>(
        &self,
        key: &str,
        job: Fut,
    ) -> impl Future<Output = Result<T, EngineError>> + Send + 'static + use<T, Fut>
    where
        T: Send + 'static,
        Fut: Future<Output = Result<T, EngineError>> + Send + 'static,
    {
        self.pool.run_without_member(key, job)
    }

    /// Starts the first child, so the first settle does not wait for it
    /// (`shot-world.js` `warmShotEngines`): it is given back to the pool at
    /// once and this resolves when it is ready.
    pub async fn warm(&self) -> Result<(), EngineError> {
        let lease = self.pool.acquire().await?;
        self.pool.release(lease.clone());
        lease.member().ready(&CancellationToken::new()).await
    }

    /// Stops every child (`stopShotEngines`).
    pub async fn stop(&self) {
        self.pool.stop().await
    }

    /// The pool itself.
    pub fn pool(&self) -> &Pool<ChildEngine> {
        &self.pool
    }
}

/// A world after a settle's ticks, as `shot-world.js` `runSettle` reads it
/// from the engine: the blueprint it writes, its entities then (the engine's
/// JSON list) and the blocks that cannot survive where they are (`SURVIVE`,
/// a line `x y z` each, at the engine's heights).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settled {
    /// The ticks run.
    pub ticks: i32,
    /// The world written after them.
    pub blueprint: Blueprint,
    /// The entities after them.
    pub entities: Vec<u8>,
    /// `SURVIVE`'s lines after them.
    pub unsupported: String,
}

/// The settle's requests on one child (`runSettle`): `commands` placed
/// afresh, then for each of `ticks` (sorted, no repeats) the ticks to it
/// unlit (nothing reads light after them: the blocks are written next; the
/// entities are written at every tick asked, at tick 0 by a tick of no
/// ticks), the world written by a placement of no commands (at tick 0 the
/// placement's own blueprint), and `SURVIVE`.
pub async fn settle_on(
    engine: &ChildEngine,
    commands: &PlaceCommands,
    ticks: &[i32],
    cancel: &CancellationToken,
) -> Result<Vec<Settled>, EngineError> {
    let placed = engine.place(commands, true, cancel).await?;
    let none = PlaceCommands(String::new());
    let mut at = 0;
    let mut out = Vec::new();
    for &tick in ticks {
        let ticked = engine.tick(tick - at, false, cancel).await?;
        at = tick;
        let blueprint = if tick > 0 {
            engine.place(&none, false, cancel).await?
        } else {
            placed.clone()
        };
        let unsupported = engine.unsupported(cancel).await?;
        out.push(Settled {
            ticks: tick,
            blueprint,
            entities: ticked.entities,
            unsupported,
        });
    }
    Ok(out)
}
