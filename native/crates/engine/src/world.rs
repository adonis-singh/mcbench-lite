//! One live world on an engine thread of its own: [`EngineWorld`] is the
//! handle the async runtime holds, [`WorldSession`] the world as block code
//! sees it, which exists only on that thread.

use crate::{Engine, EngineError, threads};
use mcbench_core::session::Session;
use mcbench_world::{Blueprint, PlaceCommands};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::{oneshot, watch};

/// What a tick or a player tick returns: the engine's patch on the stream's
/// base, the entities of the tick and the inputs, as
/// `mcbench_core::session::Ticked` holds them.
pub type Ticked = mcbench_core::session::Ticked;

/// The reply `core/src/main.rs` gives every request but a fresh placement
/// after a request that panicked.
pub const SPENT: &str = "unported: an earlier request reached what this port does not model";

/// One world of the engine as `core/src/main.rs`'s loop holds it: a
/// `Session` and whether a request panicked on it. Its methods are that
/// loop's requests with its rules: a request that panics (a refusal,
/// "unported: ...", or a bug of the port) fails with
/// `mcbench_core::failure`'s wording and spends the world, and a spent world
/// answers every request but a fresh placement with [`SPENT`].
///
/// It is made only on an engine thread (a thread with a 1 GiB stack, as
/// block code recurses as deep as Minecraft's): [`EngineWorld::call`] is the
/// only way to reach one.
pub struct WorldSession {
    session: Session,
    refused: bool,
}

impl WorldSession {
    fn new(session: Session) -> WorldSession {
        WorldSession {
            session,
            refused: false,
        }
    }

    /// One request, as `core/src/main.rs` runs it: refused on a spent world,
    /// a panic caught and worded by `mcbench_core::failure`.
    fn request<T>(
        &mut self,
        work: impl FnOnce(&mut Session) -> Result<T, String>,
    ) -> Result<T, EngineError> {
        if self.refused {
            return Err(EngineError::Failed(SPENT.into()));
        }
        match catch_unwind(AssertUnwindSafe(|| work(&mut self.session))) {
            Ok(Ok(value)) => Ok(value),
            Ok(Err(reason)) => Err(EngineError::Failed(reason)),
            Err(payload) => {
                self.refused = true;
                Err(EngineError::Failed(mcbench_core::failure(&*payload)))
            }
        }
    }

    /// `PLACE ... fresh|continue`: places the commands on an empty world
    /// (`fresh`) or on the world the last placement left, and returns the
    /// blueprint. A fresh placement is taken by a spent world, and makes it
    /// whole again before it places.
    pub fn place(
        &mut self,
        commands: &PlaceCommands,
        fresh: bool,
    ) -> Result<Blueprint, EngineError> {
        if fresh {
            self.refused = false;
        }
        self.request(|session| session.place(&commands.0, fresh))
            .map(Blueprint)
    }

    /// `TICK`: runs `ticks` game ticks, lighting what they changed when
    /// `light` (the stock request), or not (`nolight`).
    pub fn tick(&mut self, ticks: i32, light: bool) -> Result<Ticked, EngineError> {
        self.request(|session| session.tick(ticks, light))
    }

    /// `INPUT`: a player's `toggle`, `press` or `use` at a block, looking
    /// with `yaw`.
    pub fn input(&mut self, kind: &str, pos: [i32; 3], yaw: f32) -> Result<(), EngineError> {
        self.request(|session| session.input(kind, pos[0], pos[1], pos[2], yaw))
    }

    /// `PSPAWN`: places the creative player.
    pub fn spawn_player(
        &mut self,
        pos: [f64; 3],
        yaw: f32,
        pitch: f32,
        flying: bool,
    ) -> Result<(), EngineError> {
        self.request(|session| {
            session.spawn_player(pos, yaw, pitch, flying);
            Ok(())
        })
    }

    /// `PTICK`: one tick with the player's keys (the protocol's flags) and
    /// look; returns the tick and the player's view (28 doubles,
    /// `Session::player_view`).
    pub fn player_tick(
        &mut self,
        keys: u32,
        yaw: f32,
        pitch: f32,
    ) -> Result<(Ticked, [f64; 28]), EngineError> {
        self.request(|session| session.player_tick(keys, yaw, pitch))
    }

    /// `PREMOVE`: removes the player.
    pub fn remove_player(&mut self) -> Result<(), EngineError> {
        self.request(|session| {
            session.remove_player();
            Ok(())
        })
    }

    /// The player as a frame draws it (`Session::player_view`, 28 numbers);
    /// asked only of a world with a player.
    pub fn player_view(&mut self) -> Result<[f64; 28], EngineError> {
        self.request(|session| Ok(session.player_view()))
    }

