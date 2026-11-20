// The live view without a window: the renderer started in-process
// (mcbench_draw::live::Draw, the mcbench_render::Renderer), a world placed
// by the engine (a stone floor across several tiles, a lever beside a
// redstone lamp), shown, driven through its LivePane as the window drives
// it, and its frames taken. The engine's own tick patches light the lamp.
// Needs Minecraft's baked data (MCBENCH_DRAW_DATA) and the engine's tables
// (core/minecraft/1.21.10); skipped without them, as the other tests skip.
// MCBENCH_DRAW_SOFTWARE=1 runs it on the software floor.

mod common;

use mcbench_core::session::Session;
use mcbench_draw::assets::Paths;
use mcbench_draw::live::{Draw, FLOOR, STAGE};
use mcbench_render::live::{
    keys, CameraInput, CameraMode, DragButton, Frame, Framing, LivePane, LivePatch, LiveRenderer,
    OverlayBox, Overlays, PaneSize, PaneStatus, Pick, Shown, TimeOfDay, ViewKey,
};
use mcbench_render::{Clip, Renderer, WorldUpload};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

const Y_OFFSET: i32 = 64;
const LEVER: [i32; 3] = [2, 64, 0];
const LAMP: [i32; 3] = [3, 64, 0];
// The build: a floor 81 blocks square (several tiles), a lever and a lamp,
// stained glass in the lamp's tile and in another (translucent quads).
const MIN: [i32; 3] = [-40, 63, -40];
const MAX: [i32; 3] = [40, 64, 40];
const COMMANDS: &str = "fill -40 63 -40 40 63 40 minecraft:stone\nsetblock 2 64 0 minecraft:lever[face=floor,facing=north]\nsetblock 3 64 0 minecraft:redstone_lamp\nsetblock 1 64 -2 minecraft:light_blue_stained_glass\nsetblock -20 64 -20 minecraft:light_blue_stained_glass\n";

// The engine's tables read once; a session of its own for each test (the
// registry is not shared between threads).
fn engine() -> Option<Session> {
    static TABLES: OnceLock<bool> = OnceLock::new();
    let read = *TABLES.get_or_init(|| {
        let folder = common::crate_dir().join("../../core/minecraft/1.21.10");
        if !folder.join("blocks.txt").is_file() {
            eprintln!("live: skipped: the engine's tables are not in core/minecraft/1.21.10");
            return false;
        }
        mcbench_core::tables::set(mcbench_core::tables::read(&folder));
        true
    });
    read.then(|| {
        let (reg, blocks) = mcbench_core::boot();
        Session::new(reg, blocks)
    })
}

// The window's side: the waker counted, frames waited for.
struct Window {
    pane: Arc<dyn LivePane>,
    wakes: Arc<(Mutex<u64>, Condvar)>,
}

impl Window {
    fn new(pane: Arc<dyn LivePane>) -> Window {
        let wakes = Arc::new((Mutex::new(0u64), Condvar::new()));
        let w = wakes.clone();
        pane.set_waker(Box::new(move || {
            *w.0.lock().unwrap() += 1;
            w.1.notify_all();
        }));
        Window { pane, wakes }
    }

    fn wakes(&self) -> u64 {
        *self.wakes.0.lock().unwrap()
    }

    // The next frame within `ms`, or none.
    fn frame(&self, ms: u64) -> Option<Frame> {
        let deadline = Instant::now() + Duration::from_millis(ms);
        let mut n = self.wakes.0.lock().unwrap();
        loop {
            if let Some(frame) = self.pane.take() {
                return Some(frame);
            }
            let now = Instant::now();
            if now >= deadline {
                return None;
            }
            n = self.wakes.1.wait_timeout(n, deadline - now).unwrap().0;
        }
    }

    // Frames taken as a 60 Hz display paints, until none comes for 400 ms:
    // the last, and how many.
    fn settle(&self) -> (Frame, usize) {
        self.settle_after(
            self.frame(10_000)
                .unwrap_or_else(|| panic!("a frame: {:?}", self.pane.status())),
        )
    }

    // The same after a frame already taken.
    fn settle_after(&self, first: Frame) -> (Frame, usize) {
        let start = Instant::now();
        let mut last = first;
        let mut n = 1;
        loop {
            let painted = Instant::now();
            std::thread::sleep(Duration::from_micros(16_667).saturating_sub(painted.elapsed()));
            let Some(frame) = self.frame(400) else { break };
            last = frame;
            n += 1;
            assert!(
                start.elapsed() < Duration::from_secs(30),
                "the view settles: {:?}",
                last.camera
            );
        }
        (last, n)
    }

    fn pick(&self, seq: u64, at: [f32; 2]) -> Pick {
        let (tx, rx) = std::sync::mpsc::channel();
        self.pane
            .pick(seq, at, Box::new(move |p| tx.send(p).unwrap()));
        rx.recv_timeout(Duration::from_secs(10))
            .expect("a pick is answered")
    }
}

struct Setup {
    draw: Draw,
    session: Session,
    window: Window,
    size: PaneSize,
}

fn setup(test: &str) -> Option<Setup> {
    setup_with(test, COMMANDS, MIN, MAX)
}

fn setup_with(test: &str, commands: &str, min: [i32; 3], max: [i32; 3]) -> Option<Setup> {
    let data = common::data(test)?;
    let mut session = engine()?;
    let blueprint = session.place(commands, true).unwrap();
    let draw = Draw::open(
        Paths::of_data(&data).unwrap(),
        common::choice(),
        1 << 30,
        STAGE,
    )
    .unwrap();
    eprintln!(
        "{test}: {} ({}){}",
        draw.report().adapter.name,
        draw.report().adapter.backend,
        if draw.report().adapter.software {
            ", software"
        } else {
            ""
        }
    );
    draw.world(WorldUpload {
        name: "live:c".into(),
        y_offset: Y_OFFSET,
        blueprint,
        entities: None,
        base: None,
    })
    .unwrap();
    let window = Window::new(draw.live());
    let size = PaneSize {
        width: 640,
        height: 400,
        scale: 1.0,
    };
    window.pane.resize(size);
    draw.live_show(Some(Shown {
        world: "live:c".into(),
        view: ViewKey("c".into()),
        revision: 1,
        bounds: Clip { min, max },
        framing: Framing::AtOnce,
    }));
    Some(Setup {
        draw,
        session,
        window,
        size,
    })
}

fn frame_size(size: PaneSize) -> [u32; 2] {
    if common::software() {
        [size.width / 2, size.height / 2]
    } else {
        [size.width, size.height]
    }
}

// A frame's pixel (frame pixels) as the pane's (physical pixels).
fn pane_at(frame: &Frame, size: PaneSize, at: [f32; 2]) -> [f32; 2] {
    [
        at[0] * size.width as f32 / frame.size[0] as f32,
        at[1] * size.height as f32 / frame.size[1] as f32,
    ]
}

fn differing(a: &Frame, b: &Frame) -> Vec<(u32, u32)> {
    assert_eq!(a.size, b.size);
    let w = a.size[0] as usize;
    a.bgra()
        .chunks(4)
        .zip(b.bgra().chunks(4))
        .enumerate()
        .filter(|(_, (p, q))| p != q)
        .map(|(i, _)| ((i % w) as u32, (i / w) as u32))
        .collect()
}

// The corners of a block as anchors (Minecraft's coordinates).
fn corners(b: [i32; 3]) -> Vec<[f64; 3]> {
    (0..8)
        .map(|i| {
            [
                b[0] as f64 + (i & 1) as f64,
                b[1] as f64 + ((i >> 1) & 1) as f64,
                b[2] as f64 + ((i >> 2) & 1) as f64,
            ]
        })
        .collect()
}

// The pixel box of projected points, widened by `margin`.
fn rect(points: &[Option<[f32; 2]>], margin: f32) -> [f32; 4] {
    let p: Vec<[f32; 2]> = points
        .iter()
        .map(|p| p.expect("in front of the eye"))
        .collect();
    let (x0, x1) = (
        p.iter().map(|q| q[0]).fold(f32::MAX, f32::min),
        p.iter().map(|q| q[0]).fold(f32::MIN, f32::max),
    );
    let (y0, y1) = (
        p.iter().map(|q| q[1]).fold(f32::MAX, f32::min),
        p.iter().map(|q| q[1]).fold(f32::MIN, f32::max),
    );
    [x0 - margin, y0 - margin, x1 + margin, y1 + margin]
}

fn inside(r: [f32; 4], (x, y): (u32, u32)) -> bool {
    let (x, y) = (x as f32 + 0.5, y as f32 + 0.5);
    x >= r[0] && x <= r[2] && y >= r[1] && y <= r[3]
}

