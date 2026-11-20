//! The live world through the core (`Core::command` and
//! `mcbench_ui::Core::send`): every command of the live view's seam
//! (`ui-seam-viewer.md`) on success and on real refusals, with the real
//! in-process engine, a fake renderer that records `world`, `live_show`,
//! `live_patch` and `drop_world` in order, and a clock the test drives.
//!
//! The clock stands still unless the test moves it, so what is due is
//! decided by the test, not by timing: the live world's task runs every tick
//! that is due before the next command, and a command run with
//! `Core::command` has published everything it publishes when it returns. A
//! test waits for a condition and never sleeps to let something happen.
//!
//! Needs Minecraft's tables: `MCBENCH_TABLES` names their folder at build
//! time (the settles of the schematic export run `mcbench-lite headless
//! engine`, this crate's own binary).

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::Duration;

use async_trait::async_trait;
use indexmap::IndexMap;
use tokio::sync::watch;

use mcbench_engine::{Engine, EngineProgram, SettlePool};
use mcbench_lite::core::backend::Backend;
use mcbench_lite::core::live::{Clock, FELL_OUT, LiveParts, LiveSources, NO_WORLD};
use mcbench_lite::core::{Core, Parts};
use mcbench_render::live::{
    CameraInput, CameraState, Frame, LiveFacts, LivePane, LivePatch, LiveRenderer, Overlays,
    PaneSize, PaneStatus, Pick, Shown, TimeOfDay, ViewKey,
};
use mcbench_render::{
    AdapterChoice, AdapterReport, Assets, Cell, Clip, Picture, PictureFormat, RenderError,
    Renderer, Shot, WorldReport, WorldUpload,
};
use mcbench_ui::{
    Command, LiveCommand, LivePhase, LiveState, PlayerState, Running, Update, ViewerInput, Window,
};

fn tables() -> PathBuf {
    PathBuf::from(
        std::env::var("MCBENCH_TABLES").expect(
            "MCBENCH_TABLES names the folder of Minecraft's tables (core/minecraft/1.21.10)",
        ),
    )
}

fn engine() -> Engine {
    static ENGINE: OnceLock<Engine> = OnceLock::new();
    *ENGINE.get_or_init(|| Engine::start(&tables()).unwrap())
}

/// A clock that moves only when the test sets it.
struct TestClock(watch::Sender<Duration>);

impl TestClock {
    fn set(&self, ms: u64) {
        self.0.send_replace(Duration::from_millis(ms));
    }
}

#[async_trait]
impl Clock for TestClock {
    fn now(&self) -> Duration {
        *self.0.borrow()
    }
    async fn until(&self, at: Duration) {
        let mut now = self.0.subscribe();
        let _ = now.wait_for(|now| *now >= at).await;
    }
}

/// What the core asked of the renderer, in order.
#[derive(Clone, Debug)]
enum Call {
    World {
        name: String,
        base: Option<String>,
        blueprint: Vec<u8>,
    },
    Show(Option<Shown>),
    Patch(LivePatch),
    Drop(String),
}

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
        unimplemented!("the core never reads the camera")
    }
    fn facts(&self) -> LiveFacts {
        LiveFacts::default()
    }
}

#[derive(Default)]
struct FakeRenderer {
    calls: Mutex<Vec<Call>>,
    called: Condvar,
}

impl FakeRenderer {
    fn record(&self, call: Call) {
        self.calls.lock().unwrap().push(call);
        self.called.notify_all();
    }
    fn calls(&self) -> Vec<Call> {
        self.calls.lock().unwrap().clone()
    }
    fn patches(&self) -> Vec<LivePatch> {
        self.calls()
            .into_iter()
            .filter_map(|c| match c {
                Call::Patch(p) => Some(p),
                _ => None,
            })
            .collect()
    }
}

impl LiveRenderer for FakeRenderer {
    fn live(&self) -> Arc<dyn LivePane> {
        Arc::new(FakePane)
    }
    fn live_show(&self, shown: Option<Shown>) {
        self.record(Call::Show(shown));
    }
    fn live_patch(&self, patch: LivePatch) {
        self.record(Call::Patch(patch));
    }
}

impl Renderer for FakeRenderer {
    fn start(_: &Assets, _: AdapterChoice, _: u64) -> Result<Self, RenderError> {
        unimplemented!("the core is given a started renderer")
    }
    fn report(&self) -> &AdapterReport {
        unimplemented!("the core never reads the report")
    }
    fn world(&self, upload: WorldUpload) -> Result<WorldReport, RenderError> {
        self.record(Call::World {
            name: upload.name,
            base: upload.base.map(|(name, _)| name),
            blueprint: upload.blueprint,
        });
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
        _: &str,
        _: &[Shot],
        _: u32,
        _: PictureFormat,
        _: &AtomicBool,
    ) -> Result<Vec<Picture>, RenderError> {
        unimplemented!("the live world draws no shots")
    }
    fn montage(&self, _: [u32; 2], _: &[Cell], _: &[&[u8]], _: u8) -> Result<Vec<u8>, RenderError> {
        unimplemented!("the live world draws no cards")
    }
    fn drop_world(&self, world: &str) -> Result<bool, RenderError> {
        self.record(Call::Drop(world.to_string()));
        Ok(true)
    }
    fn shutdown(&self) {
        unimplemented!("the live world's tests never quit the app")
    }
}

struct NoModel;

#[async_trait]
impl Backend for NoModel {
    fn provider(&self, _: &str) -> Result<Arc<dyn mcbench_providers::Provider>, String> {
        Err("no model in the live world's tests".into())
    }
    fn tools(&self) -> mcbench_tools::Tools {
        mcbench_tools::Tools::of(Vec::new())
    }
    fn sync_attachments(&self, _: &str) -> Result<(), String> {
        Ok(())
    }
    async fn reset_sandbox(&self, _: &str) -> Result<(), String> {
        Ok(())
    }
    fn network_access(&self) -> bool {
        false
    }
    async fn shutdown(&self) {}
}

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

