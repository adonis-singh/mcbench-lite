// The live view's camera: BlueMap 5.22's ControlsManager, MapControls and
// FreeFlightControls (with the controls they hold), its view changes in
// BlueMapApp, the Compass and the zoom buttons, ported from the sources in
// the baked BlueMap web app's source map (web/assets/index-BxD3mV_W.js.map)
// and cited as BlueMap:<path>:<line>; and mcbench's own bridge on top of it
// (web/bluemap-bridge.js: the fly that frames a build, player mode's eye;
// web/bluemap-view.mjs: the target's height over a build), cited by file
// and line in this repository.
//
// BlueMap is licensed under the MIT License (MIT):
// Copyright (c) Blue (Lukas Rieger) <https://bluecolored.de>
// Copyright (c) contributors
// Permission is hereby granted, free of charge, to any person obtaining a
// copy of this software and associated documentation files (the
// "Software"), to deal in the Software without restriction, including
// without limitation the rights to use, copy, modify, merge, publish,
// distribute, sublicense, and/or sell copies of the Software, and to permit
// persons to whom the Software is furnished to do so, subject to the
// following conditions: The above copyright notice and this permission
// notice shall be included in all copies or substantial portions of the
// Software. THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
// EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
// MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN
// NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM,
// DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR
// OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE
// USE OR OTHER DEALINGS IN THE SOFTWARE.
//
// BlueMap's controls run once a frame with the frame's time; here they run
// once a drawn frame (`update`), with the time since the last one, so the
// camera moves at BlueMap's speeds whatever the frame rate. Pointer
// distances are physical pixels over the pane's physical size, which is
// BlueMap's CSS pixels over its CSS size.

use crate::camera::{invert, multiply, Mat4};
use mcbench_render::live::{keys, CameraInput, CameraMode, CameraState, DragButton, PlayerView};
use std::f64::consts::{FRAC_PI_2, PI};

// BlueMap:src/js/MapViewer.js:108: the camera's vertical field of view.
pub const MAP_FOV: f64 = 75.0;
// web/bluemap-bridge.js:821: Minecraft's default field of view.
pub const PLAYER_FOV: f64 = 70.0;
// web/bluemap-bridge.js:699: the ticks player mode's eye moves between.
const TICK_MS: f64 = 50.0;

// BlueMap:src/js/util/Utils.js:236,238
fn ease_out_quad(t: f64) -> f64 {
    t * (2.0 - t)
}
fn ease_in_out_quad(t: f64) -> f64 {
    if t < 0.5 {
        2.0 * t * t
    } else {
        -1.0 + (4.0 - 2.0 * t) * t
    }
}

// BlueMap:src/js/util/Utils.js:343-385
fn soft_min(value: f64, min: f64, stiffness: f64) -> f64 {
    if value >= min {
        return value;
    }
    let delta = min - value;
    if delta < 0.0001 {
        return min;
    }
    value + delta * stiffness
}
fn soft_max(value: f64, max: f64, stiffness: f64) -> f64 {
    if value <= max {
        return value;
    }
    let delta = value - max;
    if delta < 0.0001 {
        return max;
    }
    value - delta * stiffness
}
fn soft_clamp(value: f64, min: f64, max: f64, stiffness: f64) -> f64 {
    soft_max(soft_min(value, min, stiffness), max, stiffness)
}
fn soft_set(value: f64, target: f64, stiffness: f64) -> f64 {
    soft_clamp(value, target, target, stiffness)
}

fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

// The smoothing of a pending delta for a frame of `delta` ms:
// BlueMap:src/js/controls/map/mouse/MouseMoveControls.js:82-83 and each control alike.
fn smoothing(stiffness: f64, delta: f64) -> f64 {
    (stiffness / (16.666 / delta)).clamp(0.0, 1.0)
}

// three.js Vector2.rotateAround(0, angle)
fn rotate2(v: [f64; 2], angle: f64) -> [f64; 2] {
    let (s, c) = angle.sin_cos();
    [v[0] * c - v[1] * s, v[0] * s + v[1] * c]
}

// BlueMap:src/js/controls/map/MapControls.js:214-216
pub fn max_angle_for_distance(distance: f64) -> f64 {
    ((1.0 - ((distance - 5.0).max(0.001) * 0.0005).powf(0.5)) * FRAC_PI_2).clamp(0.0, FRAC_PI_2)
}
// BlueMap:src/js/controls/map/MapControls.js:219-221
fn max_distance_for_angle(angle: f64) -> f64 {
    (-(angle / FRAC_PI_2) + 1.0).powi(2) * 2000.0 + 5.0
}

// A pending delta's controller: speed and stiffness as BlueMap constructs it.
#[derive(Clone, Copy, Debug)]
struct Control {
    speed: f64,
    stiffness: f64,
}

// MapControls' controllers: BlueMap:src/js/controls/map/MapControls.js:70-78, 81, 85.
const MOUSE_MOVE: Control = Control {
    speed: 1.5,
    stiffness: 0.3,
}; // MapControls.js:70
const MOUSE_ROTATE: Control = Control {
    speed: 6.0,
    stiffness: 0.3,
}; // MapControls.js:71
const MOUSE_ANGLE: Control = Control {
    speed: 3.0,
    stiffness: 0.3,
}; // MapControls.js:72
const MOUSE_ZOOM: Control = Control {
    speed: 1.0,
    stiffness: 0.2,
}; // MapControls.js:73
const KEY_MOVE: Control = Control {
    speed: 0.025,
    stiffness: 0.2,
}; // MapControls.js:75
const KEY_ROTATE: Control = Control {
    speed: 0.06,
    stiffness: 0.15,
}; // MapControls.js:76
const KEY_ANGLE: Control = Control {
    speed: 0.04,
    stiffness: 0.15,
}; // MapControls.js:77
const KEY_ZOOM: Control = Control {
    speed: 0.2,
    stiffness: 0.15,
}; // MapControls.js:78
const TOUCH_ROTATE: Control = Control {
    speed: 0.0174533,
    stiffness: 0.3,
}; // MapControls.js:81
   // MapHeightControls(cameraHeightStiffness 0.2, targetHeightStiffness 0.1): MapControls.js:85
const CAMERA_HEIGHT_STIFFNESS: f64 = 0.2;
const TARGET_HEIGHT_STIFFNESS: f64 = 0.1;
// BlueMap:src/js/controls/map/MapControls.js:90-91
const MIN_DISTANCE: f64 = 5.0;
const MAX_DISTANCE: f64 = 100000.0;

// FreeFlightControls' controllers: BlueMap:src/js/controls/freeflight/FreeFlightControls.js:55-58, 64.
const FREE_KEY_MOVE_STIFFNESS: f64 = 0.1; // FreeFlightControls.js:55
const FREE_KEY_HEIGHT_STIFFNESS: f64 = 0.2; // FreeFlightControls.js:56
                                            // MouseRotateControls and MouseAngleControls(speedLeft 1.5, speedRight -2,
                                            // speedCapture -1.5, stiffness 0.5): FreeFlightControls.js:57-58, the
                                            // right and captured speeds times the mouse sensitivity, default 1
                                            // (BlueMap:src/js/BlueMapApp.js:446-449).
const FREE_LOOK_LEFT: f64 = 1.5;
const FREE_LOOK_RIGHT: f64 = -2.0;
const FREE_LOOK_CAPTURE: f64 = -1.5;
const FREE_LOOK_STIFFNESS: f64 = 0.5;
const FREE_MOVE_SPEED: f64 = 0.5; // FreeFlightControls.js:64

// What BlueMap's ControlsManager holds of the camera.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Manager {
    position: [f64; 3],
    distance: f64,
    rotation: f64,
    angle: f64,
    ortho: f64,
    tilt: f64,
}

