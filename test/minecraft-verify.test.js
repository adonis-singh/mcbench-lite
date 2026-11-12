'use strict';

const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { test } = require('node:test');
const { World } = require('../server/world');
const { MC_VERSION } = require('../server/assets');
const { BUDGETS, readBudgets } = require('../server/verification-contract');
const { MinecraftVerifier, VerifierPool, callTests, validateRequest, VerificationBudgetError, compilePlugin } = require('../server/minecraft-verify');

function world() {
  const value = new World(); value.set(-1, 0, 0, 'minecraft:stone'); value.finalize(); return value;
}

test('sample normalization preserves caller input and handles ordered and unordered duplicates', () => {
  for (const input of [[5, 5, 5], [0, 1, 1, 2, 9], [9, 2, 0, 9, 1, 2]]) {
    const original = [...input];
    const result = validateRequest(world(), { probes: [[0, 0, 0]], samples: input });
    assert.deepEqual(result.samples, [...new Set(input)].sort((a, b) => a - b));
    assert.deepEqual(input, original);
  }
});

test('observation density and input counts trade against one work budget, with no old per-field ceilings', () => {
  const probes = Array.from({ length: 4096 }, (_, i) => [i % 64, 0, Math.floor(i / 64)]);
  const samples = Array.from({ length: 129 }, (_, i) => i * 10);
  const actions = Array.from({ length: 64 }, (_, i) => ({ tick: i, type: 'toggle', at: [0, 0, 0] }));
  const result = validateRequest(world(), { probes, samples, actions, settle_ticks: 300 });
  assert.equal(result.cost.operations, 4096 * 130 + 64);
  assert.equal(result.cost.simulationTicks, 1580);
  for (const [probes, samples] of [[100, 9], [2, 499]]) {
    const request = { probes: Array.from({ length: probes }, () => [0, 0, 0]), samples: Array.from({ length: samples }, (_, i) => i) };
    const budget = { ...BUDGETS, operations: 1000 };
    assert.equal(validateRequest(world(), request, 64, budget).cost.operations, 1000);
    assert.throws(() => validateRequest(world(), { ...request, actions: [{ tick: 0, type: 'press', at: [0, 0, 0] }] }, 64, budget), error => {
      assert.ok(error instanceof VerificationBudgetError);
      assert.deepEqual(error.budget, { resource: 'operations', requested: 1001, allowed: 1000, setting: 'MCBENCH_VERIFY_MAX_OPERATIONS' });
      return true;
    });
  }
});

test('operator configuration is validated, and time/request budgets are explicit', () => {
  assert.equal(readBudgets({ MCBENCH_VERIFY_MAX_OPERATIONS: '2000000' }).operations, 2_000_000);
  for (const value of ['no', '', '-1', '1.5', 'Infinity']) assert.throws(() => readBudgets({ MCBENCH_VERIFY_MAX_WALL_MS: value }), /MAX_WALL_MS/);
  assert.throws(() => validateRequest(world(), { probes: [[0, 0, 0]], samples: [BUDGETS.simulationTicks - 3999], settle_ticks: 4000 }), error => error.budget?.resource === 'simulationTicks');
  assert.throws(() => validateRequest(world(), { probes: [[0, 0, 0]] }, 64, { ...BUDGETS, requestBytes: 1 }), error => error.budget?.resource === 'requestBytes');
  assert.equal(readBudgets({ MCBENCH_VERIFY_MAX_TESTS: '3' }).tests, 3);
  assert.throws(() => readBudgets({ MCBENCH_VERIFY_MAX_TESTS: '0' }), /MAX_TESTS/);
});

