'use strict';

// The storage limits at their write points (server/storage.js): attachments,
// images, world saves and test_build reports, with a 1 MiB chat budget and
// three reports kept.

const assert = require('node:assert/strict');
const crypto = require('node:crypto');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { after, test } = require('node:test');

const dataDir = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-storage-limits-'));
process.env.MCBENCH_DATA_DIR = dataDir;
process.env.MCBENCH_CHAT_MAX_BYTES = String(1024 * 1024);
process.env.MCBENCH_REPORTS_KEPT = '3';
require('./helpers/fake-engines').installFakeEngines();
const store = require('../server/store');
const sandbox = require('../server/sandbox');
const storage = require('../server/storage');
const { _internal: { executeBuild, executeTestBuild } } = require('../server/agent');

after(async () => {
  await require('../server/shot-world').stopShotEngines();
  store.close();
  fs.rmSync(dataDir, { recursive: true, force: true });
});

const KiB = 1024;
const full = (error) => error.statusCode === 507 && /This chat's storage is full: .*MCBENCH_CHAT_MAX_BYTES/.test(error.message);
const chat = () => store.createConversation('openrouter', 'test/model');
const file = (name, bytes) => ({ name, data: `data:application/octet-stream;base64,${Buffer.alloc(bytes, 7).toString('base64')}` });
const listing = (...parts) => (fs.existsSync(path.join(dataDir, ...parts)) ? fs.readdirSync(path.join(dataDir, ...parts)) : []);
const registry = { stone: [{}, {}], gold_block: [{}, {}] };

test('attachments that do not fit the chat\'s budget are refused with 507 and none is written', () => {
  const conversation = chat();
  store.saveAttachments(conversation.id, [file('first.bin', 400 * KiB)]);
  assert.throws(() => store.saveAttachments(conversation.id, [file('second.bin', 300 * KiB), file('third.bin', 400 * KiB)]), full);
  assert.equal(listing('attachments', conversation.id).length, 1);
  store.saveAttachments(conversation.id, [file('fits.bin', 300 * KiB)]);
  assert.equal(listing('attachments', conversation.id).length, 2);
});

test('a bench round\'s attachment is refused for a chat it does not fit', () => {
  // A round's files are stored under its bench's id before the bench is written.
  const [attachment] = store.saveAttachments(crypto.randomUUID(), [file('round.bin', 700 * KiB)]);
  const member = chat();
  store.saveAttachments(member.id, [file('own.bin', 400 * KiB)]);
  assert.throws(() => store.shareAttachment(attachment, member.id), full);
  assert.equal(listing('attachments', member.id).length, 1);
  assert.equal(store.shareAttachment(attachment, chat().id).size, 700 * KiB);
});

test('images that do not fit are refused before any is written', () => {
  const conversation = chat();
  const [url] = store.saveImages(conversation.id, [crypto.randomBytes(500 * KiB)]);
  assert.match(url, new RegExp(`^/api/images/${conversation.id}/.*\\.png$`));
  assert.throws(() => store.saveImages(conversation.id, [crypto.randomBytes(200 * KiB), crypto.randomBytes(400 * KiB)]), full);
  assert.equal(listing('images', conversation.id).length, 1);
});

