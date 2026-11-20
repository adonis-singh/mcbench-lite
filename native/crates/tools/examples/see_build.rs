//! The Rust side of the `see_build` differential: each case of
//! `tests/node/see-build-cases.json` run through
//! [`mcbench_tools::Tools::execute`] with the in-process engine's settles,
//! the store on a scratch data folder and `mcbench_draw::live::Draw` (the
//! app's renderer) on the adapter `MCBENCH_DIFF_ADAPTER` names (`software`
//! or `hardware`), with the same files as `tests/node/see-build.js` gives
//! today's helper. The renderer is wrapped to note each `shots` request's
//! shots and to cancel the call at a case's `cancel` request, as the Node
//! side aborts it when that request's frame is written. Writes the same
//! JSON line per case.
//!
//! ```text
//! MCBENCH_TABLES=<core/minecraft/1.21.10> MCBENCH_ENGINE_PROGRAM=<mcbench-lite>
//! MCBENCH_BUILD_CHILD=<mcbench-build-child> MCBENCH_DIFF_MINECRAFT=<minecraft/<id>>
//! MCBENCH_DIFF_TEXTURES=<textures/<id>> MCBENCH_DIFF_WORLDS=<folder of blueprints>
//! [MCBENCH_DIFF_SOFTWARE_VULKAN=<libvk_swiftshader.so>] MCBENCH_DIFF_ADAPTER=<software|hardware>
//! see_build <data folder> <cases.json> <out.jsonl>
//! ```

use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use mcbench_render::live::{LivePane, LivePatch, LiveRenderer, Shown};
use mcbench_render::{
    AdapterChoice, AdapterReport, Assets, Cell, Picture, PictureFormat, RenderError, Renderer,
    Shot, WorldReport, WorldUpload,
};
use mcbench_tools::testing::{RecordedSandbox, registry_from_table, services, shared};
use mcbench_tools::{Chat, Tools, Workspace, World};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;

/// The app's renderer, noting the shots it is asked for and cancelling a
/// call at the request a case names.
struct Noted {
    draw: mcbench_draw::live::Draw,
    drawn: Mutex<Vec<Value>>,
    cancel: Mutex<Option<(String, CancellationToken)>>,
}

impl Noted {
    fn asked(&self, kind: &str) {
        if let Some((at, token)) = &*self.cancel.lock().unwrap()
            && at == kind
        {
            token.cancel();
        }
    }
}

impl LiveRenderer for Noted {
    fn live(&self) -> Arc<dyn LivePane> {
        self.draw.live()
    }
    fn live_show(&self, shown: Option<Shown>) {
        self.draw.live_show(shown)
    }
    fn live_patch(&self, patch: LivePatch) {
        self.draw.live_patch(patch)
    }
}

/// A shot as `server/shot.js` sends it: the camera without the lens it
/// does not have.
fn as_sent(shot: &Shot) -> Value {
    let mut value = serde_json::to_value(shot).expect("a shot serializes");
    let camera = value["camera"].as_object_mut().expect("a camera");
    camera.retain(|key, v| !(v.is_null() && (key == "fov" || key == "ortho")));
    value
}

