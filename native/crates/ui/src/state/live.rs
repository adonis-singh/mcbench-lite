//! The 3D pane's pure rules (pane agent), no GPUI types: what the cover,
//! the banner and the toolbar say for a live state (`style-viewer-routes.md`
//! 2.3, 2.5, 2.6), the click gesture (2.13), the camera keys and the
//! player's keys by mode with their exact modifiers (2.8, 2.11, 2.12,
//! 2.16), the next selection letter (2.14), a bench comparison's labels
//! (2.15), the scale a frame is drawn at (the seam's resize rule), and the
//! pane's accessible description (the notes' Accessibility).

use crate::{LivePhase, LiveState, PaneStatus, Running, ViewKey, camera_keys as keys};

/// The cover's text for a chat's world being placed.
pub const PREPARING_CHAT: &str = "Preparing this chat\u{2019}s world\u{2026}";
/// The cover while the pane itself is not there yet (the page's viewer not
/// booted, `web/app.js` 3312).
pub const PREPARING_LIVE: &str = "Preparing the live world\u{2026}";
/// The cover's text for a bench round's world.
pub const PLACING_BUILDS: &str = "Placing every build\u{2026}";
/// Build mode's empty text while setup prepares.
pub const EMPTY_PREPARING: &str = "The world shows here once the app is ready.";
/// Build mode's empty text.
pub const EMPTY_BUILD: &str = "Open a chat to view its world.";
/// The toolbar's name.
pub const TOOLBAR: &str = "Live world";
/// The pane's accessible name.
pub const PANE_NAME: &str = "Live Minecraft world";
/// The BlueMap control bar's name (`web/bluemap-bridge.js` 102).
pub const WORLD_CONTROLS: &str = "mcbench world controls";
/// The tick counter's title.
pub const TICKS_TITLE: &str = "Ticks since the build was placed";
/// Step's title.
pub const STEP: &str = "Step one tick";
/// Restart's title.
pub const RESTART: &str = "Restart from the build";
/// Fill's title, beside the chat.
pub const FILL: &str = "Fill the window";
/// Fill's title, filling the window.
pub const BACK: &str = "Back beside the chat (Esc)";
/// The selection tool's title.
pub const SELECT_TITLE: &str =
    "Select blocks to refer to in your message (Esc to stop, right-click a box to remove it)";
/// The selection tool's name.
pub const SELECT: &str = "Select blocks";
/// The schematic download's title.
pub const DOWNLOAD: &str = "Download as a WorldEdit schematic (.schem)";
/// The world export's title.
pub const EXPORT: &str = "Export as a Minecraft world (a zip to unpack in .minecraft/saves)";
/// Player mode's button.
pub const PLAYER: &str = "Player mode: walk and fly as a Minecraft player";
/// Spectator's button.
pub const SPECTATOR: &str = "Spectator: fly through everything";
/// Perspective's button (BlueMap's `Perspective-View`).
pub const PERSPECTIVE: &str = "Perspective-View";
/// Flat's button.
pub const FLAT: &str = "Orthographic / Flat-View";
/// Reset's button.
pub const RESET: &str = "Reset Camera & Position";
/// The compass.
pub const COMPASS: &str = "Compass / Face North";
/// The Day/Night switch's title.
pub const DAY_NIGHT: &str = "Day/Night";
/// The zoom buttons' names (BlueMap's have none; the window's need one).
pub const ZOOM_IN: &str = "Zoom in";
/// The zoom out button.
pub const ZOOM_OUT: &str = "Zoom out";
/// The hint of player mode before the pointer is taken.
pub const HINT_FREE: &str = "Click to look around";
/// The hint of player mode while the pointer is taken.
pub const HINT_TAKEN: &str = "WASD move \u{b7} Space jump (twice: fly) \u{b7} Shift sneak \u{b7} Ctrl sprint \u{b7} Right-click use \u{b7} Esc frees the mouse";
/// A note shown when player mode is asked of a world that does not run.
pub const NEEDS_RUNNING: &str = "Player mode needs a running world.";
/// The note when the world let the player go (`web/bluemap-bridge.js` 203).
pub const PLAYER_GONE: &str = "The player is no longer in the world.";
/// The note when another world came on show before the player stood
/// (`web/app.js` 3198).
pub const ANOTHER_WORLD: &str = "Another world came on show before the player stood in this one";
/// How long a player note shows, ms.
pub const NOTE_MS: u64 = 5000;
/// The block label's fade in and out (`PopupMarker.js`: `300`).
pub const POPUP_FADE_MS: u64 = 300;
/// The section's tooltip beside the chat.
pub const EXPAND: &str = "Double-click to expand";
/// The section's tooltip filling the window.
pub const EXIT_FULL: &str = "Double-click or press Esc to exit fullscreen";

