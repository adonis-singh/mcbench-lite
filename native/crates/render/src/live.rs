//! The live view's seam: the window's handle on the 3D pane ([`LivePane`]),
//! the frames it takes ([`Frame`]), the camera's inputs ([`CameraInput`]),
//! what the window asks to be drawn over the world ([`Overlays`]), the
//! answer to a click ([`Pick`]), and what the core feeds the kept world
//! ([`Shown`], [`LivePatch`]). Serde only, no `wgpu`: `mcbench-ui` depends on
//! this crate for these types and on nothing of the drawing crate.
//!
//! Agreed between the renderer session and the interface session on
//! 2026-11-17 (`ui-seam-viewer.md` and the renderer's answers, in the
//! sessions' shared notes). The renderer owns these types.
//!
//! ## How a frame reaches the screen
//!
//! The renderer decides when the view changed (the camera moving or settling,
//! a fly, a patch applied, entities gliding, the player's eye between ticks,
//! the size, the overlays, the time of day, visibility coming back). It
//! starts a draw when the view changed, no draw of its own is on the GPU and
//! the slot is empty. A delivered frame fills the one slot (a newer one
//! replaces a frame not yet taken) and calls the waker. The window's paint
//! calls [`LivePane::take`], which empties the slot and so lets the next draw
//! start. The renderer therefore never draws faster than the display shows,
//! and an idle pane draws nothing and wakes nothing.
//!
//! ## Coordinates
//!
//! Pointer positions and frame sizes are physical pixels from the pane's top
//! left corner. Blocks are Minecraft's coordinates, the engine's (the page's
//! `y_offset` included).
//!
//! ## Day and night
//!
//! [`TimeOfDay::Day`] is `see_build`'s shading (the plan's Renderer section,
//! change 8), drawn from the mesher's sky and block light. [`TimeOfDay::Night`]
//! is that shading times Minecraft's own night. Each corner's colour is
//! multiplied per channel by Minecraft's lightmap at the level's time 18000
//! over its lightmap at 6000, for the corner's sky and block light. The
//! lightmap is a 16 × 16 texture that the client's shader `core/lightmap.fsh`
//! fills from the uniforms `LightTexture.updateLightTexture` computes. It is
//! sampled as the client's terrain samples it: bilinearly, at light/16 plus
//! half a texel. The night sky is drawn as the client draws it, not as a
//! ratio of the day's. The sky disc has the client's sky colour at 18000
//! (`ClientLevel.getSkyColor`), which is black in every overworld biome, and
//! fades by distance toward the fog colour at 18000 (`FogRenderer`'s
//! `computeFogColor`). Over it come the stars (`SkyRenderer.buildStars`,
//! seed 10842) and the moon (`SkyRenderer.MOON_LOCATION`,
//! `textures/environment/moon_phases.png` read from the client jar, phase 0
//! at 18000 of day 0). The fog over the world is that colour too. The values
//! come from running Minecraft's client at release time (`night.json` from
//! the extractor's night step, `server/McbenchNight.java`); each value's source
//! class and method is named where the drawing crate reads it. `see_build`'s
//! pictures always draw
//! [`TimeOfDay::Day`].

use crate::Clip;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Which world the pane shows: today's key, `"<chat>"` or
/// `"bench:<bench>:<round>"`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ViewKey(pub String);

/// The pane's size in physical pixels and the display's scale factor.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PaneSize {
    /// Width in physical pixels.
    pub width: u32,
    /// Height in physical pixels.
    pub height: u32,
    /// Physical pixels per logical pixel.
    pub scale: f32,
}

/// The live view's time of day (see the module's Day and night).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TimeOfDay {
    /// `see_build`'s shading; the pane starts in it.
    #[default]
    Day,
    /// `see_build`'s shading times Minecraft's own night.
    Night,
}

/// How the camera projects.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CameraMode {
    /// The orbiting map camera, in perspective.
    Perspective,
    /// The orbiting map camera, looking straight down, orthographic.
    Flat,
    /// The free-flying camera.
    Spectator,
    /// The player's eye, from each tick's [`PlayerView`].
    Player,
}

