//! The block registry a build program is checked against
//! (`server/assets.js` getBlockRegistry): by block name without its
//! namespace, its properties with their values and its default state's
//! values. Its text reaches models (`server/mc-dsl.js`: an unknown block's
//! closest matches in its order, a state's valid names and values in theirs),
//! so it is today's file byte for byte: the extractor runs Minecraft's data
//! generator (`net.minecraft.data.Main --reports`) and writes its blocks
//! report as misode/mcmeta's summary does (`mcbench-data`'s `registry`),
//! which gives the sha256 `server/assets.js` pins for mcmeta's
//! `blocks.json`; a test holds it. The engine's table holds the same blocks,
//! properties, values and defaults, but orders every block's properties by
//! name, where the report lists `type` first in 12 blocks, so the registry
//! is not made from the table.

use crate::Data;

/// The registry's JSON text from the shipped data.
pub fn block_registry(data: &Data) -> Result<String, String> {
    data.text("blocks.json")
}
