'use strict';

const assert = require('node:assert');
const { test } = require('node:test');

const { LiveViews } = require('../server/live-views');
const { World } = require('../server/world');

function build(...blocks) {
  const world = new World();
  for (const [x, y, z, id] of blocks) world.set(x, y, z, id, {});
  world.finalize();
  return world;
}

// A clock the test moves, so each look sees the build as it is.
function views(resolve) {
  const clock = { now: 0 };
  const live = new LiveViews({ resolve, yOffset: 64, now: () => clock.now });
  return { live, look: () => { clock.now += 1000; } };
}

test('a view has a version that changes only when its blocks do, and its descriptor names the build', () => {
  let world = build([0, 0, 0, 'minecraft:stone']);
  const { live, look } = views(() => ({ world, layout: null }));
  const first = live.describe('chat');
  assert.deepStrictEqual(first.bounds, { min: [0, 0, 0], max: [0, 0, 0] });
  assert.strictEqual(first.blockCount, 1);
  look();
  // The same blocks in a new object (a branch switched to and back): the same version.
  world = build([0, 0, 0, 'minecraft:stone']);
  assert.strictEqual(live.describe('chat').version, first.version);
  look();
  world = build([0, 0, 0, 'minecraft:stone'], [1, 0, 0, 'minecraft:oak_planks']);
  const second = live.describe('chat');
  assert.ok(second.version > first.version);
  assert.strictEqual(second.blockCount, 2);
});

test('the page gets the import that takes the world it holds to the latest, or a fresh one', () => {
  let world = build([0, 0, 0, 'minecraft:stone']);
  const { live, look } = views(() => ({ world, layout: null }));
  const { version } = live.describe('chat');
  // Nothing held: the whole build, placed afresh, as the live import writes it.
  assert.deepStrictEqual(live.imports('chat', null), { version, fresh: true, commands: 'setblock 0 64 0 minecraft:stone\n', blockCount: 1 });
  // Holding the latest: nothing to place.
  assert.deepStrictEqual(live.imports('chat', version), { version, fresh: false, commands: '', blockCount: 1 });
  look();
  world = build([0, 0, 0, 'minecraft:stone'], [1, 0, 0, 'minecraft:oak_planks']);
  // Holding the version before: only the edit, on the world as it is.
  const edit = live.imports('chat', version);
  assert.strictEqual(edit.fresh, false);
  assert.strictEqual(edit.commands, 'setblock 1 64 0 minecraft:oak_planks\n');
  assert.ok(edit.version > version);
  // A version the server never had (or no longer keeps) is placed afresh.
  assert.strictEqual(live.imports('chat', 9999).fresh, true);
  assert.strictEqual(live.imports('chat', 9999).commands.split('\n').length, 3);
});

test('a build too tall to place keeps the version before it, and says why', () => {
  let world = build([0, 0, 0, 'minecraft:stone']);
  const { live, look } = views(() => ({ world, layout: null }));
  const before = live.describe('chat');
  look();
  world = build([0, 0, 0, 'minecraft:stone'], [0, 2000, 0, 'minecraft:stone']);
  const after = live.describe('chat');
  assert.strictEqual(after.version, before.version);
  assert.match(after.syncError, /^Minecraft kept the previous build: This build reaches Minecraft y=64\.\.2064/);
  look();
  world = build([0, 0, 0, 'minecraft:stone']);
  assert.strictEqual(live.describe('chat').syncError, null);
  look();
  world = build([0, 1500, 0, 'minecraft:stone']);
  const fresh = new LiveViews({ resolve: () => ({ world, layout: null }), yOffset: 64 });
  assert.throws(() => fresh.describe('other'), /y=1564/, 'a view that never had a build has nothing to keep');
});

test('a bench\'s rounds share one camera counter, and a chat has its own', () => {
  const { live } = views(() => ({ world: build([0, 0, 0, 'minecraft:stone']), layout: [{ id: 'a' }] }));
  assert.strictEqual(live.describe('bench:b1:0').layout[0].id, 'a');
  assert.strictEqual(live.describe('bench:b1:2').frame, 0);
  live.frame('bench:b1:1');
  live.frame('chat');
  assert.strictEqual(live.describe('bench:b1:2').frame, 1);
  assert.strictEqual(live.describe('bench:b2:0').frame, 0);
  assert.strictEqual(live.describe('chat').frame, 1);
});

test('views not looked at lately are let go, and a missing view is the resolver\'s error', () => {
  const live = new LiveViews({ resolve: (key) => { if (key === 'gone') throw Object.assign(new Error('Conversation not found'), { statusCode: 404 }); return { world: build([0, 0, 0, 'minecraft:stone']), layout: null }; } });
  assert.throws(() => live.describe('gone'), /Conversation not found/);
  for (let i = 0; i < 100; i += 1) live.describe(`chat-${i}`);
  assert.ok(live.views.size <= 64);
});
