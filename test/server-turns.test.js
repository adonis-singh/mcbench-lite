'use strict';

// The turn and bench lifecycle of server/index.js, driven through handleApi
// with a fake model and sandbox: how a turn ends and holds a bench queue,
// what a chat message changes when it is refused, what holds a chat or bench
// while an edit awaits, a round that cannot start, what a bench chat refuses,
// a turn whose end was not saved, startup recovery, titles, writes that fail
// midway (a message's files, a fork, a bench edit), the data folder's one
// owner and shutdown.

const assert = require('node:assert/strict');
const crypto = require('node:crypto');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawnSync } = require('node:child_process');
const { EventEmitter } = require('node:events');
const { Readable } = require('node:stream');
const { test } = require('node:test');
const Database = require('better-sqlite3');

const dataDir = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-server-turns-'));
process.env.MCBENCH_DATA_DIR = dataDir;
process.env.MCBENCH_SETTINGS_FILE = path.join(dataDir, 'settings.json');
require('../server/assets').getBlockRegistry = async () => ({ stone: [{}, {}] });
const llm = require('../server/llm');
// Turns go to `models[model]` (default: a final answer), titles to `title`.
const models = {};
let title = async () => ({ content: 'A title' });
llm.chatCompletion = (options) => (options.tools.length ? (models[options.model] || (async () => ({ content: 'Done.' })))(options) : title(options));
llm.modelInfo = async () => ({ imageInput: true });
const sandbox = require('../server/sandbox');
let reset = async () => {};
sandbox.reset = (id) => reset(id);
let sandboxesShut = 0;
sandbox.shutdown = async () => { sandboxesShut += 1; };
const store = require('../server/store');
const storage = require('../server/storage');
const app = require('../server/index');

const PNG = `data:image/png;base64,${Buffer.from('png').toString('base64')}`;
const TXT = `data:text/plain;base64,${Buffer.from('notes').toString('base64')}`;

function deferred() {
  let resolve;
  const promise = new Promise((r) => { resolve = r; });
  return { promise, resolve };
}

// A completion that waits for `gate`, or rejects when the turn is stopped.
const waits = (gate, reply = { content: 'Done.' }) => (options) => new Promise((resolve, reject) => {
  options.signal.addEventListener('abort', () => reject(options.signal.reason));
  gate.then(() => resolve(reply));
});

// A request through handleApi, answered as route() answers a thrown error.
function request(method, pathname, body) {
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
  res.done = app.handleApi(req, res, new URL(pathname, 'http://app.invalid')).catch((error) => {
    res.status = error.statusCode || 500;
    res.body = { error: error.message };
  }).then(() => res);
  return res;
}
const call = (method, pathname, body) => request(method, pathname, body).done;

// Resolves once nothing holds `id`.
async function free(id) {
  for (let hold; (hold = app.reservations.get(id));) await hold.done;
}
const tick = () => new Promise(setImmediate);

async function newChat() {
  const { body } = await call('POST', '/api/conversations', { provider: 'openrouter', model: 'chat/model' });
  return body.id;
}

// `message` is the first round: its text, or its text with files or `blind`.
async function newBench(message, ...memberModels) {
  const { status, body } = await call('POST', '/api/benches', { resolution: 1, ...(typeof message === 'string' ? { text: message } : message), members: memberModels.map((model) => ({ provider: 'openrouter', model })) });
  assert.equal(status, 200, body.error);
  const bench = store.getBench(body.id);
  const members = store.benchMembers(bench.id);
  return { bench, ids: Object.fromEntries(members.map((member) => [member.model, member.id])) };
}

const prompts = (id) => store.getConversation(id).messages.filter((message) => message.role === 'user' && !message.seeBuild);

// What the title model is asked to name.
const titled = (options) => options.messages[1].content;

// While `work` runs, `statement` (an INSERT or UPDATE on a table, with its
// WHEN) fails as a full disk does.
async function failing(statement, work) {
  const db = new Database(store.DATABASE_FILE);
  db.exec(`CREATE TRIGGER refuse_write BEFORE ${statement} BEGIN SELECT RAISE(ABORT, 'the disk is full'); END`);
  try {
    return await work();
  } finally {
    db.exec('DROP TRIGGER refuse_write');
    db.close();
  }
}
const savingChat = (id) => `UPDATE ON conversations WHEN NEW.id = '${id}'`;

test('a stop during a tool is recorded as a stop, which holds the model\'s bench queue', async (t) => {
  const started = deferred();
  models['stop/a'] = async () => ({ tool_calls: [{ id: 'wait', function: { name: 'bash', arguments: '{"command":"sleep 100"}' } }] });
  t.mock.method(sandbox, 'exec', (id, command, timeout, { signal }) => new Promise((resolve, reject) => {
    signal.addEventListener('abort', () => reject(new Error('killed')));
    started.resolve();
  }));
  const { bench, ids } = await newBench('A hut', 'stop/a', 'stop/b');
  await started.promise;
  await call('POST', `/api/conversations/${ids['stop/a']}/stop`);
  await Promise.all([free(ids['stop/a']), free(ids['stop/b'])]);
  const last = store.getConversation(ids['stop/a']).messages.at(-1);
  assert.deepEqual([last.content, last.stopped], ['(stopped by user)', true]);
  assert.deepEqual(store.benchProgress(ids['stop/a']), { round: 1, halted: 'stopped' });

  const { status } = await call('POST', `/api/benches/${bench.id}/rounds`, { text: 'Add a door' });
  assert.equal(status, 200);
  await Promise.all([free(ids['stop/a']), free(ids['stop/b'])]);
  assert.equal(prompts(ids['stop/a']).length, 1, 'the stopped model does not take the next round');
  assert.equal(prompts(ids['stop/b']).length, 2);
});