test('a call holds a list of tests within the tests budget', () => {
  const tests = [{ probes: [[0, 0, 0]] }, { probes: [[1, 0, 0]] }];
  assert.equal(callTests({ tests }), tests);
  assert.equal(callTests({ tests }, { ...BUDGETS, tests: 2 }), tests);
  assert.throws(() => callTests({ tests }, { ...BUDGETS, tests: 1 }), error => {
    assert.deepEqual(error.budget, { resource: 'tests', requested: 2, allowed: 1, setting: 'MCBENCH_VERIFY_MAX_TESTS' });
    return true;
  });
  for (const args of [undefined, [], {}, { tests: [] }, { tests: 'x' }, { probes: [[0, 0, 0]] }, { tests, samples: [0] }]) assert.throws(() => callTests(args), /requires \{tests/);
});

test('verification validates untrusted input, tick limits, height, and simulation coverage before boot', () => {
  const build = world();
  const normalized = validateRequest(build, { probes: [[-1, 0, 0]], samples: [20, 0, 20] });
  assert.deepEqual(normalized.samples, [0, 20]);
  assert.equal(normalized.yOffset, 64);
  assert.ok(normalized.columns.some(([x, z]) => x === -2 && z === -1));
  assert.throws(() => validateRequest(build, { probes: [[0, 0, 0]], command: 'setblock' }), /Unknown/);
  assert.throws(() => validateRequest(build, { probes: [[0, '0\nstop', 0]] }), /coordinate/);
  assert.throws(() => validateRequest(build, { probes: [[0, 896, 0]] }), /coordinate/);
  assert.throws(() => validateRequest(build, { probes: [[0, 0, 0]], samples: [BUDGETS.simulationTicks] }), /simulationTicks/);
  assert.throws(() => validateRequest(build, { probes: [[0, 0, 0]], samples: [0], actions: [{ tick: 1, type: 'press', at: [0, 0, 0] }] }), /action tick/);
  assert.throws(() => validateRequest(build, { probes: [[0, 0, 0]], actions: [{ tick: 0, type: 'setblock', at: [0, 0, 0] }] }), /Malformed input action/);
  assert.throws(() => validateRequest(build, { probes: [[0, 0, 0]], settle_ticks: 0 }), /settle_ticks/);
  // A use may say the yaw the player faces (a fence gate needs it); no other input takes one.
  assert.deepEqual(validateRequest(build, { probes: [[0, 0, 0]], actions: [{ tick: 0, type: 'use', at: [0, 0, 0], yaw: -44.5 }, { tick: 0, type: 'use', at: [0, 0, 0] }] }).actions,
    [{ tick: 0, type: 'use', at: [0, 0, 0], yaw: -44.5 }, { tick: 0, type: 'use', at: [0, 0, 0] }]);
  assert.throws(() => validateRequest(build, { probes: [[0, 0, 0]], actions: [{ tick: 0, type: 'use', at: [0, 0, 0], yaw: '90' }] }), /yaw must be a finite number/);
  assert.throws(() => validateRequest(build, { probes: [[0, 0, 0]], actions: [{ tick: 0, type: 'toggle', at: [0, 0, 0], yaw: 90 }] }), /Malformed input action/);
  const scattered = new World();
  for (let x = -4096; x <= 4096; x += 48) for (let z = -4096; z <= 4096; z += 48) scattered.set(x, 0, z, 'minecraft:stone');
  assert.throws(() => validateRequest(scattered, { probes: [[0, 0, 0]] }), /columns/);
});

function harness({ resultTransform = result => result, failImport = false, onImport = async () => {}, budgets = BUDGETS, asyncPreparation, runtimeExit } = {}) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-verify-unit-'));
  const paperDir = path.join(root, 'paper');
  const runtime = path.join(paperDir, 'versions', MC_VERSION);
  fs.mkdirSync(runtime, { recursive: true });
  fs.writeFileSync(path.join(runtime, `paper-${MC_VERSION}.jar`), 'stub');
  const calls = []; const worlds = []; const requests = [];
  const verifier = new MinecraftVerifier({
    root, budgets, cacheSource: path.join(root, 'absent'), compile: async () => { calls.push('compile'); },
    makeLive: () => ({
      paperDir,
      // The Paper process: running, or already exited in the runtime-exit tests.
      server: runtimeExit ? { exitCode: 1, signalCode: null } : { exitCode: null, signalCode: null },
      alive: true,
      freshWorld(dimension) { this.dimension = dimension; },
      async ensurePaper() { calls.push('paper'); },
      async startServer() { calls.push('start'); },
      async stop() { calls.push('stop'); },
      async applyWorld(value) { worlds.push(value); if (failImport) throw new Error('import failed'); calls.push('import'); await onImport(); },
      async command(command, check, timeoutMs) {
        assert.equal(check, true);
        assert.equal(timeoutMs, budgets.wallMs, 'native preparation and run use the experiment wall budget');
        if (command.startsWith('mcbenchverify prepare')) {
          // Each experiment's request arrives in a file of its own.
          const name = command.split(' ')[2];
          assert.match(name, /^request-[0-9a-f-]{36}\.json$/);
          requests.push(JSON.parse(fs.readFileSync(path.join(paperDir, 'plugins', 'McbenchVerify', name), 'utf8')));
          command = 'mcbenchverify prepare';
        }
        calls.push(command);
        if (command === 'mcbenchverify prepare') {
          if (runtimeExit === 'preparing') return 'MCBENCH_PREPARING';
          if (asyncPreparation) {
            const folder = path.join(paperDir, 'plugins', 'McbenchVerify');
            fs.writeFileSync(path.join(folder, 'progress.json'), JSON.stringify({ phase: 'loading', tick: 3, totalTicks: 9 }));
            setTimeout(() => fs.writeFileSync(path.join(folder, 'prepared.json'), JSON.stringify(asyncPreparation)), 20);
            return 'MCBENCH_PREPARING';
          }
          return 'MCBENCH_READY minecraft:mcbench_test';
        }
        if (runtimeExit === 'running') return 'MCBENCH_RUNNING';
        const folder = path.join(paperDir, 'plugins', 'McbenchVerify');
        const request = requests.at(-1);
        if (request.sampleSequence) {
          const { start, step, count } = request.sampleSequence;
          request.samples = Array.from({ length: count }, (_, i) => start + i * step);
        }
        const blocks = request.probes.map(() => ({ block: 'minecraft:air', signal: 0 }));
        const report = {
          ok: true, engine: 'Paper fixture', elapsedTicks: request.duration,
          initial: { tick: 0, blocks }, observations: request.samples.map(tick => ({ tick, blocks })),
          inputs: request.actions.map(action => ({ ...action, applied: false, error: 'Missing input' })),
        };
        const text = JSON.stringify(report);
        fs.writeFileSync(path.join(folder, 'report.json'), text);
        const result = resultTransform({ ok: true, engine: report.engine, elapsedTicks: request.duration,
          probeCount: request.probes.length, sampleCount: request.samples.length, inputCount: request.actions.length,
          observationCount: request.probes.length * (request.samples.length + 1), reportBytes: Buffer.byteLength(text) });
        fs.writeFileSync(path.join(folder, 'result.json'), JSON.stringify(result));
        return 'MCBENCH_RUNNING';
      },
    }),
  });
  return { root, verifier, calls, worlds, requests };
}

