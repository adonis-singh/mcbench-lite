'use strict';

const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { after, beforeEach, test } = require('node:test');
const dataDir = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-settle-pool-'));
process.env.MCBENCH_DATA_DIR = dataDir;
const { FakeEngines, installFakeEngines } = require('./helpers/fake-engines');
installFakeEngines();
const { EnginePool, poolSize } = require('../server/engine-pool');
const { World } = require('../server/world');
const { settle, shotWorld, currentTicks, stopShotEngines } = require('../server/shot-world');

FakeEngines.rules = (blocks) => {
  for (const [key, spec] of [...blocks]) {
    const [x, y, z] = key.split(',').map(Number);
    if (spec === 'minecraft:sand' && y > -60 && !blocks.has(`${x},${y - 1},${z}`)) { blocks.delete(key); blocks.set(`${x},${y - 1},${z}`, spec); }
  }
};

beforeEach(() => {
  FakeEngines.places.length = 0;
  FakeEngines.ticks = [];
  FakeEngines.made.length = 0;
  FakeEngines.peak = 0;
  FakeEngines.delay = 0;
  FakeEngines.failures = 0;
  FakeEngines.warmFailures = 0;
  FakeEngines.deaths = 0;
});
after(async () => { await stopShotEngines(); fs.rmSync(dataDir, { recursive: true, force: true }); });

const pause = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

function tower(seed) {
  const world = new World();
  world.fill([0, 0, 0], [3 + seed, 0, 3], 'minecraft:stone');
  world.fill([1, 10 + seed, 1], [1, 12 + seed, 1], 'minecraft:sand');
  return world.finalize();
}

const digest = (result) => { const out = []; result.world.forEach((x, y, z, entry) => out.push(`${x},${y},${z} ${entry.id}`)); return JSON.stringify([out.sort(), result.changes]); };

test('a pool runs jobs of different keys at once on as many members as it has, and a key\'s jobs in order', async () => {
  const made = [];
  const pool = new EnginePool({ size: 3, wallMs: 60_000, make: (slot) => { made.push(slot); return { alive: () => true, stop: async () => {} }; } });
  let running = 0;
  let peak = 0;
  const log = [];
  const job = (name, ms) => async (member) => {
    running += 1; peak = Math.max(peak, running);
    log.push(`${name} start on ${member.slot}`);
    await pause(ms);
    log.push(`${name} end`);
    running -= 1;
  };
  await Promise.all([
    pool.run('a', job('a1', 40)), pool.run('a', job('a2', 5)), pool.run('a', job('a3', 5)),
    pool.run('b', job('b1', 20)), pool.run('c', job('c1', 20)), pool.run('d', job('d1', 5)),
  ]);
  assert.equal(peak, 3);
  assert.deepEqual(made, [0, 1, 2], 'members are made when jobs wait for one, never more than the size');
  const a = log.filter((line) => line.startsWith('a'));
  assert.deepEqual(a, ['a1 start on 0', 'a1 end', 'a2 start on ' + a[2].split(' ').at(-1), 'a2 end', a[4], 'a3 end']);
  assert.ok(log.indexOf('a2 start on ' + a[2].split(' ').at(-1)) > log.indexOf('a1 end'));
  assert.ok(log.indexOf('d1 end') > -1);
  await pool.stop();
});

test('a pool with one job at a time makes one member, and a dropped member is replaced for the jobs waiting', async () => {
  const made = [];
  const stopped = [];
  const pool = new EnginePool({ size: 1, wallMs: 60_000, make: (slot) => { made.push(slot); return { alive: () => true, stop: async () => { stopped.push(slot); } }; } });
  let first = true;
  const failing = pool.run('a', async (member) => { if (first) { first = false; pool.drop(member); throw new Error('did not start'); } return member.slot; });
  const waiting = pool.run('b', async (member) => member.slot);
  await assert.rejects(failing, /did not start/);
  assert.equal(await waiting, 0, 'the slot is reused');
  assert.deepEqual(made, [0, 0]);
  assert.deepEqual(stopped, [0]);
  assert.equal(await pool.run('c', async (member) => member.slot), 0);
  assert.deepEqual(made, [0, 0]);
  await pool.run('x', async (member) => assert.equal(member, null), { member: false });
  await pool.stop();
});