/// A button the cover or the banner offers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Offer {
    /// `Try again`: the view placed afresh.
    TryAgain,
}

/// What the pane shows around the world.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chrome {
    /// The cover over the world (`role="status"`), with its button.
    pub cover: Option<(String, Option<Offer>)>,
    /// The banner along the bottom (`role="status"`), with its button.
    pub banner: Option<(String, Option<Offer>)>,
    /// The toolbar: whether it shows.
    pub toolbar: bool,
}

/// Everything the window knows that decides the chrome.
pub struct Seen<'a> {
    /// The pane's handle failed to start with this sentence.
    pub pane_error: Option<&'a str>,
    /// The pane's status.
    pub status: Option<&'a PaneStatus>,
    /// The view the window shows (its place's).
    pub view: Option<&'a ViewKey>,
    /// The core's live state.
    pub state: Option<&'a LiveState>,
    /// The view and revision the last painted frame showed.
    pub painted: Option<(&'a ViewKey, u64)>,
    /// Bench mode (no empty text).
    pub bench_mode: bool,
    /// Setup is not ready yet.
    pub preparing: bool,
    /// The pane's handle has come (`Update::LivePane(Ok)`).
    pub has_pane: bool,
}

/// The cover, the banner and whether the toolbar shows
/// (`web/app.js` 3305-3369).
pub fn chrome(seen: &Seen) -> Chrome {
    let empty = || {
        (!seen.bench_mode).then(|| {
            let text = if seen.preparing {
                EMPTY_PREPARING
            } else {
                EMPTY_BUILD
            };
            (text.to_string(), None)
        })
    };
    if let Some(error) = seen.pane_error {
        return Chrome {
            cover: Some((error.to_string(), None)),
            banner: None,
            toolbar: false,
        };
    }
    if let Some(PaneStatus::Failed(error)) = seen.status {
        return Chrome {
            cover: Some((error.clone(), None)),
            banner: None,
            toolbar: false,
        };
    }
    let Some(view) = seen.view else {
        return Chrome {
            cover: empty(),
            banner: None,
            toolbar: false,
        };
    };
    // No pane yet: the page's viewer not booted (3308-3316).
    if !seen.has_pane {
        return Chrome {
            cover: Some((PREPARING_LIVE.to_string(), None)),
            banner: None,
            toolbar: false,
        };
    }
    // The core's state for another view (one asked for and not answered
    // yet) is not this view's.
    let state = seen.state.filter(|s| s.view.as_ref() == Some(view));
    let banner = state.and_then(|s| match (&s.phase, &s.running) {
        (
            LivePhase::Failed {
                error,
                nothing_shown,
            },
            _,
        ) => Some((error.clone(), nothing_shown.then_some(Offer::TryAgain))),
        (_, Some(Running::Stopped(error))) => Some((error.clone(), None)),
        _ => None,
    });
    // The cover comes off when a frame of this view, at the revision the
    // core shows or a later one, has been painted; a newer revision of the
    // same view swaps in uncovered.
    let painted = seen.painted.is_some_and(|(v, _)| v == view);
    let shown = state.is_some_and(|s| s.phase == LivePhase::Shown);
    let cover = (!painted).then(|| {
        let text = if view.0.starts_with("bench:") {
            PLACING_BUILDS
        } else {
            PREPARING_CHAT
        };
        (text.to_string(), None)
    });
    Chrome {
        cover,
        banner,
        toolbar: painted && shown && state.is_some_and(|s| s.running.is_some()),
    }
}

/// The toolbar's state: Play or Pause, and which are enabled.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Toolbar {
    /// Playing (the first button pauses).
    pub playing: bool,
    /// The first button is enabled (not stopped).
    pub play_enabled: bool,
    /// Step is enabled (paused, not stopped).
    pub step_enabled: bool,
    /// The counter's text (`1,234 ticks`).
    pub ticks: String,
}