// A move between views (BlueMap's viewAnimation), the Compass's, a fly
// (the bridge's flight): from a start over `ms`, each frame setting the
// manager from the eased progress.
#[derive(Clone, Copy, Debug)]
enum Move {
    // BlueMap:src/js/BlueMapApp.js:461-497
    Perspective {
        from: Manager,
        target_y: f64,
        target_distance: f64,
        target_angle: f64,
    },
    // BlueMap:src/js/BlueMapApp.js:499-531
    Flat {
        from: Manager,
        target_distance: f64,
    },
    // BlueMap:src/js/BlueMapApp.js:533-567
    Free {
        from: Manager,
        target_y: f64,
    },
    // BlueMap:src/js/BlueMapApp.js:799-821
    Zoom {
        from: Manager,
        target_distance: f64,
        x: f64,
        z: f64,
    },
    // BlueMap:src/components/ControlBar/Compass.vue:32-41
    North {
        from: f64,
    },
    // web/bluemap-bridge.js:511-554
    Fly {
        from: Manager,
        to: Manager,
        free: bool,
    },
}

#[derive(Clone, Copy, Debug)]
struct Animation {
    kind: Move,
    start: f64,
    ms: f64,
    // Whether the controls are taken off the manager while it runs (a
    // move between views): BlueMap sets `cm.controls = null`.
    holds: bool,
}

// Player mode's camera between two ticks (web/bluemap-bridge.js:757-886).
#[derive(Clone, Copy, Debug)]
struct Eye {
    view: PlayerView,
    walk: f64,
    walk_o: f64,
    bob: f64,
    bob_o: f64,
    eye: f64,
    eye_o: f64,
    fov: f64,
    fov_o: f64,
    // The steady clock's start (ms) and the latest tick.
    clock: Option<f64>,
    tick: u64,
}

// web/bluemap-bridge.js:824
fn fov_modifier(view: &PlayerView) -> f64 {
    (if view.flying { 1.1 } else { 1.0 }) * ((if view.sprinting { 1.3 } else { 1.0 }) + 1.0) / 2.0
}

// The view saved on entering player mode (web/bluemap-bridge.js:600-613).
#[derive(Clone, Copy, Debug)]
struct Saved {
    manager: Manager,
    mode: CameraMode,
}

pub struct Camera {
    pub mode: CameraMode,
    m: Manager,
    // MapControls' pending deltas.
    mouse_move: [f64; 2],
    mouse_rotate: f64,
    mouse_angle: f64,
    dynamic_distance: bool,
    start_distance: f64,
    mouse_zoom: f64,
    key_move: [f64; 2],
    key_rotate: f64,
    key_angle: f64,
    key_zoom: f64,
    touch_rotate: f64,
    touch_zoom: f64,
    // MapHeightControls
    target_height: f64,
    camera_height: f64,
    min_camera_height: f64,
    distance_target_height: f64,
    // FreeFlightControls' pending deltas and speed.
    free_move: [f64; 2],
    free_height: f64,
    free_rotate: f64,
    free_angle: f64,
    move_speed: f64,
    keys: u16,
    // A button went down and no drag has followed it yet.
    pressed: bool,
    rotating: bool,
    animation: Option<Animation>,
    saved: Option<Saved>,
    eye: Option<Eye>,
    look: Option<(f32, f32)>,
    // ControlsManager.averageDeltaTime and the time of the last update.
    average_delta: f64,
    last: Option<f64>,
    // The pane's physical size.
    size: [u32; 2],
    // The time of the last update (ms): what a frame is drawn at.
    now: f64,
    // Whether the last update left the camera as it found it: BlueMap's
    // controls ease (the target's height, the soft clamps) as long as its
    // frames run, so the camera is still only once an update changes nothing
    // (ControlsManager.isValueChanged, BlueMap:src/js/controls/ControlsManager.js:182-191).
    settled: bool,
}

// The shown world's terrain height at x, z (Camera::terrain).
pub type Terrain<'a> = &'a dyn Fn(f64, f64) -> Option<f64>;

// What a frame draws the camera as: its matrices (gl-matrix's convention,
// column-major, OpenGL clip space) and whether the lens is orthographic in
// part (its depth then linear).
#[derive(Clone, Copy, Debug)]
pub struct Lens {
    pub view: Mat4,
    pub proj: Mat4,
    pub sky_unproject: Mat4,
    pub linear: bool,
    pub eye: [f64; 3],
}

impl Camera {
    // BlueMap's start (BlueMap:src/js/controls/ControlsManager.js:69-74).
    pub fn new() -> Camera {
        Camera {
            mode: CameraMode::Perspective,
            m: Manager {
                position: [0.0; 3],
                distance: 300.0,
                rotation: 0.0,
                angle: 0.0,
                ortho: 0.0,
                tilt: 0.0,
            },
            mouse_move: [0.0; 2],
            mouse_rotate: 0.0,
            mouse_angle: 0.0,
            dynamic_distance: false,
            start_distance: 0.0,
            mouse_zoom: 0.0,
            key_move: [0.0; 2],
            key_rotate: 0.0,
            key_angle: 0.0,
            key_zoom: 0.0,
            touch_rotate: 0.0,
            touch_zoom: 1.0,
            target_height: 0.0,
            camera_height: 0.0,
            min_camera_height: 0.0,
            distance_target_height: 0.0,
            free_move: [0.0; 2],
            free_height: 0.0,
            free_rotate: 0.0,
            free_angle: 0.0,
            move_speed: FREE_MOVE_SPEED,
            keys: 0,
            pressed: false,
            rotating: false,
            animation: None,
            saved: None,
            eye: None,
            look: None,
            average_delta: 16.0,
            last: None,
            size: [1, 1],
            now: 0.0,
            settled: true,
        }
    }

    pub fn resize(&mut self, size: [u32; 2]) {
        self.size = [size[0].max(1), size[1].max(1)];
    }

    fn width(&self) -> f64 {
        self.size[0] as f64
    }
    fn height(&self) -> f64 {
        self.size[1] as f64
    }

    fn map_controls(&self) -> bool {
        matches!(self.mode, CameraMode::Perspective | CameraMode::Flat) && !self.held()
    }
    fn free_controls(&self) -> bool {
        self.mode == CameraMode::Spectator && !self.held()
    }
    fn held(&self) -> bool {
        self.animation.is_some_and(|a| a.holds)
    }

    // Whether the camera still moves without input: a pending delta, a key
    // held, a move between views or a fly, player mode's eye between ticks.
    pub fn moving(&self, now: f64) -> bool {
        let map = self.mouse_move != [0.0; 2]
            || self.mouse_rotate != 0.0
            || self.mouse_angle != 0.0
            || self.mouse_zoom != 0.0
            || self.key_move != [0.0; 2]
            || self.key_rotate != 0.0
            || self.key_angle != 0.0
            || self.key_zoom != 0.0
            || self.touch_rotate != 0.0
            || self.touch_zoom != 1.0;
        let free = self.free_move != [0.0; 2]
            || self.free_height != 0.0
            || self.free_rotate != 0.0
            || self.free_angle != 0.0;
        let between_ticks = self.mode == CameraMode::Player
            && self.eye.is_some_and(|e| self.partial(&e, now) < 1.0);
        map || free || self.keys != 0 || self.animation.is_some() || between_ticks || !self.settled
    }

