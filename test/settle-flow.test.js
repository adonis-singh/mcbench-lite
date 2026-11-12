'use strict';

// How the settle follows a conversation through what a user does to it: reverting to
// an earlier message, switching branches, forking, a build that is settled but not
// applied, a stopped turn, several conversations building at once, and a message with
// viewer selections. The engines are fakes (helpers/fake-engines.js) in which sand
// falls a block a tick; test/settle-flow.integration.test.js is the same with Minecraft.

const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { after, beforeEach, test } = require('node:test');
const dataDir = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-settle-flow-'));
process.env.MCBENCH_DATA_DIR = dataDir;
process.env.MCBENCH_SETTLE_ENGINES = '3';
const { FakeEngines, installFakeEngines } = require('./helpers/fake-engines');
installFakeEngines();
const shot = require('../server/shot');
shot.renderShots = async (conversationId, shots) => shots.map(() => Buffer.from('jpeg'));
const store = require('../server/store');
const sandbox = require('../server/sandbox');
const llm = require('../server/llm');
let respond;
llm.chatCompletion = (options) => respond(options);
llm.modelInfo = async () => ({ imageInput: true });
const { runTurn, _internal: { executeBuild, executeSeeBuild } } = require('../server/agent');
const { describeSelection } = require('../server/selection');
const { shotWorld, stopShotEngines } = require('../server/shot-world');

const REGISTRY = { stone: [{}, {}], sand: [{}, {}], oak_planks: [{}, {}] };
FakeEngines.rules = (blocks) => {
  const sand = [...blocks].filter(([, spec]) => spec === 'minecraft:sand').map(([key]) => key.split(',').map(Number)).sort((a, b) => a[1] - b[1]);
  for (const [x, y, z] of sand) {
    if (y > -60 && !blocks.has(`${x},${y - 1},${z}`)) { blocks.delete(`${x},${y},${z}`); blocks.set(`${x},${y - 1},${z}`, 'minecraft:sand'); }
  }
};

const TOWER = 'module.exports = mc => { mc.fill([0, 0, 0], [4, 0, 4], "stone"); mc.fill([2, 12, 2], [2, 14, 2], "sand"); };';
const planks = (x) => `module.exports = mc => mc.set([${x}, 0, 0], "oak_planks");`;
const READ = (...at) => `module.exports = mc => { mc.log(${at.map(([x, y, z]) => `(mc.settled(${x}, ${y}, ${z})?.id ?? "air").replace("minecraft:", "")`).join(', ')}); };`;

beforeEach(() => { FakeEngines.places.length = 0; FakeEngines.ticks = []; FakeEngines.failures = 0; FakeEngines.delay = 0; FakeEngines.peak = 0; });
after(async () => { await stopShotEngines(); store.close(); fs.rmSync(dataDir, { recursive: true, force: true }); });

// What handleChat does: the user's message keeps the world it was sent against.
function send(conversation, content) {
  const message = store.appendMessage(conversation, { role: 'user', content }, conversation.world.reference());
  store.save(conversation);
  return message;
}

// A model that sends each program as a build call, then ends the turn.
async function turn(conversation, programs, { signal = new AbortController().signal, onWorldChange = () => {} } = {}) {
  const steps = [...programs];
  let calls = 0;
  respond = async () => (steps.length ? { tool_calls: [{ id: `call-${calls++}`, function: { name: 'build', arguments: JSON.stringify({ program: steps.shift() }) } }] } : { content: 'Done.' });
  await runTurn(conversation, REGISTRY, () => {}, signal, onWorldChange);
}

const builds = (conversation) => conversation.messages.filter((m) => m.role === 'tool').map((m) => JSON.parse(m.content));
const blocksOf = (world) => { const out = []; world.forEach((x, y, z, entry) => out.push(`${x},${y},${z} ${entry.id.replace('minecraft:', '')}`)); return out.sort(); };
const settled = (conversation, tick = 210) => blocksOf(shotWorld(conversation, null, tick));
const sandY = (list) => list.filter((line) => line.endsWith(' sand')).map((line) => Number(line.split(',')[1])).sort((a, b) => a - b);