/// The names the block table gives the blocks these tests click.
fn names() -> IndexMap<String, String> {
    [
        ("minecraft:air", "Air"),
        ("minecraft:stone", "Stone"),
        ("minecraft:lever", "Lever"),
        ("minecraft:redstone_wire", "Redstone Wire"),
        ("minecraft:redstone_lamp", "Redstone Lamp"),
        ("minecraft:infested_stone", "Infested Stone"),
        ("minecraft:tnt", "TNT"),
        ("minecraft:redstone_block", "Block of Redstone"),
    ]
    .into_iter()
    .map(|(id, name)| (id.to_string(), name.to_string()))
    .collect()
}

/// A block of a test's build: where (build coordinates), its id and its
/// properties.
type Placed<'a> = ([i32; 3], &'a str, &'a [(&'a str, &'a str)]);

/// The lever, in Minecraft's coordinates.
const LEVER: [i32; 3] = [1, 65, 1];
const WAIT: Duration = Duration::from_secs(120);

struct App {
    core: Core,
    window: Arc<Recording>,
    renderer: Arc<FakeRenderer>,
    clock: Arc<TestClock>,
    settler: Arc<mcbench_tools::settle::Settler>,
    data: PathBuf,
}

impl App {
    fn new(name: &str) -> App {
        App::with(name, true)
    }

    fn with(name: &str, renderer_starts: bool) -> App {
        let data =
            std::env::temp_dir().join(format!("mcbench-core-live-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&data);
        std::fs::create_dir_all(&data).unwrap();
        let facts = Arc::new(
            mcbench_world::facts::MinecraftFacts::new(
                &std::fs::read_to_string(tables().join("blocks.txt")).unwrap(),
            )
            .unwrap(),
        );
        let settler = Arc::new(mcbench_tools::settle::Settler::new(
            SettlePool::new(
                EngineProgram::new(PathBuf::from(env!("CARGO_BIN_EXE_mcbench-lite")), tables()),
                1,
                Duration::from_secs(120),
            ),
            Arc::clone(&facts),
            mcbench_world::dimension::y_offset(),
            200,
        ));
        let renderer = Arc::new(FakeRenderer::default());
        let clock = Arc::new(TestClock(watch::channel(Duration::ZERO).0));
        let core = Core::start(Parts {
            data: data.clone(),
            program: std::env::current_exe().unwrap(),
            host: Arc::new(mcbench_store::SystemHost),
            shipped: Box::new(|| Err("unused".into())),
            client: mcbench_setup::fetch::client(),
            backend: Some(Arc::new(NoModel)),
            installer: None,
            share: None,
            live: LiveParts {
                clock: Arc::clone(&clock) as Arc<dyn Clock>,
                renderer: None,
                given: Some((
                    LiveSources {
                        engine: engine(),
                        facts,
                        settler: Arc::clone(&settler),
                        names: Arc::new(names()),
                    },
                    if renderer_starts {
                        Ok(Arc::clone(&renderer) as Arc<dyn Renderer>)
                    } else {
                        Err("No graphics adapter: none in this test".into())
                    },
                )),
                shared: mcbench_tools::shots::SharedRenderer::new(),
                utc_offset: |_| 3600,
            },
        })
        .unwrap();
        let window = Arc::new(Recording::default());
        mcbench_ui::Core::attach(&core, window.clone());
        let app = App {
            core,
            window,
            renderer,
            clock,
            settler,
            data,
        };
        app.on(|core| async move { core.started().await });
        // What attach publishes once the core has started comes first.
        app.wait("the favourites after attach", |u| {
            u.iter().any(|u| matches!(u, Update::Favorites(_)))
        });
        app
    }

    fn on<T: Send + 'static, F: std::future::Future<Output = T> + Send + 'static>(
        &self,
        work: impl FnOnce(Core) -> F,
    ) -> T {
        let handle = self.core.handle().clone();
        handle
            .block_on(handle.spawn(work(self.core.clone())))
            .unwrap()
    }

    fn all(&self) -> Vec<Update> {
        self.window.updates.lock().unwrap().clone()
    }

    /// Runs `command` to its end: the updates published from its start on.
    fn run(&self, command: Command) -> Vec<Update> {
        let from = self.all().len();
        self.on(move |core| async move { core.command(command).await });
        self.all()[from..].to_vec()
    }

    fn live(&self, command: LiveCommand) -> Vec<Update> {
        self.run(Command::Live(command))
    }

    fn viewer(&self, input: ViewerInput) -> Vec<Update> {
        self.run(Command::Viewer(input))
    }

    /// Sends `command` as the window does (`mcbench_ui::Core::send`).
    fn send(&self, command: Command) {
        mcbench_ui::Core::send(&self.core, command);
    }

    /// Blocks until the updates satisfy `done`.
    fn wait(&self, what: &str, done: impl Fn(&[Update]) -> bool) -> Vec<Update> {
        let updates = self.window.updates.lock().unwrap();
        let (updates, timeout) = self
            .window
            .arrived
            .wait_timeout_while(updates, WAIT, |u| !done(u))
            .unwrap();
        assert!(!timeout.timed_out(), "{what}: not within {WAIT:?}");
        updates.clone()
    }

    /// Blocks until the renderer's calls satisfy `done`.
    fn wait_calls(&self, what: &str, done: impl Fn(&[Call]) -> bool) -> Vec<Call> {
        let calls = self.renderer.calls.lock().unwrap();
        let (calls, timeout) = self
            .renderer
            .called
            .wait_timeout_while(calls, WAIT, |c| !done(c))
            .unwrap();
        assert!(!timeout.timed_out(), "{what}: not within {WAIT:?}");
        calls.clone()
    }

    /// A chat whose world holds `blocks` (build coordinates).
    fn chat(&self, blocks: &[Placed]) -> String {
        let id = self.on(|core| async move {
            core.create_conversation("openrouter", "chat/model")
                .await
                .unwrap()
                .id
        });
        self.build(&id, blocks);
        id
    }

    /// Sets `blocks` in the chat's world, as a turn's build does.
    fn build(&self, chat: &str, blocks: &[Placed]) {
        let live = self.core.store().conversation(chat).unwrap().unwrap();
        let mut conversation = live.lock().unwrap();
        for (pos, id, properties) in blocks {
            let properties: Vec<(String, String)> = properties
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect();
            conversation.world.set_block(*pos, id, &properties).unwrap();
        }
        conversation.world.finalize();
    }

    /// The lever world: a stone floor, a lever, dust and a lamp.
    fn lever_chat(&self) -> String {
        let mut blocks: Vec<Placed> = Vec::new();
        for x in 0..6 {
            for z in 0..6 {
                blocks.push(([x, 0, z], "minecraft:stone", &[]));
            }
        }
        blocks.push((
            [1, 1, 1],
            "minecraft:lever",
            &[("face", "floor"), ("facing", "north"), ("powered", "false")],
        ));
        blocks.push(([2, 1, 1], "minecraft:redstone_wire", &[]));
        blocks.push(([3, 1, 1], "minecraft:redstone_lamp", &[]));
        self.chat(&blocks)
    }

    /// Shows `chat`, and pauses it at the tick it is due at time 0.
    fn shown_paused(&self, chat: &str) {
        self.live(LiveCommand::Show {
            view: Some(view(chat)),
            afresh: false,
        });
        let paused = self.live(LiveCommand::Pause);
        assert_eq!(last_live(&paused).running, Some(Running::Paused));
    }
}

impl Drop for App {
    fn drop(&mut self) {
        let settler = Arc::clone(&self.settler);
        self.on(|_| async move { settler.pool().stop().await });
        self.core.shutdown();
        let _ = std::fs::remove_dir_all(&self.data);
    }
}

fn view(chat: &str) -> ViewKey {
    ViewKey(chat.to_string())
}

fn lives(updates: &[Update]) -> Vec<LiveState> {
    updates
        .iter()
        .filter_map(|u| match u {
            Update::Live(state) => Some(state.clone()),
            _ => None,
        })
        .collect()
}

fn last_live(updates: &[Update]) -> LiveState {
    lives(updates).pop().expect("an Update::Live")
}

fn errors(updates: &[Update]) -> Vec<String> {
    updates
        .iter()
        .filter_map(|u| match u {
            Update::Error(e) => Some(e.clone()),
            _ => None,
        })
        .collect()
}

macro_rules! live_test {
    ($name:ident, $body:block) => {
        #[test]
        #[cfg_attr(
            not(mcbench_tables),
            ignore = "needs Minecraft's tables: set MCBENCH_TABLES at build time"
        )]
        fn $name() $body
    };
}

