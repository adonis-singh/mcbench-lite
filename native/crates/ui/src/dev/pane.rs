//! A fake 3D pane (pane agent), for the in-process tests and the dev binary:
//! a [`LivePane`] and [`LiveRenderer`] that draws frames on its own thread
//! from a simple model (a camera, a coloured ground and boxes, a lamp a
//! lever lights, a block whose colour follows the tick), paced by
//! [`LivePane::take`] as the real renderer is: it draws when the view
//! changed, no draw is in flight and the slot is empty, and an idle pane
//! draws nothing and wakes nothing. Overlays' anchors are projected with the
//! frame's camera and picks are answered by a ray cast against the model
//! with the camera of the frame named (the last three kept). Every state
//! and failure of the real pane is a [`Behaviour`] or a method. Never the
//! app's renderer.
//!
//! The model ([`ModelWorld`]) is shared with the stand-in core, which keeps
//! a world here ([`FakePane::keep`]) before it shows it, as the core keeps
//! one with `Renderer::world`. A patch's `changes` are the stand-in's own
//! one byte (the lever's state), not the engine's patch format.

use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use mcbench_render::Clip;
use mcbench_render::live::{
    CameraInput, CameraMode, CameraState, DragButton, Frame, LiveFacts, LivePane, LivePatch,
    LiveRenderer, Overlays, PaneSize, PaneStatus, Pick, PlayerView, Shown, TimeOfDay, ViewKey,
    keys,
};

/// The software floor's sentence, the renderer's (`LiveFacts::floor`).
pub const FLOOR: &str = "No graphics hardware the app can use was found, so the processor draws the 3D view, at half resolution and without smoothed edges.";
/// A world too large for the renderer's memory limit (the renderer's words).
pub const TOO_LARGE: &str =
    "This world needs about 3100 MB to draw and the renderer's limit on this computer is 2048 MB";
/// A second lost device.
pub const DEVICE_LOST: &str =
    "The graphics device was lost: the driver reset the device twice (a fixture error)";
/// A patch the world refused.
pub const PATCH_REFUSED: &str = "The live world could not take a tick's changes: block 9 is outside its palette (a fixture error)";
/// The vertical field of view of the map camera, degrees.
const V_FOV: f64 = 50.0;

/// One box of the model, both corners included.
#[derive(Clone, Debug, PartialEq)]
pub struct ModelBox {
    /// The lowest corner.
    pub min: [i32; 3],
    /// The highest corner.
    pub max: [i32; 3],
    /// Its block's name (`Lever`).
    pub name: String,
    /// The input a click takes (`toggle`, `press`, `use`), none.
    pub input: Option<&'static str>,
    /// Its colour.
    pub colour: [u8; 3],
}

/// A world of the model: a ground and boxes.
#[derive(Clone, Debug, PartialEq)]
pub struct ModelWorld {
    /// The box of its blocks.
    pub bounds: [[i32; 3]; 2],
    /// The ground's top layer of blocks.
    pub ground_y: i32,
    /// The boxes, in order.
    pub boxes: Vec<ModelBox>,
    /// The bench round's members' boxes, by member.
    pub layout: Vec<(String, [[i32; 3]; 2])>,
}

/// The lever of a chat's model world.
pub const LEVER: [i32; 3] = [14, 64, 8];
/// The lamp the lever lights.
pub const LAMP: [i32; 3] = [16, 64, 8];
/// The block whose colour follows the tick.
pub const CLOCK: [i32; 3] = [18, 64, 8];

fn model_box(
    min: [i32; 3],
    max: [i32; 3],
    name: &str,
    input: Option<&'static str>,
    colour: [u8; 3],
) -> ModelBox {
    ModelBox {
        min,
        max,
        name: name.into(),
        input,
        colour,
    }
}

impl ModelWorld {
    /// A chat's world: a grass ground 24 blocks square at y 63, a stone
    /// brick house, a lever, a lamp, a door, a button and the tick's block.
    pub fn chat() -> ModelWorld {
        ModelWorld {
            bounds: [[0, 63, 0], [23, 69, 23]],
            ground_y: 63,
            boxes: vec![
                model_box(
                    [4, 64, 4],
                    [10, 68, 12],
                    "Stone Bricks",
                    None,
                    [128, 128, 128],
                ),
                model_box(LEVER, LEVER, "Lever", Some("toggle"), [120, 90, 60]),
                model_box(LAMP, LAMP, "Redstone Lamp", None, [90, 60, 40]),
                model_box(CLOCK, CLOCK, "Target", None, [200, 60, 60]),
                model_box(
                    [14, 64, 14],
                    [14, 65, 14],
                    "Oak Door",
                    Some("use"),
                    [160, 120, 70],
                ),
                model_box(
                    [10, 66, 13],
                    [10, 66, 13],
                    "Stone Button",
                    Some("press"),
                    [150, 150, 150],
                ),
            ],
            layout: Vec::new(),
        }
    }

    /// A bench round's world: each member's build side by side along x,
    /// 40 blocks apart, on one ground.
    pub fn bench(members: &[String]) -> ModelWorld {
        let mut boxes = Vec::new();
        let mut layout = Vec::new();
        for (i, member) in members.iter().enumerate() {
            let x = i as i32 * 40;
            let height = 3 + (i as i32 % 3) * 2;
            let min = [x + 4, 64, 4];
            let max = [x + 14, 64 + height, 14];
            let shade = [
                [170, 120, 70],
                [90, 140, 90],
                [100, 110, 170],
                [170, 160, 90],
            ][i % 4];
            boxes.push(model_box(min, max, "Oak Planks", None, shade));
            layout.push((member.clone(), [min, max]));
        }
        let width = (members.len().max(1) as i32) * 40 - 22;
        ModelWorld {
            bounds: [[0, 63, 0], [width, 72, 18]],
            ground_y: 63,
            boxes,
            layout,
        }
    }

