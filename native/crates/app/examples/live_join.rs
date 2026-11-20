//! The live view through the app's core and the real renderer, without a
//! window:
//!
//!   live_join <data folder> <bundle.tar.zst> <its lock file> <mcbench-lite> <out folder>
//!
//! Starts the core as the app's program does (the app's own backend: setup
//! with the bundle unpacked as the shipped data, the textures fetched and
//! baked unless the data folder has them, `<mcbench-lite>` as the engine
//! children), with the live view's renderer made by the app's own start,
//! `mcbench_lite::core::renderer::draw` (`mcbench_draw::live::Draw` on the
//! hardware adapter). The one difference: the `Draw` is wrapped in a
//! renderer that passes every call through and keeps a copy of the worlds
//! and patches the core hands it, so the program can read the lamp's state
//! in the tick's patch.
//!
//! It makes a chat whose world is a stone floor with a lever wired to a
//! redstone lamp (through the store, as a turn's build sets blocks), shows
//! it (`Live(Show)`), takes frames from the pane handle the core published
//! (`Update::LivePane(Ok)`), writes the first as `before.png` in the out
//! folder, clicks the lever through the pane's pick and the core's
//! `Viewer(Click)`, waits for `Clicked` with the input taken and for the
//! tick's patch that lights the lamp, takes frames until the lamp's pixels
//! are brighter and writes `after.png`, then calls `see_build` through the
//! core's tools (the same `Draw` draws its two default views, written as
//! `see-1.jpg` and `see-2.jpg`), then stops the core. It waits for
//! conditions, each with a bound, and never sleeps to let something
//! happen. Any failure ends it with a panic and status 101.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

use mcbench_lite::core::live::LiveParts;
use mcbench_lite::core::{Core, Parts};
use mcbench_render::live::{
    CameraInput, Frame, LivePane, LivePatch, LiveRenderer, Overlays, PaneSize, Pick, Shown, ViewKey,
};
use mcbench_render::{
    AdapterChoice, AdapterReport, Assets, Cell, Picture, PictureFormat, RenderError, Renderer,
    Shot, WorldReport, WorldUpload,
};
use mcbench_ui::{Command, LiveCommand, LivePhase, Running, Update, ViewerInput, Window};

/// Every wait's bound.
const WAIT: Duration = Duration::from_secs(120);
/// The lever and the lamp, Minecraft's coordinates (build y 1 is 65).
const LEVER: [i32; 3] = [1, 65, 1];
const LAMP: [i32; 3] = [3, 65, 1];

/// The real renderer, with a copy of the blueprints and patches it is
/// handed.
struct Recorded {
    draw: mcbench_draw::live::Draw,
    seen: Mutex<Seen>,
    changed: Condvar,
}

#[derive(Default)]
struct Seen {
    /// The last world kept, by name: its blueprint.
    worlds: Vec<(String, Vec<u8>)>,
    patches: Vec<LivePatch>,
    /// The worlds `shots` drew, in order.
    shot: Vec<String>,
}

impl LiveRenderer for Recorded {
    fn live(&self) -> Arc<dyn LivePane> {
        self.draw.live()
    }
    fn live_show(&self, shown: Option<Shown>) {
        self.draw.live_show(shown)
    }
    fn live_patch(&self, patch: LivePatch) {
        self.seen.lock().unwrap().patches.push(patch.clone());
        self.changed.notify_all();
        self.draw.live_patch(patch)
    }
}