live_test!(the_pane_is_published_once_when_the_renderer_has_started, {
    let app = App::new("pane");
    let panes = app
        .all()
        .iter()
        .filter(|u| matches!(u, Update::LivePane(_)))
        .count();
    assert_eq!(panes, 1);
    assert!(matches!(
        app.all().iter().find(|u| matches!(u, Update::LivePane(_))),
        Some(Update::LivePane(Ok(_)))
    ));
});

live_test!(
    a_renderer_that_did_not_start_says_why_and_no_view_is_placed,
    {
        let app = App::with("no-renderer", false);
        assert!(app.all().iter().any(|u| matches!(
            u,
            Update::LivePane(Err(e)) if e == "No graphics adapter: none in this test"
        )));
        let chat = app.lever_chat();
        let shown = app.live(LiveCommand::Show {
            view: Some(view(&chat)),
            afresh: false,
        });
        assert_eq!(lives(&shown)[0].phase, LivePhase::Preparing);
        assert_eq!(
            last_live(&shown).phase,
            LivePhase::Failed {
                error: "No graphics adapter: none in this test".into(),
                nothing_shown: true
            }
        );
        assert!(app.renderer.calls().is_empty());
    }
);

live_test!(show_places_the_view_fresh_and_hands_it_to_the_renderer, {
    let app = App::new("show");
    let chat = app.lever_chat();
    let shown = app.live(LiveCommand::Show {
        view: Some(view(&chat)),
        afresh: false,
    });
    let states = lives(&shown);
    assert_eq!(states[0].phase, LivePhase::Preparing);
    assert_eq!(states[0].view, Some(view(&chat)));
    let end = states.last().unwrap();
    assert_eq!(end.phase, LivePhase::Shown);
    assert_eq!(end.running, Some(Running::Playing));
    assert_eq!(end.tick, 0);
    assert_eq!(end.player, PlayerState::None);
    assert_eq!(end.blocks, 39);
    assert_eq!(end.bounds, Some([[0, 64, 0], [5, 65, 5]]));
    assert_eq!(end.y_offset, 64);
    assert!(end.layout.is_empty());
    assert!(errors(&shown).is_empty());
    // The first tick is due at once (tick 0 at 0 ms).
    let calls = app.wait_calls("the first tick", |c| {
        c.iter().any(|c| matches!(c, Call::Patch(_)))
    });
    let name = format!("live:{chat}");
    assert!(matches!(&calls[0], Call::World { name: n, base: None, .. } if *n == name));
    let Call::Show(Some(shown)) = &calls[1] else {
        panic!("{:?}", calls[1])
    };
    assert_eq!(shown.world, name);
    assert_eq!(shown.view, view(&chat));
    assert_eq!(shown.revision, end.revision.unwrap());
    assert_eq!(
        shown.bounds,
        Clip {
            min: [0, 64, 0],
            max: [5, 65, 5]
        }
    );
    assert_eq!(shown.framing, mcbench_render::live::Framing::AtOnce);
    let Call::Patch(first) = &calls[2] else {
        panic!("{:?}", calls[2])
    };
    assert_eq!((first.tick, first.player, first.framing), (1, None, None));
    // The same view shown again places nothing.
    let again = app.live(LiveCommand::Show {
        view: Some(view(&chat)),
        afresh: false,
    });
    assert_eq!(last_live(&again).phase, LivePhase::Shown);
    assert!(
        !app.renderer.calls()[3..]
            .iter()
            .any(|c| matches!(c, Call::World { .. } | Call::Show(_)))
    );
});