/// The camera as it was for one frame.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct CameraState {
    /// The mode.
    pub mode: CameraMode,
    /// The point the map camera orbits, in Minecraft's coordinates.
    pub target: [f64; 3],
    /// The eye's distance from the target.
    pub distance: f64,
    /// The heading, in radians.
    pub rotation: f64,
    /// The tilt from straight down, in radians.
    pub angle: f64,
    /// The eye, in Minecraft's coordinates.
    pub eye: [f64; 3],
    /// The vertical field of view, in degrees.
    pub v_fov: f64,
    /// Width over height.
    pub aspect: f64,
}

/// Which button drags, with which modifiers (BlueMap's map controls tell
/// Ctrl from Alt: `MouseMoveControls` pans with the left button unless Alt
/// is held, `MouseRotateControls` and `MouseAngleControls` rotate and tilt
/// with the right, or with the left and Alt or Ctrl).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DragButton {
    /// The left button: pans.
    Left,
    /// The right button: rotates and tilts.
    Right,
    /// Ctrl (without Alt) with the left button: pans, rotates and tilts.
    CtrlLeft,
    /// Alt (with or without Ctrl) with the left button: rotates and tilts.
    AltLeft,
}

/// The camera keys held, one bit each (the [`keys`] constants). The window
/// decides which key events are the camera's (exact modifiers only, released
/// on blur) and sends the whole held set at every change.
pub mod keys {
    /// W or the up arrow.
    pub const FORWARD: u16 = 1 << 0;
    /// S or the down arrow.
    pub const BACKWARD: u16 = 1 << 1;
    /// A or the left arrow.
    pub const LEFT: u16 = 1 << 2;
    /// D or the right arrow.
    pub const RIGHT: u16 = 1 << 3;
    /// Alt with PageUp.
    pub const TILT_UP: u16 = 1 << 4;
    /// Alt with PageDown.
    pub const TILT_DOWN: u16 = 1 << 5;
    /// Alt with Delete.
    pub const ROTATE_LEFT: u16 = 1 << 6;
    /// Alt with End.
    pub const ROTATE_RIGHT: u16 = 1 << 7;
    /// NumpadAdd or Insert.
    pub const ZOOM_IN: u16 = 1 << 8;
    /// NumpadSubtract or Home.
    pub const ZOOM_OUT: u16 = 1 << 9;
    /// Space, in spectator mode.
    pub const UP: u16 = 1 << 10;
    /// Shift, in spectator mode.
    pub const DOWN: u16 = 1 << 11;
}

/// One input to the camera, applied in the order sent. Speeds, stiffness,
/// inertia and clamps are the renderer's, ported from BlueMap 5.22's
/// `MapControls` and `FreeFlightControls`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum CameraInput {
    /// A button went down on the pane: a fly in progress stops.
    PointerDown,
    /// A drag, in physical pixels since the last move.
    Drag {
        /// The button.
        button: DragButton,
        /// Horizontal movement.
        dx: f32,
        /// Vertical movement.
        dy: f32,
    },
    /// The wheel or a trackpad's scroll.
    Wheel {
        /// The amount; positive zooms out.
        delta: f32,
        /// Lines (a wheel's notches) rather than pixels (a trackpad).
        lines: bool,
    },
    /// A trackpad's pinch, the scale since the last one.
    Pinch {
        /// Above 1 zooms in.
        scale: f32,
    },
    /// A trackpad's twist, since the last one.
    Twist {
        /// Clockwise in radians.
        radians: f32,
    },
    /// The zoom buttons: steps of BlueMap's zoom, negative zooms in.
    ZoomStep(i8),
    /// The camera keys held now (a [`keys`] bit each); an empty set on blur.
    Keys(u16),
    /// A double click: zooms toward the point under it in frame `seq`.
    DoubleClick {
        /// The frame the person saw.
        seq: u64,
        /// Physical pixels in that frame.
        at: [f32; 2],
    },
    /// Fly to frame a box in `ms` (0 for at once).
    Frame {
        /// The lowest corner, Minecraft's coordinates, included.
        min: [i32; 3],
        /// The highest corner, included.
        max: [i32; 3],
        /// The fly's length in milliseconds.
        ms: u32,
    },
    /// The camera's starting view of the shown world.
    Reset,
    /// Heading north, keeping the rest.
    FaceNorth,
    /// Move the map camera's target to x and z.
    Target {
        /// x.
        x: f64,
        /// z.
        z: f64,
    },
    /// Change the mode; leaving `Player` restores the map view it left.
    Mode(CameraMode),
    /// Player mode's look, at once, in degrees (Minecraft's yaw and pitch).
    Look {
        /// Yaw.
        yaw: f32,
        /// Pitch.
        pitch: f32,
    },
    /// Spectator mode under the pointer lock: the pointer's movement since
    /// the last, in physical pixels as `Drag`'s are, turning the free-flying
    /// camera at BlueMap's captured speed (FreeFlightControls' mouse
    /// controls under `document.pointerLockElement`). Ignored in other modes.
    LockedLook {
        /// Horizontal movement.
        dx: f32,
        /// Vertical movement.
        dy: f32,
    },
}