impl Renderer for Recorded {
    fn start(_: &Assets, _: AdapterChoice, _: u64) -> Result<Self, RenderError> {
        unreachable!("made from the app's start")
    }
    fn report(&self) -> &AdapterReport {
        self.draw.report()
    }
    fn world(&self, upload: WorldUpload) -> Result<WorldReport, RenderError> {
        let kept = (upload.name.clone(), upload.blueprint.clone());
        let report = self.draw.world(upload)?;
        self.seen.lock().unwrap().worlds.push(kept);
        self.changed.notify_all();
        Ok(report)
    }
    fn shots(
        &self,
        world: &str,
        shots: &[Shot],
        supersample: u32,
        format: PictureFormat,
        cancel: &AtomicBool,
    ) -> Result<Vec<Picture>, RenderError> {
        self.seen.lock().unwrap().shot.push(world.to_string());
        self.draw.shots(world, shots, supersample, format, cancel)
    }
    fn montage(
        &self,
        size: [u32; 2],
        cells: &[Cell],
        jpegs: &[&[u8]],
        quality: u8,
    ) -> Result<Vec<u8>, RenderError> {
        self.draw.montage(size, cells, jpegs, quality)
    }
    fn drop_world(&self, world: &str) -> Result<bool, RenderError> {
        self.draw.drop_world(world)
    }
    fn shutdown(&self) {
        self.draw.shutdown()
    }
}

/// The window's side of the core: every update, in order.
#[derive(Default)]
struct Recording {
    updates: Mutex<Vec<Update>>,
    arrived: Condvar,
}

impl Window for Recording {
    fn publish(&self, update: Update) {
        self.updates.lock().unwrap().push(update);
        self.arrived.notify_all();
    }
}

impl Recording {
    /// Blocks until an update from `from` on satisfies `found`, and gives it.
    fn wait<T>(&self, what: &str, from: usize, found: impl Fn(&Update) -> Option<T>) -> T {
        let deadline = Instant::now() + WAIT;
        let mut updates = self.updates.lock().unwrap();
        loop {
            if let Some(t) = updates[from..].iter().find_map(&found) {
                return t;
            }
            for update in &updates[from..] {
                match update {
                    Update::Live(state) => {
                        if let LivePhase::Failed { error, .. } = &state.phase {
                            panic!("{what}: the view failed: {error}");
                        }
                    }
                    Update::Error(error) => panic!("{what}: Update::Error({error})"),
                    Update::Setup(steps) => {
                        for step in steps.iter().filter(|s| s.id != "shell") {
                            if let mcbench_setup::StepState::Failed { error } = &step.state {
                                panic!("{what}: setup's {} failed: {error}", step.id);
                            }
                        }
                    }
                    _ => {}
                }
            }
            let now = Instant::now();
            if now >= deadline {
                let lives: Vec<&Update> = updates
                    .iter()
                    .filter(|u| matches!(u, Update::Live(_) | Update::LivePane(_)))
                    .collect();
                panic!("{what}: not within {WAIT:?}; the live updates: {lives:?}");
            }
            updates = self
                .arrived
                .wait_timeout(updates, deadline - now)
                .unwrap()
                .0;
        }
    }
    fn len(&self) -> usize {
        self.updates.lock().unwrap().len()
    }
}

/// The pane as a window holds it: frames taken when the waker says one is
/// waiting.
struct Pane {
    pane: Arc<dyn LivePane>,
    wakes: Arc<(Mutex<u64>, Condvar)>,
}

impl Pane {
    fn new(pane: Arc<dyn LivePane>) -> Pane {
        let wakes = Arc::new((Mutex::new(0u64), Condvar::new()));
        let w = Arc::clone(&wakes);
        pane.set_waker(Box::new(move || {
            *w.0.lock().unwrap() += 1;
            w.1.notify_all();
        }));
        Pane { pane, wakes }
    }