test('reverting to an earlier message: mc.settled, the build report and see_build show the reverted world, not the revision it shares a number with', async () => {
  const conversation = store.createConversation('openai', 'fixture');
  send(conversation, 'Build a sand tower.');
  await turn(conversation, [TOWER]);
  const second = send(conversation, 'Add planks at x=10.');
  await turn(conversation, [planks(10)]);
  const rejected = conversation.world;
  assert.equal(rejected.revision, 2);
  assert.ok(settled(conversation).includes('10,0,0 oak_planks'));

  store.revert(conversation, second.id);
  assert.notEqual(conversation.world, rejected);
  assert.equal(conversation.world.revision, 1);
  const placesBefore = FakeEngines.places.length;

  send(conversation, 'Add planks at x=20 instead.');
  await turn(conversation, [`module.exports = mc => { mc.log(${['(10, 0, 0)', '(20, 0, 0)', '(2, 1, 2)', '(2, 13, 2)'].map((at) => `(mc.settled${at}?.id ?? "air").replace("minecraft:", "")`).join(', ')}); mc.set([20, 0, 0], "oak_planks"); };`]);
  const reverted = builds(conversation).at(-1);
  assert.deepEqual(reverted.logs, ['air air sand air'], 'the revision-2 world of the rejected branch is not what mc.settled reads');
  assert.equal(reverted.worldRevision, 2, 'the same revision number as the rejected branch\'s world');
  assert.equal(FakeEngines.places.length, placesBefore + 2, 'the reverted world, then the build on it');
  assert.deepEqual([reverted.minecraft.removed, reverted.minecraft.added, reverted.minecraft.replaced], [3, 3, 0]);
  const now = settled(conversation);
  assert.ok(now.includes('20,0,0 oak_planks') && !now.includes('10,0,0 oak_planks'));
  assert.deepEqual(sandY(now), [1, 2, 3]);

  const seen = await executeSeeBuild(conversation, { shots: [{ view: 'front' }] });
  assert.equal(seen.ok, true, seen.error);
  assert.equal(FakeEngines.places.length, placesBefore + 2, 'see_build uses the settle the build made');
  assert.deepEqual(seen.minecraft.map((report) => [report.ticks, report.removed, report.added]), [[210, 3, 3]]);
});

test('activating another branch and back: each branch\'s world is the one settled', async () => {
  const conversation = store.createConversation('openai', 'fixture');
  send(conversation, 'Build a sand tower.');
  await turn(conversation, [TOWER]);
  const firstBranch = send(conversation, 'Planks at x=10.');
  await turn(conversation, [planks(10)]);
  store.revert(conversation, firstBranch.id);
  const secondBranch = send(conversation, 'Planks at x=20.');
  await turn(conversation, [planks(20)]);
  const rejected = conversation.world;

  store.activateBranch(conversation, firstBranch.id);
  assert.notEqual(conversation.world, rejected);
  await turn(conversation, []);
  const probe = async () => {
    const result = await executeBuild(conversation, { program: READ([10, 0, 0], [20, 0, 0], [2, 1, 2]) }, REGISTRY);
    assert.equal(result.ok, true, result.error);
    return result.logs[0];
  };
  assert.equal(await probe(), 'oak_planks air sand');
  const places = FakeEngines.places.length;
  assert.equal(await probe(), 'oak_planks air sand');
  assert.equal(FakeEngines.places.length, places, 'the branch\'s world was settled once');
  const grown = await executeBuild(conversation, { program: planks(11) }, REGISTRY);
  assert.deepEqual([grown.minecraft.removed, grown.minecraft.added], [3, 3]);
  assert.ok(settled(conversation).includes('11,0,0 oak_planks') && settled(conversation).includes('10,0,0 oak_planks') && !settled(conversation).includes('20,0,0 oak_planks'));

  store.activateBranch(conversation, secondBranch.id);
  assert.equal(await probe(), 'air oak_planks sand');
  const back = await executeSeeBuild(conversation, { shots: [{ view: 'front', tick: 210 }] });
  assert.equal(back.ok, true, back.error);
  const drawn = settled(conversation);
  assert.ok(drawn.includes('20,0,0 oak_planks') && !drawn.includes('10,0,0 oak_planks') && !drawn.includes('11,0,0 oak_planks'));
});