test('asynchronous chunk preparation reports progress and imports only after successful preparation', async () => {
  for (const prepared of [{ ok: true, dimension: 'minecraft:mcbench_test' }, { ok: false, error: 'chunk loading failed' }]) {
    const h = harness({ asyncPreparation: prepared }); const progress = [];
    try {
      const run = h.verifier.run(world(), { probes: [[0,0,0]], samples: [0] }, { onUpdate: p => progress.push(p) });
      if (prepared.ok) { assert.equal((await run).ok, true); assert.ok(h.calls.includes('import')); }
      else { await assert.rejects(run, /chunk loading failed/); assert.ok(!h.calls.includes('import')); }
      assert.ok(progress.some(p => p.phase === 'loading' && p.loadedColumns === 3));
    } finally { await h.verifier.stop(); fs.rmSync(h.root, { recursive: true, force: true }); }
  }
});

test('queued verification snapshots revisions and returns observed failure without repairing or grading', async () => {
  const h = harness();
  try {
    const build = world(); const revision = build.revision;
    const pending = h.verifier.run(build, { probes: [[-1, 0, 0]], samples: [0], actions: [{ tick: 0, type: 'press', at: [-1, 0, 0] }] });
    build.set(-1, 0, 0, 'minecraft:gold_block'); build.finalize();
    const result = await pending;
    assert.equal(result.worldRevision, revision);
    assert.equal(h.worlds[0].get(-1, 0, 0).id, 'minecraft:stone');
    assert.equal(result.ok, true);
    assert.equal(result.inputs[0].applied, false);
    assert.equal(result.observations[0].blocks[0].block, 'minecraft:air');
    assert.equal('passed' in result, false);
    assert.deepEqual(h.calls, ['paper', 'compile', 'start', 'mcbenchverify prepare', 'import', 'mcbenchverify run']);
  } finally { await h.verifier.stop(); fs.rmSync(h.root, { recursive: true, force: true }); }
});

