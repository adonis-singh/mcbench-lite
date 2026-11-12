'use strict';

// The archive of server/index.js and store.js, driven through handleApi with
// a fake model and sandbox: an archived chat or bench keeps everything for
// its 7 days, is listed with its expiry, can be restored as it was or deleted
// for good, and the purge after 7 days deletes what a delete does.

const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { EventEmitter } = require('node:events');
const { Readable } = require('node:stream');
const { after, test } = require('node:test');
const Database = require('better-sqlite3');

const dataDir = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-server-archive-'));
process.env.MCBENCH_DATA_DIR = dataDir;
process.env.MCBENCH_SETTINGS_FILE = path.join(dataDir, 'settings.json');
require('../server/assets').getBlockRegistry = async () => ({ stone: [{}, {}] });
const llm = require('../server/llm');
llm.chatCompletion = async (options) => ({ content: options.tools.length ? 'Done.' : 'A title' });
llm.modelInfo = async () => ({ imageInput: true });
const sandbox = require('../server/sandbox');
// The chats whose containers were removed.
const resets = [];
sandbox.reset = async (id) => { resets.push(id); };
const harness = require('../server/harness');
const store = require('../server/store');
const storage = require('../server/storage');
const app = require('../server/index');

after(() => fs.rmSync(dataDir, { recursive: true, force: true }));

const TXT = `data:text/plain;base64,${Buffer.from('notes').toString('base64')}`;

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

async function free(id) {
  for (let hold; (hold = app.reservations.get(id));) await hold.done;
}
const tick = () => new Promise(setImmediate);

// A chat, or a bench model's chat, with something in each of its folders: an
// attachment, an image, a block in its world, a file in /work and a harness
// session.
function fill(id) {
  const conversation = store.getConversation(id);
  conversation.world.set(0, 0, 0, 'minecraft:stone');
  store.appendMessage(conversation, { role: 'assistant', content: 'Placed a block' });
  store.save(conversation);
  store.saveImages(id, [Buffer.from('png')]);
  fs.writeFileSync(path.join(sandbox.workDir(id), 'notes.txt'), 'kept');
  harness.writeSession(id, 'claude-code', { sessionId: 'session', lastSeen: conversation.messages.at(-1).id });
}

async function newChat(text) {
  const { body: { id } } = await call('POST', '/api/conversations', { provider: 'openrouter', model: 'chat/model' });
  const sent = await call('POST', `/api/conversations/${id}/chat`, { provider: 'openrouter', model: 'chat/model', resolution: 1, text, files: [{ name: 'notes.txt', data: TXT }] });
  assert.equal(sent.lines.at(-1).type, 'done');
  fill(id);
  await tick();
  return id;
}

// A blind bench of two models, with a vote on its round.
async function newBench(text) {
  const { body: { id } } = await call('POST', '/api/benches', { text, blind: true, resolution: 1, files: [{ name: 'notes.txt', data: TXT }], members: [{ provider: 'openrouter', model: 'bench/a' }, { provider: 'openrouter', model: 'bench/b' }] });
  const members = store.benchMembers(id).map((member) => member.id);
  await Promise.all(members.map(free));
  members.forEach(fill);
  const [round] = store.getBench(id).rounds;
  const vote = await call('POST', `/api/benches/${id}/vote`, { roundId: round.id, pick: members[0] });
  assert.equal(vote.status, 200, vote.body.error);
  await tick();
  return { id, members };
}

const FOLDERS = ['worlds', 'images', 'attachments', 'sandbox', 'harness'];
const folders = (id) => FOLDERS.filter((folder) => fs.existsSync(path.join(dataDir, folder, id)));

// What the database holds of chat or bench `id`.
function rows(id) {
  const db = new Database(store.DATABASE_FILE, { readonly: true });
  try {
    return {
      chats: db.prepare('SELECT count(*) FROM conversations WHERE id = ? OR bench_id = ?').pluck().get(id, id),
      benches: db.prepare('SELECT count(*) FROM benches WHERE id = ?').pluck().get(id),
      messages: db.prepare('SELECT count(*) FROM messages WHERE conversation_id = ?').pluck().get(id),
      votes: db.prepare('SELECT count(*) FROM bench_votes WHERE bench_id = ?').pluck().get(id),
      search: db.prepare('SELECT count(*) FROM search_docs WHERE owner = ?').pluck().get(id),
    };
  } finally {
    db.close();
  }
}
const NONE = { chats: 0, benches: 0, messages: 0, votes: 0, search: 0 };

const archived = async () => (await call('GET', '/api/archive')).body;
const listed = () => store.listConversations().map((row) => row.id);

