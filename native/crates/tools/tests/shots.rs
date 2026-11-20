//! Today's tests of `see_build`'s renderer client and framing, ported:
//! `shot-context.test.js` (7: a lost device, a closed renderer, worlds kept
//! and sent again, edits and their bases), `shot-cancellation.test.js` (1),
//! `shot-recovery.test.js` (1 of 2), `shot-renderer.test.js` (2 of 9) and
//! `shot-framing.test.js` (3 of 4), over `mcbench_tools::shots` with a fake
//! renderer in place of today's fake helper (`test/helpers/fake-draw.js`):
//! it keeps worlds by name, answers `shots` with a JPEG-headed text naming
//! the world and the shot, and can lose its device, hold its shots until
//! released, forget bases or fail.
//!
//! Not ported, and why: the helper's process life (`shot-idle.test.js` 3,
//! `shot-renderer.test.js`'s stalled start, a start's lost device, close
//! killing a helper that will not exit, a crashed or exiting helper
//! restarted, the setup step's start, the memory bound given in `hello`;
//! `shot-recovery.test.js`'s helper exiting between two requests): the
//! renderer is a thread of this process, started once, never killed or
//! restarted, and its memory bound is the drawing crate's. Today's renderer
//! is made again after a lost device and holds no world; the drawing
//! crate's makes its device again itself and keeps its worlds, so a
//! retried call finds the world kept (the client still forgets what it
//! sent, as for a fresh helper). `shot-framing.test.js`'s square preview
//! is share's (not ported).

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use mcbench_render::live::{LivePane, LivePatch, LiveRenderer, Shown};
use mcbench_render::{
    AdapterChoice, AdapterReport, Assets, Cell, Clip, Picture, PictureFormat, RenderError,
    Renderer, Shot, WorldReport, WorldUpload,
};
use mcbench_tools::settle::{Changes, Scene, SettledWorld};
use mcbench_tools::shots::{
    LOST_DEVICE, SHUT_DOWN, SharedRenderer, ToDraw, montage_cells, resolve_shot,
};
use mcbench_tools::verify::ABORTED;
use mcbench_world::{BlockState, World};
use serde_json::{Map, Value, json};
use tokio_util::sync::CancellationToken;

const WAIT: Duration = Duration::from_secs(30);

/// What the fake was asked, in order.
#[derive(Clone, Debug, PartialEq)]
enum Asked {
    World {
        name: String,
        base: Option<(String, bool)>,
    },
    Shots {
        world: String,
        count: usize,
    },
    Cancelled,
}

#[derive(Default)]
struct Control {
    /// Device losses still to answer.
    losses: usize,
    /// Shots requests that find their world wait until this many are let go.
    hold: bool,
    release: usize,
    held: usize,
    /// A world with a base is answered as if the base were not kept.
    forget_bases: bool,
    /// The next shots request fails with this.
    fail: Option<String>,
    asked: Vec<Asked>,
    kept: HashSet<String>,
}

#[derive(Default)]
struct Fake {
    control: Mutex<Control>,
    changed: Condvar,
}

impl Fake {
    fn with(set: impl FnOnce(&mut Control)) -> Arc<Fake> {
        let fake = Arc::new(Fake::default());
        set(&mut fake.control.lock().unwrap());
        fake
    }
    fn asked(&self) -> Vec<Asked> {
        self.control.lock().unwrap().asked.clone()
    }
    fn worlds(&self) -> Vec<(String, Option<(String, bool)>)> {
        self.asked()
            .into_iter()
            .filter_map(|a| match a {
                Asked::World { name, base } => Some((name, base)),
                _ => None,
            })
            .collect()
    }
    fn shots(&self) -> Vec<(String, usize)> {
        self.asked()
            .into_iter()
            .filter_map(|a| match a {
                Asked::Shots { world, count } => Some((world, count)),
                _ => None,
            })
            .collect()
    }
    fn release(&self, n: usize) {
        self.control.lock().unwrap().release = n;
        self.changed.notify_all();
    }
    /// Blocks until `done` holds of what the fake was asked.
    fn until(&self, what: &str, done: impl Fn(&Control) -> bool) {
        let deadline = Instant::now() + WAIT;
        let mut control = self.control.lock().unwrap();
        while !done(&control) {
            let now = Instant::now();
            assert!(now < deadline, "{what} within {WAIT:?}");
            control = self
                .changed
                .wait_timeout(control, deadline - now)
                .unwrap()
                .0;
        }
    }
}

