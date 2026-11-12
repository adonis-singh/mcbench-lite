'use strict';

// What shot-world.js keeps of a conversation, on fake engines: a conversation
// dropped (more than 16 were settled since) leaves with all of it, and a
// settle of it that ends afterwards puts nothing back.

const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { after, beforeEach, test } = require('node:test');
const dataDir = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-settle-eviction-'));
process.env.MCBENCH_DATA_DIR = dataDir;
// One engine: the settles run in the order they were asked.
process.env.MCBENCH_SETTLE_ENGINES = '1';
const { FakeEngines, installFakeEngines } = require('./helpers/fake-engines');
installFakeEngines();
const { World } = require('../server/world');
const { settle, shotWorld, stopShotEngines } = require('../server/shot-world');

const KEPT_CONVERSATIONS = 16;

beforeEach(() => {
  FakeEngines.delay = 2;
  FakeEngines.failures = 0;
});
after(async () => { await stopShotEngines(); fs.rmSync(dataDir, { recursive: true, force: true }); });

function tower(seed) {
  const world = new World();
  world.fill([0, 0, 0], [3 + seed, 0, 3], 'minecraft:stone');
  return world.finalize();
}

test('a settle that ends after its conversation was dropped leaves no engine world of it behind', async () => {
  const world = tower(0);
  // Asked first, and dropped by the sixteen asked next, before its engine ran.
  const dropped = settle('dropped', world, [0]);
  const others = Array.from({ length: KEPT_CONVERSATIONS }, (_, i) => settle(`other-${i}`, tower(1), [0]));
  const before = (await dropped).get(0);
  const last = (await Promise.all(others)).at(-1).get(0);
  assert.equal(shotWorld({ id: 'dropped', world }, null, 0), null, 'the conversation was dropped');
  // A kept conversation's next settle takes its palette from its latest engine world.
  assert.equal((await settle(`other-${KEPT_CONVERSATIONS - 1}`, tower(2), [0])).get(0).world.palette, last.world.palette);
  const next = (await settle('dropped', tower(2), [0])).get(0);
  assert.notEqual(next.world.palette, before.world.palette, 'the dropped conversation has no latest engine world');
});

test('a settle that fails after its conversation was dropped does not take the place of a kept one', async () => {
  FakeEngines.failures = 1;
  const failing = settle('failing', tower(0), [0]);
  const worlds = Array.from({ length: KEPT_CONVERSATIONS }, (_, i) => tower(i));
  const others = worlds.map((world, i) => settle(`kept-${i}`, world, [0]));
  await assert.rejects(failing, /The engine failed: fixture/);
  await Promise.all(others);
  // Sixteen are kept: settling one again drops none of them.
  assert.equal((await settle('kept-1', worlds[1], [0])).get(0).cached, true);
  assert.equal((await settle('kept-0', worlds[0], [0])).get(0).cached, true, 'the oldest kept conversation still has its settle');
});
