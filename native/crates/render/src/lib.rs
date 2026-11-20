//! # mcbench-render
//!
//! The renderer's interface: the trait the core calls to draw `see_build`'s
//! pictures (and, from stage four, the live view), and the types that cross
//! it. It is modelled on what the stage-two helper does today
//! (`native/draw/PROTOCOL.md` on branch `native-stage2`): it is given its
//! assets and picks an adapter once ([`Renderer::start`], the helper's
//! `hello`), keeps worlds by name with a base for edits ([`Renderer::world`]),
//! draws shots exactly as `server/shot.js` `resolveShot` resolves them
//! ([`Renderer::shots`]), composes a bench card ([`Renderer::montage`]),
//! reports its adapter ([`AdapterReport`]) and keeps what it holds under a
//! memory limit ([`Limits::memory_limit`]). The implementation is the drawing
//! crate `mcbench-draw` (`native/draw` on `native-stage2`), over the mesher
//! (`renderer/`).
//!
//! ## Owner
//!
//! The stage-two session, which also arbitrates seam disputes between the
//! sessions (`native/README.md`). The types here are this crate's own: it
//! does not depend on `mcbench-world` (worlds cross as the engine's
//! blueprint bytes, as they do to the helper today), so no type is shared
//! with another owner's crate.
//!
//! ## Seams
//!
//! | Seam | What crosses | Who calls whom | Thread or process |
//! |---|---|---|---|
//! | tools | [`WorldUpload`] (the engine's blueprint bytes, the tick's entities as JSON, a base to patch), [`Shot`]s as resolved, [`PictureFormat`]; [`WorldReport`], [`Picture`]s, [`RenderError`] back | `mcbench-tools`' `see_build` and share call the trait | the async runtime calls; the implementation draws on the render thread that owns the `wgpu` device, one request at a time in arrival order |
//! | cancel | an [`AtomicBool`] the caller sets; a cancelled `shots` stops before its next shot with [`RenderError::Cancelled`] | the agent's stop sets it | any thread |
//! | assets | [`Assets`]: the pack, texture names, the block atlas and its map, the block table, render layers, the entity capture, the client jar, Linux's software Vulkan driver | `mcbench-setup` provides the files; the implementation reads them in `start` | the render thread |
//! | the stage-two helper | until the Node server is retired, the same requests as frames over the helper's stdin and stdout (`PROTOCOL.md`) | `server/shot.js` starts `mcbench-draw serve` | a child process of the Node server |
//!
//! ## Behaviour defined by
//!
//! Today's tests the stage-two helper replaces (docs/native-app.md, Stage
//! two): `shot-renderer.test.js` 7 (lifecycle: a stalled warm-up, a lost
//! device recreated once, close, a crashed helper restarted),
//! `shot-context.test.js` 2, `shot-cancellation.test.js` 1,
//! `shot-recovery.test.js` 1, `shot-browser.integration.test.js` 1,
//! `shot-framing.test.js` 4, `gpu-mesh.test.js` 1, `viewer-draw.test.js` 5,
//! `viewer-reset.test.js` 2, `light.test.js` 5, `light-mesher.test.js` 2
//! (through `mesher-parity --paper`), `entities.test.js` 15,
//! `entity-capture.test.js` 4 and `viewer-entities.test.js` 2: 52 tests in 14
//! files, and the picture thresholds of docs/native-app.md (The definition
//! of the same picture) on Metal, Vulkan, the software floor and WARP. The
//! drawing crate's own tests (`native/draw/tests/` on `native-stage2`) hold
//! an edit's result equal to a world sent whole.
//!
//! ## How the drawing crate joins this workspace
//!
//! When stage two merges to `main` and `native-app` merges `main`,
//! `native/draw` stays where it is, with its own `Cargo.lock` and its empty
//! `[workspace]` table, and this workspace takes it by path
//! (`mcbench-draw = { path = "draw" }` in `[workspace.dependencies]`;
//! `native/Cargo.toml` already excludes `draw` from the members, without
//! which cargo makes a path dependency inside the workspace folder a member
//! and refuses the second workspace root). The
//! helper's released version is the hash of `native/draw`, `renderer/src`,
//! `renderer/Cargo.toml` and `core/rust-toolchain.toml` (`server/draw.js`
//! `sourcesHash` on `native-stage2`); as a workspace member its lock would be
//! this workspace's, and every dependency another session adds would move
//! the helper's version and need a new helper release. A path dependency
//! resolves its dependencies through `native/Cargo.lock`, so the in-process
//! renderer can build with other patch versions than the helper's own lock
//! holds; the drawing crate pins the ones that matter (`wgpu` and the
//! encoders) with `=` versions, or `native/Cargo.lock` is updated to the same
//! versions when it joins. Checked on adipc with `native-stage2` 0ca58bb's
//! `native/draw` and `renderer/` as a path dependency of `mcbench-tools`:
//! `cargo check` passed, no file of `native/draw`, `renderer/` or `core/`
//! changed (their `Cargo.lock` included), and the workspace resolved the
//! same versions as the drawing crate's lock for `wgpu` 30.0.2,
//! `jpeg-encoder` 0.7.1, `image` 0.25.10, `fontdue` 0.9.5, `serde` and
//! `serde_json`, and `zip` 9.6.1 where its lock has 9.6.0.
//!
//! What moves: `mcbench-draw` implements [`Renderer`] and takes its seam
//! types from here (it depends on `native/crates/render` by path, and
//! `server/draw.js`'s sources gain that folder): `camera::Camera` and
//! `Ortho`, `picture::Grid`, `Lines`, `Edge`, `Label` and `Cell`,
//! `store::Shot` and `Clip`, and `assets::Paths` are replaced by the types
//! below, which carry the same fields and JSON names, so the helper's
//! protocol reads the same JSON. `serve.rs` and `PROTOCOL.md` stay while the
//! Node server starts the helper and go with it (stage 3c); the crate then
//! moves to `native/crates/draw` as a member.
//!
//! ## The live view
//!
//! The 3D pane's seam is [`live`]: the window holds one [`live::LivePane`]
//! and takes [`live::Frame`]s from it in its paint; the core feeds the shown
//! world through [`live::LiveRenderer`] (every [`Renderer`] is one), with the
//! engine's tick patches as they are. Agreed by the renderer and interface
//! sessions on 2026-11-17; the module's documentation says how frames are
//! paced, what a pick casts against and what night is.
//!
//! ## Open
//!
//! Whether the trait's calls are `async fn` or block the caller until the
//! render thread answers is settled with the app's async runtime (stage
//! three); the requests and replies are not open.

