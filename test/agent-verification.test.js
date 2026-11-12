'use strict';

const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { after, test } = require('node:test');
const dataDir = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-agent-verification-'));
process.env.MCBENCH_DATA_DIR = dataDir;
require('./helpers/fake-engines').installFakeEngines();
const store = require('../server/store');
const sandbox = require('../server/sandbox');
const llm = require('../server/llm');
let respond;
llm.chatCompletion = options => respond(options);
llm.modelInfo = async () => ({ imageInput: false });
const { minecraftVerifier, VerificationBudgetError } = require('../server/minecraft-verify');
const { BUDGETS } = require('../server/verification-contract');
const { runTurn, _internal: { executeTestBuild, toolsForImageInput, executeBuild, apiMessages } } = require('../server/agent');

after(async () => { await require('../server/shot-world').stopShotEngines(); store.close(); fs.rmSync(dataDir, { recursive: true, force: true }); });

test('a dropped completion retries in the live turn without replaying a completed build', async () => {
  const conversation=store.createConversation('codex','fixture');
  let calls=0,published=0;const events=[];
  respond=async options=>{
    calls++;
    if(calls===1)return {tool_calls:[{id:'one-build',function:{name:'build',arguments:JSON.stringify({program:'module.exports = mc => mc.set([0,0,0],"stone")'})}}]};
    assert.equal(options.messages.filter(m=>m.role==='tool').length,1);
    if(calls===2){options.onDelta({toolCall:{id:'unfinished',index:0,name:'build',args:'{"program":'}});throw new TypeError('terminated');}
    return {content:'Finished.'};
  };
  await runTurn(conversation,{stone:[{},{}]},e=>events.push(e),new AbortController().signal,()=>published++);
  assert.equal(calls,3);assert.equal(published,1);
  assert.equal(events.filter(e=>e.type==='tool_start').length,1);
  assert.equal(events.filter(e=>e.type==='completion_retry').length,1);
  assert.equal(conversation.messages.filter(m=>m.role==='tool').length,1);
  assert.ok(!JSON.stringify(conversation.messages).includes('unfinished'));
});

function archive(report) {
  const reportPath = path.join(dataDir, 'verifier-reports', `${Math.random().toString(36).slice(2)}.json`);
  fs.mkdirSync(path.dirname(reportPath), { recursive: true });
  fs.writeFileSync(reportPath, JSON.stringify(report));
  return { ...report, reportFile: reportPath, probeCount: report.probes.length };
}


test('all model types get native verification and large observations remain available in the workspace', async () => {
  for (const imageInput of [true, false]) assert.ok(toolsForImageInput(imageInput).some(t => t.function.name === 'test_build'));
  const probes = Array.from({ length: 200 }, (_, x) => [x, 1, 0]);
  const blocks = probes.map(() => ({ block: 'minecraft:redstone_lamp[lit=false]', signal: 0 }));
  const report = { ok: true, worldRevision: 12, engine: 'Paper fixture', probes, initial: { tick: 0, blocks }, observations: [{ tick: 20, blocks }], inputs: [] };
  const result = await executeTestBuild({ id: 'test', world: {} }, { tests: [{}] }, undefined, { run: async () => archive(report) });
  assert.equal(result.ok, true);
  const [test] = result.tests;
  assert.equal(test.observationsInFile, true);
  assert.equal(test.observations, undefined);
  assert.deepEqual(JSON.parse(sandbox.makeReadFile('test')(test.report)), report);
  assert.equal('passed' in result, false);
});

test('engine errors are delivered to the model as verification failure', async () => {
  const result = await executeTestBuild({ id: 'test', world: {} }, { tests: [{}] }, undefined, { run: async () => { throw new Error('Docker unavailable'); } });
  assert.equal(result.ok, false);
  assert.match(result.error, /^test 1: Docker unavailable/);
  assert.equal(result.tests[0].ok, false);
  assert.equal(result.tests[0].report, undefined);
});

test('the model receives actionable budget failures without a design-failure verdict', async () => {
  const result = await executeTestBuild({ id: 'test', world: {} }, { tests: [{}] }, undefined, { run: async () => { throw new VerificationBudgetError('operations', 1001, 1000); } });
  assert.equal(result.tests[0].code, 'VERIFICATION_BUDGET_EXCEEDED');
  assert.equal(result.tests[0].budget.setting, 'MCBENCH_VERIFY_MAX_OPERATIONS');
  assert.equal(result.tests[0].report, undefined);
  assert.equal('passed' in result, false);
  const tooMany = await executeTestBuild({ id: 'test', world: {} }, { tests: Array.from({ length: BUDGETS.tests + 1 }, () => ({})) }, undefined, { run: async () => assert.fail('no test starts when the call has too many') });
  assert.equal(tooMany.code, 'VERIFICATION_BUDGET_EXCEEDED');
  assert.deepEqual(tooMany.budget, { resource: 'tests', requested: BUDGETS.tests + 1, allowed: BUDGETS.tests, setting: 'MCBENCH_VERIFY_MAX_TESTS' });
});