    // An input, at `now` (ms). `pick` answers the point under a pixel of a
    // kept frame (a double click's zoom).
    pub fn input(
        &mut self,
        input: &CameraInput,
        now: f64,
        hit: Option<[f64; 3]>,
        terrain: Terrain,
    ) {
        // The first input after a still moment starts BlueMap's frame clock
        // one frame (60 Hz) back, as its loop would have been running.
        if !self.moving(now) {
            self.last = Some(now - 16.666);
        }
        self.settled = false;
        match *input {
            CameraInput::PointerDown => {
                // web/bluemap-bridge.js:1042-1043: a pointer down cancels a fly.
                if self
                    .animation
                    .is_some_and(|a| matches!(a.kind, Move::Fly { .. }))
                {
                    self.animation = None;
                }
                self.pressed = true;
            }
            CameraInput::Drag { button, dx, dy } => self.drag(button, dx as f64, dy as f64),
            CameraInput::Wheel { delta, lines } => {
                // BlueMap:src/js/controls/map/mouse/MouseZoomControls.js:89-93 and
                // BlueMap:src/js/controls/freeflight/FreeFlightControls.js:185-187
                let delta = delta as f64 * if lines { 0.33 } else { 0.01 };
                if self.map_controls() {
                    self.mouse_zoom += delta;
                    // BlueMap:src/js/controls/map/mouse/MouseAngleControls.js:146-148
                    self.dynamic_distance = false;
                } else if self.free_controls() {
                    // BlueMap:src/js/controls/freeflight/FreeFlightControls.js:189-193
                    self.move_speed =
                        (self.move_speed * 1.5f64.powf(-delta * 0.25)).clamp(0.05, 5.0);
                }
            }
            CameraInput::Pinch { scale } => {
                // BlueMap:src/js/controls/map/touch/TouchZoomControls.js:89-93
                if self.map_controls() {
                    self.touch_zoom *= scale as f64;
                    self.m.angle = self.m.angle.min(max_angle_for_distance(self.m.distance));
                }
            }
            CameraInput::Twist { radians } => {
                // BlueMap:src/js/controls/map/touch/TouchRotateControls.js:102-111: degrees
                if self.map_controls() {
                    self.touch_rotate -= radians as f64 * 180.0 / PI;
                }
            }
            CameraInput::ZoomStep(step) => {
                // BlueMap:src/components/Controls/ZoomButtons.vue:30-35: the step
                // added to the controls' mouseZoom.deltaZoom when the controls
                // have one: the map controls do (MapControls.js:73), free
                // flight's do not (FreeFlightControls.js:55-59), and during a
                // move between views there are no controls (BlueMapApp.js:467),
                // so in spectator and player mode it does nothing.
                if self.map_controls() {
                    self.mouse_zoom += step as f64;
                }
            }
            CameraInput::Keys(held) => self.keys = held,
            CameraInput::DoubleClick { .. } => {
                // BlueMap:src/js/BlueMapApp.js:799-821, from the map controls' tap
                // (BlueMap:src/js/controls/map/MapControls.js:260-274).
                if let (true, Some(point)) = (self.map_controls(), hit) {
                    let from = self.m;
                    self.animation = Some(Animation {
                        kind: Move::Zoom {
                            from,
                            target_distance: (from.distance * 0.25).max(5.0),
                            x: point[0],
                            z: point[2],
                        },
                        start: now,
                        ms: 500.0,
                        holds: false,
                    });
                }
            }
            CameraInput::Frame { min, max, ms } => self.fly(min, max, ms as f64, now),
            CameraInput::Reset => self.reset(),
            CameraInput::FaceNorth => {
                self.animation = Some(Animation {
                    kind: Move::North {
                        from: self.m.rotation,
                    },
                    start: now,
                    ms: 300.0,
                    holds: false,
                });
            }
            CameraInput::Target { x, z } => {
                self.m.position[0] = x;
                self.m.position[2] = z;
            }
            CameraInput::Mode(mode) => self.set_mode(mode, now, terrain),
            CameraInput::Look { yaw, pitch } => self.look = Some((yaw, pitch)),
            CameraInput::LockedLook { dx, dy } => {
                // BlueMap:src/js/controls/freeflight/mouse/MouseRotateControls.js and
                // MouseAngleControls.js:112-113: under the pointer lock the
                // movement times speedCapture over the target's height, with
                // no button and no press.
                if self.free_controls() {
                    let per_pixel = 1.0 / self.height();
                    self.free_rotate -= dx as f64 * FREE_LOOK_CAPTURE * per_pixel;
                    self.free_angle += dy as f64 * FREE_LOOK_CAPTURE * per_pixel;
                }
            }
        }
    }

    fn drag(&mut self, button: DragButton, dx: f64, dy: f64) {
        let press = std::mem::take(&mut self.pressed);
        if self.free_controls() {
            // BlueMap:src/js/controls/freeflight/mouse/MouseRotateControls.js:101-124,
            // MouseAngleControls.js:101-124: the left button's speed (`buttons
            // === 1`, whatever the modifiers) or the right's.
            if press {
                self.free_rotate = 0.0;
                self.free_angle = 0.0;
            }
            let speed = if button == DragButton::Right {
                FREE_LOOK_RIGHT
            } else {
                FREE_LOOK_LEFT
            };
            // MouseRotateControls.js:135-137, MouseAngleControls.js:135-137: over the height.
            let per_pixel = 1.0 / self.height();
            self.free_rotate -= dx * speed * per_pixel;
            self.free_angle += dy * speed * per_pixel;
            return;
        }
        if !self.map_controls() {
            return;
        }
        // BlueMap:src/js/controls/map/mouse/MouseMoveControls.js:106: the left
        // button pans unless Alt is held (so with Ctrl it pans too);
        // MouseRotateControls.js:98-99 and MouseAngleControls.js:115-116: the
        // right button, or the left with Alt or Ctrl, rotates and tilts.
        let pans = matches!(button, DragButton::Left | DragButton::CtrlLeft);
        let turns = matches!(
            button,
            DragButton::Right | DragButton::CtrlLeft | DragButton::AltLeft
        );
        if pans {
            // MouseMoveControls.js:105-125
            if press {
                self.mouse_move = [0.0; 2];
            }
            self.mouse_move[0] -= dx;
            self.mouse_move[1] -= dy;
        }
        if turns {
            // MouseRotateControls.js:97-116 and MouseAngleControls.js:114-136
            if press {
                self.mouse_rotate = 0.0;
                self.mouse_angle = 0.0;
                self.start_distance = self.m.distance;
                self.dynamic_distance = self.m.distance < 1000.0;
            }
            self.mouse_rotate += dx;
            self.mouse_angle -= dy;
        }
    }

    // BlueMap:src/js/BlueMapApp.js:271-299 with the map's start position
    // (0, 0) (server/bluemap.js:97).
    fn reset(&mut self) {
        self.animation = None;
        self.m = Manager {
            position: [0.0; 3],
            distance: 1500.0,
            angle: 0.0,
            rotation: 0.0,
            tilt: 0.0,
            ortho: 0.0,
        };
        self.mode = CameraMode::Perspective;
        self.saved = None;
        self.eye = None;
        self.clear_deltas();
    }

    // BlueMap's mapControls.reset() (BlueMap:src/js/controls/map/MapControls.js:202-212).
    fn clear_deltas(&mut self) {
        self.mouse_move = [0.0; 2];
        self.mouse_rotate = 0.0;
        self.mouse_angle = 0.0;
        self.mouse_zoom = 0.0;
        self.touch_rotate = 0.0;
        self.touch_zoom = 1.0;
    }

