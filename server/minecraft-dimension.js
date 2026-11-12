'use strict';

const fs = require('fs');
const path = require('path');

const MIN_MC_Y = -64;
const WORLD_HEIGHT = 1024;
const MAX_MC_Y = MIN_MC_Y + WORLD_HEIGHT - 1;
const Y_OFFSET = Number(process.env.MCBENCH_MC_Y_OFFSET || 64);
// The ticks Minecraft runs on a placed build before it is read back, unless
// the model asks for another number (build's settle_ticks, see_build's tick):
// 210 is 7 steps of overworld lava, which moves a step (a block down or a
// block across) every 30 ticks (FlowingFluid's tick delay), so lava runs its
// 7 steps before the build is read. Water (5 ticks a step), falling blocks
// and scheduled ticks are done sooner.
const DEFAULT_SETTLE_TICKS = 210;

function validateWorldHeight(world, yOffset = Y_OFFSET) {
  const bounds = world.bounds();
  if (!bounds) return;
  const min = bounds.min[1] + yOffset;
  const max = bounds.max[1] + yOffset;
  if (min < MIN_MC_Y || max > MAX_MC_Y) {
    throw new Error(`This build reaches Minecraft y=${min}..${max}; Minecraft here supports y=${MIN_MC_Y}..${MAX_MC_Y}. Keep build y within ${MIN_MC_Y - yOffset}..${MAX_MC_Y - yOffset}.`);
  }
}

function writeDimensionPack(paperDir) {
  const pack = path.join(paperDir, 'world', 'datapacks', 'mcbench');
  const types = path.join(pack, 'data', 'minecraft', 'dimension_type');
  fs.mkdirSync(types, { recursive: true });
  fs.writeFileSync(path.join(pack, 'pack.mcmeta'), JSON.stringify({
    pack: { description: 'mcbench build height', min_format: [88, 0], max_format: [88, 0] },
  }, null, 2));
  // Minecraft 1.21.10's bundled overworld dimension, with expanded height.
  fs.writeFileSync(path.join(types, 'overworld.json'), JSON.stringify({
    ambient_light: 0, bed_works: true, cloud_height: 192,
    coordinate_scale: 1, effects: 'minecraft:overworld', has_ceiling: false,
    has_raids: true, has_skylight: true, height: WORLD_HEIGHT,
    infiniburn: '#minecraft:infiniburn_overworld', logical_height: WORLD_HEIGHT,
    min_y: MIN_MC_Y, monster_spawn_block_light_limit: 0,
    monster_spawn_light_level: { type: 'minecraft:uniform', max_inclusive: 7, min_inclusive: 0 },
    natural: true, piglin_safe: false, respawn_anchor_works: false, ultrawarm: false,
  }, null, 2));
}

module.exports = { MIN_MC_Y, MAX_MC_Y, WORLD_HEIGHT, Y_OFFSET, DEFAULT_SETTLE_TICKS, validateWorldHeight, writeDimensionPack };
