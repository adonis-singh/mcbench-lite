'use strict';

const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { after, beforeEach, test } = require('node:test');
const dataDir = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-settle-'));
process.env.MCBENCH_DATA_DIR = dataDir;
const { FakeEngines, installFakeEngines } = require('./helpers/fake-engines');
installFakeEngines();
const store = require('../server/store');
const { World } = require('../server/world');
const { executeBuild } = require('../server/agent')._internal;
const { settle, shotWorld, stopShotEngines } = require('../server/shot-world');

const REGISTRY = { stone: [{}, {}], torch: [{}, {}], sand: [{}, {}], oak_planks: [{}, {}], water: [{}, {}] };
const YO = require('../server/minecraft-dimension').Y_OFFSET;

// Minecraft here removes a torch standing on planks, drops sand, and turns water to stone.
FakeEngines.rules = (blocks) => {
  for (const [key, spec] of [...blocks]) {
    const [x, y, z] = key.split(',').map(Number);
    if (spec === 'minecraft:torch' && blocks.get(`${x},${y - 1},${z}`) === 'minecraft:oak_planks') blocks.delete(key);
    if (spec.startsWith('minecraft:water')) blocks.set(key, 'minecraft:stone');
    if (spec === 'minecraft:sand') {
      let at = y;
      while (!blocks.has(`${x},${at - 1},${z}`) && at > -60) at -= 1;
      if (at !== y) { blocks.delete(key); blocks.set(`${x},${at},${z}`, spec); }
    }
  }
};

beforeEach(() => { FakeEngines.places.length = 0; FakeEngines.ticks = []; FakeEngines.failures = 0; });
after(async () => { await stopShotEngines(); store.close(); fs.rmSync(dataDir, { recursive: true, force: true }); });

test('a build that changes the world carries Minecraft\'s verdict, and mc.settled reads it back', async () => {
  const conversation = store.createConversation('openai', 'fixture');
  const first = await executeBuild(conversation, { program: `module.exports = mc => {
    mc.set([0, 0, 0], 'stone');
    mc.set([1, 0, 1], 'oak_planks');
    mc.set([1, 1, 1], 'torch');
    mc.set([0, 8, 0], 'sand');
    mc.set([3, 0, 3], 'water');
    mc.log(mc.settled(0, 0, 0));
  }` }, REGISTRY);
  assert.equal(first.ok, true, first.error);
  assert.deepEqual(first.logs, ['null'], 'the world was empty when the program started');
  assert.equal(first.minecraft.ticks, 210);
  assert.deepEqual([first.minecraft.removed, first.minecraft.added, first.minecraft.replaced], [2, 1, 1]);
  assert.deepEqual(first.minecraft.examples.removed, [{ at: [1, 1, 1], was: 'minecraft:torch' }, { at: [0, 8, 0], was: 'minecraft:sand' }]);
  assert.deepEqual(first.minecraft.examples.added, [{ at: [0, 1, 0], now: 'minecraft:sand' }]);
  assert.deepEqual(first.minecraft.examples.replaced, [{ at: [3, 0, 3], was: 'minecraft:water', now: 'minecraft:stone' }]);
  assert.equal(first.minecraft.blocks, 4);

  const second = await executeBuild(conversation, { program: `module.exports = mc => {
    mc.log(mc.settled(0, 1, 0), mc.settled(0, 8, 0), mc.settled(1, 1, 1), mc.get([0, 8, 0]));
    mc.translate(0, 1, 0, () => mc.log(mc.settled(0, 0, 0)));
    mc.set([0, 1, 0], 'stone');
    mc.log(mc.settled(0, 1, 0), mc.settled(9, 0, 9));
  }` }, REGISTRY);
  assert.equal(second.ok, true, second.error);
  assert.deepEqual(second.logs, ['{"id":"minecraft:sand","states":{}} null null {"id":"minecraft:sand","states":{}}', '{"id":"minecraft:sand","states":{}}', '{"id":"minecraft:sand","states":{}} null']);
  // The verdict is on the whole blueprint: the stone under the sand's column stops it higher.
  assert.deepEqual([second.minecraft.removed, second.minecraft.added, second.minecraft.replaced], [2, 1, 1]);
  assert.deepEqual(second.minecraft.examples.added, [{ at: [0, 2, 0], now: 'minecraft:sand' }]);
});