    /// The block at `at` and its box, if any (the ground is `Grass Block`).
    pub fn block(&self, at: [i32; 3]) -> Option<(String, Option<&'static str>)> {
        if let Some(b) = self.boxes.iter().find(|b| contains(b.min, b.max, at)) {
            return Some((b.name.clone(), b.input));
        }
        let [min, max] = self.bounds;
        (at[1] == self.ground_y
            && (min[0]..=max[0]).contains(&at[0])
            && (min[2]..=max[2]).contains(&at[2]))
        .then(|| ("Grass Block".to_string(), None))
    }

    /// The blocks in a box and the most common one's name.
    pub fn count(&self, min: [i32; 3], max: [i32; 3]) -> (u64, Option<String>) {
        let mut counts: Vec<(String, u64)> = Vec::new();
        for x in min[0]..=max[0] {
            for y in min[1]..=max[1] {
                for z in min[2]..=max[2] {
                    if let Some((name, _)) = self.block([x, y, z]) {
                        match counts.iter_mut().find(|(n, _)| *n == name) {
                            Some((_, c)) => *c += 1,
                            None => counts.push((name, 1)),
                        }
                    }
                }
            }
        }
        let total = counts.iter().map(|(_, c)| c).sum();
        let most = counts
            .iter()
            .max_by_key(|(_, c)| *c)
            .map(|(n, _)| n.clone());
        (total, most)
    }
}

fn contains(min: [i32; 3], max: [i32; 3], at: [i32; 3]) -> bool {
    (0..3).all(|a| min[a] <= at[a] && at[a] <= max[a])
}

/// How the fake pane behaves, set at start (the dev binary's `--pane`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Behaviour {
    /// A software adapter: half the width and height, the floor's sentence.
    pub software: bool,
    /// Each draw takes at least this long.
    pub slow: Option<Duration>,
    /// Every shown world is too large to keep.
    pub too_large: bool,
    /// The first patch after a show is refused.
    pub patch_fails: bool,
}

/// What the fake pane counted, for the tests and the speed runs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    /// Frames drawn.
    pub drawn: u64,
    /// Frames delivered over one not taken (dropped).
    pub replaced: u64,
    /// Takes that returned a frame.
    pub taken: u64,
    /// Waker calls.
    pub wakes: u64,
    /// Picks answered.
    pub picks: u64,
}

#[derive(Clone, Copy, Debug)]
struct Camera {
    mode: CameraMode,
    /// The map camera's (or spectator's) target.
    target: [f64; 3],
    distance: f64,
    rotation: f64,
    angle: f64,
    /// Player mode's look, degrees.
    yaw: f64,
    pitch: f64,
    /// The map view saved when player mode began.
    saved: Option<(CameraMode, [f64; 3], f64, f64, f64)>,
}

struct Fly {
    from: ([f64; 3], f64),
    to: ([f64; 3], f64),
    start: Instant,
    ms: u32,
}

type Answer = Box<dyn FnOnce(Pick) + Send>;

struct State {
    waker: Option<Arc<dyn Fn() + Send + Sync>>,
    size: PaneSize,
    visible: bool,
    camera: Camera,
    inputs: VecDeque<CameraInput>,
    held: u16,
    spin: (f64, f64),
    /// When held keys last moved the camera: they move by time, not by
    /// frames drawn (BlueMap's speeds are per 16.7 ms).
    keys_at: Option<Instant>,
    fly: Option<Fly>,
    overlays: Overlays,
    time: TimeOfDay,
    kept: Vec<(String, ModelWorld)>,
    shown: Option<(Shown, ModelWorld)>,
    tick: u64,
    lever_on: bool,
    player: Option<PlayerView>,
    patches_since_show: u64,
    slot: Option<Frame>,
    drawing: bool,
    changed: bool,
    seq: u64,
    cameras: VecDeque<(u64, ViewKey, CameraState)>,
    picks: VecDeque<(u64, [f32; 2], Answer)>,
    status: PaneStatus,
    behaviour: Behaviour,
    lost_once: bool,
    counts: Counts,
    newest: Option<CameraState>,
    stop: bool,
}

/// The fake pane; [`FakePane::start`] spawns its render thread.
pub struct FakePane {
    state: Mutex<State>,
    wake: Condvar,
}

impl FakePane {
    /// A pane drawing on its own thread with `behaviour`.
    pub fn start(behaviour: Behaviour) -> Arc<FakePane> {
        let pane = FakePane::manual(behaviour);
        let worker = pane.clone();
        std::thread::Builder::new()
            .name("fake-pane".into())
            .spawn(move || worker.run())
            .expect("the fake pane's thread starts");
        pane
    }

    /// A pane with no thread of its own: it draws and answers picks only in
    /// [`FakePane::pump`], on the caller's thread (GPUI's test scheduler
    /// refuses wakes from another thread).
    pub fn manual(behaviour: Behaviour) -> Arc<FakePane> {
        Arc::new(FakePane {
            state: Mutex::new(State {
                waker: None,
                size: PaneSize {
                    width: 0,
                    height: 0,
                    scale: 1.0,
                },
                visible: true,
                camera: Camera {
                    mode: CameraMode::Perspective,
                    target: [0.0; 3],
                    distance: 40.0,
                    rotation: 0.6,
                    angle: 1.0,
                    yaw: 0.0,
                    pitch: 0.0,
                    saved: None,
                },
                inputs: VecDeque::new(),
                held: 0,
                spin: (0.0, 0.0),
                keys_at: None,
                fly: None,
                overlays: Overlays::default(),
                time: TimeOfDay::Day,
                kept: Vec::new(),
                shown: None,
                tick: 0,
                lever_on: false,
                player: None,
                patches_since_show: 0,
                slot: None,
                drawing: false,
                changed: false,
                seq: 0,
                cameras: VecDeque::new(),
                picks: VecDeque::new(),
                status: PaneStatus::Drawing,
                behaviour,
                lost_once: false,
                counts: Counts::default(),
                newest: None,
                stop: false,
            }),
            wake: Condvar::new(),
        })
    }