test('a chat message is checked whole before anything changes, and a failed save leaves nothing in memory', async () => {
  const id = await newChat();
  const refused = await call('POST', `/api/conversations/${id}/chat`, { provider: 'nope', model: 'chat/model', resolution: 1, effort: 'high', text: 'Build', files: [{ name: 'ref.png', data: PNG }] });
  assert.deepEqual([refused.status, refused.body.error], [400, 'Choose a configured provider']);
  assert.equal(store.getConversation(id).effort, null);
  assert.equal(fs.existsSync(path.join(store.ATTACHMENTS_DIR, id)), false, 'no attachment is written for a refused message');
  assert.equal(app.reservations.has(id), false);

  const db = new Database(store.DATABASE_FILE);
  db.exec(`CREATE TRIGGER refuse_message BEFORE INSERT ON messages WHEN NEW.conversation_id = '${id}' BEGIN SELECT RAISE(ABORT, 'the disk is full'); END`);
  try {
    const failed = await call('POST', `/api/conversations/${id}/chat`, { provider: 'openrouter', model: 'chat/model', resolution: 1, effort: 'high', text: 'Build' });
    assert.deepEqual([failed.status, failed.body.error], [500, 'the disk is full']);
  } finally {
    db.exec('DROP TRIGGER refuse_message');
    db.close();
  }
  const conversation = store.getConversation(id);
  assert.deepEqual(conversation.messages, [], 'the unsaved message is not served');
  assert.equal(conversation.effort, null);
  const sent = await call('POST', `/api/conversations/${id}/chat`, { provider: 'openrouter', model: 'chat/model', resolution: 1, text: 'Build' });
  assert.equal(sent.lines.at(-1).type, 'done');
  assert.deepEqual(store.getConversation(id).messages.map((message) => message.role), ['user', 'assistant']);
});

test('a revert or branch holds the chat until its sandbox is reset, and none runs under a turn', async () => {
  const id = await newChat();
  await call('POST', `/api/conversations/${id}/chat`, { provider: 'openrouter', model: 'chat/model', resolution: 1, text: 'Build a hut' });
  const [first] = prompts(id);

  const gate = deferred();
  models['chat/model'] = waits(gate.promise);
  const running = request('POST', `/api/conversations/${id}/chat`, { provider: 'openrouter', model: 'chat/model', resolution: 1, text: 'Add a door' });
  await tick();
  const underTurn = await call('POST', `/api/conversations/${id}/revert`, { messageId: first.id });
  assert.equal(underTurn.status, 409);
  assert.equal((await call('POST', `/api/conversations/${id}/branch`, { messageId: first.id })).status, 409);
  gate.resolve();
  await running.done;
  delete models['chat/model'];

  const resetting = deferred();
  reset = () => resetting.promise;
  const revert = request('POST', `/api/conversations/${id}/revert`, { messageId: first.id });
  await tick();
  assert.deepEqual(prompts(id), [], 'the revert itself is done before the reset');
  const blocked = await call('POST', `/api/conversations/${id}/chat`, { provider: 'openrouter', model: 'chat/model', resolution: 1, text: 'Again' });
  assert.equal(blocked.status, 409, 'no turn starts on a shell about to be killed');
  resetting.resolve();
  assert.equal((await revert.done).status, 200);
  reset = async () => {};
  const again = await call('POST', `/api/conversations/${id}/chat`, { provider: 'openrouter', model: 'chat/model', resolution: 1, text: 'Again' });
  assert.equal(again.lines.at(-1).type, 'done');
});

test('a bench retry holds the model until its sandbox is reset, even when another model finishes meanwhile', async () => {
  models['retry/a'] = async () => { throw new Error('HTTP 400: bad request'); };
  const gate = deferred();
  models['retry/b'] = waits(gate.promise);
  const { bench, ids } = await newBench('A tower', 'retry/a', 'retry/b');
  await free(ids['retry/a']);
  assert.deepEqual(store.benchProgress(ids['retry/a']), { round: 1, halted: 'failed' });
  delete models['retry/a'];

  const resetting = deferred();
  reset = () => resetting.promise;
  const retry = request('POST', `/api/benches/${bench.id}/retry`, { memberId: ids['retry/a'] });
  await tick();
  assert.equal((await call('POST', `/api/benches/${bench.id}/retry`, { memberId: ids['retry/a'] })).status, 409);
  // The other model finishing runs advanceBench while the retry awaits.
  gate.resolve();
  await free(ids['retry/b']);
  assert.equal(app.reservations.turn(ids['retry/a']), undefined, 'the reverted round does not start under the reset');
  assert.deepEqual(prompts(ids['retry/a']), []);
  resetting.resolve();
  assert.equal((await retry.done).status, 200);
  reset = async () => {};
  await free(ids['retry/a']);
  assert.deepEqual(store.getConversation(ids['retry/a']).messages.map((message) => message.role), ['user', 'assistant']);
  assert.deepEqual(store.benchProgress(ids['retry/a']), { round: 1, halted: null });
});

test('a bench round edit holds the bench: its head moves first, and no round is added until the resets end', async () => {
  const { bench, ids } = await newBench('A tower', 'edit/a', 'edit/b');
  await Promise.all(Object.values(ids).map(free));
  await call('POST', `/api/benches/${bench.id}/rounds`, { text: 'Add a roof' });
  await Promise.all(Object.values(ids).map(free));
  const roof = store.getBench(bench.id).rounds[1];

  const resetting = deferred();
  reset = () => resetting.promise;
  const revert = request('POST', `/api/benches/${bench.id}/revert`, { roundId: roof.id });
  await tick();
  assert.equal(store.getBench(bench.id).rounds.length, 1);
  const added = await call('POST', `/api/benches/${bench.id}/rounds`, { text: 'Add a spire' });
  assert.equal(added.status, 409);
  assert.equal(store.getBench(bench.id).nodes.length, 2);
  resetting.resolve();
  const { status, body } = await revert.done;
  assert.deepEqual([status, body.rounds, body.text], [200, 1, 'Add a roof']);
  reset = async () => {};
  for (const id of Object.values(ids)) assert.equal(prompts(id).length, 1);
});