fn jpeg(text: &str) -> Vec<u8> {
    let mut bytes = vec![0xff, 0xd8, 0xff];
    bytes.extend(text.as_bytes());
    bytes
}

impl LiveRenderer for Fake {
    fn live(&self) -> Arc<dyn LivePane> {
        unimplemented!("see_build draws no live view")
    }
    fn live_show(&self, _: Option<Shown>) {}
    fn live_patch(&self, _: LivePatch) {}
}

impl Renderer for Fake {
    fn start(_: &Assets, _: AdapterChoice, _: u64) -> Result<Self, RenderError> {
        unimplemented!("the tools are given a started renderer")
    }
    fn report(&self) -> &AdapterReport {
        unimplemented!("the tools never read the report")
    }
    fn world(&self, upload: WorldUpload) -> Result<WorldReport, RenderError> {
        let mut control = self.control.lock().unwrap();
        control.asked.push(Asked::World {
            name: upload.name.clone(),
            base: upload.base.clone(),
        });
        self.changed.notify_all();
        if let Some((base, keep)) = &upload.base {
            if control.forget_bases || !control.kept.contains(base) {
                return Err(RenderError::UnknownWorld(format!(
                    "No world named {base} is kept"
                )));
            }
            if !keep {
                control.kept.remove(base);
            }
        }
        control.kept.insert(upload.name);
        Ok(WorldReport {
            quads: 0,
            entity_quads: 0,
            tiles: 0,
            blocks: 0,
            bounds: Clip {
                min: [0; 3],
                max: [0; 3],
            },
            bytes: 0,
            missing_textures: Vec::new(),
            tiles_meshed: None,
        })
    }
    fn shots(
        &self,
        world: &str,
        shots: &[Shot],
        _: u32,
        format: PictureFormat,
        cancel: &AtomicBool,
    ) -> Result<Vec<Picture>, RenderError> {
        assert_eq!(format, PictureFormat::Jpeg(90));
        let mut control = self.control.lock().unwrap();
        control.asked.push(Asked::Shots {
            world: world.to_string(),
            count: shots.len(),
        });
        self.changed.notify_all();
        if !control.kept.contains(world) {
            return Err(RenderError::UnknownWorld(format!(
                "No world named {world} is kept"
            )));
        }
        if let Some(error) = control.fail.take() {
            return Err(RenderError::Failed(error));
        }
        if control.losses > 0 {
            control.losses -= 1;
            return Err(RenderError::DeviceLost(
                "The graphics device was lost: a test".into(),
            ));
        }
        if control.hold {
            control.held += 1;
            let mine = control.held;
            while control.release < mine {
                if cancel.load(Ordering::SeqCst) {
                    control.asked.push(Asked::Cancelled);
                    self.changed.notify_all();
                    return Err(RenderError::Cancelled);
                }
                // The cancel flag is no condition variable's: looked at
                // every few milliseconds while held.
                control = self
                    .changed
                    .wait_timeout(control, Duration::from_millis(5))
                    .unwrap()
                    .0;
            }
        }
        Ok((0..shots.len())
            .map(|i| Picture {
                width: 1,
                height: 1,
                bytes: jpeg(&format!("{world} shot {i}")),
            })
            .collect())
    }
    fn montage(
        &self,
        _: [u32; 2],
        cells: &[Cell],
        jpegs: &[&[u8]],
        _: u8,
    ) -> Result<Vec<u8>, RenderError> {
        assert_eq!(cells.len(), jpegs.len());
        Ok(jpeg("card"))
    }
    fn drop_world(&self, _: &str) -> Result<bool, RenderError> {
        unimplemented!("see_build drops no world")
    }
    fn shutdown(&self) {
        unimplemented!("see_build never shuts the renderer down")
    }
}