    /// Answers every waiting pick and draws while the view changed and the
    /// slot is empty, on this thread (a [`FakePane::manual`] pane).
    pub fn pump(&self) {
        let mut s = self.lock();
        loop {
            let (next, worked) = self.work(s);
            s = next;
            if !worked {
                return;
            }
        }
    }

    /// Keeps a world under `name`, as `Renderer::world` does.
    pub fn keep(&self, name: &str, world: ModelWorld) {
        let mut s = self.lock();
        s.kept.retain(|(n, _)| n != name);
        s.kept.push((name.to_string(), world));
    }

    /// The device is lost: recovered once (the pane draws again), or not
    /// (`Failed` with [`DEVICE_LOST`]) when it was lost before.
    pub fn lose_device(&self) {
        let mut s = self.lock();
        if s.lost_once {
            s.status = PaneStatus::Failed(DEVICE_LOST.into());
            self.notify(&mut s);
        } else {
            s.lost_once = true;
            s.changed = true;
        }
        self.wake.notify_all();
    }

    /// What the pane counted so far.
    pub fn counts(&self) -> Counts {
        self.lock().counts
    }

    /// The camera keys held now.
    pub fn held(&self) -> u16 {
        self.lock().held
    }

    /// The overlays last asked for.
    pub fn overlays_now(&self) -> Overlays {
        self.lock().overlays.clone()
    }

    /// The time of day last asked for.
    pub fn time_now(&self) -> TimeOfDay {
        self.lock().time
    }

    /// Whether the pane is visible.
    pub fn is_visible(&self) -> bool {
        self.lock().visible
    }

    /// The size last given.
    pub fn size_now(&self) -> PaneSize {
        self.lock().size
    }

    /// The camera inputs applied so far are logged here when set.
    pub fn camera_now(&self) -> CameraState {
        let s = self.lock();
        camera_state(&s.camera, s.player.as_ref(), aspect(&s))
    }

    /// Stops the render thread.
    pub fn stop(&self) {
        self.lock().stop = true;
        self.wake.notify_all();
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().expect("the fake pane's lock")
    }

    fn notify(&self, s: &mut State) {
        if let Some(waker) = s.waker.clone() {
            s.counts.wakes += 1;
            waker();
        }
    }

    fn run(self: Arc<Self>) {
        let mut s = self.lock();
        loop {
            if s.stop {
                return;
            }
            let (next, worked) = self.work(s);
            s = next;
            if !worked {
                s = self.wake.wait(s).expect("the fake pane's lock");
            }
        }
    }

    /// One pick answered or one frame drawn; false when there was nothing
    /// to do.
    fn work<'a>(
        &'a self,
        mut s: std::sync::MutexGuard<'a, State>,
    ) -> (std::sync::MutexGuard<'a, State>, bool) {
        {
            // Picks go first, within one draw.
            if let Some((seq, at, answer)) = s.picks.pop_front() {
                let pick = pick(&s, seq, at);
                s.counts.picks += 1;
                drop(s);
                answer(pick);
                return (self.lock(), true);
            }
            let moving = s.held != 0
                || s.fly.is_some()
                || s.spin.0.abs() > 1e-4
                || s.spin.1.abs() > 1e-4
                || !s.inputs.is_empty();
            let ready = s.visible
                && s.size.width > 0
                && s.size.height > 0
                && s.status == PaneStatus::Drawing
                && s.shown.is_some()
                && s.slot.is_none()
                && !s.drawing
                && (s.changed || moving);
            if !ready {
                return (s, false);
            }
            s.drawing = true;
            s.changed = false;
            apply_inputs(&mut s);
            let size = s.size;
            let software = s.behaviour.software;
            let (w, h) = if software {
                ((size.width / 2).max(1), (size.height / 2).max(1))
            } else {
                (size.width, size.height)
            };
            s.seq += 1;
            let seq = s.seq;
            let camera = camera_state(&s.camera, s.player.as_ref(), w as f64 / h as f64);
            let (shown, world) = s.shown.clone().expect("checked above");
            let scene = Scene {
                world,
                camera,
                lever_on: s.lever_on,
                tick: s.tick,
                marked: s.overlays.marked,
                boxes: s.overlays.boxes.clone(),
                night: s.time == TimeOfDay::Night,
            };
            let anchors = s
                .overlays
                .anchors
                .iter()
                .map(|p| project(&camera, *p, w, h))
                .collect();
            let time = s.time;
            let slow = s.behaviour.slow;
            drop(s);
            let started = Instant::now();
            let pixels = draw(&scene, w, h);
            if let Some(slow) = slow {
                std::thread::sleep(slow.saturating_sub(started.elapsed()));
            }
            let frame = Frame::new(
                seq,
                shown.view.clone(),
                shown.revision,
                [w, h],
                anchors,
                camera,
                time,
                pixels,
            );
            s = self.lock();
            s.drawing = false;
            s.counts.drawn += 1;
            s.cameras.push_back((seq, shown.view.clone(), camera));
            while s.cameras.len() > 3 {
                s.cameras.pop_front();
            }
            // A hide while drawing: the frame is kept, the next take has it.
            if s.slot.replace(frame).is_some() {
                s.counts.replaced += 1;
            }
            s.newest = Some(camera);
            self.notify(&mut s);
            (s, true)
        }
    }
}

fn aspect(s: &State) -> f64 {
    if s.size.height == 0 {
        1.0
    } else {
        s.size.width as f64 / s.size.height as f64
    }
}

