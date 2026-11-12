'use strict';

const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { after, beforeEach, test } = require('node:test');
const dataDir = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-settle-ticks-'));
process.env.MCBENCH_DATA_DIR = dataDir;
process.env.MCBENCH_SETTLE_ENGINES = '2';
const { FakeEngines, installFakeEngines } = require('./helpers/fake-engines');
installFakeEngines();
// The page that draws shots is not under test: it is told which world each shot shows.
const shot = require('../server/shot');
const rendered = [];
shot.renderShots = async (conversationId, shots) => {
  rendered.push(shots.map((one) => ({ tick: one.tick, clip: one.clip })));
  return shots.map(() => Buffer.from('jpeg'));
};
const store = require('../server/store');
const { World } = require('../server/world');
const { BUDGETS } = require('../server/verification-contract');
const { executeBuild, executeSeeBuild } = require('../server/agent')._internal;
const { settle, shotWorld, currentTicks, stopShotEngines } = require('../server/shot-world');

const REGISTRY = { stone: [{}, {}], sand: [{}, {}] };

// Sand falls a block each tick.
FakeEngines.rules = (blocks) => {
  const sand = [...blocks].filter(([, spec]) => spec === 'minecraft:sand').map(([key]) => key.split(',').map(Number)).sort((a, b) => a[1] - b[1]);
  for (const [x, y, z] of sand) {
    if (y > -60 && !blocks.has(`${x},${y - 1},${z}`)) { blocks.delete(`${x},${y},${z}`); blocks.set(`${x},${y - 1},${z}`, 'minecraft:sand'); }
  }
};

const TOWER = 'module.exports = mc => { mc.fill([0, 0, 0], [4, 0, 4], "stone"); mc.fill([2, 12, 2], [2, 14, 2], "sand"); };';
const sandAt = (world) => { const found = []; world.forEach((x, y, z, entry) => { if (entry.id === 'minecraft:sand') found.push(y); }); return found.sort((a, b) => a - b); };

beforeEach(() => { FakeEngines.places.length = 0; FakeEngines.ticks = []; FakeEngines.failures = 0; rendered.length = 0; });
after(async () => { await stopShotEngines(); store.close(); fs.rmSync(dataDir, { recursive: true, force: true }); });

test('a build settles for 210 ticks unless it asks for another number, and says how many', async () => {
  const conversation = store.createConversation('openai', 'fixture');
  const first = await executeBuild(conversation, { program: TOWER }, REGISTRY);
  assert.equal(first.minecraft.ticks, 210);
  assert.deepEqual(FakeEngines.ticks, [210]);
  assert.deepEqual(sandAt(shotWorld(conversation)), [1, 2, 3]);

  const placed = store.createConversation('openai', 'fixture');
  const asPlaced = await executeBuild(placed, { program: TOWER, settle_ticks: 0 }, REGISTRY);
  assert.equal(asPlaced.minecraft.ticks, 0);
  assert.deepEqual(sandAt(shotWorld(placed)), [12, 13, 14]);
  assert.deepEqual([asPlaced.minecraft.removed, asPlaced.minecraft.added], [0, 0]);

  const midway = store.createConversation('openai', 'fixture');
  const some = await executeBuild(midway, { program: TOWER, settle_ticks: 5 }, REGISTRY);
  assert.equal(some.minecraft.ticks, 5);
  assert.deepEqual(sandAt(shotWorld(midway)), [7, 8, 9]);
  assert.deepEqual([some.minecraft.removed, some.minecraft.added], [3, 3]);
});

test('a number of ticks beyond test_build\'s budget, or not a whole number, is refused before anything runs', async () => {
  const conversation = store.createConversation('openai', 'fixture');
  for (const settle_ticks of [BUDGETS.simulationTicks + 1, -1, 2.5, '40']) {
    const result = await executeBuild(conversation, { program: TOWER, settle_ticks }, REGISTRY);
    assert.equal(result.ok, false, String(settle_ticks));
    assert.match(result.error, /settle_ticks/);
    assert.equal(result.note, 'program not run — no blocks or files were changed');
  }
  assert.match((await executeBuild(conversation, { program: TOWER, settle_ticks: BUDGETS.simulationTicks + 1 }, REGISTRY)).error, new RegExp(`the budget of simulated ticks, the same as test_build's, is ${BUDGETS.simulationTicks} \\(MCBENCH_VERIFY_MAX_SIMULATION_TICKS\\)`));
  assert.equal(conversation.world.size(), 0);
  assert.equal(FakeEngines.places.length, 0);
  const edge = await executeBuild(conversation, { program: 'module.exports = mc => mc.set([0, 0, 0], "stone")', settle_ticks: 3 }, REGISTRY);
  assert.equal(edge.ok, true);
});

