//! `see_build` through the core: a turn whose model calls `see_build` gets
//! its pictures from the core's one renderer, the live view's
//! (`LiveParts::shared`, given the live world's renderer when it starts),
//! here a fake that records `world` and `shots`. Started before the turn
//! (the live world's renderer, given at start), the call draws on it; not
//! started yet, the call waits and draws once it is.
//!
//! With the real engine children (the settles) and the build child:
//! `MCBENCH_TABLES` and `MCBENCH_ENGINE_PROGRAM` (a built `mcbench-lite`)
//! and `MCBENCH_BUILD_CHILD`; ignored without them.

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use indexmap::IndexMap;
use tokio_util::sync::CancellationToken;

use mcbench_lite::core::backend::Backend;
use mcbench_lite::core::live::{LiveParts, LiveSources};
use mcbench_lite::core::{Core, Parts};
use mcbench_providers::{
    AssistantMessage, CompletionRequest, Delta, ModelFacts, ModelInfo, Outcome, Provider,
    ProviderError, ProviderId, Telemetry, ToolCall,
};
use mcbench_render::live::{
    CameraInput, CameraState, Frame, LiveFacts, LivePane, LivePatch, LiveRenderer, Overlays,
    PaneSize, PaneStatus, Pick, Shown, TimeOfDay,
};
use mcbench_render::{
    AdapterChoice, AdapterReport, Assets, Cell, Clip, Picture, PictureFormat, RenderError,
    Renderer, Shot, WorldReport, WorldUpload,
};
use mcbench_tools::shots::SharedRenderer;
use mcbench_ui::{Command, Outgoing, Target, TurnEvent, Update, Window};

const WAIT: Duration = Duration::from_secs(120);

fn answer(content: &str, tool_calls: Vec<ToolCall>) -> AssistantMessage {
    AssistantMessage {
        content: content.into(),
        reasoning: None,
        reasoning_details: None,
        finish_reason: None,
        telemetry: Telemetry {
            provider: ProviderId::OpenRouter,
            model: String::new(),
            effort: None,
            started_at: String::new(),
            wall_ms: 0,
            first_delta_ms: None,
            context_bytes: 0,
            message_count: 0,
            usage: None,
            outcome: Outcome::Completed {
                context_tokens: None,
            },
        },
        notices: None,
        plan: None,
        rate_limit: None,
        tool_calls,
    }
}

/// A model that calls `see_build` once, then says it looked.
struct Model;

#[async_trait]
impl Provider for Model {
    fn id(&self) -> ProviderId {
        ProviderId::OpenRouter
    }
    async fn complete(
        &self,
        request: &CompletionRequest,
        _: &mut (dyn FnMut(Delta) + Send),
        _: &CancellationToken,
    ) -> Result<AssistantMessage, ProviderError> {
        if request.tools.is_empty() {
            return Ok(answer("A title", Vec::new()));
        }
        if request.messages.iter().any(|m| m.0["role"] == "tool") {
            return Ok(answer("Looked.", Vec::new()));
        }
        Ok(answer(
            "",
            vec![ToolCall {
                id: "call-1".into(),
                kind: "function".into(),
                name: "see_build".into(),
                arguments: "{}".into(),
                extra_content: None,
            }],
        ))
    }
    async fn models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        Ok(Vec::new())
    }
    async fn model_info(&self, _: &str) -> Result<ModelFacts, ProviderError> {
        Ok(ModelFacts {
            image_input: Some(true),
            ..ModelFacts::unknown()
        })
    }
    async fn close_session(&self, _: &str) {}
}

/// What the tools asked of the renderer, in order.
#[derive(Clone, Debug, PartialEq)]
enum Asked {
    World(String),
    Shots(String, usize),
}

#[derive(Default)]
struct FakeRenderer {
    asked: Mutex<Vec<Asked>>,
}

/// The pane the core publishes when the renderer starts; no view is shown.
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

impl LiveRenderer for FakeRenderer {
    fn live(&self) -> Arc<dyn LivePane> {
        Arc::new(FakePane)
    }
    fn live_show(&self, _: Option<Shown>) {}
    fn live_patch(&self, _: LivePatch) {}
}