    fn set_mode(&mut self, mode: CameraMode, now: f64, terrain: Terrain) {
        let from_free = matches!(self.mode, CameraMode::Spectator | CameraMode::Player);
        // web/bluemap-bridge.js:614-637: leaving player mode for the map gives
        // back the view it left, exactly, then moves to the other map view if
        // that one is asked.
        if self.mode == CameraMode::Player
            && matches!(mode, CameraMode::Perspective | CameraMode::Flat)
        {
            if let Some(saved) = self.saved.take() {
                self.animation = None;
                self.m = saved.manager;
                self.mode = saved.mode;
                self.eye = None;
                self.look = None;
                self.clear_deltas();
                if saved.mode != mode {
                    self.set_mode(mode, now, terrain);
                }
                return;
            }
        }
        match mode {
            CameraMode::Perspective => {
                // setPerspectiveView(500, free ? 100 : 0) (web/bluemap-bridge.js:617-637;
                // BlueMap:src/components/ControlBar/ControlsSwitch.vue)
                let min = if from_free { 100.0 } else { 0.0 };
                let from = self.m;
                // BlueMap:src/js/BlueMapApp.js:469-476
                let target_distance = 5f64.max(min).max(from.distance);
                let target_y = lerp(
                    terrain(from.position[0], from.position[2]).unwrap_or(0.0) + 3.0,
                    0.0,
                    target_distance / 500.0,
                );
                let target_angle = FRAC_PI_2
                    .min(from.angle)
                    .min(max_angle_for_distance(target_distance));
                self.animation = Some(Animation {
                    kind: Move::Perspective {
                        from,
                        target_y,
                        target_distance,
                        target_angle,
                    },
                    start: now,
                    ms: 500.0,
                    holds: true,
                });
            }
            CameraMode::Flat => {
                let min = if from_free { 100.0 } else { 0.0 };
                let from = self.m;
                // BlueMap:src/js/BlueMapApp.js:507-508
                let target_distance = 5f64.max(min).max(from.distance);
                self.animation = Some(Animation {
                    kind: Move::Flat {
                        from,
                        target_distance,
                    },
                    start: now,
                    ms: 500.0,
                    holds: true,
                });
            }
            CameraMode::Spectator => {
                // setFreeFlight(500) (web/bluemap-bridge.js:639-647)
                let from = self.m;
                // BlueMap:src/js/BlueMapApp.js:543-544
                let target_y = terrain(from.position[0], from.position[2])
                    .map(|h| h + 3.0)
                    .unwrap_or(from.position[1]);
                self.animation = Some(Animation {
                    kind: Move::Free { from, target_y },
                    start: now,
                    ms: 500.0,
                    holds: true,
                });
            }
            CameraMode::Player => {
                // web/bluemap-bridge.js:600-613: the map's view saved, no move;
                // the camera keeps it until the player stands.
                if self.mode == CameraMode::Player {
                    return;
                }
                if matches!(self.mode, CameraMode::Perspective | CameraMode::Flat) {
                    self.saved = Some(Saved {
                        manager: self.m,
                        mode: self.mode,
                    });
                }
                self.animation = None;
                self.eye = None;
                self.look = None;
            }
        }
        self.mode = mode;
    }

    // web/bluemap-bridge.js:511-554: the build's bounding sphere filling the
    // view from the current heading, at an angle within 0.45 and 1.3 of
    // straight down; in player mode the player is moved instead (the window's
    // SpawnPlayer), so the camera does nothing.
    pub fn fly(&mut self, min: [i32; 3], max: [i32; 3], ms: f64, now: f64) {
        if self.mode == CameraMode::Player {
            return;
        }
        let center = [0, 1, 2].map(|i| (min[i] + max[i] + 1) as f64 / 2.0);
        let size = [0, 1, 2].map(|i| (max[i] - min[i] + 1) as f64);
        let radius = (size[0] * size[0] + size[1] * size[1] + size[2] * size[2]).sqrt() / 2.0;
        let vertical = self.fov() * PI / 180.0;
        let horizontal = 2.0 * ((vertical / 2.0).tan() * self.aspect()).atan();
        // A fifth to spare keeps labels over builds at the edges in view.
        let distance = (radius / (vertical.min(horizontal) / 2.0).sin() * 1.2).max(12.0);
        let angle = self.m.angle.clamp(0.45, 1.3);
        let free = self.mode == CameraMode::Spectator;
        let from = self.m;
        let mut to = self.m;
        if free {
            let heading = [self.m.rotation.sin(), 0.0, -self.m.rotation.cos()];
            let look = tilt(heading, FRAC_PI_2 - angle);
            to.position = [0, 1, 2].map(|i| center[i] - look[i] * distance);
            to.angle = angle;
        } else {
            // Map controls lower the target toward y 0 by distance / 500, which
            // brings the camera that much nearer the build; stand back to match.
            to.position[0] = center[0];
            to.position[2] = center[2];
            to.angle = angle;
            to.distance = distance / (1.0 - center[1] * angle.cos() / 500.0);
        }
        // The previous fly and BlueMap's double-click zoom are cancelled.
        self.animation = Some(Animation {
            kind: Move::Fly { from, to, free },
            start: now,
            ms,
            holds: false,
        });
        if ms == 0.0 {
            self.step_animation(now);
        }
    }

    // Player mode: the view after tick `tick`; the first since the mode was
    // entered starts the eye there (web/bluemap-bridge.js:857-861).
    pub fn player(&mut self, view: PlayerView, tick: u64, now: f64) {
        if self.mode != CameraMode::Player {
            return;
        }
        if !self.moving(now) {
            self.last = Some(now - 16.666);
        }
        match self.eye {
            // web/bluemap-bridge.js:825
            None => {
                let m = fov_modifier(&view);
                self.eye = Some(Eye {
                    view,
                    walk: 0.0,
                    walk_o: 0.0,
                    bob: 0.0,
                    bob_o: 0.0,
                    eye: view.eye_height as f64,
                    eye_o: view.eye_height as f64,
                    fov: m,
                    fov_o: m,
                    clock: None,
                    tick,
                });
            }
            Some(mut c) => {
                // web/bluemap-bridge.js:827-844 (cameraTick)
                let before = c.view;
                c.walk_o = c.walk;
                c.walk +=
                    (view.pos[0] - view.old_pos[0]).hypot(view.pos[2] - view.old_pos[2]) * 0.6;
                let target = if before.on_ground && !before.swimming {
                    0.1f64.min(before.delta[0].hypot(before.delta[2]))
                } else {
                    0.0
                };
                c.bob_o = c.bob;
                c.bob += (target - c.bob) * 0.4;
                c.eye_o = c.eye;
                c.eye += (view.eye_height as f64 - c.eye) * 0.5;
                c.fov_o = c.fov;
                c.fov = (c.fov + (fov_modifier(&view) - c.fov) * 0.5).clamp(0.1, 1.5);
                // web/bluemap-bridge.js:864-868: a steady clock eased toward when ticks arrive.
                let at = now - tick as f64 * TICK_MS;
                c.clock = Some(match c.clock {
                    Some(clock) if (at - clock).abs() <= 3.0 * TICK_MS => {
                        clock + (at - clock) * 0.1
                    }
                    _ => at,
                });
                c.tick = tick;
                c.view = view;
                self.eye = Some(c);
            }
        }
    }

    // The partial tick (web/bluemap-bridge.js:770).
    fn partial(&self, eye: &Eye, now: f64) -> f64 {
        match eye.clock {
            None => 1.0,
            Some(clock) => ((now - (clock + eye.tick as f64 * TICK_MS)) / TICK_MS).clamp(0.0, 1.0),
        }
    }

    // Runs the controls for a frame at `now` (ms): ControlsManager.update
    // (BlueMap:src/js/controls/ControlsManager.js:83-90). `terrain` is
    // BlueMap's map.terrainHeightAt over the shown world (live/world.rs).
    pub fn update(&mut self, now: f64, terrain: Terrain) {
        let (before, fov) = (self.m, self.fov());
        self.now = now;
        let mut delta = match self.last {
            Some(last) => now - last,
            None => 16.666,
        };
        self.last = Some(now);
        // ControlsManager.js:84-85
        if delta > 50.0 {
            delta = 50.0;
        }
        self.average_delta = self.average_delta * 0.9 + delta * 0.1;
        let delta = self.average_delta;
        self.step_animation(now);
        if self.map_controls() {
            self.map_update(delta, terrain);
        } else if self.free_controls() {
            self.free_update(delta);
        } else if self.mode == CameraMode::Player {
            self.player_update(now);
        }
        // ControlsManager.js:98-100: the rotation wrapped.
        while self.m.rotation >= PI {
            self.m.rotation -= PI * 2.0;
        }
        while self.m.rotation <= -PI {
            self.m.rotation += PI * 2.0;
        }
        self.settled = self.m == before && self.fov() == fov;
    }