/// The toolbar for a state (`web/app.js` 3377-3393): Restart is always
/// enabled, as today.
pub fn toolbar(state: &LiveState) -> Toolbar {
    let stopped = matches!(state.running, Some(Running::Stopped(_)));
    let playing = state.running == Some(Running::Playing);
    Toolbar {
        playing,
        play_enabled: !stopped,
        step_enabled: !stopped && !playing,
        ticks: format!("{} ticks", grouped(state.tick)),
    }
}

/// A count in the page's locale format (`toLocaleString()`, en: commas).
pub fn grouped(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// The pane's accessible description (`Castle gate, 42,862 blocks,
/// playing`); the tick count is not in it, so ticks announce nothing.
pub fn description(title: &str, state: Option<&LiveState>, cover: Option<&str>) -> String {
    if let Some(cover) = cover {
        return cover.to_string();
    }
    let Some(state) = state else {
        return title.to_string();
    };
    let running = match &state.running {
        Some(Running::Playing) => "playing",
        Some(Running::Paused) => "paused",
        Some(Running::Stopped(_)) => "stopped",
        None => "not running",
    };
    format!("{title}, {} blocks, {running}", grouped(state.blocks))
}

/// A click: the button went down and up within 500 ms and 4 px
/// (`web/bluemap-bridge.js` 1110-1125).
pub fn is_click(down_ms: u64, up_ms: u64, down: [f32; 2], up: [f32; 2]) -> bool {
    up_ms.saturating_sub(down_ms) <= 500 && (up[0] - down[0]).hypot(up[1] - down[1]) <= 4.0
}

/// A right click that removes a box: moved at most 4 px.
pub fn is_right_click(down: [f32; 2], up: [f32; 2]) -> bool {
    (up[0] - down[0]).hypot(up[1] - down[1]) <= 4.0
}

/// The modifiers of a key press.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mods {
    /// Ctrl.
    pub control: bool,
    /// Alt (Option).
    pub alt: bool,
    /// Shift.
    pub shift: bool,
    /// Cmd or the Windows key.
    pub platform: bool,
}

impl Mods {
    fn none(self) -> bool {
        self == Mods::default()
    }

    fn alt_only(self) -> bool {
        self == Mods {
            alt: true,
            ..Mods::default()
        }
    }
}

/// How the camera moves now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CameraKind {
    /// Perspective or flat (BlueMap's `MapControls`).
    Map,
    /// Spectator (`FreeFlightControls`).
    Spectator,
}

/// The camera key a press is, by GPUI's key name and its exact modifiers
/// (`KeyCombination.testDown`: Shift+W does not move). GPUI's key event
/// does not say which `+` and `-` came from the numeric keypad, so the main
/// row's zoom too (a toolkit limit, `inventory/README.md`).
pub fn camera_key(key: &str, mods: Mods, kind: CameraKind) -> Option<u16> {
    match kind {
        CameraKind::Map if mods.none() => match key {
            "w" | "up" => Some(keys::FORWARD),
            "s" | "down" => Some(keys::BACKWARD),
            "a" | "left" => Some(keys::LEFT),
            "d" | "right" => Some(keys::RIGHT),
            "pageup" => Some(keys::TILT_UP),
            "pagedown" => Some(keys::TILT_DOWN),
            "delete" => Some(keys::ROTATE_LEFT),
            "end" => Some(keys::ROTATE_RIGHT),
            "insert" | "+" => Some(keys::ZOOM_IN),
            "home" | "-" => Some(keys::ZOOM_OUT),
            _ => None,
        },
        CameraKind::Map if mods.alt_only() => match key {
            "up" | "w" => Some(keys::TILT_UP),
            "down" | "s" => Some(keys::TILT_DOWN),
            "left" | "a" => Some(keys::ROTATE_LEFT),
            "right" | "d" => Some(keys::ROTATE_RIGHT),
            _ => None,
        },
        CameraKind::Spectator if mods.none() => match key {
            "w" | "up" => Some(keys::FORWARD),
            "s" | "down" => Some(keys::BACKWARD),
            "a" | "left" => Some(keys::LEFT),
            "d" | "right" => Some(keys::RIGHT),
            "space" | "pageup" => Some(keys::UP),
            "pagedown" => Some(keys::DOWN),
            _ => None,
        },
        _ => None,
    }
}

/// Player mode's keys (`web/bluemap-bridge.js` 696): W 1, S 2, A 4, D 8,
/// Space 16; Shift (32) and Ctrl (64) come as modifiers.
pub fn player_key(key: &str, mods: Mods) -> Option<u32> {
    if mods.alt || mods.platform {
        return None;
    }
    match key {
        "w" => Some(1),
        "s" => Some(2),
        "a" => Some(4),
        "d" => Some(8),
        "space" => Some(16),
        _ => None,
    }
}