fn apply_inputs(s: &mut State) {
    let height = s.size.height.max(1) as f64;
    while let Some(input) = s.inputs.pop_front() {
        let c = &mut s.camera;
        match input {
            CameraInput::PointerDown => {
                s.fly = None;
                s.spin = (0.0, 0.0);
            }
            CameraInput::Drag { button, dx, dy } => {
                let (dx, dy) = (dx as f64, dy as f64);
                let pan = |c: &mut Camera| {
                    let k = c.distance / height;
                    let (sn, cs) = c.rotation.sin_cos();
                    // The ground follows the pointer: right is (cos r, 0,
                    // sin r), ahead (sin r, 0, -cos r).
                    c.target[0] += (-dx * cs + dy * sn) * k;
                    c.target[2] += (-dx * sn - dy * cs) * k;
                };
                let turn = |c: &mut Camera, spin: &mut (f64, f64)| {
                    c.rotation += dx * 0.005;
                    c.angle = (c.angle + dy * 0.005).clamp(0.0, 1.45);
                    *spin = (dx * 0.005, dy * 0.005);
                };
                match button {
                    DragButton::Left => pan(c),
                    DragButton::Right | DragButton::AltLeft => turn(c, &mut s.spin),
                    DragButton::CtrlLeft => {
                        pan(c);
                        turn(c, &mut s.spin);
                    }
                }
            }
            CameraInput::Wheel { delta, lines } => {
                let per = if lines { 0.15 } else { 0.004 };
                c.distance = (c.distance * (1.0 + delta as f64 * per)).clamp(4.0, 400.0);
            }
            CameraInput::Pinch { scale } => {
                c.distance = (c.distance / scale.max(0.01) as f64).clamp(4.0, 400.0);
            }
            CameraInput::Twist { radians } => c.rotation += radians as f64,
            CameraInput::ZoomStep(n) => {
                c.distance = (c.distance * 1.2f64.powi(n as i32)).clamp(4.0, 400.0);
            }
            CameraInput::Keys(held) => s.held = held,
            CameraInput::DoubleClick { .. } => c.distance = (c.distance * 0.5).max(4.0),
            CameraInput::Frame { min, max, ms } => {
                let to = framing(min, max);
                if ms == 0 {
                    (c.target, c.distance) = to;
                    s.fly = None;
                } else {
                    s.fly = Some(Fly {
                        from: (c.target, c.distance),
                        to,
                        start: Instant::now(),
                        ms,
                    });
                }
            }
            CameraInput::Reset => {
                if let Some((_, world)) = &s.shown {
                    let [min, max] = world.bounds;
                    (c.target, c.distance) = framing(min, max);
                }
                c.rotation = 0.6;
                c.angle = 1.0;
            }
            CameraInput::FaceNorth => c.rotation = 0.0,
            CameraInput::Target { x, z } => {
                c.target[0] = x;
                c.target[2] = z;
            }
            CameraInput::Mode(mode) => {
                if mode == CameraMode::Player && c.mode != CameraMode::Player {
                    c.saved = Some((c.mode, c.target, c.distance, c.rotation, c.angle));
                }
                if c.mode == CameraMode::Player
                    && mode != CameraMode::Player
                    && let Some((_, target, distance, rotation, angle)) = c.saved.take()
                {
                    (c.target, c.distance, c.rotation, c.angle) =
                        (target, distance, rotation, angle);
                }
                c.mode = mode;
            }
            CameraInput::Look { yaw, pitch } => {
                c.yaw = yaw as f64;
                c.pitch = pitch as f64;
            }
            // The renderer's spectator look under the pointer lock: the
            // stand-in turns as a turning drag does, in spectator only.
            CameraInput::LockedLook { dx, dy } => {
                if c.mode == CameraMode::Spectator {
                    c.rotation += dx as f64 * 0.005;
                    c.angle = (c.angle + dy as f64 * 0.005).clamp(0.0, 1.45);
                }
            }
        }
    }
    // Held keys move the target by the time since the last step, a fly
    // advances, inertia decays.
    let now = Instant::now();
    let frames = match (s.held, s.keys_at) {
        (0, _) => 0.0,
        (_, Some(at)) => (now - at).as_secs_f64() * 1000.0 / 16.667,
        (_, None) => 1.0,
    };
    s.keys_at = (s.held != 0).then_some(now);
    let c = &mut s.camera;
    let step = c.distance * 0.02 * frames;
    let (sn, cs) = c.rotation.sin_cos();
    let mut go = |forward: f64, right: f64| {
        c.target[0] += (sn * forward + cs * right) * step;
        c.target[2] += (-cs * forward + sn * right) * step;
    };
    if s.held & keys::FORWARD != 0 {
        go(1.0, 0.0);
    }
    if s.held & keys::BACKWARD != 0 {
        go(-1.0, 0.0);
    }
    if s.held & keys::LEFT != 0 {
        go(0.0, -1.0);
    }
    if s.held & keys::RIGHT != 0 {
        go(0.0, 1.0);
    }
    if s.held & keys::ZOOM_IN != 0 {
        c.distance = (c.distance * 0.97f64.powf(frames)).max(4.0);
    }
    if s.held & keys::ZOOM_OUT != 0 {
        c.distance = (c.distance * 1.03f64.powf(frames)).min(400.0);
    }
    if s.held & keys::ROTATE_LEFT != 0 {
        c.rotation -= 0.03 * frames;
    }
    if s.held & keys::ROTATE_RIGHT != 0 {
        c.rotation += 0.03 * frames;
    }
    if s.held & keys::TILT_UP != 0 {
        c.angle = (c.angle - 0.02 * frames).max(0.0);
    }
    if s.held & keys::TILT_DOWN != 0 {
        c.angle = (c.angle + 0.02 * frames).min(1.45);
    }
    if let Some(fly) = &s.fly {
        let t = (fly.start.elapsed().as_secs_f64() * 1000.0 / fly.ms as f64).min(1.0);
        let e = t * t * (3.0 - 2.0 * t);
        for a in 0..3 {
            c.target[a] = fly.from.0[a] + (fly.to.0[a] - fly.from.0[a]) * e;
        }
        c.distance = fly.from.1 + (fly.to.1 - fly.from.1) * e;
        if t >= 1.0 {
            s.fly = None;
        }
    }
    if s.held == 0 && s.inputs.is_empty() {
        s.spin.0 *= 0.85;
        s.spin.1 *= 0.85;
        c.rotation += s.spin.0;
        c.angle = (c.angle + s.spin.1).clamp(0.0, 1.45);
    }
}

