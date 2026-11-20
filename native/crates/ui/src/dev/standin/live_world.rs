//! The stand-in's live world (pane agent): every command of the live
//! view's seam answered as the agreed notes say the core answers it
//! (`ui-seam-viewer.md`, the core's answers), over the fake pane
//! ([`crate::dev::pane`]) and its model worlds, with a failure on demand for
//! each. Ticks advance on a clock in the dev binary ([`StandIn::live_clock`])
//! and by hand in the tests ([`StandIn::live_ticks`]).

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use mcbench_render::Clip;
use mcbench_render::live::{Framing, LivePatch, LiveRenderer, PlayerView, Shown};

use super::StandIn;
use crate::dev::pane::{FakePane, FakeRenderer, LEVER, ModelWorld};
use crate::{
    LiveCommand, LivePhase, LiveState, PaneHandle, PlayerState, Running, SelectionDescription,
    Update, ViewKey, ViewerInput,
};

/// The engine's y offset of the stand-in's worlds (the drawing crate's
/// tests use the same).
pub const Y_OFFSET: i32 = 64;
/// The renderer's sentence without a graphics adapter.
pub const NO_ADAPTER: &str =
    "No graphics adapter: no Vulkan, Metal or DirectX 12 device was found (a fixture error)";
/// A placement that fails.
pub const PLACE_FAILED: &str =
    "Minecraft kept the previous world: the build has 70000 commands (a fixture error)";
/// The engine stopping a world.
pub const STOPPED: &str = "The live world stopped: the engine ran out of memory (a fixture error)";
/// A click the engine refused.
pub const CLICK_FAILED: &str =
    "The click could not be run: the world is being placed (a fixture error)";
/// A spawn that fails.
pub const SPAWN_FAILED: &str = "There is no room to stand beside this build (a fixture error)";
/// The player fell out (the page's sentence).
pub const FELL_OUT: &str = "Fell out of the world";
/// An export that fails.
pub const EXPORT_FAILED: &str = "The file could not be written: the disk is full (a fixture error)";
/// The selection route's 400.
pub const SELECTION_REFUSED: &str =
    "A selection needs integer corners within +/-4096 with min at most max";
/// A command for a stopped world.
pub const STOPPED_REFUSED: &str = "The live world stopped; place it again with Try again";
/// A command with no world on show, and an empty world's export (the
/// core's sentence).
pub const NOTHING_SHOWN: &str = "There is no live world on show";
/// Before setup has given Minecraft's tables (the core's sentence).
pub const DATA_NOT_READY: &str = "Minecraft's data is not ready yet";
/// A block the table lacks (the core's sentence).
pub const UNKNOWN_BLOCK: &str = "Unknown block";
/// The real core's answer while no renderer is joined to the app.
pub const NOT_JOINED: &str = "The 3D viewer has not joined the app yet";

/// What the stand-in fails on demand (the dev binary's `--live`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LiveFailures {
    /// No adapter: `Update::LivePane(Err)`.
    pub no_adapter: bool,
    /// Every placement fails.
    pub place: bool,
    /// The engine stops the world after this many ticks.
    pub stop_after: Option<u64>,
    /// Clicks fail.
    pub click: bool,
    /// Spawns fail.
    pub spawn: bool,
    /// Selections fail with the route's 400.
    pub selection: bool,
    /// Exports fail.
    pub export: bool,
    /// A placement takes this long (the cover's `Preparing`).
    pub slow_place: Option<Duration>,
    /// Setup has not given Minecraft's tables: selections and the
    /// schematic answer [`DATA_NOT_READY`].
    pub data_not_ready: bool,
    /// No renderer is joined, as the real core today: `LivePane(Err)` with
    /// [`NOT_JOINED`] and every `Show` failed with it.
    pub not_joined: bool,
}

/// The live world's state in the stand-in.
/// pane agent: the dev binary's real renderer and its kept world.
struct RealWorld {
    renderer: Arc<dyn LiveRenderer>,
    world: String,
    bounds: Clip,
    shutdown: Arc<dyn Fn() + Send + Sync>,
}

#[derive(Default)]
pub struct LiveWorld {
    pane: Option<Arc<FakePane>>,
    /// pane agent: a real renderer in place of the fake pane (the dev
    /// binary's `--blueprint`), with its one kept world.
    real: Option<RealWorld>,
    failures: LiveFailures,
    view: Option<ViewKey>,
    world: Option<ModelWorld>,
    revision: u64,
    phase: Option<LivePhase>,
    running: Option<Running>,
    tick: u64,
    player: Option<PlayerState>,
    standing: Option<PlayerView>,
    input: (u32, u32, f32, f32),
    lever_on: bool,
    changed: bool,
    last_publish: Option<Instant>,
    /// `Core::shutdown`'s count: the clock ends once it is set.
    shutdowns: u32,
    shown_once: Vec<ViewKey>,
}

impl LiveWorld {
    fn state(&self) -> LiveState {
        LiveState {
            view: self.view.clone(),
            revision: self.view.as_ref().map(|_| self.revision),
            phase: self.phase.clone().unwrap_or(LivePhase::Idle),
            running: self.running.clone(),
            tick: self.tick,
            player: self.player.unwrap_or(PlayerState::None),
            bounds: self.world.as_ref().map(|w| w.bounds),
            y_offset: Y_OFFSET,
            blocks: self
                .world
                .as_ref()
                // par-chat: an empty chat's world has no block (the core's
                // `blocks: 0`), its ground aside.
                .map_or(0, |w| {
                    if w.boxes.is_empty() {
                        0
                    } else {
                        w.count(w.bounds[0], w.bounds[1]).0
                    }
                }),
            layout: self.world.as_ref().map_or(Vec::new(), |w| w.layout.clone()),
        }
    }
}