test('an archived chat keeps its files, is listed with its expiry, and is restored as it was', async () => {
  const id = await newChat('Build a hut');
  const before = store.getConversation(id);
  const transcript = store.publicMessages(before);
  const watcher = request('GET', '/api/conversations/stream');
  await tick();

  const archive = await call('POST', `/api/conversations/${id}/archive`);
  assert.equal(archive.status, 200);
  assert.ok(resets.includes(id), 'its containers are removed');
  assert.equal(listed().includes(id), false);
  assert.equal((await call('GET', `/api/conversations/${id}`)).status, 404);
  assert.deepEqual(folders(id), FOLDERS, 'nothing on disk is removed');
  const [entry] = (await archived()).conversations;
  assert.deepEqual(Object.keys(entry), ['id', 'title', 'provider', 'model', 'archivedAt', 'expiresAt']);
  assert.deepEqual([entry.id, entry.title, entry.provider, entry.model], [id, 'A title', 'openrouter', 'chat/model']);
  assert.ok(Math.abs(entry.archivedAt - Date.now()) < 5000);
  assert.equal(entry.expiresAt, entry.archivedAt + store.ARCHIVE_TTL_MS);

  const shown = watcher.lines.length;
  const restore = await call('POST', `/api/conversations/${id}/restore`);
  assert.deepEqual([restore.status, restore.body], [200, { ok: true }]);
  assert.ok(watcher.lines.length > shown && watcher.lines.at(-1).conversations.some((row) => row.id === id), 'the list is published');
  watcher.emit('close');
  assert.equal((await archived()).conversations.some((row) => row.id === id), false);
  const restored = store.getConversation(id);
  assert.notEqual(restored, before, 'it is read back from what is stored');
  assert.deepEqual(store.publicMessages(restored), transcript);
  assert.equal(restored.world.size(), 1);
  assert.equal(sandbox.readWorkFile(id, 'notes.txt', { encoding: 'utf8' }), 'kept');
  assert.equal(harness.readSession(id, 'claude-code').sessionId, 'session');
  const again = await call('POST', `/api/conversations/${id}/chat`, { provider: 'openrouter', model: 'chat/model', resolution: 1, text: 'Add a door' });
  assert.equal(again.lines.at(-1).type, 'done', 'it takes turns again');
});

test('an archived chat deleted for good leaves no row, no search entry and no folder; one not archived is refused', async () => {
  const id = await newChat('Build a well');
  const refused = await call('DELETE', `/api/conversations/${id}`);
  assert.deepEqual([refused.status, refused.body.error], [409, 'Archive it first']);
  assert.equal((await call('POST', `/api/conversations/${id}/restore`)).status, 404, 'what is not archived cannot be restored');
  assert.ok(storage.used(id) > 0);

  await call('POST', `/api/conversations/${id}/archive`);
  const deleted = await call('DELETE', `/api/conversations/${id}`);
  assert.deepEqual([deleted.status, deleted.body], [200, { ok: true }]);
  assert.deepEqual(rows(id), NONE);
  assert.deepEqual(folders(id), []);
  assert.equal(storage.used(id), 0, 'its measurement is forgotten');
  assert.equal((await archived()).conversations.some((row) => row.id === id), false);
  assert.equal((await call('DELETE', `/api/conversations/${id}`)).status, 404);
  assert.equal((await call('POST', `/api/conversations/${id}/restore`)).status, 404);
});

test('an archived bench keeps its models, rounds and votes, which leave the leaderboard until it is restored', async () => {
  const { id, members } = await newBench('A tower');
  const leaderboard = async () => (await call('GET', '/api/leaderboard')).body.entries;
  const ranked = await leaderboard();
  assert.equal(ranked.length, 2);

  assert.equal((await call('POST', `/api/benches/${id}/archive`)).status, 200);
  assert.ok(members.every((member) => resets.includes(member)), 'every model\'s containers are removed');
  assert.deepEqual(await leaderboard(), []);
  for (const member of members) assert.deepEqual(folders(member), FOLDERS);
  const { conversations, benches } = await archived();
  assert.equal(conversations.some((row) => members.includes(row.id)), false, 'its models are part of the bench');
  const [entry] = benches;
  assert.deepEqual(Object.keys(entry), ['id', 'title', 'archivedAt', 'expiresAt', 'models']);
  assert.deepEqual([entry.id, entry.models, entry.expiresAt], [id, 2, entry.archivedAt + store.ARCHIVE_TTL_MS]);
  // A model's chat goes nowhere on its own.
  for (const [method, route] of [['POST', 'restore'], ['DELETE', '']]) {
    const refused = await call(method, `/api/conversations/${members[0]}${route ? `/${route}` : ''}`);
    assert.deepEqual([refused.status, refused.body.error], [409, 'A bench chat is changed only through its bench']);
  }

  const restore = await call('POST', `/api/benches/${id}/restore`);
  assert.deepEqual([restore.status, restore.body], [200, { ok: true }]);
  assert.deepEqual(await leaderboard(), ranked, 'its votes count again');
  const bench = (await call('GET', `/api/benches/${id}`)).body;
  assert.deepEqual([bench.rounds.length, Object.keys(bench.votes).length], [1, 1]);
  assert.deepEqual(store.benchMembers(id).map((member) => [member.id, member.world.size()]), members.map((member) => [member, 1]));
  assert.equal(sandbox.readWorkFile(members[1], 'notes.txt', { encoding: 'utf8' }), 'kept');
  assert.equal((await call('POST', `/api/benches/${id}/restore`)).status, 404);
  const refused = await call('DELETE', `/api/benches/${id}`);
  assert.deepEqual([refused.status, refused.body.error], [409, 'Archive it first']);

  await call('POST', `/api/benches/${id}/archive`);
  assert.deepEqual((await call('DELETE', `/api/benches/${id}`)).body, { ok: true });
  assert.deepEqual(rows(id), NONE);
  for (const member of members) {
    assert.deepEqual(rows(member), NONE);
    assert.deepEqual(folders(member), []);
  }
  assert.deepEqual(folders(id), [], 'the bench\'s own attachments go too');
  assert.equal((await call('DELETE', `/api/benches/${id}`)).status, 404);
});

