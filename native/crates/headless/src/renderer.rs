//! The renderer: `mcbench_draw::live::Draw` (the drawing crate,
//! `mcbench_render::Renderer`'s implementation) on the platform's hardware
//! adapter ([`draw`]), started once per process and shared by every
//! `see_build` call through `mcbench_tools::shots::SharedRenderer`. The
//! app starts it for its live view and gives the same one to its tools
//! (`mcbench_lite::core::live`); `headless turn` and `headless shots` start
//! their own ([`process`]) from the shipped data at `MCBENCH_TABLES` and the
//! baked textures at `MCBENCH_TEXTURES`.

use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use mcbench_draw::live::Draw;
use mcbench_render::{AdapterChoice, Assets, RenderError, Renderer};
use mcbench_tools::shots::SharedRenderer;

/// The bytes the renderer may hold: the helper's default on a hardware
/// adapter (`native/draw/PROTOCOL.md`, What is kept: 1 GiB).
pub const MEMORY_LIMIT: u64 = mcbench_draw::store::DEFAULT_BYTES;

/// The renderer: `Draw` on the platform's hardware adapter with
/// [`MEMORY_LIMIT`]. It opens the adapter and reads the assets, which
/// takes seconds on a cold machine: never call it on the window's thread
/// or a runtime worker.
pub fn draw(assets: &Assets) -> Result<Draw, RenderError> {
    Draw::start(assets, AdapterChoice::Hardware, MEMORY_LIMIT)
}

/// A renderer's failure as its sentence.
pub fn sentence(error: RenderError) -> String {
    match error {
        RenderError::UnknownWorld(text)
        | RenderError::DeviceLost(text)
        | RenderError::NoAdapter(text)
        | RenderError::Stalled(text)
        | RenderError::Failed(text) => text,
        RenderError::Cancelled => "The renderer's request was cancelled".into(),
    }
}

/// The renderer's files for a headless process: the shipped data at
/// `MCBENCH_TABLES` (`minecraft/<id>` of a data folder, which holds the
/// engine's tables too) and the baked textures at `MCBENCH_TEXTURES`
/// (`textures/<id>`).
pub fn assets() -> Result<Assets, String> {
    let folder = |name: &str, what: &str| {
        std::env::var_os(name).map(PathBuf::from).ok_or(format!(
            "{name} names no folder: see_build draws with {what}"
        ))
    };
    let data = folder(
        "MCBENCH_TABLES",
        "the shipped Minecraft data (minecraft/<id> of a data folder)",
    )?;
    let textures = folder(
        "MCBENCH_TEXTURES",
        "the baked textures (textures/<id> of a data folder)",
    )?;
    Ok(mcbench_setup::start::assets(
        &mcbench_setup::Data::at(&data),
        &mcbench_setup::Textures::at(&textures),
    ))
}

/// One line for a log saying what draws: the drawing crate's version and
/// the adapter it took (`[renderer] mcbench-draw 0.1.0: <name> (<backend>,
/// <device type>, hardware|software)`).
pub fn adapter_line(renderer: &dyn Renderer) -> String {
    let report = renderer.report();
    let adapter = &report.adapter;
    format!(
        "[renderer] {}: {} ({}, {}, {})",
        report.version,
        adapter.name,
        adapter.backend,
        adapter.device_type,
        if adapter.software {
            "software"
        } else {
            "hardware"
        }
    )
}

/// This process's renderer: started at the first call, on the blocking
/// pool of the runtime it is called on, from [`assets`]; a `see_build`
/// call before it has started waits for it. Its start writes one line to
/// stderr: the adapter ([`adapter_line`]) or why it did not start.
pub fn process() -> Result<Arc<SharedRenderer>, String> {
    static PROCESS: OnceLock<Arc<SharedRenderer>> = OnceLock::new();
    if let Some(shared) = PROCESS.get() {
        return Ok(Arc::clone(shared));
    }
    let assets = assets()?;
    let shared = PROCESS.get_or_init(|| {
        let shared = SharedRenderer::new();
        let starting = Arc::clone(&shared);
        tokio::task::spawn_blocking(move || {
            let started = draw(&assets)
                .map(|draw| Arc::new(draw) as Arc<dyn Renderer>)
                .map_err(sentence);
            match &started {
                Ok(renderer) => eprintln!("{}", adapter_line(renderer.as_ref())),
                Err(error) => eprintln!("[renderer] {error}"),
            }
            starting.set(started);
        });
        shared
    });
    Ok(Arc::clone(shared))
}

/// `headless turn`'s renderer: [`process`], or, where the renderer's files
/// are not named, one that did not start and says so. A turn that never
/// calls `see_build` needs none of them, as before `see_build` was drawn;
/// a `see_build` call then answers why it cannot draw
/// (`Screenshot initialization: MCBENCH_TEXTURES names no folder: …`).
pub fn for_turn() -> Arc<SharedRenderer> {
    process().unwrap_or_else(|error| {
        let unnamed = SharedRenderer::new();
        unnamed.set(Err(error));
        unnamed
    })
}