/// A box drawn over the world.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct OverlayBox {
    /// The lowest corner, Minecraft's coordinates, included.
    pub min: [i32; 3],
    /// The highest corner, included.
    pub max: [i32; 3],
    /// The fill's opacity, 0 for none (0.22 while its chip is hovered).
    pub fill: f32,
    /// Whether its edges are drawn.
    pub outline: bool,
}

/// What the window asks to be drawn over the world; the latest wins.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Overlays {
    /// Points whose projections each [`Frame`] carries, in this order
    /// (labels the window draws over the picture).
    pub anchors: Vec<[f64; 3]>,
    /// Boxes, in this order.
    pub boxes: Vec<OverlayBox>,
    /// The block the person clicked, marked with a white cube.
    pub marked: Option<[i32; 3]>,
}

/// The answer to [`LivePane::pick`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Pick {
    /// The frame it was cast in.
    pub seq: u64,
    /// The world that frame showed.
    pub view: ViewKey,
    /// The block hit, none for the sky. Translucent quads (water, glass,
    /// ice) and overlays pass through. A `seq` older than the renderer's last
    /// three delivered frames answers none.
    pub block: Option<[i32; 3]>,
    /// The hit face's outward normal.
    pub face: Option<[i8; 3]>,
    /// The yaw a click uses, in degrees: the camera's heading
    /// (`rotation·180/π − 180`), or the player's yaw in player mode.
    pub yaw: f32,
}

/// Whether the pane draws.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PaneStatus {
    /// It draws when the view changes.
    Drawing,
    /// It stopped, for the reason given as a plain sentence the window shows
    /// in its cover: a second lost device, a patch the world refused, a world
    /// that does not fit the renderer's memory limit.
    Failed(String),
}

/// Facts about how the pane draws on this machine.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LiveFacts {
    /// On a software adapter, the sentence the window shows once in the
    /// pane's bar: the processor draws the view at half the pane's width and
    /// height and without smoothed edges, and [`Frame::size`] says so.
    pub floor: Option<String>,
}

/// One drawn picture of the pane.
#[derive(Debug)]
pub struct Frame {
    /// The pane's own counter; a pick names the frame under the pointer.
    pub seq: u64,
    /// The world it shows.
    pub view: ViewKey,
    /// The world's revision it shows.
    pub revision: u64,
    /// Its size in physical pixels: the pane's, or half its width and height
    /// on a software adapter (the window scales it up to the pane).
    pub size: [u32; 2],
    /// [`Overlays::anchors`] projected into this frame, in physical pixels;
    /// none for a point behind the eye.
    pub anchors: Vec<Option<[f32; 2]>>,
    /// The camera it was drawn with.
    pub camera: CameraState,
    /// The time of day it was drawn in.
    pub time: TimeOfDay,
    bgra: Vec<u8>,
}