test('output is bounded in bytes even for few large states or a large probe list; the full report is readable', async () => {
  for (const [count, block] of [[64, `minecraft:fixture[state=${'長'.repeat(100)}]`], [2048, 'minecraft:air']]) {
    const probes = Array.from({ length: count }, (_, i) => [4096, 895, 4096 - i]);
    const blocks = probes.map(() => ({ block, signal: 0 }));
    const report = { ok: true, probes, initial: { tick: 0, blocks }, observations: [{ tick: 0, blocks }], inputs: [] };
    const result = await executeTestBuild({ id: 'test', world: {} }, { tests: [{}] }, undefined, { run: async () => archive(report) });
    assert.ok(Buffer.byteLength(JSON.stringify(result)) <= 20_000);
    const [test] = result.tests;
    assert.equal(test.probeCount, count);
    assert.equal(test.observationsInFile, true);
    if (count === 2048) assert.equal(test.probesInFile, true);
    assert.deepEqual(JSON.parse(sandbox.makeReadFile('test')(test.report)), report);
  }
});

test('cancelled tests return a distinct persisted outcome', async () => {
  const signal = AbortSignal.abort();
  const result = await executeTestBuild({ id: 'test', world: {} }, { tests: [{}] }, signal, { run: async () => signal.throwIfAborted() });
  assert.equal(result.ok, false);
  assert.equal(result.cancelled, true);
  assert.equal(result.tests[0].report, undefined);
});

test('the tests of a call run side by side, each on the same blueprint, and come back in order; a failing test does not stop the others', async () => {
  const world = {};
  const tests = [{ probes: [[0, 0, 0]], name: 'slow' }, { probes: [[1, 0, 0]], name: 'fails' }, { probes: [[2, 0, 0]], name: 'fast' }];
  let started = 0;
  let release;
  const everyoneStarted = new Promise(resolve => { release = resolve; });
  const updates = [];
  const verifier = {
    run: async (given, test, { onUpdate }) => {
      assert.equal(given, world);
      if (++started === tests.length) release();
      // A test that waited for the others to start proves they run at once.
      await everyoneStarted;
      onUpdate({ phase: 'running', message: `Observing ${test.name}` });
      if (test.name === 'fails') throw new VerificationBudgetError('columns', 2000, 1024);
      if (test.name === 'fast') return archive({ ok: true, engine: 'fixture', probes: test.probes, inputs: [], initial: { tick: 0, blocks: [] }, observations: [] });
      await new Promise(resolve => setTimeout(resolve, 20));
      return archive({ ok: true, engine: 'fixture', probes: test.probes, inputs: [], initial: { tick: 0, blocks: [] }, observations: [] });
    },
  };
  const result = await executeTestBuild({ id: 'test', world }, { tests }, undefined, verifier, update => updates.push(update.message));
  assert.equal(result.ok, false);
  assert.deepEqual(result.tests.map(test => test.ok), [true, false, true]);
  assert.deepEqual(result.tests.map(test => test.probeCount), [1, undefined, 1]);
  assert.equal(result.tests[1].code, 'VERIFICATION_BUDGET_EXCEEDED');
  assert.match(result.error, /^test 2: Minecraft test needs 2000 columns/);
  assert.equal(new Set(result.tests.filter(test => test.ok).map(test => test.report)).size, 2, 'each test keeps a report file of its own');
  result.tests.forEach((test, index) => { if (test.ok) assert.deepEqual(JSON.parse(sandbox.makeReadFile('test')(test.report)).probes, tests[index].probes); });
  assert.match(updates.at(-1), /^Test 1: .*; Test 2: .*; Test 3: /);
});

test('a call of many large tests stays within the inline budget and keeps every report in a file', async () => {
  const probes = Array.from({ length: 200 }, (_, x) => [x, 1, 0]);
  const blocks = probes.map(() => ({ block: 'minecraft:redstone_lamp[lit=false]', signal: 0 }));
  const verifier = { run: async () => archive({ ok: true, worldRevision: 3, engine: 'fixture', probes, initial: { tick: 0, blocks }, observations: [{ tick: 20, blocks }], inputs: [], preview: { initial: blocks.slice(0, 50) } }) };
  const result = await executeTestBuild({ id: 'test', world: {} }, { tests: Array.from({ length: BUDGETS.tests }, () => ({})) }, undefined, verifier);
  assert.equal(result.ok, true);
  assert.equal(result.tests.length, BUDGETS.tests);
  assert.ok(Buffer.byteLength(JSON.stringify(result)) <= 20_000);
  for (const test of result.tests) assert.equal(JSON.parse(sandbox.makeReadFile('test')(test.report)).observations.length, 1);
});