test('a bench whose message is refused leaves nothing saved', async () => {
  const before = store.listBenches().length;
  const { status, body } = await call('POST', '/api/benches', { text: '  ', resolution: 1, members: [{ provider: 'openrouter', model: 'x/a' }, { provider: 'openrouter', model: 'x/b' }] });
  assert.deepEqual([status, body.error], [400, 'Message text or an attachment is required']);
  assert.equal(store.listBenches().length, before);
});

test('after a restart a crashed model reads as failed and can be retried, and queued rounds start', async () => {
  const id = crypto.randomUUID();
  const { bench, members: [crashed, queued] } = store.createBench(id, {
    title: 'Crash', blind: false, content: 'A well',
    members: [{ provider: 'openrouter', model: 'crash/a', effort: null }, { provider: 'openrouter', model: 'crash/b', effort: null }],
  });
  // The server died during the first model's turn, before the second started.
  store.appendMessage(crashed, { role: 'user', content: 'A well', round: bench.rounds[0].id }, crashed.world.reference());
  store.appendMessage(crashed, { role: 'assistant', content: '', tool_calls: [{ id: 'lost', function: { name: 'bash', arguments: '{}' } }] });
  store.save(crashed);

  assert.ok(store.interruptUnfinished().includes(crashed.id));
  app.advanceBench(bench.id);
  assert.ok(app.reservations.turn(queued.id), 'the queued round starts');
  assert.equal(app.reservations.has(crashed.id), false, 'the crashed model is held for a retry');
  await free(queued.id);
  assert.deepEqual(store.benchProgress(crashed.id), { round: 1, halted: 'failed' });
  assert.equal((await call('POST', `/api/benches/${bench.id}/retry`, { memberId: crashed.id })).status, 200);
  await free(crashed.id);
  assert.deepEqual(store.getConversation(crashed.id).messages.map((message) => message.role), ['user', 'assistant']);
});

test('a turn stopped while its reply streams keeps the text the user saw on the stopped message, which no model request carries', async () => {
  const id = await newChat();
  models['chat/model'] = (options) => new Promise((resolve, reject) => {
    options.signal.addEventListener('abort', () => reject(Object.assign(options.signal.reason, { reply: 'First the floor, then' })));
  });
  const turn = request('POST', `/api/conversations/${id}/chat`, { provider: 'openrouter', model: 'chat/model', resolution: 1, text: 'Plan a hut' });
  await tick();
  await call('POST', `/api/conversations/${id}/stop`);
  await turn.done;
  const last = store.getConversation(id).messages.at(-1);
  assert.deepEqual([last.content, last.stopped, last.reply], ['(stopped by user)', true, 'First the floor, then']);
  let sent;
  models['chat/model'] = async (options) => { sent = options.messages; return { content: 'Done.' }; };
  await call('POST', `/api/conversations/${id}/chat`, { provider: 'openrouter', model: 'chat/model', resolution: 1, text: 'continue' });
  assert.ok(sent.every((message) => !('reply' in message)));
  delete models['chat/model'];
});

test('stopping a turn does not stop its title, and a chat still untitled gets one on its next turn', async () => {
  const id = await newChat();
  const named = deferred();
  title = async () => { await named.promise; return { content: 'Stone hut' }; };
  const gate = deferred();
  models['chat/model'] = waits(gate.promise);
  const turn = request('POST', `/api/conversations/${id}/chat`, { provider: 'openrouter', model: 'chat/model', resolution: 1, text: 'Build a hut' });
  await tick();
  await call('POST', `/api/conversations/${id}/stop`);
  await turn.done;
  named.resolve();
  await tick();
  assert.equal(store.getConversation(id).title, 'Stone hut');

  const other = await newChat();
  title = async () => { throw new Error('HTTP 400: no title'); };
  models['chat/model'] = async () => ({ content: 'Done.' });
  await call('POST', `/api/conversations/${other}/chat`, { provider: 'openrouter', model: 'chat/model', resolution: 1, text: 'Build a tower' });
  await tick();
  assert.equal(store.getConversation(other).title, store.DEFAULT_TITLE);
  assert.equal(store.getConversation(other).messages.at(-1).content, 'Title generation failed: HTTP 400: no title');
  title = async () => ({ content: 'Tall tower' });
  await call('POST', `/api/conversations/${other}/chat`, { provider: 'openrouter', model: 'chat/model', resolution: 1, text: 'Make it taller' });
  await tick();
  assert.equal(store.getConversation(other).title, 'Tall tower');
  delete models['chat/model'];
});