test('a world is settled once: builds start from the last settle, an unchanged world is not settled again', async () => {
  const conversation = store.createConversation('openai', 'fixture');
  const base = await executeBuild(conversation, { program: 'module.exports = mc => mc.set([0, 0, 0], "stone")' }, REGISTRY);
  assert.equal(FakeEngines.places.length, 1, 'the new world, once; the empty world before it needs no engine');
  assert.equal(base.minecraft.removed, 0);
  const read = await executeBuild(conversation, { program: 'module.exports = mc => mc.log(mc.settled(0, 0, 0).id)' }, REGISTRY);
  assert.equal(read.changed, false);
  assert.equal('minecraft' in read, false);
  assert.deepEqual(read.logs, ['minecraft:stone']);
  assert.equal(FakeEngines.places.length, 1);
  const settled = (await settle(conversation.id, conversation.world, [210])).get(210);
  assert.equal(settled.cached, true);
  assert.equal(FakeEngines.places.length, 1);
  assert.equal(shotWorld(conversation).get(0, 0, 0).id, 'minecraft:stone');
  // A world the conversation did not settle (a revert, a reload) is settled when a build starts from it.
  conversation.world = conversation.world.reference();
  await executeBuild(conversation, { program: 'module.exports = mc => mc.log(mc.settled(0, 0, 0).id)' }, REGISTRY);
  assert.equal(FakeEngines.places.length, 2);
});

test('a fresh placement of the whole blueprint is what is settled', async () => {
  const conversation = store.createConversation('openai', 'fixture');
  await executeBuild(conversation, { program: 'module.exports = mc => { mc.fill([0, 0, 0], [3, 0, 3], "stone"); mc.set([1, 1, 1], "torch"); }' }, REGISTRY);
  await executeBuild(conversation, { program: 'module.exports = mc => mc.set([2, 1, 2], "torch")' }, REGISTRY);
  assert.equal(FakeEngines.places.length, 2);
  const lines = FakeEngines.places[1].split('\n').filter(Boolean);
  assert.ok(lines.includes(`fill 0 ${YO} 0 3 ${YO} 3 minecraft:stone`));
  assert.ok(lines.includes(`setblock 1 ${YO + 1} 1 minecraft:torch`));
  assert.ok(lines.includes(`setblock 2 ${YO + 1} 2 minecraft:torch`));
});

test('a build is applied only with its verdict: when Minecraft fails, nothing changes', async () => {
  const conversation = store.createConversation('openai', 'fixture');
  await executeBuild(conversation, { program: 'module.exports = mc => mc.set([0, 0, 0], "stone")' }, REGISTRY);
  const before = conversation.world;
  FakeEngines.failures = 1;
  const failed = await executeBuild(conversation, { program: 'module.exports = mc => { mc.set([1, 0, 0], "stone"); mc.writeFile("note.txt", "x"); }' }, REGISTRY);
  assert.equal(failed.ok, false);
  assert.match(failed.error, /Minecraft could not settle this build: The engine failed: fixture/);
  assert.equal(conversation.world, before);
  assert.throws(() => require('../server/sandbox').makeReadFile(conversation.id)('note.txt'));
  const retried = await executeBuild(conversation, { program: 'module.exports = mc => mc.set([1, 0, 0], "stone")' }, REGISTRY);
  assert.equal(retried.ok, true);
});

