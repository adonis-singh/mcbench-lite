#!/usr/bin/env node
'use strict';

const assert = require('node:assert/strict');
const { performance } = require('node:perf_hooks');
const { World, MAX_BLOCKS } = require('../server/world');
const { runProgram } = require('../server/build-sandbox');

const registry = { stone: [{}, {}], oak_planks: [{}, {}] };
const samples = {};
async function measure(name, fn) {
  const start = performance.now();
  const result = await fn();
  (samples[name] ||= []).push(performance.now() - start);
  return result;
}

(async () => {
  for (let pass = 0; pass < 5; pass++) {
    const world = new World();
    world.fill([0, 0, 0], [299, 199, 49], 'minecraft:stone');
    world.finalize();
    const candidate = world.clone();
    await measure('replace3mMs', () => runProgram(
      'module.exports = mc => mc.replace("stone", "oak_planks")', candidate, registry,
    ));
    assert.deepEqual(candidate.diff(world), { added: 0, removed: 0, changed: MAX_BLOCKS });
    const outline = new World();
    await measure('outline3mMs', () => runProgram(
      'module.exports = mc => mc.fill([0,0,0], [299,199,49], "stone", "outline")', outline, registry,
    ));
    assert.equal(outline.size(), 4 * (300 + 200 + 50) - 16);
    if (global.gc) global.gc();
  }

  process.stdout.write(`${JSON.stringify(Object.fromEntries(Object.entries(samples).map(([name, values]) => [
    name, { median: values.toSorted((a, b) => a - b)[2], samples: values },
  ])), null, 2)}\n`);
})();