fn player_view(at: [f64; 3], yaw: f32, pitch: f32, flying: bool) -> PlayerView {
    PlayerView {
        pos: at,
        old_pos: at,
        delta: [0.0; 3],
        y_rot: yaw,
        x_rot: pitch,
        y_rot_o: yaw,
        x_rot_o: pitch,
        width: 0.6,
        height: 1.8,
        eye_height: 1.62,
        pose: 0,
        flying,
        sprinting: false,
        crouching: false,
        swimming: false,
        on_ground: !flying,
        in_water: false,
        in_lava: false,
        eye_in_water: false,
        eye_in_lava: false,
        horizontal_collision: false,
        fall_distance: 0.0,
    }
}

impl StandIn {
    /// Gives the stand-in its fake pane; `attach` then publishes
    /// `Update::LivePane(Ok)` (or `Err` with [`NO_ADAPTER`] when
    /// [`LiveFailures::no_adapter`] is set).
    pub fn set_pane(&self, pane: Arc<FakePane>, failures: LiveFailures) {
        let mut live = self.live_world.lock().expect("lock");
        live.pane = Some(pane);
        live.failures = failures;
    }

    // pane agent: the dev binary's real renderer.
    /// Draws every chat's view with a real renderer and the one world it
    /// keeps (`world`, inside `bounds`), in place of the fake pane: frames,
    /// picks and the camera are the renderer's; the stand-in's ticks change
    /// nothing in it. `shutdown` ends it at `Core::shutdown`.
    pub fn set_real(
        &self,
        renderer: Arc<dyn LiveRenderer>,
        world: String,
        bounds: Clip,
        shutdown: Box<dyn Fn() + Send + Sync>,
    ) {
        self.live_world.lock().expect("lock").real = Some(RealWorld {
            renderer,
            world,
            bounds,
            shutdown: Arc::from(shutdown),
        });
    }

    /// `Core::shutdown`: the clock ends at its next tick and the fake
    /// pane stops, as the real core stops its live world and renderer.
    pub(super) fn stop_live(&self) {
        let mut live = self.live_world.lock().expect("lock");
        live.shutdowns += 1;
        // The dev binary's checks read it from stderr (check_quit.py).
        eprintln!("[standin] shutdown");
        let pane = live.pane.clone();
        let real = live.real.as_ref().map(|r| r.shutdown.clone());
        drop(live);
        if let Some(pane) = pane {
            pane.stop();
        }
        if let Some(shutdown) = real {
            shutdown();
        }
    }

    /// The keys the last `PlayerInput` left held, as the core holds them
    /// until the next one.
    pub fn held_keys(&self) -> u32 {
        self.live_world.lock().expect("lock").input.0
    }

    /// How many times `Core::shutdown` was called.
    pub fn shutdowns(&self) -> u32 {
        self.live_world.lock().expect("lock").shutdowns
    }

    /// Changes what fails from now on.
    pub fn set_live_failures(&self, failures: LiveFailures) {
        self.live_world.lock().expect("lock").failures = failures;
    }

    /// The pane's handle, published at attach.
    pub(super) fn publish_pane(&self) {
        let live = self.live_world.lock().expect("lock");
        if let Some(real) = &live.real {
            let update = Update::LivePane(Ok(PaneHandle(real.renderer.live())));
            drop(live);
            return self.window().publish(update);
        }
        let Some(pane) = live.pane.clone() else {
            return;
        };
        let update = if live.failures.not_joined {
            Update::LivePane(Err(NOT_JOINED.into()))
        } else if live.failures.no_adapter {
            Update::LivePane(Err(NO_ADAPTER.into()))
        } else {
            Update::LivePane(Ok(PaneHandle(FakeRenderer(pane).live())))
        };
        drop(live);
        self.window().publish(update);
    }

    fn renderer(&self) -> Option<Arc<dyn LiveRenderer>> {
        let live = self.live_world.lock().expect("lock");
        if let Some(real) = &live.real {
            return Some(real.renderer.clone());
        }
        live.pane
            .clone()
            .map(|p| Arc::new(FakeRenderer(p)) as Arc<dyn LiveRenderer>)
    }

    fn publish_live(&self) {
        let mut live = self.live_world.lock().expect("lock");
        live.last_publish = Some(Instant::now());
        let state = live.state();
        drop(live);
        self.window().publish(Update::Live(state));
    }

    /// A made-up icon: a 16 by 16 PNG of one colour for each of a few
    /// names (the blocks of the model worlds), `Unknown block` for others,
    /// `Minecraft's data is not ready yet` while [`LiveFailures::data_not_ready`].
    pub(super) fn icon_of(&self, id: &str) -> Result<Vec<u8>, String> {
        if self
            .live_world
            .lock()
            .expect("lock")
            .failures
            .data_not_ready
        {
            return Err(DATA_NOT_READY.into());
        }
        let colour: [u8; 3] = match id {
            "stone" => [128, 128, 128],
            "stone_bricks" => [118, 118, 118],
            "grass_block" => [90, 150, 70],
            "oak_planks" => [170, 120, 70],
            "lever" => [120, 90, 60],
            "redstone_lamp" => [90, 60, 40],
            "target" => [200, 60, 60],
            _ => return Err(UNKNOWN_BLOCK.into()),
        };
        let picture = image::RgbaImage::from_pixel(
            16,
            16,
            image::Rgba([colour[0], colour[1], colour[2], 255]),
        );
        let mut bytes = std::io::Cursor::new(Vec::new());
        picture
            .write_to(&mut bytes, image::ImageFormat::Png)
            .map_err(|e| format!("The made-up icon could not be written: {e}"))?;
        Ok(bytes.into_inner())
    }

