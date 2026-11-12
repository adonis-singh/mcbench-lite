'use strict';

const test = require('node:test');
const assert = require('node:assert');
const fs = require('fs');
const os = require('os');
const path = require('path');
const { EngineVerifier, WallBudgetError } = require('../server/minecraft-verify');
const { BUDGETS } = require('../server/verification-contract');
const { World } = require('../server/world');

function world() {
  const w = new World();
  w.set(0, 0, 0, 'minecraft:stone');
  w.finalize();
  return w;
}

// An engine that answers each experiment as McbenchExperiment writes its
// files, with `answer` deciding the result.
function fakeEngine(answer) {
  const engine = {
    warmed: [], verified: [], stopped: 0,
    async warm(options) { engine.warmed.push(options); },
    async verify(requestFile, commandsFile, folder, options) {
      const request = JSON.parse(fs.readFileSync(requestFile, 'utf8'));
      engine.verified.push({ request, commands: fs.readFileSync(commandsFile, 'utf8') });
      await answer(request, folder, engine, options);
    },
    async stop() { engine.stopped += 1; },
  };
  return engine;
}

// The files a completed experiment leaves.
function complete(request, folder) {
  const samples = request.sampleSequence ? request.sampleSequence.count : request.samples.length;
  const report = JSON.stringify({ initial: {}, observations: [] });
  fs.writeFileSync(path.join(folder, 'progress.json'), JSON.stringify({ phase: 'running', tick: request.duration, totalTicks: request.duration }));
  fs.writeFileSync(path.join(folder, 'report.json'), report);
  return {
    ok: true, elapsedTicks: request.duration, sampleCount: samples, probeCount: request.probes.length,
    inputCount: request.actions.length, observationCount: request.probes.length * (samples + 1), reportBytes: Buffer.byteLength(report),
  };
}

function verifier(engine, budgets) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-engine-verify-'));
  return new EngineVerifier({ root, engine, ...(budgets ? { budgets } : {}) });
}

test('the engine runs a test and its report comes back archived', async () => {
  const engine = fakeEngine((request, folder) => {
    fs.writeFileSync(path.join(folder, 'result.json'), JSON.stringify(complete(request, folder)));
  });
  const v = verifier(engine);
  const updates = [];
  const result = await v.run(world(), { probes: [[0, 0, 0]] }, { onUpdate: (update) => updates.push(update.phase) });
  assert.equal(result.ok, true);
  assert.ok(fs.existsSync(result.reportFile));
  assert.equal(engine.warmed[0].folder, path.join(v.root, 'engine'));
  assert.match(engine.verified[0].commands, /^setblock 0 64 0 minecraft:stone/);
  assert.equal(engine.verified[0].request.placementMode, 'updates');
  assert.ok(updates.includes('running'));
  // The run's own files go once it is done.
  assert.deepEqual(fs.readdirSync(path.join(v.root, 'runs')), []);
  await v.stop();
  assert.equal(engine.stopped, 1);
});

test('a failed experiment, incomplete observations and an engine fault are errors', async () => {
  const failed = verifier(fakeEngine((request, folder) => {
    fs.writeFileSync(path.join(folder, 'result.json'), JSON.stringify({ ok: false, error: 'java.io.IOException: No block entity at [0,0,0]' }));
  }));
  await assert.rejects(failed.run(world(), { probes: [[0, 0, 0]] }), /Minecraft verification failed: .*No block entity/);
  const incomplete = verifier(fakeEngine((request, folder) => {
    fs.writeFileSync(path.join(folder, 'result.json'), JSON.stringify({ ...complete(request, folder), sampleCount: 0 }));
  }));
  await assert.rejects(incomplete.run(world(), { probes: [[0, 0, 0]] }), /incomplete observations/);
  const fault = verifier(fakeEngine(async () => { throw new Error('The engine failed: java.lang.NullPointerException'); }));
  await assert.rejects(fault.run(world(), { probes: [[0, 0, 0]] }), /NullPointerException/);
});

