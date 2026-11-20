//! # mcbench-world
//!
//! The voxel world a chat builds in, the engine's blueprint bytes, block
//! states and the viewer's selections: the types every server-side crate and
//! the interface share. It is a port of `server/world.js` (chunked sparse
//! voxels in 16³ chunks, copy-on-write clones, palette references),
//! `blueprint.js`, `block-states.js`, `live-placement.js`, `live-views.js`,
//! `bench-world.js`, `selection.js`, `minecraft-dimension.js`, `region.js`,
//! `zip-entry.js`, the parent's side of `build-transfer.js` and the page's
//! `web/world-export.mjs`, a module per file:
//!
//! | Module | Today's file |
//! |---|---|
//! | [`world`], [`chunk`], [`chunk_map`], [`palette`], [`snapshot`] | `world.js` |
//! | [`blueprint`] | `blueprint.js` |
//! | [`block_states`] | `block-states.js` |
//! | [`facts`] | the part of `minecraft-facts.js` the import and the region reader ask (`lastPass`, `isAir`, `stateId`) |
//! | [`live_placement`] | `live-placement.js` |
//! | [`live_views`] | `live-views.js` (and `bluemap.js` `renderMask`, which its descriptor carries) |
//! | [`bench`](mod@bench) | `bench-world.js` |
//! | [`selection`] | `selection.js` |
//! | [`dimension`] | `minecraft-dimension.js` |
//! | [`region`] | `region.js` |
//! | [`export`] | `web/world-export.mjs` (the Minecraft save the viewer exports) |
//! | [`zip`] | `zip-entry.js` |
//! | [`transfer`] | `build-transfer.js` |
//! | [`js`] | what the output takes from JavaScript (`JSON.stringify`'s strings, key order, `sort`, `localeCompare`) |
//!
//! Every text a model or the page can read out of these modules (the
//! coordinate and block-limit errors, block-state errors, the selection's
//! label, the place commands) is today's, byte for byte; the differential
//! harness (`tests/differential.rs` with `tests/node/diff.js`) holds it to
//! Node on every stored world of a data folder.
//!
//! ## Owner
//!
//! The stage-three session. These are the shared world and blueprint types:
//! a change to a public type here is a seam change for every crate that
//! names it (engine, store, build, tools, agent, ui), proposed to the other
//! owners first (`native/README.md`). The renderer's interface
//! (`mcbench-render`) deliberately does not depend on this crate: it takes
//! the engine's blueprint bytes, as the stage-two helper does, so the
//! renderer's owner and this crate's owner do not share a type.
//!
//! ## Seams
//!
//! | Seam | What crosses | Who calls whom | Thread or process |
//! |---|---|---|---|
//! | engine | [`Blueprint`] bytes out of a placement; the live import's command text into it ([`PlaceCommands`]) | `mcbench-engine` reads and writes these | the engine's worker threads |
//! | build | a [`World`] cloned before the program and the changes the child sends back ([`transfer`]: `encode_world`, `apply_changes`) | `mcbench-build` reads a `World` and applies changes to a clone | the async runtime; the V8 child is another process |
//! | store | [`WorldManifest`] and chunk files under `worlds/<chat>/chunks/` | `mcbench-store` calls [`World::from_manifest_cached`] and [`World::persist`] | the store's writer |
//! | tools | `World` and [`BlockBox`] for `see_build`'s framing and `test_build`'s request; [`snapshot`] for the world route | `mcbench-tools` reads | the async runtime |
//! | app | the engine's export bytes in ([`export::EngineExport::read`]), the save's zip out ([`export::world_zip`]) for `ExportWorld` | the core calls it with the times it read from the clock | CPU work: `spawn_blocking` or an engine thread |
//! | ui | [`Selection`] parts the viewer makes, sent with a message ([`selection`]) | `mcbench-ui` builds them, the agent stores them in the user message | the interface thread to the core |
//!
//! ## Behaviour defined by
//!
//! Today's tests (docs/native-app.md, Appendix A), 72 tests in 12 files:
//! `world.test.js` 8, `block-states.test.js` 8, `blueprint.test.js` 3,
//! `bench-world.test.js` 3, `live-placement.test.js` 6, `live-views.test.js`
//! 5, `minecraft-dimension.test.js` 2, `selection.test.js` 4,
//! `import-data-version.test.js` 2 (with `mcbench-engine`),
//! `live-patch.test.js` 3, `core-world.test.js` 11 (with the mesher) and
//! `live-world.test.js` 17 (with `mcbench-core`). The ones whose subject is
//! this crate are ported in `tests/`, named after today's file; the
//! differential harness compares this crate with Node on a data folder
//! (`MCBENCH_DIFF_DATA`, `MCBENCH_NODE_DIFF`; see `tests/differential.rs`).
//!
//! ## Decided
//!
//! The chunk representation and its sharing: today's, a sparse
//! insertion-ordered map up to 512 blocks and 4096 `u16` past it, each chunk
//! an `Arc` shared by the worlds that hold it and copied on a world's first
//! write to it (`Arc::make_mut`). Measured against Node in-process on adipc
//! (2026-11-17, load 7.2 to 7.9, medians of 5 alternated runs) on the
//! 2,752,373- and 2,139,980-block chats: loading 2.5 to 2.8 times faster,
//! clone and one set 3.2 to 3.8, persist 1.4 to 3.9, blueprint 10.5 to
//! 13.6, fill of 3M blocks 1.8; every operation measured is faster, so no
//! arena was needed. The sparse map's insertion order is kept because the
//! build transfer writes it.
//!
//! The build transfer is encoded from the chunks directly
//! ([`transfer::encode_world`] writes each chunk's cells into one buffer,
//! with no intermediate copy): on the same chats encoding is 3.0 to 7.0
//! times faster than Node and the codec's round trip to the child and back
//! 2.2 to 2.5 times (13.5 ms against 33.7 ms for the 2.1M world).
//!
//! The world export's compressed data is flate2's deflate (miniz_oxide) at
//! zlib's default level, not the page's bytes: Node's `CompressionStream`
//! is Chromium's zlib, whose deflate streams no Rust backend reproduces
//! (miniz_oxide, zlib-rs and stock C zlib were tried at every level), so
//! the differential holds the containers, headers, trailers and layout to
//! the page byte for byte and the chunks and `level.dat` after inflating.
//!
//! Differences from today that no caller reaches, kept and known: a world
//! made by [`World::reference`] from a world with writes since its last
//! finalize does not see that world's later writes to the same chunks
//! (today's object aliasing); a fill whose box has min past max on an axis
//! leaves an empty chunk as today's does, after which [`World::bounds`]
//! panics where today's throws a TypeError (`mc-dsl.js` orders every box);
//! [`js::locale_cmp`] refuses text past printable ASCII (property names are
//! checked against the block registry before they reach a world);
//! [`selection::read_selection`] keeps corners within 2^53 and drops the
//! half of a surrogate pair its 120-unit cut splits, which a Rust string
//! cannot hold alone.