    /// A click on the block at `pos` by a player looking with `yaw`
    /// (`Session::click`, the page's `click` request): the input the block's
    /// state took (`toggle`, `press`, `use`), or none.
    pub fn click(&mut self, pos: [i32; 3], yaw: f32) -> Result<Option<&'static str>, EngineError> {
        self.request(|session| {
            session
                .click(pos[0], pos[1], pos[2], yaw)
                .map(|taken| taken.map(|input| input.name()))
        })
    }

    /// Where a player can stand on top of the columns of the box `min` to
    /// `max` nearest to the feet position `near` (`Session::standing_spot`,
    /// the page's `stand` request): the feet, or none.
    pub fn standing_spot(
        &mut self,
        min: [i32; 3],
        max: [i32; 3],
        near: [f64; 3],
    ) -> Result<Option<[f64; 3]>, EngineError> {
        self.request(|session| Ok(session.standing_spot(min, max, near)))
    }

    /// The page's `edit` request: the commands placed on the world as it is,
    /// answered as a tick of no ticks (`Session::edit`).
    pub fn edit(&mut self, commands: &PlaceCommands) -> Result<Ticked, EngineError> {
        self.request(|session| session.edit(&commands.0))
    }

    /// The page's `export` request: the world as a Minecraft save holds it
    /// (`Session::export`: the game time, then each chunk with a block).
    pub fn export(&mut self) -> Result<Vec<u8>, EngineError> {
        self.request(|session| Ok(session.export()))
    }

    /// The input each entry of the last placement's palette takes, a byte
    /// each (`Session::base_inputs`: 0 none, 1 toggle, 2 press, 3 use);
    /// asked only after a placement that worked.
    pub fn base_inputs(&mut self) -> Result<Vec<u8>, EngineError> {
        self.request(|session| Ok(session.base_inputs()))
    }

    /// Whether a request panicked on this world: it then takes only a fresh
    /// placement (the page's `world.refused !== null`).
    pub fn spent(&self) -> bool {
        self.refused
    }

    /// `SURVIVE`: the blocks that cannot survive where they are, a line
    /// `x y z` each.
    pub fn unsupported(&mut self) -> Result<String, EngineError> {
        self.request(|session| Ok(session.unsupported()))
    }

    /// `STATE`: what the world holds besides its blocks
    /// (`McbenchEngine.state`), for the parity scripts.
    pub fn state(&mut self) -> Result<String, EngineError> {
        self.request(|session| Ok(session.state()))
    }
}

type Job = Box<dyn FnOnce(&mut WorldSession) -> bool + Send>;

/// One live world of the engine, on an engine thread of its own that runs
/// its requests one at a time in the order they were asked. The handle is
/// what the async runtime holds; [`EngineWorld::call`] sends work to the
/// thread and awaits its answer, so no caller runs block code on its own
/// stack.
///
/// A world's thread ends when the handle is stopped or dropped (after the
/// work already sent), or when work it was given panicked outside the
/// engine's requests (a bug of the caller's job; the engine's own panics are
/// caught by [`WorldSession`]). An ended world answers every call with
/// [`EngineError::Stopped`].
pub struct EngineWorld {
    jobs: Mutex<Option<std::sync::mpsc::Sender<Job>>>,
    alive: Arc<AtomicBool>,
    exited: watch::Receiver<bool>,
}

impl EngineWorld {
    /// A new, empty world on an engine thread of its own for its life (an
    /// idle one, or a new one).
    pub fn new(engine: Engine) -> Result<EngineWorld, EngineError> {
        let _ = engine;
        let (sender, receiver) = std::sync::mpsc::channel::<Job>();
        let alive = Arc::new(AtomicBool::new(true));
        let (exit, exited) = watch::channel(false);
        let flag = alive.clone();
        threads::run(Box::new(move |booted| {
            let mut world = WorldSession::new(booted.session());
            while let Ok(job) = receiver.recv() {
                if !job(&mut world) {
                    break;
                }
            }
            flag.store(false, Ordering::SeqCst);
            drop(receiver);
            drop(world);
            let _ = exit.send(true);
        }))?;
        Ok(EngineWorld {
            jobs: Mutex::new(Some(sender)),
            alive,
            exited,
        })
    }