#[test]
fn frames_come_after_a_change_and_stop_when_still() {
    let Some(s) = setup("frames_come_after_a_change_and_stop_when_still") else {
        return;
    };
    let w = &s.window;
    let (first, n) = w.settle();
    eprintln!("show: {n} frames until still; stats {:?}", summary(&s.draw));
    assert_eq!(first.size, frame_size(s.size));
    assert_eq!(first.view, ViewKey("c".into()));
    assert_eq!(first.time, TimeOfDay::Day);
    assert_eq!(
        first.camera,
        w.pane.camera_state(),
        "the newest frame's camera"
    );
    assert_eq!(
        s.draw.live().facts().floor.as_deref(),
        common::software().then_some(FLOOR)
    );
    // Still: nothing drawn, nothing woken.
    let (frames, wakes) = (s.draw.stats().frames, w.wakes());
    std::thread::sleep(Duration::from_millis(600));
    assert_eq!(
        (s.draw.stats().frames, w.wakes()),
        (frames, wakes),
        "an idle pane draws nothing and wakes nothing"
    );
    assert!(w.pane.take().is_none());
    // Inputs while nothing is taken: one frame waits, the latest.
    for _ in 0..20 {
        w.pane.camera(CameraInput::Wheel {
            delta: 1.0,
            lines: true,
        });
    }
    std::thread::sleep(Duration::from_millis(400));
    assert_eq!(
        s.draw.stats().frames,
        frames + 1,
        "no frame is drawn while one waits in the slot"
    );
    let (last, _) = w.settle();
    assert!(
        last.camera.distance > first.camera.distance,
        "the wheel zoomed out: {} then {}",
        first.camera.distance,
        last.camera.distance
    );
    assert_eq!(last.camera, w.pane.camera_state());
    // A hidden pane draws nothing; shown again, it draws what changed.
    w.pane.visible(false);
    w.pane.camera(CameraInput::Wheel {
        delta: -1.0,
        lines: true,
    });
    assert!(w.frame(500).is_none(), "a hidden pane draws nothing");
    w.pane.visible(true);
    let (back, _) = w.settle();
    assert!(back.camera.distance < last.camera.distance);
    // Day and night: the frame carries the time it was drawn in.
    w.pane.time_of_day(TimeOfDay::Night);
    let (night, n) = w.settle();
    assert_eq!(
        (night.time, n),
        (TimeOfDay::Night, 1),
        "one frame for the switch, drawn at night"
    );
    assert!(night.seq > back.seq);
}

#[test]
fn the_camera_takes_drags_wheels_keys_flies_and_modes() {
    let Some(s) = setup("the_camera_takes_drags_wheels_keys_flies_and_modes") else {
        return;
    };
    let w = &s.window;
    let (start, _) = w.settle();
    // A fly frames the build (web/bluemap-bridge.js flyTo): x and z at its centre, the angle within 0.45 and 1.3.
    assert!(
        (start.camera.target[0] - 0.5).abs() < 1e-9 && (start.camera.target[2] - 0.5).abs() < 1e-9,
        "{:?}",
        start.camera.target
    );
    assert!((0.45..=1.3).contains(&start.camera.angle));
    // A right drag rotates; a left drag pans.
    w.pane.camera(CameraInput::PointerDown);
    w.pane.camera(CameraInput::Drag {
        button: DragButton::Right,
        dx: 60.0,
        dy: 0.0,
    });
    let (rotated, _) = w.settle();
    assert!(
        rotated.camera.rotation > start.camera.rotation,
        "{} then {}",
        start.camera.rotation,
        rotated.camera.rotation
    );
    w.pane.camera(CameraInput::PointerDown);
    w.pane.camera(CameraInput::Drag {
        button: DragButton::Left,
        dx: 0.0,
        dy: 50.0,
    });
    let (panned, _) = w.settle();
    assert_ne!(panned.camera.target, rotated.camera.target);
    // Keys held move the target while held, and stop when released.
    w.pane.camera(CameraInput::Keys(keys::FORWARD));
    std::thread::sleep(Duration::from_millis(300));
    w.pane.camera(CameraInput::Keys(0));
    let (keyed, n) = w.settle();
    assert!(n > 2, "frames while a key is held: {n}");
    assert_ne!(keyed.camera.target, panned.camera.target);
    // FaceNorth turns to heading 0.
    w.pane.camera(CameraInput::FaceNorth);
    let (north, _) = w.settle();
    assert!(
        north.camera.rotation.abs() < 1e-6,
        "{}",
        north.camera.rotation
    );
    // A fly of 700 ms back to the lamp.
    w.pane.camera(CameraInput::Frame {
        min: LAMP,
        max: LAMP,
        ms: 700,
    });
    let t = Instant::now();
    let (flown, n) = w.settle();
    assert!(
        t.elapsed() >= Duration::from_millis(700) && n > 3,
        "a fly of 700 ms draws frames: {n}"
    );
    assert!((flown.camera.target[0] - 3.5).abs() < 1e-9);
    // Flat: orthographic, straight down, north.
    w.pane.camera(CameraInput::Mode(CameraMode::Flat));
    let (flat, _) = w.settle();
    assert_eq!(
        (flat.camera.mode, flat.camera.angle),
        (CameraMode::Flat, 0.0)
    );
    // Spectator flies at eye level; player mode keeps the map view until a
    // player stands, and leaving it gives that view back.
    w.pane.camera(CameraInput::Mode(CameraMode::Spectator));
    let (spectator, _) = w.settle();
    assert_eq!(
        (spectator.camera.mode, spectator.camera.distance),
        (CameraMode::Spectator, 0.0)
    );
    w.pane.camera(CameraInput::Mode(CameraMode::Perspective));
    let (map, _) = w.settle();
    w.pane.camera(CameraInput::Mode(CameraMode::Player));
    w.pane.camera(CameraInput::Mode(CameraMode::Perspective));
    let (back, _) = w.settle();
    assert_eq!(
        (
            back.camera.target,
            back.camera.distance,
            back.camera.rotation
        ),
        (map.camera.target, map.camera.distance, map.camera.rotation)
    );
    // Reset: BlueMap's start, (0, 0) from 1500 straight down.
    w.pane.camera(CameraInput::Reset);
    let (reset, _) = w.settle();
    assert_eq!(
        (
            reset.camera.distance,
            reset.camera.angle,
            reset.camera.rotation
        ),
        (1500.0, 0.0, 0.0)
    );
}

#[test]
fn picks_name_the_block_and_face_under_known_pixels() {
    let Some(s) = setup("picks_name_the_block_and_face_under_known_pixels") else {
        return;
    };
    let w = &s.window;
    let lamp_top = [
        LAMP[0] as f64 + 0.5,
        LAMP[1] as f64 + 1.0,
        LAMP[2] as f64 + 0.5,
    ];
    let floor = [-10.5, 64.0, 7.5];
    let lamp_side = [
        LAMP[0] as f64 + 1.0,
        LAMP[1] as f64 + 0.5,
        LAMP[2] as f64 + 0.5,
    ];
    w.settle();
    w.pane.camera(CameraInput::Frame {
        min: LAMP,
        max: LAMP,
        ms: 0,
    });
    w.pane.camera(CameraInput::PointerDown);
    w.pane.camera(CameraInput::Drag {
        button: DragButton::Right,
        dx: -25.0,
        dy: 0.0,
    });
    let glass_top = [1.5, 65.0, -1.5];
    w.pane.overlays(Overlays {
        anchors: vec![lamp_top, floor, lamp_side, glass_top],
        ..Default::default()
    });
    let (frame, _) = w.settle();
    let at = |i: usize| pane_at(&frame, s.size, frame.anchors[i].expect("anchor in front"));
    for (i, block, face) in [
        (0, LAMP, [0, 1, 0]),
        (1, [-11, 63, 7], [0, 1, 0]),
        (2, LAMP, [1, 0, 0]),
    ] {
        let t = Instant::now();
        let pick = w.pick(frame.seq, at(i));
        eprintln!(
            "pick {i} at {:?}: {pick:?} in {:.2} ms",
            at(i),
            t.elapsed().as_secs_f64() * 1e3
        );
        assert_eq!(
            (pick.block, pick.face, pick.seq, &pick.view),
            (Some(block), Some(face), frame.seq, &frame.view),
            "anchor {i}"
        );
        assert!(
            (pick.yaw - (frame.camera.rotation * 180.0 / std::f64::consts::PI - 180.0) as f32)
                .abs()
                < 1e-3
        );
    }
    // Translucent quads pass through: the glass's top answers what is behind it.
    let through = w.pick(frame.seq, at(3));
    assert!(
        through.block.is_some() && through.block != Some([1, 64, -2]),
        "{through:?}"
    );
    // A double click zooms toward the point under it (BlueMapApp.mapInteraction).
    w.pane.camera(CameraInput::DoubleClick {
        seq: frame.seq,
        at: at(0),
    });
    let (zoomed, _) = w.settle();
    assert!(
        (zoomed.camera.target[0] - 3.5).abs() < 0.1 && (zoomed.camera.target[2] - 0.5).abs() < 0.1,
        "{:?}",
        zoomed.camera.target
    );
    assert!(
        (zoomed.camera.distance - (frame.camera.distance * 0.25).max(5.0)).abs() < 1e-6,
        "{} from {}",
        zoomed.camera.distance,
        frame.camera.distance
    );
    // The sky: the top left corner looks over the void.
    w.pane.camera(CameraInput::Mode(CameraMode::Spectator));
    let (level, _) = w.settle();
    let sky = w.pick(level.seq, [1.0, 1.0]);
    assert_eq!((sky.block, sky.face), (None, None), "the sky is a miss");
    // A frame older than the three kept answers a miss.
    let mut now = level;
    for _ in 0..4 {
        // In spectator mode the wheel sets the flying speed: one frame each, the same view.
        w.pane.camera(CameraInput::Wheel {
            delta: 1.0,
            lines: true,
        });
        now = w.frame(5000).expect("a frame");
    }
    let old = w.pick(frame.seq, at(0));
    assert_eq!(
        (old.block, old.seq),
        (None, frame.seq),
        "an old frame's pick misses"
    );
    // A pick of the frame being looked at, with the camera moved since:
    // the frame's camera is the one cast with.
    w.pane.camera(CameraInput::PointerDown);
    w.pane.camera(CameraInput::Drag {
        button: DragButton::Right,
        dx: 200.0,
        dy: 0.0,
    });
    let moved = w.pick(
        now.seq,
        [s.size.width as f32 / 2.0, s.size.height as f32 / 2.0],
    );
    let again = w.pick(
        now.seq,
        [s.size.width as f32 / 2.0, s.size.height as f32 / 2.0],
    );
    assert_eq!(moved.block, again.block);
    let stats = s.draw.stats();
    eprintln!(
        "picks: {:?} ms",
        stats
            .pick_ms
            .iter()
            .map(|v| (v * 100.0).round() / 100.0)
            .collect::<Vec<_>>()
    );
}