test('mc.settled takes three integers and rejects anything else', async () => {
  const conversation = store.createConversation('openai', 'fixture');
  await executeBuild(conversation, { program: 'module.exports = mc => mc.set([0, 0, 0], "stone")' }, REGISTRY);
  for (const call of ['mc.settled([0, 0, 0])', 'mc.settled(0, 0)', 'mc.settled(0.5, 0, 0)', 'mc.settled(0, 0, 99999)']) {
    const result = await executeBuild(conversation, { program: `module.exports = mc => { ${call}; }` }, REGISTRY);
    assert.equal(result.ok, false, call);
  }
  const tuple = await executeBuild(conversation, { program: 'module.exports = mc => mc.settled([0, 0, 0])' }, REGISTRY);
  assert.match(tuple.error, /mc\.settled takes three integers, mc\.settled\(x, y, z\), not/);
});

test('two requests for one world share one engine run, and only the latest conversations stay settled', async () => {
  const world = new World();
  world.set(0, 0, 0, 'minecraft:stone');
  world.finalize();
  const one = async (id) => (await settle(id, world, [100])).get(100);
  const [a, b] = await Promise.all([one('shared'), one('shared')]);
  assert.equal(FakeEngines.places.length, 1);
  assert.equal(a.world, b.world);
  assert.deepEqual([a.cached, b.cached], [false, true]);
  for (let i = 0; i < 16; i += 1) await one(`other-${i}`);
  assert.equal(FakeEngines.places.length, 17);
  assert.equal((await one('other-15')).cached, true);
  assert.equal(shotWorld({ id: 'shared', world }), null, 'the oldest conversation was let go');
  assert.equal((await one('shared')).cached, false);
  assert.equal(FakeEngines.places.length, 18);
});

test('the changed block ids are those a cell-by-cell comparison finds, in the same order', () => {
  const { idChanges } = require('../server/shot-world')._internal;
  let state = 99;
  const random = () => { state = (Math.imul(state, 1664525) + 1013904223) >>> 0; return state / 2 ** 32; };
  const names = ['stone', 'dirt', 'sand', 'water', 'torch', 'glass'];
  const reference = (authored, settled) => {
    const out = { removed: 0, added: 0, replaced: 0, examples: { removed: [], added: [], replaced: [] } };
    const at = new Set();
    authored.forEach((x, y, z) => at.add(`${x},${y},${z}`));
    settled.forEach((x, y, z) => at.add(`${x},${y},${z}`));
    const cells = [...at].map((key) => key.split(',').map(Number));
    // The order of the real thing: chunks as the world lists them, then cell index.
    const chunkOf = (cell) => World.keyOf(cell[0] >> 4, cell[1] >> 4, cell[2] >> 4);
    const index = (cell) => ((cell[1] & 15) << 8) | ((cell[2] & 15) << 4) | (cell[0] & 15);
    const order = [...new Set([...authored.chunks.keys(), ...settled.chunks.keys()])];
    cells.sort((a, b) => order.indexOf(chunkOf(a)) - order.indexOf(chunkOf(b)) || index(a) - index(b));
    for (const cell of cells) {
      const was = authored.get(...cell)?.id ?? null;
      const now = settled.get(...cell)?.id ?? null;
      if (was === now) continue;
      const kind = !now ? 'removed' : !was ? 'added' : 'replaced';
      out[kind] += 1;
      if (out.examples[kind].length < 8) out.examples[kind].push(kind === 'removed' ? { at: cell, was } : kind === 'added' ? { at: cell, now } : { at: cell, was, now });
    }
    return out;
  };
  for (let round = 0; round < 5; round += 1) {
    // Two worlds with palettes interned in different orders.
    const authored = new World();
    const settled = new World();
    for (const name of round % 2 ? names : [...names].reverse()) settled.palette.intern(`minecraft:${name}`, {});
    for (let i = 0; i < 4000; i += 1) {
      const cell = [Math.floor(random() * 40) - 8, Math.floor(random() * 20), Math.floor(random() * 40)];
      const roll = random();
      if (roll < 0.5) authored.set(...cell, `minecraft:${names[Math.floor(random() * names.length)]}`);
      if (roll > 0.35) settled.set(...cell, `minecraft:${names[Math.floor(random() * names.length)]}`);
    }
    assert.deepEqual(idChanges(authored, settled), reference(authored, settled));
  }
});