test('incomplete engine observations and import failures never become successful verification', async () => {
  for (const options of [{ resultTransform: r => ({ ...r, sampleCount: 0 }) }, { failImport: true }]) {
    const h = harness(options);
    try {
      await assert.rejects(h.verifier.run(world(), { probes: [[0, 0, 0]] }), /incomplete observations|import failed/);
      assert.equal(h.calls.at(-1), 'stop');
    } finally { await h.verifier.stop(); fs.rmSync(h.root, { recursive: true, force: true }); }
  }
});

test('cancelled queued tests do not start a server', async () => {
  const h = harness();
  try {
    await assert.rejects(h.verifier.run(world(), { probes: [[0, 0, 0]] }, { signal: AbortSignal.abort() }), /abort/i);
    assert.deepEqual(h.calls, []);
  } finally { await h.verifier.stop(); fs.rmSync(h.root, { recursive: true, force: true }); }
});

test('cancellation during import cleans up without starting the input timeline', async () => {
  const controller = new AbortController();
  const h = harness({ onImport: async () => controller.abort() });
  try {
    await assert.rejects(h.verifier.run(world(), { probes: [[0, 0, 0]] }, { signal: controller.signal }), /abort/i);
    assert.equal(h.calls.includes('mcbenchverify run'), false);
    assert.equal(h.calls.at(-1), 'stop');
    assert.equal(h.verifier.active, null);
  } finally { await h.verifier.stop(); fs.rmSync(h.root, { recursive: true, force: true }); }
});

test('cancelling behind an active test returns promptly without stopping it or bypassing cleanup', { timeout: 2000 }, async () => {
  let release;
  let entered;
  const importing = new Promise(resolve => { entered = resolve; });
  const gate = new Promise(resolve => { release = resolve; });
  const h = harness({ onImport: async () => { entered(); await gate; } });
  const controller = new AbortController();
  const first = h.verifier.run(world(), { probes: [[0, 0, 0]] });
  let timer;
  try {
    await importing;
    const queued = h.verifier.run(world(), { probes: [[0, 0, 0]] }, { signal: controller.signal });
    controller.abort();
    await assert.rejects(Promise.race([queued, new Promise((_, reject) => {
      timer = setTimeout(() => reject(new Error('Queue cancellation did not return promptly')), 500);
    })]), /abort/i);
    assert.equal(h.calls.filter(c => c === 'start').length, 1);
    assert.equal(h.calls.includes('stop'), false, 'a queued cancellation must not stop another test');
    const next = h.verifier.run(world(), { probes: [[0, 0, 0]] });
    release();
    await first;
    await next;
    assert.deepEqual(h.calls, ['paper', 'compile', 'start', ...Array(2).fill(['mcbenchverify prepare', 'import', 'mcbenchverify run']).flat()]);
  } finally {
    clearTimeout(timer);
    release();
    await first;
    await h.verifier.stop();
    fs.rmSync(h.root, { recursive: true, force: true });
  }
});