fn framing(min: [i32; 3], max: [i32; 3]) -> ([f64; 3], f64) {
    let centre = [0, 1, 2].map(|a| (min[a] as f64 + max[a] as f64 + 1.0) / 2.0);
    let extent = (0..3)
        .map(|a| (max[a] - min[a] + 1) as f64)
        .fold(1.0, f64::max);
    (centre, (extent * 1.6).clamp(8.0, 400.0))
}

fn camera_state(c: &Camera, player: Option<&PlayerView>, aspect: f64) -> CameraState {
    // Player mode keeps the map view until the player stands.
    if let (CameraMode::Player, Some(p)) = (c.mode, player) {
        return CameraState {
            mode: CameraMode::Player,
            target: c.target,
            distance: c.distance,
            // As BlueMap's camera holds the player's eye: rotation
            // (yaw + 180)°, angle (90 − pitch)° (`web/bluemap-bridge.js`).
            rotation: (c.yaw + 180.0).to_radians(),
            angle: (90.0 - c.pitch).to_radians(),
            eye: [p.pos[0], p.pos[1] + p.eye_height as f64, p.pos[2]],
            v_fov: 70.0,
            aspect,
        };
    }
    let mode = match c.mode {
        CameraMode::Player => CameraMode::Perspective,
        m => m,
    };
    let eye = match mode {
        CameraMode::Flat => [c.target[0], c.target[1] + c.distance, c.target[2]],
        _ => [
            // BlueMap's orbit: the heading looks along (sin r, 0, -cos r).
            c.target[0] - c.distance * c.angle.sin() * c.rotation.sin(),
            c.target[1] + c.distance * c.angle.cos(),
            c.target[2] + c.distance * c.angle.sin() * c.rotation.cos(),
        ],
    };
    CameraState {
        mode,
        target: c.target,
        distance: c.distance,
        rotation: c.rotation,
        angle: if mode == CameraMode::Flat {
            0.0
        } else {
            c.angle
        },
        eye,
        v_fov: V_FOV,
        aspect,
    }
}