impl Frame {
    /// A frame from its rows of `Bgra8Unorm` pixels, top row first, each row
    /// `size[0] * 4` bytes with no padding.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        seq: u64,
        view: ViewKey,
        revision: u64,
        size: [u32; 2],
        anchors: Vec<Option<[f32; 2]>>,
        camera: CameraState,
        time: TimeOfDay,
        bgra: Vec<u8>,
    ) -> Frame {
        assert_eq!(
            bgra.len(),
            size[0] as usize * size[1] as usize * 4,
            "a frame's pixels are its width times its height times 4 bytes"
        );
        Frame {
            seq,
            view,
            revision,
            size,
            anchors,
            camera,
            time,
            bgra,
        }
    }

    /// The pixels, `Bgra8Unorm`, top row first, rows unpadded. Read by one
    /// function of the window, so a shared texture can replace the bytes
    /// later without the seam changing.
    pub fn bgra(&self) -> &[u8] {
        &self.bgra
    }
}

/// The window's handle on the 3D pane. Every method returns at once on the
/// interface thread; the work happens on the render thread.
pub trait LivePane: Send + Sync {
    /// Called on the render thread for each delivered frame and each change
    /// of [`LivePane::status`].
    fn set_waker(&self, wake: Box<dyn Fn() + Send + Sync>);
    /// The pane's size; the latest wins.
    fn resize(&self, size: PaneSize);
    /// Whether the pane is on screen; a hidden pane draws nothing, and the
    /// world's patches are still applied.
    fn visible(&self, visible: bool);
    /// One camera input, applied in order.
    fn camera(&self, input: CameraInput);
    /// What is drawn over the world; the latest wins.
    fn overlays(&self, overlays: Overlays);
    /// Day or night; the latest wins, and the next frame is drawn in it.
    fn time_of_day(&self, time: TimeOfDay);
    /// The block under `at` (physical pixels) in frame `seq`, cast with that
    /// frame's camera on the GPU; `answer` runs on the render thread within
    /// one draw.
    fn pick(&self, seq: u64, at: [f32; 2], answer: Box<dyn FnOnce(Pick) + Send>);
    /// The newest delivered frame, emptying the slot; also the signal that
    /// lets the next draw start.
    fn take(&self) -> Option<Frame>;
    /// Whether the pane draws.
    fn status(&self) -> PaneStatus;
    /// The camera of the newest delivered frame.
    fn camera_state(&self) -> CameraState;
    /// How the pane draws on this machine.
    fn facts(&self) -> LiveFacts;
}

/// How a shown world is framed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Framing {
    /// At once.
    AtOnce,
    /// Flown to in this many milliseconds (a turn's new revision: 700).
    Fly(u32),
    /// The camera stays.
    Keep,
}

/// A world the live view shows: one the core has kept with
/// [`crate::Renderer::world`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Shown {
    /// The kept world's name (`live:<view>`).
    pub world: String,
    /// The view it is.
    pub view: ViewKey,
    /// Its revision.
    pub revision: u64,
    /// The box of its blocks, which framing fits.
    pub bounds: Clip,
    /// How it is framed.
    pub framing: Framing,
}

/// The player's view after a tick: `core/src/player/mod.rs` `View`, each
/// field named as there, positions in Minecraft's coordinates and angles in
/// degrees.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlayerView {
    /// The feet.
    pub pos: [f64; 3],
    /// The feet at the tick before.
    pub old_pos: [f64; 3],
    /// The motion.
    pub delta: [f64; 3],
    /// Yaw.
    pub y_rot: f32,
    /// Pitch.
    pub x_rot: f32,
    /// Yaw at the tick before.
    pub y_rot_o: f32,
    /// Pitch at the tick before.
    pub x_rot_o: f32,
    /// The box's width.
    pub width: f32,
    /// The box's height.
    pub height: f32,
    /// The eye above the feet.
    pub eye_height: f32,
    /// 0 standing, 1 crouching, 2 swimming.
    pub pose: u8,
    /// Flying.
    pub flying: bool,
    /// Sprinting.
    pub sprinting: bool,
    /// Crouching.
    pub crouching: bool,
    /// Swimming.
    pub swimming: bool,
    /// On the ground.
    pub on_ground: bool,
    /// In water.
    pub in_water: bool,
    /// In lava.
    pub in_lava: bool,
    /// The eye in water.
    pub eye_in_water: bool,
    /// The eye in lava.
    pub eye_in_lava: bool,
    /// Pushing against a wall.
    pub horizontal_collision: bool,
    /// How far it has fallen.
    pub fall_distance: f64,
}