test('a candidate settled but not applied leaves the conversation\'s own settle, ticks included', async (t) => {
  const conversation = store.createConversation('openai', 'fixture');
  await executeBuild(conversation, { program: TOWER, settle_ticks: 5 }, REGISTRY);
  const kept = conversation.world;
  const places = FakeEngines.places.length;
  t.mock.method(sandbox, 'writeWorkFile', () => { throw new Error('the disk is full'); });
  await assert.rejects(executeBuild(conversation, { program: 'module.exports = mc => { mc.writeFile("a.txt", "x"); mc.set([30, 0, 0], "oak_planks"); }', settle_ticks: 40 }, REGISTRY), /the disk is full/);
  t.mock.restoreAll();
  assert.equal(conversation.world, kept);
  assert.equal(FakeEngines.places.length, places + 1, 'the candidate was settled');
  const next = await executeBuild(conversation, { program: 'module.exports = mc => { mc.log(mc.settledTicks, mc.settled(30, 0, 0)?.id ?? "air"); mc.set([31, 0, 0], "oak_planks"); }' }, REGISTRY);
  assert.deepEqual(next.logs, ['5 air'], 'the world the program started from keeps the ticks it was settled with');
  assert.equal(FakeEngines.places.length, places + 2, 'the earlier world needed no second settle');
  assert.ok(!settled(conversation, 210).includes('30,0,0 oak_planks'));
});

test('a stopped turn keeps the build it applied and its settle', async () => {
  const conversation = store.createConversation('openai', 'fixture');
  const controller = new AbortController();
  send(conversation, 'Build, then I will stop you.');
  await assert.rejects(turn(conversation, [TOWER, planks(10)], { signal: controller.signal, onWorldChange: () => controller.abort() }), { name: 'AbortError' });
  assert.equal(builds(conversation).length, 1);
  // What executeTurn (server/index.js) records for the stop.
  store.appendMessage(conversation, { role: 'assistant', content: '(stopped by user)', stopped: true });
  assert.equal(conversation.world.size(), 25 + 3);
  const places = FakeEngines.places.length;
  send(conversation, 'Continue.');
  await turn(conversation, [READ([2, 1, 2], [2, 13, 2])]);
  assert.deepEqual(builds(conversation).at(-1).logs, ['sand air']);
  assert.equal(FakeEngines.places.length, places, 'no settle for a world that was settled');
});

test('a fork settles the world it was made with, apart from its source', async () => {
  const source = store.createConversation('openai', 'fixture');
  send(source, 'Build a sand tower.');
  await turn(source, [TOWER]);
  const places = FakeEngines.places.length;
  const fork = store.forkConversation(source, source.messages.length, source.world);
  assert.notEqual(fork.id, source.id);
  const forked = await executeBuild(fork, { program: `module.exports = mc => { mc.log((mc.settled(2, 1, 2)?.id ?? "air").replace("minecraft:", "")); mc.set([40, 0, 0], "oak_planks"); }` }, REGISTRY);
  assert.deepEqual(forked.logs, ['sand']);
  assert.equal(FakeEngines.places.length, places + 2, 'the fork\'s world, then its build');
  assert.deepEqual([forked.minecraft.removed, forked.minecraft.added], [3, 3]);
  assert.ok(settled(fork).includes('40,0,0 oak_planks'));
  assert.ok(!settled(source).includes('40,0,0 oak_planks'));
  const again = await executeBuild(source, { program: READ([40, 0, 0], [2, 1, 2]) }, REGISTRY);
  assert.deepEqual(again.logs, ['air sand']);
  assert.equal(FakeEngines.places.length, places + 2, 'the source\'s settle was kept');
});

