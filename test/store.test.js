'use strict';

const assert = require('node:assert/strict');
const crypto = require('node:crypto');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { after, test } = require('node:test');
const Database = require('better-sqlite3');

const dataDir = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-store-'));
process.env.MCBENCH_DATA_DIR = dataDir;

let store = require('../server/store');
const sandbox = require('../server/sandbox');

after(() => {
  try { store.close(); } catch { /* already closed */ }
  fs.rmSync(dataDir, { recursive: true, force: true });
  delete process.env.MCBENCH_DATA_DIR;
});

function reopenStore() {
  store.close();
  delete require.cache[require.resolve('../server/store')];
  store = require('../server/store');
}

function sql() {
  return new Database(store.DATABASE_FILE, { readonly: true });
}

test('a fresh database is created at the current schema version', () => {
  const db = sql();
  try {
    assert.equal(db.pragma('user_version', { simple: true }), 13);
    assert.equal(db.pragma('auto_vacuum', { simple: true }), 2);
    assert.deepEqual(store.listConversations(), []);
  } finally {
    db.close();
  }
});

test('generated title survives a new chat and reload before the running turn is saved', () => {
  const conversation = store.createConversation('openrouter', 'test/model');
  store.appendMessage(conversation, { role: 'user', content: 'Build a clock' });
  store.save(conversation);
  const pending = store.appendMessage(conversation, { role: 'assistant', content: 'Working...' });

  store.setTitle(conversation, 'Redstone Clock');
  store.createConversation('openrouter', 'test/model');
  assert.equal(store.listConversations().find((entry) => entry.id === conversation.id).title, 'Redstone Clock');
  assert.equal(conversation.title, 'Redstone Clock');
  assert.ok(conversation.unsaved.has(pending.id));

  reopenStore();
  const restored = store.getConversation(conversation.id);
  assert.equal(restored.title, 'Redstone Clock');
  assert.equal(restored.messages.length, 1, 'updating the title must not save unfinished turn messages');
});

test('a chat is unread from the end of a turn until it is seen', () => {
  const conversation = store.createConversation('openrouter', 'test/model');
  conversation.effort = 'high';
  store.appendMessage(conversation, { role: 'user', content: 'Build a clock' });
  store.save(conversation);
  const row = () => store.listConversations().find((entry) => entry.id === conversation.id);
  assert.equal(row().unread, false);
  assert.equal(row().effort, 'high');
  store.markFinished(conversation.id);
  assert.equal(row().unread, true);
  store.markSeen(conversation.id);
  assert.equal(row().unread, false);
  store.save(conversation);
  assert.equal(row().unread, false, 'saving a conversation keeps its seen state');
});

test('title updates do not restore archived conversations', () => {
  const conversation = store.createConversation('openrouter', 'test/model');
  store.archiveConversation(conversation.id);
  assert.throws(() => store.setTitle(conversation, 'Late title'), /not found or archived/);
  assert.equal(conversation.title, 'New build');
  assert.ok(!store.listConversations().some((entry) => entry.id === conversation.id));
});

test('editing the first prompt keeps the title, and only an unchanged resend repeats it', () => {
  const conversation = store.createConversation('openrouter', 'test/model');
  assert.equal(store.repeatsLatestRootPrompt(conversation, 'Build a clock'), false);
  const image = (bytes) => ({ type: 'attachment', attachment: store.saveAttachment(conversation.id, { name: 'ref.png', data: `data:image/png;base64,${Buffer.from(bytes).toString('base64')}` }) });
  const first = store.appendMessage(conversation, { role: 'user', content: [{ type: 'text', text: 'Build a clock' }, image('png-a')] }, conversation.world.reference());
  store.save(conversation);
  store.setTitle(conversation, 'Redstone Clock');

  store.revert(conversation, first.id);
  assert.equal(conversation.title, 'Redstone Clock');
  assert.equal(store.repeatsLatestRootPrompt(conversation, [{ type: 'text', text: 'Build a clock' }, image('png-a')]), true,
    'the same text and attachment bytes under a newly stored name are a resend');
  assert.equal(store.repeatsLatestRootPrompt(conversation, [{ type: 'text', text: 'Build a clock tower' }, image('png-a')]), false);
  assert.equal(store.repeatsLatestRootPrompt(conversation, 'Build a clock'), false, 'dropping the attachment is an edit');
  assert.equal(store.repeatsLatestRootPrompt(conversation, [{ type: 'text', text: 'Build a clock' }, image('png-b')]), false,
    'an attachment with the same name but different bytes is an edit');

  store.appendMessage(conversation, { role: 'user', content: 'Build a clock tower' }, conversation.world.reference());
  store.save(conversation);
  assert.equal(store.repeatsLatestRootPrompt(conversation, 'Build a clock tower'), true, 'the newest root prompt is the comparison');
  assert.equal(store.repeatsLatestRootPrompt(conversation, [{ type: 'text', text: 'Build a clock' }, image('png-a')]), false);
});