test('a bench round that cannot start is stored as that model\'s failed round, which a retry runs again', async (t) => {
  const share = store.shareAttachment;
  let full = true;
  t.mock.method(store, 'shareAttachment', (attachment, id) => {
    if (full && store.getConversation(id).model === 'start/a') throw new Error('This chat is out of storage');
    return share(attachment, id);
  });
  const { bench, ids } = await newBench({ text: 'A hut', blind: true, files: [{ name: 'notes.txt', data: TXT }] }, 'start/a', 'start/b', 'start/c');
  await Promise.all(Object.values(ids).map(free));
  const failed = store.getConversation(ids['start/a']).messages;
  assert.deepEqual(failed.map((message) => message.role), ['user', 'error']);
  assert.deepEqual([failed[0].round, failed[1].content], [bench.rounds[0].id, 'This chat is out of storage']);
  assert.deepEqual(store.benchProgress(ids['start/a']), { round: 1, halted: 'failed' });
  // The round is over for every model, so the two that built it can be judged.
  const vote = await call('POST', `/api/benches/${bench.id}/vote`, { roundId: bench.rounds[0].id, pick: ids['start/b'] });
  assert.equal(vote.status, 200, vote.body.error);

  full = false;
  const retry = await call('POST', `/api/benches/${bench.id}/retry`, { memberId: ids['start/a'] });
  assert.equal(retry.status, 200, retry.body.error);
  await free(ids['start/a']);
  assert.deepEqual(store.getConversation(ids['start/a']).messages.map((message) => message.role), ['user', 'assistant']);
  assert.deepEqual(store.benchProgress(ids['start/a']), { round: 1, halted: null });
});

test('a bench round whose prompt cannot be saved is stored as failed once the store takes it', async (t) => {
  const { bench, members: [unsaved, other] } = store.createBench(crypto.randomUUID(), {
    title: 'Full', blind: false, content: 'A well',
    members: [{ provider: 'openrouter', model: 'full/a', effort: null }, { provider: 'openrouter', model: 'full/b', effort: null }],
  });
  // The disk is full for the prompt, and has room again right after.
  const db = new Database(store.DATABASE_FILE);
  db.exec(`CREATE TRIGGER refuse_message BEFORE INSERT ON messages WHEN NEW.conversation_id = '${unsaved.id}' BEGIN SELECT RAISE(ABORT, 'the disk is full'); END`);
  const { save } = store;
  t.mock.method(store, 'save', (conversation) => {
    try {
      save(conversation);
    } finally {
      if (conversation.id === unsaved.id) db.exec('DROP TRIGGER IF EXISTS refuse_message');
    }
  });
  try {
    app.advanceBench(bench.id);
  } finally {
    db.close();
  }
  await free(other.id);
  assert.equal(app.reservations.has(unsaved.id), false);
  const stored = store.getConversation(unsaved.id).messages;
  assert.deepEqual(stored.map((message) => [message.role, message.content]), [['user', 'A well'], ['error', 'the disk is full']]);
  assert.deepEqual(store.benchProgress(unsaved.id), { round: 1, halted: 'failed' });
});

test('a bench chat is changed only through its bench: no chat-level prompt, revert, branch or archive', async () => {
  const { bench, ids } = await newBench('A hut', 'own/a', 'own/b');
  await Promise.all(Object.values(ids).map(free));
  const [prompt] = prompts(ids['own/a']);
  const message = { provider: 'openrouter', model: 'own/a', resolution: 1, text: 'Add a door' };
  for (const [route, body] of [['chat', message], ['revert', { messageId: prompt.id }], ['branch', { messageId: prompt.id }], ['archive']]) {
    const refused = await call('POST', `/api/conversations/${ids['own/a']}/${route}`, body);
    assert.deepEqual([route, refused.status, refused.body.error], [route, 409, 'A bench chat is changed only through its bench']);
  }
  assert.equal(prompts(ids['own/a']).length, 1, 'the model keeps its round');
  assert.equal(store.benchMembers(bench.id).length, 2, 'and its place in the bench');
});

test('a refused fork does not reveal a blind bench; the fork made does', async () => {
  const gate = deferred();
  models['blind/a'] = waits(gate.promise);
  const { bench, ids } = await newBench({ text: 'A hut', blind: true }, 'blind/a', 'blind/b');
  await free(ids['blind/b']);
  const unknown = await call('POST', `/api/conversations/${ids['blind/a']}/fork`, { round: 9 });
  assert.deepEqual([unknown.status, unknown.body.error], [400, 'Unknown round']);
  const running = await call('POST', `/api/conversations/${ids['blind/a']}/fork`, { round: 1 });
  assert.equal(running.status, 409);
  assert.equal(store.getBench(bench.id).revealed, false);

  gate.resolve();
  await free(ids['blind/a']);
  const fork = await call('POST', `/api/conversations/${ids['blind/a']}/fork`, { round: 1 });
  assert.equal(fork.status, 200, fork.body.error);
  assert.equal(store.getBench(bench.id).revealed, true);
});

test('a bench takes votes only while it hides its models\' names', async () => {
  const open = await newBench('A hut', 'open/a', 'open/b');
  await Promise.all(Object.values(open.ids).map(free));
  const shown = await call('POST', `/api/benches/${open.bench.id}/vote`, { roundId: open.bench.rounds[0].id, pick: open.ids['open/a'] });
  assert.deepEqual([shown.status, shown.body.error], [409, 'Votes are cast while the models\' names are hidden']);

  const blind = await newBench({ text: 'A hut', blind: true }, 'hid/a', 'hid/b');
  await Promise.all(Object.values(blind.ids).map(free));
  const round = blind.bench.rounds[0].id;
  const cast = await call('POST', `/api/benches/${blind.bench.id}/vote`, { roundId: round, pick: blind.ids['hid/a'] });
  assert.equal(cast.status, 200, cast.body.error);
  assert.equal((await call('POST', `/api/benches/${blind.bench.id}/reveal`)).status, 200);
  // Revealed, the vote can be neither changed nor taken back.
  for (const pick of [blind.ids['hid/b'], 'tie', null]) {
    assert.equal((await call('POST', `/api/benches/${blind.bench.id}/vote`, { roundId: round, pick })).status, 409);
  }
  assert.deepEqual(store.getBench(blind.bench.id).votes[round], { pick: blind.ids['hid/a'], blind: true });
});

