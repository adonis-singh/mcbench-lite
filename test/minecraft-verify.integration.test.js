'use strict';

const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { test } = require('node:test');
const { World } = require('../server/world');
const { MinecraftVerifier } = require('../server/minecraft-verify');

// Opt-in: uses a JDK and real Paper. Fixtures are independent of user builds.
test('Paper observes native button release, both toggle transitions, broken wiring, and gravity', {
  skip: process.env.MCBENCH_VERIFY_INTEGRATION !== '1', timeout: 600_000,
}, async () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-verify-integration-'));
  const verifier = new MinecraftVerifier({ root, physicsGuard: process.env.MCBENCH_PHYSICS_GUARD === '1' });
  const world = new World();
  world.fill([-1, 0, -4], [25, 0, 3], 'minecraft:stone');
  for (const x of [0, 8]) {
    world.set(x, 1, 1, 'minecraft:waxed_copper_bulb', { lit: 'false', powered: 'false' });
    world.set(x, 1, 2, 'minecraft:stone_button', { face: 'wall', facing: 'south', powered: 'false' });
    world.set(x, 1, 0, 'minecraft:comparator', { facing: x === 0 ? 'south' : 'north', mode: 'compare', powered: 'false' });
    world.set(x, 1, -1, 'minecraft:redstone_wire');
    world.set(x, 1, -2, 'minecraft:repeater', { facing: 'south', delay: '1', powered: 'false' });
    world.set(x, 1, -3, 'minecraft:redstone_lamp', { lit: 'false' });
  }
  world.set(16, 1, -1, 'minecraft:stone');
  world.set(16, 1, 0, 'minecraft:lever', { face: 'wall', facing: 'south', powered: 'false' });
  world.set(16, 1, -2, 'minecraft:redstone_lamp', { lit: 'false' });
  world.set(24, 4, 0, 'minecraft:sand');
  world.finalize();
  const revision = world.revision;
  const probes = [[0, 1, 2], [0, 1, 1], [0, 1, -3], [8, 1, -3], [16, 1, -2], [24, 4, 0], [24, 1, 0]];
  const progress = [];
  try {
    const request = {
      probes, samples: [0, 10, 25, 40, 50, 70],
      actions: [0, 40].flatMap(tick => [
        { tick, type: 'press', at: [0, 1, 2] }, { tick, type: 'press', at: [8, 1, 2] },
        { tick, type: 'toggle', at: [16, 1, 0] },
      ]),
    };
    const result = await verifier.run(world, request, { onUpdate: event => progress.push(event) });
    assert.equal(result.sprinting, true);
    assert.equal(result.physicsGuard, process.env.MCBENCH_PHYSICS_GUARD === '1' ? 'listener-guard-v1' : 'stock');
    verifier.sprint = false;
    const normal = await verifier.run(world, request);
    verifier.sprint = true;
    for (const field of ['initial', 'observations', 'inputs']) {
      assert.deepEqual(result[field], normal[field], `${field} must match at normal speed and sprint speed`);
    }
    console.log('Native timing comparison (simulation ms):', { sprint: result.timings.simulationMs, normal: normal.timings.simulationMs });
    verifier.reportEncoding = 'rle-v1';
    const denseRequest = { ...request, samples: Array.from({ length: 71 }, (_, i) => i) };
    const packed = await verifier.run(world, denseRequest);
    verifier.reportEncoding = 'full';
    const fullDense = await verifier.run(world, denseRequest);
    const packedReport = JSON.parse(fs.readFileSync(packed.reportFile));
    const fullReport = JSON.parse(fs.readFileSync(fullDense.reportFile));
    verifier.asyncPreparation = false;
    const legacyPreparation = await verifier.run(world, denseRequest);
    verifier.asyncPreparation = true;
    const legacyReport = JSON.parse(fs.readFileSync(legacyPreparation.reportFile));
    for (const field of ['initial', 'observations', 'inputs']) assert.deepEqual(legacyReport[field], fullReport[field], 'legacy preparation remains compatible');
    const expanded = packedReport.observations.flatMap(o => Array.from({ length: o.endTick - o.tick + 1 }, (_, i) => ({ tick: o.tick + i, blocks: o.blocks })));
    assert.deepEqual(expanded, fullReport.observations, 'compressed observations preserve every sampled tick, state and signal');
    assert.deepEqual(packedReport.initial, fullReport.initial);
    assert.deepEqual(packedReport.inputs, fullReport.inputs);
    assert.deepEqual(packedReport.preview, fullReport.preview);
    assert.deepEqual(packedReport.sampleSequence, { start: 0, step: 1, count: 71 });
    assert.ok(packed.reportBytes < fullDense.reportBytes);
    fs.writeFileSync(path.join(os.tmpdir(), 'mcbench-verification-integration.json'), JSON.stringify(result, null, 2));
    assert.equal(result.ok, true);
    assert.match(result.engine, /Paper/);
    assert.equal(result.worldRevision, revision);
    assert.equal(result.elapsedTicks, 70);
    assert.ok(progress.some(p => p.phase === 'running' && Number.isInteger(p.tick) && p.totalTicks === 70));
    assert.equal(progress.at(-1).phase, 'cleanup');
    assert.ok(result.inputs.every(input => input.applied));
    const at = (tick, probe) => result.observations.find(s => s.tick === tick).blocks[probe].block;
    assert.match(at(0, 0), /powered=true/);
    assert.match(at(25, 0), /powered=false/, 'native stone button releases without a scripted block edit');
    assert.match(at(10, 1), /lit=true/);
    assert.match(at(10, 2), /lit=true/);
    assert.match(at(10, 3), /lit=false/, 'the verifier must not repair the reversed comparator');
    assert.match(at(10, 4), /lit=true/);
    assert.equal(result.initial.blocks[5].block, 'minecraft:air');
    assert.equal(result.initial.blocks[6].block, 'minecraft:sand');
    assert.match(at(50, 1), /lit=false/);
    assert.match(at(50, 2), /lit=false/);
    assert.match(at(50, 4), /lit=false/);
    assert.equal(world.revision, revision);
    assert.equal(world.get(24, 4, 0).id, 'minecraft:sand', 'simulation must not change the blueprint');
    assert.equal(result.timings.reusedRuntime, false);
    assert.equal(result.preview.truncated, false);
    assert.ok(result.preview.changes.some(c => c.probe === 2 && c.tick === 10 && /lit=true/.test(c.block)));
    assert.equal(result.preview.changeCount, result.preview.changes.length);
    const runtime = verifier.runtime;
    const firstUid = fs.readFileSync(path.join(runtime.paperDir, 'mcbench_test', 'uid.dat'));
    await runtime.command('forceload add 96 96');
    await runtime.command('setblock 100 70 100 minecraft:water');
    await runtime.command('summon minecraft:armor_stand 100 70 100 {Tags:["old-test"]}');
    // A second call must start from the blueprint, not the prior simulated world.
    const fresh = await verifier.run(world, { probes: [[0, 1, 1], [100, 6, 100]], samples: [0], actions: [{ tick: 0, type: 'press', at: [0, 1, 1] }] });
    assert.match(fresh.initial.blocks[0].block, /lit=false/);
    assert.equal(fresh.initial.blocks[1].block, 'minecraft:air');
    assert.equal(fresh.timings.reusedRuntime, true);
    assert.equal(verifier.runtime, runtime);
    assert.notDeepEqual(fs.readFileSync(path.join(runtime.paperDir, 'mcbench_test', 'uid.dat')), firstUid);
    assert.doesNotMatch(await runtime.command('execute if entity @e[tag=old-test] run say LEAKED'), /LEAKED/);
    assert.equal(fresh.inputs[0].applied, false);
    assert.match(fresh.inputs[0].error, /requires a button/);
    fs.writeFileSync(path.join(os.tmpdir(), 'mcbench-verification-integration.json'), JSON.stringify({ result, fresh }, null, 2));
    const large = await verifier.run(world, {
      probes: Array.from({ length: 512 }, () => [16, 1, -2]), samples: [0, 20],
      actions: [{ tick: 0, type: 'toggle', at: [16, 1, 0] }],
    });
    assert.equal(large.observationsInFile, true);
    assert.equal(large.observations, undefined);
    assert.equal(large.preview.changedProbeCount, 512);
    assert.equal(large.preview.changeCount, 512);
    assert.equal(large.preview.truncated, true);
    assert.ok(large.preview.changes.some(c => /lit=true/.test(c.block)));
    assert.ok(Buffer.byteLength(JSON.stringify(large)) < 20_000);
    const full = JSON.parse(fs.readFileSync(large.reportFile, 'utf8'));
    assert.equal(full.observations[1].blocks.length, 512);
    verifier.budgets = { ...verifier.budgets, reportBytes: 256 };
    await assert.rejects(verifier.run(world, { probes, samples: [0] }), error => {
      assert.equal(error.code, 'VERIFICATION_BUDGET_EXCEEDED');
      assert.equal(error.budget.resource, 'reportBytes');
      assert.equal(error.budget.allowed, 256);
      return true;
    });
    assert.equal(verifier.active, null);
  } finally {
    await verifier.stop();
    fs.rmSync(root, { recursive: true, force: true });
  }
});