live_test!(a_view_that_does_not_exist_fails_with_the_routes_sentence, {
    let app = App::new("missing");
    let shown = app.live(LiveCommand::Show {
        view: Some(view("no-such-chat")),
        afresh: false,
    });
    assert_eq!(
        last_live(&shown).phase,
        LivePhase::Failed {
            error: "Conversation not found".into(),
            nothing_shown: true
        }
    );
    assert!(errors(&shown).is_empty());
    let bench = app.live(LiveCommand::Show {
        view: Some(view("bench:none:0")),
        afresh: false,
    });
    assert_eq!(
        last_live(&bench).phase,
        LivePhase::Failed {
            error: "Bench not found".into(),
            nothing_shown: true
        }
    );
});

live_test!(show_none_ends_the_view_and_the_renderer_draws_nothing, {
    let app = App::new("none");
    let chat = app.lever_chat();
    app.shown_paused(&chat);
    let ended = app.live(LiveCommand::Show {
        view: None,
        afresh: false,
    });
    let state = last_live(&ended);
    assert_eq!((state.view, state.phase), (None, LivePhase::Idle));
    let calls = app.renderer.calls();
    let name = format!("live:{chat}");
    assert!(matches!(&calls[calls.len() - 2], Call::Show(None)));
    assert!(matches!(&calls[calls.len() - 1], Call::Drop(n) if *n == name));
    // Without a view the toolbar's commands are refused.
    for command in [LiveCommand::Play, LiveCommand::Pause, LiveCommand::Step] {
        let refused = app.live(command);
        assert_eq!(errors(&refused), [NO_WORLD]);
        assert_eq!(last_live(&refused).view, None);
    }
});

live_test!(
    the_clock_makes_up_late_ticks_and_counts_again_after_a_second_behind,
    {
        let app = App::new("clock");
        let chat = app.lever_chat();
        app.live(LiveCommand::Show {
            view: Some(view(&chat)),
            afresh: false,
        });
        // At 0 ms the first tick is due and runs; 49 ms later none more is.
        app.clock.set(49);
        let at_49 = app.live(LiveCommand::Pause);
        assert_eq!(last_live(&at_49).tick, 1);
        let played = app.live(LiveCommand::Play);
        // Play starts the count again from now (49 ms): a tick at once.
        assert_eq!(last_live(&played).running, Some(Running::Playing));
        app.clock.set(98);
        assert_eq!(last_live(&app.live(LiveCommand::Pause)).tick, 2);
        app.live(LiveCommand::Play); // ticks 3 at 98 ms
        // A late timer is made up: at 348 ms the ticks due at 148, 198, 248, 298
        // and 348 all run.
        app.clock.set(348);
        assert_eq!(last_live(&app.live(LiveCommand::Pause)).tick, 8);
        // Playing from 348 ms (tick 9 at once), then 2 s pass at once: the tick
        // due at 398 ms ends more than a second after the next was due, so the
        // count starts again from 2348 ms (tick 11 at once, the next at 2398).
        app.live(LiveCommand::Play);
        // A command that publishes nothing: the live world's task takes it
        // after the tick due at once and that tick's look at the clock.
        app.viewer(ViewerInput::PlayerInput {
            keys: 0,
            taps: 0,
            yaw: 0.0,
            pitch: 0.0,
        });
        assert_eq!(app.renderer.patches().len(), 9);
        app.clock.set(2348);
        let lagged = app.live(LiveCommand::Pause);
        assert_eq!(last_live(&lagged).tick, 11);
        // Every tick reached the renderer, in order, none dropped.
        let ticks: Vec<u64> = app.renderer.patches().iter().map(|p| p.tick).collect();
        assert_eq!(ticks, (1..=11).collect::<Vec<_>>());
        // Paused, time passing ticks nothing.
        app.clock.set(9000);
        assert_eq!(last_live(&app.live(LiveCommand::Pause)).tick, 11);
    }
);

live_test!(
    the_running_count_is_published_at_most_every_250_ms_and_the_last_always,
    {
        let app = App::new("throttle");
        let chat = app.lever_chat();
        app.live(LiveCommand::Show {
            view: Some(view(&chat)),
            afresh: false,
        });
        let from = app.all().len();
        // Ticks at 0 (its count held: the Show's update was at 0 too),
        // 50, 100, 150 and 200: no count before 250 ms.
        app.clock.set(200);
        app.viewer(ViewerInput::PlayerInput {
            keys: 0,
            taps: 0,
            yaw: 0.0,
            pitch: 0.0,
        });
        assert_eq!(app.renderer.patches().len(), 5);
        assert!(lives(&app.all()[from..]).is_empty());
        app.clock.set(250);
        let published = app.wait("the count at 250 ms", |u| {
            lives(&u[from..]).iter().any(|s| s.tick == 6)
        });
        let counts: Vec<u64> = lives(&published[from..]).iter().map(|s| s.tick).collect();
        assert!(counts.iter().all(|&t| t == 6), "{counts:?}");
    }
);

