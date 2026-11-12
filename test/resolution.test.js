'use strict';

// A chat's build resolution (web/resolution.mjs): its stops, the section it
// adds to the system prompt (server/prompt.js), and the message and bench
// routes of server/index.js that set it, driven through handleApi with a
// fake model and sandbox.

const assert = require('node:assert/strict');
const crypto = require('node:crypto');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { Readable } = require('node:stream');
const { EventEmitter } = require('node:events');
const { after, test } = require('node:test');

const dataDir = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-resolution-'));
process.env.MCBENCH_DATA_DIR = dataDir;
process.env.MCBENCH_SETTINGS_FILE = path.join(dataDir, 'settings.json');
require('../server/assets').getBlockRegistry = async () => ({ stone: [{}, {}] });
const llm = require('../server/llm');
// The system prompt of every turn, by model.
const systems = {};
llm.chatCompletion = async (options) => {
  if (options.tools.length) systems[options.model] = options.messages[0].content;
  return { content: options.tools.length ? 'Done.' : 'A title' };
};
llm.modelInfo = async () => ({ imageInput: true });
const sandbox = require('../server/sandbox');
sandbox.reset = async () => {};
sandbox.shutdown = async () => {};
const store = require('../server/store');
const app = require('../server/index');
const { RESOLUTIONS, resolutionStop, resolutionHelp } = require('../web/resolution.mjs');
const { systemPrompt } = require('../server/prompt');

after(async () => {
  await app.shutdown();
  fs.rmSync(dataDir, { recursive: true, force: true });
});

// A request through handleApi, answered as route() answers a thrown error.
function call(method, pathname, body) {
  const req = Readable.from(body === undefined ? [] : [Buffer.from(JSON.stringify(body))]);
  Object.assign(req, { method, url: pathname, headers: {} });
  const res = new EventEmitter();
  res.lines = [];
  res.writeHead = (status) => { res.status = status; res.headersSent = true; };
  res.write = (line) => { res.lines.push(JSON.parse(line)); return true; };
  res.end = (payload) => {
    if (payload) res.body = JSON.parse(payload);
    res.writableEnded = true;
    res.emit('close');
  };
  return app.handleApi(req, res, new URL(pathname, 'http://app.invalid')).catch((error) => {
    res.status = error.statusCode || 500;
    res.body = { error: error.message };
  }).then(() => res);
}

async function free(id) {
  for (let hold; (hold = app.reservations.get(id));) await hold.done;
}

const section = (options) => systemPrompt(options).split('## Resolution\n')[1];

test('the stops are Standard, High, Very high and Maximum, and any other value throws', () => {
  assert.deepEqual(RESOLUTIONS.map((stop) => [stop.value, stop.label, stop.feature, stop.across]), [
    [1, 'Standard', null, null],
    [2, 'High (2×)', 20, 100],
    [5, 'Very high (5×)', 50, 250],
    [10, 'Maximum (10×)', 100, 500],
  ]);
  assert.equal(resolutionStop(5).label, 'Very high (5×)');
  for (const value of [0, 3, '5', null, undefined, true]) assert.throws(() => resolutionStop(value), /not a resolution/);
  assert.throws(() => systemPrompt({ resolution: 3 }), /not a resolution/);
});

test("each stop's help in the picker says only what that stop does", () => {
  assert.deepEqual(RESOLUTIONS.map(resolutionHelp), [
    'The model chooses the size.',
    'About 20 blocks for a main feature such as an eye or a window. About 100 blocks across the whole build.',
    'About 50 blocks for a main feature such as an eye or a window. About 250 blocks across the whole build.',
    'About 100 blocks for a main feature such as an eye or a window. About 500 blocks across the whole build.',
  ]);
});

test('at Standard the system prompt is byte for byte the prompt from before resolutions', () => {
  // SHA-256 of the prompt every chat got before resolutions, for each kind
  // of model; the private benchmark depends on it being unchanged.
  const pinned = {
    'true,true': 'aabff65ea2e4c0cb064ccf10a13b16e4b64c4e36ccb82f096f81b31d152a08b5',
    'true,false': '931a363b83f6a833aa083c06601ee9161a09f829a660fc51930baf0a9d6084eb',
    'false,true': '7b8245514d8bfe962c77940fb5748ee13bda7fce263cd3adb63956230fc0397c',
    'false,false': '8a232ba98fd14fa7f38430a2497e8e9231350445cf1e4f98313797906abe26a0',
  };
  for (const [key, hash] of Object.entries(pinned)) {
    const [imageInput, networkAccess] = key.split(',').map((value) => value === 'true');
    const prompt = systemPrompt({ imageInput, networkAccess, resolution: 1 });
    assert.equal(crypto.createHash('sha256').update(prompt).digest('hex'), hash, key);
    assert.ok(!prompt.includes('## Resolution'));
  }
});