test('mc.settled reads the settle the world has whatever its ticks, and mc.settledTicks says which', async () => {
  const conversation = store.createConversation('openai', 'fixture');
  const empty = await executeBuild(conversation, { program: 'module.exports = mc => { mc.log(mc.settledTicks); mc.set([0, 0, 0], "stone"); }' }, REGISTRY);
  assert.deepEqual(empty.logs, ['210'], 'a world nothing has settled starts from the default');
  await executeBuild(conversation, { program: TOWER, settle_ticks: 4 }, REGISTRY);
  const read = await executeBuild(conversation, { program: 'module.exports = mc => { mc.log(mc.settledTicks, mc.settled(2, 8, 2)?.id, mc.settled(2, 11, 2)?.id ?? "air"); mc.set([6, 0, 6], "stone"); }', settle_ticks: 60 }, REGISTRY);
  assert.deepEqual(read.logs, ['4 minecraft:sand air'], 'ticks 4: the sand fell from 12 to 8');
  assert.equal(read.minecraft.ticks, 60);
  const next = await executeBuild(conversation, { program: 'module.exports = mc => mc.log(mc.settledTicks, mc.settled(2, 1, 2)?.id, mc.settled(2, 8, 2)?.id ?? "air")' }, REGISTRY);
  assert.deepEqual(next.logs, ['60 minecraft:sand air']);
  assert.equal(currentTicks(conversation.world), 60);
  // A reloaded conversation (its world read back from the store, no settle
  // in memory) keeps the ticks its last build asked.
  store.save(conversation);
  const saved = new Map([...conversation.world.chunks.values()].map((chunk) => [chunk.persistedHash, chunk.pairs()]));
  const manifest = JSON.parse(JSON.stringify(conversation.world.serializeManifest(() => assert.fail('the store saved every chunk'))));
  conversation.world = World.fromManifest(manifest, (hash) => saved.get(hash));
  const reloaded = await executeBuild(conversation, { program: 'module.exports = mc => mc.log(mc.settledTicks)' }, REGISTRY);
  assert.deepEqual(reloaded.logs, ['60']);
});

test('settles are kept by conversation, world, revision and ticks', async () => {
  const conversation = store.createConversation('openai', 'fixture');
  await executeBuild(conversation, { program: TOWER, settle_ticks: 3 }, REGISTRY);
  assert.equal(FakeEngines.places.length, 1);
  const again = await settle(conversation.id, conversation.world, [3]);
  assert.equal(again.get(3).cached, true);
  const more = await settle(conversation.id, conversation.world, [3, 9]);
  assert.deepEqual([more.get(3).cached, more.get(9).cached], [true, false]);
  assert.equal(FakeEngines.places.length, 2, 'only the new ticks were run');
  assert.deepEqual(FakeEngines.ticks, [3, 9]);
  assert.equal((await settle(conversation.id, conversation.world, [9])).get(9).cached, true);
  // Another conversation with the same blocks is its own settle.
  const other = store.createConversation('openai', 'fixture');
  await settle(other.id, conversation.world, [3]);
  assert.equal(FakeEngines.places.length, 3);
  // The same blocks as another world object are settled again; a revision is a world.
  const copy = conversation.world.reference();
  assert.equal((await settle(conversation.id, copy, [3])).get(3).cached, false);
  assert.equal(FakeEngines.places.length, 4);
  assert.equal((await settle(conversation.id, conversation.world, [3])).get(3).cached, true, 'the world before it is still the conversation\'s: its settle is kept');
  assert.equal(FakeEngines.places.length, 4);
  conversation.world.revision += 1;
  assert.equal((await settle(conversation.id, conversation.world, [3])).get(3).cached, false);
  assert.equal(FakeEngines.places.length, 5);
});