live_test!(step_ticks_once_when_paused_and_is_refused_otherwise, {
    let app = App::new("step");
    let chat = app.lever_chat();
    app.live(LiveCommand::Show {
        view: Some(view(&chat)),
        afresh: false,
    });
    let refused = app.live(LiveCommand::Step);
    assert_eq!(errors(&refused), ["Pause the live world to step it"]);
    let paused = app.live(LiveCommand::Pause);
    let tick = last_live(&paused).tick;
    let stepped = app.live(LiveCommand::Step);
    assert!(errors(&stepped).is_empty());
    assert_eq!(last_live(&stepped).tick, tick + 1);
    assert_eq!(app.renderer.patches().last().unwrap().tick, tick + 1);
});

live_test!(
    a_click_names_the_block_and_runs_the_input_between_ticks_in_order,
    {
        let app = App::new("click");
        let chat = app.lever_chat();
        app.shown_paused(&chat);
        let click = |block: [i32; 3], view_key: &str| {
            Command::Viewer(ViewerInput::Click {
                view: view(view_key),
                revision: 0,
                block,
                yaw: 0.0,
            })
        };
        // The lever toggles; its change is the next tick's patch.
        let patches = app.renderer.patches().len();
        app.send(click(LEVER, &chat));
        app.send(Command::Live(LiveCommand::Step));
        let updates = app.wait("the step after the click", |u| {
            u.iter().any(|u| matches!(u, Update::Clicked { .. }))
                && lives(u).last().is_some_and(|s| s.tick == 2)
        });
        let clicked = updates
            .iter()
            .position(|u| matches!(u, Update::Clicked { .. }))
            .unwrap();
        let Update::Clicked {
            name, input, block, ..
        } = &updates[clicked]
        else {
            unreachable!()
        };
        assert_eq!(
            (name.as_str(), input.as_deref(), *block),
            ("Lever", Some("toggle"), LEVER)
        );
        let stepped = updates
            .iter()
            .rposition(|u| matches!(u, Update::Live(s) if s.tick == 2));
        assert!(stepped.unwrap() > clicked);
        let after = &app.renderer.patches()[patches];
        assert_eq!(after.tick, 2);
        assert!(
            after.changes.len() > 40,
            "the lever's change is in the tick's patch"
        );
        // A block that takes no input is named and not run.
        let stone = app.viewer(ViewerInput::Click {
            view: view(&chat),
            revision: 0,
            block: [0, 64, 0],
            yaw: 0.0,
        });
        assert!(matches!(
            stone.as_slice(),
            [Update::Clicked { name, input: None, .. }] if name == "Stone"
        ));
        // A click for a view not on show is not run.
        let other = app.run(click(LEVER, "another-chat"));
        assert!(matches!(
            other.as_slice(),
            [Update::Clicked { name, input: None, view, .. }] if name.is_empty() && view.0 == "another-chat"
        ));
    }
);

live_test!(
    a_turns_change_is_an_edit_with_ticks_going_on_and_a_restart_places_afresh,
    {
        let app = App::new("edit");
        let chat = app.lever_chat();
        app.shown_paused(&chat);
        app.live(LiveCommand::Step);
        let before = last_live(&app.all());
        assert_eq!(before.tick, 2);
        let worlds = |app: &App| {
            app.renderer
                .calls()
                .iter()
                .filter(|c| matches!(c, Call::World { .. }))
                .count()
        };
        let uploads = worlds(&app);
        // A block within the render mask: an edit, a patch at the same tick.
        app.build(&chat, &[([0, 1, 5], "minecraft:stone", &[])]);
        let edited = app.live(LiveCommand::Show {
            view: Some(view(&chat)),
            afresh: false,
        });
        let state = last_live(&edited);
        assert_eq!(state.tick, 2);
        assert_eq!(state.blocks, 40);
        assert!(state.revision > before.revision);
        assert_eq!(worlds(&app), uploads);
        let patch = app.renderer.patches().pop().unwrap();
        assert_eq!((patch.tick, patch.revision), (2, state.revision.unwrap()));
        // Ticks go on from there.
        assert_eq!(last_live(&app.live(LiveCommand::Step)).tick, 3);
        // A block past the mask: a placement on the world as it is, drawn as a
        // new base of the same view, ticks going on.
        app.build(&chat, &[([300, 1, 0], "minecraft:stone", &[])]);
        let moved = app.live(LiveCommand::Show {
            view: Some(view(&chat)),
            afresh: false,
        });
        assert_eq!(last_live(&moved).tick, 3);
        let name = format!("live:{chat}");
        let calls = app.renderer.calls();
        assert!(calls.iter().any(
            |c| matches!(c, Call::World { name: n, base: Some(b), .. } if *n == name && *b == name)
        ));
        let Some(Call::Show(Some(shown))) = calls.last() else {
            panic!("{:?}", calls.last())
        };
        assert_eq!(shown.framing, mcbench_render::live::Framing::Fly(700));
        // A restart places afresh at tick 0, paused as it was.
        let restarted = app.live(LiveCommand::Restart);
        let state = last_live(&restarted);
        assert_eq!((state.tick, state.running), (0, Some(Running::Paused)));
        assert_eq!(worlds(&app), uploads + 2);
    }
);