    fn step_animation(&mut self, now: f64) {
        let Some(a) = self.animation else { return };
        // BlueMap:src/js/util/Utils.js:185
        let p = if a.ms == 0.0 {
            1.0
        } else {
            ((now - a.start) / a.ms).clamp(0.0, 1.0)
        };
        let ep = ease_in_out_quad(p);
        let m = &mut self.m;
        match a.kind {
            Move::Perspective {
                from,
                target_y,
                target_distance,
                target_angle,
            } => {
                // BlueMap:src/js/BlueMapApp.js:481-488
                m.position[1] = lerp(from.position[1], target_y, ep);
                m.distance = lerp(from.distance, target_distance, ep);
                m.angle = lerp(from.angle, target_angle, ep);
                m.ortho = lerp(from.ortho, 0.0, p);
                m.tilt = lerp(from.tilt, 0.0, ep);
            }
            Move::Flat {
                from,
                target_distance,
            } => {
                // BlueMap:src/js/BlueMapApp.js:515-521
                m.distance = lerp(from.distance, target_distance, ep);
                m.rotation = lerp(from.rotation, 0.0, ep);
                m.angle = lerp(from.angle, 0.0, ep);
                m.ortho = lerp(from.ortho, 1.0, p);
                m.tilt = lerp(from.tilt, 0.0, ep);
            }
            Move::Free { from, target_y } => {
                // BlueMap:src/js/BlueMapApp.js:552-558
                m.position[1] = lerp(from.position[1], target_y, ep);
                m.distance = lerp(from.distance, 0.0, ep);
                m.angle = lerp(from.angle, FRAC_PI_2, ep);
                m.ortho = lerp(from.ortho, 0.0, (p * 2.0).min(1.0));
                m.tilt = lerp(from.tilt, 0.0, ep);
            }
            Move::Zoom {
                from,
                target_distance,
                x,
                z,
            } => {
                // BlueMap:src/js/BlueMapApp.js:814-818
                m.distance = lerp(from.distance, target_distance, ep);
                m.position[0] = lerp(from.position[0], x, ep);
                m.position[2] = lerp(from.position[2], z, ep);
            }
            Move::North { from } => {
                // BlueMap:src/components/ControlBar/Compass.vue:38-40
                m.rotation = from * (1.0 - ease_out_quad(p));
            }
            Move::Fly { from, to, free } => {
                // web/bluemap-bridge.js:537-545
                m.position[0] = lerp(from.position[0], to.position[0], ep);
                m.position[2] = lerp(from.position[2], to.position[2], ep);
                m.angle = lerp(from.angle, to.angle, ep);
                if free {
                    m.position[1] = lerp(from.position[1], to.position[1], ep);
                } else {
                    m.distance = lerp(from.distance, to.distance, ep);
                }
            }
        }
        if p >= 1.0 {
            self.animation = None;
            // BlueMap:src/js/BlueMapApp.js:489-493, 523-527: the map controls'
            // deltas cleared when a move between views ends.
            if matches!(a.kind, Move::Perspective { .. } | Move::Flat { .. }) {
                self.clear_deltas();
            }
        }
    }

    // MapControls.update (BlueMap:src/js/controls/map/MapControls.js:149-200).
    fn map_update(&mut self, delta: f64, terrain: Terrain) {
        // MapControls.js:150: the target's y reset; the height controls set it
        // again (MapHeightControls.js:65).
        self.m.position[1] = -10000.0;
        let held = self.keys;
        // move: MouseMoveControls.js:79-95, KeyMoveControls.js:94-115
        if self.mouse_move != [0.0; 2] {
            let s = smoothing(MOUSE_MOVE.stiffness, delta);
            let d = rotate2(self.mouse_move, self.m.rotation);
            let (px, py) = (1.0 / self.height(), 1.0 / self.height());
            self.m.position[0] += d[0] * s * self.m.distance * MOUSE_MOVE.speed * px;
            self.m.position[2] += d[1] * s * self.m.distance * MOUSE_MOVE.speed * py;
            self.mouse_move = self.mouse_move.map(|v| v * (1.0 - s));
            if self.mouse_move[0] * self.mouse_move[0] + self.mouse_move[1] * self.mouse_move[1]
                < 0.0001
            {
                self.mouse_move = [0.0; 2];
            }
        }
        if held & keys::FORWARD != 0 {
            self.key_move[1] -= 1.0;
        }
        if held & keys::BACKWARD != 0 {
            self.key_move[1] += 1.0;
        }
        if held & keys::LEFT != 0 {
            self.key_move[0] -= 1.0;
        }
        if held & keys::RIGHT != 0 {
            self.key_move[0] += 1.0;
        }
        if self.key_move != [0.0; 2] {
            let s = smoothing(KEY_MOVE.stiffness, delta);
            let d = rotate2(self.key_move, self.m.rotation);
            // KeyMoveControls.js:108-109
            self.m.position[0] += d[0] * s * self.m.distance * KEY_MOVE.speed * delta * 0.06;
            self.m.position[2] += d[1] * s * self.m.distance * KEY_MOVE.speed * delta * 0.06;
            self.key_move = self.key_move.map(|v| v * (1.0 - s));
            if self.key_move[0] * self.key_move[0] + self.key_move[1] * self.key_move[1] < 0.0001 {
                self.key_move = [0.0; 2];
            }
        }
        // zoom: MouseZoomControls.js:63-76, KeyZoomControls.js:82-98, TouchZoomControls.js:65-70
        if self.mouse_zoom != 0.0 {
            let s = smoothing(MOUSE_ZOOM.stiffness, delta);
            self.m.distance *= 1.5f64.powf(self.mouse_zoom * s * MOUSE_ZOOM.speed);
            self.m.angle = self.m.angle.min(max_angle_for_distance(self.m.distance));
            self.mouse_zoom *= 1.0 - s;
            if self.mouse_zoom.abs() < 0.0001 {
                self.mouse_zoom = 0.0;
            }
        }
        if held & keys::ZOOM_IN != 0 {
            self.key_zoom -= 1.0;
        }
        if held & keys::ZOOM_OUT != 0 {
            self.key_zoom += 1.0;
        }
        if self.key_zoom != 0.0 {
            let s = smoothing(KEY_ZOOM.stiffness, delta);
            self.m.distance *= 1.5f64.powf(self.key_zoom * s * KEY_ZOOM.speed * delta * 0.06);
            self.m.angle = self.m.angle.min(max_angle_for_distance(self.m.distance));
            self.key_zoom *= 1.0 - s;
            if self.key_zoom.abs() < 0.0001 {
                self.key_zoom = 0.0;
            }
        }
        if self.touch_zoom != 1.0 {
            self.m.distance /= self.touch_zoom;
            self.touch_zoom = 1.0;
        }
        // MapControls.js:173
        self.m.distance = soft_clamp(self.m.distance, MIN_DISTANCE, MAX_DISTANCE, 0.8);
        // rotation: MouseRotateControls.js:75-87, KeyRotateControls.js:83-98, TouchRotateControls.js:70-82
        let mouse_rotating = self.mouse_rotate != 0.0;
        if self.mouse_rotate != 0.0 {
            let s = smoothing(MOUSE_ROTATE.stiffness, delta);
            // MouseRotateControls.js:127: over the width
            self.m.rotation += self.mouse_rotate * s * MOUSE_ROTATE.speed / self.width();
            self.mouse_rotate *= 1.0 - s;
            if self.mouse_rotate.abs() < 0.0001 {
                self.mouse_rotate = 0.0;
            }
        }
        if held & keys::ROTATE_LEFT != 0 {
            self.key_rotate += 1.0;
        }
        if held & keys::ROTATE_RIGHT != 0 {
            self.key_rotate -= 1.0;
        }
        if self.key_rotate != 0.0 {
            let s = smoothing(KEY_ROTATE.stiffness, delta);
            self.m.rotation += self.key_rotate * s * KEY_ROTATE.speed * delta * 0.06;
            self.key_rotate *= 1.0 - s;
            if self.key_rotate.abs() < 0.0001 {
                self.key_rotate = 0.0;
            }
        }
        if self.touch_rotate != 0.0 {
            let s = smoothing(TOUCH_ROTATE.stiffness, delta);
            self.m.rotation += self.touch_rotate * s * TOUCH_ROTATE.speed;
            self.touch_rotate *= 1.0 - s;
            if self.touch_rotate.abs() < 0.0001 {
                self.touch_rotate = 0.0;
            }
        }
        // MapControls.js:180-186: the rotation snaps to north in flat view.
        // `moving` of the mouse and touch rotation is a button held, which a
        // pending delta stands for here.
        let rotating = mouse_rotating
            || self.touch_rotate != 0.0
            || held & (keys::ROTATE_LEFT | keys::ROTATE_RIGHT) != 0;
        self.rotating = rotating;
        if self.m.ortho != 0.0 && self.m.rotation.abs() < if rotating { 0.05 } else { 0.3 } {
            self.m.rotation = soft_clamp(self.m.rotation, 0.0, 0.0, 0.1);
        }
        // tilt: MapControls.js:189-194
        if self.m.ortho == 0.0 {
            if self.mouse_angle != 0.0 {
                // MouseAngleControls.js:82-104
                let s = smoothing(MOUSE_ANGLE.stiffness, delta);
                self.m.angle += self.mouse_angle * s * MOUSE_ANGLE.speed / self.height();
                if self.dynamic_distance {
                    let target = self
                        .start_distance
                        .min(max_distance_for_angle(self.m.angle))
                        .max(MIN_DISTANCE);
                    self.m.distance = soft_set(self.m.distance, target, 0.4);
                    self.m.angle = soft_max(self.m.angle, max_angle_for_distance(target), 0.8);
                } else {
                    self.m.angle =
                        soft_max(self.m.angle, max_angle_for_distance(self.m.distance), 0.8);
                }
                self.mouse_angle *= 1.0 - s;
                if self.mouse_angle.abs() < 0.0001 {
                    self.mouse_angle = 0.0;
                }
            }
            // KeyAngleControls.js:85-101
            if held & keys::TILT_UP != 0 {
                self.key_angle -= 1.0;
            }
            if held & keys::TILT_DOWN != 0 {
                self.key_angle += 1.0;
            }
            if self.key_angle != 0.0 {
                let s = smoothing(KEY_ANGLE.stiffness, delta);
                self.m.angle += self.key_angle * s * KEY_ANGLE.speed * delta * 0.06;
                self.m.angle = soft_max(self.m.angle, max_angle_for_distance(self.m.distance), 0.8);
                self.key_angle *= 1.0 - s;
                if self.key_angle.abs() < 0.0001 {
                    self.key_angle = 0.0;
                }
            }
            self.m.angle = soft_clamp(self.m.angle, 0.0, FRAC_PI_2, 0.8);
        }
        // target height: MapControls.js:197-199
        if self.m.ortho == 0.0 || self.m.angle == 0.0 {
            self.height_update(delta, terrain);
        }
    }

