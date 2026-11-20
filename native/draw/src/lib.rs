// mcbench-draw: see_build's pictures drawn natively. The mesher's quads
// (renderer/) with see_build's shading, camera, sky, fog, sizes and
// supersample, entities from the bake's entity capture, the labelled grid,
// JPEG; and the checks that define the same picture (compare.rs).
// PROTOCOL.md is the helper's contract with the server.

pub mod assets;
pub mod blueprint;
pub mod camera;
pub mod compare;
pub mod entities;
#[cfg(target_os = "linux")]
pub mod floor;
pub mod gpu;
pub mod live;
pub mod memory;
pub mod picture;
pub mod scene;
pub mod serve;
pub mod store;
pub mod synthetic;
#[cfg(windows)]
pub mod warp;

// Milliseconds from a fixed start.
pub fn now_ms() -> f64 {
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    START
        .get_or_init(std::time::Instant::now)
        .elapsed()
        .as_secs_f64()
        * 1e3
}

pub fn round2(ms: f64) -> f64 {
    (ms * 100.0).round() / 100.0
}

pub const VERSION: &str = concat!("mcbench-draw ", env!("CARGO_PKG_VERSION"));