impl PlayerView {
    /// From the 28 numbers of `View::encode`, in its order; fails on a pose
    /// or flag that is not one of its values.
    pub fn decode(n: &[f64; 28]) -> Result<PlayerView, String> {
        let flag = |i: usize| match n[i] {
            0.0 => Ok(false),
            1.0 => Ok(true),
            v => Err(format!(
                "The player view's number {i} is {v}, not a flag (0 or 1)"
            )),
        };
        let pose = match n[16] {
            0.0 => 0,
            1.0 => 1,
            2.0 => 2,
            v => return Err(format!("The player view's pose is {v}, not 0, 1 or 2")),
        };
        Ok(PlayerView {
            pos: [n[0], n[1], n[2]],
            old_pos: [n[3], n[4], n[5]],
            delta: [n[6], n[7], n[8]],
            y_rot: n[9] as f32,
            x_rot: n[10] as f32,
            y_rot_o: n[11] as f32,
            x_rot_o: n[12] as f32,
            width: n[13] as f32,
            height: n[14] as f32,
            eye_height: n[15] as f32,
            pose,
            flying: flag(17)?,
            sprinting: flag(18)?,
            crouching: flag(19)?,
            swimming: flag(20)?,
            on_ground: flag(21)?,
            in_water: flag(22)?,
            in_lava: flag(23)?,
            eye_in_water: flag(24)?,
            eye_in_lava: flag(25)?,
            horizontal_collision: flag(26)?,
            fall_distance: n[27],
        })
    }
}

/// A tick's or an edit's changes to the shown world. The renderer applies
/// every patch in order, all those waiting before a draw, and never drops
/// one.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LivePatch {
    /// The view it changes.
    pub view: ViewKey,
    /// The world's revision.
    pub revision: u64,
    /// The engine's tick after it.
    pub tick: u64,
    /// The engine's `Ticked.changes` as they are (the live contract's patch:
    /// the palette entries it adds, the bounds and built-column count after
    /// it, the blocks that changed), read by the mesher's `World::patch`,
    /// which re-meshes only the tiles that can draw differently.
    pub changes: Vec<u8>,
    /// The engine's `Ticked.entities` as they are.
    pub entities: Vec<u8>,
    /// The player's view after the tick, in player mode.
    pub player: Option<PlayerView>,
    /// A new framing, when the change is a turn's new revision.
    pub framing: Option<Framing>,
}

/// The live view's side of [`crate::Renderer`], which the core calls; kept
/// apart so the core's fakes and the window's fakes write only what they
/// use.
pub trait LiveRenderer: Send + Sync {
    /// The one pane, made by `start`.
    fn live(&self) -> Arc<dyn LivePane>;
    /// Shows a kept world, or nothing; returns at once.
    fn live_show(&self, shown: Option<Shown>);
    /// One patch of the shown world, in order, never dropped; returns at
    /// once. A patch the world refuses stops the pane with its sentence.
    fn live_patch(&self, patch: LivePatch);
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakePane;
    impl LivePane for FakePane {
        fn set_waker(&self, _: Box<dyn Fn() + Send + Sync>) {}
        fn resize(&self, _: PaneSize) {}
        fn visible(&self, _: bool) {}
        fn camera(&self, _: CameraInput) {}
        fn overlays(&self, _: Overlays) {}
        fn time_of_day(&self, _: TimeOfDay) {}
        fn pick(&self, _: u64, _: [f32; 2], _: Box<dyn FnOnce(Pick) + Send>) {}
        fn take(&self) -> Option<Frame> {
            None
        }
        fn status(&self) -> PaneStatus {
            PaneStatus::Drawing
        }
        fn camera_state(&self) -> CameraState {
            unimplemented!("a fake for the seam test")
        }
        fn facts(&self) -> LiveFacts {
            LiveFacts::default()
        }
    }