test('progress follows the execution lifecycle and shutdown rejects future calls', async () => {
  const h = harness(); const events = [];
  try {
    await h.verifier.run(world(), { probes: [[0, 0, 0]] }, { onUpdate: event => events.push(event) });
    assert.equal(events[0].phase, 'queued');
    assert.ok(events.some(e => e.phase === 'placing' && /revision/.test(e.message)));
    assert.ok(events.some(e => e.phase === 'running'));
    assert.equal(events.at(-1).phase, 'cleanup');
    await h.verifier.stop();
    const count = h.calls.length;
    await assert.rejects(h.verifier.run(world(), { probes: [[0, 0, 0]] }), /shutting down/);
    assert.equal(h.calls.length, count);
  } finally { await h.verifier.stop(); fs.rmSync(h.root, { recursive: true, force: true }); }
});

test('runtime and report budget exhaustion are harness errors with cleanup', async () => {
  // A run cut off by its wall budget stops its server; a report over budget
  // came from a finished run, whose server stays ready for the next test.
  for (const options of [
    { budgets: { ...BUDGETS, wallMs: 5 }, onImport: () => new Promise(resolve => setTimeout(resolve, 20)), resource: 'wallMs', kept: false },
    { budgets: { ...BUDGETS, reportBytes: 1 }, resource: 'reportBytes', kept: true },
  ]) {
    const h = harness(options);
    try {
      await assert.rejects(h.verifier.run(world(), { probes: [[0, 0, 0]] }), error => error.budget?.resource === options.resource);
      assert.equal(h.calls.at(-1), options.kept ? 'mcbenchverify run' : 'stop');
      assert.equal(Boolean(h.verifier.runtime), options.kept);
      assert.equal(h.verifier.active, null);
    } finally { await h.verifier.stop(); fs.rmSync(h.root, { recursive: true, force: true }); }
  }
});

test('a test that fails in Minecraft keeps the runtime for the next test', async () => {
  let fail = true;
  const h = harness({ resultTransform: r => (fail ? { ok: false, error: 'Unknown block type' } : r) });
  try {
    await assert.rejects(h.verifier.run(world(), { probes: [[0, 0, 0]] }), /Minecraft verification failed: Unknown block type/);
    fail = false;
    const result = await h.verifier.run(world(), { probes: [[0, 0, 0]] });
    assert.equal(result.timings.reusedRuntime, true);
    assert.equal(h.calls.filter((call) => call === 'start').length, 1);
    assert.equal(h.calls.includes('stop'), false);
  } finally { await h.verifier.stop(); fs.rmSync(h.root, { recursive: true, force: true }); }
});

test('a kept runtime whose server died since is stopped and replaced, not reused', async () => {
  const h = harness();
  try {
    await h.verifier.run(world(), { probes: [[0, 0, 0]] });
    // Paper crashed while it idled.
    h.verifier.runtime.alive = false;
    const result = await h.verifier.run(world(), { probes: [[0, 0, 0]] });
    assert.equal(result.timings.reusedRuntime, false);
    assert.equal(h.calls.filter((call) => call === 'start').length, 2);
    assert.equal(h.calls.filter((call) => call === 'stop').length, 1);
  } finally { await h.verifier.stop(); fs.rmSync(h.root, { recursive: true, force: true }); }
});