#[test]
fn player_mode_draws_from_the_players_eye_and_gives_the_map_view_back() {
    let Some(mut s) = setup("player_mode_draws_from_the_players_eye_and_gives_the_map_view_back")
    else {
        return;
    };
    let w = &s.window;
    let (map, _) = w.settle();
    w.pane.camera(CameraInput::Mode(CameraMode::Player));
    let (waiting, _) = w.settle();
    assert_eq!(
        (
            waiting.camera.mode,
            waiting.camera.target,
            waiting.camera.distance
        ),
        (CameraMode::Player, map.camera.target, map.camera.distance),
        "the map's view until the player stands"
    );
    s.session.spawn_player([0.5, 64.0, 5.5], 180.0, 20.0, false);
    let mut last = None;
    for tick in 1..=6u64 {
        let (ticked, view) = s.session.player_tick(1, 180.0, 20.0).unwrap();
        let view = mcbench_render::live::PlayerView::decode(&view).unwrap();
        s.draw.live_patch(LivePatch {
            view: ViewKey("c".into()),
            revision: 1,
            tick,
            changes: ticked.changes,
            entities: ticked.entities,
            player: Some(view),
            framing: None,
        });
        last = Some(view);
        std::thread::sleep(Duration::from_millis(50));
        while w.pane.take().is_some() {}
    }
    let (eye, n) = w.settle();
    let v = last.unwrap();
    eprintln!(
        "player: {n} frames to settle, {:?}; the view's feet {:?}",
        eye.camera, v.pos
    );
    assert_eq!(eye.camera.mode, CameraMode::Player);
    let expected = [v.pos[0], v.pos[1] + v.eye_height as f64, v.pos[2]];
    assert!(
        (0..3).all(|i| (eye.camera.eye[i] - expected[i]).abs() < 0.3),
        "{:?} against {expected:?}",
        eye.camera.eye
    );
    assert!(v.pos[2] < 5.5, "the player walked north");
    assert!(
        (eye.camera.v_fov - 70.0).abs() < 7.0,
        "Minecraft's field of view: {}",
        eye.camera.v_fov
    );
    w.pane.camera(CameraInput::Mode(CameraMode::Perspective));
    let (back, _) = w.settle();
    assert_eq!(
        (
            back.camera.mode,
            back.camera.target,
            back.camera.distance,
            back.camera.rotation
        ),
        (
            CameraMode::Perspective,
            map.camera.target,
            map.camera.distance,
            map.camera.rotation
        )
    );
}

#[test]
fn overlays_draw_over_the_world_and_go_without_a_trace() {
    let Some(s) = setup("overlays_draw_over_the_world_and_go_without_a_trace") else {
        return;
    };
    let w = &s.window;
    let (plain, _) = w.settle();
    w.pane.overlays(Overlays {
        anchors: corners([-3, 63, -3]),
        boxes: vec![OverlayBox {
            min: [-3, 63, -3],
            max: [-1, 64, -1],
            fill: 0.22,
            outline: true,
        }],
        marked: Some(LAMP),
    });
    let (marked, n) = w.settle();
    assert_eq!(n, 1, "one frame for new overlays");
    let changed = differing(&plain, &marked);
    let boxed = rect(&marked.anchors, 3.0);
    assert!(
        changed.iter().filter(|p| inside(boxed, **p)).count() > 20,
        "the box drawn"
    );
    assert!(
        changed.iter().any(|p| !inside(boxed, *p)),
        "the marked cube drawn"
    );
    w.pane.overlays(Overlays::default());
    let (cleared, _) = w.settle();
    assert!(
        plain.bgra() == cleared.bgra(),
        "without overlays the frame is the plain one again, byte for byte"
    );
}

#[test]
fn a_selection_lying_on_the_ground_shows_and_its_hover_fill_changes_it() {
    let Some(s) = setup("a_selection_lying_on_the_ground_shows_and_its_hover_fill_changes_it")
    else {
        return;
    };
    let w = &s.window;
    let (plain, _) = w.settle();
    // A 9 x 1 x 4 selection of the floor's own blocks (y 63), fill only, as a
    // selection's chip draws it; with its faces on the blocks' faces it would
    // lose to them under the depth test and change nothing.
    let selection = |fill: f32| Overlays {
        anchors: corners([-8, 63, 4])
            .into_iter()
            .chain(corners([0, 63, 7]))
            .collect(),
        boxes: vec![OverlayBox {
            min: [-8, 63, 4],
            max: [0, 63, 7],
            fill,
            outline: false,
        }],
        marked: None,
    };
    w.pane.overlays(selection(0.1));
    let (shown, _) = w.settle();
    let boxed = rect(&shown.anchors, 3.0);
    let changed = differing(&plain, &shown);
    assert!(
        changed.iter().filter(|p| inside(boxed, **p)).count() > 20,
        "the selection's fill is drawn over the blocks it lies on"
    );
    w.pane.overlays(selection(0.22));
    let (hovered, _) = w.settle();
    assert!(
        differing(&shown, &hovered)
            .iter()
            .filter(|p| inside(boxed, **p))
            .count()
            > 20,
        "the hovered chip's higher fill changes the frame again"
    );
}

// The engine's ticks after the lever is pulled, as patches.
fn pull_lever(session: &mut Session, view: &str, revision: u64) -> Vec<LivePatch> {
    session
        .input("toggle", LEVER[0], LEVER[1], LEVER[2], 0.0)
        .unwrap();
    (1..=3)
        .map(|tick| {
            let ticked = session.tick(1, true).unwrap();
            LivePatch {
                view: ViewKey(view.into()),
                revision,
                tick,
                changes: ticked.changes,
                entities: ticked.entities,
                player: None,
                framing: None,
            }
        })
        .collect()
}

