'use strict';

// test/settle-flow.test.js with Minecraft: the real engines settle the worlds of a
// conversation that is reverted, branched, forked and built in beside another. Run
// with MCBENCH_SETTLE_INTEGRATION=1 and MCBENCH_DATA_DIR set to a data folder with
// the Paper server (npm run test:settle); wrap in the native-engine lock.

const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { after, test } = require('node:test');

const enabled = process.env.MCBENCH_SETTLE_INTEGRATION === '1';
const dataDir = enabled ? process.env.MCBENCH_DATA_DIR : fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-settle-integration-'));
process.env.MCBENCH_DATA_DIR = dataDir;
process.env.MCBENCH_SETTLE_ENGINES = '2';
const shot = require('../server/shot');
shot.renderShots = async (conversationId, shots) => shots.map(() => Buffer.from('jpeg'));
const store = require('../server/store');
const llm = require('../server/llm');
let respond;
llm.chatCompletion = (options) => respond(options);
llm.modelInfo = async () => ({ imageInput: true });
const { runTurn, _internal: { executeBuild, executeSeeBuild } } = require('../server/agent');
const { getBlockRegistry } = require('../server/assets');
const { shotWorld, settle, stopShotEngines } = require('../server/shot-world');

after(async () => { await stopShotEngines(); store.close(); });

const TOWER = 'module.exports = mc => { mc.fill([0, 0, 0], [4, 0, 4], "stone"); mc.fill([2, 12, 2], [2, 14, 2], "sand"); mc.set([4, 1, 4], "water"); };';
const planks = (x) => `module.exports = mc => mc.set([${x}, 0, 0], "oak_planks");`;
const names = (conversation, tick) => { const out = []; shotWorld(conversation, null, tick).forEach((x, y, z, entry) => out.push(`${x},${y},${z} ${entry.id.replace('minecraft:', '')}${Object.keys(entry.states).length ? JSON.stringify(entry.states) : ''}`)); return out.sort(); };

function send(conversation, content) {
  const message = store.appendMessage(conversation, { role: 'user', content }, conversation.world.reference());
  store.save(conversation);
  return message;
}

async function turn(registry, conversation, programs) {
  const steps = [...programs];
  let calls = 0;
  respond = async () => (steps.length ? { tool_calls: [{ id: `call-${calls++}`, function: { name: 'build', arguments: JSON.stringify({ program: steps.shift() }) } }] } : { content: 'Done.' });
  await runTurn(conversation, registry, () => {}, new AbortController().signal, () => {});
}

const probe = async (registry, conversation) => (await executeBuild(conversation, { program: 'module.exports = mc => mc.log(...[[10, 0, 0], [20, 0, 0], [2, 1, 2], [2, 13, 2]].map(([x, y, z]) => (mc.settled(x, y, z)?.id ?? "air").replace("minecraft:", "")), mc.settledTicks);' }, registry)).logs[0];

test('revert, branches and forks are settled by Minecraft as the worlds they are', { skip: !enabled, timeout: 600_000 }, async () => {
  const registry = await getBlockRegistry();
  const conversation = store.createConversation('openai', 'fixture');
  send(conversation, 'Tower.');
  await turn(registry, conversation, [TOWER]);
  const first = send(conversation, 'Planks at 10.');
  await turn(registry, conversation, [planks(10)]);
  const rejected = conversation.world;
  assert.equal(await probe(registry, conversation), 'oak_planks air sand air 210');

  store.revert(conversation, first.id);
  assert.notEqual(conversation.world, rejected);
  const second = send(conversation, 'Planks at 20.');
  await turn(registry, conversation, [planks(20)]);
  assert.equal(await probe(registry, conversation), 'air oak_planks sand air 210');
  const secondBlocks = names(conversation, 210);
  assert.ok(secondBlocks.includes('2,3,2 sand') && secondBlocks.includes('20,0,0 oak_planks') && !secondBlocks.includes('10,0,0 oak_planks'));
  assert.ok(secondBlocks.some((line) => line.startsWith('4,1,4 water')), 'the water source is still there');
  assert.ok(secondBlocks.filter((line) => / water/.test(line)).length > 1, 'and has flowed off the platform');

  store.activateBranch(conversation, first.id);
  assert.equal(await probe(registry, conversation), 'oak_planks air sand air 210');
  store.activateBranch(conversation, second.id);
  assert.equal(await probe(registry, conversation), 'air oak_planks sand air 210');

  const fork = store.forkConversation(conversation, conversation.messages.length, conversation.world);
  assert.equal(await probe(registry, fork), 'air oak_planks sand air 210');
  assert.deepEqual(names(fork, 210), names(conversation, 210), 'the fork\'s settle is the source\'s');
  const seen = await executeSeeBuild(fork, { shots: [{ view: 'front', tick: 0 }, { view: 'front', tick: 30 }, { view: 'front' }] });
  assert.equal(seen.ok, true, seen.error);
  assert.deepEqual(seen.minecraft.map((report) => report.ticks), [0, 30, 210]);
  assert.equal(seen.minecraft[0].removed + seen.minecraft[0].added + seen.minecraft[0].replaced, 0, 'as placed, nothing has moved');
  assert.equal(names(fork, 0).filter((line) => line.endsWith('sand')).length, 3);
});

test('conversations settled at once on separate engines give what one engine gives', { skip: !enabled, timeout: 600_000 }, async () => {
  const registry = await getBlockRegistry();
  const programs = [TOWER, 'module.exports = mc => { mc.fill([0, 0, 0], [6, 0, 6], "stone"); mc.fill([3, 9, 3], [3, 9, 3], "sand"); mc.fill([0, 1, 0], [0, 1, 0], "lava"); };', planks(1), 'module.exports = mc => { mc.fill([0, 0, 0], [8, 0, 8], "stone"); mc.set([4, 6, 4], "water"); };'];
  const together = programs.map(() => store.createConversation('openai', 'fixture'));
  const built = await Promise.all(together.map((conversation, i) => executeBuild(conversation, { program: programs[i], settle_ticks: 120 }, registry)));
  const alone = [];
  for (const program of programs) {
    const conversation = store.createConversation('openai', 'fixture');
    await executeBuild(conversation, { program, settle_ticks: 120 }, registry);
    alone.push(conversation);
  }
  for (let i = 0; i < programs.length; i += 1) {
    assert.equal(built[i].ok, true, built[i].error);
    assert.deepEqual(names(together[i], 120), names(alone[i], 120), `build ${i}`);
    const again = await settle(alone[i].id, alone[i].world, [120]);
    assert.equal(again.get(120).cached, true);
  }
});