    /// The next frame within `within`, or none.
    fn frame(&self, within: Duration) -> Option<Frame> {
        let deadline = Instant::now() + within;
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

    /// Frames until the view is still: the last of them, once no frame
    /// came for 400 ms.
    fn settle(&self) -> Frame {
        let mut last = self
            .frame(WAIT)
            .unwrap_or_else(|| panic!("a frame: {:?}", self.pane.status()));
        let start = Instant::now();
        while let Some(frame) = self.frame(Duration::from_millis(400)) {
            last = frame;
            assert!(start.elapsed() < WAIT, "the view settles");
        }
        last
    }

    fn pick(&self, seq: u64, at: [f32; 2]) -> Pick {
        let (tx, rx) = std::sync::mpsc::channel();
        self.pane
            .pick(seq, at, Box::new(move |p| tx.send(p).unwrap()));
        rx.recv_timeout(WAIT).expect("a pick is answered")
    }
}

/// A frame's size, its distinct colours and the sha256 of its pixels.
fn describe(frame: &Frame) -> (usize, String) {
    let colours: HashSet<&[u8]> = frame.bgra().chunks(4).collect();
    let hash = Sha256::digest(frame.bgra());
    let hex: String = hash.iter().map(|b| format!("{b:02x}")).collect();
    (colours.len(), hex)
}

fn write_png(frame: &Frame, to: &std::path::Path) {
    let file = std::io::BufWriter::new(std::fs::File::create(to).expect("the PNG's file"));
    let mut encoder = png::Encoder::new(file, frame.size[0], frame.size[1]);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let rgba: Vec<u8> = frame
        .bgra()
        .chunks(4)
        .flat_map(|p| [p[2], p[1], p[0], p[3]])
        .collect();
    let mut writer = encoder.write_header().expect("the PNG's header");
    writer.write_image_data(&rgba).expect("the PNG's pixels");
}

/// The corners of a block (Minecraft's coordinates).
fn corners(b: [i32; 3]) -> Vec<[f64; 3]> {
    (0..8)
        .map(|i| {
            [
                (b[0] + (i & 1)) as f64,
                (b[1] + ((i >> 1) & 1)) as f64,
                (b[2] + ((i >> 2) & 1)) as f64,
            ]
        })
        .collect()
}

/// The pixel box of projected points.
fn rect(points: &[Option<[f32; 2]>]) -> [f32; 4] {
    let p: Vec<[f32; 2]> = points
        .iter()
        .map(|p| p.expect("in front of the eye"))
        .collect();
    [
        p.iter().map(|q| q[0]).fold(f32::MAX, f32::min),
        p.iter().map(|q| q[1]).fold(f32::MAX, f32::min),
        p.iter().map(|q| q[0]).fold(f32::MIN, f32::max),
        p.iter().map(|q| q[1]).fold(f32::MIN, f32::max),
    ]
}

/// The summed red, green and blue of a frame's pixels inside `r`.
fn luma(frame: &Frame, r: [f32; 4]) -> u64 {
    let w = frame.size[0];
    frame
        .bgra()
        .chunks(4)
        .enumerate()
        .filter(|(i, _)| {
            let (x, y) = ((*i as u32 % w) as f32 + 0.5, (*i as u32 / w) as f32 + 0.5);
            x >= r[0] && x <= r[2] && y >= r[1] && y <= r[3]
        })
        .map(|(_, p)| p[0] as u64 + p[1] as u64 + p[2] as u64)
        .sum()
}

/// A blueprint's or a patch's palette entries: id and properties.
type Entry = (String, Vec<(String, String)>);

struct Bytes<'a>(&'a [u8], usize);

impl Bytes<'_> {
    fn take(&mut self, n: usize) -> &[u8] {
        let taken = &self.0[self.1..self.1 + n];
        self.1 += n;
        taken
    }
    fn int(&mut self) -> i32 {
        i32::from_be_bytes(self.take(4).try_into().unwrap())
    }
    fn short(&mut self) -> u16 {
        u16::from_be_bytes(self.take(2).try_into().unwrap())
    }
    fn utf(&mut self) -> String {
        let n = self.short() as usize;
        String::from_utf8(self.take(n).to_vec()).unwrap()
    }
    fn entries(&mut self) -> Vec<Entry> {
        (0..self.int())
            .map(|_| {
                let id = self.utf();
                let properties = (0..self.int()).map(|_| (self.utf(), self.utf())).collect();
                (id, properties)
            })
            .collect()
    }
}

/// The palette entry a patch sets at `pos`, reading the patch format the
/// engine writes (`core/src/engine.rs` `Ticked.changes`, read by the
/// mesher's `World::patch`): the entries it adds to `palette`, the bounds
/// and built-column count (28 bytes), then each changed block.
fn set_at(palette: &mut Vec<Entry>, changes: &[u8], pos: [i32; 3]) -> Option<Entry> {
    let mut bytes = Bytes(changes, 0);
    palette.extend(bytes.entries());
    bytes.take(28);
    let mut found = None;
    for _ in 0..bytes.int() {
        let at = [bytes.int(), bytes.int(), bytes.int()];
        let index = bytes.short() as usize;
        if at == pos {
            found = Some(palette[index].clone());
        }
    }
    assert_eq!(bytes.1, changes.len(), "the patch is read to its end");
    found
}