    #[test]
    fn a_pane_can_be_held_as_a_shared_handle() {
        let pane: Arc<dyn LivePane> = Arc::new(FakePane);
        assert!(pane.take().is_none());
        assert_eq!(pane.status(), PaneStatus::Drawing);
    }

    #[test]
    fn the_camera_keys_are_distinct_bits() {
        let all = [
            keys::FORWARD,
            keys::BACKWARD,
            keys::LEFT,
            keys::RIGHT,
            keys::TILT_UP,
            keys::TILT_DOWN,
            keys::ROTATE_LEFT,
            keys::ROTATE_RIGHT,
            keys::ZOOM_IN,
            keys::ZOOM_OUT,
            keys::UP,
            keys::DOWN,
        ];
        let mut seen = 0u16;
        for bit in all {
            assert_eq!(bit.count_ones(), 1);
            assert_eq!(seen & bit, 0);
            seen |= bit;
        }
    }

    #[test]
    fn a_player_view_decodes_the_engines_28_numbers_in_order() {
        let mut n = [0.0; 28];
        for (i, v) in n.iter_mut().enumerate().take(16) {
            *v = i as f64 + 0.5;
        }
        n[16] = 1.0;
        n[17] = 1.0;
        n[21] = 1.0;
        n[27] = 3.25;
        let view = PlayerView::decode(&n).unwrap();
        assert_eq!(view.pos, [0.5, 1.5, 2.5]);
        assert_eq!(view.old_pos, [3.5, 4.5, 5.5]);
        assert_eq!(view.delta, [6.5, 7.5, 8.5]);
        assert_eq!(
            (view.y_rot, view.x_rot, view.y_rot_o, view.x_rot_o),
            (9.5, 10.5, 11.5, 12.5)
        );
        assert_eq!(
            (view.width, view.height, view.eye_height),
            (13.5, 14.5, 15.5)
        );
        assert_eq!(view.pose, 1);
        assert!(view.flying && view.on_ground && !view.sprinting);
        assert_eq!(view.fall_distance, 3.25);
    }

    #[test]
    fn a_player_view_with_a_flag_that_is_not_0_or_1_is_refused() {
        let mut n = [0.0; 28];
        n[20] = 0.5;
        assert!(PlayerView::decode(&n).unwrap_err().contains("number 20"));
    }

    #[test]
    fn a_frame_holds_exactly_its_pixels() {
        let camera = CameraState {
            mode: CameraMode::Perspective,
            target: [0.0; 3],
            distance: 10.0,
            rotation: 0.0,
            angle: 0.5,
            eye: [0.0, 10.0, 0.0],
            v_fov: 70.0,
            aspect: 2.0,
        };
        let frame = Frame::new(
            1,
            ViewKey("c".into()),
            0,
            [2, 1],
            Vec::new(),
            camera,
            TimeOfDay::Night,
            vec![0; 8],
        );
        assert_eq!(frame.bgra().len(), 8);
        assert_eq!(frame.time, TimeOfDay::Night);
    }

    #[test]
    #[should_panic(expected = "width times its height")]
    fn a_frame_with_the_wrong_number_of_bytes_is_a_bug() {
        let camera = CameraState {
            mode: CameraMode::Flat,
            target: [0.0; 3],
            distance: 1.0,
            rotation: 0.0,
            angle: 0.0,
            eye: [0.0; 3],
            v_fov: 70.0,
            aspect: 1.0,
        };
        Frame::new(
            1,
            ViewKey("c".into()),
            0,
            [2, 2],
            Vec::new(),
            camera,
            TimeOfDay::Day,
            vec![0; 8],
        );
    }
}