test('a build whose world does not fit fails as the build\'s result and changes nothing', async () => {
  const conversation = chat();
  const before = conversation.world;
  // 36 dense chunks, each different (chunk files are named by content): about 1.3 MiB.
  const big = await executeBuild(conversation, { program: 'module.exports = mc => { mc.fill([0,0,0],[95,15,95],"stone"); for (let i = 0; i < 36; i++) mc.set([(i % 6) * 16, i % 16, Math.floor(i / 6) * 16 + Math.floor(i / 16)], "gold_block"); }' }, registry);
  assert.equal(big.ok, false);
  assert.match(big.error, /This chat's storage is full/);
  assert.equal(big.note, 'program aborted — no blocks or files were changed');
  assert.equal(conversation.world, before);
  assert.deepEqual(listing('worlds', conversation.id, 'chunks'), [], 'the chunks written before the refusal are removed');
  assert.ok(storage.used(conversation.id) < 64 * KiB, 'and refunded');

  const files = await executeBuild(conversation, { program: 'module.exports = mc => { mc.set([0,0,0],"stone"); mc.writeFile("big.txt", "x".repeat(2 * 1024 * 1024)); }' }, registry);
  assert.equal(files.ok, false);
  assert.match(files.error, /this write needs 2\.0 MiB/);
  assert.equal(sandbox.workFileExists(conversation.id, 'big.txt'), false);
  assert.equal(conversation.world, before);

  const small = await executeBuild(conversation, { program: 'module.exports = mc => mc.fill([0,0,0],[15,15,15],"stone")' }, registry);
  assert.equal(small.ok, true);
  assert.equal(listing('worlds', conversation.id, 'chunks').length, 1, 'a build that fits is written before it is applied');
  store.appendMessage(conversation, { role: 'user', content: 'saved' });
  store.save(conversation);
  assert.equal(listing('worlds', conversation.id, 'chunks').length, 1, 'and its save writes no chunk again');
});

function verifier(bytes, runs) {
  return {
    run: async () => {
      runs.push(1);
      const reportFile = path.join(dataDir, 'verifier-reports', `${crypto.randomUUID()}.json`);
      fs.mkdirSync(path.dirname(reportFile), { recursive: true });
      fs.writeFileSync(reportFile, JSON.stringify({ padding: 'x'.repeat(bytes) }));
      return { ok: true, reportFile, probes: [] };
    },
  };
}

test('a test_build report that does not fit fails its test and is not kept', async () => {
  const conversation = chat();
  const runs = [];
  const result = await executeTestBuild(conversation, { tests: [{}, {}] }, undefined, verifier(700 * KiB, runs));
  assert.equal(runs.length, 2);
  assert.equal(result.tests.filter((test) => test.ok).length, 1);
  assert.match(result.tests.find((test) => !test.ok).error, /This chat's storage is full/);
  assert.equal(listing('sandbox', conversation.id, 'verification').length, 1);
  assert.deepEqual(listing('verifier-reports'), [], 'the verifier\'s archive is removed either way');
});

test('a chat over its budget runs no test_build experiments', async () => {
  const conversation = chat();
  fs.writeFileSync(path.join(sandbox.workDir(conversation.id), 'filled.bin'), Buffer.alloc(1100 * KiB));
  await storage.measure(conversation.id);
  const runs = [];
  const result = await executeTestBuild(conversation, { tests: [{}] }, undefined, verifier(10, runs));
  assert.equal(result.ok, false);
  assert.match(result.error, /This chat's storage is full: .* hold 1\.\d MiB of its 1\.0 MiB budget/);
  assert.equal(runs.length, 0);
});

test('a test_build call prunes the oldest reports past the cap, never one a stored message names', async (t) => {
  const conversation = chat();
  const dir = path.join(sandbox.workDir(conversation.id), 'verification');
  fs.mkdirSync(dir);
  const names = Array.from({ length: 5 }, () => `${crypto.randomUUID()}.json`);
  names.forEach((name, index) => {
    fs.writeFileSync(path.join(dir, name), '{}');
    fs.utimesSync(path.join(dir, name), 1000 + index, 1000 + index);
  });
  fs.writeFileSync(path.join(dir, 'notes.json'), '{}');
  // The oldest report is named by a saved tool result.
  store.appendMessage(conversation, { role: 'user', content: 'test it' });
  store.appendMessage(conversation, { role: 'tool', tool_call_id: 'call', content: JSON.stringify({ ok: true, tests: [{ report: `/work/verification/${names[0]}` }] }) });
  store.save(conversation);
  const removed = [];
  t.mock.method(sandbox, 'removeWorkFiles', async (id, relPaths) => {
    assert.equal(id, conversation.id);
    removed.push(...relPaths);
    for (const relPath of relPaths) fs.rmSync(path.join(sandbox.workDir(id), relPath));
  });
  const result = await executeTestBuild(conversation, { tests: [{}] }, undefined, verifier(10, []));
  assert.equal(result.ok, true);
  assert.deepEqual(removed, [`verification/${names[1]}`, `verification/${names[2]}`]);
  assert.deepEqual(fs.readdirSync(dir).sort(), [names[0], names[3], names[4], 'notes.json', path.basename(result.tests[0].report)].sort());
});

test('under the disk\'s reserve attachments, images, world saves and test_build are refused', async (t) => {
  const conversation = chat();
  t.mock.method(fs, 'statfsSync', () => ({ bavail: 100, bsize: 4096 }));
  storage._internal.dropReading();
  const disk = (error) => error.statusCode === 507 && /MCBENCH_DISK_RESERVE_BYTES/.test(error.message);
  assert.throws(() => store.saveAttachments(conversation.id, [file('small.bin', 10)]), disk);
  assert.throws(() => store.saveImages(conversation.id, [Buffer.from('png')]), disk);
  const candidate = conversation.world.clone();
  candidate.set(0, 0, 0, 'minecraft:stone');
  candidate.finalize();
  assert.throws(() => store.persistWorld(conversation.id, candidate), disk);
  assert.equal([...candidate.chunks.values()][0].persistedHash, null);
  const result = await executeTestBuild(conversation, { tests: [{}] }, undefined, verifier(10, []));
  assert.match(result.error, /MCBENCH_DISK_RESERVE_BYTES/);
  t.mock.restoreAll();
  storage._internal.dropReading();
  store.persistWorld(conversation.id, candidate);
  assert.ok([...candidate.chunks.values()][0].persistedHash);
});