/// The player's sneak bit.
pub const SNEAK: u32 = 32;
/// The player's sprint bit.
pub const SPRINT: u32 = 64;
/// Degrees of look per pixel of pointer motion (Minecraft's default
/// sensitivity, `LOOK_PER_PIXEL`).
pub const LOOK_PER_PIXEL: f32 = 0.15;

/// The keys of the camera, the selection, the type-ahead exclusion: a
/// printable key the pane keeps instead of the composer while it has
/// focus (`CAMERA_KEYS`: `KeyW KeyA KeyS KeyD Space NumpadAdd
/// NumpadSubtract`).
pub fn pane_keeps(key: &str) -> bool {
    matches!(key, "w" | "a" | "s" | "d" | "space" | "+" | "-")
}

/// The next free selection letter, A to Z; none when all 26 are used (a
/// 27th box is dropped silently, as today).
pub fn next_letter(used: &[String]) -> Option<String> {
    ('A'..='Z')
        .map(|c| c.to_string())
        .find(|l| !used.contains(l))
}

/// The box two picked blocks span, both corners included.
pub fn span(a: [i32; 3], b: [i32; 3]) -> ([i32; 3], [i32; 3]) {
    (
        [a[0].min(b[0]), a[1].min(b[1]), a[2].min(b[2])],
        [a[0].max(b[0]), a[1].max(b[1]), a[2].max(b[2])],
    )
}

/// A box in build coordinates (y minus the engine's offset), for naming.
pub fn build_box(min: [i32; 3], max: [i32; 3], y_offset: i32) -> ([i64; 3], [i64; 3]) {
    let to = |c: [i32; 3]| [c[0] as i64, (c[1] - y_offset) as i64, c[2] as i64];
    (to(min), to(max))
}

/// Where a bench member's label stands: its box's centre in x and z, two
/// blocks above its top (the layout is in Minecraft's coordinates).
pub fn label_anchor(min: [i32; 3], max: [i32; 3]) -> [f64; 3] {
    [
        (min[0] as f64 + max[0] as f64 + 1.0) / 2.0,
        max[1] as f64 + 2.0,
        (min[2] as f64 + max[2] as f64 + 1.0) / 2.0,
    ]
}

/// Where a selection's icon stands: `(centre x, max y + 1.6, centre z)`.
pub fn icon_anchor(min: [i32; 3], max: [i32; 3]) -> [f64; 3] {
    [
        (min[0] as f64 + max[0] as f64 + 1.0) / 2.0,
        max[1] as f64 + 1.6,
        (min[2] as f64 + max[2] as f64 + 1.0) / 2.0,
    ]
}

/// Where a frame is drawn in a pane of `pane` physical pixels: scaled
/// uniformly so its height fills the pane's (the camera keeps its vertical
/// field of view, so this is exact), centred, the rest black. A software
/// adapter's half-size frame is scaled up by the same rule. Returns the
/// drawn box's left, top, width and height in physical pixels.
pub fn placed(frame: [u32; 2], pane: [u32; 2]) -> [f32; 4] {
    let s = pane[1] as f32 / frame[1].max(1) as f32;
    let w = frame[0] as f32 * s;
    [(pane[0] as f32 - w) / 2.0, 0.0, w, pane[1] as f32]
}

/// A pointer at `at` in the pane (physical pixels) in the frame's own
/// pixels, for a pick, by the inverse of [`placed`]; none outside it.
pub fn in_frame(at: [f32; 2], frame: [u32; 2], pane: [u32; 2]) -> Option<[f32; 2]> {
    let [x, y, w, h] = placed(frame, pane);
    let u = (at[0] - x) / w * frame[0] as f32;
    let v = (at[1] - y) / h * frame[1] as f32;
    (u >= 0.0 && v >= 0.0 && u < frame[0] as f32 && v < frame[1] as f32).then_some([u, v])
}

/// The click's announcement (`Lever at 1, 65, 0, toggled`; `Stone at …,
/// takes no input`).
pub fn clicked_text(name: &str, block: [i32; 3], input: Option<&str>) -> String {
    let at = format!("{}, {}, {}", block[0], block[1], block[2]);
    match input {
        Some("toggle") => format!("{name} at {at}, toggled"),
        Some("press") => format!("{name} at {at}, pressed"),
        Some("use") => format!("{name} at {at}, used"),
        Some(other) => format!("{name} at {at}, {other}"),
        None => format!("{name} at {at}, takes no input"),
    }
}