    /// The bench round's members (`bench:<bench>:<round>`), by slot.
    fn members(&self, bench: &str) -> Vec<String> {
        let data = self.data.lock().expect("lock");
        let mut seats: Vec<(u32, String)> = data
            .chats
            .iter()
            .filter_map(|c| {
                c.row
                    .bench
                    .as_ref()
                    .filter(|b| b.bench == bench)
                    .map(|b| (b.slot, c.row.id.clone()))
            })
            .collect();
        seats.sort();
        seats.into_iter().map(|(_, id)| id).collect()
    }

    fn world_of(&self, view: &ViewKey) -> Result<ModelWorld, String> {
        if let Some(rest) = view.0.strip_prefix("bench:") {
            let bench = rest.rsplit_once(':').map_or(rest, |(b, _)| b);
            let members = self.members(bench);
            if members.is_empty() {
                return Err(format!("Bench not found: {bench}"));
            }
            return Ok(ModelWorld::bench(&members));
        }
        let data = self.data.lock().expect("lock");
        match data.chats.iter().find(|c| c.row.id == view.0) {
            // par-chat: a new chat's row with no message yet shows its
            // ground and no block (the core's `Shown` with 0 blocks and
            // nothing running).
            Some(c) if c.rows.is_empty() => Ok(ModelWorld {
                boxes: Vec::new(),
                ..ModelWorld::chat()
            }),
            Some(_) => Ok(ModelWorld::chat()),
            None => Err("Conversation not found".into()),
        }
    }

    /// One live command (`Command::Live`).
    pub(super) fn live_command(&self, command: LiveCommand) {
        match command {
            LiveCommand::Show { view: None, .. } => {
                if let Some(r) = self.renderer() {
                    r.live_show(None);
                }
                let mut live = self.live_world.lock().expect("lock");
                live.view = None;
                live.world = None;
                live.phase = Some(LivePhase::Idle);
                live.running = None;
                live.player = None;
                live.standing = None;
                live.tick = 0;
                drop(live);
                self.publish_live();
            }
            LiveCommand::Show {
                view: Some(view),
                afresh,
            } => {
                {
                    let mut live = self.live_world.lock().expect("lock");
                    if !afresh
                        && live.view.as_ref() == Some(&view)
                        && live.phase == Some(LivePhase::Shown)
                    {
                        drop(live);
                        return self.publish_live();
                    }
                    live.view = Some(view.clone());
                    live.world = None;
                    live.phase = Some(LivePhase::Preparing);
                    live.running = None;
                    live.player = None;
                    live.standing = None;
                    live.tick = 0;
                }
                if let Some(r) = self.renderer() {
                    r.live_show(None);
                }
                self.publish_live();
                let slow = self.live_world.lock().expect("lock").failures.slow_place;
                if let Some(slow) = slow {
                    std::thread::sleep(slow);
                }
                self.place(view);
            }
            LiveCommand::Play | LiveCommand::Pause | LiveCommand::Step | LiveCommand::Restart => {
                self.toolbar(command)
            }
        }
    }

    fn place(&self, view: ViewKey) {
        let world = self.world_of(&view);
        let failures = self.live_world.lock().expect("lock").failures.clone();
        let fails = failures.place;
        let mut live = self.live_world.lock().expect("lock");
        if live.view.as_ref() != Some(&view) {
            return;
        }
        let error = match (&world, fails) {
            _ if failures.not_joined => Some(NOT_JOINED.to_string()),
            (Err(e), _) => Some(e.clone()),
            (Ok(_), true) => Some(PLACE_FAILED.to_string()),
            _ => None,
        };
        if let Some(error) = error {
            live.phase = Some(LivePhase::Failed {
                error,
                nothing_shown: !live.shown_once.contains(&view),
            });
            // A failed placement lets a standing player go with it.
            if live.standing.take().is_some() {
                live.player = Some(PlayerState::None);
            }
            drop(live);
            return self.publish_live();
        }
        let world = world.expect("checked above");
        // The core's framing: at once for a view's first placement, else the
        // camera stays (a restart; a turn's moved bounds fly over 700 ms).
        let framing = if live.shown_once.contains(&view) {
            Framing::Keep
        } else {
            Framing::AtOnce
        };
        live.revision += 1;
        live.world = Some(world.clone());
        live.phase = Some(LivePhase::Shown);
        // par-chat: a world with no block runs nothing.
        live.running = (!world.boxes.is_empty()).then_some(Running::Playing);
        live.tick = 0;
        live.lever_on = false;
        if !live.shown_once.contains(&view) {
            live.shown_once.push(view.clone());
        }
        let revision = live.revision;
        let pane = live.pane.clone();
        let real = live
            .real
            .as_ref()
            .map(|r| (r.renderer.clone(), r.world.clone(), r.bounds));
        drop(live);
        if let Some((renderer, world, bounds)) = real {
            renderer.live_show(Some(Shown {
                world,
                view,
                revision,
                bounds,
                framing,
            }));
        } else if let Some(pane) = pane {
            let name = format!("live:{}", view.0);
            pane.keep(&name, world.clone());
            FakeRenderer(pane).live_show(Some(Shown {
                world: name,
                view,
                revision,
                bounds: Clip {
                    min: world.bounds[0],
                    max: world.bounds[1],
                },
                framing,
            }));
        }
        self.publish_live();
    }