    // MapHeightControls.update (BlueMap:src/js/controls/map/MapHeightControls.js:61-102).
    fn height_update(&mut self, delta: f64, terrain: Terrain) {
        let target_smoothing = smoothing(TARGET_HEIGHT_STIFFNESS, delta);
        // MapHeightControls.js:73: `terrainHeightAt(x, z) + 3 || 0`, where a
        // world without a surface under the point answers false (3).
        let target_terrain = terrain(self.m.position[0], self.m.position[2]).unwrap_or(0.0) + 3.0;
        let target_delta = target_terrain - self.target_height;
        self.target_height += target_delta * target_smoothing;
        if target_delta.abs() < 0.001 {
            self.target_height = target_terrain;
        }
        self.min_camera_height = 0.0;
        let max_angle = max_angle_for_distance(self.m.distance);
        if max_angle >= 0.1 {
            let camera_smoothing = smoothing(CAMERA_HEIGHT_STIFFNESS, delta);
            let eye = self.eye_position();
            let camera_terrain = terrain(eye[0], eye[2]).unwrap_or(0.0);
            let camera_delta = camera_terrain - self.camera_height;
            self.camera_height += camera_delta * camera_smoothing;
            if camera_delta.abs() < 0.001 {
                self.camera_height = camera_terrain;
            }
            let max_angle_height = max_angle.cos() * self.m.distance;
            self.min_camera_height = self.camera_height - max_angle_height + 1.0;
        }
        self.distance_target_height =
            lerp(self.target_height, 0.0, (self.m.distance / 500.0).min(1.0));
        self.m.position[1] =
            self.m.position[1].max(self.distance_target_height.max(self.min_camera_height));
    }

    // FreeFlightControls.update (BlueMap:src/js/controls/freeflight/FreeFlightControls.js:104-134).
    fn free_update(&mut self, delta: f64) {
        let held = self.keys;
        // freeflight/keyboard/KeyMoveControls.js:94-115
        if held & keys::FORWARD != 0 {
            self.free_move[1] -= 1.0;
        }
        if held & keys::BACKWARD != 0 {
            self.free_move[1] += 1.0;
        }
        if held & keys::LEFT != 0 {
            self.free_move[0] -= 1.0;
        }
        if held & keys::RIGHT != 0 {
            self.free_move[0] += 1.0;
        }
        if self.free_move != [0.0; 2] {
            let s = smoothing(FREE_KEY_MOVE_STIFFNESS, delta);
            let d = rotate2(self.free_move, self.m.rotation);
            self.m.position[0] += d[0] * s * self.move_speed * delta * 0.06;
            self.m.position[2] += d[1] * s * self.move_speed * delta * 0.06;
            self.free_move = self.free_move.map(|v| v * (1.0 - s));
            if self.free_move[0] * self.free_move[0] + self.free_move[1] * self.free_move[1]
                < 0.0001
            {
                self.free_move = [0.0; 2];
            }
        }
        // freeflight/keyboard/KeyHeightControls.js:82-97
        if held & keys::UP != 0 {
            self.free_height += 1.0;
        }
        if held & keys::DOWN != 0 {
            self.free_height -= 1.0;
        }
        if self.free_height != 0.0 {
            let s = smoothing(FREE_KEY_HEIGHT_STIFFNESS, delta);
            self.m.position[1] += self.free_height * s * self.move_speed * delta * 0.06;
            self.free_height *= 1.0 - s;
            if self.free_height.abs() < 0.0001 {
                self.free_height = 0.0;
            }
        }
        // freeflight/mouse/MouseRotateControls.js:79-91, MouseAngleControls.js:79-91
        if self.free_rotate != 0.0 {
            let s = smoothing(FREE_LOOK_STIFFNESS, delta);
            self.m.rotation += self.free_rotate * s;
            self.free_rotate *= 1.0 - s;
            if self.free_rotate.abs() < 0.0001 {
                self.free_rotate = 0.0;
            }
        }
        if self.free_angle != 0.0 {
            let s = smoothing(FREE_LOOK_STIFFNESS, delta);
            self.m.angle += self.free_angle * s;
            self.free_angle *= 1.0 - s;
            if self.free_angle.abs() < 0.0001 {
                self.free_angle = 0.0;
            }
        }
        // FreeFlightControls.js:131-133
        self.m.angle = self.m.angle.clamp(0.0, PI);
        self.m.distance = 0.0;
        self.m.ortho = 0.0;
    }