#[test]
fn the_lamp_lit_by_a_tick_changes_only_its_pixels_and_tiles() {
    let Some(mut s) = setup("the_lamp_lit_by_a_tick_changes_only_its_pixels_and_tiles") else {
        return;
    };
    let w = &s.window;
    w.settle();
    w.pane.camera(CameraInput::Frame {
        min: LEVER,
        max: LAMP,
        ms: 0,
    });
    let mut anchors = corners(LAMP);
    anchors.extend(corners(LEVER));
    w.pane.overlays(Overlays {
        anchors,
        ..Default::default()
    });
    let (before, _) = w.settle();
    let patches = pull_lever(&mut s.session, "c", 1);
    // The mesher's own answer for the same patches, on the same world.
    let data = common::data("").unwrap();
    let assets = mcbench_draw::assets::Assets::load(&Paths::of_data(&data).unwrap()).unwrap();
    let mut world = mcbench_renderer::world::World::read(
        &assets.pack,
        &engine_blueprint(),
        mcbench_renderer::world::Mask {
            min_x: -30_000_000,
            max_x: 30_000_000,
            min_z: -30_000_000,
            max_z: 30_000_000,
        },
    )
    .unwrap();
    let all = mcbench_renderer::mesh::tiles(&world);
    let mut answered: Vec<(i32, i32)> = patches
        .iter()
        .flat_map(|p| world.patch(&assets.pack, &p.changes).unwrap())
        .collect();
    answered.sort();
    answered.dedup();
    let frames = s.draw.stats().frames;
    for p in patches {
        s.draw.live_patch(p);
    }
    let (after, _) = w.settle();
    assert!(s.draw.stats().frames > frames);
    let changed = differing(&before, &after);
    eprintln!("{} pixels changed", changed.len());
    let lamp = rect(&after.anchors[..8], 1.5);
    let lever = rect(&after.anchors[8..], 1.5);
    let outside: Vec<_> = changed
        .iter()
        .filter(|p| !inside(lamp, **p) && !inside(lever, **p))
        .collect();
    assert!(
        outside.is_empty(),
        "{} pixels changed outside the lamp and the lever: {:?}",
        outside.len(),
        &outside[..outside.len().min(10)]
    );
    let lit = changed
        .iter()
        .filter(|p| inside(lamp, **p) && !inside(lever, **p))
        .count();
    assert!(lit > 30, "the lamp's pixels changed: {lit}");
    // Brighter: the lit lamp's texture and its emitter's shade.
    let luma = |f: &Frame| -> u64 {
        changed
            .iter()
            .filter(|p| inside(lamp, **p))
            .map(|&(x, y)| {
                let i = ((y * f.size[0] + x) * 4) as usize;
                f.bgra()[i] as u64 + f.bgra()[i + 1] as u64 + f.bgra()[i + 2] as u64
            })
            .sum()
    };
    assert!(luma(&after) > luma(&before), "the lamp is brighter lit");
    let stats = s.draw.stats();
    let mut meshed: Vec<(i32, i32)> = stats.meshed.iter().flatten().copied().collect();
    meshed.sort();
    meshed.dedup();
    eprintln!(
        "tiles: {} in all, World::patch answered {:?}, meshed {:?}, batches {:?}",
        all.len(),
        answered,
        meshed,
        stats.batches
    );
    assert_eq!(
        meshed, answered,
        "only the tiles World::patch answers are meshed again"
    );
    assert!(meshed.len() < all.len());
    // The patched world draws as the same world shown fresh, byte for byte.
    let mut again = engine().unwrap();
    again.place(COMMANDS, true).unwrap();
    pull_lever(&mut again, "c", 1);
    let blueprint = again.place("", false).unwrap();
    s.draw
        .world(WorldUpload {
            name: "live:fresh".into(),
            y_offset: Y_OFFSET,
            blueprint,
            entities: None,
            base: None,
        })
        .unwrap();
    s.draw.live_show(Some(Shown {
        world: "live:fresh".into(),
        view: ViewKey("c".into()),
        revision: 2,
        bounds: Clip { min: MIN, max: MAX },
        framing: Framing::Keep,
    }));
    let (fresh, _) = w.settle();
    assert_eq!(fresh.camera, after.camera);
    assert!(
        fresh.bgra() == after.bgra(),
        "{} pixels differ between the patched world and the same world fresh",
        differing(&fresh, &after).len()
    );
}

fn engine_blueprint() -> Vec<u8> {
    static BLUEPRINT: OnceLock<Vec<u8>> = OnceLock::new();
    BLUEPRINT
        .get_or_init(|| engine().unwrap().place(COMMANDS, true).unwrap())
        .clone()
}

