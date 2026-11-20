//! The world's height in Minecraft (`server/minecraft-dimension.js`): the
//! expanded overworld the app builds in, the browser-to-Minecraft y offset,
//! the default settle ticks and the data pack that makes the dimension.

use std::path::Path;

use crate::World;

/// The lowest y Minecraft has here (`MIN_MC_Y`).
pub const MIN_MC_Y: i32 = -64;
/// The dimension's height (`WORLD_HEIGHT`).
pub const WORLD_HEIGHT: i32 = 1024;
/// The highest y Minecraft has here (`MAX_MC_Y`).
pub const MAX_MC_Y: i32 = MIN_MC_Y + WORLD_HEIGHT - 1;
/// The one biome of the world the app builds in (`BIOME`).
pub const BIOME: &str = "minecraft:plains";
/// The ticks Minecraft runs on a placed build before it is read back, unless
/// the model asks for another number (`DEFAULT_SETTLE_TICKS`; its reasoning
/// is in today's file).
pub const DEFAULT_SETTLE_TICKS: u32 = 210;

/// Browser y + this is Minecraft y (`Y_OFFSET`): `MCBENCH_MC_Y_OFFSET`, 64
/// when unset, as today's server reads it.
pub fn y_offset() -> i32 {
    match std::env::var("MCBENCH_MC_Y_OFFSET") {
        Ok(text) if !text.is_empty() => text
            .trim()
            .parse()
            .unwrap_or_else(|_| panic!("MCBENCH_MC_Y_OFFSET is {text:?}, not an integer")),
        _ => 64,
    }
}

/// Refuses a world Minecraft cannot hold at `y_offset`
/// (`validateWorldHeight`), with today's message.
pub fn validate_world_height(world: &World, y_offset: i32) -> Result<(), String> {
    let Some(bounds) = world.bounds() else {
        return Ok(());
    };
    let min = bounds.min[1] + y_offset;
    let max = bounds.max[1] + y_offset;
    if min < MIN_MC_Y || max > MAX_MC_Y {
        return Err(format!(
            "This build reaches Minecraft y={min}..{max}; Minecraft here supports y={MIN_MC_Y}..{MAX_MC_Y}. Keep build y within {}..{}.",
            MIN_MC_Y - y_offset,
            MAX_MC_Y - y_offset
        ));
    }
    Ok(())
}

/// The data pack's files, by path inside the save's folder, with the text
/// `JSON.stringify(..., null, 2)` writes (`writeDimensionPack`).
pub fn dimension_pack() -> Vec<(&'static str, String)> {
    let mcmeta = serde_json::json!({
        "pack": { "description": "mcbench build height", "min_format": [88, 0], "max_format": [88, 0] },
    });
    let overworld = serde_json::json!({
        "ambient_light": 0, "bed_works": true, "cloud_height": 192,
        "coordinate_scale": 1, "effects": "minecraft:overworld", "has_ceiling": false,
        "has_raids": true, "has_skylight": true, "height": WORLD_HEIGHT,
        "infiniburn": "#minecraft:infiniburn_overworld", "logical_height": WORLD_HEIGHT,
        "min_y": MIN_MC_Y, "monster_spawn_block_light_limit": 0,
        "monster_spawn_light_level": { "type": "minecraft:uniform", "max_inclusive": 7, "min_inclusive": 0 },
        "natural": true, "piglin_safe": false, "respawn_anchor_works": false, "ultrawarm": false,
    });
    vec![
        (
            "datapacks/mcbench/pack.mcmeta",
            serde_json::to_string_pretty(&mcmeta).expect("JSON"),
        ),
        (
            "datapacks/mcbench/data/minecraft/dimension_type/overworld.json",
            serde_json::to_string_pretty(&overworld).expect("JSON"),
        ),
    ]
}

/// Writes the data pack under `<paper_dir>/world/` (`writeDimensionPack`).
pub fn write_dimension_pack(paper_dir: &Path) -> Result<(), String> {
    for (file, text) in dimension_pack() {
        let target = paper_dir.join("world").join(file);
        let folder = target.parent().expect("a file in a folder");
        std::fs::create_dir_all(folder)
            .map_err(|e| format!("Cannot make {}: {e}", folder.display()))?;
        std::fs::write(&target, text)
            .map_err(|e| format!("Cannot write {}: {e}", target.display()))?;
    }
    Ok(())
}