test('a turn whose end could not be saved is ended before its chat takes the next prompt', async (t) => {
  const id = await newChat();
  const running = deferred();
  const lost = deferred();
  models['chat/model'] = async () => ({ tool_calls: [{ id: 'lost', function: { name: 'bash', arguments: '{"command":"make"}' } }] });
  t.mock.method(sandbox, 'exec', async () => {
    running.resolve();
    await lost.promise;
    throw new Error('the sandbox is gone');
  });
  const turn = request('POST', `/api/conversations/${id}/chat`, { provider: 'openrouter', model: 'chat/model', resolution: 1, text: 'Build a hut' });
  await running.promise;
  // The call is stored; neither its result nor how the turn ends can be.
  const db = new Database(store.DATABASE_FILE);
  db.exec(`CREATE TRIGGER refuse_message BEFORE INSERT ON messages WHEN NEW.conversation_id = '${id}' BEGIN SELECT RAISE(ABORT, 'the disk is full'); END`);
  try {
    lost.resolve();
    const { lines } = await turn.done;
    assert.match(lines.at(-1).message, /saving this failed: the disk is full/);
  } finally {
    db.exec('DROP TRIGGER refuse_message');
    db.close();
  }
  assert.equal(app.reservations.has(id), false);
  assert.deepEqual(store.getConversation(id).messages.map((message) => message.role), ['user', 'assistant'], 'the stored turn is under way');

  let sent;
  models['chat/model'] = async (options) => { sent = options.messages; return { content: 'Done.' }; };
  const next = await call('POST', `/api/conversations/${id}/chat`, { provider: 'openrouter', model: 'chat/model', resolution: 1, text: 'Add a door' });
  delete models['chat/model'];
  assert.equal(next.lines.at(-1).type, 'done');
  assert.deepEqual(store.getConversation(id).messages.map((message) => message.role), ['user', 'assistant', 'tool', 'error', 'user', 'assistant']);
  const calls = sent.flatMap((message) => message.tool_calls ?? []).map((made) => made.id);
  assert.deepEqual(sent.filter((message) => message.role === 'tool').map((message) => message.tool_call_id), calls, 'the model is sent a result for every call');
});

test('a bench model whose turn end was not saved holds its queue as failed, and can be retried', async () => {
  const { bench, members: [unsaved, queued] } = store.createBench(crypto.randomUUID(), {
    title: 'Unsaved', blind: false, content: 'A well',
    members: [{ provider: 'openrouter', model: 'unsaved/a', effort: null }, { provider: 'openrouter', model: 'unsaved/b', effort: null }],
  });
  store.appendMessage(unsaved, { role: 'user', content: 'A well', round: bench.rounds[0].id }, unsaved.world.reference());
  store.appendMessage(unsaved, { role: 'assistant', content: '', tool_calls: [{ id: 'lost', function: { name: 'bash', arguments: '{}' } }] });
  store.save(unsaved);

  app.advanceBench(bench.id);
  await free(queued.id);
  assert.equal(app.reservations.has(unsaved.id), false);
  assert.deepEqual(store.getConversation(unsaved.id).messages.map((message) => message.role), ['user', 'assistant', 'tool', 'error']);
  assert.deepEqual(store.benchProgress(unsaved.id), { round: 1, halted: 'failed' });
  assert.equal((await call('POST', `/api/benches/${bench.id}/retry`, { memberId: unsaved.id })).status, 200);
  await free(unsaved.id);
  assert.deepEqual(store.getConversation(unsaved.id).messages.map((message) => message.role), ['user', 'assistant']);
});

test('a title that arrives for a reverted first prompt is dropped, and the new first prompt is titled', async () => {
  const id = await newChat();
  const slow = deferred();
  title = (options) => (titled(options) === 'Build a hut' ? slow.promise.then(() => ({ content: 'Stone hut' })) : Promise.resolve({ content: 'Tall tower' }));
  await call('POST', `/api/conversations/${id}/chat`, { provider: 'openrouter', model: 'chat/model', resolution: 1, text: 'Build a hut' });
  // The hut's title is still under way when the prompt is replaced.
  const [hut] = prompts(id);
  assert.equal((await call('POST', `/api/conversations/${id}/revert`, { messageId: hut.id })).status, 200);
  await call('POST', `/api/conversations/${id}/chat`, { provider: 'openrouter', model: 'chat/model', resolution: 1, text: 'Build a tower' });
  await tick();
  const before = store.getConversation(id).title;
  slow.resolve();
  await tick();
  assert.equal(before, 'Tall tower', 'the new first prompt is titled while the old title is under way');
  assert.equal(store.getConversation(id).title, 'Tall tower', 'the title of the prompt that is gone does not land');
  // A follow-up does not title the chat again.
  title = async () => ({ content: 'A door' });
  await call('POST', `/api/conversations/${id}/chat`, { provider: 'openrouter', model: 'chat/model', resolution: 1, text: 'Add a door' });
  await tick();
  assert.equal(store.getConversation(id).title, 'Tall tower');
});