test('cancelling a running test stops its engine', async () => {
  const controller = new AbortController();
  const engine = fakeEngine(async (request, folder, self) => {
    controller.abort();
    while (!self.stopped) await new Promise((resolve) => setTimeout(resolve, 1));
    throw new Error('The engine stopped (SIGTERM)');
  });
  const v = verifier(engine);
  await assert.rejects(v.run(world(), { probes: [[0, 0, 0]] }, { signal: controller.signal }), /abort|stopped/i);
  assert.equal(engine.stopped, 1);
});

test('a test that outlasts the wall budget fails plainly, with the ticks it reached', async () => {
  const engine = fakeEngine(async (request, folder, self) => {
    fs.writeFileSync(path.join(folder, 'progress.json'), JSON.stringify({ phase: 'running', tick: 3200, totalTicks: 9000 }));
    while (!self.stopped) await new Promise((resolve) => setTimeout(resolve, 1));
    throw new Error('The engine stopped (SIGTERM)');
  });
  const v = verifier(engine, { ...BUDGETS, wallMs: 400 });
  await assert.rejects(v.run(world(), { probes: [[0, 0, 0]] }), (error) => {
    assert.ok(error instanceof WallBudgetError);
    assert.equal(error.code, 'VERIFICATION_BUDGET_EXCEEDED');
    assert.match(error.message, /too dense to finish within its 0-second wall-clock budget .*reached tick 3200 of 9000 of its timeline.*shorten it.*split it into several tests/i);
    assert.equal(error.budget.resource, 'wallMs');
    assert.equal(error.budget.phase, 'running');
    assert.equal(error.budget.ticksReached, 3200);
    assert.equal(error.budget.totalTicks, 9000);
    return true;
  });
});

test('a test that reaches what the engine does not model fails with the reason, and nothing runs it again', async () => {
  const engine = fakeEngine(async () => { throw new Error('The engine failed: unported: LiquidBlock.getDrops (minecraft:water[level=1])'); });
  await assert.rejects(verifier(engine).run(world(), { probes: [[0, 0, 0]] }), /Minecraft verification failed: unported: LiquidBlock\.getDrops/);
  assert.equal(engine.verified.length, 1);
});

test('a test cancelled while its engine is still being built returns at once, and nothing spawns', async () => {
  const { Engine } = require('../server/engine');
  let spawned = 0;
  // A build that never ends on its own, as a long cargo build.
  const engine = new Engine({ build: () => new Promise(() => {}), spawnEngine: () => { spawned += 1; throw new Error('spawned'); } });
  const v = verifier(engine);
  const controller = new AbortController();
  const running = v.run(world(), { probes: [[0, 0, 0]] }, { signal: controller.signal });
  await new Promise((resolve) => setTimeout(resolve, 20));
  const cancelled = Date.now();
  controller.abort(new Error('Cancelled by user'));
  await assert.rejects(running);
  assert.ok(Date.now() - cancelled < 1000);
  assert.equal(spawned, 0);
});

test('run folders a crash left are swept before the next test, and ones in use are kept', async () => {
  const engine = fakeEngine((request, folder) => {
    fs.writeFileSync(path.join(folder, 'result.json'), JSON.stringify(complete(request, folder)));
  });
  const v = verifier(engine);
  const runs = path.join(v.root, 'runs');
  fs.mkdirSync(path.join(runs, 'crashed'), { recursive: true });
  fs.mkdirSync(path.join(runs, 'running'), { recursive: true });
  const long = new Date(Date.now() - 3 * v.budgets.wallMs);
  fs.utimesSync(path.join(runs, 'crashed'), long, long);
  await v.run(world(), { probes: [[0, 0, 0]] });
  assert.deepEqual(fs.readdirSync(runs), ['running']);
});

test('a verifier pool whose memory reading failed reads it again for the next test', async () => {
  const { VerifierPool } = require('../server/minecraft-verify');
  let reads = 0;
  const lane = (root) => ({ root, pending: 0, run: async () => root, stop: async () => {} });
  const pool = new VerifierPool({ root: '/verify', memory: async () => { reads += 1; if (reads === 1) throw new Error('vm_stat failed'); return 16 * 2 ** 30; }, makeLane: lane });
  await assert.rejects(pool.run(), /vm_stat failed/);
  assert.equal(await pool.run(), '/verify');
  assert.equal(reads, 2);
});