/// A settled-world stand-in of the build's edition `edition`, with a scene
/// of serial `serial`.
fn settled(edition: i64, serial: u64, ticks: i64) -> Arc<SettledWorld> {
    let mut world = World::new();
    let stone = BlockState::new("minecraft:stone", &[]);
    world.set([0, 0, 0], stone.clone()).unwrap();
    world.set([1, 1, 1], stone).unwrap();
    Arc::new(SettledWorld {
        world,
        entities: Vec::new(),
        changes: Changes {
            removed: 0,
            added: 0,
            replaced: 0,
            examples: json!({}),
        },
        unsupported: Vec::new(),
        ticks,
        scene: Scene {
            serial,
            edition: (1, edition),
            blueprint: b"blueprint".to_vec(),
            y_offset: 64,
        },
    })
}

fn shot(of: &SettledWorld, view: &str) -> Shot {
    let mut spec = Map::new();
    spec.insert("view".into(), json!(view));
    resolve_shot(&spec, &of.world, &of.entities).unwrap().shot
}

/// `renderShots(conversation, ...)` of `shots`: each a settle and its view.
async fn render(
    shared: &SharedRenderer,
    conversation: &str,
    shots: &[(&Arc<SettledWorld>, &str)],
    cancel: &CancellationToken,
) -> Result<Vec<String>, String> {
    let resolved: Vec<Shot> = shots.iter().map(|(of, view)| shot(of, view)).collect();
    let to_draw: Vec<ToDraw<'_>> = shots
        .iter()
        .zip(&resolved)
        .map(|((of, _), shot)| ToDraw {
            shot,
            tick: of.ticks,
            settled: of,
        })
        .collect();
    let jpegs = shared.render(conversation, &to_draw, cancel).await?;
    Ok(jpegs
        .iter()
        .map(|j| String::from_utf8(j[3..].to_vec()).unwrap())
        .collect())
}

fn started(fake: &Arc<Fake>) -> Arc<SharedRenderer> {
    let shared = SharedRenderer::new();
    shared.set(Ok(Arc::clone(fake) as Arc<dyn Renderer>));
    shared
}

fn never() -> CancellationToken {
    CancellationToken::new()
}

// ---- shot-context.test.js ----

/// "a lost device sends the world to a fresh helper and retries the whole
/// inspection once; lost twice, the call fails"
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_lost_device_retries_the_whole_call_once_and_twice_fails() {
    let fake = Fake::with(|c| c.losses = 1);
    let shared = started(&fake);
    let of = settled(1, 1, 0);
    let images = render(
        &shared,
        "fixture",
        &[(&of, "front"), (&of, "top")],
        &never(),
    )
    .await
    .unwrap();
    assert_eq!(
        images,
        ["fixture/tick 0/1 shot 0", "fixture/tick 0/1 shot 1"]
    );
    // The world kept through the loss (the drawing crate's own recovery).
    assert_eq!(fake.worlds().len(), 1);
    assert_eq!(
        fake.shots().len(),
        3,
        "found no world, lost the device, drew"
    );

    let twice = Fake::with(|c| c.losses = 2);
    let shared = started(&twice);
    assert_eq!(
        render(
            &shared,
            "fixture",
            &[(&of, "front"), (&of, "top")],
            &never()
        )
        .await,
        Err(LOST_DEVICE.to_string())
    );
    assert_eq!(
        twice.shots().len(),
        3,
        "a lost device is retried once, not again"
    );
}

/// "a helper that exited is started again, and a render queued behind
/// closeRenderer starts none" (the queued half: a closed renderer stays
/// closed; the call under way ends on the renderer it had)
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_call_queued_behind_the_close_is_answered_shut_down() {
    let fake = Fake::with(|c| c.hold = true);
    let shared = started(&fake);
    let of = settled(1, 1, 0);
    let running = tokio::spawn({
        let (shared, of) = (Arc::clone(&shared), Arc::clone(&of));
        async move { render(&shared, "fixture", &[(&of, "front")], &never()).await }
    });
    let blocking = Arc::clone(&fake);
    tokio::task::spawn_blocking(move || blocking.until("the held shots", |c| c.held == 1))
        .await
        .unwrap();
    let queued = tokio::spawn({
        let (shared, of) = (Arc::clone(&shared), Arc::clone(&of));
        async move { render(&shared, "fixture", &[(&of, "front")], &never()).await }
    });
    shared.close();
    fake.release(1);
    assert_eq!(running.await.unwrap().unwrap().len(), 1);
    assert_eq!(queued.await.unwrap(), Err(SHUT_DOWN.to_string()));
    assert_eq!(
        render(&shared, "fixture", &[(&of, "front")], &never()).await,
        Err(SHUT_DOWN.to_string())
    );
}