test('a bench whose title failed is titled from its first round when it is sent its next round', async () => {
  title = async () => { throw new Error('HTTP 400: no title'); };
  const { bench, ids } = await newBench({ text: 'A hut', files: [{ name: 'ref.png', data: PNG }] }, 'title/a', 'title/b');
  await Promise.all(Object.values(ids).map(free));
  await tick();
  assert.equal(store.getBench(bench.id).title, 'New bench');
  let asked;
  title = async (options) => { asked = titled(options); return { content: 'Two huts' }; };
  await call('POST', `/api/benches/${bench.id}/rounds`, { text: 'Add a door' });
  await Promise.all(Object.values(ids).map(free));
  await tick();
  assert.equal(store.getBench(bench.id).title, 'Two huts');
  assert.deepEqual(asked, [{ type: 'text', text: 'A hut' }, { type: 'image_url', image_url: { url: PNG } }], 'the first round names the bench, with its image');
  assert.deepEqual(Object.values(ids).map((id) => store.getConversation(id).title), ['Two huts', 'Two huts']);
  // Once titled, a round does not title it again.
  title = async () => ({ content: 'Doors' });
  await call('POST', `/api/benches/${bench.id}/rounds`, { text: 'Add a window' });
  await Promise.all(Object.values(ids).map(free));
  await tick();
  assert.equal(store.getBench(bench.id).title, 'Two huts');
});

test('a bench title that arrives for a first round since edited is dropped, and the new first round is titled', async () => {
  const slow = deferred();
  title = (options) => (titled(options) === 'A hut' ? slow.promise.then(() => ({ content: 'Stone hut' })) : Promise.resolve({ content: 'Tall tower' }));
  const { bench, ids } = await newBench('A hut', 'retitle/a', 'retitle/b');
  await Promise.all(Object.values(ids).map(free));
  assert.equal((await call('POST', `/api/benches/${bench.id}/revert`, { roundId: bench.rounds[0].id })).status, 200);
  await call('POST', `/api/benches/${bench.id}/rounds`, { text: 'A tower' });
  await Promise.all(Object.values(ids).map(free));
  await tick();
  const before = store.getBench(bench.id).title;
  slow.resolve();
  await tick();
  title = async () => ({ content: 'A title' });
  assert.equal(before, 'Tall tower');
  assert.equal(store.getBench(bench.id).title, 'Tall tower');
});

test('a message, bench or round that cannot be saved leaves none of its files and gives their bytes back', async () => {
  const files = [{ name: 'notes.txt', data: TXT }];
  const id = await newChat();
  const none = storage.used(id);
  const message = await failing(`INSERT ON messages WHEN NEW.conversation_id = '${id}'`, () => call('POST', `/api/conversations/${id}/chat`, { provider: 'openrouter', model: 'chat/model', resolution: 1, text: 'Build', files }));
  assert.deepEqual([message.status, message.body.error], [500, 'the disk is full']);
  assert.equal(fs.existsSync(path.join(store.ATTACHMENTS_DIR, id)), false);
  assert.equal(storage.used(id), none);

  const folders = () => (fs.existsSync(store.ATTACHMENTS_DIR) ? fs.readdirSync(store.ATTACHMENTS_DIR).sort() : []);
  const before = folders();
  const members = [{ provider: 'openrouter', model: 'files/a' }, { provider: 'openrouter', model: 'files/b' }];
  const created = await failing('INSERT ON benches', () => call('POST', '/api/benches', { text: 'A hut', resolution: 1, files, members }));
  assert.deepEqual([created.status, created.body.error], [500, 'the disk is full']);
  assert.deepEqual(folders(), before, 'no folder is left for a bench that was not made');

  const { bench, ids } = await newBench('A hut', 'files/a', 'files/b');
  await Promise.all(Object.values(ids).map(free));
  const round = await failing(`UPDATE ON benches WHEN NEW.id = '${bench.id}'`, () => call('POST', `/api/benches/${bench.id}/rounds`, { text: 'Add a door', files }));
  assert.deepEqual([round.status, round.body.error], [500, 'the disk is full']);
  assert.equal(store.getBench(bench.id).nodes.length, 1);
  assert.equal(fs.existsSync(path.join(store.ATTACHMENTS_DIR, bench.id)), false);
  assert.equal(storage.used(bench.id), 0);
});

test('a fork whose workspace cannot be copied is not made: nothing is stored, listed or revealed', async (t) => {
  const { bench, ids } = await newBench({ text: 'A hut', blind: true, files: [{ name: 'notes.txt', data: TXT }] }, 'fork/a', 'fork/b');
  await Promise.all(Object.values(ids).map(free));
  const chats = () => store.listConversations().filter((row) => !row.benchId).length;
  const folders = () => ['sandbox', 'attachments', 'worlds'].map((name) => path.join(dataDir, name)).map((dir) => (fs.existsSync(dir) ? fs.readdirSync(dir).sort() : []));
  const before = [chats(), folders()];
  const { cpSync } = fs;
  t.mock.method(fs, 'cpSync', (from, to, options) => {
    if (to.startsWith(path.join(dataDir, 'sandbox'))) throw new Error('the disk is full');
    return cpSync(from, to, options);
  });
  const failed = await call('POST', `/api/conversations/${ids['fork/a']}/fork`, { round: 1 });
  t.mock.restoreAll();
  assert.deepEqual([failed.status, failed.body.error], [500, 'the disk is full']);
  assert.deepEqual([chats(), folders()], before, 'no chat and no folder of the fork is left');
  assert.equal(store.getBench(bench.id).revealed, false, 'and the bench stays blind');

  const fork = await call('POST', `/api/conversations/${ids['fork/a']}/fork`, { round: 1 });
  assert.equal(fork.status, 200, fork.body.error);
  assert.equal(chats(), before[0] + 1);
  assert.equal(fs.existsSync(path.join(dataDir, 'sandbox', fork.body.id, 'attachments')), true);
});