live_test!(player_mode_enters_walks_falls_out_and_leaves, {
    let app = App::new("player");
    let chat = app.lever_chat();
    app.shown_paused(&chat);
    let entered = app.viewer(ViewerInput::EnterPlayer {
        view: view(&chat),
        target: [2.5, 65.0, 2.5],
        rotation: 0.0,
        yaw: 0.0,
        pitch: 0.0,
    });
    let players: Vec<PlayerState> = lives(&entered).iter().map(|s| s.player).collect();
    assert_eq!(players, [PlayerState::Spawning, PlayerState::Standing]);
    // Input is held: the next tick takes it, and its view rides the patch.
    let none = app.viewer(ViewerInput::PlayerInput {
        keys: 1,
        taps: 0,
        yaw: 0.0,
        pitch: 0.0,
    });
    assert!(none.is_empty());
    app.live(LiveCommand::Step);
    let walked = app.renderer.patches().pop().unwrap().player.unwrap();
    assert!(
        walked.delta[2] > 0.0,
        "walking forward along +z: {walked:?}"
    );
    // A player below y -128 is spawned again where it last spawned.
    let fell = app.viewer(ViewerInput::SpawnPlayer {
        view: view(&chat),
        at: [100.5, -120.0, 100.5],
        yaw: 0.0,
        pitch: 0.0,
        flying: false,
    });
    assert_eq!(last_live(&fell).player, PlayerState::Standing);
    app.viewer(ViewerInput::PlayerInput {
        keys: 0,
        taps: 0,
        yaw: 0.0,
        pitch: 0.0,
    });
    let mut steps = 0;
    while !app
        .all()
        .iter()
        .any(|u| matches!(u, Update::PlayerNote(n) if n == FELL_OUT))
    {
        steps += 1;
        assert!(steps < 100, "the player did not fall below y -128");
        app.live(LiveCommand::Step);
    }
    let below = app.renderer.patches().pop().unwrap().player.unwrap();
    assert!(below.pos[1] < -128.0);
    app.live(LiveCommand::Step);
    let respawned = app.renderer.patches().pop().unwrap().player.unwrap();
    assert!(respawned.pos[1] > -128.0, "{respawned:?}");
    let notes = app
        .all()
        .iter()
        .filter(|u| matches!(u, Update::PlayerNote(_)))
        .count();
    assert_eq!(notes, 1);
    let left = app.viewer(ViewerInput::LeavePlayer);
    assert_eq!(last_live(&left).player, PlayerState::None);
    app.live(LiveCommand::Step);
    assert!(app.renderer.patches().pop().unwrap().player.is_none());
    // Player mode on a view not on show is a note.
    let refused = app.viewer(ViewerInput::EnterPlayer {
        view: view("another-chat"),
        target: [0.0; 3],
        rotation: 0.0,
        yaw: 0.0,
        pitch: 0.0,
    });
    assert!(matches!(&refused[0], Update::PlayerNote(n) if n == NO_WORLD));
    assert_eq!(last_live(&refused).player, PlayerState::None);
});

live_test!(a_restart_stands_a_standing_player_again_where_it_stood, {
    let app = App::new("respawn");
    let chat = app.lever_chat();
    app.shown_paused(&chat);
    app.viewer(ViewerInput::SpawnPlayer {
        view: view(&chat),
        at: [2.5, 65.0, 2.5],
        yaw: 0.0,
        pitch: 0.0,
        flying: true,
    });
    app.live(LiveCommand::Step);
    let stood = app.renderer.patches().pop().unwrap().player.unwrap();
    let restarted = app.live(LiveCommand::Restart);
    assert_eq!(last_live(&restarted).player, PlayerState::Standing);
    app.live(LiveCommand::Step);
    let again = app.renderer.patches().pop().unwrap();
    assert_eq!(again.tick, 1);
    let again = again.player.unwrap();
    assert_eq!(again.old_pos, stood.pos);
    assert!(again.flying);
});

live_test!(a_world_the_engine_stops_takes_only_a_fresh_placement, {
    let app = App::new("stopped");
    let mut blocks: Vec<Placed> = Vec::new();
    for x in 0..6 {
        blocks.push(([x, 0, 4], "minecraft:stone", &[]));
    }
    blocks.push(([4, 1, 4], "minecraft:infested_stone", &[]));
    blocks.push(([5, 1, 4], "minecraft:tnt", &[("unstable", "false")]));
    blocks.push(([5, 1, 3], "minecraft:redstone_block", &[]));
    let chat = app.chat(&blocks);
    app.shown_paused(&chat);
    let mut steps = 0;
    let stopped = loop {
        steps += 1;
        assert!(steps < 200, "the TNT did not reach the infested stone");
        let stepped = app.live(LiveCommand::Step);
        assert!(errors(&stepped).is_empty(), "{:?}", errors(&stepped));
        if let Some(Running::Stopped(reason)) = last_live(&stepped).running {
            break reason;
        }
    };
    assert!(
        stopped.starts_with("The live world stopped: The engine failed: unported: "),
        "{stopped}"
    );
    assert_eq!(
        errors(&app.live(LiveCommand::Step)),
        ["The live world cannot step while stopped"]
    );
    assert_eq!(
        errors(&app.live(LiveCommand::Play)),
        ["The live world cannot go from stopped to playing"]
    );
    let again = app.live(LiveCommand::Show {
        view: Some(view(&chat)),
        afresh: true,
    });
    let state = last_live(&again);
    assert_eq!((state.tick, state.running), (0, Some(Running::Playing)));
});