/// The camera's basis: forward, right, up.
fn basis(c: &CameraState, look: Option<(f64, f64)>) -> ([f64; 3], [f64; 3], [f64; 3]) {
    let forward = match look {
        Some((yaw, pitch)) => {
            let (y, p) = (yaw.to_radians(), pitch.to_radians());
            [-y.sin() * p.cos(), -p.sin(), y.cos() * p.cos()]
        }
        None => norm(sub(c.target, c.eye)),
    };
    let right = match look {
        Some((yaw, _)) => {
            let y = yaw.to_radians();
            [-y.cos(), 0.0, -y.sin()]
        }
        None => [c.rotation.cos(), 0.0, c.rotation.sin()],
    };
    let up = cross(right, forward);
    (forward, right, up)
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn norm(a: [f64; 3]) -> [f64; 3] {
    let l = dot(a, a).sqrt().max(1e-9);
    [a[0] / l, a[1] / l, a[2] / l]
}

/// The look of player mode in a camera state, from its eye and target.
fn player_look(c: &CameraState) -> Option<(f64, f64)> {
    (c.mode == CameraMode::Player)
        .then(|| (c.rotation.to_degrees() - 180.0, 90.0 - c.angle.to_degrees()))
}

/// The ray of pixel `(x, y)` of a `w` by `h` frame.
fn ray(
    c: &CameraState,
    look: Option<(f64, f64)>,
    x: f64,
    y: f64,
    w: u32,
    h: u32,
) -> ([f64; 3], [f64; 3]) {
    let (forward, right, up) = basis(c, look);
    let t = (c.v_fov.to_radians() / 2.0).tan();
    let nx = (2.0 * x / w as f64 - 1.0) * t * (w as f64 / h as f64);
    let ny = (1.0 - 2.0 * y / h as f64) * t;
    if c.mode == CameraMode::Flat {
        let half = c.distance * t;
        let origin = [0, 1, 2].map(|a| c.eye[a] + right[a] * nx / t * half + up[a] * ny / t * half);
        (origin, forward)
    } else {
        let dir = norm([0, 1, 2].map(|a| forward[a] + right[a] * nx + up[a] * ny));
        (c.eye, dir)
    }
}

fn project(c: &CameraState, p: [f64; 3], w: u32, h: u32) -> Option<[f32; 2]> {
    let (forward, right, up) = basis(c, player_look(c));
    let d = sub(p, c.eye);
    let z = dot(d, forward);
    let t = (c.v_fov.to_radians() / 2.0).tan();
    let (x, y) = if c.mode == CameraMode::Flat {
        let half = c.distance * t;
        (dot(d, right) / half * t, dot(d, up) / half * t)
    } else {
        if z <= 0.0 {
            return None;
        }
        (dot(d, right) / z, dot(d, up) / z)
    };
    let px = (x / (t * w as f64 / h as f64) + 1.0) / 2.0 * w as f64;
    let py = (1.0 - y / t) / 2.0 * h as f64;
    Some([px as f32, py as f32])
}

/// The nearest hit of a ray on a box of blocks: distance and normal.
fn hit_box(o: [f64; 3], d: [f64; 3], min: [i32; 3], max: [i32; 3]) -> Option<(f64, [i8; 3])> {
    let (mut near, mut far, mut normal) = (0.0f64, f64::INFINITY, [0i8; 3]);
    for a in 0..3 {
        let (lo, hi) = (min[a] as f64, max[a] as f64 + 1.0);
        if d[a].abs() < 1e-12 {
            if o[a] < lo || o[a] > hi {
                return None;
            }
            continue;
        }
        let (mut t0, mut t1) = ((lo - o[a]) / d[a], (hi - o[a]) / d[a]);
        let mut n = -(d[a].signum() as i8);
        if t0 > t1 {
            std::mem::swap(&mut t0, &mut t1);
        }
        if d[a] < 0.0 {
            n = 1;
        }
        if t0 > near {
            near = t0;
            normal = [0; 3];
            normal[a] = n;
        }
        far = far.min(t1);
        if near > far {
            return None;
        }
    }
    (normal != [0; 3]).then_some((near, normal))
}

struct Scene {
    world: ModelWorld,
    camera: CameraState,
    lever_on: bool,
    tick: u64,
    marked: Option<[i32; 3]>,
    boxes: Vec<mcbench_render::live::OverlayBox>,
    night: bool,
}

/// The block a ray hits: the block, its face and the distance.
/// A ray's hit: the block, its face, the distance and the block's colour.
type Hit = ([i32; 3], [i8; 3], f64, [u8; 3]);

fn cast(world: &ModelWorld, o: [f64; 3], d: [f64; 3]) -> Option<Hit> {
    let [min, max] = world.bounds;
    let ground = hit_box(
        o,
        d,
        [min[0], world.ground_y, min[2]],
        [max[0], world.ground_y, max[2]],
    )
    .map(|(t, n)| (t, n, [90u8, 150, 70]));
    let best = world
        .boxes
        .iter()
        .filter_map(|b| hit_box(o, d, b.min, b.max).map(|(t, n)| (t, n, b.colour)))
        .chain(ground)
        .min_by(|a, b| a.0.total_cmp(&b.0))?;
    let (t, n, colour) = best;
    let p = [0, 1, 2].map(|a| o[a] + d[a] * t - n[a] as f64 * 0.01);
    Some((p.map(|v| v.floor() as i32), n, t, colour))
}

fn pick(s: &State, seq: u64, at: [f32; 2]) -> Pick {
    let view = s
        .shown
        .as_ref()
        .map(|(shown, _)| shown.view.clone())
        .unwrap_or_else(|| ViewKey(String::new()));
    let kept = s.cameras.iter().find(|(n, ..)| *n == seq);
    let yaw = |c: &CameraState| match s.player {
        Some(p) if c.mode == CameraMode::Player => p.y_rot,
        _ => (c.rotation * 180.0 / std::f64::consts::PI - 180.0) as f32,
    };
    match (kept, &s.shown) {
        (Some((_, frame_view, camera)), Some((_, world))) if *frame_view == view => {
            let (w, h) = if s.behaviour.software {
                ((s.size.width / 2).max(1), (s.size.height / 2).max(1))
            } else {
                (s.size.width.max(1), s.size.height.max(1))
            };
            let (o, d) = ray(
                camera,
                player_look(camera),
                at[0] as f64,
                at[1] as f64,
                w,
                h,
            );
            let hit = cast(world, o, d);
            Pick {
                seq,
                view,
                block: hit.map(|h| h.0),
                face: hit.map(|h| h.1),
                yaw: yaw(camera),
            }
        }
        _ => Pick {
            seq,
            view,
            block: None,
            face: None,
            yaw: s.newest.as_ref().map_or(0.0, yaw),
        },
    }
}

/// The picture: rays cast per cell of a few pixels, filled.
fn draw(scene: &Scene, w: u32, h: u32) -> Vec<u8> {
    let mut pixels = vec![0u8; w as usize * h as usize * 4];
    // Coarse cells keep the fake cheap (a debug build draws every tick).
    let cell = (h / 120).max(1);
    let look = player_look(&scene.camera);
    for cy in (0..h).step_by(cell as usize) {
        for cx in (0..w).step_by(cell as usize) {
            let (o, d) = ray(&scene.camera, look, cx as f64 + 0.5, cy as f64 + 0.5, w, h);
            let mut colour = match cast(&scene.world, o, d) {
                Some((block, normal, _, base)) => {
                    let mut c = base;
                    if block == LAMP && scene.lever_on {
                        c = [250, 210, 120];
                    }
                    if block == CLOCK {
                        c = [
                            (scene.tick * 12 % 256) as u8,
                            60,
                            200 - (scene.tick * 12 % 200) as u8,
                        ];
                    }
                    if block[1] == scene.world.ground_y
                        && (block[0] + block[2]) % 2 == 0
                        && base == [90, 150, 70]
                    {
                        c = [80, 138, 62];
                    }
                    if scene.marked == Some(block) {
                        c = [255, 255, 255];
                    }
                    let shade = match normal {
                        [0, 1, 0] => 1.0,
                        [0, -1, 0] => 0.5,
                        [1, 0, 0] | [-1, 0, 0] => 0.8,
                        _ => 0.65,
                    };
                    c.map(|v| (v as f64 * shade) as u8)
                }
                None => {
                    let k = cy as f64 / h as f64;
                    [(120.0 + 80.0 * k) as u8, (170.0 + 50.0 * k) as u8, 235]
                }
            };
            for b in &scene.boxes {
                if b.fill > 0.0 && hit_box(o, d, b.min, b.max).is_some() {
                    let f = b.fill as f64;
                    colour = colour.map(|v| (v as f64 * (1.0 - f) + 255.0 * f) as u8);
                }
                if b.outline && hit_box(o, d, b.min, b.max).is_some() {
                    colour = [
                        colour[0],
                        colour[1].saturating_add(40),
                        colour[2].saturating_add(60),
                    ];
                }
            }
            if scene.night {
                colour = colour.map(|v| (v as f64 * 0.3) as u8);
            }
            let [r, g, b] = colour;
            for y in cy..(cy + cell).min(h) {
                let row = y as usize * w as usize * 4;
                for x in cx..(cx + cell).min(w) {
                    let i = row + x as usize * 4;
                    pixels[i..i + 4].copy_from_slice(&[b, g, r, 255]);
                }
            }
        }
    }
    pixels
}

impl LivePane for FakePane {
    fn set_waker(&self, wake: Box<dyn Fn() + Send + Sync>) {
        self.lock().waker = Some(Arc::from(wake));
    }

    fn resize(&self, size: PaneSize) {
        let mut s = self.lock();
        if s.size != size {
            s.size = size;
            s.changed = true;
            self.wake.notify_all();
        }
    }

    fn visible(&self, visible: bool) {
        let mut s = self.lock();
        if s.visible != visible {
            s.visible = visible;
            s.changed |= visible;
            self.wake.notify_all();
        }
    }

    fn camera(&self, input: CameraInput) {
        let mut s = self.lock();
        s.inputs.push_back(input);
        self.wake.notify_all();
    }

    fn overlays(&self, overlays: Overlays) {
        let mut s = self.lock();
        if s.overlays != overlays {
            s.overlays = overlays;
            s.changed = true;
            self.wake.notify_all();
        }
    }

    fn time_of_day(&self, time: TimeOfDay) {
        let mut s = self.lock();
        if s.time != time {
            s.time = time;
            s.changed = true;
            self.wake.notify_all();
        }
    }

    fn pick(&self, seq: u64, at: [f32; 2], answer: Box<dyn FnOnce(Pick) + Send>) {
        self.lock().picks.push_back((seq, at, answer));
        self.wake.notify_all();
    }

    fn take(&self) -> Option<Frame> {
        let mut s = self.lock();
        let frame = s.slot.take();
        if frame.is_some() {
            s.counts.taken += 1;
            self.wake.notify_all();
        }
        frame
    }

    fn status(&self) -> PaneStatus {
        self.lock().status.clone()
    }

    fn camera_state(&self) -> CameraState {
        let s = self.lock();
        s.newest
            .unwrap_or_else(|| camera_state(&s.camera, s.player.as_ref(), aspect(&s)))
    }

    fn facts(&self) -> LiveFacts {
        LiveFacts {
            floor: self.lock().behaviour.software.then(|| FLOOR.to_string()),
        }
    }
}

/// The fake pane's renderer side, as the core calls it.
pub struct FakeRenderer(pub Arc<FakePane>);

impl LiveRenderer for FakeRenderer {
    fn live(&self) -> Arc<dyn LivePane> {
        self.0.clone()
    }

    fn live_show(&self, shown: Option<Shown>) {
        let pane = &self.0;
        let mut s = pane.lock();
        match shown {
            None => s.shown = None,
            Some(shown) => {
                if s.behaviour.too_large {
                    s.status = PaneStatus::Failed(TOO_LARGE.into());
                    pane.notify(&mut s);
                    return;
                }
                let world = s
                    .kept
                    .iter()
                    .find(|(n, _)| *n == shown.world)
                    .map(|(_, w)| w.clone())
                    .unwrap_or_else(|| panic!("the world {} was not kept", shown.world));
                if shown.framing != mcbench_render::live::Framing::Keep {
                    let Clip { min, max } = shown.bounds;
                    let (target, distance) = framing(min, max);
                    s.camera.target = target;
                    s.camera.distance = distance;
                }
                s.tick = 0;
                s.lever_on = false;
                s.patches_since_show = 0;
                s.shown = Some((shown, world));
                s.changed = true;
            }
        }
        pane.wake.notify_all();
    }

    fn live_patch(&self, patch: LivePatch) {
        let pane = &self.0;
        let mut s = pane.lock();
        if s.status != PaneStatus::Drawing {
            return;
        }
        if s.behaviour.patch_fails && s.patches_since_show == 0 {
            s.status = PaneStatus::Failed(PATCH_REFUSED.into());
            pane.notify(&mut s);
            return;
        }
        s.patches_since_show += 1;
        s.tick = patch.tick;
        if let Some(on) = patch.changes.first() {
            s.lever_on = *on == 1;
        }
        // A spawn: the eye looks where the core stood the player.
        if let (None, Some(view)) = (&s.player, &patch.player) {
            s.camera.yaw = view.y_rot as f64;
            s.camera.pitch = view.x_rot as f64;
        }
        s.player = patch.player;
        if let Some((shown, _)) = &mut s.shown {
            shown.revision = patch.revision;
        }
        s.changed = true;
        pane.wake.notify_all();
    }
}

/// The dev binary's `--pane FLAG` (repeated): how the fake pane and the
/// stand-in's live world behave.
#[derive(Clone, Debug, Default)]
pub struct DevFlags {
    /// The pane's behaviour.
    pub behaviour: Behaviour,
    /// What the stand-in fails.
    pub failures: crate::dev::standin::LiveFailures,
    /// The device is lost this many ms after start, once per entry.
    pub lose_device_ms: Vec<u64>,
}

impl DevFlags {
    /// One `--pane` value: `software`, `too-large`, `patch-fails`,
    /// `no-adapter`, `not-joined`, `data-not-ready`, `place-fails`, `click-fails`, `spawn-fails`,
    /// `selection-fails`, `export-fails`, `stop-after:N` (ticks),
    /// `slow-frames:MS`, `slow-place:MS`, `lose-device:MS` (repeat it for a
    /// second loss).
    pub fn add(&mut self, flag: &str) {
        let (name, n) = match flag.split_once(':') {
            Some((name, n)) => (
                name,
                Some(
                    n.parse::<u64>()
                        .unwrap_or_else(|_| panic!("--pane {flag}: {n} is not a number")),
                ),
            ),
            None => (flag, None),
        };
        let need = || n.unwrap_or_else(|| panic!("--pane {name} takes a number: {name}:N"));
        match name {
            "software" => self.behaviour.software = true,
            "too-large" => self.behaviour.too_large = true,
            "patch-fails" => self.behaviour.patch_fails = true,
            "no-adapter" => self.failures.no_adapter = true,
            "place-fails" => self.failures.place = true,
            "click-fails" => self.failures.click = true,
            "spawn-fails" => self.failures.spawn = true,
            "selection-fails" => self.failures.selection = true,
            "export-fails" => self.failures.export = true,
            "data-not-ready" => self.failures.data_not_ready = true,
            "not-joined" => self.failures.not_joined = true,
            "stop-after" => self.failures.stop_after = Some(need()),
            "slow-frames" => self.behaviour.slow = Some(Duration::from_millis(need())),
            "slow-place" => self.failures.slow_place = Some(Duration::from_millis(need())),
            "lose-device" => self.lose_device_ms.push(need()),
            other => panic!(
                "--pane takes software, too-large, patch-fails, no-adapter, not-joined, data-not-ready, place-fails, click-fails, spawn-fails, selection-fails, export-fails, stop-after:N, slow-frames:MS, slow-place:MS or lose-device:MS, not {other}"
            ),
        }
    }

    /// Starts the fake pane, gives it to the stand-in, starts the
    /// stand-in's tick clock and the device losses.
    pub fn start(self, core: &Arc<crate::dev::standin::StandIn>) -> Arc<FakePane> {
        let pane = FakePane::start(self.behaviour);
        core.set_pane(pane.clone(), self.failures);
        core.live_clock();
        for ms in self.lose_device_ms {
            let pane = pane.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(ms));
                pane.lose_device();
            });
        }
        pane
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shown(view: &str) -> Shown {
        let world = ModelWorld::chat();
        Shown {
            world: format!("live:{view}"),
            view: ViewKey(view.into()),
            revision: 1,
            bounds: Clip {
                min: world.bounds[0],
                max: world.bounds[1],
            },
            framing: mcbench_render::live::Framing::AtOnce,
        }
    }

    fn started(behaviour: Behaviour) -> (Arc<FakePane>, FakeRenderer) {
        let pane = FakePane::start(behaviour);
        pane.keep("live:c", ModelWorld::chat());
        let renderer = FakeRenderer(pane.clone());
        pane.resize(PaneSize {
            width: 64,
            height: 48,
            scale: 1.0,
        });
        (pane, renderer)
    }

    fn frame(pane: &FakePane) -> Frame {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(f) = pane.take() {
                return f;
            }
            assert!(Instant::now() < deadline, "no frame within 5 s");
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn an_idle_pane_draws_nothing_after_its_first_frame() {
        let (pane, renderer) = started(Behaviour::default());
        renderer.live_show(Some(shown("c")));
        let first = frame(&pane);
        assert_eq!(first.size, [64, 48]);
        std::thread::sleep(Duration::from_millis(100));
        assert!(pane.take().is_none());
        assert_eq!(pane.counts().drawn, 1);
        pane.stop();
    }

    #[test]
    fn a_frame_not_taken_paces_the_next_draw() {
        let (pane, renderer) = started(Behaviour::default());
        renderer.live_show(Some(shown("c")));
        std::thread::sleep(Duration::from_millis(50));
        pane.camera(CameraInput::ZoomStep(1));
        std::thread::sleep(Duration::from_millis(50));
        // The slot was full: the zoom waits for the take.
        assert_eq!(pane.counts().drawn, 1);
        frame(&pane);
        frame(&pane);
        assert_eq!(pane.counts().drawn, 2);
        pane.stop();
    }

    #[test]
    fn a_pick_names_the_block_under_the_centre() {
        let (pane, renderer) = started(Behaviour::default());
        renderer.live_show(Some(shown("c")));
        let f = frame(&pane);
        let (tx, rx) = std::sync::mpsc::channel();
        pane.pick(f.seq, [32.0, 24.0], Box::new(move |p| tx.send(p).unwrap()));
        let p = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(p.block.is_some(), "{p:?}");
        let (tx, rx) = std::sync::mpsc::channel();
        pane.pick(
            f.seq + 10,
            [32.0, 24.0],
            Box::new(move |p| tx.send(p).unwrap()),
        );
        assert_eq!(rx.recv_timeout(Duration::from_secs(5)).unwrap().block, None);
        pane.stop();
    }

    #[test]
    fn the_software_floor_draws_half_the_size_and_says_so() {
        let (pane, renderer) = started(Behaviour {
            software: true,
            ..Behaviour::default()
        });
        renderer.live_show(Some(shown("c")));
        assert_eq!(frame(&pane).size, [32, 24]);
        assert_eq!(pane.facts().floor.as_deref(), Some(FLOOR));
        pane.stop();
    }

    #[test]
    fn a_second_lost_device_fails_the_pane() {
        let (pane, renderer) = started(Behaviour::default());
        renderer.live_show(Some(shown("c")));
        frame(&pane);
        pane.lose_device();
        frame(&pane);
        assert_eq!(pane.status(), PaneStatus::Drawing);
        pane.lose_device();
        assert_eq!(pane.status(), PaneStatus::Failed(DEVICE_LOST.into()));
        pane.stop();
    }

    #[test]
    fn a_hidden_pane_draws_nothing() {
        let (pane, renderer) = started(Behaviour::default());
        pane.visible(false);
        renderer.live_show(Some(shown("c")));
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(pane.counts().drawn, 0);
        pane.visible(true);
        frame(&pane);
        pane.stop();
    }

    #[test]
    fn anchors_are_projected_and_one_behind_the_eye_is_none() {
        let (pane, renderer) = started(Behaviour::default());
        let world = ModelWorld::chat();
        let centre = [12.0, 64.0, 12.0];
        let _ = world;
        renderer.live_show(Some(shown("c")));
        frame(&pane);
        let eye = pane.camera_state().eye;
        let behind = [0, 1, 2].map(|a| eye[a] * 2.0 - centre[a]);
        pane.overlays(Overlays {
            anchors: vec![centre, behind],
            ..Overlays::default()
        });
        let f = frame(&pane);
        assert!(f.anchors[0].is_some());
        assert!(f.anchors[1].is_none());
        pane.stop();
    }

    #[test]
    fn a_pick_at_the_centre_of_a_window_sized_pane_hits_the_world() {
        let pane = FakePane::manual(Behaviour::default());
        pane.keep("live:c", ModelWorld::chat());
        let renderer = FakeRenderer(pane.clone());
        pane.resize(PaneSize {
            width: 497,
            height: 816,
            scale: 1.0,
        });
        renderer.live_show(Some(shown("c")));
        pane.pump();
        let f = pane.take().expect("a frame");
        let (tx, rx) = std::sync::mpsc::channel();
        pane.pick(
            f.seq,
            [248.5, 408.0],
            Box::new(move |p| tx.send(p).unwrap()),
        );
        pane.pump();
        let p = rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(p.block.is_some(), "{p:?} {:?}", f.camera);
    }
}