// A bench of three models on its second round, which the third failed, with
// a vote on that round for the first.
async function benchOnRoundTwo(name) {
  let broken = false;
  models[`${name}/c`] = async () => { if (broken) throw new Error('HTTP 400: bad request'); return { content: 'Done.' }; };
  const { bench, ids } = await newBench({ text: 'A tower', blind: true }, `${name}/a`, `${name}/b`, `${name}/c`);
  const all = Object.values(ids);
  await Promise.all(all.map(free));
  broken = true;
  await call('POST', `/api/benches/${bench.id}/rounds`, { text: 'Add a roof' });
  await Promise.all(all.map(free));
  broken = false;
  const roof = store.getBench(bench.id).rounds[1];
  const vote = await call('POST', `/api/benches/${bench.id}/vote`, { roundId: roof.id, pick: all[0] });
  assert.equal(vote.status, 200, vote.body.error);
  // The rounds each model's chat is on.
  const lines = () => all.map((id) => prompts(id).map((prompt) => prompt.round));
  return { bench, all, roof, lines };
}

test('a bench round edit that fails at one model moves no model: none is sent the round again', async () => {
  const { bench, all, roof, lines } = await benchOnRoundTwo('edit-whole');
  const before = lines();
  // The first model's chat is saved; the second's is not.
  const revert = await failing(savingChat(all[1]), () => call('POST', `/api/benches/${bench.id}/revert`, { roundId: roof.id }));
  assert.deepEqual([revert.status, revert.body.error], [500, 'the disk is full']);
  assert.deepEqual(lines(), before, 'no model went back without the bench');
  assert.equal(store.getBench(bench.id).rounds.length, 2);
  app.advanceBench(bench.id);
  assert.deepEqual(app.reservations.turns(), [], 'so no model has a round to run again');
});

test('a bench retry that fails takes back no vote', async () => {
  const { bench, all, roof, lines } = await benchOnRoundTwo('retry-whole');
  const before = lines();
  const retry = await failing(savingChat(all[2]), () => call('POST', `/api/benches/${bench.id}/retry`, { memberId: all[2] }));
  assert.deepEqual([retry.status, retry.body.error], [500, 'the disk is full']);
  assert.deepEqual(store.getBench(bench.id).votes[roof.id], { pick: all[0], blind: true });
  assert.deepEqual(lines(), before);
});

test('a bench branch switch that fails at one model leaves the bench and every model on their line', async () => {
  const { bench, all, roof, lines } = await benchOnRoundTwo('branch-whole');
  assert.equal((await call('POST', `/api/benches/${bench.id}/revert`, { roundId: roof.id })).status, 200);
  await call('POST', `/api/benches/${bench.id}/rounds`, { text: 'Add a spire' });
  await Promise.all(all.map(free));
  const spire = store.getBench(bench.id).rounds[1];
  const before = lines();
  assert.deepEqual(before, all.map(() => [bench.rounds[0].id, spire.id]));
  const branch = await failing(savingChat(all[1]), () => call('POST', `/api/benches/${bench.id}/branch`, { roundId: roof.id }));
  assert.deepEqual([branch.status, branch.body.error], [500, 'the disk is full']);
  assert.equal(store.getBench(bench.id).head, spire.id, 'the bench stays on the line its models are on');
  assert.deepEqual(lines(), before);
  app.advanceBench(bench.id);
  assert.deepEqual(app.reservations.turns(), []);
});

test('an error is not dismissed under a running turn: a save that failed would drop the chat from under it', async () => {
  const id = await newChat();
  models['chat/model'] = async () => { throw new Error('HTTP 400: bad request'); };
  await call('POST', `/api/conversations/${id}/chat`, { provider: 'openrouter', model: 'chat/model', resolution: 1, text: 'Build a hut' });
  const error = store.getConversation(id).messages.at(-1);
  assert.equal(error.role, 'error');
  const gate = deferred();
  models['chat/model'] = waits(gate.promise);
  const turn = request('POST', `/api/conversations/${id}/chat`, { provider: 'openrouter', model: 'chat/model', resolution: 1, text: 'Again' });
  await tick();
  const under = await failing(savingChat(id), () => call('POST', `/api/conversations/${id}/dismiss-error`, { messageId: error.id }));
  gate.resolve();
  const { lines } = await turn.done;
  delete models['chat/model'];
  assert.equal(lines.at(-1).type, 'done', 'the turn is saved');
  assert.deepEqual([under.status, under.body.error], [409, 'Wait for the running turn before dismissing an error']);
  assert.equal((await call('POST', `/api/conversations/${id}/dismiss-error`, { messageId: error.id })).status, 200);
  assert.equal(store.getConversation(id).messages.find((message) => message.id === error.id).dismissed, true);
});

test('with no title model chosen, a chat is titled by its own model and a bench by its first model, at low effort whatever effort they run at', async (t) => {
  const settings = require('../server/settings');
  const { get } = settings;
  let titleModel = null;
  t.mock.method(settings, 'get', () => ({ ...get(), titleModel }));
  const asked = [];
  title = async (options) => { asked.push([options.provider, options.model, options.effort]); return { content: 'A title' }; };
  const id = await newChat();
  await call('POST', `/api/conversations/${id}/chat`, { provider: 'openrouter', model: 'chat/model', resolution: 1, effort: 'high', text: 'Build a hut' });
  await tick();
  const { ids } = await newBench('A tower', 'first/model', 'second/model');
  await Promise.all(Object.values(ids).map(free));
  await tick();
  titleModel = { provider: 'openrouter', model: 'title/model', effort: 'low' };
  const other = await newChat();
  await call('POST', `/api/conversations/${other}/chat`, { provider: 'openrouter', model: 'chat/model', resolution: 1, text: 'Build a well' });
  await tick();
  title = async () => ({ content: 'A title' });
  assert.deepEqual(asked, [['openrouter', 'chat/model', 'low'], ['openrouter', 'first/model', 'low'], ['openrouter', 'title/model', 'low']]);
});

