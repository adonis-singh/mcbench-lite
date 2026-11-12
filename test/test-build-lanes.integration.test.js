'use strict';

// test_build runs the tests of one call on the verifier pool's lanes side by
// side. A test is a level of its own, so its report must be the same whether
// it runs alone on one lane or beside others on several. This runs the same
// tests both ways on real engines (the Rust engine first, the Java engine for
// what Rust refuses) and compares the reports.
//
//   MCBENCH_ENGINE_INTEGRATION=1 MCBENCH_DATA_DIR=<scratch copy> node --test test/test-build-lanes.integration.test.js

const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { test } = require('node:test');
const { VerifierPool } = require('../server/minecraft-verify');
const { World } = require('../server/world');

// A lever into dust into a lamp, an observer ring that fires for good, and a
// note block a sculk sensor hears, beside a sculk shrieker (which the Rust
// engine refuses to let hear the input player: a test that uses the note block
// runs on the Java engine).
function circuit() {
  const world = new World();
  world.fill([0, 0, 0], [20, 0, 12], 'minecraft:stone');
  world.set(0, 1, 0, 'minecraft:lever', { face: 'floor', facing: 'north', powered: 'false' });
  for (let x = 1; x <= 4; x++) world.set(x, 1, 0, 'minecraft:redstone_wire', { north: 'none', south: 'none', east: 'side', west: 'side', power: '0' });
  world.set(5, 1, 0, 'minecraft:redstone_lamp', { lit: 'false' });
  world.set(10, 1, 4, 'minecraft:observer', { facing: 'east', powered: 'false' });
  world.set(11, 1, 4, 'minecraft:observer', { facing: 'south', powered: 'false' });
  world.set(11, 1, 5, 'minecraft:observer', { facing: 'west', powered: 'false' });
  world.set(10, 1, 5, 'minecraft:observer', { facing: 'north', powered: 'false' });
  world.set(15, 1, 8, 'minecraft:note_block', { instrument: 'harp', note: '0', powered: 'false' });
  world.set(16, 1, 8, 'minecraft:sculk_sensor', { power: '0', sculk_sensor_phase: 'inactive', waterlogged: 'false' });
  world.set(17, 1, 8, 'minecraft:sculk_shrieker', { can_summon: 'false', shrieking: 'false', waterlogged: 'false' });
  world.finalize();
  return world;
}

const lamp = [[0, 1, 0], [3, 1, 0], [5, 1, 0]];
const tests = [
  { probes: lamp, actions: [{ tick: 3, type: 'toggle', at: [0, 1, 0] }], samples: Array.from({ length: 21 }, (_, i) => i * 5) },
  { probes: [[10, 1, 4], [11, 1, 4], [11, 1, 5], [10, 1, 5]], samples: Array.from({ length: 100 }, (_, i) => i * 2) },
  { probes: [...lamp, [15, 1, 8]], actions: [{ tick: 2, type: 'use', at: [15, 1, 8] }, { tick: 8, type: 'toggle', at: [0, 1, 0] }], samples: [0, 10, 20, 40] },
  { probes: lamp, actions: [{ tick: 9, type: 'toggle', at: [0, 1, 0] }, { tick: 30, type: 'toggle', at: [0, 1, 0] }], samples: [0, 9, 10, 30, 31, 60], settle_ticks: 7 },
  { probes: lamp, actions: [{ tick: 1, type: 'toggle', at: [0, 1, 0] }], samples: [0, 5], item_entities: true, living_entities: true, world_inventories: true },
  { probes: [[10, 1, 4]], samples: [0, 300], settle_ticks: 100 },
  { probes: [[3, 1, 0]], actions: [{ tick: 2, type: 'drop', at: [3.5, 3, 0.5], item: 'minecraft:redstone', count: 5, velocity: [0, 0, 0] }], samples: [0, 10, 40], item_entities: true },
  { probes: [[3, 1, 0], [15, 1, 8]], actions: [{ tick: 1, type: 'use', at: [15, 1, 8] }, { tick: 2, type: 'drop', at: [3.5, 3, 0.5], item: 'minecraft:redstone', count: 5, velocity: [0, 0, 0] }], samples: [0, 10, 40], item_entities: true },
];

// What a result holds that depends on the run, not the experiment: timings
// (and so the size of a report that holds them), and the UUIDs the Java engine
// draws at random for the entities it makes (the Rust engine's are fixed by
// the level, and compared as they are). Those are numbered by first appearance.
function comparable(result) {
  const { wallMs, timings, reportFile, metrics, reportBytes, ...rest } = result;
  const { metrics: reportMetrics, ...report } = JSON.parse(fs.readFileSync(reportFile, 'utf8'));
  const compared = { ...rest, report };
  if (!report.engine.startsWith('McbenchLevel')) return compared;
  const numbers = new Map();
  return JSON.parse(JSON.stringify(compared).replace(/[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}/g, (uuid) => `uuid-${numbers.has(uuid) ? numbers.get(uuid) : numbers.set(uuid, numbers.size).size - 1}`));
}

test('tests run side by side on the pool\'s lanes report what they report one at a time', { skip: !process.env.MCBENCH_ENGINE_INTEGRATION, timeout: 600_000 }, async () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-lanes-'));
  const pool = new VerifierPool({ root, memory: async () => 16 * 2 ** 30 });
  try {
    await pool.warm();
    assert.equal(pool.lanes.length, 3);
    const world = circuit();
    const alone = [];
    for (const one of tests) alone.push(comparable(await pool.run(world, one, {})));
    const again = [];
    for (const one of tests) again.push(comparable(await pool.run(world, one, {})));
    const runs = pool.lanes.map((lane) => {
      let count = 0;
      const run = lane.run.bind(lane);
      lane.run = (...args) => { count += 1; return run(...args); };
      return () => count;
    });
    const together = (await Promise.all(tests.map((one) => pool.run(world, one, {})))).map(comparable);
    assert.deepEqual(runs.map((count) => count()).sort(), [2, 3, 3], 'the tests spread over the lanes');
    assert.deepEqual(together.map((r) => r.report.engine.split(' ')[0]), ['mcbench-core', 'mcbench-core', 'McbenchLevel', 'mcbench-core', 'mcbench-core', 'mcbench-core', 'mcbench-core', 'McbenchLevel'], 'the tests whose shrieker hears the input player ran on the Java engine, the rest on Rust');
    assert.ok(together[6].report.inputs[0].entityUUID, 'a dropped item has a UUID, the same on every run');
    for (let i = 0; i < tests.length; i++) {
      // A test that makes entities may differ run to run (unseeded entity ids and UUIDs); anything else must not.
      assert.deepEqual(again[i], alone[i], `test ${i} twice alone`);
      assert.deepEqual(together[i], alone[i], `test ${i} beside the others`);
    }
  } finally {
    await pool.stop();
    fs.rmSync(root, { recursive: true, force: true });
  }
});