test('above Standard one section follows the unchanged prompt, with the blocks per feature and across', () => {
  const base = systemPrompt({ resolution: 1 });
  for (const [resolution, label, feature, across] of [[2, 'High (2×)', 20, 100], [5, 'Very high (5×)', 50, 250], [10, 'Maximum (10×)', 100, 500]]) {
    const prompt = systemPrompt({ resolution });
    assert.ok(prompt.startsWith(`${base}\n\n## Resolution\n`), String(resolution));
    assert.equal(prompt.split('## Resolution').length, 2);
    const text = section({ resolution });
    assert.ok(text.includes(`This chat's resolution is ${label}`));
    assert.ok(text.includes(`about ${resolution} times your usual linear scale`));
    assert.ok(text.includes(`give a main feature about 10 blocks (an eye, a window, a wheel), give it about ${feature}`));
    assert.ok(text.includes(`about ${across} blocks across where the subject allows`));
    assert.match(text, /A size or scale the user states still comes first, and a mechanism keeps the size it needs to work/);
    assert.match(text, /Spend the size on detail, not on bigger flat surfaces/);
  }
});

test('stops 5 and 10 give the numbers that size a build program and a shell; stop 2 does not', () => {
  assert.doesNotMatch(section({ resolution: 2 }), /Make the geometry with code/);
  for (const resolution of [5, 10]) {
    const text = section({ resolution });
    assert.match(text, /Loop over surfaces, not over every cell of a bounding box/);
    assert.match(text, /a program that runs out of time places nothing/);
    assert.match(text, /a solid cube 250 blocks across is more than five times the limit/);
    assert.match(text, /about 500 blocks across for a box/);
    assert.match(text, /at most 8 MiB/);
  }
});

test('the section tells a model with no image input nothing about screenshots', () => {
  assert.match(section({ resolution: 10, imageInput: true }), /see_build from far enough[^]*size: "small"/);
  for (const resolution of [2, 5, 10]) assert.doesNotMatch(section({ resolution, imageInput: false }), /see_build/);
});

const message = (resolution) => ({ provider: 'openrouter', model: 'chat/model', text: 'Build a hut', ...(resolution === undefined ? {} : { resolution }) });

test('a chat takes the resolution sent with each message, and its turn gets that prompt', async () => {
  const { body: { id } } = await call('POST', '/api/conversations', { provider: 'openrouter', model: 'chat/model' });
  const sent = await call('POST', `/api/conversations/${id}/chat`, message(5));
  assert.equal(sent.lines.at(-1).type, 'done');
  await free(id);
  assert.equal(store.getConversation(id).resolution, 5);
  assert.equal(store.listConversations().find((row) => row.id === id).resolution, 5);
  assert.equal(systems['chat/model'], systemPrompt({ resolution: 5 }));
  await call('POST', `/api/conversations/${id}/chat`, message(1));
  await free(id);
  assert.equal(store.getConversation(id).resolution, 1);
  assert.equal(systems['chat/model'], systemPrompt({ resolution: 1 }));
});

test('a message or bench with a missing or unknown resolution is refused with 400 and changes nothing', async () => {
  const { body: { id } } = await call('POST', '/api/conversations', { provider: 'openrouter', model: 'chat/model' });
  for (const resolution of [undefined, 3, '10', true]) {
    const refused = await call('POST', `/api/conversations/${id}/chat`, message(resolution));
    assert.deepEqual([refused.status, refused.body.error], [400, 'Resolution must be one of 1, 2, 5, 10'], String(resolution));
    const bench = await call('POST', '/api/benches', { text: 'A hut', resolution, members: [{ provider: 'openrouter', model: 'x/a' }, { provider: 'openrouter', model: 'x/b' }] });
    assert.deepEqual([bench.status, bench.body.error], [400, 'Resolution must be one of 1, 2, 5, 10'], String(resolution));
  }
  assert.deepEqual(store.getConversation(id).messages, []);
  assert.deepEqual(store.listBenches(), []);
});

test("a new bench passes its resolution to every model's chat", async () => {
  const { status, body } = await call('POST', '/api/benches', { text: 'A cathedral', resolution: 10, members: [{ provider: 'openrouter', model: 'bench/a' }, { provider: 'openrouter', model: 'bench/b' }] });
  assert.equal(status, 200, body.error);
  const members = store.benchMembers(body.id);
  await Promise.all(members.map((member) => free(member.id)));
  assert.deepEqual(members.map((member) => store.getConversation(member.id).resolution), [10, 10]);
  assert.equal(store.listBenches().find((row) => row.id === body.id).resolution, 10);
  assert.equal(systems['bench/a'], systemPrompt({ resolution: 10 }));
});