test('two conversations building at once each get the verdict on their own world', async () => {
  const [one, two] = [store.createConversation('openai', 'fixture'), store.createConversation('openai', 'fixture')];
  FakeEngines.delay = 25;
  const [a, b] = await Promise.all([
    executeBuild(one, { program: TOWER }, REGISTRY),
    executeBuild(two, { program: 'module.exports = mc => { mc.fill([0, 0, 0], [2, 0, 2], "stone"); mc.fill([1, 5, 1], [1, 5, 1], "sand"); }' }, REGISTRY),
  ]);
  assert.equal(FakeEngines.peak, 2);
  assert.deepEqual([a.minecraft.removed, a.minecraft.added, a.minecraft.blocks], [3, 3, 28]);
  assert.deepEqual([b.minecraft.removed, b.minecraft.added, b.minecraft.blocks], [1, 1, 10]);
  const [readA, readB] = await Promise.all([
    executeBuild(one, { program: READ([2, 1, 2], [1, 1, 1]) }, REGISTRY),
    executeBuild(two, { program: READ([2, 1, 2], [1, 1, 1]) }, REGISTRY),
  ]);
  assert.deepEqual([readA.logs[0], readB.logs[0]], ['sand air', 'air sand']);
});

test('a message with viewer selections names build coordinates that a following build can use', async () => {
  const conversation = store.createConversation('openai', 'fixture');
  send(conversation, 'Build a sand tower.');
  await turn(conversation, [TOWER]);
  // The viewer selects blocks of the blueprint (build coordinates): the stone under the tower.
  const min = [1, 0, 1];
  const max = [3, 0, 3];
  const selection = { letter: 'A', ...describeSelection(conversation.world, min, max), min, max };
  send(conversation, [{ type: 'text', text: 'Make A planks.' }, { type: 'selection', selection }]);
  let seen = null;
  let sent = false;
  respond = async (options) => {
    if (sent) return { content: 'Done.' };
    const text = options.messages.findLast((m) => m.role === 'user').content.map((part) => part.text).join('\n');
    seen = /\[Selection 1 \(([^)]*)\): x (-?\d+)\.\.(-?\d+), y (-?\d+)\.\.(-?\d+), z (-?\d+)\.\.(-?\d+); (\d+) blocks\]/.exec(text);
    sent = true;
    const [, , x0, x1, y0, y1, z0, z1] = seen;
    const program = `module.exports = mc => { mc.log(mc.settled(${x0}, ${y0}, ${z0}).id); mc.fill([${x0}, ${y0}, ${z0}], [${x1}, ${y1}, ${z1}], "oak_planks"); };`;
    return { tool_calls: [{ id: 'select', function: { name: 'build', arguments: JSON.stringify({ program }) } }] };
  };
  await runTurn(conversation, REGISTRY, () => {}, new AbortController().signal, () => {});
  assert.deepEqual(seen.slice(2, 8).map(Number), [1, 3, 0, 0, 1, 3], 'the message names the blocks in build coordinates');
  assert.equal(seen[8], '9');
  const result = builds(conversation).at(-1);
  assert.deepEqual(result.logs, ['minecraft:stone'], 'mc.settled reads the same coordinates');
  assert.deepEqual([result.minecraft.replaced, result.minecraft.removed, result.minecraft.added], [0, 3, 3], 'against the blueprint with the planks');
  assert.equal(result.diff.changed, 9);
  const now = settled(conversation);
  assert.equal(now.filter((line) => line.endsWith('oak_planks')).length, 9);
  assert.ok(now.includes('1,0,1 oak_planks') && now.includes('3,0,3 oak_planks') && now.includes('0,0,0 stone'));
});
