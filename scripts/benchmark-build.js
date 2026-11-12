#!/usr/bin/env node
'use strict';

// End-to-end build tool cost on a large world, the way executeBuild runs it:
// clone, run the program, diff, finalize, size, bounds. A ~2.1M-block terrain gets a
// 36-house village (readFile heightmap, ~80k mc.set calls, fills, layers,
// replace, find), and separately a one-block edit that is dominated by moving
// the world across the process boundary. A pause stands in for the model
// turn between builds, during which the next build process starts.

const assert = require('node:assert/strict');
const { performance } = require('node:perf_hooks');
const { World } = require('../server/world');
const { runProgram } = require('../server/build-sandbox');
const { getBlockRegistry } = require('../server/assets');

const SIZE = 256;
const heights = Array.from({ length: SIZE }, (_, x) => Array.from({ length: SIZE }, (_, z) => 31 + ((x * 7 + z * 13) % 8)));
const heightmap = JSON.stringify(heights);

const VILLAGE = `module.exports = (mc) => {
  const h = JSON.parse(mc.readFile('heightmap.json'));
  for (let x = 0; x < ${SIZE}; x++) for (let z = 0; z < ${SIZE}; z++) mc.set([x, h[x][z], z], 'grass_block[snowy=false]');
  for (let i = 0; i < 6; i++) for (let j = 0; j < 6; j++) {
    mc.translate(20 + i * 36, 40, 20 + j * 36, () => {
      mc.fill([0, 0, 0], [12, 7, 10], 'spruce_planks', 'hollow');
      mc.fill([1, 0, 1], [11, 0, 9], 'oak_planks');
      for (let x = -1; x <= 13; x++) for (let k = 0; k < 6; k++) {
        mc.set([x, 8 + k, k - 1], 'oak_stairs[facing=south,half=bottom]');
        mc.set([x, 8 + k, 11 - k], 'oak_stairs[facing=north,half=bottom]');
      }
      for (let w = 2; w < 11; w += 3) { mc.set([w, 3, 0], 'glass_pane'); mc.set([w, 3, 10], 'glass_pane'); }
      mc.set([6, 1, 0], 'oak_door[facing=north,half=lower]');
      mc.set([6, 2, 0], 'oak_door[facing=north,half=upper]');
      mc.layers({ origin: [2, 1, 2], palette: { c: 'crafting_table', f: 'furnace[facing=south]', b: 'bookshelf' }, layers: [['c.f..b', '......', 'b....b']] });
    });
  }
  mc.replace('stone', 'andesite', { from: [0, 0, 0], to: [127, 30, 127] });
  mc.log('village', mc.count('oak_stairs'), mc.find('oak_door', { from: [0, 0, 0], to: [60, 60, 60] }).length);
};`;
const ONE_BLOCK = 'module.exports = (mc) => mc.set([0, 60, 0], "gold_block")';

async function build(world, program, registry) {
  await new Promise((resolve) => setTimeout(resolve, 100));
  const candidate = world.clone();
  const start = performance.now();
  const { logs } = await runProgram(program, candidate, registry, () => heightmap);
  const diff = candidate.diff(world);
  candidate.finalize();
  candidate.size();
  candidate.bounds();
  return { ms: performance.now() - start, logs, diff, candidate };
}

(async () => {
  const registry = await getBlockRegistry();
  const terrain = new World();
  terrain.fill([0, 0, 0], [SIZE - 1, 30, SIZE - 1], 'minecraft:stone');
  terrain.fill([0, 0, 0], [SIZE - 1, 4, SIZE - 1], 'minecraft:deepslate');
  terrain.finalize();
  const samples = { villageMs: [], oneBlockOnLargeWorldMs: [], emptyWorldMs: [] };
  for (let pass = 0; pass < 5; pass++) {
    const village = await build(terrain, VILLAGE, registry);
    assert.deepEqual(village.logs, ['village 6480 4']);
    assert.equal(village.candidate.size(), 2_123_612);
    samples.villageMs.push(village.ms);
    const edit = await build(village.candidate, ONE_BLOCK, registry);
    assert.deepEqual(edit.diff, { added: 1, removed: 0, changed: 0 });
    samples.oneBlockOnLargeWorldMs.push(edit.ms);
    samples.emptyWorldMs.push((await build(new World(), ONE_BLOCK, registry)).ms);
  }
  process.stdout.write(`${JSON.stringify(Object.fromEntries(Object.entries(samples).map(([name, values]) => [
    name, { median: Math.round(values.toSorted((a, b) => a - b)[2] * 10) / 10, samples: values.map((v) => Math.round(v * 10) / 10) },
  ])), null, 2)}\n`);
})();