// A settle with no end in sight used to hold its engine until the server
// stopped; eight of them held the whole pool.
test('a job past the wall-clock budget fails, and its member is stopped and replaced', async () => {
  const made = [];
  const stopped = [];
  const pool = new EnginePool({ size: 1, wallMs: 50, make: (slot) => { made.push(slot); return { alive: () => true, stop: async () => { stopped.push(slot); } }; } });
  const stuck = pool.run('a', () => new Promise(() => {}));
  const next = pool.run('b', async (member) => member.slot);
  await assert.rejects(stuck, /ran past its 0.05-second wall-clock budget \(MCBENCH_VERIFY_MAX_WALL_MS\)/);
  assert.equal(await next, 0);
  assert.deepEqual(made, [0, 0]);
  assert.ok(stopped.includes(0));
  await pool.stop();
});

test('a stopped pool fails the jobs waiting for a member and makes none for a job that ends after it', async () => {
  const made = [];
  const pool = new EnginePool({ size: 1, wallMs: 60_000, make: (slot) => { made.push(slot); return { alive: () => true, stop: async () => {} }; } });
  let finish;
  const running = pool.run('a', () => new Promise((resolve) => { finish = resolve; }));
  const waiting = pool.run('b', async (member) => member.slot);
  while (!finish) await pause(1);
  await pool.stop();
  await assert.rejects(waiting, /The engine pool was stopped/);
  finish('done');
  assert.equal(await running, 'done');
  await assert.rejects(pool.run('c', async () => {}), /The engine pool was stopped/);
  assert.deepEqual(made, [0]);
  assert.equal(pool.members.size + pool.free.length, 0);
});

test('a slot is not given a new engine until the engine dropped from it has exited', async () => {
  const made = [];
  let exited;
  const pool = new EnginePool({ size: 1, wallMs: 30, make: (slot) => {
    made.push(slot);
    // The first engine takes its time to exit after SIGTERM.
    return { alive: () => true, stop: made.length === 1 ? () => new Promise((resolve) => { exited = resolve; }) : async () => {} };
  } });
  const stuck = pool.run('a', () => new Promise(() => {}));
  const next = pool.run('b', async (member) => member.slot);
  await assert.rejects(stuck, /wall-clock budget/);
  await pause(20);
  assert.deepEqual(made, [0], 'nothing new while the old engine runs');
  exited();
  assert.equal(await next, 0);
  assert.deepEqual(made, [0, 0]);
  await pool.stop();
});

test('a free member whose engine died is replaced, not handed out', async () => {
  const members = [];
  const pool = new EnginePool({ size: 1, wallMs: 60_000, make: (slot) => { const member = { alive: () => !member.died, stop: async () => {} }; members.push(member); return member; } });
  const first = await pool.run('a', async (member) => member);
  first.died = true;
  const second = await pool.run('b', async (member) => member);
  assert.notEqual(second, first);
  assert.equal(members.length, 2);
  await pool.stop();
});

// An engine that died used to stay in the pool: every later settle on it
// failed until the server restarted.
test('an engine that stopped under a settle is replaced; one that refused a request is kept', async () => {
  await stopShotEngines();
  process.env.MCBENCH_SETTLE_ENGINES = '1';
  FakeEngines.failures = 1;
  await assert.rejects(settle('refused', tower(0), [0]), /The engine failed: fixture/);
  assert.equal((await settle('refused', tower(1), [0])).get(0).world.size() > 0, true);
  assert.equal(FakeEngines.made.length, 1);
  FakeEngines.deaths = 1;
  await assert.rejects(settle('died', tower(2), [0]), /The engine stopped/);
  assert.equal((await settle('died', tower(3), [0])).get(0).world.size() > 0, true);
  assert.equal(FakeEngines.made.length, 2);
});

test('the pool is sized from the cores and memory, or by MCBENCH_SETTLE_ENGINES', () => {
  const size = poolSize({});
  assert.ok(Number.isInteger(size) && size >= 1 && size <= 8 && size <= Math.max(1, os.availableParallelism() - 1));
  assert.equal(poolSize({ MCBENCH_SETTLE_ENGINES: '3' }), 3);
  assert.throws(() => poolSize({ MCBENCH_SETTLE_ENGINES: '0' }), /MCBENCH_SETTLE_ENGINES must be an integer of at least 1/);
});