/// "a world is sent once and drawn again from the helper's copy; one the
/// helper no longer keeps is sent again"
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_world_is_sent_once_and_drawn_again_from_the_renderers_copy() {
    let fake = Fake::with(|_| {});
    let shared = started(&fake);
    let of = settled(1, 1, 0);
    render(&shared, "fixture", &[(&of, "front")], &never())
        .await
        .unwrap();
    render(
        &shared,
        "fixture",
        &[(&of, "top"), (&of, "front")],
        &never(),
    )
    .await
    .unwrap();
    assert_eq!(fake.worlds().len(), 1);
    assert_eq!(
        fake.shots().iter().map(|(_, n)| *n).collect::<Vec<_>>(),
        [1, 1, 2],
        "the first shots found no world; then each call is one request"
    );
    let other = settled(1, 2, 0);
    render(&shared, "fixture", &[(&other, "front")], &never())
        .await
        .unwrap();
    assert_eq!(
        fake.worlds().len(),
        2,
        "another settled world is another world"
    );
    // One the renderer no longer keeps (let go under its memory bound).
    fake.control.lock().unwrap().kept.clear();
    render(&shared, "fixture", &[(&of, "front")], &never())
        .await
        .unwrap();
    assert_eq!(
        fake.worlds()[2..],
        [
            (
                "fixture/tick 0/1".to_string(),
                Some(("fixture/tick 0/2".to_string(), true))
            ),
            ("fixture/tick 0/1".to_string(), None),
        ],
        "sent again: as an edit of the last world sent, which is not kept either, then whole"
    );
}

/// "an edit of the build names the previous world as its base and drops it"
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_edit_names_the_previous_world_as_its_base_and_drops_it() {
    let fake = Fake::with(|_| {});
    let shared = started(&fake);
    let (before, after) = (settled(1, 1, 210), settled(2, 2, 210));
    render(&shared, "chat", &[(&before, "front")], &never())
        .await
        .unwrap();
    render(&shared, "chat", &[(&after, "front")], &never())
        .await
        .unwrap();
    assert_eq!(
        fake.worlds(),
        [
            ("chat/tick 210/1".to_string(), None),
            (
                "chat/tick 210/2".to_string(),
                Some(("chat/tick 210/1".to_string(), false))
            ),
        ]
    );
    // The dropped world is no one's base again: the next edition patches the last one.
    render(&shared, "chat", &[(&settled(3, 3, 210), "front")], &never())
        .await
        .unwrap();
    assert_eq!(
        fake.worlds().last().unwrap().1,
        Some(("chat/tick 210/2".to_string(), false))
    );
}

/// "another tick of the same build keeps its base, and a later call draws
/// both without sending either"
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn another_tick_of_the_same_build_keeps_its_base() {
    let fake = Fake::with(|_| {});
    let shared = started(&fake);
    let (at_210, at_20) = (settled(1, 1, 210), settled(1, 2, 20));
    let both = [(&at_210, "front"), (&at_20, "front")];
    render(&shared, "chat", &both, &never()).await.unwrap();
    assert_eq!(
        fake.worlds(),
        [
            ("chat/tick 210/1".to_string(), None),
            (
                "chat/tick 20/2".to_string(),
                Some(("chat/tick 210/1".to_string(), true))
            ),
        ]
    );
    render(&shared, "chat", &both, &never()).await.unwrap();
    assert_eq!(fake.worlds().len(), 2, "both are kept");
}