    fn toolbar(&self, command: LiveCommand) {
        let mut live = self.live_world.lock().expect("lock");
        if live.phase != Some(LivePhase::Shown) {
            drop(live);
            self.window().publish(Update::Error(NOTHING_SHOWN.into()));
            return self.publish_live();
        }
        if matches!(live.running, Some(Running::Stopped(_))) && command != LiveCommand::Restart {
            drop(live);
            self.window().publish(Update::Error(STOPPED_REFUSED.into()));
            return self.publish_live();
        }
        match command {
            LiveCommand::Play => live.running = Some(Running::Playing),
            LiveCommand::Pause => live.running = Some(Running::Paused),
            LiveCommand::Step => {
                drop(live);
                self.tick_once();
                return self.publish_live();
            }
            LiveCommand::Restart => {
                let view = live.view.clone().expect("a shown world has a view");
                let paused = live.running == Some(Running::Paused);
                let standing = live.standing;
                drop(live);
                self.place(view);
                let mut live = self.live_world.lock().expect("lock");
                if paused && live.phase == Some(LivePhase::Shown) {
                    live.running = Some(Running::Paused);
                }
                // A fresh placement that succeeds keeps the player; one that
                // fails lets it go, as the core does.
                if let Some(at) = standing {
                    if live.phase == Some(LivePhase::Shown) {
                        live.standing = Some(at);
                        live.player = Some(PlayerState::Standing);
                    } else {
                        live.standing = None;
                        live.player = Some(PlayerState::None);
                    }
                }
                drop(live);
                return self.publish_live();
            }
            LiveCommand::Show { .. } => unreachable!("handled by live_command"),
        }
        drop(live);
        self.publish_live();
    }

    /// One tick: the engine's patch to the pane, the player moved by its
    /// input; a world set to stop stops.
    fn tick_once(&self) {
        self.tick(false);
    }

    /// A tick; `clock` ticks only a world that plays, decided under the
    /// same lock as the tick (a Pause cannot fall between). Whether it
    /// ticked.
    fn tick(&self, clock: bool) -> bool {
        let mut live = self.live_world.lock().expect("lock");
        if live.phase != Some(LivePhase::Shown) || (clock && live.running != Some(Running::Playing))
        {
            return false;
        }
        live.tick += 1;
        if live.failures.stop_after.is_some_and(|n| live.tick >= n) {
            live.running = Some(Running::Stopped(STOPPED.into()));
            live.changed = true;
            return true;
        }
        let (keys, taps, yaw, pitch) = live.input;
        live.input.1 = 0;
        if let Some(view) = &mut live.standing {
            let held = keys | taps;
            let y = (yaw as f64).to_radians();
            let (fx, fz) = (-y.sin() * 0.2, y.cos() * 0.2);
            view.old_pos = view.pos;
            if held & 1 != 0 {
                view.pos[0] += fx;
                view.pos[2] += fz;
            }
            if held & 2 != 0 {
                view.pos[0] -= fx;
                view.pos[2] -= fz;
            }
            view.y_rot_o = view.y_rot;
            view.x_rot_o = view.x_rot;
            view.y_rot = yaw;
            view.x_rot = pitch;
        }
        let patch = LivePatch {
            view: live.view.clone().expect("a shown world has a view"),
            revision: live.revision,
            tick: live.tick,
            changes: vec![u8::from(live.lever_on)],
            entities: Vec::new(),
            player: live.standing,
            framing: None,
        };
        let pane = live.pane.clone();
        drop(live);
        if let Some(pane) = pane {
            FakeRenderer(pane).live_patch(patch);
        }
        true
    }

    /// The clock's tick: one tick if the world plays now; whether it ticked.
    fn clock_tick(&self) -> bool {
        self.tick(true)
    }

    /// `count` ticks, by hand, then the state (the tests).
    pub fn live_ticks(&self, count: u64) {
        for _ in 0..count {
            self.tick_once();
        }
        self.publish_live();
    }

    /// The player fell below y −128: respawned, with the page's note.
    pub fn fall_out(&self) {
        self.window().publish(Update::PlayerNote(FELL_OUT.into()));
    }

    /// Ticks at 20 a second while playing, publishing the state at most
    /// every 250 ms with the last count always sent (the dev binary).
    pub fn live_clock(self: &Arc<Self>) {
        let core = Arc::downgrade(self);
        std::thread::Builder::new()
            .name("standin-ticks".into())
            .spawn(move || {
                let start = Instant::now();
                let mut n: u32 = 0;
                let mut owed = false;
                loop {
                    n += 1;
                    let due = start + Duration::from_millis(50) * n;
                    std::thread::sleep(due.saturating_duration_since(Instant::now()));
                    let Some(core) = core.upgrade() else { return };
                    let live = core.live_world.lock().expect("lock");
                    if live.shutdowns > 0 {
                        return;
                    }
                    let playing = live.running == Some(Running::Playing)
                        && live.phase == Some(LivePhase::Shown);
                    let changed = live.changed;
                    let recent = live
                        .last_publish
                        .is_some_and(|t| t.elapsed() < Duration::from_millis(250));
                    drop(live);
                    // The clock's tick runs only if the world still plays
                    // when it is taken (a Pause between the look above and
                    // the tick ticks nothing).
                    if playing && core.clock_tick() {
                        owed = true;
                    }
                    let stopped_now =
                        std::mem::take(&mut core.live_world.lock().expect("lock").changed);
                    if (owed && !recent) || changed || stopped_now {
                        owed = false;
                        core.publish_live();
                    }
                }
            })
            .expect("the stand-in's clock starts");
    }