pub mod live;

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

/// The files the renderer draws with (`PROTOCOL.md` `hello`'s `assets`),
/// all absolute paths.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Assets {
    /// `mcbench-pack.bin`: the mesher's pack.
    pub pack: PathBuf,
    /// `textures.json`: the pack's texture names by material index.
    pub textures: PathBuf,
    /// `see_build`'s block texture atlas.
    pub atlas: PathBuf,
    /// Each block texture's box in the atlas.
    #[serde(rename = "atlasMap")]
    pub atlas_map: PathBuf,
    /// The block table: full faces per state.
    #[serde(rename = "blockTable")]
    pub block_table: PathBuf,
    /// `render-layers.tsv`.
    #[serde(rename = "renderLayers")]
    pub render_layers: PathBuf,
    /// `mcbench-entities.json`: the entity capture.
    pub entities: PathBuf,
    /// Linux only: the shipped SwiftShader driver, where the build ships one.
    #[serde(rename = "softwareVulkan", default)]
    pub software_vulkan: Option<PathBuf>,
    /// The client jar the entity textures are read from.
    #[serde(rename = "clientJar")]
    pub client_jar: PathBuf,
    /// `night.json`: Minecraft's night for the live view (the extractor's
    /// night step, `server/McbenchNight.java`, shipped with the data).
    pub night: PathBuf,
}