impl Renderer for Noted {
    fn start(_: &Assets, _: AdapterChoice, _: u64) -> Result<Self, RenderError> {
        unreachable!("made from Draw")
    }
    fn report(&self) -> &AdapterReport {
        self.draw.report()
    }
    fn world(&self, upload: WorldUpload) -> Result<WorldReport, RenderError> {
        self.asked("world");
        self.draw.world(upload)
    }
    fn shots(
        &self,
        world: &str,
        shots: &[Shot],
        supersample: u32,
        format: PictureFormat,
        cancel: &AtomicBool,
    ) -> Result<Vec<Picture>, RenderError> {
        self.drawn
            .lock()
            .unwrap()
            .push(Value::Array(shots.iter().map(as_sent).collect()));
        self.asked("shots");
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

fn env(name: &str) -> PathBuf {
    PathBuf::from(std::env::var_os(name).unwrap_or_else(|| panic!("{name} is not set")))
}

fn sha(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let [_, data, cases, out] = &args[..] else {
        panic!("usage: see_build <data folder> <cases.json> <out.jsonl>");
    };
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(run(
        PathBuf::from(data),
        PathBuf::from(cases),
        PathBuf::from(out),
    ));
}

async fn run(data: PathBuf, cases: PathBuf, out: PathBuf) {
    let tables = env("MCBENCH_TABLES");
    let (minecraft, textures, worlds) = (
        env("MCBENCH_DIFF_MINECRAFT"),
        env("MCBENCH_DIFF_TEXTURES"),
        env("MCBENCH_DIFF_WORLDS"),
    );
    let adapter = match std::env::var("MCBENCH_DIFF_ADAPTER").as_deref() {
        Ok("software") => AdapterChoice::Software,
        Ok("hardware") => AdapterChoice::Hardware,
        other => panic!("MCBENCH_DIFF_ADAPTER is software or hardware, not {other:?}"),
    };
    let assets = Assets {
        pack: minecraft.join("mcbench-pack.bin"),
        textures: textures.join("textures.json"),
        atlas: textures.join("atlas.png"),
        atlas_map: textures.join("atlas_map.json"),
        block_table: minecraft.join("block-table.tsv"),
        render_layers: minecraft.join("render-layers.tsv"),
        entities: minecraft.join("mcbench-entities.json"),
        software_vulkan: std::env::var_os("MCBENCH_DIFF_SOFTWARE_VULKAN").map(PathBuf::from),
        client_jar: textures.join("client-entries.zip"),
        night: minecraft.join("night.json"),
    };
    let draw = tokio::task::spawn_blocking(move || {
        mcbench_draw::live::Draw::start(&assets, adapter, mcbench_draw::store::DEFAULT_BYTES)
    })
    .await
    .unwrap()
    .unwrap_or_else(|error| panic!("the renderer: {error:?}"));
    let report = draw.report();
    eprintln!(
        "see_build: Draw on {} ({}, {}, software {})",
        report.adapter.name,
        report.adapter.backend,
        report.adapter.device_type,
        report.adapter.software
    );
    let noted = Arc::new(Noted {
        draw,
        drawn: Mutex::new(Vec::new()),
        cancel: Mutex::new(None),
    });
    let engines = shared(&tables).unwrap();
    let limits = mcbench_store::Limits::from_env(|name| std::env::var(name).ok()).unwrap();
    let verifier = engines.verifier(data.join("minecraft-verify")).unwrap();
    let services = services(
        &engines,
        &data,
        limits,
        Arc::new(RecordedSandbox::new()),
        verifier,
        Arc::new(registry_from_table(&tables).unwrap()),
    )
    .unwrap();
    services
        .renderer
        .set(Ok(Arc::clone(&noted) as Arc<dyn Renderer>));
    let tools = Tools::new(Arc::clone(&services));
    let cases: Vec<Value> =
        serde_json::from_str(&std::fs::read_to_string(&cases).unwrap()).unwrap();
    let mut chats: HashMap<String, Chat> = HashMap::new();
    let mut pictures: HashMap<String, Vec<Vec<u8>>> = HashMap::new();
    let mut file = std::fs::File::create(&out).unwrap();
    for case in &cases {
        noted.drawn.lock().unwrap().clear();
        let key = case["key"].as_str().unwrap().to_string();
        let line = if let Some(montage) = case.get("montage") {
            let tiles: Vec<Vec<u8>> = pictures[montage["of"].as_str().unwrap()].clone();
            let cells: Vec<Cell> = serde_json::from_value(montage["cells"].clone()).unwrap();
            let size: [u32; 2] = serde_json::from_value(montage["size"].clone()).unwrap();
            let card = services.renderer.montage(tiles, cells, size).await.unwrap();
            json!({"key": key, "card": sha(&card)})
        } else {
            let id = case["chat"].as_str().unwrap().to_string();
            let chat = chats.entry(id.clone()).or_insert_with(|| {
                let name = case["world"].as_str().unwrap();
                let world = if name == "empty" {
                    World::new()
                } else {
                    let bytes = std::fs::read(worlds.join(format!("{name}.blueprint"))).unwrap();
                    World::from_blueprint(&mcbench_world::Blueprint(bytes), 0, None).unwrap()
                };
                Chat {
                    id: id.clone(),
                    model: "fixture".into(),
                    world,
                    workspace: Workspace {
                        chat: id.clone(),
                        host: data.join("sandbox").join(&id),
                    },
                    image_input: true,
                    chat: false,
                }
            });
            if let Some(edit) = case.get("edit").and_then(Value::as_array) {
                for block in edit {
                    let at = |i: usize| block[i].as_i64().unwrap() as i32;
                    chat.world
                        .set_block([at(0), at(1), at(2)], block[3].as_str().unwrap(), &[])
                        .unwrap();
                }
                chat.world.finalize();
            }
            let cancel = CancellationToken::new();
            *noted.cancel.lock().unwrap() = case
                .get("cancel")
                .and_then(Value::as_str)
                .map(|at| (at.to_string(), cancel.clone()));
            let arguments: Map<String, Value> = case["arguments"].as_object().unwrap().clone();
            let executed = tools
                .execute(chat, "see_build", &arguments, &mut |_| {}, &cancel)
                .await;
            *noted.cancel.lock().unwrap() = None;
            let (outcome, threw) = match executed {
                Ok(outcome) => (Some(outcome), None),
                Err(error) => (None, Some(error.0)),
            };
            let urls: Vec<String> = outcome
                .as_ref()
                .map(|o| match &o.screenshots {
                    Some(shots) => shots.images.iter().map(|i| i.url.clone()).collect(),
                    None => o.images.iter().map(|i| i.url.clone()).collect(),
                })
                .unwrap_or_default();
            let bytes: Vec<Vec<u8>> = urls
                .iter()
                .map(|url| std::fs::read(services.store.file_path(url).unwrap()).unwrap())
                .collect();
            let images: Vec<String> = bytes.iter().map(|b| sha(b)).collect();
            pictures.insert(key.clone(), bytes);
            json!({
                "key": key,
                "content": outcome.as_ref().map(|o| o.content("see_build")),
                "screenshots": outcome.as_ref().and_then(|o| o.screenshots.as_ref()).map(|s| json!({"text": s.text, "images": s.images.len()})),
                "images": images,
                "drawn": noted.drawn.lock().unwrap().clone(),
                "threw": threw,
            })
        };
        writeln!(file, "{}", mcbench_json::stringify(&line)).unwrap();
        eprintln!("see_build: {key}");
    }
}