/// "worlds of other conversations are never bases"
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn worlds_of_other_conversations_are_never_bases() {
    let fake = Fake::with(|_| {});
    let shared = started(&fake);
    render(&shared, "one", &[(&settled(1, 1, 210), "front")], &never())
        .await
        .unwrap();
    render(&shared, "two", &[(&settled(2, 2, 210), "front")], &never())
        .await
        .unwrap();
    assert!(fake.worlds().iter().all(|(_, base)| base.is_none()));
}

/// "a base the helper no longer holds is answered unknownWorld, and the
/// world is sent whole"
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_base_no_longer_held_is_sent_whole() {
    let fake = Fake::with(|c| c.forget_bases = true);
    let shared = started(&fake);
    render(&shared, "chat", &[(&settled(1, 1, 210), "front")], &never())
        .await
        .unwrap();
    let images = render(&shared, "chat", &[(&settled(2, 2, 210), "front")], &never())
        .await
        .unwrap();
    assert_eq!(images, ["chat/tick 210/2 shot 0"]);
    assert_eq!(
        fake.worlds()
            .into_iter()
            .map(|(name, base)| (name, base.map(|b| b.0)))
            .collect::<Vec<_>>(),
        [
            ("chat/tick 210/1".to_string(), None),
            (
                "chat/tick 210/2".to_string(),
                Some("chat/tick 210/1".to_string())
            ),
            ("chat/tick 210/2".to_string(), None),
        ]
    );
}

// ---- shot-cancellation.test.js ----

/// "queued screenshots cancel without touching another render; active
/// cancellation stops only the helper's job, and the helper stays up"
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_queued_call_cancels_without_reaching_the_renderer_and_an_active_one_cancels_its_job() {
    let fake = Fake::with(|c| c.hold = true);
    let shared = started(&fake);
    let of = settled(1, 1, 0);
    let spawn = |cancel: CancellationToken| {
        let (shared, of) = (Arc::clone(&shared), Arc::clone(&of));
        tokio::spawn(async move { render(&shared, "fixture", &[(&of, "front")], &cancel).await })
    };
    let wait = |what: &'static str, n: usize| {
        let fake = Arc::clone(&fake);
        tokio::task::spawn_blocking(move || {
            fake.until(what, |c| {
                c.asked
                    .iter()
                    .filter(|a| matches!(a, Asked::Shots { .. }))
                    .count()
                    == n
            })
        })
    };
    let first = spawn(never());
    wait("the first shots held", 2).await.unwrap();
    let waiting = CancellationToken::new();
    let second = spawn(waiting.clone());
    waiting.cancel();
    assert_eq!(second.await.unwrap(), Err(ABORTED.to_string()));
    fake.release(1);
    assert_eq!(first.await.unwrap().unwrap().len(), 1);
    assert_eq!(
        fake.shots().len(),
        2,
        "the cancelled call never reached the renderer"
    );
    assert!(!fake.asked().contains(&Asked::Cancelled));

    let active = CancellationToken::new();
    let third = spawn(active.clone());
    wait("the third shots held", 3).await.unwrap();
    active.cancel();
    assert_eq!(third.await.unwrap(), Err(ABORTED.to_string()));
    let cancelled = Arc::clone(&fake);
    tokio::task::spawn_blocking(move || {
        cancelled.until("the job cancelled", |c| c.asked.contains(&Asked::Cancelled))
    })
    .await
    .unwrap();
    let fourth = spawn(never());
    wait("the fourth shots held", 4).await.unwrap();
    fake.release(3);
    assert_eq!(fourth.await.unwrap().unwrap().len(), 1);
}

// ---- shot-recovery.test.js ----

/// "renderer failures release the queue" (a failure the renderer reports;
/// a crash and a stall are a helper process's)
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_renderer_failure_is_named_by_its_step_and_releases_the_queue() {
    let fake = Fake::with(|_| {});
    let shared = started(&fake);
    let of = settled(1, 1, 0);
    render(&shared, "fixture", &[(&of, "front")], &never())
        .await
        .unwrap();
    fake.control.lock().unwrap().fail = Some("A shot: a test".into());
    assert_eq!(
        render(&shared, "fixture", &[(&of, "front")], &never()).await,
        Err("Screenshot capture: A shot: a test".to_string())
    );
    assert_eq!(
        render(&shared, "fixture", &[(&of, "front")], &never())
            .await
            .unwrap(),
        ["fixture/tick 0/1 shot 0"]
    );
}