/// The player moved to look at a box as a fly-to frames it
/// (`teleportTo`, `web/bluemap-bridge.js` 670-685): from the box's centre,
/// back along the player's heading tilted to the clamped angle, far enough
/// for the box to fit the camera's narrower field of view (`v_fov`
/// degrees, `aspect`) with a fifth to spare and never nearer than 12; the
/// feet 1.62 under that eye, the yaw kept, the pitch the angle's
/// (`pitchOf`). The renderer's camera state gives the field of view.
pub fn teleport(
    min: [i32; 3],
    max: [i32; 3],
    yaw: f32,
    pitch: f32,
    v_fov: f64,
    aspect: f64,
) -> ([f64; 3], f32, f32) {
    let centre: [f64; 3] = std::array::from_fn(|i| (min[i] as f64 + max[i] as f64 + 1.0) / 2.0);
    let size: [f64; 3] = std::array::from_fn(|i| max[i] as f64 - min[i] as f64 + 1.0);
    let radius = (size[0] * size[0] + size[1] * size[1] + size[2] * size[2]).sqrt() / 2.0;
    let vertical = v_fov.to_radians();
    let horizontal = 2.0 * ((vertical / 2.0).tan() * aspect).atan();
    let distance = (radius / (vertical.min(horizontal) / 2.0).sin() * 1.2).max(12.0);
    let angle = (90.0 - pitch as f64).to_radians().clamp(0.45, 1.3);
    let rotation = (yaw as f64 + 180.0).to_radians();
    // The heading (sin, 0, -cos) turned down about (up x heading) by
    // pi/2 - angle: heading * sin(angle) - up * cos(angle).
    let look = [
        rotation.sin() * angle.sin(),
        -angle.cos(),
        -rotation.cos() * angle.sin(),
    ];
    let eye: [f64; 3] = std::array::from_fn(|i| centre[i] - look[i] * distance);
    (
        [eye[0], eye[1] - 1.62, eye[2]],
        yaw,
        (90.0 - angle.to_degrees()) as f32,
    )
}

/// The player's start look from the map camera (`web/bluemap-bridge.js`
/// 723-744): yaw from the heading, pitch from the tilt, clamped to ±90.
pub fn start_look(rotation: f64, angle: f64) -> (f32, f32) {
    let yaw = rotation.to_degrees() - 180.0;
    let pitch = (90.0 - angle.to_degrees()).clamp(-90.0, 90.0);
    (yaw as f32, pitch as f32)
}

/// The player's look from a painted frame, once the frame shows the
/// player's eye: the renderer keeps the map's view (`map_eye`, the eye
/// when player mode began) in player mode until the core stands the
/// player, and a frame of that view would give the map camera's tilt.
pub fn stood_look(camera: &crate::CameraState, map_eye: [f64; 3]) -> Option<(f32, f32)> {
    (camera.mode == crate::CameraMode::Player && camera.eye != map_eye)
        .then(|| start_look(camera.rotation, camera.angle))
}