#[test]
fn a_refused_patch_fails_the_pane_and_a_new_show_draws_again() {
    let Some(s) = setup("a_refused_patch_fails_the_pane_and_a_new_show_draws_again") else {
        return;
    };
    let w = &s.window;
    w.settle();
    let wakes = w.wakes();
    // A block of a palette entry the stream never had.
    let mut changes = Vec::new();
    changes.extend_from_slice(&0i32.to_be_bytes());
    for v in MIN.iter().chain(MAX.iter()) {
        changes.extend_from_slice(&v.to_be_bytes());
    }
    changes.extend_from_slice(&36i32.to_be_bytes());
    changes.extend_from_slice(&1i32.to_be_bytes());
    for v in [0i32, 64, 0] {
        changes.extend_from_slice(&v.to_be_bytes());
    }
    changes.extend_from_slice(&999u16.to_be_bytes());
    s.draw.live_patch(LivePatch {
        view: ViewKey("c".into()),
        revision: 1,
        tick: 1,
        changes,
        entities: Vec::new(),
        player: None,
        framing: None,
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    while w.pane.status() == PaneStatus::Drawing {
        assert!(Instant::now() < deadline, "the pane fails");
        std::thread::sleep(Duration::from_millis(10));
    }
    let PaneStatus::Failed(sentence) = w.pane.status() else {
        unreachable!()
    };
    eprintln!("refused: {sentence}");
    assert!(sentence.contains("outside its palette"), "{sentence}");
    assert!(w.wakes() > wakes, "the status change woke the window");
    w.pane.camera(CameraInput::Wheel {
        delta: 1.0,
        lines: true,
    });
    assert!(w.frame(500).is_none(), "a failed pane draws nothing");
    // The core shows the world again.
    let blueprint = engine_blueprint();
    s.draw
        .world(WorldUpload {
            name: "live:c2".into(),
            y_offset: Y_OFFSET,
            blueprint,
            entities: None,
            base: None,
        })
        .unwrap();
    s.draw.live_show(Some(Shown {
        world: "live:c2".into(),
        view: ViewKey("c".into()),
        revision: 2,
        bounds: Clip { min: MIN, max: MAX },
        framing: Framing::Keep,
    }));
    let (frame, _) = w.settle();
    assert_eq!((frame.revision, w.pane.status()), (2, PaneStatus::Drawing));
}

#[test]
fn a_world_too_large_for_the_limit_fails_with_its_sentence() {
    let Some(data) = common::data("a_world_too_large_for_the_limit_fails_with_its_sentence") else {
        return;
    };
    let Some(mut session) = engine() else { return };
    let blueprint = session.place(COMMANDS, true).unwrap();
    // A limit below what the world's quads and the drawing targets take.
    let draw = Draw::open(
        Paths::of_data(&data).unwrap(),
        common::choice(),
        1 << 20,
        STAGE,
    )
    .unwrap();
    draw.world(WorldUpload {
        name: "live:c".into(),
        y_offset: Y_OFFSET,
        blueprint,
        entities: None,
        base: None,
    })
    .unwrap();
    let w = Window::new(draw.live());
    w.pane.resize(PaneSize {
        width: 1280,
        height: 800,
        scale: 2.0,
    });
    draw.live_show(Some(Shown {
        world: "live:c".into(),
        view: ViewKey("c".into()),
        revision: 1,
        bounds: Clip { min: MIN, max: MAX },
        framing: Framing::AtOnce,
    }));
    assert!(w.frame(2000).is_none());
    let PaneStatus::Failed(sentence) = w.pane.status() else {
        panic!("the pane fails: {:?}", w.pane.status())
    };
    eprintln!("too large: {sentence}");
    assert!(
        sentence.starts_with("This world needs about ")
            && sentence.ends_with(" MB to draw and the renderer's limit on this computer is 1 MB"),
        "{sentence}"
    );
}

#[test]
fn a_lost_device_is_made_again_once_and_a_second_loss_fails() {
    let Some(s) = setup("a_lost_device_is_made_again_once_and_a_second_loss_fails") else {
        return;
    };
    let w = &s.window;
    let (before, _) = w.settle();
    s.draw.lose_device();
    // The world drawn again on the new device from the renderer's own copy.
    let deadline = Instant::now() + Duration::from_secs(10);
    while s.draw.stats().recreated == 0 {
        assert!(
            Instant::now() < deadline && w.pane.status() == PaneStatus::Drawing,
            "the device made again: {:?}",
            w.pane.status()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    let (after, _) = w.settle();
    assert_eq!(s.draw.stats().recreated, 1);
    assert_eq!(w.pane.status(), PaneStatus::Drawing);
    assert!(
        before.bgra() == after.bgra(),
        "the same picture on the new device"
    );
    // Shots still draw.
    let shot: mcbench_render::Shot = serde_json::from_str(r#"{"width":64,"height":48,"camera":{"yaw":-0.785,"pitch":0.524,"focus":[0,0,0],"focusDist":40,"fov":70},"clip":null,"grid":null}"#).unwrap();
    let blueprint = engine_blueprint();
    s.draw
        .world(WorldUpload {
            name: "shots".into(),
            y_offset: Y_OFFSET,
            blueprint,
            entities: None,
            base: None,
        })
        .unwrap();
    let pictures = s
        .draw
        .shots(
            "shots",
            &[shot],
            2,
            mcbench_render::PictureFormat::Png,
            &std::sync::atomic::AtomicBool::new(false),
        )
        .unwrap();
    assert_eq!(pictures.len(), 1);
    s.draw.lose_device();
    w.pane.camera(CameraInput::Keys(0));
    let deadline = Instant::now() + Duration::from_secs(10);
    while w.pane.status() == PaneStatus::Drawing {
        assert!(Instant::now() < deadline, "the second loss fails the pane");
        std::thread::sleep(Duration::from_millis(10));
    }
    let PaneStatus::Failed(sentence) = w.pane.status() else {
        unreachable!()
    };
    eprintln!("second loss: {sentence}");
    assert!(
        sentence.starts_with("The graphics device was lost: "),
        "{sentence}"
    );
}

#[test]
fn live_frames_go_between_the_shots_of_a_see_build_call() {
    let Some(s) = setup("live_frames_go_between_the_shots_of_a_see_build_call") else {
        return;
    };
    let w = &s.window;
    w.settle();
    let blueprint = engine_blueprint();
    s.draw
        .world(WorldUpload {
            name: "shots".into(),
            y_offset: Y_OFFSET,
            blueprint,
            entities: None,
            base: None,
        })
        .unwrap();
    let shot: mcbench_render::Shot = serde_json::from_str(r#"{"width":1092,"height":820,"camera":{"yaw":-0.785,"pitch":0.524,"focus":[0,0,0],"focusDist":40,"fov":70},"clip":null,"grid":null}"#).unwrap();
    let shots = vec![shot; 12];
    let draw = &s.draw;
    std::thread::scope(|scope| {
        let call = scope.spawn(|| {
            let t = Instant::now();
            let pictures = draw
                .shots(
                    "shots",
                    &shots,
                    2,
                    mcbench_render::PictureFormat::Jpeg(90),
                    &std::sync::atomic::AtomicBool::new(false),
                )
                .unwrap();
            (pictures.len(), t.elapsed())
        });
        // The camera keeps moving while the shots draw.
        let mut during = 0;
        let start = Instant::now();
        w.pane.camera(CameraInput::Keys(keys::ROTATE_LEFT));
        let mut longest = Duration::ZERO;
        let mut last = Instant::now();
        while !call.is_finished() {
            if w.frame(50).is_some() {
                during += 1;
                longest = longest.max(last.elapsed());
                last = Instant::now();
            }
        }
        w.pane.camera(CameraInput::Keys(0));
        let (n, took) = call.join().unwrap();
        eprintln!("{n} shots in {took:?}; {during} live frames meanwhile, the longest wait between two {longest:?}; {:?} from the start", start.elapsed());
        assert_eq!(n, 12);
        assert!(during >= 6, "live frames between the shots: {during}");
    });
}

fn summary(draw: &Draw) -> String {
    let s = draw.stats();
    let med = |v: &[f64]| {
        let mut v = v.to_vec();
        v.sort_by(f64::total_cmp);
        v.get(v.len() / 2).copied().unwrap_or(0.0)
    };
    format!(
        "{} frames, draw median {:.2} ms, readback median {:.2} ms",
        s.frames,
        med(&s.draw_ms),
        med(&s.readback_ms)
    )
}

// Each corner carries its sky and block light for the live view's night:
// the floor under open sky has sky 15 and no block light far from the lit
// lamp, and block light near it.
#[test]
fn quads_carry_their_corners_sky_and_block_light() {
    let Some(data) = common::data("quads_carry_their_corners_sky_and_block_light") else {
        return;
    };
    let Some(mut session) = engine() else { return };
    session.place(COMMANDS, true).unwrap();
    pull_lever(&mut session, "c", 1);
    let blueprint = session.place("", false).unwrap();
    let assets = mcbench_draw::assets::Assets::load(&Paths::of_data(&data).unwrap()).unwrap();
    let world = mcbench_renderer::world::World::read(
        &assets.pack,
        &blueprint,
        mcbench_renderer::world::Mask {
            min_x: -30_000_000,
            max_x: 30_000_000,
            min_z: -30_000_000,
            max_z: 30_000_000,
        },
    )
    .unwrap();
    let draw = mcbench_draw::scene::Draw {
        pack: &assets.pack,
        facts: &assets.facts,
        y_offset: Y_OFFSET,
    };
    let quads = draw.quads(&world);
    let top = |x: i32, z: i32| {
        quads
            .iter()
            .find(|q| q.block == [x, 63 - Y_OFFSET, z] && q.normal[1] > 0.99)
            .unwrap_or_else(|| panic!("the floor's top at {x}, {z}"))
    };
    assert_eq!(
        top(-30, -30).light,
        [[15.0, 0.0]; 4],
        "open sky, no lamp near"
    );
    // Two blocks east of the lamp: its light, 15 less 2 at the cell above, averaged at the corners.
    let near = top(5, 0);
    assert!(
        near.light
            .iter()
            .all(|c| c[0] == 15.0 && (12.0..=14.0).contains(&c[1])),
        "{:?}",
        near.light
    );
    // The lit lamp itself: its faces read the cells around it, lit 14 by it.
    let lamp: Vec<_> = quads
        .iter()
        .filter(|q| q.block == [LAMP[0], LAMP[1] - Y_OFFSET, LAMP[2]])
        .collect();
    assert!(
        !lamp.is_empty()
            && lamp
                .iter()
                .all(|q| q.emissive && q.light.iter().all(|c| c[1] >= 13.0)),
        "{:?}",
        lamp.iter().map(|q| q.light).collect::<Vec<_>>()
    );
}

// Timings of the live view on a real build (MCBENCH_DRAW_BUILD: a folder
// with blueprint.bin and job.json's yOffset), at MCBENCH_LIVE_SIZE (default
// 2560x1600): the show, frames while the camera turns (draw: from the
// draw's start to its rows mapped; readback: the rows copied out), picks,
// and edits of a few blocks as patches (World::patch and the meshing on
// rayon's threads, the tiles written on the render thread, and from
// live_patch to the frame showing it). Run by hand: cargo test --release
// --test live -- --ignored --nocapture live_timings.
#[test]
#[ignore]
fn live_timings() {
    let data = common::data("live_timings").expect("MCBENCH_DRAW_DATA");
    let build = std::path::PathBuf::from(
        std::env::var_os("MCBENCH_DRAW_BUILD").expect("MCBENCH_DRAW_BUILD"),
    );
    let blueprint = std::fs::read(build.join("blueprint.bin")).unwrap();
    let job: serde_json::Value =
        serde_json::from_slice(&std::fs::read(build.join("job.json")).unwrap()).unwrap();
    let y = job["yOffset"].as_i64().unwrap() as i32;
    let (width, height) = std::env::var("MCBENCH_LIVE_SIZE").map_or((2560, 1600), |s| {
        let (w, h) = s.split_once('x').unwrap();
        (w.parse().unwrap(), h.parse().unwrap())
    });
    let paths = Paths::of_data(&data).unwrap();
    let draw = Draw::open(paths.clone(), common::choice(), 1 << 30, STAGE).unwrap();
    let t = Instant::now();
    let report = draw
        .world(WorldUpload {
            name: "live:t".into(),
            y_offset: y,
            blueprint: blueprint.clone(),
            entities: None,
            base: None,
        })
        .unwrap();
    let world_ms = t.elapsed().as_secs_f64() * 1e3;
    let w = Window::new(draw.live());
    w.pane.resize(PaneSize {
        width,
        height,
        scale: 2.0,
    });
    let (lo, hi) = (report.bounds.min, report.bounds.max);
    let t = Instant::now();
    draw.live_show(Some(Shown {
        world: "live:t".into(),
        view: ViewKey("t".into()),
        revision: 1,
        bounds: Clip {
            min: [lo[0], lo[1] + y, lo[2]],
            max: [hi[0], hi[1] + y, hi[2]],
        },
        framing: Framing::AtOnce,
    }));
    let first = w
        .frame(60_000)
        .unwrap_or_else(|| panic!("a first frame: {:?}", w.pane.status()));
    let first_ms = t.elapsed().as_secs_f64() * 1e3;
    let first_size = first.size;
    w.settle_after(first);
    // Turning for 3 s, painted at 60 Hz: by day, then by night.
    let turn = || -> (Vec<f64>, Vec<f64>, Frame) {
        let skip = draw.stats().frames as usize;
        w.pane.camera(CameraInput::Keys(keys::ROTATE_LEFT));
        let t = Instant::now();
        while t.elapsed() < Duration::from_secs(3) {
            let painted = Instant::now();
            w.frame(1000);
            std::thread::sleep(Duration::from_micros(16_667).saturating_sub(painted.elapsed()));
        }
        w.pane.camera(CameraInput::Keys(0));
        let (last, _) = w.settle();
        let stats = draw.stats();
        (
            stats.draw_ms[skip..].to_vec(),
            stats.readback_ms[skip..].to_vec(),
            last,
        )
    };
    let (draws, reads, _) = turn();
    w.pane.time_of_day(TimeOfDay::Night);
    w.settle();
    let (night_draws, night_reads, _) = turn();
    w.pane.time_of_day(TimeOfDay::Day);
    let (last, _) = w.settle();
    // Picks over a grid of the last frame.
    let mut picks = Vec::new();
    for i in 0..5 {
        for j in 0..4 {
            let at = [
                width as f32 * (i as f32 + 0.5) / 5.0,
                height as f32 * (j as f32 + 0.5) / 4.0,
            ];
            let t = Instant::now();
            w.pick(last.seq, at);
            picks.push(t.elapsed().as_secs_f64() * 1e3);
        }
    }
    // Edits of a few blocks each, as patches on the stream's palette.
    let assets = mcbench_draw::assets::Assets::load(&paths).unwrap();
    let states = mcbench_draw::blueprint::palette_states(&assets.pack, &blueprint).unwrap();
    let map: Vec<u16> = (0..states.len() as u16).collect();
    let mut edited = vec![blueprint.clone()];
    edited.extend(mcbench_draw::blueprint::edits_for_tests(&blueprint, 7, 10));
    let mut to_frame = Vec::new();
    let mut cells = Vec::new();
    for (k, pair) in edited.windows(2).enumerate() {
        let edit =
            mcbench_draw::blueprint::edit(&assets.pack, &states, &map, &pair[1], &pair[0]).unwrap();
        cells.push(edit.cells);
        let installed = draw.stats().install_ms.len();
        let t = Instant::now();
        draw.live_patch(LivePatch {
            view: ViewKey("t".into()),
            revision: 1,
            tick: k as u64 + 1,
            changes: edit.patch,
            entities: Vec::new(),
            player: None,
            framing: None,
        });
        loop {
            let frame = w.frame(30_000).expect("a frame after a patch");
            if draw.stats().install_ms.len() > installed && frame.seq > last.seq {
                break;
            }
        }
        to_frame.push(t.elapsed().as_secs_f64() * 1e3);
        while w.frame(400).is_some() {}
    }
    let stats = draw.stats();
    let line = |v: &[f64]| {
        let mut v = v.to_vec();
        v.sort_by(f64::total_cmp);
        let at = |q: f64| v[((v.len() - 1) as f64 * q).round() as usize];
        format!(
            "n {} median {:.2} p95 {:.2} max {:.2}",
            v.len(),
            at(0.5),
            at(0.95),
            at(1.0)
        )
    };
    let a = draw.report().adapter.clone();
    eprintln!(
        "adapter: {} ({}){}",
        a.name,
        a.backend,
        if a.software { ", software" } else { "" }
    );
    eprintln!("world: {} blocks, {} quads, {} tiles, world() {world_ms:.0} ms; first frame {first_ms:.0} ms after live_show; frame {:?}", report.blocks, report.quads, report.tiles, first_size);
    eprintln!("draw (start to rows mapped): {}", line(&draws));
    eprintln!("readback (rows copied out): {}", line(&reads));
    eprintln!("night draw (start to rows mapped): {}", line(&night_draws));
    eprintln!("night readback: {}", line(&night_reads));
    eprintln!("pick: {}", line(&picks));
    eprintln!(
        "patch cells {:?}; tiles meshed {:?}",
        cells,
        stats.meshed.iter().map(Vec::len).collect::<Vec<_>>()
    );
    eprintln!("patch World::patch: {}", line(&stats.apply_ms));
    eprintln!("patch meshing: {}", line(&stats.mesh_ms));
    eprintln!(
        "patch tiles written (render thread): {}",
        line(&stats.install_ms)
    );
    eprintln!("live_patch to its frame: {}", line(&to_frame));
}

// The core's flow for a turn's new revision (ui-seam-viewer.md, Edits and
// ticks reach the renderer without the window): the shown world, patched
// by ticks, is the base of the upload under its own name, and see_build
// draws it as it is shown; it cannot be dropped while shown.
#[test]
fn the_shown_world_is_a_base_draws_shots_and_is_kept() {
    let Some(mut s) = setup("the_shown_world_is_a_base_draws_shots_and_is_kept") else {
        return;
    };
    let w = &s.window;
    w.settle();
    for p in pull_lever(&mut s.session, "c", 1) {
        s.draw.live_patch(p);
    }
    w.settle();
    // A shot of the shown world draws it with the ticks' changes: as the
    // same world uploaded fresh, whole and clipped.
    let shot = |clip: &str| -> mcbench_render::Shot {
        serde_json::from_str(&format!(r#"{{"width":96,"height":64,"camera":{{"yaw":0.6,"pitch":0.5,"focus":[3,1,0],"focusDist":9,"fov":70}},"clip":{clip},"grid":null}}"#)).unwrap()
    };
    let shots = [shot("null"), shot(r#"{"min":[0,0,-2],"max":[4,1,2]}"#)];
    let png = |draw: &Draw, name: &str| -> Vec<Vec<u8>> {
        draw.shots(
            name,
            &shots,
            2,
            mcbench_render::PictureFormat::Png,
            &std::sync::atomic::AtomicBool::new(false),
        )
        .unwrap()
        .into_iter()
        .map(|p| p.bytes)
        .collect()
    };
    let shown = png(&s.draw, "live:c");
    let mut again = engine().unwrap();
    again.place(COMMANDS, true).unwrap();
    pull_lever(&mut again, "c", 1);
    let lit = again.place("", false).unwrap();
    s.draw
        .world(WorldUpload {
            name: "lit".into(),
            y_offset: Y_OFFSET,
            blueprint: lit,
            entities: None,
            base: None,
        })
        .unwrap();
    assert!(
        png(&s.draw, "lit") == shown,
        "the shown world's shots are those of the same world fresh"
    );
    // It cannot be dropped while shown.
    let refused = s.draw.drop_world("live:c").unwrap_err();
    eprintln!("drop refused: {refused:?}");
    // The next revision (the lamp's tile edited, a gold block placed) under
    // its own name with the shown world as base: meshed as an edit of the
    // world as it is, shown, and drawn as that revision fresh.
    let next = format!("{COMMANDS}setblock 6 64 0 minecraft:gold_block\n");
    let mut turn = engine().unwrap();
    let blueprint = turn.place(&next, true).unwrap();
    let report = s
        .draw
        .world(WorldUpload {
            name: "live:c".into(),
            y_offset: Y_OFFSET,
            blueprint: blueprint.clone(),
            entities: None,
            base: Some(("live:c".into(), false)),
        })
        .unwrap();
    eprintln!(
        "the next revision: {} of {} tiles meshed",
        report.tiles_meshed.unwrap(),
        report.tiles
    );
    assert!(
        report.tiles_meshed.unwrap() < report.tiles,
        "an edit of the shown world, not a fresh read"
    );
    s.draw.live_show(Some(Shown {
        world: "live:c".into(),
        view: ViewKey("c".into()),
        revision: 2,
        bounds: Clip { min: MIN, max: MAX },
        framing: Framing::Keep,
    }));
    let (edited, _) = w.settle();
    assert_eq!(edited.revision, 2);
    s.draw
        .world(WorldUpload {
            name: "fresh".into(),
            y_offset: Y_OFFSET,
            blueprint,
            entities: None,
            base: None,
        })
        .unwrap();
    s.draw.live_show(Some(Shown {
        world: "fresh".into(),
        view: ViewKey("c".into()),
        revision: 3,
        bounds: Clip { min: MIN, max: MAX },
        framing: Framing::Keep,
    }));
    let (fresh, _) = w.settle();
    assert!(
        fresh.bgra() == edited.bgra(),
        "{} pixels differ between the edited shown world and the revision fresh",
        differing(&fresh, &edited).len()
    );
    // No longer shown, the first world can go.
    assert!(s.draw.drop_world("live:c").unwrap());
}

// A pick far from the origin answers its block exactly: the hit quad's own
// block, not a position the GPU interpolated.
#[test]
fn a_pick_far_from_the_origin_is_exact() {
    const AT: i32 = 3_000_000;
    let commands = format!(
        "fill {} 63 {} {} 63 {} minecraft:stone\nsetblock {} 64 {} minecraft:redstone_lamp\n",
        AT - 8,
        AT - 8,
        AT + 8,
        AT + 8,
        AT + 1,
        AT + 2
    );
    let Some(s) = setup_with(
        "a_pick_far_from_the_origin_is_exact",
        &commands,
        [AT - 8, 63, AT - 8],
        [AT + 8, 64, AT + 8],
    ) else {
        return;
    };
    let w = &s.window;
    w.settle();
    let lamp = [AT + 1, 64, AT + 2];
    w.pane.camera(CameraInput::Frame {
        min: lamp,
        max: lamp,
        ms: 0,
    });
    let top = [lamp[0] as f64 + 0.5, 65.0, lamp[2] as f64 + 0.5];
    let floor = [(AT - 5) as f64 + 0.5, 64.0, (AT + 4) as f64 + 0.5];
    w.pane.overlays(Overlays {
        anchors: vec![top, floor],
        ..Default::default()
    });
    let (frame, _) = w.settle();
    for (i, block) in [(0, lamp), (1, [AT - 5, 63, AT + 4])] {
        let pick = w.pick(
            frame.seq,
            pane_at(&frame, s.size, frame.anchors[i].unwrap()),
        );
        assert_eq!(
            (pick.block, pick.face),
            (Some(block), Some([0, 1, 0])),
            "anchor {i}"
        );
    }
}

// The packed block a pick reads back is exact over all of Minecraft's world.
#[test]
fn a_quads_block_packs_exactly_to_the_world_border() {
    for x in [-30_000_000, -29_999_999, -1, 0, 1, 16_777_217, 29_999_999] {
        for y in [-64, 0, 319] {
            let tile = (
                mcbench_renderer::mesh::cell(x),
                mcbench_renderer::mesh::cell(-x),
            );
            let b = [x, y, -x];
            assert_eq!(
                mcbench_draw::scene::unpack_block(mcbench_draw::scene::pack_block(b, tile), tile),
                b
            );
        }
    }
}

// The map camera's target height (BlueMap's terrainHeightAt): the first
// drawn front face a ray from y 300 meets, whatever its texture, the
// translucent glass included; none past the drawn world.
#[test]
fn the_target_height_is_the_drawn_surface_under_it() {
    let Some(s) = setup("the_target_height_is_the_drawn_surface_under_it") else {
        return;
    };
    s.window.settle();
    assert_eq!(s.draw.terrain(-30.5, -30.5), Some(64.0), "the floor's top");
    assert_eq!(s.draw.terrain(3.5, 0.5), Some(65.0), "the lamp's top");
    assert_eq!(s.draw.terrain(-19.5, -19.5), Some(65.0), "the glass's top");
    assert_eq!(s.draw.terrain(60.5, 60.5), None, "past the world");
}

// Entities from each tick's list glide between ticks: a frame drawn between
// two lists draws the falling block between where they put it.
#[test]
fn a_tick_lists_entity_glides_between_ticks() {
    let commands = "fill -8 63 -8 8 63 8 minecraft:stone\nsetblock 0 90 0 minecraft:sand\n";
    let Some(mut s) = setup_with(
        "a_tick_lists_entity_glides_between_ticks",
        commands,
        [-8, 63, -8],
        [8, 90, 8],
    ) else {
        return;
    };
    let w = &s.window;
    w.settle();
    let mut ys = Vec::new();
    for tick in 1..=12u64 {
        let ticked = s.session.tick(1, true).unwrap();
        let list: Vec<serde_json::Value> = serde_json::from_slice(&ticked.entities).unwrap();
        if let Some(e) = list.first() {
            ys.push(e["pos"][1].as_f64().unwrap() - Y_OFFSET as f64);
        }
        s.draw.live_patch(LivePatch {
            view: ViewKey("c".into()),
            revision: 1,
            tick,
            changes: ticked.changes,
            entities: ticked.entities,
            player: None,
            framing: None,
        });
        // The tick clock's 50 ms, painted at 60 Hz meanwhile.
        let t = Instant::now();
        while t.elapsed() < Duration::from_millis(50) {
            let painted = Instant::now();
            w.frame(20);
            std::thread::sleep(Duration::from_micros(16_667).saturating_sub(painted.elapsed()));
        }
    }
    let drawn: Vec<f64> = s
        .draw
        .stats()
        .entities
        .iter()
        .filter_map(|(_, p)| p.first().map(|p| p[1]))
        .collect();
    eprintln!("the falling block at its ticks: {ys:?}; drawn at: {drawn:?}");
    assert!(
        ys.len() >= 3 && ys.windows(2).any(|w| w[1] < w[0]),
        "the block falls over the ticks: {ys:?}"
    );
    let between = drawn
        .iter()
        .filter(|&&y| {
            ys.windows(2)
                .any(|w| y < w[0].max(w[1]) - 1e-9 && y > w[0].min(w[1]) + 1e-9)
        })
        .count();
    assert!(
        between > 0,
        "frames between two ticks draw the block between them"
    );
}

// Night is Minecraft's: the open floor goes dark, the lit lamp stays
// bright (its block light keeps the lightmap's ratio near 1), the sky is the
// client's night sky; a frame says the time it was drawn in. see_build's
// pictures are the same bytes whatever the pane shows.
#[test]
fn night_darkens_the_open_world_and_leaves_see_builds_pictures_as_they_are() {
    let Some(mut s) =
        setup("night_darkens_the_open_world_and_leaves_see_builds_pictures_as_they_are")
    else {
        return;
    };
    let w = &s.window;
    w.settle();
    for p in pull_lever(&mut s.session, "c", 1) {
        s.draw.live_patch(p);
    }
    w.settle();
    w.pane.camera(CameraInput::Frame {
        min: LAMP,
        max: LAMP,
        ms: 0,
    });
    let lamp_top = [
        LAMP[0] as f64 + 0.5,
        LAMP[1] as f64 + 1.0,
        LAMP[2] as f64 + 0.5,
    ];
    let floor = [-3.5, 64.0, 3.5];
    w.pane.overlays(Overlays {
        anchors: vec![lamp_top, floor],
        ..Default::default()
    });
    let (day, _) = w.settle();
    let shot: mcbench_render::Shot = serde_json::from_str(r#"{"width":96,"height":64,"camera":{"yaw":0.6,"pitch":0.5,"focus":[3,1,0],"focusDist":9,"fov":70},"clip":null,"grid":null}"#).unwrap();
    let png = || -> Vec<u8> {
        s.draw
            .shots(
                "live:c",
                std::slice::from_ref(&shot),
                2,
                mcbench_render::PictureFormat::Png,
                &std::sync::atomic::AtomicBool::new(false),
            )
            .unwrap()
            .remove(0)
            .bytes
    };
    let before = png();
    w.pane.time_of_day(TimeOfDay::Night);
    let (night, _) = w.settle();
    assert_eq!((day.time, night.time), (TimeOfDay::Day, TimeOfDay::Night));
    assert!(
        png() == before,
        "see_build's picture is the same bytes at night"
    );
    let luma = |f: &Frame, at: [f32; 2]| {
        let i = ((at[1] as u32 * f.size[0] + at[0] as u32) * 4) as usize;
        let p = &f.bgra()[i..i + 3];
        0.114 * p[0] as f64 + 0.587 * p[1] as f64 + 0.299 * p[2] as f64
    };
    let (lamp, open) = (day.anchors[0].unwrap(), day.anchors[1].unwrap());
    let (lamp_day, lamp_night, open_day, open_night) = (
        luma(&day, lamp),
        luma(&night, lamp),
        luma(&day, open),
        luma(&night, open),
    );
    eprintln!("luma: the lit lamp {lamp_day:.1} by day, {lamp_night:.1} by night; the open floor {open_day:.1} by day, {open_night:.1} by night");
    assert!(open_night < 0.5 * open_day, "the open floor goes dark");
    assert!(lamp_night > 0.75 * lamp_day, "the lit lamp stays bright");
    // The top left corner is sky: Minecraft's night sky there, near black.
    w.pane.camera(CameraInput::Mode(CameraMode::Spectator));
    let (level, _) = w.settle();
    assert!(luma(&level, [2.0, 2.0]) < 30.0, "the night sky");
}

// Minecraft's own night of night-capture's reference scene (MCBENCH_NIGHT_REFERENCE:
// a folder with scene.blueprint and night-reference-6000.png and -18000.png),
// drawn at its camera, compared by the plan's picture statistics: the sky
// against the reference at 18000; the world against the native day times
// the reference's 18000 over its 6000 (the night is a ratio on see_build's
// day); and the whole picture against 18000. Writes the frames to the
// tests' temporary folder. Run by hand: cargo test --release --test live -- --ignored --nocapture night_reference
#[test]
#[ignore]
fn night_reference() {
    use mcbench_draw::compare::{self, Image};
    let folder = std::path::PathBuf::from(
        std::env::var_os("MCBENCH_NIGHT_REFERENCE").expect("MCBENCH_NIGHT_REFERENCE"),
    );
    let data = common::data("night_reference").expect("MCBENCH_DRAW_DATA");
    let draw = Draw::open(
        Paths::of_data(&data).unwrap(),
        common::choice(),
        1 << 30,
        STAGE,
    )
    .unwrap();
    let w = Window::new(draw.live());
    // reference.py's camera: eye (8.5, 68.5, -5.5) toward (10, 64.5, 10),
    // Minecraft's yaw and pitch, 70 degrees, 960 x 540.
    let (eye, target) = ([8.5f64, 68.5, -5.5], [10.0f64, 64.5, 10.0]);
    let (dx, dy, dz) = (target[0] - eye[0], target[1] - eye[1], target[2] - eye[2]);
    let yaw = -dx.atan2(dz).to_degrees();
    let pitch = (-dy).atan2(dx.hypot(dz)).to_degrees();
    let size = PaneSize {
        width: 960,
        height: 540,
        scale: 1.0,
    };
    w.pane.resize(size);
    let frames = |name: &str, blueprint: Vec<u8>| -> (Frame, Frame) {
        draw.world(WorldUpload {
            name: name.into(),
            y_offset: Y_OFFSET,
            blueprint: blueprint.clone(),
            entities: None,
            base: None,
        })
        .unwrap();
        draw.live_show(Some(Shown {
            world: name.into(),
            view: ViewKey(name.into()),
            revision: 1,
            bounds: Clip {
                min: [0, 62, 0],
                max: [15, 67, 15],
            },
            framing: Framing::Keep,
        }));
        w.pane.camera(CameraInput::Mode(CameraMode::Player));
        let mut n = [0.0; 28];
        n[0..3].copy_from_slice(&[eye[0], eye[1] - 1.62, eye[2]]);
        n[3..6].copy_from_slice(&[eye[0], eye[1] - 1.62, eye[2]]);
        n[9] = yaw;
        n[10] = pitch;
        n[11] = yaw;
        n[12] = pitch;
        n[13] = 0.6;
        n[14] = 1.8;
        n[15] = 1.62;
        n[21] = 1.0;
        let view = mcbench_render::live::PlayerView::decode(&n).unwrap();
        draw.live_patch(LivePatch {
            view: ViewKey(name.into()),
            revision: 1,
            tick: 1,
            changes: still_patch(&blueprint),
            entities: Vec::new(),
            player: Some(view),
            framing: None,
        });
        w.pane.time_of_day(TimeOfDay::Day);
        let (day, _) = w.settle();
        w.pane.time_of_day(TimeOfDay::Night);
        let (night, _) = w.settle();
        (day, night)
    };
    let (day, night) = frames(
        "scene",
        std::fs::read(folder.join("scene.blueprint")).unwrap(),
    );
    // The same camera over nothing but a block behind it: the sky alone.
    let mut lone = engine().unwrap();
    let (empty_day, _) = frames(
        "empty",
        lone.place("setblock 8 60 -30 minecraft:stone\n", true)
            .unwrap(),
    );
    let rgb = |f: &Frame| -> Vec<u8> {
        f.bgra()
            .chunks(4)
            .flat_map(|p| [p[2], p[1], p[0]])
            .collect()
    };
    let save = |f: &Frame, name: &str| {
        let path = std::env::temp_dir().join(name);
        image::save_buffer(
            &path,
            &rgb(f),
            f.size[0],
            f.size[1],
            image::ExtendedColorType::Rgb8,
        )
        .unwrap();
        eprintln!("wrote {}", path.display());
    };
    save(&day, "native-6000.png");
    save(&night, "native-18000.png");
    let (dayi, nighti) = (
        Image::from_rgb(&rgb(&day), 960, 540),
        Image::from_rgb(&rgb(&night), 960, 540),
    );
    let sky: Vec<bool> = day
        .bgra()
        .chunks(4)
        .zip(empty_day.bgra().chunks(4))
        .map(|(a, b)| a == b)
        .collect();
    let world: Vec<bool> = sky.iter().map(|s| !s).collect();
    for prefix in ["night", "swiftshader"] {
        let r6 = Image::open(&folder.join(format!("{prefix}-reference-6000.png"))).unwrap();
        let r18 = Image::open(&folder.join(format!("{prefix}-reference-18000.png"))).unwrap();
        // The native day times the reference's own night over its day.
        let expected = Image {
            w: 960,
            h: 540,
            rgb: dayi
                .rgb
                .iter()
                .zip(&r6.rgb)
                .zip(&r18.rgb)
                .map(|((d, a), b)| {
                    [0, 1, 2].map(|k| (d[k] * b[k] / a[k].max(1.0)).round().min(255.0))
                })
                .collect(),
        };
        let sky_stats = compare::colour(&nighti, &r18, Some(&sky)).unwrap();
        let world_stats = compare::colour(&nighti, &expected, Some(&world)).unwrap();
        let whole = compare::colour(&nighti, &r18, None).unwrap();
        eprintln!("{prefix}: sky against the reference at 18000 ({} pixels): {sky_stats:?}, within the plan's thresholds: {}", sky.iter().filter(|s| **s).count(), sky_stats.across_backends());
        eprintln!("{prefix}: world against the native day times the reference's ratio: {world_stats:?}, within: {}", world_stats.across_backends());
        eprintln!(
            "{prefix}: the whole picture against the reference at 18000: {whole:?}, within: {}",
            whole.across_backends()
        );
    }
}

// A tick's patch that changes no block of a blueprint's world: no palette
// entry added, its bounds and built chunk columns as they are, no block.
fn still_patch(blueprint: &[u8]) -> Vec<u8> {
    let int = |at: usize| i32::from_be_bytes(blueprint[at..at + 4].try_into().unwrap());
    let utf = |at: usize| at + 2 + u16::from_be_bytes([blueprint[at], blueprint[at + 1]]) as usize;
    let mut at = 4;
    for _ in 0..int(0) {
        at = utf(at);
        let n = int(at);
        at += 4;
        for _ in 0..n {
            at = utf(utf(at));
        }
    }
    let bounds: Vec<i32> = (0..6).map(|k| int(at + 4 * k)).collect();
    at += 24;
    let mut columns = std::collections::BTreeSet::new();
    let count = int(at);
    at += 4;
    for _ in 0..count {
        columns.insert((int(at), int(at + 8)));
        at += 12 + 8192;
    }
    let mut out = Vec::new();
    out.extend_from_slice(&0i32.to_be_bytes());
    for b in bounds {
        out.extend_from_slice(&b.to_be_bytes());
    }
    out.extend_from_slice(&(columns.len() as i32).to_be_bytes());
    out.extend_from_slice(&0i32.to_be_bytes());
    out
}

// The app's quit (mcbench_render::Renderer::shutdown): with a world shown
// and frames drawn, the render thread stops and lets the device go before
// the call returns, so nothing calls into the graphics driver while the
// process exits; after it, requests answer the sentence and the live view's
// calls do nothing.
#[test]
fn shutdown_stops_the_render_thread_and_later_calls_do_nothing() {
    let Some(s) = setup("shutdown_stops_the_render_thread_and_later_calls_do_nothing") else {
        return;
    };
    s.window.settle();
    // A frame on its way when the quit comes.
    s.window.pane.camera(CameraInput::Wheel {
        delta: 3.0,
        lines: true,
    });
    assert!(s.draw.running());
    s.draw.shutdown();
    assert!(!s.draw.running(), "the render thread has ended");
    let failed = s
        .draw
        .world(WorldUpload {
            name: "after".into(),
            y_offset: Y_OFFSET,
            blueprint: Vec::new(),
            entities: None,
            base: None,
        })
        .unwrap_err();
    assert!(
        matches!(&failed, mcbench_render::RenderError::Failed(s) if s == mcbench_draw::live::SHUT_DOWN),
        "{failed:?}"
    );
    s.draw.live_show(None);
    s.draw.live_patch(LivePatch {
        view: ViewKey("c".into()),
        revision: 2,
        tick: 1,
        changes: Vec::new(),
        entities: Vec::new(),
        player: None,
        framing: None,
    });
    s.window.pane.resize(PaneSize {
        width: 10,
        height: 10,
        scale: 1.0,
    });
    s.draw.shutdown();
}

// What the renderer holds with a world shown (live::Accounting): the block
// atlas's pixels only on the GPU, and the device allocator's blocks sized
// for what it holds (wgpu's MemoryHints::MemoryUsage). With the default
// hints the first readback buffer took a 128 MiB host block on NVIDIA's
// Vulkan driver, all of it resident, and the device reserved 256 MiB for
// this world.
#[test]
fn the_renderer_holds_no_atlas_pixels_and_small_allocator_blocks() {
    let Some(s) = setup("the_renderer_holds_no_atlas_pixels_and_small_allocator_blocks") else {
        return;
    };
    s.window.settle();
    let a = s.draw.accounting();
    eprintln!("{a:?}");
    assert_eq!(
        a.atlas_cpu, 0,
        "the atlas's pixels are let go once uploaded"
    );
    assert!(a.atlas_gpu > 0);
    if let Some(device) = a.device {
        assert!(
            device < 128 << 20,
            "the device reserves {device} bytes for one small world"
        );
    }
}