/// Which adapter to draw on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AdapterChoice {
    /// The first hardware adapter of the platform's backends, else the
    /// software floor (`PROTOCOL.md`, The adapter).
    Hardware,
    /// The software floor even where a hardware adapter exists (tests, CI).
    Software,
}

/// The adapter drawn on (`hello`'s reply `adapter`).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Adapter {
    /// `wgpu`'s adapter name.
    pub name: String,
    /// The backend: Metal, Vulkan, Gl or Dx12.
    pub backend: String,
    /// `wgpu`'s device type.
    #[serde(rename = "deviceType")]
    pub device_type: String,
    /// The driver.
    pub driver: String,
    /// The driver's own description.
    #[serde(rename = "driverInfo")]
    pub driver_info: String,
    /// Whether `wgpu` reports a CPU rasteriser.
    pub software: bool,
}

/// What the renderer keeps and draws at most (`hello`'s reply `limits`).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Limits {
    /// The largest texture side.
    #[serde(rename = "maxTextureSide")]
    pub max_texture_side: u32,
    /// The largest buffer.
    #[serde(rename = "maxBufferBytes")]
    pub max_buffer_bytes: u64,
    /// The largest picture side.
    #[serde(rename = "maxPictureSide")]
    pub max_picture_side: u32,
    /// Worlds kept (4 on a hardware adapter, 2 on a software one).
    #[serde(rename = "worldsKept")]
    pub worlds_kept: u32,
    /// Scenes kept (6 and 3).
    #[serde(rename = "scenesKept")]
    pub scenes_kept: u32,
    /// The bytes kept between requests (default 805,306,368, 768 MiB).
    #[serde(rename = "memoryLimit")]
    pub memory_limit: u64,
}

/// The adapter report (`hello`'s reply).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AdapterReport {
    /// `mcbench-draw <crate version>`.
    pub version: String,
    /// The adapter.
    pub adapter: Adapter,
    /// The limits.
    pub limits: Limits,
}

/// A world to keep under a name (`PROTOCOL.md` `world`).
#[derive(Clone, Debug)]
pub struct WorldUpload {
    /// The world's name (the core's: conversation, revision, tick).
    pub name: String,
    /// The page's height offset (`minecraftLive.yOffset`).
    pub y_offset: i32,
    /// The engine's blueprint bytes, the whole world.
    pub blueprint: Vec<u8>,
    /// The entities of the tick drawn, UTF-8 JSON as `server/shot-world.js`
    /// keeps them.
    pub entities: Option<Vec<u8>>,
    /// The kept world this one is an edit of, and whether to keep that base
    /// beside it.
    pub base: Option<(String, bool)>,
}

/// What a kept world holds (`world`'s reply, less its timings).
#[derive(Clone, Debug)]
pub struct WorldReport {
    /// Block quads.
    pub quads: u64,
    /// Entity quads.
    pub entity_quads: u64,
    /// Tiles.
    pub tiles: u64,
    /// Blocks.
    pub blocks: u64,
    /// The box of the blocks, at the page's heights.
    pub bounds: Clip,
    /// GPU geometry in bytes.
    pub bytes: u64,
    /// Textures the block atlas lacks (their quads are not drawn).
    pub missing_textures: Vec<String>,
    /// `Some(tiles meshed)` when the upload was applied to its base as an
    /// edit (`patched: true`, `tilesMeshed`).
    pub tiles_meshed: Option<u64>,
}

/// A box of blocks, both corners included.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Clip {
    /// The lowest corner.
    pub min: [i32; 3],
    /// The highest corner.
    pub max: [i32; 3],
}

/// An orthographic lens.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Ortho {
    /// Half the view's width, in blocks.
    #[serde(rename = "halfWidth")]
    pub half_width: f64,
    /// Half its height.
    #[serde(rename = "halfHeight")]
    pub half_height: f64,
    /// The near plane.
    pub near: f64,
    /// The far plane.
    pub far: f64,
}