test('persists stable message branches and world checkpoints across restarts', () => {
  const conversation = store.createConversation('openrouter', 'test/model');
  const user1 = store.appendMessage(conversation, { role: 'user', content: 'first' }, conversation.world.reference());

  let world = conversation.world.clone();
  world.set(0, 0, 0, 'minecraft:stone', {});
  conversation.world = world.finalize();
  const assistant1 = store.appendMessage(conversation, { role: 'assistant', content: 'stone' });
  const user2 = store.appendMessage(conversation, { role: 'user', content: 'second' }, conversation.world.reference());

  world = conversation.world.clone();
  world.set(0, 0, 0, 'minecraft:oak_planks', {});
  conversation.world = world.finalize();
  store.appendMessage(conversation, {
    role: 'assistant',
    content: 'oak',
    reasoning_details: [{ type: 'signed', data: 'opaque' }],
  });
  const user3 = store.appendMessage(conversation, { role: 'user', content: 'third' }, conversation.world.reference());

  world = conversation.world.clone();
  world.set(0, 0, 0, 'minecraft:gold_block', {});
  conversation.world = world.finalize();
  const oldHead = store.appendMessage(conversation, { role: 'assistant', content: 'gold' });
  store.save(conversation);

  const db = sql();
  const user3Checkpoint = JSON.parse(db.prepare('SELECT checkpoint_manifest_json FROM messages WHERE id = ?').get(user3.id).checkpoint_manifest_json);
  const inactiveHash = user3Checkpoint.chunks[0][1];
  const oldBranchWorld = JSON.parse(db.prepare('SELECT world_manifest_json FROM messages WHERE id = ?').get(oldHead.id).world_manifest_json);
  const oldBranchHash = oldBranchWorld.chunks[0][1];
  const originalCount = db.prepare('SELECT COUNT(*) AS count FROM messages WHERE conversation_id = ?').get(conversation.id).count;
  const maxPosition = db.prepare('SELECT MAX(position) AS position FROM messages WHERE conversation_id = ?').get(conversation.id).position;
  db.close();

  reopenStore();
  const restored = store.getConversation(conversation.id);
  assert.equal(restored.messages[0].id, user1.id);
  assert.equal(restored.messages[2].id, user2.id);
  assert.equal(restored.world.get(0, 0, 0).id, 'minecraft:gold_block');
  assert.equal(restored.checkpoints.get(user2.id).get(0, 0, 0).id, 'minecraft:stone');
  assert.equal(restored.messages[3].reasoning_details[0].data, 'opaque');

  const removed = store.revert(restored, user2.id);
  assert.equal(removed.content, 'second');
  assert.deepEqual(restored.messages.map((message) => message.id), [user1.id, assistant1.id]);
  assert.equal(restored.currentMessageId, assistant1.id);
  assert.equal(restored.world.get(0, 0, 0).id, 'minecraft:stone');
  assert.ok(fs.existsSync(path.join(dataDir, 'worlds', conversation.id, 'chunks', `${inactiveHash}.json`)));
  assert.ok(fs.existsSync(path.join(dataDir, 'worlds', conversation.id, 'chunks', `${oldBranchHash}.json`)));

  const edited = store.appendMessage(restored, { role: 'user', content: 'edited second' }, restored.world.reference());
  store.save(restored);
  const publicMessage = store.publicMessages(restored).at(-1);
  assert.equal(publicMessage.id, edited.id);
  assert.equal(publicMessage.parentId, undefined);
  assert.equal(publicMessage.position, undefined);

  const branchDb = sql();
  try {
    const row = branchDb.prepare('SELECT parent_id, position FROM messages WHERE id = ?').get(edited.id);
    assert.equal(row.parent_id, assistant1.id);
    assert.ok(row.position > maxPosition);
    assert.equal(branchDb.prepare('SELECT COUNT(*) AS count FROM messages WHERE conversation_id = ?').get(conversation.id).count, originalCount + 1);
  } finally {
    branchDb.close();
  }

  const tree = store.messageTree(restored);
  assert.equal(tree.headId, edited.id);
  const fork = tree.messages
    .filter((message) => message.parentId === assistant1.id && message.branchable)
    .map((message) => message.id);
  assert.deepEqual(fork, [user2.id, edited.id]);

  store.activateBranch(restored, user2.id);
  assert.equal(restored.currentMessageId, oldHead.id);
  assert.equal(restored.messages.at(-1).content, 'gold');
  assert.equal(restored.world.get(0, 0, 0).id, 'minecraft:gold_block');

  reopenStore();
  const switched = store.getConversation(conversation.id);
  assert.equal(switched.currentMessageId, oldHead.id);
  store.activateBranch(switched, edited.id);
  assert.equal(switched.currentMessageId, edited.id);
  assert.equal(switched.messages.at(-1).content, 'edited second');
  assert.equal(switched.world.get(0, 0, 0).id, 'minecraft:stone');
  assert.equal(store.messageTree(switched).headId, edited.id);
});

test('a failed save cleans its new chunks and drops the chat, so memory is never ahead of the database', () => {
  const stale = store.createConversation('openrouter', 'test/rollback');
  store.appendMessage(stale, { role: 'user', content: 'retry' }, stale.world.reference());
  const world = stale.world.clone();
  world.set(0, 0, 0, 'minecraft:diamond_block', {});
  stale.world = world.finalize();
  stale.currentMessageId = 'invalid-head';

  assert.throws(() => store.save(stale), /invalid active head/);
  const chunks = path.join(dataDir, 'worlds', stale.id, 'chunks');
  assert.deepEqual(fs.existsSync(chunks) ? fs.readdirSync(chunks) : [], []);
  // What is served next is what is stored: the unsaved message is gone, and
  // the dropped object can no longer be saved over it.
  const conversation = store.getConversation(stale.id);
  assert.notEqual(conversation, stale);
  assert.deepEqual(conversation.messages, []);
  assert.equal(conversation.world.size(), 0);
  assert.throws(() => store.save(stale), /no longer loaded/);

  const message = store.appendMessage(conversation, { role: 'user', content: 'retry' }, conversation.world.reference());
  conversation.world = world;
  assert.equal(conversation.currentMessageId, message.id);
  store.save(conversation);
  assert.equal(fs.readdirSync(chunks).filter((name) => name.endsWith('.json')).length, 1);
  const db = sql();
  const manifest = JSON.parse(db.prepare('SELECT world_manifest_json FROM conversations WHERE id = ?').get(conversation.id).world_manifest_json);
  db.close();
  assert.equal(fs.existsSync(path.join(chunks, `${manifest.chunks[0][1]}.json`)), true);
});