/// The export's name (`exportName`): the chat's title, `<bench title or
/// Bench> round <n>`, or `mcbench world`.
pub fn export_name(chat_title: Option<&str>, bench: Option<(&str, usize)>) -> String {
    match (chat_title, bench) {
        (_, Some((title, round))) => {
            let title = if title.is_empty() { "Bench" } else { title };
            format!("{title} round {round}")
        }
        (Some(title), None) if !title.is_empty() => title.to_string(),
        _ => "mcbench world".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PlayerState;

    fn state(view: &str, phase: LivePhase, running: Option<Running>) -> LiveState {
        LiveState {
            view: Some(ViewKey(view.into())),
            revision: Some(1),
            phase,
            running,
            tick: 1234,
            player: PlayerState::None,
            bounds: None,
            y_offset: 64,
            blocks: 42862,
            layout: Vec::new(),
        }
    }

    fn seen<'a>(view: Option<&'a ViewKey>, state: Option<&'a LiveState>) -> Seen<'a> {
        Seen {
            pane_error: None,
            status: Some(&PaneStatus::Drawing),
            view,
            state,
            painted: None,
            bench_mode: false,
            preparing: false,
            has_pane: true,
        }
    }

    #[test]
    fn the_players_look_waits_for_its_eye() {
        let map = crate::CameraState {
            mode: crate::CameraMode::Player,
            target: [0.0, 64.0, 0.0],
            distance: 40.0,
            rotation: 0.5,
            angle: 0.45,
            eye: [10.0, 100.0, -20.0],
            v_fov: 50.0,
            aspect: 1.5,
        };
        // Player mode, the map's view still held: no look from it.
        assert_eq!(stood_look(&map, map.eye), None);
        // A map frame is never the player's.
        let perspective = crate::CameraState {
            mode: crate::CameraMode::Perspective,
            ..map
        };
        assert_eq!(stood_look(&perspective, [0.0; 3]), None);
        // The player's eye: its rotation and angle are the stood look.
        let stood = crate::CameraState {
            eye: [3.5, 65.62, -2.5],
            rotation: std::f64::consts::PI,
            angle: std::f64::consts::FRAC_PI_2,
            ..map
        };
        assert_eq!(stood_look(&stood, map.eye), Some((0.0, 0.0)));
    }

    #[test]
    fn teleport_stands_the_player_where_the_bridge_does() {
        let near = |a: [f64; 3], b: [f64; 3]| (0..3).all(|i| (a[i] - b[i]).abs() < 1e-3);
        // One block, looking south, 30 degrees down: the 12-block floor.
        let (at, yaw, pitch) = teleport([0, 64, 0], [0, 64, 0], 0.0, 30.0, 70.0, 1.5);
        assert!(near(at, [0.5, 68.88, -9.8923]), "{at:?}");
        assert_eq!(yaw, 0.0);
        assert!((pitch - 30.0).abs() < 1e-3);
        // A 9 x 1 x 4 selection facing west, 10 degrees down: the angle
        // held at 1.3 rad (pitch 15.5155).
        let (at, _, pitch) = teleport([-10, 64, 17], [-2, 64, 20], -90.0, 10.0, 70.0, 1.5);
        assert!(near(at, [-17.0627, 66.09, 19.0]), "{at:?}");
        assert!((pitch - 15.5155).abs() < 1e-3);
        // A tall box in a narrow pane: the horizontal field of view decides.
        let (at, _, pitch) = teleport([0, 64, 0], [2, 140, 2], 45.0, 60.0, 70.0, 0.6);
        assert!(near(at, [43.7351, 204.3346, -40.7351]), "{at:?}");
        assert!((pitch - 60.0).abs() < 1e-3);
    }

    #[test]
    fn no_view_shows_builds_empty_text_and_bench_shows_none() {
        assert_eq!(
            chrome(&seen(None, None)).cover,
            Some((EMPTY_BUILD.into(), None))
        );
        let s = Seen {
            preparing: true,
            ..seen(None, None)
        };
        assert_eq!(chrome(&s).cover, Some((EMPTY_PREPARING.into(), None)));
        let s = Seen {
            bench_mode: true,
            ..seen(None, None)
        };
        assert_eq!(chrome(&s).cover, None);
    }

    #[test]
    fn a_view_not_painted_is_covered_with_its_kind_of_text() {
        let chat = ViewKey("c".into());
        let bench = ViewKey("bench:b:1".into());
        assert_eq!(
            chrome(&seen(Some(&chat), None)).cover,
            Some((PREPARING_CHAT.into(), None))
        );
        assert_eq!(
            chrome(&seen(Some(&bench), None)).cover,
            Some((PLACING_BUILDS.into(), None))
        );
    }

    #[test]
    fn a_painted_shown_world_has_no_cover_and_its_toolbar() {
        let chat = ViewKey("c".into());
        let s = state("c", LivePhase::Shown, Some(Running::Playing));
        let c = chrome(&Seen {
            painted: Some((&chat, 1)),
            ..seen(Some(&chat), Some(&s))
        });
        assert_eq!((c.cover, c.banner, c.toolbar), (None, None, true));
    }

    #[test]
    fn a_first_placement_that_failed_offers_try_again_and_a_later_one_does_not() {
        let chat = ViewKey("c".into());
        let s = state(
            "c",
            LivePhase::Failed {
                error: "HTTP 503".into(),
                nothing_shown: true,
            },
            None,
        );
        let c = chrome(&seen(Some(&chat), Some(&s)));
        assert_eq!(c.banner, Some(("HTTP 503".into(), Some(Offer::TryAgain))));
        assert!(c.cover.is_some());
        let s = state(
            "c",
            LivePhase::Failed {
                error: "HTTP 503".into(),
                nothing_shown: false,
            },
            None,
        );
        assert_eq!(
            chrome(&seen(Some(&chat), Some(&s))).banner,
            Some(("HTTP 503".into(), None))
        );
    }

    #[test]
    fn a_stopped_world_says_why_in_the_banner_and_keeps_its_toolbar() {
        let chat = ViewKey("c".into());
        let s = state(
            "c",
            LivePhase::Shown,
            Some(Running::Stopped("The live world stopped: x".into())),
        );
        let c = chrome(&Seen {
            painted: Some((&chat, 1)),
            ..seen(Some(&chat), Some(&s))
        });
        assert_eq!(c.banner, Some(("The live world stopped: x".into(), None)));
        assert!(c.toolbar);
        let t = toolbar(&s);
        assert!(!t.play_enabled && !t.step_enabled);
    }

    #[test]
    fn the_renderers_failure_covers_the_pane_with_its_sentence() {
        let chat = ViewKey("c".into());
        let failed = PaneStatus::Failed("The graphics device was lost: x".into());
        let c = chrome(&Seen {
            status: Some(&failed),
            ..seen(Some(&chat), None)
        });
        assert_eq!(
            c.cover,
            Some(("The graphics device was lost: x".into(), None))
        );
        let c = chrome(&Seen {
            pane_error: Some("No graphics adapter: x"),
            ..seen(Some(&chat), None)
        });
        assert_eq!(c.cover, Some(("No graphics adapter: x".into(), None)));
    }

    #[test]
    fn a_view_before_the_pane_says_the_live_world_is_preparing() {
        let chat = ViewKey("c".into());
        let c = chrome(&Seen {
            has_pane: false,
            ..seen(Some(&chat), None)
        });
        assert_eq!(c.cover, Some((PREPARING_LIVE.into(), None)));
        assert!(!c.toolbar);
    }

    #[test]
    fn a_state_for_another_view_is_not_this_views() {
        let chat = ViewKey("b".into());
        let s = state("a", LivePhase::Shown, Some(Running::Playing));
        let c = chrome(&Seen {
            painted: Some((&chat, 1)),
            ..seen(Some(&chat), Some(&s))
        });
        assert!(!c.toolbar);
    }

    #[test]
    fn the_toolbar_follows_playing_paused_and_counts_in_the_locale() {
        let t = toolbar(&state("c", LivePhase::Shown, Some(Running::Playing)));
        assert!(t.playing && t.play_enabled && !t.step_enabled);
        assert_eq!(t.ticks, "1,234 ticks");
        let t = toolbar(&state("c", LivePhase::Shown, Some(Running::Paused)));
        assert!(!t.playing && t.step_enabled);
        assert_eq!(grouped(0), "0");
        assert_eq!(grouped(999), "999");
        assert_eq!(grouped(1_000_000), "1,000,000");
    }

    #[test]
    fn a_click_is_within_500_ms_and_4_px() {
        assert!(is_click(0, 500, [0.0, 0.0], [4.0, 0.0]));
        assert!(!is_click(0, 501, [0.0, 0.0], [0.0, 0.0]));
        assert!(!is_click(0, 10, [0.0, 0.0], [3.0, 3.0]));
    }

    #[test]
    fn camera_keys_match_only_with_their_exact_modifiers() {
        let none = Mods::default();
        let alt = Mods { alt: true, ..none };
        let shift = Mods {
            shift: true,
            ..none
        };
        assert_eq!(camera_key("w", none, CameraKind::Map), Some(keys::FORWARD));
        assert_eq!(camera_key("w", shift, CameraKind::Map), None);
        assert_eq!(camera_key("w", alt, CameraKind::Map), Some(keys::TILT_UP));
        assert_eq!(
            camera_key("pageup", none, CameraKind::Map),
            Some(keys::TILT_UP)
        );
        assert_eq!(camera_key("pageup", alt, CameraKind::Map), None);
        assert_eq!(
            camera_key("delete", none, CameraKind::Map),
            Some(keys::ROTATE_LEFT)
        );
        assert_eq!(
            camera_key("left", alt, CameraKind::Map),
            Some(keys::ROTATE_LEFT)
        );
        assert_eq!(
            camera_key("insert", none, CameraKind::Map),
            Some(keys::ZOOM_IN)
        );
        assert_eq!(
            camera_key("home", none, CameraKind::Map),
            Some(keys::ZOOM_OUT)
        );
        assert_eq!(
            camera_key("space", none, CameraKind::Spectator),
            Some(keys::UP)
        );
        assert_eq!(camera_key("space", none, CameraKind::Map), None);
    }

    #[test]
    fn player_keys_are_not_taken_with_alt_or_cmd() {
        assert_eq!(player_key("w", Mods::default()), Some(1));
        assert_eq!(player_key("space", Mods::default()), Some(16));
        let alt = Mods {
            alt: true,
            ..Mods::default()
        };
        assert_eq!(player_key("w", alt), None);
        let ctrl = Mods {
            control: true,
            ..Mods::default()
        };
        assert_eq!(player_key("w", ctrl), Some(1));
    }

    #[test]
    fn the_next_letter_is_the_first_free_one_and_a_27th_is_none() {
        assert_eq!(next_letter(&[]), Some("A".into()));
        assert_eq!(next_letter(&["A".into(), "C".into()]), Some("B".into()));
        let all: Vec<String> = ('A'..='Z').map(|c| c.to_string()).collect();
        assert_eq!(next_letter(&all), None);
    }

    #[test]
    fn a_selection_is_named_in_build_coordinates() {
        let (min, max) = span([5, 70, 1], [2, 64, 3]);
        assert_eq!((min, max), ([2, 64, 1], [5, 70, 3]));
        assert_eq!(build_box(min, max, 64), ([2, 0, 1], [5, 6, 3]));
    }

    #[test]
    fn labels_stand_two_blocks_above_the_box_centre() {
        assert_eq!(label_anchor([0, 64, 0], [9, 66, 9]), [5.0, 68.0, 5.0]);
        assert_eq!(icon_anchor([0, 64, 0], [1, 64, 1]), [1.0, 65.6, 1.0]);
    }

    #[test]
    fn a_frame_is_drawn_at_the_new_heights_scale_centred() {
        assert_eq!(
            placed([1360, 1600], [1360, 1600]),
            [0.0, 0.0, 1360.0, 1600.0]
        );
        // The split widened: the old frame unscaled, centred.
        assert_eq!(
            placed([1000, 1600], [1360, 1600]),
            [180.0, 0.0, 1000.0, 1600.0]
        );
        // A software adapter's half-size frame scaled up to the pane.
        assert_eq!(placed([680, 800], [1360, 1600]), [0.0, 0.0, 1360.0, 1600.0]);
        assert_eq!(
            in_frame([180.0, 0.0], [1000, 1600], [1360, 1600]),
            Some([0.0, 0.0])
        );
        assert_eq!(in_frame([10.0, 0.0], [1000, 1600], [1360, 1600]), None);
        assert_eq!(
            in_frame([680.0, 800.0], [680, 800], [1360, 1600]),
            Some([340.0, 400.0])
        );
    }

    #[test]
    fn a_click_is_announced_with_its_input() {
        assert_eq!(
            clicked_text("Lever", [1, 65, 0], Some("toggle")),
            "Lever at 1, 65, 0, toggled"
        );
        assert_eq!(
            clicked_text("Stone", [2, 64, 0], None),
            "Stone at 2, 64, 0, takes no input"
        );
    }

    #[test]
    fn the_player_starts_looking_where_the_camera_looked() {
        let (yaw, pitch) = start_look(std::f64::consts::PI, std::f64::consts::FRAC_PI_2);
        assert!((yaw - 0.0).abs() < 1e-4 && pitch.abs() < 1e-4);
        assert_eq!(start_look(0.0, 0.0).1, 90.0);
    }

    #[test]
    fn the_export_is_named_as_today() {
        assert_eq!(export_name(Some("Castle"), None), "Castle");
        assert_eq!(export_name(None, Some(("", 2))), "Bench round 2");
        assert_eq!(export_name(None, Some(("Towers", 1))), "Towers round 1");
        assert_eq!(export_name(None, None), "mcbench world");
    }

    #[test]
    fn the_description_says_title_blocks_and_running_but_not_ticks() {
        let s = state("c", LivePhase::Shown, Some(Running::Playing));
        assert_eq!(
            description("Castle gate", Some(&s), None),
            "Castle gate, 42,862 blocks, playing"
        );
        assert_eq!(description("x", Some(&s), Some("Preparing")), "Preparing");
    }
}