/// A shot's camera, in the viewer's convention (`resolveShot`: forward =
/// [cos p sin y, -sin p, -cos p cos y], eye = focus - forward * focusDist),
/// with a perspective `fov` or an `ortho` lens.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Camera {
    /// Yaw in radians.
    pub yaw: f64,
    /// Pitch in radians.
    pub pitch: f64,
    /// The point looked at.
    pub focus: [f64; 3],
    /// The eye's distance from it.
    #[serde(rename = "focusDist")]
    pub focus_dist: f64,
    /// The vertical field of view in degrees, for a perspective shot.
    #[serde(default)]
    pub fov: Option<f64>,
    /// The lens of an orthographic shot.
    #[serde(default)]
    pub ortho: Option<Ortho>,
}

/// A grid line.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Edge {
    /// Where, in pixels.
    pub at: f64,
    /// A major line.
    pub major: bool,
}

/// A grid label.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Label {
    /// Where, in pixels.
    pub at: f64,
    /// The text.
    pub text: String,
}

/// The lines and labels of one axis of a grid.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Lines {
    /// The lines.
    pub edges: Vec<Edge>,
    /// The labels.
    pub labels: Vec<Label>,
}

/// The labelled grid of an orthographic shot (`resolveShot` `gridFor`).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Grid {
    /// The columns.
    pub columns: Lines,
    /// The rows.
    pub rows: Lines,
    /// The legend the model reads in `described`.
    pub legend: String,
    /// Blocks between lines.
    pub step: f64,
}

/// A shot exactly as `server/shot.js` `resolveShot` returns it, less
/// `described` (which the renderer does not read).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Shot {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// The camera.
    pub camera: Camera,
    /// Only the blocks (and entities) inside it are drawn.
    pub clip: Option<Clip>,
    /// The grid, for straight orthographic shots.
    pub grid: Option<Grid>,
}

/// How pictures are encoded.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PictureFormat {
    /// JPEG at a quality of 1 to 100 (today 90).
    Jpeg(u8),
    /// PNG: the same picture before the JPEG, for comparing adapters.
    Png,
}

/// One drawn shot.
#[derive(Clone, Debug)]
pub struct Picture {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// The encoded picture.
    pub bytes: Vec<u8>,
}

/// One cell of a bench card (`server/shot.js` `montageCells`).
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Cell {
    /// Left edge.
    pub x: u32,
    /// Top edge.
    pub y: u32,
    /// Width.
    pub width: u32,
    /// Height.
    pub height: u32,
}

/// Why a request failed (`PROTOCOL.md`, Errors). Each message is a plain
/// sentence.
#[derive(Clone, Debug)]
pub enum RenderError {
    /// `No world named <name> is kept`: the caller sends the world again
    /// (without a base) and retries.
    UnknownWorld(String),
    /// `The graphics device was lost: <wgpu's reason>`: the caller retries
    /// the request once, as today's server retries a call on a fresh helper
    /// (`server/shot.js` `retryOnce`). The renderer makes the device again
    /// once in place; after a second loss the next request makes the
    /// renderer again from nothing, as a fresh helper.
    DeviceLost(String),
    /// A `shots` request cancelled before its next shot.
    Cancelled,
    /// The renderer could not start: today's helper's `hello` failed, with
    /// its sentence as it is (`Gpu::open`'s, such as `No Metal device`, or
    /// its files'). At `start`, or when a request made the renderer again;
    /// either is today's step `initialization`.
    NoAdapter(String),
    /// A step of a request (or the start of a fresh renderer for it) did
    /// not finish within today's bound of 120 s: the whole sentence today's
    /// server answers (`server/shot.js` `stage`: `Screenshot <step> stalled
    /// for 120s; renderer reset, screenshot unavailable`), given to the
    /// model as it is. The renderer has left the stalled work behind; the
    /// next request starts it afresh, holding no world.
    Stalled(String),
    /// Any other failure, with its sentence.
    Failed(String),
}