test('a call must hold a list of tests', async () => {
  for (const args of [{}, { probes: [[0, 0, 0]] }, { tests: [] }, { tests: {} }, { tests: [{}], probes: [[0, 0, 0]] }]) {
    const result = await executeTestBuild({ id: 'test', world: {} }, args, undefined, { run: async () => assert.fail('nothing runs') });
    assert.equal(result.ok, false);
    assert.match(result.error, /test_build requires \{tests/);
  }
});

test('the turn loop executes verification, persists observations, and sends them back to the model', async () => {
  const conversation = store.createConversation('openai', 'fixture');
  conversation.world.set(0, 0, 0, 'minecraft:stone');
  conversation.world.finalize();
  const revision = conversation.world.revision;
  const events = [];
  let completions = 0;
  let edits = 0;
  const oldRun = minecraftVerifier.run;
  const args = { tests: [{ probes: [[0, 0, 0]], samples: [0] }] };
  try {
    minecraftVerifier.run = async (world, input, { signal, onUpdate, reportPath }) => {
      assert.equal(world.revision, revision);
      assert.deepEqual(input, args.tests[0]);
      assert.ok(signal instanceof AbortSignal);
      onUpdate({ phase: 'running', message: 'Observing Minecraft: 0/20 ticks' });
      return archive({ ok: true, worldRevision: revision, engine: 'Paper fixture', probes: args.tests[0].probes, inputs: [],
        initial: { tick: 0, blocks: [{ block: 'minecraft:stone', signal: 0 }] },
        observations: [{ tick: 0, blocks: [{ block: 'minecraft:stone', signal: 0 }] }],
      }, reportPath);
    };
    respond = async options => {
      completions++;
      assert.ok(options.tools.some(t => t.function.name === 'test_build'));
      if (completions === 1) return { tool_calls: [{ id: 'native-test', type: 'function', function: { name: 'test_build', arguments: JSON.stringify(args) } }] };
      const message = options.messages.find(m => m.role === 'tool');
      const result = JSON.parse(message.content);
      assert.doesNotMatch(JSON.stringify(options.messages), /Observing Minecraft: 0\/20 ticks/);
      assert.equal(result.ok, true);
      assert.equal(result.tests.length, 1);
      assert.equal(result.tests[0].observations[0].blocks[0].block, 'minecraft:stone');
      assert.equal(result.tests[0].worldRevision, revision);
      return { content: 'Observed stone; no functional success claim.' };
    };
    await runTurn(conversation, {}, event => events.push(event), new AbortController().signal, () => { edits++; });
    assert.equal(completions, 2);
    assert.equal(edits, 0);
    assert.equal(conversation.world.revision, revision);
    assert.match(events.find(e => e.type === 'tool_start').program, /probes/);
    assert.match(events.find(e => e.type === 'tool_result').output, /minecraft:stone/);
    assert.equal(events.find(e => e.type === 'tool_progress').callId, 'native-test');
    assert.ok(conversation.messages.some(m => m.role === 'tool' && m.content.includes('/work/verification/')));
    assert.equal(events.at(-1).final, true);
  } finally { minecraftVerifier.run = oldRun; }
});


test('inspection, exports, and reverted writes preserve the verified world revision and do not publish live edits', async () => {
  const conversation = store.createConversation('openai', 'fixture');
  conversation.world.set(0, 0, 0, 'minecraft:stone'); conversation.world.finalize();
  const before = conversation.world;
  const registry = { stone: [{}, {}], gold_block: [{}, {}] };
  for (const program of [
    'module.exports = mc => mc.log(mc.bounds())',
    'module.exports = mc => mc.dump("inspection.json")',
    'module.exports = mc => { mc.set([0,0,0],"gold_block"); mc.set([0,0,0],"stone"); }',
  ]) {
    const result = await executeBuild(conversation, { program }, registry);
    assert.equal(result.ok, true);
    assert.equal(result.changed, false);
    assert.equal(conversation.world, before);
    assert.equal(result.worldRevision, before.revision);
  }
  assert.equal(JSON.parse(sandbox.makeReadFile(conversation.id)('inspection.json')).blocks.length, 1);
  let calls = 0; let published = 0;
  respond = async () => ++calls === 1 ? { tool_calls: [{ id: 'read', function: {
    name: 'build', arguments: JSON.stringify({ program: 'module.exports = mc => mc.log(mc.count())' }),
  } }] } : { content: 'Inspected.', telemetry: { wallMs: 12, usage: { inputTokens: 10 } } };
  await runTurn(conversation, registry, () => {}, new AbortController().signal, () => published++);
  assert.equal(published, 0);
  const tool = conversation.messages.find(m => m.role === 'tool');
  assert.ok(tool.telemetry.wallMs >= 0);
  assert.equal(tool.telemetry.name, 'build');
  assert.ok(tool.createdAt);
  assert.equal(conversation.messages.at(-1).telemetry.wallMs, 12);
  assert.ok(apiMessages(conversation).every(m => !m.telemetry && !m.createdAt));
  const changed = await executeBuild(conversation, { program: 'module.exports = mc => mc.set([1,0,0],"stone")' }, registry);
  assert.equal(changed.changed, true);
  assert.equal(changed.worldRevision, before.revision + 1);
});