test('settles of different conversations run at once, each in an engine pair of its own, and give the results one queue gives', async () => {
  const worlds = [0, 1, 2, 3, 4].map(tower);
  const run = async (engines) => {
    await stopShotEngines();
    process.env.MCBENCH_SETTLE_ENGINES = String(engines);
    FakeEngines.made.length = 0;
    FakeEngines.peak = 0;
    FakeEngines.delay = 15;
    const results = await Promise.all(worlds.map((world, i) => settle(`pool-${engines}-${i}`, world, [0, 3, 40])));
    return { results, peak: FakeEngines.peak, made: FakeEngines.made.length, folders: new Set(FakeEngines.made.map((made) => made.options.folder)).size };
  };
  const queued = await run(1);
  const pooled = await run(3);
  assert.deepEqual([queued.peak, queued.made], [1, 1]);
  assert.deepEqual([pooled.peak, pooled.made, pooled.folders], [3, 3, 3]);
  for (let i = 0; i < worlds.length; i += 1) for (const tick of [0, 3, 40]) assert.equal(digest(pooled.results[i].get(tick)), digest(queued.results[i].get(tick)), `world ${i}, tick ${tick}`);
  assert.deepEqual(new Set(pooled.results.map((result) => result.get(40).world.palette)).size, 5, 'palette and unchanged chunks stay per conversation');
});

test('one conversation\'s settles keep their order, and its state is not shared with the others\'', async () => {
  await stopShotEngines();
  process.env.MCBENCH_SETTLE_ENGINES = '3';
  FakeEngines.delay = 10;
  const world = tower(0);
  const second = tower(1);
  const neighbour = tower(2);
  const asked = await Promise.all([settle('ordered', world, [5]), settle('ordered', second, [5]), settle('ordered', world, [9]), settle('neighbour', neighbour, [5])]);
  assert.equal(FakeEngines.peak, 2, 'the three of one conversation ran one at a time, beside the other conversation');
  // The stone floor of tower(seed) reaches x = 3 + seed.
  const reach = FakeEngines.places.map((commands) => Number(/fill 0 \d+ 0 (\d+) /.exec(commands)[1]));
  assert.deepEqual(reach.filter((x) => x !== 5), [3, 4, 3]);
  assert.equal(asked[0].get(5).cached, false);
  assert.equal((await settle('ordered', world, [9])).get(9).cached, true, 'the last world asked is the one kept');
});

test('an engine that does not start is dropped, and the next settle starts another', async () => {
  await stopShotEngines();
  process.env.MCBENCH_SETTLE_ENGINES = '1';
  FakeEngines.warmFailures = 1;
  const world = tower(2);
  await assert.rejects(settle('warming', world, [3]), /The engine stopped/);
  const retried = await settle('warming', world, [3]);
  assert.equal(retried.get(3).cached, false);
  assert.equal(FakeEngines.made.length, 2);
});

test('the engine world shown is the conversation\'s current world, not a candidate settled after it', async () => {
  await stopShotEngines();
  const world = tower(0);
  const candidate = tower(1);
  const shown = (await settle('shown', world, [5], { primary: 5 })).get(5).world;
  // A build's candidate, settled and then not applied.
  await settle('shown', candidate, [7], { primary: 7 });
  assert.equal(currentTicks(world), 5);
  assert.equal(shotWorld({ id: 'shown', world }), shown);
  assert.equal(shotWorld({ id: 'shown', world }, null, 7), null);
  // A tick that fails keeps the world's settles that worked.
  FakeEngines.failures = 1;
  await assert.rejects(settle('shown', world, [9], { primary: 5 }), /The engine failed: fixture/);
  assert.equal((await settle('shown', world, [5], { primary: 5 })).get(5).cached, true);
  assert.equal(shotWorld({ id: 'shown', world }), shown);
});

test('a free pool member whose engine died is replaced at the next settle', async () => {
  await stopShotEngines();
  process.env.MCBENCH_SETTLE_ENGINES = '1';
  const world = tower(4);
  await settle('restart', world, [0]);
  const [engine] = FakeEngines.made;
  engine.child = null;
  FakeEngines.made.length = 0;
  assert.equal((await settle('restart-2', tower(5), [0])).get(0).world.size() > 0, true);
  // The dead member was let go at take time and a new one made in its slot.
  assert.equal(FakeEngines.made.length, 1);
});

// A warm-up the server's shutdown overtakes: its engine goes back to the
// stopped pool, not to a new one, and nothing starts it again.
test('a warm-up stopped part-way leaves no engine running and no pool behind', async () => {
  const { warmShotEngines } = require('../server/shot-world');
  await stopShotEngines();
  process.env.MCBENCH_SETTLE_ENGINES = '1';
  const warming = warmShotEngines();
  const [engine] = FakeEngines.made;
  let warms = 0;
  const warm = engine.warm.bind(engine);
  engine.warm = (options) => { warms += 1; return warm(options); };
  await stopShotEngines();
  await warming;
  assert.equal(warms, 0, 'the stopped engine was not started again');
  await settle('after-stop', tower(6), [0]);
  assert.equal(FakeEngines.made.length, 2, 'the next settle runs on a new pool of its own');
});