test('one run to the largest tick writes the world at every tick asked, as separate runs would', async () => {
  const world = new World();
  world.fill([0, 0, 0], [6, 0, 6], 'minecraft:stone');
  world.fill([3, 20, 3], [3, 24, 3], 'minecraft:sand');
  world.fill([5, 9, 5], [5, 11, 5], 'minecraft:sand');
  world.finalize();
  const ticks = [0, 1, 4, 9, 40];
  const together = await settle('snapshots', world, ticks);
  assert.equal(FakeEngines.places.length, 1);
  assert.deepEqual(FakeEngines.ticks, [1, 3, 5, 31], 'one placement, the ticks between the writes');
  assert.deepEqual(together.get(0).changes, { removed: 0, added: 0, replaced: 0, examples: { removed: [], added: [], replaced: [] } });
  const blocksOf = (result) => { const out = []; result.world.forEach((x, y, z, entry) => out.push(`${x},${y},${z} ${entry.id}`)); return out.sort(); };
  for (const tick of ticks) {
    const alone = (await settle(`alone-${tick}`, world, [tick])).get(tick);
    assert.deepEqual(blocksOf(together.get(tick)), blocksOf(alone), `tick ${tick}`);
    assert.deepEqual(together.get(tick).changes, alone.changes, `tick ${tick}`);
    assert.equal(together.get(tick).ticks, tick);
  }
  assert.deepEqual(sandAt(together.get(4).world), [5, 6, 7, 16, 17, 18, 19, 20]);
  assert.notDeepEqual(blocksOf(together.get(1)), blocksOf(together.get(40)));
});

test('see_build shows several ticks of one run, reports each, and defaults to the last build\'s ticks', async () => {
  const conversation = store.createConversation('openai', 'fixture');
  await executeBuild(conversation, { program: TOWER, settle_ticks: 6 }, REGISTRY);
  FakeEngines.places.length = 0;
  FakeEngines.ticks = [];
  const result = await executeSeeBuild(conversation, { shots: [{ view: 'front' }, { view: 'front', tick: 0 }, { view: 'top', tick: 2 }, { view: 'iso_se', tick: 6 }, { view: 'iso_se', tick: 40 }] });
  assert.equal(result.ok, true, result.error);
  assert.equal(FakeEngines.places.length, 1, 'one run for the ticks not settled yet (0, 2, 40)');
  assert.deepEqual(FakeEngines.ticks, [2, 38]);
  assert.deepEqual(result.shots.map((one) => one.tick), [6, 0, 2, 6, 40]);
  assert.deepEqual(rendered[0].map((one) => one.tick), [6, 0, 2, 6, 40]);
  assert.deepEqual(result.minecraft.map((report) => report.ticks), [0, 2, 6, 40]);
  assert.deepEqual(result.minecraft.map((report) => [report.removed, report.added]), [[0, 0], [2, 2], [3, 3], [3, 3]]);
  const again = await executeSeeBuild(conversation, { shots: [{ view: 'front', tick: 2 }] });
  assert.equal(FakeEngines.places.length, 1);
  assert.deepEqual(again.minecraft.map((report) => report.ticks), [2]);
  const refused = await executeSeeBuild(conversation, { shots: [{ view: 'front' }, { tick: BUDGETS.simulationTicks + 1 }] });
  assert.equal(refused.ok, false);
  assert.match(refused.error, /^shot 2: tick is \d+ ticks; the budget/);
  assert.equal(FakeEngines.places.length, 1);
  assert.equal(shotWorld(conversation, null, 2) !== null, true);
  assert.equal(shotWorld(conversation, null, 3), null);
});

// The page's request names the clip box: a malformed one is refused, and a
// settle keeps the latest few clips instead of one for every box ever asked.
test('a clip box is checked, and a settle keeps a few clips', async () => {
  const conversation = store.createConversation('openai', 'fixture');
  await executeBuild(conversation, { program: TOWER }, REGISTRY);
  for (const clip of [{ from: [0, 0], to: [1, 1, 1] }, { from: [0, 0, 0], to: [1, 1, 5000] }, { from: [0, 0, 0.5], to: [1, 1, 1] }, { to: [1, 1, 1] }]) {
    assert.throws(() => shotWorld(conversation, clip), (error) => error.statusCode === 400 && /clip needs from and to corners/.test(error.message));
  }
  const first = shotWorld(conversation, { from: [0, 0, 0], to: [4, 0, 4] });
  assert.equal(first.size(), 25);
  assert.equal(shotWorld(conversation, { from: [4, 0, 4], to: [0, 0, 0] }), first, 'a box is kept by its corners');
  for (let x = 1; x <= 8; x += 1) shotWorld(conversation, { from: [0, 0, 0], to: [x, 0, 0] });
  assert.notEqual(shotWorld(conversation, { from: [0, 0, 0], to: [4, 0, 4] }), first, 'the oldest clip was let go');
});