test('archives in SQLite and removes artifacts only after the retention period', () => {
  const conversation = store.createConversation('openrouter', 'test/archive');
  store.appendMessage(conversation, { role: 'user', content: 'archive me' }, conversation.world.reference());
  store.save(conversation);

  const worldDir = path.join(dataDir, 'worlds', conversation.id);
  const imageDir = path.join(store.IMAGES_DIR, conversation.id);
  const attachmentDir = path.join(store.ATTACHMENTS_DIR, conversation.id);
  for (const dir of [worldDir, imageDir, attachmentDir]) {
    fs.mkdirSync(dir, { recursive: true });
    fs.writeFileSync(path.join(dir, 'keep'), 'x');
  }

  store.archiveConversation(conversation.id);
  const archiveDb = sql();
  const archived = archiveDb.prepare('SELECT archived_at FROM conversations WHERE id = ?').get(conversation.id).archived_at;
  archiveDb.close();
  assert.equal(store.getConversation(conversation.id), null);
  assert.equal(store.listConversations().some((entry) => entry.id === conversation.id), false);
  store.clearExpiredArchive(archived + store.ARCHIVE_TTL_MS - 1);
  assert.equal(fs.existsSync(worldDir), true);

  store.clearExpiredArchive(archived + store.ARCHIVE_TTL_MS + 1);
  assert.equal(fs.existsSync(worldDir), false);
  assert.equal(fs.existsSync(imageDir), false);
  assert.equal(fs.existsSync(attachmentDir), false);
  const db = sql();
  try {
    assert.equal(db.prepare('SELECT COUNT(*) AS count FROM conversations WHERE id = ?').get(conversation.id).count, 0);
  } finally {
    db.close();
  }
});

test('attachments preserve metadata and bytes for the sandbox bridge', () => {
  const conversationId = `test-${Date.now()}`;
  const payload = Buffer.from('hello attachment');
  const attachment = store.saveAttachment(conversationId, {
    name: 'reference notes.md',
    data: `data:text/markdown;base64,${payload.toString('base64')}`,
  });

  assert.equal(attachment.name, 'reference notes.md');
  assert.equal(attachment.type, 'text/markdown');
  assert.equal(attachment.size, payload.length);
  assert.match(attachment.path, /^\/work\/attachments\/[a-f0-9]{8}-reference-notes\.md$/);
  assert.deepEqual(store.readAttachment(attachment.url), payload);
  sandbox.syncAttachments(conversationId, store.ATTACHMENTS_DIR);
  assert.deepEqual(fs.readFileSync(sandbox.resolveWorkPath(conversationId, attachment.path)), payload);
});

test('100 MiB attachments survive storage and sandbox sync; larger files are rejected', () => {
  const id = `large-upload-${Date.now()}`;
  const payload = Buffer.alloc(100 * 1024 * 1024, 0x5a);
  const attachment = store.saveAttachment(id, { name: 'reference.mp4', data: `data:video/mp4;base64,${payload.toString('base64')}` });
  assert.equal(attachment.size, payload.length);
  sandbox.syncAttachments(id, store.ATTACHMENTS_DIR);
  const hash = bytes => crypto.createHash('sha256').update(bytes).digest('hex');
  assert.equal(hash(fs.readFileSync(sandbox.resolveWorkPath(id, attachment.path))), hash(payload));
  assert.throws(() => store.saveAttachment(id, { name: 'too-large.mp4', data: `data:video/mp4;base64,${Buffer.alloc(payload.length + 1).toString('base64')}` }), /exceeds 100 MB/);
});

// A message whose attachments are refused used to leave the ones before the
// refused one on disk.
test('a refused message writes none of its attachments', () => {
  const id = `refused-upload-${Date.now()}`;
  const file = (name, bytes) => ({ name, data: `data:application/octet-stream;base64,${Buffer.alloc(bytes, 1).toString('base64')}` });
  const dir = path.join(store.ATTACHMENTS_DIR, id);
  assert.throws(() => store.saveAttachments(id, [file('a.bin', 4), { name: 'b.bin', data: 'https://example.com/b' }]),
    (error) => error.statusCode === 400 && /not base64 data/.test(error.message));
  assert.throws(() => store.saveAttachments(id, [file('a.bin', 70 * 1024 * 1024), file('b.bin', 70 * 1024 * 1024), file('c.bin', 70 * 1024 * 1024)]),
    (error) => error.statusCode === 400 && /exceed 200 MB total/.test(error.message));
  assert.equal(fs.existsSync(dir), false);
  assert.equal(store.saveAttachments(id, [file('a.bin', 4), file('b.bin', 5)]).length, 2);
  assert.equal(fs.readdirSync(dir).length, 2);
});

test('the data folder is readable by its owner alone', { skip: process.platform === 'win32' }, () => {
  assert.equal(fs.statSync(dataDir).mode & 0o777, 0o700);
});

test('a chat joins the list only once it has a message', () => {
  const conversation = store.createConversation('openrouter', 'test/model');
  assert.equal(store.listConversations().some((entry) => entry.id === conversation.id), false);
  store.appendMessage(conversation, { role: 'user', content: 'Build a well' });
  store.save(conversation);
  assert.equal(store.listConversations().some((entry) => entry.id === conversation.id), true);
});

const newBench = (title, content, models, { blind = false } = {}) => store.createBench(crypto.randomUUID(), {
  title, blind, content, members: models.map((model) => (typeof model === 'string' ? { provider: 'openrouter', model, effort: null } : { provider: 'openrouter', ...model })),
});

test('a bench keeps its rounds and runs each model as a chat outside the build list', () => {
  const { bench, members: [first, second, third] } = newBench('New bench', 'A lighthouse', ['a/model', { model: 'b/model', effort: 'high' }, 'c/model']);
  assert.deepEqual(bench.order, [first.id, second.id, third.id]);
  for (const member of [first, second, third]) {
    store.appendMessage(member, { role: 'user', content: 'A lighthouse', round: bench.rounds[0].id }, member.world.reference());
    store.save(member);
  }
  store.appendMessage(first, { role: 'assistant', content: 'Built it' });
  store.save(first);
  store.appendMessage(second, { role: 'error', content: 'Provider failed' });
  store.save(second);
  store.appendMessage(third, { role: 'assistant', content: '(stopped by user)', stopped: true });
  store.save(third);
  store.setBenchTitle(bench, 'Stormy lighthouse');
  store.appendRound(bench, 'Add a keeper');

  reopenStore();
  assert.deepEqual(store.getBench(bench.id).rounds.map((round) => round.content), ['A lighthouse', 'Add a keeper']);
  assert.deepEqual(store.listBenches().find((entry) => entry.id === bench.id), {
    id: bench.id, title: 'Stormy lighthouse', createdAt: bench.createdAt, head: bench.head, rounds: 2, blind: false, revealed: false, resolution: 1, order: [first.id, second.id, third.id],
  });
  const rows = store.listConversations().filter((row) => row.benchId === bench.id).sort((a, b) => a.benchSlot - b.benchSlot);
  assert.deepEqual(rows.map((row) => [row.model, row.benchSlot, row.title]), [['a/model', 0, 'Stormy lighthouse'], ['b/model', 1, 'Stormy lighthouse'], ['c/model', 2, 'Stormy lighthouse']]);
  assert.deepEqual(store.benchProgress(first.id), { round: 1, halted: null });
  assert.deepEqual(store.benchProgress(second.id), { round: 1, halted: 'failed' });
  assert.deepEqual(store.benchProgress(third.id), { round: 1, halted: 'stopped' });
  assert.deepEqual(store.benchMembers(bench.id).map((member) => [member.model, member.effort]), [['a/model', null], ['b/model', 'high'], ['c/model', null]]);

  assert.deepEqual(store.archiveBench(bench.id).sort(), [first.id, second.id, third.id].sort());
  assert.equal(store.getBench(bench.id), null);
  assert.equal(store.getConversation(first.id), null);
});