// ---- shot-renderer.test.js ----

/// "a helper that finds no graphics adapter fails the call saying so" (and
/// a call that came before the start waited for it)
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_renderer_that_cannot_start_fails_the_call_saying_so() {
    let shared = SharedRenderer::new();
    let of = settled(1, 1, 0);
    let waiting = tokio::spawn({
        let (shared, of) = (Arc::clone(&shared), Arc::clone(&of));
        async move { render(&shared, "fixture", &[(&of, "front")], &never()).await }
    });
    shared.set(Err("No graphics adapter: No Metal device".into()));
    let sentence = "Screenshot initialization: No graphics adapter: No Metal device".to_string();
    assert_eq!(waiting.await.unwrap(), Err(sentence.clone()));
    assert_eq!(
        render(&shared, "fixture", &[(&of, "front")], &never()).await,
        Err(sentence)
    );
}

/// "a start ... lets the next screenshot through": a call made before the
/// renderer has started waits for it, and is cancelled while it waits.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_call_before_the_start_waits_for_it() {
    let shared = SharedRenderer::new();
    let of = settled(1, 1, 0);
    let cancel = CancellationToken::new();
    let cancelled = tokio::spawn({
        let (shared, of, cancel) = (Arc::clone(&shared), Arc::clone(&of), cancel.clone());
        async move { render(&shared, "fixture", &[(&of, "front")], &cancel).await }
    });
    let waiting = tokio::spawn({
        let (shared, of) = (Arc::clone(&shared), Arc::clone(&of));
        async move { render(&shared, "fixture", &[(&of, "front")], &never()).await }
    });
    cancel.cancel();
    assert_eq!(cancelled.await.unwrap(), Err(ABORTED.to_string()));
    let fake = Fake::with(|_| {});
    shared.set(Ok(Arc::clone(&fake) as Arc<dyn Renderer>));
    assert_eq!(waiting.await.unwrap().unwrap(), ["fixture/tick 0/1 shot 0"]);
}

/// `composeMontage`: one JPEG of the tiles, each tile in its cell.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_card_is_one_jpeg_of_one_tile_per_cell() {
    let shared = started(&Fake::with(|_| {}));
    let cells = montage_cells(3, 1024).unwrap();
    let card = shared
        .montage(
            vec![jpeg("a"), jpeg("b"), jpeg("c")],
            cells.clone(),
            [1024, 1024],
        )
        .await
        .unwrap();
    assert_eq!(card, jpeg("card"));
    assert_eq!(
        shared.montage(vec![jpeg("a")], cells, [1024, 1024]).await,
        Err("A card needs one shot per cell".to_string())
    );
}

// ---- shot-framing.test.js ----

fn spec(value: Value) -> Map<String, Value> {
    value.as_object().unwrap().clone()
}

/// "a world of about 16,000 chunks is framed whole"
#[test]
fn a_world_of_about_16000_chunks_is_framed_whole() {
    let mut world = World::new();
    for i in 0..126 {
        for j in 0..126 {
            world
                .set([16 * i, 0, 16 * j], BlockState::new("minecraft:stone", &[]))
                .unwrap();
        }
    }
    assert_eq!(world.chunk_count(), 15876);
    let top = resolve_shot(
        &spec(json!({"view": "top", "projection": "orthographic"})),
        &world,
        &[],
    )
    .unwrap();
    let center: Vec<f64> = top.described["center"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_f64().unwrap().round())
        .collect();
    assert_eq!(center, [1001.0, 1.0, 1001.0]);
    assert!(top.described["widthBlocks"].as_f64().unwrap() >= 2001.0);
    assert!(
        resolve_shot(&Map::new(), &world, &[])
            .unwrap()
            .shot
            .camera
            .focus_dist
            > 0.0
    );
}