pub mod bench;
pub mod block_states;
pub mod blueprint;
pub mod chunk;
pub mod chunk_map;
pub mod dimension;
pub mod export;
pub mod facts;
pub mod js;
pub mod live_placement;
pub mod live_views;
pub mod palette;
pub mod region;
pub mod selection;
pub mod snapshot;
pub mod transfer;
pub mod world;
pub mod zip;

use serde::{Deserialize, Serialize};

pub use chunk::{CHUNK_SIZE, CHUNK_VOLUME, DENSE_THRESHOLD};
pub use palette::{MAX_PALETTE, Palette};
pub use selection::Selection;
pub use world::{ChunkCache, KEY_OFFSET, World, coords_of, key_of};

/// The largest coordinate on any axis, as `server/world.js` `MAX_COORD`.
pub const MAX_COORD: i32 = 4096;
/// The most blocks a world holds, as `server/world.js` `MAX_BLOCKS`.
pub const MAX_BLOCKS: usize = 3_000_000;
/// Air's id: a cell set to it holds nothing.
pub const AIR: &str = "minecraft:air";

/// A block's position, at Minecraft's heights.
pub type BlockPos = [i32; 3];

/// A box of blocks, both corners included, as `server/shot.js` `blockBox`
/// and `server/selection.js` read them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BlockBox {
    /// The lowest corner.
    pub min: BlockPos,
    /// The highest corner, included.
    pub max: BlockPos,
}

/// A block state as a model writes it and the world keeps it: an id and its
/// properties in the order a JavaScript object keeps them (`World.set(x, y,
/// z, id, states)`). Equality is by id and the properties sorted, as
/// `server/world.js` `stateSignature`; the order is kept because text a model
/// reads lists the properties as they were stored.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BlockState {
    /// The namespaced id, `minecraft:stone`.
    pub id: String,
    /// The properties, in insertion order.
    pub properties: Vec<(String, String)>,
}

impl BlockState {
    /// A state of `id` with `properties`.
    pub fn new(id: &str, properties: &[(&str, &str)]) -> BlockState {
        BlockState {
            id: id.to_string(),
            properties: properties
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        }
    }

    /// The value of property `name`.
    pub fn property(&self, name: &str) -> Option<&str> {
        self.properties
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }
}

impl PartialEq for BlockState {
    fn eq(&self, other: &BlockState) -> bool {
        palette::signature(&self.id, &js::object_order(self.properties.clone()))
            == palette::signature(&other.id, &js::object_order(other.properties.clone()))
    }
}

impl Eq for BlockState {}

/// The engine's blueprint: the bytes `McbenchEngine.write` and
/// `mcbench_core::session::Session::place` produce, which the renderer, the
/// live view and the store read. The format is the engine's and does not
/// change here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Blueprint(pub Vec<u8>);

/// The live import's command text the engine places (`setblock` and `fill`
/// lines, `server/live-placement.js`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlaceCommands(pub String);

/// A chat's world as the store keeps it: the JSON of `world_manifest_json`
/// (chunk keys to chunk file hashes, palette, counts), byte for byte as
/// `JSON.stringify(world.serializeManifest(...))` writes it, so today's
/// data folders open unchanged.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorldManifest(pub String);

/// Blocks added, removed and replaced between a world and the clone made
/// from it (`server/world.js` `diff`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Diff {
    /// Cells that were air and hold a block.
    pub added: u64,
    /// Cells that held a block and are air.
    pub removed: u64,
    /// Cells whose block changed.
    pub changed: u64,
}

/// Reads a stored chunk file by its hash.
pub type ReadChunk<'a> = dyn FnMut(&str) -> Result<Vec<u8>, String> + 'a;

/// Writes a chunk file under its hash (the first 24 hex digits of the
/// sha256 of its bytes), when no file of that hash is stored yet.
pub type WriteChunk<'a> = dyn FnMut(&str, &[u8]) -> Result<(), String> + 'a;