test('a forked bench chat continues as an ordinary chat with its own copy of the files', () => {
  const { members: [source] } = newBench('Tower', 'Build a tower', ['test/model']);
  const attachment = store.saveAttachment(source.id, { name: 'ref.png', data: `data:image/png;base64,${Buffer.from('png').toString('base64')}` });
  store.appendMessage(source, { role: 'user', content: [{ type: 'text', text: 'Build a tower' }, { type: 'attachment', attachment }] }, source.world.reference());
  source.world.set(0, 0, 0, 'minecraft:stone');
  store.appendMessage(source, { role: 'assistant', content: 'Built' });
  const roundTwo = store.appendMessage(source, { role: 'user', content: 'Make it taller' }, source.world.reference());
  source.world.set(0, 1, 0, 'minecraft:stone');
  store.save(source);

  const fork = store.forkConversation(source, source.messages.indexOf(roundTwo), source.checkpoints.get(roundTwo.id));
  reopenStore();
  const restored = store.getConversation(fork.id);
  assert.equal(restored.benchId, null);
  assert.equal(restored.title, 'Tower');
  assert.deepEqual(restored.messages.map((message) => message.role), ['user', 'assistant']);
  assert.equal(restored.world.size(), 1, 'the fork has the world as round one left it');
  const copied = restored.messages[0].content[1].attachment;
  assert.equal(copied.url, attachment.url.replace(source.id, fork.id));
  assert.equal(store.readAttachment(copied.url).toString(), 'png');
  assert.ok(store.listConversations().some((row) => row.id === fork.id && row.benchId === null));
});

test('search finds chats in Build and benches in Bench by what was said', () => {
  const chat = store.createConversation('openrouter', 'test/model');
  store.appendMessage(chat, { role: 'user', content: 'Build a lighthouse on a cliff' }, chat.world.reference());
  store.appendMessage(chat, { role: 'assistant', content: 'Added a spiral staircase inside the tower.' });
  store.save(chat);
  store.setTitle(chat, 'Coastal beacon');

  const { bench, members: [member] } = newBench('Castle duel', 'A castle with a drawbridge', ['a/model']);
  store.appendMessage(member, { role: 'user', content: 'A castle with a drawbridge' }, member.world.reference());
  store.appendMessage(member, { role: 'assistant', content: 'The portcullis now has chains.' });
  store.save(member);

  const ids = (scope, query) => store.search(scope, query).map((result) => result.id);
  assert.deepEqual(ids('build', 'staircase'), [chat.id]);
  assert.deepEqual(ids('build', 'beacon'), [chat.id], 'titles are searchable after a rename');
  assert.deepEqual(ids('build', 'portcullis'), [], 'bench chats stay out of Build');
  assert.deepEqual(ids('bench', 'portcullis'), [bench.id], 'a model reply finds its bench');
  assert.deepEqual(ids('bench', 'drawbr'), [bench.id], 'words match by prefix');
  assert.deepEqual(ids('bench', 'staircase'), []);
  assert.equal(store.search('build', 'spiral staircase')[0].snippet, 'Added a \u0001spiral\u0002 \u0001staircase\u0002 inside the tower.');
  assert.deepEqual(store.search('build', '"*('), []);
  // A query's cost grows with the square of its terms, and SQLite holds the
  // server while it runs: repeats count once, and 17 different words are refused.
  const started = performance.now();
  assert.deepEqual(ids('build', 'staircase '.repeat(4000)), [chat.id]);
  assert.ok(performance.now() - started < 500, `a repeated word took ${performance.now() - started} ms`);
  assert.throws(() => store.search('build', 'a b c d e f g h i j k l m n o p q'), (error) => error.statusCode === 400 && /at most 16 different words/.test(error.message));

  store.archiveConversation(chat.id);
  store.archiveBench(bench.id);
  assert.deepEqual(ids('build', 'staircase'), []);
  assert.deepEqual(ids('bench', 'portcullis'), []);
});

test('an edited bench round branches, and each model follows the branch it answered', () => {
  const { bench, members: [member] } = newBench('Branches', 'A tower', ['a/model']);
  const turn = (round, block) => {
    store.appendMessage(member, { role: 'user', content: round.content, round: round.id }, member.world.reference());
    member.world.set(0, member.world.size(), 0, block);
    store.appendMessage(member, { role: 'assistant', content: `Built ${block}` });
    store.save(member);
  };
  const [first] = bench.rounds;
  turn(first, 'minecraft:stone');
  store.appendRound(bench, 'Add a roof');
  const roof = bench.rounds[1];
  turn(roof, 'minecraft:oak_planks');

  // Editing the second round: back to before it, then a new version.
  assert.deepEqual(store.revertBenchRound(bench, 1), [member]);
  assert.deepEqual(bench.rounds.map((round) => round.content), ['A tower']);
  store.appendRound(bench, 'Add a spire');
  const spire = bench.rounds[1];
  turn(spire, 'minecraft:cobblestone');
  assert.deepEqual(spire.branch, { index: 1, count: 2, previousId: roof.id, nextId: null });

  store.followBenchBranch(bench, roof.id);
  assert.deepEqual(bench.rounds.map((round) => round.content), ['A tower', 'Add a roof']);
  assert.equal(member.world.get(0, 1, 0).id, 'minecraft:oak_planks');
  assert.equal(member.messages.at(-1).content, 'Built minecraft:oak_planks');

  store.followBenchBranch(bench, spire.id);
  reopenStore();
  const reopened = store.getConversation(member.id);
  assert.equal(reopened.world.get(0, 1, 0).id, 'minecraft:cobblestone');
  assert.deepEqual(reopened.messages.filter((message) => message.role === 'user').map((message) => message.content), ['A tower', 'Add a spire']);
  assert.deepEqual(store.listBenches().find((entry) => entry.id === bench.id).rounds, 2);
});