    /// False once the world's thread ended or [`EngineWorld::stop`] was
    /// called: it takes no more work.
    pub fn alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst) && self.jobs.lock().unwrap().is_some()
    }

    /// Runs `work` on the world's thread after the work sent before it, and
    /// answers what it returns. Work that panics ends the world (its state
    /// is no longer known) and fails with [`EngineError::Stopped`].
    pub async fn call<T: Send + 'static>(
        &self,
        work: impl FnOnce(&mut WorldSession) -> T + Send + 'static,
    ) -> Result<T, EngineError> {
        let (answer, answered) = oneshot::channel();
        let alive = self.alive.clone();
        let job: Job =
            Box::new(
                move |world| match catch_unwind(AssertUnwindSafe(|| work(world))) {
                    Ok(value) => {
                        let _ = answer.send(Ok(value));
                        true
                    }
                    Err(payload) => {
                        // The world is ended before its caller hears so.
                        alive.store(false, Ordering::SeqCst);
                        let _ = answer.send(Err(EngineError::Stopped(format!(
                            "The engine stopped: {}",
                            mcbench_core::failure(&*payload)
                        ))));
                        false
                    }
                },
            );
        let sent = match self.jobs.lock().unwrap().as_ref() {
            Some(jobs) => jobs.send(job).is_ok(),
            None => false,
        };
        if !sent {
            return Err(stopped());
        }
        answered.await.unwrap_or_else(|_| Err(stopped()))
    }

    /// Ends the world: the work already sent runs to its end, nothing more
    /// is taken. Resolves once its thread has exited; a second stop waits
    /// for the same exit.
    pub async fn stop(&self) {
        self.jobs.lock().unwrap().take();
        let mut exited = self.exited.clone();
        let _ = exited.wait_for(|exited| *exited).await;
    }

    /// [`WorldSession::place`] on this world.
    pub async fn place(
        &self,
        commands: &PlaceCommands,
        fresh: bool,
    ) -> Result<Blueprint, EngineError> {
        let commands = commands.clone();
        self.call(move |world| world.place(&commands, fresh))
            .await?
    }

    /// [`WorldSession::tick`] on this world.
    pub async fn tick(&self, ticks: i32, light: bool) -> Result<Ticked, EngineError> {
        self.call(move |world| world.tick(ticks, light)).await?
    }

    /// [`WorldSession::input`] on this world.
    pub async fn input(&self, kind: &str, pos: [i32; 3], yaw: f32) -> Result<(), EngineError> {
        let kind = kind.to_string();
        self.call(move |world| world.input(&kind, pos, yaw)).await?
    }

    /// [`WorldSession::spawn_player`] on this world.
    pub async fn spawn_player(
        &self,
        pos: [f64; 3],
        yaw: f32,
        pitch: f32,
        flying: bool,
    ) -> Result<(), EngineError> {
        self.call(move |world| world.spawn_player(pos, yaw, pitch, flying))
            .await?
    }

    /// [`WorldSession::player_tick`] on this world.
    pub async fn player_tick(
        &self,
        keys: u32,
        yaw: f32,
        pitch: f32,
    ) -> Result<(Ticked, [f64; 28]), EngineError> {
        self.call(move |world| world.player_tick(keys, yaw, pitch))
            .await?
    }

    /// [`WorldSession::remove_player`] on this world.
    pub async fn remove_player(&self) -> Result<(), EngineError> {
        self.call(|world| world.remove_player()).await?
    }

    /// [`WorldSession::player_view`] on this world.
    pub async fn player_view(&self) -> Result<[f64; 28], EngineError> {
        self.call(|world| world.player_view()).await?
    }

    /// [`WorldSession::click`] on this world.
    pub async fn click(
        &self,
        pos: [i32; 3],
        yaw: f32,
    ) -> Result<Option<&'static str>, EngineError> {
        self.call(move |world| world.click(pos, yaw)).await?
    }

    /// [`WorldSession::standing_spot`] on this world.
    pub async fn standing_spot(
        &self,
        min: [i32; 3],
        max: [i32; 3],
        near: [f64; 3],
    ) -> Result<Option<[f64; 3]>, EngineError> {
        self.call(move |world| world.standing_spot(min, max, near))
            .await?
    }

    /// [`WorldSession::edit`] on this world.
    pub async fn edit(&self, commands: &PlaceCommands) -> Result<Ticked, EngineError> {
        let commands = commands.clone();
        self.call(move |world| world.edit(&commands)).await?
    }

    /// [`WorldSession::export`] on this world.
    pub async fn export(&self) -> Result<Vec<u8>, EngineError> {
        self.call(|world| world.export()).await?
    }

    /// [`WorldSession::base_inputs`] on this world.
    pub async fn base_inputs(&self) -> Result<Vec<u8>, EngineError> {
        self.call(|world| world.base_inputs()).await?
    }

    /// [`WorldSession::spent`] of this world, after the work sent before it.
    pub async fn spent(&self) -> Result<bool, EngineError> {
        self.call(|world| world.spent()).await
    }

    /// [`WorldSession::unsupported`] on this world.
    pub async fn unsupported(&self) -> Result<String, EngineError> {
        self.call(|world| world.unsupported()).await?
    }

    /// [`WorldSession::state`] on this world.
    pub async fn state(&self) -> Result<String, EngineError> {
        self.call(|world| world.state()).await?
    }
}

impl Drop for EngineWorld {
    fn drop(&mut self) {
        // The thread ends after the work already sent.
        self.jobs.lock().unwrap().take();
    }
}

fn stopped() -> EngineError {
    EngineError::Stopped("The engine was stopped".into())
}