impl Renderer for FakeRenderer {
    fn start(_: &Assets, _: AdapterChoice, _: u64) -> Result<Self, RenderError> {
        unimplemented!("the core is given a started renderer")
    }
    fn report(&self) -> &AdapterReport {
        unimplemented!("never read")
    }
    fn world(&self, upload: WorldUpload) -> Result<WorldReport, RenderError> {
        self.asked.lock().unwrap().push(Asked::World(upload.name));
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
        _: PictureFormat,
        _: &AtomicBool,
    ) -> Result<Vec<Picture>, RenderError> {
        let mut asked = self.asked.lock().unwrap();
        let kept = asked.contains(&Asked::World(world.to_string()));
        asked.push(Asked::Shots(world.to_string(), shots.len()));
        if !kept {
            return Err(RenderError::UnknownWorld(format!(
                "No world named {world} is kept"
            )));
        }
        Ok(shots
            .iter()
            .map(|_| Picture {
                width: 1,
                height: 1,
                bytes: vec![0xff, 0xd8, 0xff, 1],
            })
            .collect())
    }
    fn montage(&self, _: [u32; 2], _: &[Cell], _: &[&[u8]], _: u8) -> Result<Vec<u8>, RenderError> {
        unimplemented!("see_build makes no card")
    }
    fn drop_world(&self, _: &str) -> Result<bool, RenderError> {
        unimplemented!("no view is shown in these tests")
    }
    fn shutdown(&self) {
        unimplemented!("these tests never quit the app")
    }
}

/// The real tools once made (on the core's runtime, over its store).
type Real = Arc<Mutex<Option<mcbench_tools::Tools>>>;

struct Fake {
    real: Real,
}