test('drain empties one container through native removal, records the removed stacks and matches sprint and normal pacing', {
  skip: process.env.MCBENCH_VERIFY_INTEGRATION !== '1', timeout: 300_000,
}, async () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-drain-integration-'));
  const verifier = new MinecraftVerifier({ root });
  const world = new World();
  world.fill([0, 0, 0], [2, 0, 2], 'minecraft:stone');
  world.set(0, 1, 0, 'minecraft:chest', { facing: 'north', type: 'single', waterlogged: 'false' });
  world.set(0, 1, 1, 'minecraft:comparator', { facing: 'north', mode: 'compare', powered: 'false' });
  world.set(0, 1, 2, 'minecraft:redstone_lamp', { lit: 'false' });
  world.finalize();
  const request = { probes: [[0, 1, 0], [0, 1, 2]], inventory_probes: [[0, 1, 0]], samples: Array.from({ length: 41 }, (_, i) => i), actions: [
    { tick: 5, type: 'insert', at: [0, 1, 0], slot: 0, item: 'minecraft:stone', count: 64 },
    { tick: 5, type: 'insert', at: [0, 1, 0], slot: 3, item: 'minecraft:ender_pearl', count: 16 },
    { tick: 20, type: 'drain', at: [0, 1, 0] }, { tick: 30, type: 'drain', at: [0, 1, 0] }, { tick: 30, type: 'drain', at: [2, 0, 0] },
  ] };
  try {
    let reference;
    for (const sprint of [true, false]) {
      verifier.sprint = sprint;
      const result = await verifier.run(world, request);
      assert.equal(result.sprinting, sprint);
      const full = JSON.parse(fs.readFileSync(result.reportFile, 'utf8'));
      const trace = { initial: full.initial, observations: full.observations, inputs: full.inputs };
      if (reference) assert.deepEqual(trace, reference, 'drain traces must match at normal speed and sprint speed');
      else reference = trace;
    }
    const [,, first, second, stone] = reference.inputs;
    assert.equal(first.applied, true);
    assert.deepEqual(first.removed.map(i => [i.slot, i.id, i.count, i.plain]), [[0, 'minecraft:stone', 64, true], [3, 'minecraft:ender_pearl', 16, true]]);
    assert.ok(first.removed.every(i => typeof i.snbt === 'string'));
    assert.deepEqual(second.removed, [], 'draining an empty container removes nothing');
    assert.equal(stone.applied, false);
    assert.match(stone.error, /not a container/);
    const at = tick => reference.observations.find(o => o.tick === tick).blocks;
    assert.equal(at(10)[0].inventory.length, 2);
    assert.match(at(15)[1].block, /lit=true/);
    assert.deepEqual(at(20)[0].inventory, [], 'the drained container is observed empty in the same tick');
    assert.match(at(30)[1].block, /lit=false/, 'the comparator sees the native removal');
  } finally {
    await verifier.stop();
    fs.rmSync(root, { recursive: true, force: true });
  }
});

test('fresh-world falling-block traces match sprint and normal pacing from the first tick', {
  skip: process.env.MCBENCH_VERIFY_INTEGRATION !== '1', timeout: 300_000,
}, async () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-entity-readiness-'));
  const verifier = new MinecraftVerifier({ root });
  const world = new World();
  world.set(0, 0, 0, 'minecraft:stone');
  world.set(0, 4, 0, 'minecraft:sand');
  world.finalize();
  const request = { probes: [[0, 1, 0], [0, 2, 0], [0, 3, 0], [0, 4, 0]], samples: Array.from({ length: 31 }, (_, i) => i), settle_ticks: 1 };
  let reference;
  try {
    for (const sprint of [true, false, true, false]) {
      verifier.sprint = sprint;
      const result = await verifier.run(world, request);
      const trace = { initial: result.initial, observations: result.observations, inputs: result.inputs };
      if (reference) assert.deepEqual(trace, reference, 'wall-clock chunk readiness must not shift native entity ticks');
      else reference = trace;
      assert.equal(result.observations.at(-1).blocks[0].block, 'minecraft:sand');
    }
  } finally {
    await verifier.stop();
    fs.rmSync(root, { recursive: true, force: true });
  }
});