    // PlayerControls.update (web/bluemap-bridge.js:757-786): the eye between
    // the last two ticks, the look of the window's last Look (else the
    // tick's), the field of view eased.
    fn player_update(&mut self, now: f64) {
        let Some(c) = self.eye else { return };
        let v = c.view;
        let t = self.partial(&c, now);
        self.m.distance = 0.0;
        self.m.ortho = 0.0;
        let eye = lerp(c.eye_o, c.eye, t);
        self.m.position = [
            lerp(v.old_pos[0], v.pos[0], t),
            lerp(v.old_pos[1], v.pos[1], t) + eye,
            lerp(v.old_pos[2], v.pos[2], t),
        ];
        let (yaw, pitch) = self.look.unwrap_or((v.y_rot, v.x_rot));
        self.m.rotation = (yaw as f64 + 180.0) * PI / 180.0;
        self.m.angle = (90.0 - pitch as f64) * PI / 180.0;
        self.m.tilt = 0.0;
    }

    pub fn fov(&self) -> f64 {
        match (self.mode, self.eye) {
            // web/bluemap-bridge.js:779
            (CameraMode::Player, Some(c)) => {
                let t = self.partial(&c, self.now);
                PLAYER_FOV
                    * lerp(c.fov_o, c.fov, t)
                    * if c.view.eye_in_water || c.view.eye_in_lava {
                        0.85714287
                    } else {
                        1.0
                    }
            }
            _ => MAP_FOV,
        }
    }

    fn aspect(&self) -> f64 {
        self.width() / self.height()
    }

    // ControlsManager.updateCamera's rotatable angle and distance and the
    // eye (BlueMap:src/js/controls/ControlsManager.js:102-122).
    fn rotatable(&self) -> (f64, f64) {
        let mut angle = self.m.angle;
        if angle.abs() <= 0.0001 {
            angle = 0.0001;
        } else if angle.abs() - PI <= 0.0001 {
            angle -= 0.0001;
        }
        let mut distance = self.m.distance;
        if distance.abs() <= 0.0001 {
            distance = 0.0001;
        }
        // ControlsManager.js:110-112: the orthographic camera's distance.
        if self.m.ortho > 0.0 {
            distance = lerp(distance, distance.max(300.0), self.m.ortho.powi(8));
        }
        (angle, distance)
    }

    fn eye_position(&self) -> [f64; 3] {
        let (angle, distance) = self.rotatable();
        let v = tilt(
            [self.m.rotation.sin(), 0.0, -self.m.rotation.cos()],
            FRAC_PI_2 - angle,
        );
        [0, 1, 2].map(|i| self.m.position[i] - v[i] * distance)
    }

    // The camera's matrices for a frame of `aspect` (ControlsManager.js:114-139,
    // CombinedCamera.updateProjectionMatrix: BlueMap:src/js/util/CombinedCamera.js:77-130),
    // with player mode's view bob (web/bluemap-bridge.js:848-855).
    pub fn lens(&self, aspect: f64) -> Lens {
        let (angle, distance) = self.rotatable();
        let eye = self.eye_position();
        // three.js Matrix4.lookAt for a camera (Object3D.lookAt): z from the
        // target to the eye, x = up x z, nudged as three.js nudges it when z
        // is along up.
        let mut z = sub(eye, self.m.position);
        if dot(z, z) == 0.0 {
            z[2] = 1.0;
        }
        let mut z = normalize(z);
        let mut x = cross([0.0, 1.0, 0.0], z);
        if dot(x, x) == 0.0 {
            z[2] += 0.0001;
            z = normalize(z);
            x = cross([0.0, 1.0, 0.0], z);
        }
        let x = normalize(x);
        let y = cross(z, x);
        // The camera's world matrix, column-major.
        let mut world: Mat4 = [
            x[0], x[1], x[2], 0.0, y[0], y[1], y[2], 0.0, z[0], z[1], z[2], 0.0, eye[0], eye[1],
            eye[2], 1.0,
        ];
        // ControlsManager.js:124
        if self.m.tilt + angle < 0.0 {
            world = multiply(&world, &rot_z(PI));
        }
        if let (CameraMode::Player, Some(c)) = (self.mode, self.eye) {
            let t = self.partial(&c, self.now);
            let walk = -(c.walk + (c.walk - c.walk_o) * t);
            let bob = lerp(c.bob_o, c.bob, t);
            let w = walk * PI;
            world = multiply(
                &world,
                &rot_x(-((w - 0.2).cos() * bob).abs() * 5.0 * PI / 180.0),
            );
            world = multiply(&world, &rot_z(-w.sin() * bob * 3.0 * PI / 180.0));
            world = multiply(
                &world,
                &crate::camera::translate([-w.sin() * bob * 0.5, 0.0, 0.0]),
            );
            world = multiply(
                &world,
                &crate::camera::translate([0.0, (w.cos() * bob).abs(), 0.0]),
            );
        }
        let view = invert(&world);
        let fov = self.fov();
        let tan = (fov * PI / 180.0 * 0.5).tan();
        let ortho = self.m.ortho;
        let lens_eye = [world[12], world[13], world[14]];
        let rotation_only = {
            let mut r = view;
            r[12] = 0.0;
            r[13] = 0.0;
            r[14] = 0.0;
            r
        };
        // see_build's perspective lens (camera.rs) when the camera is not
        // orthographic at all: the shader's log depth needs no planes. The sky
        // of an orthographic lens is drawn through 70 degrees, as see_build's.
        let persp = crate::camera::perspective(
            fov * PI / 180.0,
            aspect,
            0.1,
            crate::camera::MAX_CAMERA_DISTANCE,
        );
        if ortho <= 0.0 {
            let sky_unproject = invert(&multiply(&persp, &rotation_only));
            return Lens {
                view,
                proj: persp,
                sky_unproject,
                linear: false,
                eye: lens_eye,
            };
        }
        let sky = crate::camera::perspective(
            70.0 * PI / 180.0,
            aspect,
            0.1,
            crate::camera::MAX_CAMERA_DISTANCE,
        );
        let sky_unproject = invert(&multiply(&sky, &rotation_only));
        // ControlsManager.js:133-139: the planes of an orthographic camera.
        let (near, far) = if self.m.angle == 0.0 {
            (1.0, distance + 300.0)
        } else {
            (1.0, 100000.0)
        };
        let top = near * tan;
        let p = frustum(
            -0.5 * aspect * 2.0 * top,
            0.5 * aspect * 2.0 * top,
            -top,
            top,
            near,
            far,
        );
        let normalized = -(ortho - 1.0).powi(6) + 1.0;
        let o_top = self.m.distance.max(0.0001) * tan;
        let o = crate::camera::ortho(
            -0.5 * aspect * 2.0 * o_top,
            0.5 * aspect * 2.0 * o_top,
            -o_top,
            o_top,
            near,
            far,
        );
        let mut proj = [0.0; 16];
        for i in 0..16 {
            proj[i] = p[i] * (1.0 - normalized) + o[i] * normalized;
        }
        Lens {
            view,
            proj,
            sky_unproject,
            linear: true,
            eye: lens_eye,
        }
    }

