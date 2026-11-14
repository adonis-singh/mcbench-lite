'use strict';

const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { after, test } = require('node:test');
const dataDir = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-verdict-'));
process.env.MCBENCH_DATA_DIR = dataDir;
const { FakeEngines, installFakeEngines } = require('./helpers/fake-engines');
installFakeEngines();
const store = require('../server/store');
const { executeBuild } = require('../server/agent')._internal;
const { stopShotEngines } = require('../server/shot-world');

const REGISTRY = {
  stone: [{}, {}],
  torch: [{}, {}],
  wall_torch: [{ facing: ['north', 'south', 'east', 'west'] }, { facing: 'north' }],
  red_bed: [{ facing: ['north', 'south', 'east', 'west'], part: ['head', 'foot'], occupied: ['true', 'false'] }, { facing: 'north', part: 'foot', occupied: 'false' }],
};
const WALL = { north: [0, 0, 1], south: [0, 0, -1], east: [-1, 0, 0], west: [1, 0, 0] };

// What the engine's SURVIVE answers here: a torch with nothing under it, and
// a wall torch with nothing behind it, cannot survive.
FakeEngines.unsupported = (blocks) => [...blocks].filter(([key, spec]) => {
  const [x, y, z] = key.split(',').map(Number);
  if (spec === 'minecraft:torch') return !blocks.has(`${x},${y - 1},${z}`);
  const facing = /^minecraft:wall_torch\[facing=(\w+)\]$/.exec(spec)?.[1];
  return facing && !blocks.has(`${x + WALL[facing][0]},${y},${z + WALL[facing][2]}`);
}).map(([key]) => key);

after(async () => { await stopShotEngines(); store.close(); fs.rmSync(dataDir, { recursive: true, force: true }); });

const build = (conversation, program) => executeBuild(conversation, { program: `module.exports = mc => { ${program} }` }, REGISTRY);

test('a block Minecraft says cannot survive where it is refuses the build, saying where a full block would hold it', async () => {
  const conversation = store.createConversation('openai', 'fixture');
  const floor = await build(conversation, `mc.set([0, 1, 0], 'torch'); mc.log('placed');`);
  assert.equal(floor.ok, false);
  assert.equal(floor.error, 'Minecraft cannot keep a block of this build where it is (Block.canSurvive: a player could not place it there, and the first update that asks again removes it): torch at [0, 1, 0]: a full block at [0, 0, 0] would hold it');
  assert.equal(floor.note, 'program aborted — no blocks or files were changed');
  assert.deepEqual(floor.logs, ['placed']);
  assert.equal(conversation.world.size(), 0, 'nothing was applied');
  const held = await build(conversation, `mc.set([0, 0, 0], 'stone'); mc.set([0, 1, 0], 'torch'); mc.set([3, 1, 0], 'stone'); mc.set([3, 1, 1], 'wall_torch[facing=south]');`);
  assert.equal(held.ok, true, held.error);
  assert.equal(conversation.world.size(), 4);
});

test('a build that takes away a block\'s support across a chunk border is refused, and blocks far from what it changed are not asked', async () => {
  const conversation = store.createConversation('openai', 'fixture');
  assert.equal((await build(conversation, `mc.set([15, 0, 0], 'stone'); mc.set([16, 0, 0], 'wall_torch[facing=east]');`)).ok, true);
  const removed = await build(conversation, `mc.replace('stone', 'air');`);
  assert.equal(removed.ok, false);
  assert.match(removed.error, /wall_torch\[facing=east\] at \[16, 0, 0\]: a full block at \[16, -1, 0\] would hold it$/);
  assert.equal(conversation.world.size(), 2);
  // A torch the fake engine finds unsupported, far from a later build, does not refuse it.
  FakeEngines.unsupported = () => ['200,64,200'];
  try {
    assert.equal((await build(conversation, `mc.set([0, 5, 0], 'stone');`)).ok, true);
  } finally {
    FakeEngines.unsupported = () => [];
  }
});

test('a bed needs its other half one block toward its facing, also across a chunk border', async () => {
  const conversation = store.createConversation('openai', 'fixture');
  assert.equal((await build(conversation, `mc.set([0, 0, 0], 'red_bed[facing=south,part=foot]'); mc.set([0, 0, 1], 'red_bed[facing=south,part=head]');`)).ok, true);
  const wrong = await build(conversation, `mc.set([0, 0, -2], 'red_bed[facing=south,part=head]'); mc.set([0, 0, -1], 'red_bed[facing=south,part=foot]');`);
  assert.equal(wrong.ok, false);
  assert.equal(wrong.error, 'Incomplete bed at [0, 0, -2]: red_bed[part=head,facing=south] needs its foot at [0, 0, -3] with the same facing (a bed faces from its foot toward its head)');
  assert.match((await build(conversation, `mc.set([5, 0, 5], 'red_bed[facing=east]');`)).error, /red_bed\[part=foot,facing=east\] needs its head at \[6, 0, 5\]/);
  assert.equal((await build(conversation, `mc.set([15, 0, 4], 'red_bed[facing=east,part=foot]'); mc.set([16, 0, 4], 'red_bed[facing=east,part=head]');`)).ok, true);
});