test('archived reports survive later tests and large reports are not inlined', async () => {
  const h = harness();
  try {
    const result = await h.verifier.run(world(), { probes: Array.from({ length: 4096 }, (_, i) => [i % 64, 0, Math.floor(i / 64)]), samples: [0, 1] });
    assert.equal(result.observationsInFile, true);
    assert.equal(result.observations, undefined);
    const bytes = fs.readFileSync(result.reportFile);
    await h.verifier.run(world(), { probes: [[0, 0, 0]] });
    assert.deepEqual(fs.readFileSync(result.reportFile), bytes);
    assert.equal(JSON.parse(bytes).observations[1].blocks.length, 4096);
  } finally { await h.verifier.stop(); fs.rmSync(h.root, { recursive: true, force: true }); }
});

test('placement mode is frozen with each queued experiment and invalid modes are rejected', async () => {
  assert.throws(() => new MinecraftVerifier({placementMode:'approximate'}), /Unsupported placement mode/);
  const h=harness();
  try {
    h.verifier.placementMode='strict';
    const run=h.verifier.run(world(),{probes:[[0,0,0]],samples:[0]});
    h.verifier.placementMode='updates';
    await run;
    const request=h.requests.at(-1);
    assert.equal(request.placementMode,'strict');
    await h.verifier.run(world(),{probes:[[0,0,0]],samples:[0]});
    assert.equal(h.requests.at(-1).placementMode,'updates');
  } finally { await h.verifier.stop(); fs.rmSync(h.root,{recursive:true,force:true}); }
});

test('dynamic inventory and dropped-item observation are opt-in and validated',()=>{
 const args={probes:[[0,0,0]],inventory_probes:[[0,0,0]],samples:[0],inventory_missing:'report',item_entities:true};
 const result=validateRequest(world(),args);assert.equal(result.inventory_missing,'report');assert.equal(result.item_entities,true);
 assert.throws(()=>validateRequest(world(),{...args,inventory_missing:'empty'}),/inventory_missing/);
 assert.throws(()=>validateRequest(world(),{...args,item_entities:'yes'}),/item_entities/);
 const baseline=validateRequest(world(),{probes:[[0,0,0]],samples:[0]});assert.equal(baseline.inventory_missing,undefined);assert.equal(baseline.item_entities,undefined);
});

test('entity imports validate fractional coordinates and include their chunk margin',()=>{
 const entity={at:[96.125,2.5,-96.75],data_version:4189,snbt:'{}'};
 const request={probes:[[0,0,0]],entities:[entity]};
 const v=validateRequest(world(),request);
 assert.deepEqual(v.entities,[entity]);assert.ok(v.columns.some(([x,z])=>x===7&&z===-8));
 for(const at of [[NaN,0,0],[0,Infinity,0],[0,896,0],[4096,0,0],['1',0,0],[0,0]])assert.throws(()=>validateRequest(world(),{...request,entities:[{...entity,at}]}));
 assert.throws(()=>validateRequest(world(),{...request,entities:[{...entity,command:'summon'}]}),/entities require/);
});

test('native drop supply validates all fields and keeps fractional inlet coordinates',()=>{
 const action={tick:0,type:'drop',at:[6.5,2,2.12],velocity:[-1,0,0],item:'minecraft:iron_ingot',count:1};
 assert.deepEqual(validateRequest(world(),{probes:[[0,0,0]],actions:[action]}).actions,[action]);
 for(const bad of [{velocity:[0,0,NaN]},{velocity:[11,0,0]},{count:0},{slot:0},{at:[1,2]},{item:'stone'}])assert.throws(()=>validateRequest(world(),{probes:[[0,0,0]],actions:[{...action,...bad}]}));
 assert.throws(()=>validateRequest(world(),{probes:[[0,0,0]],actions:[{tick:0,type:'toggle',at:[.5,0,0]}]}),/coordinate/);
});

