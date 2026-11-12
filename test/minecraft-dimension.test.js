'use strict';

const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { World } = require('../server/world');
const { runProgram } = require('../server/mc-dsl');
const { renderMask } = require('../server/bluemap');
const { MIN_MC_Y, MAX_MC_Y, WORLD_HEIGHT, writeDimensionPack } = require('../server/minecraft-dimension');

test('the expanded dimension and the viewer include the 388-block-tall monitor', () => {
  const world = new World();
  runProgram('module.exports = mc => mc.fill([0,0,0], [0,387,0], "stone")', world, { stone: [{}, {}] });
  assert.equal(world.size(), 388);
  // The render mask has no height limit, so the whole column is drawn.
  const mask = renderMask(world.bounds());
  assert.equal('minY' in mask || 'maxY' in mask, false);
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-dimension-'));
  try {
    writeDimensionPack(root);
    const pack = path.join(root, 'world/datapacks/mcbench');
    const type = JSON.parse(fs.readFileSync(path.join(pack, 'data/minecraft/dimension_type/overworld.json')));
    assert.equal(type.height, WORLD_HEIGHT);
    assert.equal(type.min_y, MIN_MC_Y);
    assert.equal(type.min_y + type.height - 1, MAX_MC_Y);
    assert.ok(type.min_y + type.height > world.bounds().max[1] + 64);
    assert.deepEqual(JSON.parse(fs.readFileSync(path.join(pack, 'pack.mcmeta'))).pack.min_format, [88, 0]);
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
});

test('out-of-range edits fail before the candidate world can be committed', () => {
  const original = new World();
  original.set(0, 0, 0, 'minecraft:stone');
  original.finalize();
  const candidate = original.clone();
  assert.throws(() => runProgram('module.exports = mc => { mc.set([1,1,1], "stone"); mc.set([0,896,0], "stone"); }', candidate, { stone: [{}, {}] }), /Keep build y within -128\.\.895/);
  assert.equal(original.size(), 1);
  assert.deepEqual(original.bounds(), { min: [0, 0, 0], max: [0, 0, 0] });
  assert.throws(() => runProgram('module.exports = mc => mc.set([0,-129,0], "stone")', original.clone(), { stone: [{}, {}] }), /y=-65\.\.64/);
});