live_test!(a_selection_is_named_from_the_stored_world, {
    let app = App::new("selection");
    let chat = app.lever_chat();
    let described = app.run(Command::DescribeSelection {
        chat: chat.clone(),
        request: 7,
        min: [0, 0, 0],
        max: [5, 0, 5],
    });
    let [
        Update::SelectionDescribed {
            request: 7,
            result: Ok(named),
        },
    ] = described.as_slice()
    else {
        panic!("{described:?}")
    };
    assert_eq!(
        (named.blocks, named.block.as_deref()),
        (36, Some("minecraft:stone"))
    );
    assert!(named.label.starts_with("Stone, "), "{}", named.label);
    let refused = app.run(Command::DescribeSelection {
        chat: chat.clone(),
        request: 8,
        min: [5, 0, 0],
        max: [0, 0, 0],
    });
    assert!(matches!(
        refused.as_slice(),
        [Update::SelectionDescribed { request: 8, result: Err(e) }]
            if e.starts_with("A selection needs integer corners")
    ));
    let missing = app.run(Command::DescribeSelection {
        chat: "no-such-chat".into(),
        request: 9,
        min: [0; 3],
        max: [0; 3],
    });
    assert!(matches!(
        missing.as_slice(),
        [Update::SelectionDescribed { request: 9, result: Err(e) }] if e == "Conversation not found"
    ));
});

live_test!(
    the_exports_write_through_a_temporary_file_and_leave_none_behind,
    {
        let app = App::new("exports");
        let chat = app.lever_chat();
        let folder = app.data.join("saved");
        std::fs::create_dir_all(&folder).unwrap();
        let to = folder.join("build.schem");
        let exported = app.run(Command::ExportSchematic {
            chat: chat.clone(),
            to: to.clone(),
        });
        assert!(
            matches!(exported.as_slice(), [Update::Exported(Ok(path))] if *path == to),
            "{exported:?}"
        );
        let bytes = std::fs::read(&to).unwrap();
        assert_eq!(&bytes[..2], [0x1f, 0x8b], "a gzipped schematic");
        let listed: Vec<_> = std::fs::read_dir(&folder)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(listed, ["build.schem"]);
        // A folder that does not exist: the file's error, nothing written.
        let nowhere = app.data.join("nowhere").join("build.schem");
        let failed = app.run(Command::ExportSchematic {
            chat: chat.clone(),
            to: nowhere.clone(),
        });
        assert!(
            matches!(failed.as_slice(), [Update::Exported(Err(e))] if e.starts_with(&nowhere.display().to_string()))
        );
        assert!(!app.data.join("nowhere").exists());
        // An empty build is the route's refusal.
        let empty = app.chat(&[]);
        let refused = app.run(Command::ExportSchematic {
            chat: empty,
            to: folder.join("empty.schem"),
        });
        assert!(matches!(
            refused.as_slice(),
            [Update::Exported(Err(e))] if e == "Minecraft keeps none of the blocks of this build: there is nothing to export"
        ));
        // The world: refused while the view is not on show, then a zip.
        let world = |view_key: &str| Command::ExportWorld {
            view: view(view_key),
            name: "Build".into(),
            to: folder.join("build.zip"),
        };
        assert!(
            matches!(app.run(world(&chat)).as_slice(), [Update::Exported(Err(e))] if e == NO_WORLD)
        );
        app.shown_paused(&chat);
        let zipped = app.run(world(&chat));
        assert!(
            matches!(zipped.as_slice(), [Update::Exported(Ok(p))] if *p == folder.join("build.zip"))
        );
        assert_eq!(
            &std::fs::read(folder.join("build.zip")).unwrap()[..2],
            b"PK"
        );
        let mut listed: Vec<_> = std::fs::read_dir(&folder)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        listed.sort();
        assert_eq!(listed, ["build.schem", "build.zip"]);
        // The save the zip is made of: the spawn above the build's middle.
        let save = app
            .on(|core| async move { core.live_world_save().await })
            .unwrap();
        assert_eq!(save.spawn, [3, 73, 3]);
        assert!(!save.export.chunks.is_empty());
    }
);

live_test!(an_empty_build_is_shown_with_nothing_to_tick, {
    let app = App::new("empty");
    let chat = app.chat(&[]);
    let shown = app.live(LiveCommand::Show {
        view: Some(view(&chat)),
        afresh: false,
    });
    let state = last_live(&shown);
    assert_eq!(
        (state.phase, state.running, state.blocks),
        (LivePhase::Shown, None, 0)
    );
    app.clock.set(500);
    assert_eq!(errors(&app.live(LiveCommand::Step)), [NO_WORLD]);
    assert!(app.renderer.patches().is_empty());
    // An empty world exports nothing, as the page's `shownEngine` refuses it.
    let to = app.data.join("empty.zip");
    let exported = app.run(Command::ExportWorld {
        view: view(&chat),
        name: "Empty".into(),
        to: to.clone(),
    });
    assert!(matches!(exported.as_slice(), [Update::Exported(Err(e))] if e == NO_WORLD));
    assert!(!to.exists());
    // Its first block is placed afresh.
    app.build(&chat, &[([0, 0, 0], "minecraft:stone", &[])]);
    let placed = app.live(LiveCommand::Show {
        view: Some(view(&chat)),
        afresh: false,
    });
    assert_eq!(last_live(&placed).running, Some(Running::Playing));
});

/// The world as the core holds the chat's (build coordinates).
fn stored(app: &App, chat: &str) -> mcbench_world::World {
    let live = app.core.store().conversation(chat).unwrap().unwrap();
    live.lock().unwrap().world.reference()
}