    // The camera as the seam describes it, for a frame of `aspect`.
    pub fn state(&self, aspect: f64) -> CameraState {
        CameraState {
            mode: self.mode,
            target: self.m.position,
            distance: self.m.distance,
            rotation: self.m.rotation,
            angle: self.m.angle,
            eye: self.lens(aspect).eye,
            v_fov: self.fov(),
            aspect,
        }
    }

    // The yaw a click uses, in degrees (web/bluemap-bridge.js:718-722): the
    // camera's heading, or the player's yaw.
    pub fn yaw(&self) -> f32 {
        match (self.mode, self.eye) {
            (CameraMode::Player, Some(c)) => self.look.map_or(c.view.y_rot, |l| l.0),
            _ => (self.m.rotation * 180.0 / PI - 180.0) as f32,
        }
    }
}

impl Default for Camera {
    fn default() -> Camera {
        Camera::new()
    }
}

// three.js Vector3.applyAxisAngle about (0, 1, 0) x v by `angle`
// (ControlsManager.js:115-117, web/bluemap-bridge.js:530-531).
fn tilt(v: [f64; 3], angle: f64) -> [f64; 3] {
    let axis = normalize(cross([0.0, 1.0, 0.0], v));
    let (s, c) = angle.sin_cos();
    // Rodrigues' rotation.
    let k = axis;
    let kv = cross(k, v);
    let kd = dot(k, v);
    [0, 1, 2].map(|i| v[i] * c + kv[i] * s + k[i] * kd * (1.0 - c))
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
fn normalize(a: [f64; 3]) -> [f64; 3] {
    let l = dot(a, a).sqrt();
    if l == 0.0 {
        a
    } else {
        a.map(|v| v / l)
    }
}

// three.js Object3D.rotateX and rotateZ: about the object's own axis.
fn rot_x(angle: f64) -> Mat4 {
    crate::camera::rotate_x(angle)
}
fn rot_z(angle: f64) -> Mat4 {
    let (s, c) = angle.sin_cos();
    let mut m = crate::camera::identity();
    m[0] = c;
    m[1] = s;
    m[4] = -s;
    m[5] = c;
    m
}

// three.js Matrix4.makePerspective(left, right, top, bottom, near, far).
fn frustum(left: f64, right: f64, bottom: f64, top: f64, near: f64, far: f64) -> Mat4 {
    let mut m = [0.0; 16];
    m[0] = 2.0 * near / (right - left);
    m[5] = 2.0 * near / (top - bottom);
    m[8] = (right + left) / (right - left);
    m[9] = (top + bottom) / (top - bottom);
    m[10] = -(far + near) / (far - near);
    m[11] = -1.0;
    m[14] = -2.0 * far * near / (far - near);
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn soft_clamps_are_blue_maps() {
        assert_eq!(soft_min(1.0, 5.0, 0.8), 1.0 + 4.0 * 0.8);
        assert_eq!(soft_max(10.0, 5.0, 0.8), 10.0 - 5.0 * 0.8);
        assert_eq!(soft_clamp(5.00005, 0.0, 5.0, 0.8), 5.0);
    }

    // The bridge's test (test/bluemap-bridge.test.js:560-581): a box from
    // BlueMap's start frames to x 7, angle 0.45, a distance within 12 and 40.
    #[test]
    fn a_fly_frames_a_build_as_the_bridge_does() {
        for ms in [0.0, 700.0] {
            let mut c = Camera::new();
            c.resize([1280, 800]);
            c.m.distance = 1500.0;
            c.m.rotation = 0.3;
            c.fly([-1, 64, -2], [14, 66, 2], ms, 0.0);
            c.update(ms + 1.0, &|_, _| None);
            assert!((c.m.position[0] - 7.0).abs() < 1e-9, "{:?}", c.m.position);
            assert!((c.m.angle - 0.45).abs() < 1e-9);
            assert!(
                c.m.distance > 12.0 && c.m.distance < 40.0,
                "{}",
                c.m.distance
            );
            assert!(c.animation.is_none());
        }
    }

    #[test]
    fn a_drag_moves_and_settles() {
        let mut c = Camera::new();
        c.resize([800, 600]);
        c.input(&CameraInput::PointerDown, 0.0, None, &|_, _| None);
        c.input(
            &CameraInput::Drag {
                button: DragButton::Left,
                dx: 40.0,
                dy: 0.0,
            },
            0.0,
            None,
            &|_, _| None,
        );
        let start = c.m.position;
        let mut t = 0.0;
        while c.moving(t) {
            t += 16.666;
            c.update(t, &|_, _| None);
            assert!(t < 10_000.0, "settles");
        }
        assert!(
            c.m.position[0] < start[0],
            "a drag to the right pans the target left: {:?}",
            c.m.position
        );
    }

    // BlueMap's MouseMoveControls.js:106 and MouseRotateControls.js:98-99:
    // Ctrl with the left button pans and rotates, Alt only rotates.
    #[test]
    fn ctrl_and_alt_with_the_left_button_follow_blue_map() {
        for (button, pans) in [(DragButton::CtrlLeft, true), (DragButton::AltLeft, false)] {
            let mut c = Camera::new();
            c.resize([800, 600]);
            c.input(&CameraInput::PointerDown, 0.0, None, &|_, _| None);
            c.input(
                &CameraInput::Drag {
                    button,
                    dx: 40.0,
                    dy: 0.0,
                },
                0.0,
                None,
                &|_, _| None,
            );
            assert_eq!(c.mouse_move != [0.0; 2], pans, "{button:?}");
            assert!(c.mouse_rotate != 0.0, "{button:?} rotates");
        }
    }

    // BlueMap's MouseRotateControls.js and MouseAngleControls.js:112-113:
    // under the pointer lock the movement turns the free-flying camera at
    // speedCapture over the height; the map view takes no locked look.
    #[test]
    fn a_locked_look_turns_the_free_flying_camera_at_blue_maps_captured_speed() {
        let mut c = Camera::new();
        c.resize([800, 600]);
        c.input(
            &CameraInput::Mode(CameraMode::Spectator),
            0.0,
            None,
            &|_, _| None,
        );
        // The switch is BlueMap's 500 ms transition (setFreeFlight(500)), which
        // holds the controls until it ends.
        let mut t = 0.0;
        while c.held() {
            t += 16.666;
            c.update(t, &|_, _| None);
            assert!(t < 2_000.0, "the transition ends");
        }
        c.input(
            &CameraInput::LockedLook {
                dx: 30.0,
                dy: -12.0,
            },
            t,
            None,
            &|_, _| None,
        );
        let per_pixel = 1.0 / 600.0;
        assert_eq!(c.free_rotate, -(30.0 * FREE_LOOK_CAPTURE * per_pixel));
        assert_eq!(c.free_angle, -12.0 * FREE_LOOK_CAPTURE * per_pixel);
        let mut m = Camera::new();
        m.resize([800, 600]);
        m.input(
            &CameraInput::LockedLook {
                dx: 30.0,
                dy: -12.0,
            },
            0.0,
            None,
            &|_, _| None,
        );
        assert_eq!(
            (m.free_rotate, m.free_angle, m.mouse_rotate),
            (0.0, 0.0, 0.0)
        );
    }

    #[test]
    fn player_mode_gives_back_the_map_view() {
        let mut c = Camera::new();
        c.m.rotation = 0.7;
        c.m.distance = 80.0;
        c.input(
            &CameraInput::Mode(CameraMode::Player),
            0.0,
            None,
            &|_, _| None,
        );
        let saved = c.m;
        c.m.distance = 3.0;
        c.input(
            &CameraInput::Mode(CameraMode::Perspective),
            10.0,
            None,
            &|_, _| None,
        );
        assert_eq!(c.m, saved);
        assert_eq!(c.mode, CameraMode::Perspective);
    }
}