test('page inputs are explicit zero-based native lectern selections',()=>{
 const action={tick:0,type:'page',at:[0,0,0],page:14};assert.deepEqual(validateRequest(world(),{probes:[[0,0,0]],actions:[action]}).actions,[action]);
 for(const bad of [{page:-1},{page:1.5},{slot:1},{page:2147483647},{at:[.5,0,0]}])assert.throws(()=>validateRequest(world(),{probes:[[0,0,0]],actions:[{...action,...bad}]}));
});

test('book inputs require explicit boolean presence and integer lectern coordinates',()=>{
 const action={tick:0,type:'book',at:[0,0,0],present:false};
 assert.deepEqual(validateRequest(world(),{probes:[[0,0,0]],actions:[action]}).actions,[action]);
 for(const bad of [{present:0},{present:'false'},{present:null},{page:1},{at:[.5,0,0]}])assert.throws(()=>validateRequest(world(),{probes:[[0,0,0]],actions:[{...action,...bad}]}));
});

test('saved native queues retain signed order exactly and reject malformed entries',()=>{
 const e={kind:'fluid',id:'minecraft:water',at:[0,0,0],delay:5,priority:0,sub_tick:'9007199254740993'};
 const saved={bounds:[[0,0,0],[2,2,2]],data_version:4189,entries:[e]};
 const req={probes:[[0,0,0]],scheduled_ticks:saved};assert.deepEqual(validateRequest(world(),req).scheduled_ticks,saved);
 for(const bad of [{priority:4},{sub_tick:1},{sub_tick:'9223372036854775808'},{delay:1.5},{at:[3,0,0]},{kind:'entity'},{id:'fake'},{extra:true}])assert.throws(()=>validateRequest(world(),{...req,scheduled_ticks:{...saved,entries:[{...e,...bad}]}}));
 assert.throws(()=>validateRequest(world(),{...req,scheduled_ticks:{...saved,entries:[e,e]}}),/Duplicate/);
 assert.throws(()=>validateRequest(world(),{...req,scheduled_ticks:{...saved,bounds:[[2,2,2],[0,0,0]]}}),/Reversed/);
});

test('world inventory census is explicit and boolean',()=>{assert.equal(validateRequest(world(),{probes:[[0,0,0]],world_inventories:true}).world_inventories,true);assert.throws(()=>validateRequest(world(),{probes:[[0,0,0]],world_inventories:1}),/boolean/);});

test('complete item input is accepted only on supply actions and retains its data version',()=>{
 const action={tick:0,type:'insert',at:[0,0,0],slot:0,item:'minecraft:enchanted_book',count:1,stack:{data_version:3465,snbt:'{id:"minecraft:enchanted_book",Count:1b}'}};
 const request=actions=>({probes:[[0,0,0]],samples:[0,1],actions});
 assert.deepEqual(validateRequest(world(),request([action])).actions[0].stack,action.stack);
 for(const stack of [null,[],{}, {data_version:-1,snbt:'{}'},{data_version:4556,snbt:''},{data_version:4556,snbt:'{}',ignore:true}])assert.throws(()=>validateRequest(world(),request([{...action,stack}])));
 assert.throws(()=>validateRequest(world(),request([{tick:0,type:'toggle',at:[0,0,0],stack:action.stack}])));
 assert.throws(()=>validateRequest(world(),request([{tick:0,type:'extract',at:[0,0,0],slot:0,count:1,stack:action.stack}])));
});

test('historical block version is explicit and cannot silently coerce invalid metadata',()=>{
 const q={probes:[[0,0,0]],samples:[0],block_data_version:3218};assert.equal(validateRequest(world(),q).block_data_version,3218);
 for(const v of [-1,1.5,'3218',null])assert.throws(()=>validateRequest(world(),{...q,block_data_version:v}));
 assert.equal(validateRequest(world(),{probes:[[0,0,0]]}).block_data_version,undefined);
});