/// The differential against today's page: the same sequence through the
/// core's live world and through the page's engine as `web/live-world.mjs`
/// drives it (`web/core-worker.mjs` in a Node worker thread with the
/// released wasm engine, `tests/node/live.js`) gives the same blueprint,
/// tick and edit patches, entities, click and save at each step. Needs
/// `MCBENCH_LIVE_DIFF_NODE` (the `node` program) and `MCBENCH_LIVE_DIFF_WASM`
/// (the `mcbench-core.wasm` of this checkout's engine version) at run time,
/// and fails without them; it is run with `--ignored`.
#[test]
#[ignore = "needs MCBENCH_TABLES at build time, MCBENCH_LIVE_DIFF_NODE and MCBENCH_LIVE_DIFF_WASM: run with --ignored"]
fn the_live_world_drives_the_engine_as_the_page_does() {
    use base64::Engine as _;
    let node = std::env::var_os("MCBENCH_LIVE_DIFF_NODE")
        .expect("MCBENCH_LIVE_DIFF_NODE names the node program");
    let wasm = std::env::var_os("MCBENCH_LIVE_DIFF_WASM")
        .expect("MCBENCH_LIVE_DIFF_WASM names the engine's mcbench-core.wasm");
    let app = App::new("diff");
    let chat = app.lever_chat();
    let facts = mcbench_world::facts::MinecraftFacts::new(
        &std::fs::read_to_string(tables().join("blocks.txt")).unwrap(),
    )
    .unwrap();
    let y = mcbench_world::dimension::y_offset();
    let first = stored(&app, &chat);
    let fresh = first.place_commands(None, y, None, &facts).unwrap().0;
    // Core: placed (tick 1 is due at once), paused, 4 steps, the lever, a
    // step, an edit, 2 steps, the save.
    app.shown_paused(&chat);
    for _ in 0..4 {
        app.live(LiveCommand::Step);
    }
    let clicked = app.viewer(ViewerInput::Click {
        view: view(&chat),
        revision: 0,
        block: LEVER,
        yaw: 0.0,
    });
    app.live(LiveCommand::Step);
    app.build(
        &chat,
        &[
            ([4, 1, 4], "minecraft:redstone_lamp", &[]),
            (
                [4, 1, 3],
                "minecraft:lever",
                &[("face", "floor"), ("facing", "south"), ("powered", "true")],
            ),
        ],
    );
    let edit = stored(&app, &chat)
        .place_commands(Some(&first), y, None, &facts)
        .unwrap()
        .0;
    app.live(LiveCommand::Show {
        view: Some(view(&chat)),
        afresh: false,
    });
    for _ in 0..2 {
        app.live(LiveCommand::Step);
    }
    let save = app
        .on(|core| async move { core.live_world_save().await })
        .unwrap();
    // Node: the same requests to the page's engine.
    let folder = app.data.join("diff");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("fresh.txt"), &fresh).unwrap();
    std::fs::write(folder.join("edit.txt"), &edit).unwrap();
    std::fs::write(
        folder.join("steps.json"),
        format!(
            r#"{{"before":5,"after":2,"lever":[{},{},{}]}}"#,
            LEVER[0], LEVER[1], LEVER[2]
        ),
    )
    .unwrap();
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let out = folder.join("out.json");
    let ran = std::process::Command::new(node)
        .arg(manifest.join("tests/node/live.js"))
        .arg(manifest.join("../../.."))
        .arg(wasm)
        .arg(tables())
        .arg(&folder)
        .arg(&out)
        .status()
        .unwrap();
    assert!(ran.success(), "node failed: {ran}");
    let page: serde_json::Value = serde_json::from_slice(&std::fs::read(&out).unwrap()).unwrap();
    let bytes = |v: &serde_json::Value| {
        base64::engine::general_purpose::STANDARD
            .decode(v.as_str().unwrap())
            .unwrap()
    };
    let mut same = 0;
    let calls = app.renderer.calls();
    let Some(Call::World { blueprint, .. }) =
        calls.iter().find(|c| matches!(c, Call::World { .. }))
    else {
        panic!("no world uploaded")
    };
    assert_eq!(*blueprint, bytes(&page["blueprint"]), "the blueprint");
    same += 1;
    assert!(matches!(
        clicked.as_slice(),
        [Update::Clicked { input: Some(i), .. }] if Some(i.as_str()) == page["clicked"].as_str()
    ));
    same += 1;
    let patches = app.renderer.patches();
    let frames = page["frames"].as_array().unwrap();
    // The core's patches: ticks 1 to 6, the edit, ticks 7 and 8.
    assert_eq!(patches.len(), 9);
    assert_eq!(frames.len(), 8);
    let frame = |f: &serde_json::Value| {
        (
            bytes(&f["bytes"]),
            f["entities"].as_str().unwrap().as_bytes().to_vec(),
        )
    };
    let mut expected: Vec<(Vec<u8>, Vec<u8>)> = frames[..6].iter().map(frame).collect();
    expected.push((
        bytes(&page["edit"]["bytes"]),
        page["edit"]["entities"]
            .as_str()
            .unwrap()
            .as_bytes()
            .to_vec(),
    ));
    expected.extend(frames[6..].iter().map(frame));
    for (i, (patch, (changes, entities))) in patches.iter().zip(&expected).enumerate() {
        assert_eq!(patch.changes, *changes, "patch {i}'s changes");
        assert_eq!(patch.entities, *entities, "patch {i}'s entities");
        same += 2;
    }
    let ticks: Vec<u64> = frames
        .iter()
        .map(|f| f["ticks"].as_u64().unwrap())
        .collect();
    assert_eq!(ticks, (1..=8).collect::<Vec<_>>());
    assert_eq!(
        save.export.game_time.to_string(),
        page["save"]["gameTime"].as_str().unwrap()
    );
    same += 1;
    let chunks = page["save"]["chunks"].as_array().unwrap();
    assert_eq!(save.export.chunks.len(), chunks.len());
    for (ours, theirs) in save.export.chunks.iter().zip(chunks) {
        assert_eq!(
            (ours.x as i64, ours.z as i64),
            (theirs["x"].as_i64().unwrap(), theirs["z"].as_i64().unwrap())
        );
        assert_eq!(
            ours.nbt,
            bytes(&theirs["nbt"]),
            "chunk {} {}",
            ours.x,
            ours.z
        );
        same += 1;
    }
    eprintln!(
        "the live world's differential: {same} of {same} identical (the blueprint, the click, 9 patches' changes and entities, the game time, {} chunks)",
        chunks.len()
    );
}