/// "an entity past the blocks is framed with them, and a clip cuts it as
/// it cuts the blocks"
#[test]
fn an_entity_past_the_blocks_is_framed_with_them_and_a_clip_cuts_it() {
    let mut world = World::new();
    for x in 0..8 {
        world
            .set([x, 0, 0], BlockState::new("minecraft:stone", &[]))
            .unwrap();
    }
    let cart = json!({"id": 1, "type": "minecraft:minecart", "pos": [12.5, 1, 0.5], "yaw": 0, "pitch": 0, "width": 0.98, "height": 0.7});
    let ortho = |entities: &[Value], extra: Value| {
        let mut s = spec(json!({"view": "front", "projection": "orthographic"}));
        s.extend(spec(extra));
        resolve_shot(&s, &world, entities).unwrap().described
    };
    let at = |d: &Map<String, Value>, key: &str| d[key].as_f64().unwrap();
    let x = |d: &Map<String, Value>| d["center"][0].as_f64().unwrap();
    let without = ortho(&[], json!({}));
    let framed = ortho(std::slice::from_ref(&cart), json!({}));
    assert!(
        x(&framed) - at(&framed, "widthBlocks") / 2.0 <= 0.0
            && x(&framed) + at(&framed, "widthBlocks") / 2.0 >= 12.99,
        "the cart's box (to x 12.99) is in the picture: {framed:?}"
    );
    assert!(
        x(&without) + at(&without, "widthBlocks") / 2.0 < 12.0,
        "without it the blocks alone are framed"
    );
    let clip = json!({"clip": {"from": [0, 0, 0], "to": [7, 2, 0]}});
    assert_eq!(
        ortho(&[cart], clip.clone())["center"],
        ortho(&[], clip)["center"],
        "a clip that leaves the cart out frames as without it"
    );
}

/// "a card of 1 to 8 shots is one row for 1 or 2 and two rows otherwise,
/// the top one holding the extra shot"
#[test]
fn a_card_of_1_to_8_shots_is_laid_out_in_one_or_two_rows() {
    let cell = |x, y, w, h| [x, y, w, h];
    let expected: [Vec<[u32; 4]>; 8] = [
        vec![cell(0, 0, 1024, 1024)],
        vec![cell(0, 0, 512, 1024), cell(512, 0, 512, 1024)],
        vec![
            cell(0, 0, 512, 512),
            cell(512, 0, 512, 512),
            cell(0, 512, 1024, 512),
        ],
        vec![
            cell(0, 0, 512, 512),
            cell(512, 0, 512, 512),
            cell(0, 512, 512, 512),
            cell(512, 512, 512, 512),
        ],
        vec![
            cell(0, 0, 341, 512),
            cell(341, 0, 342, 512),
            cell(683, 0, 341, 512),
            cell(0, 512, 512, 512),
            cell(512, 512, 512, 512),
        ],
        vec![
            cell(0, 0, 341, 512),
            cell(341, 0, 342, 512),
            cell(683, 0, 341, 512),
            cell(0, 512, 341, 512),
            cell(341, 512, 342, 512),
            cell(683, 512, 341, 512),
        ],
        vec![
            cell(0, 0, 256, 512),
            cell(256, 0, 256, 512),
            cell(512, 0, 256, 512),
            cell(768, 0, 256, 512),
            cell(0, 512, 341, 512),
            cell(341, 512, 342, 512),
            cell(683, 512, 341, 512),
        ],
        [0, 256, 512, 768]
            .map(|x| cell(x, 0, 256, 512))
            .into_iter()
            .chain([0, 256, 512, 768].map(|x| cell(x, 512, 256, 512)))
            .collect(),
    ];
    for (count, expected) in (1..=8).zip(expected) {
        let cells: Vec<[u32; 4]> = montage_cells(count, 1024)
            .unwrap()
            .iter()
            .map(|c| [c.x, c.y, c.width, c.height])
            .collect();
        assert_eq!(cells, expected, "{count} shots");
        assert_eq!(
            cells.iter().map(|c| c[2] * c[3]).sum::<u32>(),
            1024 * 1024,
            "{count} shots cover the card"
        );
    }
    assert_eq!(
        montage_cells(0, 1024).unwrap_err(),
        "A card holds at least one shot"
    );
}
