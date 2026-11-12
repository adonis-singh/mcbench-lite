// mcbench-core: Minecraft's block-update engine ported from the pinned
// server's code. The same library is the native program (main.rs, speaking
// the Java engine's protocol over files, McbenchEngine's) and the
// WebAssembly module (wasm.rs, the live world in a browser): a Session is
// one world, and both front ends only move its bytes.
//
// A request that reaches something this port does not model fails with a
// reason starting "unported:", and the level it leaves takes only fresh
// placements after.

mod blocks;
mod clip;
mod chunk_save;
mod codec;
mod components;
mod collision;
mod dir;
mod engine;
mod entity;
mod explosion;
mod fast;
mod game_event;
mod heightmap;
mod fluid;
mod jfmt;
mod jmath;
mod jmap;
mod mth;
mod json;
mod level;
mod light;
mod loot;
mod nbt;
mod phys;
// The creative player's movement (scripts/movement-parity.js checks it against the client's own classes).
pub mod player;
mod random;
mod registry;
mod shape_ops;
mod text;
mod ticks;
mod world;

pub mod session;
// The verifier writes its reports as files, and the probe catches panics.
#[cfg(not(target_arch = "wasm32"))]
pub mod probe;
#[cfg(not(target_arch = "wasm32"))]
pub mod verify;
// Minecraft's data fixer, for what a verifier experiment imports at an older
// data version (scripts/datafix-parity.js checks it against the server's).
#[cfg(not(target_arch = "wasm32"))]
pub mod datafix;
#[cfg(target_arch = "wasm32")]
mod wasm;

use level::Refusal;
use std::any::Any;

const REGISTRY: &str = include_str!("../minecraft/1.21.10/blocks.txt");

// What Minecraft's registries hold, read once and kept for the process.
pub fn boot() -> (&'static registry::Registry, &'static blocks::Blocks) {
    let reg: &'static registry::Registry = Box::leak(Box::new(registry::Registry::parse(REGISTRY)));
    let blocks: &'static blocks::Blocks = Box::leak(Box::new(blocks::Blocks::new(reg)));
    (reg, blocks)
}

// Why a request panicked, as the protocol's failure reply words it: a
// refusal ("unported: ...") or a bug of this port.
pub fn failure(payload: &(dyn Any + Send)) -> String {
    match payload.downcast_ref::<Refusal>() {
        Some(refusal) => format!("unported: {}", refusal.0),
        None => {
            let text = payload.downcast_ref::<String>().cloned().or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_default();
            format!("mcbench-core failed: {}", text.replace('\n', " "))
        }
    }
}

// Whether a panic payload is a refusal rather than a bug.
pub fn is_refusal(payload: &(dyn Any + Send)) -> bool {
    payload.is::<Refusal>()
}