#[async_trait]
impl Backend for Fake {
    fn provider(&self, _: &str) -> Result<Arc<dyn Provider>, String> {
        Ok(Arc::new(Model))
    }
    fn tools(&self) -> mcbench_tools::Tools {
        self.real.lock().unwrap().clone().expect("the tools made")
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
struct Recording(Mutex<Vec<Update>>);

impl Window for Recording {
    fn publish(&self, update: Update) {
        self.0.lock().unwrap().push(update);
    }
}

impl Recording {
    fn find<T>(&self, found: impl Fn(&Update) -> Option<T>) -> Option<T> {
        self.0.lock().unwrap().iter().find_map(found)
    }
}

fn until(what: &str, done: impl Fn() -> bool) {
    let started = Instant::now();
    while !done() {
        assert!(started.elapsed() < WAIT, "{what}: not within {WAIT:?}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn env(name: &str) -> PathBuf {
    PathBuf::from(std::env::var_os(name).unwrap_or_else(|| panic!("set {name}")))
}

struct App {
    core: Core,
    window: Arc<Recording>,
    data: PathBuf,
}

/// A core whose live world's renderer is `given` (or none yet), its tools
/// the real ones over its store, drawing with `shared`.
fn app(name: &str, shared: Arc<SharedRenderer>, given: Option<Arc<FakeRenderer>>) -> App {
    let tables = env("MCBENCH_TABLES");
    let data = std::env::temp_dir().join(format!("mcbench-core-see-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&data);
    std::fs::create_dir_all(&data).unwrap();
    let real: Real = Arc::default();
    let live = match given {
        Some(renderer) => {
            let facts = Arc::new(
                mcbench_world::facts::MinecraftFacts::new(
                    &std::fs::read_to_string(tables.join("blocks.txt")).unwrap(),
                )
                .unwrap(),
            );
            let settler = Arc::new(mcbench_tools::settle::Settler::new(
                mcbench_engine::SettlePool::new(
                    mcbench_engine::EngineProgram::new(
                        env("MCBENCH_ENGINE_PROGRAM"),
                        tables.clone(),
                    ),
                    1,
                    WAIT,
                ),
                Arc::clone(&facts),
                mcbench_world::dimension::y_offset(),
                200,
            ));
            LiveParts {
                renderer: None,
                given: Some((
                    LiveSources {
                        engine: mcbench_engine::Engine::start(&tables).unwrap(),
                        facts,
                        settler,
                        names: Arc::new(IndexMap::new()),
                    },
                    Ok(renderer as Arc<dyn Renderer>),
                )),
                shared: Arc::clone(&shared),
                ..LiveParts::default()
            }
        }
        None => LiveParts {
            renderer: Some(Box::new(|_| unreachable!("no setup runs in these tests"))),
            shared: Arc::clone(&shared),
            ..LiveParts::default()
        },
    };
    let core = Core::start(Parts {
        data: data.clone(),
        program: env("MCBENCH_ENGINE_PROGRAM"),
        host: Arc::new(mcbench_store::SystemHost),
        shipped: Box::new(|| Err("unused".into())),
        client: mcbench_setup::fetch::client(),
        backend: Some(Arc::new(Fake {
            real: Arc::clone(&real),
        })),
        installer: None,
        share: None,
        live,
    })
    .unwrap();
    let window = Arc::new(Recording::default());
    mcbench_ui::Core::attach(&core, window.clone());
    let handle = core.handle().clone();
    handle.block_on(core.started());
    let store = Arc::clone(core.store());
    let tools = handle
        .block_on(handle.spawn(async move {
            let engines = mcbench_tools::testing::shared(&tables).unwrap();
            let verifier = engines
                .verifier(store.folder().root.join("minecraft-verify"))
                .unwrap();
            let registry = mcbench_tools::testing::registry_from_table(&tables).unwrap();
            let services = mcbench_tools::Services::on(
                &engines,
                store,
                Arc::new(mcbench_tools::testing::RecordedSandbox::new()),
                verifier,
                Arc::new(registry),
                shared,
            );
            // The engines live as long as the process.
            Box::leak(Box::new(engines));
            mcbench_tools::Tools::new(services)
        }))
        .unwrap();
    *real.lock().unwrap() = Some(tools);
    App { core, window, data }
}

impl App {
    /// A chat whose world is a stone floor with a glass block.
    fn chat(&self) -> String {
        let handle = self.core.handle().clone();
        let core = self.core.clone();
        let chat = handle
            .block_on(handle.spawn(async move {
                core.create_conversation("openrouter", "vision/model")
                    .await
                    .unwrap()
                    .id
            }))
            .unwrap();
        let live = self.core.store().conversation(&chat).unwrap().unwrap();
        let mut conversation = live.lock().unwrap();
        for x in 0..4 {
            for z in 0..4 {
                conversation
                    .world
                    .set_block([x, 0, z], "minecraft:stone", &[])
                    .unwrap();
            }
        }
        conversation
            .world
            .set_block([1, 1, 1], "minecraft:glass", &[])
            .unwrap();
        conversation.world.finalize();
        chat
    }

    fn send(&self, chat: &str) {
        mcbench_ui::Core::send(
            &self.core,
            Command::Send(Outgoing {
                target: Target::Chat {
                    id: chat.into(),
                    provider: "openrouter".into(),
                    model: "vision/model".into(),
                },
                text: "Look at it.".into(),
                attachments: Vec::new(),
                selections: Vec::new(),
                prompts: Vec::new(),
                effort: None,
                resolution: mcbench_ui::Resolution::Standard,
                edit_of: None,
            }),
        );
    }

    /// The see_build call's result and its images, once the turn gave it.
    fn result(&self, chat: &str) -> (serde_json::Value, usize) {
        let found = || {
            self.window.find(|u| match u {
                Update::Turn {
                    chat: of,
                    event:
                        TurnEvent::ToolResult {
                            name,
                            result,
                            shown,
                            ..
                        },
                } if of == chat && name == "see_build" => {
                    Some((result.clone(), shown.images.len()))
                }
                _ => None,
            })
        };
        until("the see_build result", || found().is_some());
        found().unwrap()
    }

    fn started_call(&self, chat: &str) -> bool {
        self.window
            .find(|u| match u {
                Update::Turn {
                    chat: of,
                    event: TurnEvent::ToolStart { name, .. },
                } if of == chat && name == "see_build" => Some(()),
                _ => None,
            })
            .is_some()
    }

    fn end(self) {
        self.core.shutdown();
        std::fs::remove_dir_all(&self.data).unwrap();
    }
}

#[test]
#[ignore = "needs MCBENCH_TABLES, MCBENCH_ENGINE_PROGRAM (a built mcbench-lite) and MCBENCH_BUILD_CHILD"]
fn a_turns_see_build_draws_with_the_live_views_renderer() {
    let renderer = Arc::new(FakeRenderer::default());
    let app = app("after", SharedRenderer::new(), Some(Arc::clone(&renderer)));
    let chat = app.chat();
    app.send(&chat);
    let (result, images) = app.result(&chat);
    assert_eq!(result["ok"], true, "{result}");
    assert_eq!(images, 2, "the two default views, saved");
    let asked = renderer.asked.lock().unwrap().clone();
    let world = format!("{chat}/tick 210/");
    assert!(
        matches!(&asked[..], [Asked::Shots(a, 2), Asked::World(b), Asked::Shots(c, 2)]
            if a.starts_with(&world) && a == b && b == c),
        "the live view's renderer was sent the settled world and drew the shots: {asked:?}"
    );
    app.end();
}

#[test]
#[ignore = "needs MCBENCH_TABLES, MCBENCH_ENGINE_PROGRAM (a built mcbench-lite) and MCBENCH_BUILD_CHILD"]
fn a_see_build_before_the_renderer_has_started_waits_for_it() {
    let shared = SharedRenderer::new();
    let app = app("before", Arc::clone(&shared), None);
    let chat = app.chat();
    app.send(&chat);
    until("the see_build call starts", || app.started_call(&chat));
    // No renderer, no result: the call waits for one.
    assert!(
        app.window
            .find(|u| matches!(
                u,
                Update::Turn {
                    event: TurnEvent::ToolResult { .. },
                    ..
                }
            )
            .then_some(()))
            .is_none()
    );
    let renderer = Arc::new(FakeRenderer::default());
    shared.set(Ok(Arc::clone(&renderer) as Arc<dyn Renderer>));
    let (result, images) = app.result(&chat);
    assert_eq!(result["ok"], true, "{result}");
    assert_eq!(images, 2);
    assert_eq!(renderer.asked.lock().unwrap().len(), 3);
    app.end();
}