test('living census is optional and accepts only a boolean', () => {
  const args={probes:[[0,0,0]],samples:[0]};
  assert.equal(Object.hasOwn(validateRequest(world(),args),'living_entities'),false);
  for(const enabled of [false,true])assert.equal(validateRequest(world(),{...args,living_entities:enabled}).living_entities,enabled);
  for(const invalid of [0,1,'true',null,[],{}])assert.throws(()=>validateRequest(world(),{...args,living_entities:invalid}),/living_entities must be a boolean/);
});

for (const runtimeExit of ['preparing', 'running']) test(`runtime exit during ${runtimeExit} rejects and cleans up without waiting for a result file`, async () => {
  const h = harness({ runtimeExit, budgets: { ...BUDGETS, wallMs: 1500 } });
  try {
    await assert.rejects(h.verifier.run(world(), { probes: [[0,0,0]], samples: [0] }), /infrastructure failure/);
    assert.ok(h.calls.includes('stop'));
  } finally { await h.verifier.stop(); fs.rmSync(h.root, { recursive: true, force: true }); }
});

test('a pool runs concurrent tests on as many runtimes as Docker memory allows', async () => {
  const runs = [];
  const lane = (root, options) => ({
    root, pending: 0, placementMode: options.placementMode || 'updates',
    run(world, args) {
      this.pending += 1;
      runs.push(root);
      return new Promise((resolve) => setTimeout(() => { this.pending -= 1; resolve(root); }, 5));
    },
    async stop() {},
  });
  const pool = new VerifierPool({ root: '/verify', memory: async () => 16 * 2 ** 30, makeLane: lane });
  pool.placementMode = 'strict';
  const results = await Promise.all([pool.run(), pool.run(), pool.run(), pool.run()]);
  assert.deepEqual(pool.lanes.map((l) => l.root), ['/verify', '/verify/lanes/1', '/verify/lanes/2']);
  assert.deepEqual(pool.lanes.map((l) => l.placementMode), ['strict', 'strict', 'strict']);
  assert.deepEqual(results.slice(0, 3).sort(), ['/verify', '/verify/lanes/1', '/verify/lanes/2']);
  const small = new VerifierPool({ root: '/verify', memory: async () => 8 * 2 ** 30, makeLane: lane });
  await small.run();
  assert.equal(small.lanes.length, 1);
});

test('a compiler whose version could not be asked is asked again by the next compile', async () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-verify-compiler-'));
  const paperDir = path.join(root, 'paper');
  fs.mkdirSync(path.join(paperDir, 'versions', MC_VERSION), { recursive: true });
  fs.writeFileSync(path.join(paperDir, 'versions', MC_VERSION, `paper-${MC_VERSION}.jar`), 'stub');
  // A javac that is not there yet, and a jar that writes its archive.
  const javac = path.join(root, 'javac');
  const jar = path.join(root, 'jar');
  fs.writeFileSync(jar, '#!/bin/sh\necho plugin > "$2"\n', { mode: 0o755 });
  const saved = { MCBENCH_JAVAC: process.env.MCBENCH_JAVAC, MCBENCH_JAR: process.env.MCBENCH_JAR };
  Object.assign(process.env, { MCBENCH_JAVAC: javac, MCBENCH_JAR: jar });
  try {
    const compile = () => compilePlugin(paperDir, { cacheDir: path.join(root, 'cache') });
    await assert.rejects(compile(), /ENOENT/);
    fs.writeFileSync(javac, '#!/bin/sh\n[ "$1" = -version ] && echo "javac 21.0.1"\nexit 0\n', { mode: 0o755 });
    await compile();
    assert.equal(fs.readFileSync(path.join(paperDir, 'plugins', 'mcbench-verify.jar'), 'utf8'), 'plugin\n');
  } finally {
    for (const [name, value] of Object.entries(saved)) {
      if (value === undefined) delete process.env[name];
      else process.env[name] = value;
    }
    fs.rmSync(root, { recursive: true, force: true });
  }
});