    /// One viewer input (`Command::Viewer`).
    pub(super) fn viewer_input(&self, input: ViewerInput) {
        let mut live = self.live_world.lock().expect("lock");
        match input {
            ViewerInput::Click { view, block, .. } => {
                if live.view.as_ref() != Some(&view) || live.world.is_none() {
                    drop(live);
                    return self.window().publish(Update::Clicked {
                        view,
                        block,
                        name: String::new(),
                        input: None,
                    });
                }
                if live.failures.click {
                    drop(live);
                    // The core: `Error`, then `Clicked` with no input.
                    self.window().publish(Update::Error(CLICK_FAILED.into()));
                    return self.window().publish(Update::Clicked {
                        view,
                        block,
                        name: String::new(),
                        input: None,
                    });
                }
                let found = live.world.as_ref().and_then(|w| w.block(block));
                let (name, input) = found.unwrap_or_else(|| ("Air".into(), None));
                if block == LEVER {
                    live.lever_on = !live.lever_on;
                }
                drop(live);
                self.window().publish(Update::Clicked {
                    view,
                    block,
                    name,
                    input: input.map(str::to_string),
                });
            }
            ViewerInput::EnterPlayer {
                view,
                target,
                yaw,
                pitch,
                ..
            } => {
                if live.view.as_ref() != Some(&view) {
                    drop(live);
                    return self.window().publish(Update::Error(
                        "Player mode needs the world it was asked for on show".into(),
                    ));
                }
                live.player = Some(PlayerState::Spawning);
                let fails = live.failures.spawn;
                drop(live);
                self.publish_live();
                let mut live = self.live_world.lock().expect("lock");
                if fails {
                    live.player = Some(PlayerState::None);
                    drop(live);
                    self.window()
                        .publish(Update::PlayerNote(SPAWN_FAILED.into()));
                    return self.publish_live();
                }
                // The core takes the look from the spot beside the build
                // (`besideBuild`), not from the window: it faces the build's
                // middle; the window's yaw and pitch are not used.
                let _ = (yaw, pitch);
                let bounds = live.world.as_ref().map(|w| w.bounds).expect("shown");
                let at = [bounds[0][0] as f64 - 2.0, target[1].max(64.0), target[2]];
                let middle = [0, 1, 2].map(|a| (bounds[0][a] + bounds[1][a] + 1) as f64 / 2.0);
                let (dx, dz) = (middle[0] - at[0], middle[2] - at[2]);
                let face = (-dx).atan2(dz).to_degrees() as f32;
                live.standing = Some(player_view(at, face, 0.0, false));
                live.player = Some(PlayerState::Standing);
                live.input = (0, 0, face, 0.0);
                drop(live);
                self.tick_once_paused_patch();
                self.publish_live();
            }
            ViewerInput::SpawnPlayer {
                at,
                yaw,
                pitch,
                flying,
                ..
            } => {
                live.player = Some(PlayerState::Spawning);
                drop(live);
                self.publish_live();
                let mut live = self.live_world.lock().expect("lock");
                live.standing = Some(player_view(at, yaw, pitch, flying));
                live.player = Some(PlayerState::Standing);
                drop(live);
                self.tick_once_paused_patch();
                self.publish_live();
            }
            ViewerInput::PlayerInput {
                keys,
                taps,
                yaw,
                pitch,
            } => {
                let held = live.input.1;
                live.input = (keys, held | taps, yaw, pitch);
            }
            ViewerInput::LeavePlayer => {
                live.player = Some(PlayerState::None);
                live.standing = None;
                drop(live);
                self.publish_live();
            }
        }
    }

    /// The player's view handed to the pane at once (a spawn), as the
    /// core's spawn patch does.
    fn tick_once_paused_patch(&self) {
        let live = self.live_world.lock().expect("lock");
        let Some(view) = live.view.clone() else {
            return;
        };
        let patch = LivePatch {
            view,
            revision: live.revision,
            tick: live.tick,
            changes: vec![u8::from(live.lever_on)],
            entities: Vec::new(),
            player: live.standing,
            framing: None,
        };
        let pane = live.pane.clone();
        drop(live);
        if let Some(pane) = pane {
            FakeRenderer(pane).live_patch(patch);
        }
    }

    /// Names a selection box, in build coordinates.
    pub(super) fn describe_selection(
        &self,
        chat: String,
        request: u64,
        min: [i64; 3],
        max: [i64; 3],
    ) {
        let known = self
            .data
            .lock()
            .expect("lock")
            .chats
            .iter()
            .any(|c| c.row.id == chat);
        let live = self.live_world.lock().expect("lock");
        if live.failures.data_not_ready {
            drop(live);
            return self.window().publish(Update::SelectionDescribed {
                request,
                result: Err(DATA_NOT_READY.into()),
            });
        }
        let in_range =
            (0..3).all(|a| min[a] <= max[a] && min[a].abs() <= 4096 && max[a].abs() <= 4096);
        let result = if !known {
            Err("Conversation not found".to_string())
        } else if live.failures.selection || !in_range {
            Err(SELECTION_REFUSED.to_string())
        } else {
            let world = ModelWorld::chat();
            let to = |c: [i64; 3]| [c[0] as i32, c[1] as i32 + Y_OFFSET, c[2] as i32];
            let (blocks, most) = world.count(to(min), to(max));
            Ok(SelectionDescription {
                blocks,
                block: most
                    .as_ref()
                    .map(|n| format!("minecraft:{}", n.to_lowercase().replace(' ', "_"))),
                label: most.map_or_else(|| "Empty space".to_string(), |n| format!("{n}, middle")),
            })
        };
        drop(live);
        self.window()
            .publish(Update::SelectionDescribed { request, result });
    }

