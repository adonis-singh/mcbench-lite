'use strict';

const fs = require('fs');
const path = require('path');

const MIN_MC_Y = -64;
const WORLD_HEIGHT = 1024;
const MAX_MC_Y = MIN_MC_Y + WORLD_HEIGHT - 1;
const Y_OFFSET = Number(process.env.MCBENCH_MC_Y_OFFSET || 64);
// The one biome of the world the app builds in (paper-server.js's void
// world), whose colours tinted blocks take (McbenchClient.java).
const BIOME = 'minecraft:plains';
// The ticks Minecraft runs on a placed build before it is read back, unless
// the model asks for another number (build's settle_ticks, see_build's tick):
// the longest spread of a fluid across flat ground, water's 7 steps (its
// source's amount, 8, less its drop-off, 1, a block across, down to 1), at
// the slowest fluid's tick delay, lava's 30 in the overworld: every fluid
// spreads as far as flat ground lets it, and lava goes 7 steps, a block down
// or across each, before the build is read. Falling blocks and scheduled
// ticks are done sooner. Minecraft's own rules are the engine's table's
// (FlowingFluid.getTickDelay and getDropOff, McbenchEngine.dump's fluidrule
// lines); test/settle-ticks-minecraft.test.js holds the number to them.
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

module.exports = { MIN_MC_Y, MAX_MC_Y, WORLD_HEIGHT, Y_OFFSET, BIOME, DEFAULT_SETTLE_TICKS, validateWorldHeight, writeDimensionPack };