/// Sends a live command and waits for the live world to be `running`.
fn run_live(core: &Core, window: &Recording, command: LiveCommand, running: Running) {
    let from = window.len();
    let what = format!("{command:?}");
    mcbench_ui::Core::send(core, Command::Live(command));
    window.wait(&what, from, |u| match u {
        Update::Live(state) if state.running.as_ref() == Some(&running) => Some(()),
        _ => None,
    });
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [data, bundle, lock, program, out] = args.as_slice() else {
        eprintln!(
            "usage: live_join <data folder> <bundle.tar.zst> <its lock file> <mcbench-lite> <out folder>"
        );
        std::process::exit(2)
    };
    let (data, out) = (PathBuf::from(data), PathBuf::from(out));
    std::fs::create_dir_all(&out).expect("the out folder");
    let bundle = std::fs::read(bundle).expect("the bundle");
    let lock = mcbench_setup::bundle::Lock::parse(&std::fs::read_to_string(lock).unwrap()).unwrap();
    let recorded: Arc<Mutex<Option<Arc<Recorded>>>> = Arc::default();
    let kept = Arc::clone(&recorded);
    let folder = data.clone();
    let started = Instant::now();
    let core = Core::start(Parts {
        data: data.clone(),
        program: PathBuf::from(program),
        host: Arc::new(mcbench_store::SystemHost),
        shipped: Box::new(move || mcbench_setup::Data::unpack(&bundle, &lock, &folder)),
        client: mcbench_setup::fetch::client(),
        backend: None,
        installer: None,
        share: None,
        live: LiveParts {
            renderer: Some(Box::new(move |assets| {
                let draw = mcbench_lite::core::renderer::draw(assets)
                    .map_err(mcbench_lite::core::renderer::sentence)?;
                let recorded = Arc::new(Recorded {
                    draw,
                    seen: Mutex::default(),
                    changed: Condvar::new(),
                });
                *kept.lock().unwrap() = Some(Arc::clone(&recorded));
                Ok(recorded as Arc<dyn Renderer>)
            })),
            ..LiveParts::default()
        },
    })
    .expect("the core starts");
    let window = Arc::new(Recording::default());
    mcbench_ui::Core::attach(&core, window.clone());

    // The renderer's start, once setup has the assets.
    let pane = window.wait("Update::LivePane", 0, |u| match u {
        Update::LivePane(pane) => Some(pane.clone()),
        _ => None,
    });
    let pane = Pane::new(
        pane.unwrap_or_else(|error| panic!("the renderer did not start: {error}"))
            .0,
    );
    let recorded = recorded.lock().unwrap().take().expect("the renderer made");
    let report = recorded.report();
    println!(
        "renderer: {} ({}, {}, software {}) in {:.1} s from the core's start",
        report.adapter.name,
        report.adapter.backend,
        report.adapter.device_type,
        report.adapter.software,
        started.elapsed().as_secs_f64()
    );

    // The chat: a stone floor 6 by 6, a lever, dust and a lamp.
    let handle = core.handle().clone();
    let on_core = core.clone();
    let chat = handle
        .block_on(handle.spawn(async move {
            on_core
                .create_conversation("openrouter", "chat/model")
                .await
                .expect("a chat")
                .id
        }))
        .unwrap();
    {
        let live = core.store().conversation(&chat).unwrap().unwrap();
        let mut conversation = live.lock().unwrap();
        let world = &mut conversation.world;
        for x in 0..6 {
            for z in 0..6 {
                world.set_block([x, 0, z], "minecraft:stone", &[]).unwrap();
            }
        }
        let lever: Vec<(String, String)> =
            [("face", "floor"), ("facing", "north"), ("powered", "false")]
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect();
        world
            .set_block([1, 1, 1], "minecraft:lever", &lever)
            .unwrap();
        world
            .set_block([2, 1, 1], "minecraft:redstone_wire", &[])
            .unwrap();
        world
            .set_block([3, 1, 1], "minecraft:redstone_lamp", &[])
            .unwrap();
        world.finalize();
    }
    let view = ViewKey(chat.clone());
    println!("chat {chat}: a stone floor, a lever, dust and a lamp");

    // Shown, framed on the lever and the lamp.
    pane.pane.resize(PaneSize {
        width: 640,
        height: 400,
        scale: 1.0,
    });
    let from = window.len();
    mcbench_ui::Core::send(
        &core,
        Command::Live(LiveCommand::Show {
            view: Some(view.clone()),
            afresh: false,
        }),
    );
    window.wait("Live(Shown)", from, |u| match u {
        Update::Live(state) if state.phase == LivePhase::Shown => Some(()),
        _ => None,
    });
    // Paused, so the view is still while it is looked at (a playing world
    // draws a frame for every tick).
    run_live(&core, &window, LiveCommand::Pause, Running::Paused);
    pane.pane.camera(CameraInput::Frame {
        min: LEVER,
        max: LAMP,
        ms: 0,
    });
    let mut anchors = corners(LAMP);
    anchors.push([
        LEVER[0] as f64 + 0.5,
        LEVER[1] as f64 + 0.1,
        LEVER[2] as f64 + 0.5,
    ]);
    pane.pane.overlays(Overlays {
        anchors,
        ..Overlays::default()
    });
    let before = pane.settle();
    let lamp = rect(&before.anchors[..8]);
    let (colours, hash) = describe(&before);
    println!(
        "before: frame {} by {}, {colours} colours, sha256 {hash}, lamp's pixels {}",
        before.size[0],
        before.size[1],
        luma(&before, lamp)
    );
    assert!(colours > 16, "the frame is a picture, not blank");
    write_png(&before, &out.join("before.png"));

    // The lever clicked through the pick.
    let at = before.anchors[8].expect("the lever in front of the eye");
    let pick = pane.pick(before.seq, at);
    println!("pick at {at:?}: {pick:?}");
    assert_eq!(pick.block, Some(LEVER), "the pick names the lever");
    let from = window.len();
    let patches_before = recorded.seen.lock().unwrap().patches.len();
    mcbench_ui::Core::send(
        &core,
        Command::Viewer(ViewerInput::Click {
            view: view.clone(),
            revision: before.revision,
            block: LEVER,
            yaw: pick.yaw,
        }),
    );
    let (name, input) = window.wait("Clicked", from, |u| match u {
        Update::Clicked { name, input, .. } => Some((name.clone(), input.clone())),
        _ => None,
    });
    println!("clicked: {name}, input {input:?}");
    assert_eq!(
        input.as_deref(),
        Some("toggle"),
        "the lever takes the input"
    );
    run_live(&core, &window, LiveCommand::Play, Running::Playing);

    // The tick's patch that lights the lamp.
    let lit_tick = {
        let deadline = Instant::now() + WAIT;
        let mut seen = recorded.seen.lock().unwrap();
        loop {
            let world = format!("live:{chat}");
            let blueprint = &seen
                .worlds
                .iter()
                .rev()
                .find(|(name, _)| *name == world)
                .expect("the shown world kept")
                .1;
            let mut palette = Bytes(blueprint, 0).entries();
            let mut lit = None;
            for patch in &seen.patches {
                let entry = set_at(&mut palette, &patch.changes, LAMP);
                if lit.is_none()
                    && entry.is_some_and(|(id, properties)| {
                        id == "minecraft:redstone_lamp"
                            && properties.contains(&("lit".into(), "true".into()))
                    })
                {
                    lit = Some(patch.tick);
                }
            }
            if let Some(tick) = lit {
                break tick;
            }
            let now = Instant::now();
            assert!(now < deadline, "a patch lighting the lamp within {WAIT:?}");
            seen = recorded
                .changed
                .wait_timeout(seen, deadline - now)
                .unwrap()
                .0;
        }
    };
    let patches_after = recorded.seen.lock().unwrap().patches.len();
    println!(
        "the lamp is lit in tick {lit_tick}'s patch ({} patches since the click)",
        patches_after - patches_before
    );

    // A frame with the lamp brighter.
    let start = Instant::now();
    let after = loop {
        let frame = pane.frame(WAIT).expect("a frame after the patch");
        if luma(&frame, lamp) > luma(&before, lamp) {
            run_live(&core, &window, LiveCommand::Pause, Running::Paused);
            break pane.settle_from(frame);
        }
        assert!(start.elapsed() < WAIT, "the lamp brighter within {WAIT:?}");
    };
    let (colours, after_hash) = describe(&after);
    println!(
        "after: frame {} by {}, {colours} colours, sha256 {after_hash}, lamp's pixels {}",
        after.size[0],
        after.size[1],
        luma(&after, lamp)
    );
    assert_ne!(hash, after_hash, "the frame changed");
    assert!(
        luma(&after, lamp) > luma(&before, lamp),
        "the lamp is brighter"
    );
    write_png(&after, &out.join("after.png"));

    // see_build in the same process: the tools draw with the live view's
    // `Draw`, beside the world on show.
    let tools = core.tools();
    let store = Arc::clone(core.store());
    let id = chat.clone();
    let (outcome, files) = handle
        .block_on(handle.spawn(async move {
            let live = store.conversation(&id).unwrap().unwrap();
            let world = live.lock().unwrap().world.clone();
            let mut chat = mcbench_tools::Chat {
                id: id.clone(),
                model: "chat/model".into(),
                world,
                workspace: mcbench_tools::Workspace {
                    chat: id.clone(),
                    host: store.folder().root.join("sandbox").join(&id),
                },
                image_input: true,
                chat: true,
            };
            let outcome = tools
                .execute(
                    &mut chat,
                    "see_build",
                    &serde_json::Map::new(),
                    &mut |_| {},
                    &tokio_util::sync::CancellationToken::new(),
                )
                .await
                .expect("see_build answers");
            let files: Vec<PathBuf> = outcome
                .shown_images("see_build")
                .iter()
                .map(|image| store.file_path(&image.url).unwrap())
                .collect();
            (outcome, files)
        }))
        .unwrap();
    println!("see_build: {}", outcome.content("see_build"));
    assert_eq!(outcome.result["ok"], true, "see_build drew its shots");
    assert_eq!(files.len(), 2, "the two default views");
    for (index, file) in files.iter().enumerate() {
        let jpeg = std::fs::read(file).unwrap();
        assert!(jpeg.starts_with(&[0xff, 0xd8, 0xff]), "a JPEG");
        let hex: String = Sha256::digest(&jpeg)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        println!(
            "see_build picture {}: {} bytes, sha256 {hex}",
            index + 1,
            jpeg.len()
        );
        std::fs::write(out.join(format!("see-{}.jpg", index + 1)), jpeg).unwrap();
    }
    let shot = recorded.seen.lock().unwrap().shot.clone();
    println!("the live view's renderer drew see_build's shots of {shot:?}");
    assert!(
        shot.iter()
            .any(|name| name.starts_with(&format!("{chat}/tick "))),
        "see_build's shots were drawn by the live view's Draw"
    );

    // The end: the pane let go, the core stopped, and with it the `Draw`,
    // whose drop joins its render thread.
    drop(pane);
    let draw = Arc::downgrade(&recorded);
    drop(recorded);
    core.shutdown();
    assert!(
        draw.upgrade().is_none(),
        "the core's shutdown dropped the renderer (Draw's drop joins its render thread)"
    );
    println!("the core stopped and the renderer was dropped, its render thread joined");
    println!("ok");
}

impl Pane {
    /// [`Pane::settle`] after a frame already taken.
    fn settle_from(&self, first: Frame) -> Frame {
        let mut last = first;
        let start = Instant::now();
        while let Some(frame) = self.frame(Duration::from_millis(400)) {
            last = frame;
            assert!(start.elapsed() < WAIT, "the view settles");
        }
        last
    }
}