test('the archive lists the newest first, and the purge after 7 days deletes what a delete does', async () => {
  const chat = await newChat('Build a barn');
  const { id: bench, members } = await newBench('A bridge');
  await call('POST', `/api/conversations/${chat}/archive`);
  await new Promise((resolve) => setTimeout(resolve, 5));
  await call('POST', `/api/benches/${bench}/archive`);
  const later = await newChat('Build a mill');
  await new Promise((resolve) => setTimeout(resolve, 5));
  await call('POST', `/api/conversations/${later}/archive`);
  const list = await archived();
  assert.deepEqual(list.conversations.slice(0, 2).map((row) => row.id), [later, chat]);
  assert.equal(list.benches[0].id, bench);

  store.clearExpiredArchive(list.conversations[1].expiresAt - 1);
  assert.deepEqual(folders(chat), FOLDERS, 'nothing goes before its 7 days');
  store.clearExpiredArchive(list.benches[0].expiresAt + 1);
  for (const id of [chat, bench, ...members]) {
    assert.deepEqual(rows(id), NONE, id);
    assert.deepEqual(folders(id), [], id);
  }
  assert.equal(storage.used(chat), 0);
  assert.deepEqual(folders(later), FOLDERS, 'the one archived since is kept');
  const remaining = await archived();
  assert.deepEqual([remaining.conversations.map((row) => row.id), remaining.benches], [[later], []]);
});

test('the chat list stream says how many chats and benches are archived, after each archive, restore and delete', async () => {
  // The counts in a new stream's first line, and in the last line an open one was sent.
  const opened = async () => {
    const watcher = request('GET', '/api/conversations/stream');
    await tick();
    const [first] = watcher.lines;
    watcher.emit('close');
    return first.archived;
  };
  const watcher = request('GET', '/api/conversations/stream');
  await tick();
  const published = () => watcher.lines.at(-1).archived;
  const start = await opened();
  const { conversations: chats, benches } = await archived();
  assert.deepEqual(start, { conversations: chats.length, benches: benches.length });
  const counts = (conversations, benchCount) => ({ conversations: start.conversations + conversations, benches: start.benches + benchCount });

  const chat = await newChat('Build a shed');
  const { id: bench } = await newBench('A gate');
  await call('POST', `/api/conversations/${chat}/archive`);
  assert.deepEqual([await opened(), published()], [counts(1, 0), counts(1, 0)]);
  await call('POST', `/api/benches/${bench}/archive`);
  assert.deepEqual([await opened(), published()], [counts(1, 1), counts(1, 1)]);
  await call('POST', `/api/conversations/${chat}/restore`);
  assert.deepEqual([await opened(), published()], [counts(0, 1), counts(0, 1)]);
  await call('POST', `/api/benches/${bench}/restore`);
  assert.deepEqual([await opened(), published()], [counts(0, 0), counts(0, 0)]);

  await call('POST', `/api/conversations/${chat}/archive`);
  await call('POST', `/api/benches/${bench}/archive`);
  assert.deepEqual(await opened(), counts(1, 1));
  assert.deepEqual((await call('DELETE', `/api/conversations/${chat}`)).body, { ok: true });
  assert.deepEqual([await opened(), published()], [counts(0, 1), counts(0, 1)]);
  assert.deepEqual((await call('DELETE', `/api/benches/${bench}`)).body, { ok: true });
  assert.deepEqual([await opened(), published()], [counts(0, 0), counts(0, 0)]);
  watcher.emit('close');
});