test('a revert, branch switch or dismissal that cannot be saved is a 500; one the store refuses is a 400', async () => {
  const id = await newChat();
  models['chat/model'] = async () => { throw new Error('HTTP 400: bad request'); };
  await call('POST', `/api/conversations/${id}/chat`, { provider: 'openrouter', model: 'chat/model', resolution: 1, text: 'Build a hut' });
  delete models['chat/model'];
  const [prompt] = prompts(id);
  const error = store.getConversation(id).messages.at(-1);
  for (const [route, body] of [['revert', { messageId: prompt.id }], ['branch', { messageId: prompt.id }], ['dismiss-error', { messageId: error.id }]]) {
    const failed = await failing(savingChat(id), () => call('POST', `/api/conversations/${id}/${route}`, body));
    assert.deepEqual([route, failed.status, failed.body.error], [route, 500, 'the disk is full']);
    const refused = await call('POST', `/api/conversations/${id}/${route}`, { messageId: 'nope' });
    assert.equal(refused.status, 400, route);
  }
});

test('a bench whose model cannot be loaded is logged by its id when it is advanced, and other benches carry on', async (t) => {
  const make = (name) => store.createBench(crypto.randomUUID(), {
    title: name, blind: false, content: 'A well',
    members: [{ provider: 'openrouter', model: `${name}/a`, effort: null }, { provider: 'openrouter', model: `${name}/b`, effort: null }],
  });
  const broken = make('unloadable');
  const fine = make('loadable');
  // One model's stored world is cut short, and the chat is not in memory.
  const [member] = broken.members;
  const db = new Database(store.DATABASE_FILE);
  db.prepare('UPDATE conversations SET world_manifest_json = ? WHERE id = ?').run('{"chunks":', member.id);
  db.close();
  await failing(savingChat(member.id), () => assert.throws(() => store.save(member)));
  const logged = t.mock.method(console, 'error', () => {});
  app.advanceBench(broken.bench.id);
  assert.ok(logged.mock.calls.some((entry) => String(entry.arguments[0]).includes(broken.bench.id)), 'the failure names its bench');
  app.advanceBench(fine.bench.id);
  assert.ok(app.reservations.turn(fine.members[0].id), 'the next bench starts its rounds');
  await Promise.all(fine.members.map((entry) => free(entry.id)));
});

test('a second server on the same data folder fails at once, naming this one, and changes nothing in the folder', () => {
  // What a starting server collects: the world folder of a chat that is not stored.
  const orphan = path.join(dataDir, 'worlds', crypto.randomUUID());
  fs.mkdirSync(orphan, { recursive: true });
  // The server's module, loaded as a starting server loads it; nothing listens.
  const second = spawnSync(process.execPath, ['-e', `require(${JSON.stringify(require.resolve('../server/index'))})`], { encoding: 'utf8', timeout: 60_000 });
  const kept = fs.existsSync(orphan);
  fs.rmSync(orphan, { recursive: true, force: true });
  assert.equal(kept, true, 'the second server collected nothing');
  assert.notEqual(second.status, 0);
  assert.match(second.stderr, new RegExp(`server\\.pid is held by process ${process.pid}, which is still running`));
  assert.equal(fs.readFileSync(path.join(dataDir, 'server.pid'), 'utf8'), String(process.pid));
});

// Last: shutdown closes the store.
test('shutdown refuses new turns and rounds and waits for every turn under way, and every signal waits for that one shutdown', async () => {
  const gate = deferred();
  // Notices the stop only once `gate` opens; shutdown waits for it until then.
  models['down/a'] = (options) => gate.promise.then(() => { throw options.signal.reason; });
  const { bench, ids } = await newBench('A hut', 'down/a', 'down/b');
  await free(ids['down/b']);
  const id = await newChat();
  let down = false;
  const stopping = app.shutdown().then(() => { down = true; });
  const refused = await call('POST', `/api/conversations/${id}/chat`, { provider: 'openrouter', model: 'chat/model', resolution: 1, text: 'Build' });
  assert.deepEqual([refused.status, refused.body.error], [503, 'The server is shutting down']);
  // A second call is the same shutdown, and so is every signal from here on.
  let again = false;
  app.shutdown().then(() => { again = true; });
  const proc = new EventEmitter();
  const exits = [];
  proc.exit = (code) => exits.push(code);
  app.exitOnSignals(proc);
  for (const signal of ['SIGTERM', 'SIGINT', 'SIGTERM']) proc.emit(signal);
  await tick();
  await tick();
  assert.deepEqual([down, again, exits], [false, false, []], 'nothing ends before the turn under way is saved');
  assert.equal(app.reservations.has(ids['down/a']), true);
  gate.resolve();
  await stopping;
  await tick();
  assert.deepEqual([again, exits], [true, [0, 0, 0]], 'each signal exits once the shutdown is done');
  assert.equal(app.reservations.has(ids['down/a']), false, 'the running turn ended before shutdown did');
  assert.equal(sandboxesShut, 1, 'every container is removed');
  // The stopped turn was recorded before the store closed; no round started after.
  const db = new Database(store.DATABASE_FILE, { readonly: true });
  try {
    const head = db.prepare('SELECT m.payload_json FROM conversations c JOIN messages m ON m.id = c.current_message_id WHERE c.id = ?').pluck().get(ids['down/a']);
    assert.equal(JSON.parse(head).stopped, true);
  } finally {
    db.close();
  }
  app.advanceBench(bench.id);
  assert.deepEqual(app.reservations.turns(), []);
  assert.equal(fs.existsSync(path.join(dataDir, 'server.pid')), false, 'the stopped server gives the data folder back');
  fs.rmSync(dataDir, { recursive: true, force: true });
});