/// The renderer the core draws with. One implementation, `mcbench-draw`;
/// tests of `mcbench-tools` write fakes. It also draws the live view
/// ([`live::LiveRenderer`]).
pub trait Renderer: live::LiveRenderer + Send + Sync {
    /// Opens the adapter and reads the assets (the helper's `hello`).
    fn start(
        assets: &Assets,
        adapter: AdapterChoice,
        memory_limit: u64,
    ) -> Result<Self, RenderError>
    where
        Self: Sized;
    /// The adapter and limits `start` found.
    fn report(&self) -> &AdapterReport;
    /// Keeps a world under its name, replacing one of the same name, meshing
    /// only what an edit of `base` changed.
    fn world(&self, upload: WorldUpload) -> Result<WorldReport, RenderError>;
    /// Draws `shots` of a kept world at `supersample` times their size,
    /// scaled down (today 2), in order.
    fn shots(
        &self,
        world: &str,
        shots: &[Shot],
        supersample: u32,
        format: PictureFormat,
        cancel: &AtomicBool,
    ) -> Result<Vec<Picture>, RenderError>;
    /// One JPEG of `jpegs` placed in `cells` on a card of `size`.
    fn montage(
        &self,
        size: [u32; 2],
        cells: &[Cell],
        jpegs: &[&[u8]],
        quality: u8,
    ) -> Result<Vec<u8>, RenderError>;
    /// Frees a world and its scenes; false when it was not kept. The world
    /// the live view shows is kept while it is: dropping it fails with a
    /// sentence.
    fn drop_world(&self, world: &str) -> Result<bool, RenderError>;
    /// Stops the renderer as the app quits, before the process exits: its
    /// thread stops, the GPU's work is waited for and the device is let go,
    /// so nothing calls into the graphics driver while the driver's own exit
    /// handlers run. Returns once that is done. After it, `world`, `shots`
    /// and `drop_world` fail with `Failed("The renderer was shut down")`,
    /// and the live view's calls do nothing. Called once by the app, after
    /// the live world stopped.
    fn shutdown(&self);
}

#[cfg(test)]
mod seam {
    use super::*;

    struct FakeRenderer;
    impl live::LiveRenderer for FakeRenderer {
        fn live(&self) -> std::sync::Arc<dyn live::LivePane> {
            unimplemented!("a fake for the seam test")
        }
        fn live_show(&self, _: Option<live::Shown>) {}
        fn live_patch(&self, _: live::LivePatch) {}
    }
    impl Renderer for FakeRenderer {
        fn start(_: &Assets, _: AdapterChoice, _: u64) -> Result<Self, RenderError> {
            unimplemented!("a fake for the seam test")
        }
        fn report(&self) -> &AdapterReport {
            unimplemented!("a fake for the seam test")
        }
        fn world(&self, _: WorldUpload) -> Result<WorldReport, RenderError> {
            unimplemented!("a fake for the seam test")
        }
        fn shots(
            &self,
            _: &str,
            _: &[Shot],
            _: u32,
            _: PictureFormat,
            _: &AtomicBool,
        ) -> Result<Vec<Picture>, RenderError> {
            unimplemented!("a fake for the seam test")
        }
        fn montage(
            &self,
            _: [u32; 2],
            _: &[Cell],
            _: &[&[u8]],
            _: u8,
        ) -> Result<Vec<u8>, RenderError> {
            unimplemented!("a fake for the seam test")
        }
        fn drop_world(&self, _: &str) -> Result<bool, RenderError> {
            unimplemented!("a fake for the seam test")
        }
        fn shutdown(&self) {
            unimplemented!("a fake for the seam test")
        }
    }

    #[test]
    fn a_fake_can_be_written_against_the_trait() {
        let _: Box<dyn Renderer> = Box::new(FakeRenderer);
    }

    #[test]
    fn a_resolved_shot_reads_as_the_helper_reads_it() {
        let shot: Shot = serde_json::from_str(
            r#"{"width":2184,"height":1640,"camera":{"yaw":-0.785,"pitch":0.524,"focus":[0,64,0],"focusDist":40,"fov":70},"clip":null,"grid":null}"#,
        )
        .unwrap();
        assert_eq!(shot.camera.fov, Some(70.0));
    }
}