    /// Writes an export to `to` through a temporary file beside it.
    pub(super) fn export(&self, what: &str, to: &Path, known: Result<(), String>) {
        let (fails, not_ready) = {
            let live = self.live_world.lock().expect("lock");
            (live.failures.export, live.failures.data_not_ready)
        };
        let result = if let Err(e) = known {
            Err(e)
        } else if not_ready && what == "schematic" {
            Err(DATA_NOT_READY.to_string())
        } else if fails {
            Err(EXPORT_FAILED.to_string())
        } else {
            let temporary = to.with_extension("part");
            std::fs::write(&temporary, format!("mcbench stand-in {what}\n"))
                .and_then(|()| std::fs::rename(&temporary, to))
                .map(|()| to.to_path_buf())
                .map_err(|e| format!("The file could not be written: {e}"))
        };
        self.window().publish(Update::Exported(result));
    }

    /// Whether a chat exists, for the schematic export (the route's 404).
    pub(super) fn chat_known(&self, chat: &str) -> Result<(), String> {
        self.data
            .lock()
            .expect("lock")
            .chats
            .iter()
            .any(|c| c.row.id == chat)
            .then_some(())
            .ok_or_else(|| "Conversation not found".to_string())
    }

    /// Whether the live world shows `view`, for the world export.
    pub(super) fn view_shown(&self, view: &ViewKey) -> Result<(), String> {
        let live = self.live_world.lock().expect("lock");
        (live.view.as_ref() == Some(view) && live.phase == Some(LivePhase::Shown))
            .then_some(())
            .ok_or_else(|| NOTHING_SHOWN.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Command, Core, Window};
    use std::sync::Mutex;

    struct Inbox(Mutex<Vec<Update>>);

    impl Window for Inbox {
        fn publish(&self, update: Update) {
            self.0.lock().expect("lock").push(update);
        }
    }

    fn core(failures: LiveFailures) -> (StandIn, Arc<Inbox>, Arc<FakePane>) {
        let texts = crate::dev::fixtures::Texts::new(Path::new("lamp.png"));
        let core = StandIn::new(
            crate::dev::fixtures::gate(&texts),
            texts.streamed.clone(),
            Duration::ZERO,
            None,
        );
        let pane = FakePane::start(crate::dev::pane::Behaviour::default());
        core.set_pane(pane.clone(), failures);
        let inbox = Arc::new(Inbox(Mutex::new(Vec::new())));
        core.attach(inbox.clone());
        (core, inbox, pane)
    }

    fn taken(inbox: &Inbox) -> Vec<Update> {
        std::mem::take(&mut *inbox.0.lock().expect("lock"))
    }

    fn live(updates: &[Update]) -> Vec<LiveState> {
        updates
            .iter()
            .filter_map(|u| match u {
                Update::Live(s) => Some(s.clone()),
                _ => None,
            })
            .collect()
    }

    const CHAT: &str = "chat-gate";

    fn shown(core: &StandIn, inbox: &Inbox) {
        core.send(Command::Live(LiveCommand::Show {
            view: Some(ViewKey(CHAT.into())),
            afresh: false,
        }));
        taken(inbox);
    }

    #[test]
    fn attach_hands_over_the_pane_or_the_adapters_sentence() {
        let (_, inbox, pane) = core(LiveFailures::default());
        assert!(
            taken(&inbox)
                .iter()
                .any(|u| matches!(u, Update::LivePane(Ok(_))))
        );
        pane.stop();
        let (_, inbox, pane) = core(LiveFailures {
            no_adapter: true,
            ..LiveFailures::default()
        });
        assert!(
            taken(&inbox)
                .iter()
                .any(|u| matches!(u, Update::LivePane(Err(e)) if e == NO_ADAPTER))
        );
        pane.stop();
    }

    #[test]
    fn show_ends_in_preparing_then_shown_playing_at_tick_0() {
        let (core, inbox, pane) = core(LiveFailures::default());
        taken(&inbox);
        core.send(Command::Live(LiveCommand::Show {
            view: Some(ViewKey(CHAT.into())),
            afresh: false,
        }));
        let states = live(&taken(&inbox));
        assert_eq!(states.len(), 2);
        assert_eq!(states[0].phase, LivePhase::Preparing);
        assert_eq!(states[1].phase, LivePhase::Shown);
        assert_eq!(states[1].running, Some(Running::Playing));
        assert_eq!(states[1].tick, 0);
        assert_eq!(states[1].y_offset, Y_OFFSET);
        assert!(states[1].blocks > 0);
        pane.stop();
    }

    #[test]
    fn a_failed_placement_ends_in_the_phase_with_no_error_before_it() {
        let (core, inbox, pane) = core(LiveFailures {
            place: true,
            ..LiveFailures::default()
        });
        taken(&inbox);
        core.send(Command::Live(LiveCommand::Show {
            view: Some(ViewKey(CHAT.into())),
            afresh: false,
        }));
        let updates = taken(&inbox);
        assert!(!updates.iter().any(|u| matches!(u, Update::Error(_))));
        assert_eq!(
            live(&updates).last().unwrap().phase,
            LivePhase::Failed {
                error: PLACE_FAILED.into(),
                nothing_shown: true
            }
        );
        pane.stop();
    }

    #[test]
    fn the_toolbar_ends_in_the_state_it_changed() {
        let (core, inbox, pane) = core(LiveFailures::default());
        shown(&core, &inbox);
        core.send(Command::Live(LiveCommand::Pause));
        assert_eq!(live(&taken(&inbox))[0].running, Some(Running::Paused));
        core.send(Command::Live(LiveCommand::Step));
        let step = live(&taken(&inbox));
        assert_eq!(
            (step[0].tick, step[0].running.clone()),
            (1, Some(Running::Paused))
        );
        core.send(Command::Live(LiveCommand::Restart));
        let restart = live(&taken(&inbox));
        let last = restart.last().unwrap();
        assert_eq!(
            (last.tick, last.running.clone()),
            (0, Some(Running::Paused))
        );
        core.send(Command::Live(LiveCommand::Play));
        assert_eq!(live(&taken(&inbox))[0].running, Some(Running::Playing));
        pane.stop();
    }

    #[test]
    fn a_stopped_world_takes_only_a_fresh_placement() {
        let (core, inbox, pane) = core(LiveFailures {
            stop_after: Some(2),
            ..LiveFailures::default()
        });
        shown(&core, &inbox);
        core.live_ticks(2);
        let state = live(&taken(&inbox)).pop().unwrap();
        assert_eq!(state.running, Some(Running::Stopped(STOPPED.into())));
        core.send(Command::Live(LiveCommand::Play));
        let updates = taken(&inbox);
        assert!(matches!(&updates[0], Update::Error(e) if e == STOPPED_REFUSED));
        assert!(matches!(&updates[1], Update::Live(_)));
        pane.stop();
    }

    #[test]
    fn a_click_names_the_block_and_its_input() {
        let (core, inbox, pane) = core(LiveFailures::default());
        shown(&core, &inbox);
        let click = |block| {
            Command::Viewer(ViewerInput::Click {
                view: ViewKey(CHAT.into()),
                revision: 1,
                block,
                yaw: 0.0,
            })
        };
        core.send(click(LEVER));
        assert!(
            matches!(&taken(&inbox)[..], [Update::Clicked { name, input: Some(i), .. }] if name == "Lever" && i == "toggle")
        );
        core.send(click([5, 64, 5]));
        assert!(
            matches!(&taken(&inbox)[..], [Update::Clicked { name, input: None, .. }] if name == "Stone Bricks")
        );
        core.send(Command::Viewer(ViewerInput::Click {
            view: ViewKey("elsewhere".into()),
            revision: 1,
            block: LEVER,
            yaw: 0.0,
        }));
        assert!(
            matches!(&taken(&inbox)[..], [Update::Clicked { name, input: None, .. }] if name.is_empty())
        );
        core.set_live_failures(LiveFailures {
            click: true,
            ..LiveFailures::default()
        });
        core.send(click(LEVER));
        assert!(
            matches!(&taken(&inbox)[..], [Update::Error(e), Update::Clicked { input: None, .. }] if e == CLICK_FAILED)
        );
        pane.stop();
    }

    #[test]
    fn player_mode_spawns_stands_and_leaves_and_input_ends_in_nothing() {
        let (core, inbox, pane) = core(LiveFailures::default());
        shown(&core, &inbox);
        core.send(Command::Viewer(ViewerInput::EnterPlayer {
            view: ViewKey(CHAT.into()),
            target: [12.0, 64.0, 12.0],
            rotation: 0.0,
            yaw: 0.0,
            pitch: 0.0,
        }));
        let states = live(&taken(&inbox));
        assert_eq!(
            states.iter().map(|s| s.player).collect::<Vec<_>>(),
            [PlayerState::Spawning, PlayerState::Standing]
        );
        core.send(Command::Viewer(ViewerInput::PlayerInput {
            keys: 1,
            taps: 0,
            yaw: 10.0,
            pitch: 5.0,
        }));
        assert!(taken(&inbox).is_empty());
        core.send(Command::Viewer(ViewerInput::LeavePlayer));
        assert_eq!(live(&taken(&inbox))[0].player, PlayerState::None);
        core.set_live_failures(LiveFailures {
            spawn: true,
            ..LiveFailures::default()
        });
        core.send(Command::Viewer(ViewerInput::EnterPlayer {
            view: ViewKey(CHAT.into()),
            target: [12.0, 64.0, 12.0],
            rotation: 0.0,
            yaw: 0.0,
            pitch: 0.0,
        }));
        let updates = taken(&inbox);
        assert!(
            updates
                .iter()
                .any(|u| matches!(u, Update::PlayerNote(n) if n == SPAWN_FAILED))
        );
        assert_eq!(live(&updates).last().unwrap().player, PlayerState::None);
        pane.stop();
    }

    #[test]
    fn a_selection_is_named_or_refused() {
        let (core, inbox, pane) = core(LiveFailures::default());
        taken(&inbox);
        core.send(Command::DescribeSelection {
            chat: CHAT.into(),
            request: 7,
            min: [4, 0, 4],
            max: [5, 1, 5],
        });
        assert!(
            matches!(&taken(&inbox)[..], [Update::SelectionDescribed { request: 7, result: Ok(d) }] if d.label == "Stone Bricks, middle" && d.blocks == 8)
        );
        core.send(Command::DescribeSelection {
            chat: CHAT.into(),
            request: 8,
            min: [5, 0, 0],
            max: [4, 0, 0],
        });
        assert!(
            matches!(&taken(&inbox)[..], [Update::SelectionDescribed { request: 8, result: Err(e) }] if e == SELECTION_REFUSED)
        );
        pane.stop();
    }

    #[test]
    fn an_export_writes_the_file_asked_for_or_says_why_not() {
        let (core, inbox, pane) = core(LiveFailures::default());
        taken(&inbox);
        let folder = std::env::temp_dir().join(format!("mcbench-ui-export-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let to = folder.join("Castle.schem");
        core.send(Command::ExportSchematic {
            chat: CHAT.into(),
            to: to.clone(),
        });
        assert!(matches!(&taken(&inbox)[..], [Update::Exported(Ok(p))] if *p == to));
        assert!(to.is_file() && !to.with_extension("part").exists());
        core.set_live_failures(LiveFailures {
            export: true,
            ..LiveFailures::default()
        });
        core.send(Command::ExportSchematic {
            chat: CHAT.into(),
            to: folder.join("b.schem"),
        });
        assert!(matches!(&taken(&inbox)[..], [Update::Exported(Err(e))] if e == EXPORT_FAILED));
        std::fs::remove_dir_all(&folder).unwrap();
        pane.stop();
    }

    #[test]
    fn with_no_renderer_joined_the_pane_and_every_show_fail_with_its_sentence() {
        let (core, inbox, pane) = core(LiveFailures {
            not_joined: true,
            ..LiveFailures::default()
        });
        assert!(
            taken(&inbox)
                .iter()
                .any(|u| matches!(u, Update::LivePane(Err(e)) if e == NOT_JOINED))
        );
        core.send(Command::Live(LiveCommand::Show {
            view: Some(ViewKey(CHAT.into())),
            afresh: false,
        }));
        let last = live(&taken(&inbox)).pop().unwrap();
        assert_eq!(
            last.phase,
            LivePhase::Failed {
                error: NOT_JOINED.into(),
                nothing_shown: true
            }
        );
        pane.stop();
    }

    #[test]
    fn a_restart_and_a_show_of_the_view_on_show_publish_no_preparing() {
        let (core, inbox, pane) = core(LiveFailures::default());
        shown(&core, &inbox);
        core.send(Command::Live(LiveCommand::Restart));
        core.send(Command::Live(LiveCommand::Show {
            view: Some(ViewKey(CHAT.into())),
            afresh: false,
        }));
        let states = live(&taken(&inbox));
        assert!(!states.is_empty());
        assert!(
            states.iter().all(|s| s.phase != LivePhase::Preparing),
            "{states:?}"
        );
        pane.stop();
    }

    #[test]
    fn before_setup_a_selection_and_a_schematic_say_the_data_is_not_ready() {
        let (core, inbox, pane) = core(LiveFailures {
            data_not_ready: true,
            ..LiveFailures::default()
        });
        taken(&inbox);
        core.send(Command::DescribeSelection {
            chat: CHAT.into(),
            request: 1,
            min: [0; 3],
            max: [1; 3],
        });
        assert!(
            matches!(&taken(&inbox)[..], [Update::SelectionDescribed { result: Err(e), .. }] if e == DATA_NOT_READY)
        );
        core.send(Command::ExportSchematic {
            chat: CHAT.into(),
            to: std::env::temp_dir().join("never-written.schem"),
        });
        assert!(matches!(&taken(&inbox)[..], [Update::Exported(Err(e))] if e == DATA_NOT_READY));
        pane.stop();
    }

    #[test]
    fn nothing_on_show_refuses_the_toolbar_and_the_world_export() {
        let (core, inbox, pane) = core(LiveFailures::default());
        taken(&inbox);
        core.send(Command::Live(LiveCommand::Play));
        let updates = taken(&inbox);
        assert!(matches!(&updates[..], [Update::Error(e), Update::Live(_)] if e == NOTHING_SHOWN));
        core.send(Command::ExportWorld {
            view: ViewKey(CHAT.into()),
            name: "x".into(),
            to: std::env::temp_dir().join("never-written.zip"),
        });
        assert!(matches!(&taken(&inbox)[..], [Update::Exported(Err(e))] if e == NOTHING_SHOWN));
        pane.stop();
    }

    #[test]
    fn a_failed_click_is_an_error_then_a_click_with_no_input() {
        let (core, inbox, pane) = core(LiveFailures::default());
        shown(&core, &inbox);
        core.set_live_failures(LiveFailures {
            click: true,
            ..LiveFailures::default()
        });
        core.send(Command::Viewer(ViewerInput::Click {
            view: ViewKey(CHAT.into()),
            revision: 1,
            block: LEVER,
            yaw: 0.0,
        }));
        assert!(
            matches!(&taken(&inbox)[..], [Update::Error(e), Update::Clicked { input: None, .. }] if e == CLICK_FAILED)
        );
        pane.stop();
    }

    #[test]
    fn a_block_icon_is_a_png_or_the_cores_sentence() {
        let (core, _inbox, pane) = core(LiveFailures::default());
        let png = core.block_icon("oak_planks").expect("an icon");
        assert_eq!(&png[1..4], b"PNG");
        assert_eq!(
            core.block_icon("oak_planks").unwrap(),
            png,
            "the same bytes each time"
        );
        assert_eq!(core.block_icon("no_such_block").unwrap_err(), UNKNOWN_BLOCK);
        core.set_live_failures(LiveFailures {
            data_not_ready: true,
            ..LiveFailures::default()
        });
        assert_eq!(core.block_icon("oak_planks").unwrap_err(), DATA_NOT_READY);
        pane.stop();
    }
}