test('blind votes rank the models; a bench that shows its names takes no votes', () => {
  const { bench, members: [a, b, c] } = newBench('Votes', 'A castle', ['a/model', 'b/model', { model: 'b/model', effort: 'high' }], { blind: true });
  const first = bench.rounds[0].id;
  store.appendRound(bench, 'Add a moat');
  const second = bench.rounds[1].id;
  store.castVote(bench, first, a.id, [a.id, b.id, c.id]);
  store.castVote(bench, second, null, [a.id, b.id, c.id]);
  assert.deepEqual(store.getBench(bench.id).votes, { [first]: { pick: a.id, blind: true }, [second]: { pick: null, blind: true } });
  assert.deepEqual([...store.getBench(bench.id).order].sort(), [a.id, b.id, c.id].sort());

  let board = store.leaderboard();
  assert.deepEqual(board.map((entry) => [entry.model, entry.effort, entry.wins, entry.votes, entry.benches]), [
    ['a/model', null, 1, 2, 1], ['b/model', null, 0, 2, 1], ['b/model', 'high', 0, 2, 1],
  ]);
  assert.ok(board[0].rating > 1000 && board[1].rating < 1000);

  store.revealBench(bench);
  assert.throws(() => store.castVote(bench, second, b.id, [a.id, b.id, c.id]), /shows its models' names: it takes no votes/);
  assert.deepEqual(store.getBench(bench.id).votes[second], { pick: null, blind: true }, 'the vote cast blind stays');
  assert.deepEqual(store.leaderboard().map((entry) => entry.votes), [2, 2, 2]);
  const open = newBench('Open', 'A hut', ['a/model', 'b/model']);
  assert.throws(() => store.castVote(open.bench, open.bench.rounds[0].id, open.members[0].id, open.members.map((member) => member.id)), /takes no votes/);
  store.clearVote(bench, first);
  assert.deepEqual(store.leaderboard().map((entry) => [entry.wins, entry.votes]), [[0, 1], [0, 1], [0, 1]], 'the tie is the one vote left');
  assert.equal(store.voteCount(), 1);
  store.clearAllVotes();
  assert.equal(store.voteCount(), 0);
  assert.deepEqual(store.getBench(bench.id).votes, {});
});

test('startup removes chunk files no stored world names and folders of deleted chats', () => {
  const conversation = store.createConversation('openrouter', 'test/model');
  conversation.world.set(0, 0, 0, 'minecraft:stone');
  conversation.world.finalize();
  store.save(conversation);
  const chunks = path.join(dataDir, 'worlds', conversation.id, 'chunks');
  const kept = fs.readdirSync(chunks);
  assert.equal(kept.length, 1);
  fs.writeFileSync(path.join(chunks, 'ffffffffffffffffffffffff.json'), '[]');
  fs.mkdirSync(path.join(dataDir, 'worlds', 'deleted-chat', 'chunks'), { recursive: true });
  store.collectChunks();
  assert.deepEqual(fs.readdirSync(chunks), kept);
  assert.equal(fs.existsSync(path.join(dataDir, 'worlds', 'deleted-chat')), false);
  assert.equal(store.getConversation(conversation.id).world.get(0, 0, 0).id, 'minecraft:stone');
});

test('a bench is written whole or not at all', () => {
  const before = store.listBenches().length;
  const members = [{ provider: 'openrouter', model: 'a/model', effort: null }, { provider: null, model: 'b/model', effort: null }];
  assert.throws(() => store.createBench(crypto.randomUUID(), { title: 'Broken', blind: false, content: 'A hut', members }), /provider and model are required/);
  members[1].provider = 'openrouter';
  const id = crypto.randomUUID();
  const db = new Database(store.DATABASE_FILE);
  // The second chat's insert fails inside the bench's transaction.
  db.exec(`CREATE TRIGGER refuse_second BEFORE INSERT ON conversations WHEN NEW.bench_slot = 1 BEGIN SELECT RAISE(ABORT, 'refused'); END`);
  try {
    assert.throws(() => store.createBench(id, { title: 'Half', blind: false, content: 'A hut', members }), /refused/);
  } finally {
    db.exec('DROP TRIGGER refuse_second');
    db.close();
  }
  assert.equal(store.getBench(id), null);
  assert.equal(store.listBenches().length, before);
  assert.deepEqual(store.benchMembers(id), []);
  const { bench, members: chats } = store.createBench(id, { title: 'Whole', blind: false, content: 'A hut', members });
  assert.deepEqual(bench.rounds.map((round) => round.content), ['A hut']);
  assert.deepEqual(store.benchMembers(id).map((member) => member.id), chats.map((chat) => chat.id));
});

test('startup ends every turn the server stopped in the middle of, with a result for each unanswered call', () => {
  const chat = (...messages) => {
    const conversation = store.createConversation('openrouter', 'test/model');
    for (const message of messages) store.appendMessage(conversation, message, message.role === 'user' ? conversation.world.reference() : null);
    store.save(conversation);
    return conversation.id;
  };
  const prompt = { role: 'user', content: 'Build a hut' };
  const calls = { role: 'assistant', content: '', tool_calls: [
    { id: 'done', function: { name: 'build', arguments: '{}' } },
    { id: 'lost', function: { name: 'bash', arguments: '{}' } },
  ] };
  const ids = {
    prompt: chat(prompt),
    calls: chat(prompt, calls),
    partial: chat(prompt, calls, { role: 'tool', tool_call_id: 'done', content: '{"ok":true}' }),
    shots: chat(prompt, calls, { role: 'tool', tool_call_id: 'done', content: '{"ok":true}' }, { role: 'tool', tool_call_id: 'lost', content: '{"ok":true}' },
      { role: 'user', seeBuild: true, content: [{ type: 'text', text: 'shots' }] }),
    final: chat(prompt, { role: 'assistant', content: 'Built' }),
    stopped: chat(prompt, { role: 'assistant', content: '(stopped by user)', stopped: true }),
    failed: chat(prompt, { role: 'error', content: 'Provider failed' }),
  };
  reopenStore();
  const ended = store.interruptUnfinished();
  // Earlier tests in this file leave turns under way too.
  assert.deepEqual(Object.entries(ids).filter(([, id]) => ended.includes(id)).map(([name]) => name), ['prompt', 'calls', 'partial', 'shots']);
  reopenStore();
  const tail = (id) => store.getConversation(id).messages.slice(1).map((message) => (message.role === 'tool' ? `tool ${message.tool_call_id}${JSON.parse(message.content).interrupted ? ' interrupted' : ''}` : message.role));
  assert.deepEqual(tail(ids.prompt), ['error']);
  assert.deepEqual(tail(ids.calls), ['assistant', 'tool done interrupted', 'tool lost interrupted', 'error']);
  assert.deepEqual(tail(ids.partial), ['assistant', 'tool done', 'tool lost interrupted', 'error']);
  assert.deepEqual(tail(ids.shots), ['assistant', 'tool done', 'tool lost', 'user', 'error']);
  assert.equal(store.getConversation(ids.prompt).messages.at(-1).content, store.INTERRUPTED);
  assert.deepEqual(tail(ids.final), ['assistant']);
  assert.deepEqual(store.interruptUnfinished(), [], 'every stored head now ends a turn');
});

test('a chat that cannot be loaded at startup is logged, and the other chats\' turns are still ended', (t) => {
  const unfinished = () => {
    const conversation = store.createConversation('openrouter', 'test/model');
    store.appendMessage(conversation, { role: 'user', content: 'Build a hut' }, conversation.world.reference());
    store.save(conversation);
    return conversation.id;
  };
  const [broken, fine] = [unfinished(), unfinished()];
  const db = new Database(store.DATABASE_FILE);
  // Its stored head is cut short.
  const head = db.prepare('SELECT m.id, m.payload_json FROM conversations c JOIN messages m ON m.id = c.current_message_id WHERE c.id = ?').get(broken);
  db.prepare('UPDATE messages SET payload_json = ? WHERE id = ?').run(head.payload_json.slice(0, 10), head.id);
  reopenStore();
  const logged = t.mock.method(console, 'error', () => {});
  try {
    const ended = store.interruptUnfinished();
    assert.ok(ended.includes(fine), 'the loadable chat is ended');
    assert.equal(ended.includes(broken), false);
    assert.ok(logged.mock.calls.some((entry) => String(entry.arguments[0]).includes(broken)), 'the failure names its chat');
  } finally {
    db.prepare('UPDATE messages SET payload_json = ? WHERE id = ?').run(head.payload_json, head.id);
    db.close();
  }
  assert.equal(store.getConversation(fine).messages.at(-1).content, store.INTERRUPTED);
});

test('a turn still under way is ended as failed with the reason given, and a head that ends a turn is left alone', () => {
  const conversation = store.createConversation('openrouter', 'test/model');
  assert.equal(store.endUnfinishedTurn(conversation, store.UNSAVED), false, 'a chat with no turn');
  store.appendMessage(conversation, { role: 'user', content: 'Build a hut' }, conversation.world.reference());
  store.appendMessage(conversation, { role: 'assistant', content: '', tool_calls: [{ id: 'lost', function: { name: 'bash', arguments: '{}' } }] });
  store.save(conversation);
  assert.equal(store.endUnfinishedTurn(conversation, store.UNSAVED), true);
  reopenStore();
  const stored = store.getConversation(conversation.id);
  assert.deepEqual(stored.messages.map((message) => message.role), ['user', 'assistant', 'tool', 'error']);
  assert.deepEqual([stored.messages[2].tool_call_id, stored.messages[3].content], ['lost', store.UNSAVED]);
  assert.equal(store.endUnfinishedTurn(stored, store.UNSAVED), false);
  assert.equal(stored.messages.length, 4);
});

// One model's chat cannot be saved while `work` runs.
function failingSave(id, work) {
  const db = new Database(store.DATABASE_FILE);
  db.exec(`CREATE TRIGGER refuse_chat BEFORE UPDATE ON conversations WHEN NEW.id = '${id}' BEGIN SELECT RAISE(ABORT, 'the disk is full'); END`);
  try {
    return work();
  } finally {
    db.exec('DROP TRIGGER refuse_chat');
    db.close();
  }
}

test('a bench round edit, branch switch or retry is stored whole or not at all', () => {
  const { bench, members: [first, second] } = newBench('Whole', 'A tower', ['a/model', 'b/model'], { blind: true });
  const turn = (member, round) => {
    store.appendMessage(member, { role: 'user', content: round.content, round: round.id }, member.world.reference());
    store.appendMessage(member, { role: 'assistant', content: 'Built' });
    store.save(member);
  };
  for (const member of [first, second]) turn(member, bench.rounds[0]);
  store.appendRound(bench, 'Add a roof');
  const roof = bench.rounds[1];
  for (const member of [first, second]) turn(member, roof);
  store.castVote(bench, roof.id, first.id, [first.id, second.id]);
  // What is stored and what is served, which a failed change must leave alike.
  const lines = () => {
    const db = sql();
    try {
      const stored = db.prepare('SELECT current_message_id FROM conversations WHERE bench_id = ? ORDER BY bench_slot').pluck().all(bench.id);
      const served = store.benchMembers(bench.id).map((member) => member.currentMessageId);
      assert.deepEqual(served, stored);
      return [store.getBench(bench.id).head, ...stored];
    } finally {
      db.close();
    }
  };
  const before = lines();

  // The first model's chat is saved, the second's is not.
  assert.throws(() => failingSave(second.id, () => store.revertBenchRound(bench, 1)), /the disk is full/);
  assert.deepEqual(lines(), before, 'no model goes back without the bench');
  assert.equal(bench.rounds.length, 2);

  assert.throws(() => failingSave(second.id, () => store.retryBenchRound(bench, store.getConversation(second.id))), /the disk is full/);
  assert.deepEqual(store.getBench(bench.id).votes[roof.id], { pick: first.id, blind: true }, 'a retry that failed takes no vote back');
  assert.deepEqual(bench.votes, store.getBench(bench.id).votes);
  assert.deepEqual(lines(), before);

  store.revertBenchRound(bench, 1);
  store.appendRound(bench, 'Add a spire');
  for (const member of store.benchMembers(bench.id)) turn(member, bench.rounds[1]);
  const onSpire = lines();
  assert.throws(() => failingSave(second.id, () => store.followBenchBranch(bench, roof.id)), /the disk is full/);
  assert.deepEqual(lines(), onSpire, 'the bench does not change line without its models');
  assert.equal(bench.head, onSpire[0]);
  store.followBenchBranch(bench, roof.id);
  assert.equal(lines()[0], roof.id);
});

test('attachments are written whole or not at all, and those of a message that was not saved are removed', (t) => {
  const storage = require('../server/storage');
  const id = crypto.randomUUID();
  const file = (name) => ({ name, data: `data:application/octet-stream;base64,${Buffer.alloc(64).toString('base64')}` });
  const used = storage.used(id);
  // The disk fails at the second file.
  const { writeFileSync } = fs;
  let writes = 0;
  t.mock.method(fs, 'writeFileSync', (...args) => {
    if ((writes += 1) === 2) throw new Error('the disk is full');
    return writeFileSync(...args);
  });
  assert.throws(() => store.saveAttachments(id, [file('a.bin'), file('b.bin')]), /the disk is full/);
  t.mock.restoreAll();
  assert.equal(fs.existsSync(path.join(store.ATTACHMENTS_DIR, id)), false, 'the first file does not stay');
  assert.equal(storage.used(id), used, 'and its bytes are given back');

  const kept = store.saveAttachments(id, [file('kept.bin')]);
  const saved = store.saveAttachments(id, [file('a.bin'), file('b.bin')]);
  store.discardAttachments(id, saved);
  assert.deepEqual(fs.readdirSync(path.join(store.ATTACHMENTS_DIR, id)), [path.basename(kept[0].url)]);
  assert.equal(storage.used(id), used + 64);
});

test('a fork that cannot be saved leaves none of its copied files', () => {
  const { members: [source] } = newBench('Forked tower', 'Build a tower', ['test/model']);
  const attachment = store.saveAttachment(source.id, { name: 'ref.png', data: `data:image/png;base64,${Buffer.from('png').toString('base64')}` });
  store.appendMessage(source, { role: 'user', content: [{ type: 'text', text: 'Build a tower' }, { type: 'attachment', attachment }] }, source.world.reference());
  source.world.set(0, 0, 0, 'minecraft:stone');
  store.appendMessage(source, { role: 'assistant', content: 'Built' });
  store.save(source);
  const folders = () => [store.ATTACHMENTS_DIR, path.join(dataDir, 'worlds')].map((dir) => fs.readdirSync(dir).sort());
  const before = folders();
  const chats = store.listConversations().length;
  const db = new Database(store.DATABASE_FILE);
  db.exec(`CREATE TRIGGER refuse_fork BEFORE INSERT ON conversations WHEN NEW.title = 'Forked tower' AND NEW.bench_id IS NULL BEGIN SELECT RAISE(ABORT, 'the disk is full'); END`);
  try {
    assert.throws(() => store.forkConversation(source, source.messages.length, source.world), /the disk is full/);
  } finally {
    db.exec('DROP TRIGGER refuse_fork');
    db.close();
  }
  assert.deepEqual(folders(), before);
  assert.equal(store.listConversations().length, chats);
});

// Last: the retention period is run past for every chat in the file.
test('an empty chat is not found by search, and is removed after the retention period unless this run used it', () => {
  const empty = store.createConversation('openrouter', 'test/model');
  const held = store.createConversation('openrouter', 'test/model');
  const started = store.createConversation('openrouter', 'test/model');
  store.appendMessage(started, { role: 'user', content: 'Build a well' }, started.world.reference());
  store.save(started);
  const found = store.search('build', 'New build').map((result) => result.id);
  assert.ok(found.includes(started.id), 'a chat with a message is found by its title');
  assert.equal(found.includes(empty.id) || found.includes(held.id), false, 'an empty chat is not');
  // A refused first message can leave an empty chat its folder.
  const attachments = path.join(store.ATTACHMENTS_DIR, empty.id);
  fs.mkdirSync(attachments, { recursive: true });

  // The server starts again: nothing is loaded. An open page asks for the chat it holds.
  reopenStore();
  assert.ok(store.getConversation(held.id));
  const rows = () => {
    const db = sql();
    try {
      return db.prepare('SELECT id FROM conversations WHERE id IN (?, ?, ?)').pluck().all(empty.id, held.id, started.id).sort();
    } finally {
      db.close();
    }
  };
  store.clearExpiredArchive(Date.now() + store.ARCHIVE_TTL_MS - 60_000);
  assert.deepEqual(rows(), [empty.id, held.id, started.id].sort(), 'an empty chat is kept for the retention period');
  store.clearExpiredArchive(Date.now() + store.ARCHIVE_TTL_MS + 60_000);
  assert.deepEqual(rows(), [held.id, started.id].sort(), 'then removed, unless this run made it or was asked for it');
  assert.equal(store.getConversation(empty.id), null);
  assert.equal(fs.existsSync(attachments), false);
  const db = sql();
  try {
    assert.equal(db.prepare('SELECT COUNT(*) FROM search_docs WHERE owner = ?').pluck().get(empty.id), 0);
  } finally {
    db.close();
  }
  // The page's first message still has its chat.
  store.appendMessage(store.getConversation(held.id), { role: 'user', content: 'Build a hut' });
  store.save(store.getConversation(held.id));
  assert.ok(store.listConversations().some((row) => row.id === held.id));
});

test("a chat's resolution is stored with it, listed, and kept by its fork", () => {
  const conversation = store.createConversation('openrouter', 'test/model');
  assert.equal(conversation.resolution, 1, 'a chat starts at Standard');
  conversation.resolution = 5;
  store.appendMessage(conversation, { role: 'user', content: 'A lighthouse' }, conversation.world.reference());
  store.appendMessage(conversation, { role: 'assistant', content: 'Built.' });
  store.save(conversation);
  reopenStore();
  const stored = store.getConversation(conversation.id);
  assert.equal(stored.resolution, 5);
  assert.equal(store.listConversations().find((row) => row.id === conversation.id).resolution, 5);
  const fork = store.forkConversation(stored, stored.messages.length, stored.world);
  assert.equal(store.getConversation(fork.id).resolution, 5);
  stored.resolution = 1;
  store.save(stored);
  reopenStore();
  assert.equal(store.getConversation(conversation.id).resolution, 1, 'the resolution can be changed again');
});

test("a bench's resolution is every model's chat's, and is listed with it", () => {
  const members = [{ provider: 'openrouter', model: 'test/one', effort: null }, { provider: 'openrouter', model: 'test/two', effort: null }];
  const fine = store.createBench(crypto.randomUUID(), { title: 'Fine bench', blind: false, resolution: 10, content: 'A cathedral', members });
  const plain = store.createBench(crypto.randomUUID(), { title: 'Plain bench', blind: false, resolution: 1, content: 'A shed', members });
  assert.deepEqual(fine.members.map((chat) => chat.resolution), [10, 10]);
  assert.deepEqual(plain.members.map((chat) => chat.resolution), [1, 1]);
  const listed = store.listBenches();
  assert.equal(listed.find((row) => row.id === fine.bench.id).resolution, 10);
  assert.equal(listed.find((row) => row.id === plain.bench.id).resolution, 1);
});

test('a version 10 database gains a resolution, with every chat at Standard', () => {
  const conversation = store.createConversation('openrouter', 'test/model');
  store.appendMessage(conversation, { role: 'user', content: 'From before extra mode' }, conversation.world.reference());
  store.save(conversation);
  store.close();
  const db = new Database(path.join(dataDir, 'mcbench.sqlite3'));
  db.exec('ALTER TABLE conversations DROP COLUMN resolution');
  db.pragma('user_version = 10');
  db.close();
  delete require.cache[require.resolve('../server/store')];
  store = require('../server/store');
  const check = sql();
  try {
    assert.equal(check.pragma('user_version', { simple: true }), 13);
  } finally {
    check.close();
  }
  assert.equal(store.getConversation(conversation.id).resolution, 1);
});

test('a version 11 database turns extra mode into the highest resolution and drops it', () => {
  const plain = store.createConversation('openrouter', 'test/model');
  const extra = store.createConversation('openrouter', 'test/model');
  for (const conversation of [plain, extra]) {
    store.appendMessage(conversation, { role: 'user', content: 'From extra mode' }, conversation.world.reference());
    store.save(conversation);
  }
  store.close();
  const db = new Database(path.join(dataDir, 'mcbench.sqlite3'));
  db.exec(`
    ALTER TABLE conversations DROP COLUMN resolution;
    ALTER TABLE conversations ADD COLUMN extra INTEGER NOT NULL DEFAULT 0;
  `);
  db.prepare('UPDATE conversations SET extra = 1 WHERE id = ?').run(extra.id);
  db.pragma('user_version = 11');
  db.close();
  delete require.cache[require.resolve('../server/store')];
  store = require('../server/store');
  const check = sql();
  try {
    assert.equal(check.pragma('user_version', { simple: true }), 13);
    assert.ok(!check.prepare('PRAGMA table_info(conversations)').all().some((column) => column.name === 'extra'));
  } finally {
    check.close();
  }
  assert.equal(store.getConversation(plain.id).resolution, 1);
  assert.equal(store.getConversation(extra.id).resolution, 10);
});

test("a version 12 database drops Claude Code's context readings and keeps every other provider's", () => {
  const conversation = store.createConversation('claude-code', 'claude-opus-5-5');
  store.appendMessage(conversation, { role: 'user', content: 'Build a hut.' }, conversation.world.reference());
  const telemetry = (provider, extra) => ({ provider, model: 'm', usage: { inputTokens: 21798, outputTokens: 108 }, ...extra });
  const summed = store.appendMessage(conversation, { role: 'assistant', content: 'Built.', telemetry: telemetry('claude-code', { contextTokens: 21798 }) });
  const unread = store.appendMessage(conversation, { role: 'assistant', content: 'Taller.', telemetry: telemetry('claude-code') });
  const anthropic = store.appendMessage(conversation, { role: 'assistant', content: 'Roofed.', telemetry: telemetry('anthropic', { contextTokens: 7383 }) });
  store.save(conversation);
  store.close();
  const db = new Database(path.join(dataDir, 'mcbench.sqlite3'));
  const payloads = () => Object.fromEntries(db.prepare('SELECT id, payload_json FROM messages WHERE conversation_id = ?').all(conversation.id).map((row) => [row.id, JSON.parse(row.payload_json)]));
  const before = payloads();
  db.pragma('user_version = 12');
  db.close();
  delete require.cache[require.resolve('../server/store')];
  store = require('../server/store');
  const check = sql();
  try {
    assert.equal(check.pragma('user_version', { simple: true }), 13);
    const after = Object.fromEntries(check.prepare('SELECT id, payload_json FROM messages WHERE conversation_id = ?').all(conversation.id).map((row) => [row.id, JSON.parse(row.payload_json)]));
    assert.equal(before[summed.id].telemetry.contextTokens, 21798);
    const { contextTokens, ...rest } = before[summed.id].telemetry;
    assert.deepEqual(after[summed.id], { ...before[summed.id], telemetry: rest });
    assert.deepEqual(after[unread.id], before[unread.id]);
    assert.deepEqual(after[anthropic.id], before[anthropic.id]);
    assert.equal(after[anthropic.id].telemetry.contextTokens, 7383);
  } finally {
    check.close();
  }
});
