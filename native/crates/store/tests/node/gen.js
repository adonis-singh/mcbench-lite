'use strict';

// Writes the differential's inputs for a clone of a data folder:
// queries.json (search queries drawn from the data, and edge cases),
// urls.json (every image and attachment URL the messages name, and bad
// ones) and ops.json (the write script both stores run). Reads the clone's
// database read-only; nothing it writes holds chat text except the search
// words, which stay in the scratch folder.
//
//   MCBENCH_NODE_DIFF=<checkout> node gen.js <clone> <inputs folder>

const fs = require('node:fs');
const path = require('node:path');

const W = process.env.MCBENCH_NODE_DIFF;
const Database = require(path.join(W, 'node_modules', 'better-sqlite3'));
const [clone, inputs] = process.argv.slice(2);
fs.mkdirSync(inputs, { recursive: true });
const db = new Database(path.join(clone, 'mcbench.sqlite3'), { readonly: true });

// A fixed-seed generator, so the inputs are the same every run.
let seed = 20261117;
const random = () => {
  seed = (seed * 1103515245 + 12345) % 2147483648;
  return seed / 2147483648;
};
const pick = (list) => list[Math.floor(random() * list.length)];

// ---- queries --------------------------------------------------------------

const texts = [];
for (const { title } of db.prepare('SELECT title FROM conversations UNION ALL SELECT title FROM benches').all()) texts.push(title);
for (const { body } of db.prepare('SELECT body FROM search_docs ORDER BY id').all()) texts.push(body);
const counts = new Map();
for (const text of texts) for (const word of text.match(/[\p{L}\p{N}_]+/gu) || []) counts.set(word, (counts.get(word) || 0) + 1);
const byCount = [...counts].sort((a, b) => b[1] - a[1] || (a[0] < b[0] ? -1 : 1)).map(([word]) => word);
const queries = [];
queries.push(...byCount.slice(0, 70));
const rare = byCount.filter((word) => counts.get(word) <= 2);
for (let i = 0; i < 50; i += 1) queries.push(pick(rare));
for (let i = 0; i < 30; i += 1) {
  const word = pick(byCount.slice(0, 400));
  queries.push(word.slice(0, Math.max(1, Math.ceil(word.length / 2))));
}
for (let i = 0; i < 30; i += 1) {
  const text = pick(texts);
  const words = text.match(/[\p{L}\p{N}_]+/gu) || ['x'];
  const at = Math.floor(random() * words.length);
  queries.push(words.slice(at, at + 1 + Math.floor(random() * 4)).join(' '));
}
for (let i = 0; i < 10; i += 1) queries.push(`${pick(byCount.slice(0, 200)).toUpperCase()} ${pick(rare)}`);
queries.push(
  '', '   ', '"', '"*(', '*', '()', 'AND', 'OR', 'NOT', 'NEAR', 'a OR b', 'build AND tower', 'NOT tower', 'NEAR(build tower)',
  '"build a"', "'", '\\', 'col:umn', '-x', '^build', 'tower*', 'a_b', '_', '0', '12345', 'é', 'Ünïcödé', '日本語', 'Ⅻ', '٣',
  '🚀 rocket', 'naïve café', 'STAIRCASE', 'build build build', 'build '.repeat(4000), 'a b c d e f g h i j k l m n o p',
  'a b c d e f g h i j k l m n o p q', 'the', 'minecraft:stone', '/api/images', 'x'.repeat(300), '\u0000', 'line\nbreak',
);
fs.writeFileSync(path.join(inputs, 'queries.json'), JSON.stringify(queries));

// ---- urls -----------------------------------------------------------------

const urls = new Set();
for (const { payload_json: text } of db.prepare('SELECT payload_json FROM messages ORDER BY conversation_id, position').all()) {
  for (const match of text.matchAll(/\/api\/(?:images|attachments)\/[^"\s)\\]+/g)) urls.add(match[0]);
}
const sorted = [...urls].sort();
sorted.push(
  '/api/images/missing/none.png', '/api/images/../../etc/passwd', '/api/images/../x', '/api/images/a/..', '/api/images//b',
  '/api/attachments/missing/none.txt', '/api/attachments/../x/y', '/api/attachments/a/..', '/api/attachments/a/../../b',
  '/api/attachments//x',
);
fs.writeFileSync(path.join(inputs, 'urls.json'), JSON.stringify(sorted));

// ---- ops ------------------------------------------------------------------

const ops = [];
const chats = db.prepare(`
  SELECT c.id, (SELECT count(*) FROM messages m WHERE m.conversation_id = c.id) AS n FROM conversations c
  WHERE c.archived_at IS NULL AND c.bench_id IS NULL ORDER BY n DESC, c.id
`).all();
const withMessages = chats.filter((chat) => chat.n > 0);
const benches = db.prepare('SELECT id FROM benches WHERE archived_at IS NULL ORDER BY created_at').all().map((row) => row.id);
const archived = db.prepare('SELECT id FROM conversations WHERE archived_at IS NOT NULL AND bench_id IS NULL ORDER BY id').all().map((row) => row.id);
const errors = db.prepare(`
  SELECT m.id, m.conversation_id AS chat FROM messages m JOIN conversations c ON c.id = m.conversation_id
  WHERE json_extract(m.payload_json, '$.role') = 'error' AND c.archived_at IS NULL AND c.bench_id IS NULL ORDER BY m.id
`).all();
const userPrompts = (chat) => db.prepare(`
  SELECT id FROM messages WHERE conversation_id = ? AND json_extract(payload_json, '$.role') = 'user'
    AND COALESCE(json_extract(payload_json, '$.seeBuild'), 0) = 0 AND checkpoint_manifest_json IS NOT NULL ORDER BY position
`).all(chat).map((row) => row.id);
const telemetry = (provider, extra = {}) => ({ provider, model: 'test/model', effort: null, startedAt: '2026-11-17T00:00:00.000Z', wallMs: 1234, firstDeltaMs: 456, contextBytes: 7890, messageCount: 3, usage: { inputTokens: 100, outputTokens: 20, cachedInputTokens: 0, cacheWriteTokens: 0, reasoningTokens: null }, ...extra });
const call = (id, name) => ({ id, type: 'function', function: { name, arguments: '{"program":"x"}' } });
const now = (ms) => ops.push({ op: 'now', ms });
const T0 = 1794873600000;
let clock = T0;
const tick = () => now((clock += 1000));
let nameCount = 0;
const name = (prefix) => `$${prefix}${(nameCount += 1)}`;
const fill = (fileName, byte, count, mime = 'application/octet-stream') => ({ name: fileName, fill: { byte, count, mime } });

// Settings and prompts first, so their errors are on a known file.
ops.push({ op: 'favorites', value: ['codex::gpt-5.6-sol', 'codex::gpt-5.6-sol', 'openrouter::openai/x', 'unknown::m', 'openai::', ' codex::  y ', null, 'constructor::m'] });
ops.push({ op: 'favorites', value: 'not an array' });
ops.push({ op: 'settingsUpdate', next: { providers: { openai: { apiKey: '  sk-test-new  ', enabled: false, hiddenModels: ['a', 'a', '', 3], modelOrder: ['b'] }, google: { apiKey: '' } }, titleModel: { provider: 'google', model: ' gemini ', effort: ' low ' }, shareRepo: ' owner/builds.site ' } });
ops.push({ op: 'settingsUpdate', next: { providers: {}, titleModel: { provider: 'nowhere', model: 'm' }, shareRepo: null } });
ops.push({ op: 'settingsUpdate', next: { providers: {}, titleModel: { provider: 'google', model: ' ' }, shareRepo: null } });
ops.push({ op: 'settingsUpdate', next: { providers: {}, titleModel: { provider: 'toString', model: 'm' }, shareRepo: null } });
ops.push({ op: 'settingsUpdate', next: { providers: {}, titleModel: null, shareRepo: 'not a repo' } });
ops.push({ op: 'settingsUpdate', next: { providers: {}, titleModel: null } });
ops.push({ op: 'settingsUpdate', next: { providers: { anthropic: { enabled: false } }, titleModel: null, shareRepo: null } });
ops.push({ op: 'promptsApply', args: {}, by: { chat: 'chat-1', model: 'test/model' } });
ops.push({ op: 'promptsApply', args: { save: [{ title: 'Moat', text: 'Dig a {{shape}} moat', tags: ['Medieval', ' medieval ', ''] }] }, by: { chat: 'chat-1', model: 'test/model' } });
ops.push({ op: 'promptsApply', args: { save: [{ title: 'Empty' }] }, by: { chat: 'chat-1', model: 'test/model' } });
ops.push({ op: 'promptsApply', args: { remove: 'b' }, by: { chat: 'chat-1', model: 'test/model' } });
ops.push({ op: 'promptsApply', args: { order: ['x'] }, by: { chat: 'chat-1', model: 'test/model' } });
ops.push({ op: 'promptsApply', args: { save: [{ id: 'medieval-house', text: 'x' }] }, by: { chat: 'chat-1', model: 'test/model' } });
ops.push({ op: 'promptsApply', args: { remove: ['missing'] }, by: { chat: 'chat-1', model: 'test/model' } });
ops.push({ op: 'promptsApply', args: { save: [{ text: 'Build a well', favorite: true }] }, by: { chat: 'chat-1', model: 'test/model' } });
ops.push({ op: 'promptsApply', args: { save: [{ id: '00000001-0000-4000-8000-000000000001', text: 'Dig a deep moat', tags: ['Water'] }] }, by: { chat: 'chat-1', model: 'test/model' } });
ops.push({ op: 'promptsApply', args: { remove: ['00000001-0000-4000-8000-000000000001'] }, by: { chat: 'chat-2', model: 'test/model' } });
ops.push({ op: 'promptsSave', prompts: [{ id: 'ref', title: ' Ref ', text: 'x', tags: ['A', 'a', 1], favorite: true, usedAt: '17', attachments: [{ name: 'ref.png', type: 'text/html', size: 3, data: 'data:image/png;base64,cG5n', url: 'javascript:alert(1)' }], addedBy: { chat: 'c', model: 'm', extra: 1 } }, { title: 'No id', text: 'y' }] });
ops.push({ op: 'promptsSave', prompts: [{ id: 'bad', attachments: [{ name: 'a.png', data: 'https://example.com/a.png' }] }] });
ops.push({ op: 'promptsList' });

// Existing chats: load, reply, tool results, screenshots, builds, branches.
for (const [index, chat] of withMessages.slice(0, 8).entries()) {
  tick();
  ops.push({ op: 'get', chat: chat.id });
  ops.push({ op: 'activateAll', chat: chat.id });
  const prompt = name('m');
  ops.push({ op: 'prompt', chat: chat.id, content: `Follow-up ${index}: add a tower`, prompts: index % 2 ? [{ id: 'medieval-house', title: 'Medieval house' }] : [], as: prompt });
  // Saved at once, as index.js 473 does, before the turn builds.
  ops.push({ op: 'save', chat: chat.id });
  ops.push({ op: 'build', chat: chat.id, blocks: [[index, 0, 0, 'minecraft:stone', {}], [index, 1, 0, 'minecraft:oak_stairs', { facing: 'north', half: 'top' }], [16 + index, 0, 0, 'minecraft:gold_block', {}]] });
  ops.push({ op: 'reply', chat: chat.id, reply: { content: '', reasoning: index % 3 ? 'thinking' : '', finish_reason: 'tool_calls', telemetry: telemetry('openrouter'), tool_calls: [call(`call-${index}-a`, 'build'), call(`call-${index}-b`, 'see_build')] } });
  ops.push({ op: 'toolResult', chat: chat.id, callId: `call-${index}-a`, telemetry: { name: 'build', startedAt: '2026-11-17T00:00:00.000Z', wallMs: 12, ok: true, cancelled: false }, content: JSON.stringify({ ok: true, worldRevision: 2, changed: true, report: '/work/verification/00000000-1111-2222-3333-444444444444.json' }) });
  ops.push({ op: 'images', chat: chat.id, images: [{ png: Buffer.from(`png-${index}`).toString('base64') }, { dataUrl: `data:image/jpeg;base64,${Buffer.from(`jpeg-${index}`).toString('base64')}` }], prefix: '', as: name('img') });
  ops.push({ op: 'save', chat: chat.id });
  if (index % 2) ops.push({ op: 'endTurn', chat: chat.id, final: { kind: 'stopped', content: '(stopped by user)', reply: index % 4 === 1 ? 'partial' : null, telemetry: telemetry('anthropic', { usage: null, failed: true, cancelled: true }) } });
  else {
    ops.push({ op: 'toolResult', chat: chat.id, callId: `call-${index}-b`, telemetry: { name: 'see_build', startedAt: '2026-11-17T00:00:00.000Z', wallMs: 40, ok: true, cancelled: false }, content: '{"ok":true}' });
    ops.push({ op: 'screenshots', chat: chat.id, text: 'Here are the shots.', urls: [`/api/images/${chat.id}/a.png`] });
    ops.push({ op: 'reply', chat: chat.id, reply: { content: `Built tower ${index}.`, finish_reason: 'stop', telemetry: telemetry('openrouter', { contextTokens: null }), notices: [{ level: 'warning', text: 'n' }], plan: [{ text: 't', status: 'done' }], rateLimit: { status: 'allowed', resetsAt: 1, utilization: 0.25 } } });
    ops.push({ op: 'save', chat: chat.id });
  }
  ops.push({ op: 'markFinished', chat: chat.id });
  if (index % 3 === 0) ops.push({ op: 'markSeen', chat: chat.id });
  ops.push({ op: 'setTitle', chat: chat.id, title: `Renamed ${index} — tower ✓` });
  // An edit of the prompt just sent: a branch.
  ops.push({ op: 'editPrompt', chat: chat.id, message: prompt, content: `Edited follow-up ${index}` });
  ops.push({ op: 'build', chat: chat.id, blocks: [[index, 5, 5, 'minecraft:cobblestone', {}]] });
  ops.push({ op: 'reply', chat: chat.id, reply: { content: 'Edited build done.', finish_reason: 'stop' } });
  ops.push({ op: 'save', chat: chat.id });
  ops.push({ op: 'activateAll', chat: chat.id });
  ops.push({ op: 'activate', chat: chat.id, message: prompt });
  const earlier = userPrompts(chat.id);
  if (earlier.length > 1) ops.push({ op: 'revert', chat: chat.id, message: earlier[earlier.length - 1] });
  ops.push({ op: 'revert', chat: chat.id, message: 'not-a-message' });
  ops.push({ op: 'activate', chat: chat.id, message: 'not-a-message' });
  ops.push({ op: 'fork', chat: chat.id, count: 2, world: 'head', as: name('fork') });
  ops.push({ op: 'endUnfinished', chat: chat.id, reason: 'How this turn ended was not saved' });
}

// Errors a user dismisses.
for (const error of errors.slice(0, 6)) ops.push({ op: 'dismiss', chat: error.chat, message: error.id });
ops.push({ op: 'dismiss', chat: withMessages[0].id, message: 'not-a-message' });

// New chats: attachments, budgets, branches, turn ends.
for (let i = 0; i < 6; i += 1) {
  tick();
  const chat = `$new${i}`;
  ops.push({ op: 'create', provider: 'openrouter', model: `test/model-${i}`, effort: i % 2 ? 'high' : null, resolution: i % 3 ? 1 : 5, as: chat });
  const files = name('files');
  ops.push({ op: 'attachments', chat, files: [fill('reference notes.md', 65, 100, 'text/markdown'), fill('photo.JPEG', 66, 200, 'image/jpeg'), fill('..odd name ü.tar.gz', 67, 50), fill('noext', 68, 10), fill('.hidden', 69, 10), fill('a\u0001b.txt', 70, 10, 'text/plain')], as: files });
  ops.push({ op: 'attachments', chat, files: [{ name: '', data: 'data:text/plain;base64,eA==' }] });
  ops.push({ op: 'attachments', chat, files: [fill('ok.bin', 1, 4), { name: 'b.bin', data: 'https://example.com/b' }] });
  ops.push({ op: 'attachments', chat, files: [{ name: 'empty.bin', data: 'data:application/octet-stream;base64,' }] });
  ops.push({ op: 'attachments', chat, files: [{ name: 'dir/evil.txt', data: 'data:text/plain;base64,ZXZpbA==' }] });
  const prompt = name('m');
  ops.push({ op: 'prompt', chat, content: [{ type: 'text', text: `New chat ${i}: a lighthouse` }, { type: 'attachment', attachment: `${files}[0]` }, { type: 'selection', selection: { letter: 'A', label: 'Stone, top', min: [0, 0, 0], max: [1, 1, 1], blocks: 8, block: 'minecraft:stone' } }], prompts: [], as: prompt });
  // Saved at once, as index.js 473 does, before the turn builds.
  ops.push({ op: 'save', chat: chat });
  ops.push({ op: 'repeats', chat, content: [{ type: 'text', text: `New chat ${i}: a lighthouse` }, { type: 'attachment', attachment: `${files}[0]` }, { type: 'selection', selection: { letter: 'A', label: 'Stone, top', min: [0, 0, 0], max: [1, 1, 1], blocks: 8, block: 'minecraft:stone' } }] });
  ops.push({ op: 'build', chat, blocks: [[0, 0, 0, 'minecraft:stone', {}], [1, 0, 0, 'minecraft:stone', {}], [0, 64, 0, 'minecraft:glass', {}]] });
  ops.push({ op: 'reply', chat, reply: { content: '', finish_reason: 'tool_calls', tool_calls: [call(`n${i}a`, 'build'), call(`n${i}b`, 'bash'), call(`n${i}c`, 'test_build')] } });
  ops.push({ op: 'toolResult', chat, callId: `n${i}a`, telemetry: { name: 'build', startedAt: '2026-11-17T00:00:00.000Z', wallMs: 1, ok: true, cancelled: false }, content: '{"ok":true}' });
  if (i % 2) ops.push({ op: 'unanswered', chat, callId: `n${i}b`, stopped: i % 4 === 1, reason: 'the engine stopped' });
  ops.push({ op: 'save', chat });
  ops.push({ op: 'endTurn', chat, final: { kind: 'error', content: `Provider failed ${i}`, telemetry: i % 2 ? telemetry('groq', { usage: null, failed: true, cancelled: false }) : null } });
  ops.push({ op: 'repeats', chat, content: `New chat ${i}: a lighthouse` });
  ops.push({ op: 'dismiss', chat, message: '$last' });
  ops.push({ op: 'dismiss', chat, message: '$last' });
  ops.push({ op: 'dismiss', chat, message: prompt });
  const edited = name('m');
  ops.push({ op: 'editPrompt', chat, message: prompt, content: `New chat ${i}: a taller lighthouse`, as: edited });
  ops.push({ op: 'reply', chat, reply: { content: 'Taller.', finish_reason: 'stop' } });
  ops.push({ op: 'save', chat });
  ops.push({ op: 'activate', chat, message: prompt });
  ops.push({ op: 'activateAll', chat });
  ops.push({ op: 'persistWorld', chat, blocks: [[100, 0, 100, 'minecraft:diamond_block', {}]] });
  ops.push({ op: 'reserve', chat, bytes: 1024 });
  ops.push({ op: 'refund', chat, bytes: 1024 });
  ops.push({ op: 'fork', chat, count: 1, world: `checkpoint:${edited}`, as: name('fork') });
}

// Budgets: a chat filled up to and past its budget (MCBENCH_CHAT_MAX_BYTES
// is 256 MiB for the write run).
{
  tick();
  const chat = '$budget';
  ops.push({ op: 'create', provider: 'openrouter', model: 'test/budget', as: chat });
  ops.push({ op: 'attachments', chat, files: [fill('a.bin', 1, 90 * 1024 * 1024), fill('b.bin', 2, 90 * 1024 * 1024)] });
  ops.push({ op: 'attachments', chat, files: [fill('c.bin', 3, 70 * 1024 * 1024)] });
  ops.push({ op: 'attachments', chat, files: [fill('x.bin', 3, 10 * 1024 * 1024)] });
  ops.push({ op: 'attachments', chat, files: [fill('d.bin', 4, 100 * 1024 * 1024 + 1)] });
  ops.push({ op: 'attachments', chat, files: [fill('e.bin', 5, 70 * 1024 * 1024), fill('f.bin', 6, 70 * 1024 * 1024), fill('g.bin', 7, 70 * 1024 * 1024)] });
  ops.push({ op: 'images', chat, images: [{ png: Buffer.alloc(15 * 1024 * 1024, 9).toString('base64') }], prefix: 'view-' });
  ops.push({ op: 'images', chat, images: [{ dataUrl: 'https://example.com/x.png' }], prefix: '' });
  ops.push({ op: 'reserve', chat, bytes: 1 });
  ops.push({ op: 'refusal', chat });
  ops.push({ op: 'refund', chat, bytes: 100 * 1024 * 1024 });
  ops.push({ op: 'refusal', chat });
  ops.push({ op: 'build', chat, blocks: Array.from({ length: 64 }, (_, i) => [i * 16, 0, 0, 'minecraft:stone', {}]) });
  ops.push({ op: 'prompt', chat, content: 'Budget prompt', prompts: [], as: name('m') });
  // Saved at once, as index.js 473 does, before the turn builds.
  ops.push({ op: 'save', chat: chat });
  ops.push({ op: 'save', chat });
  for (const big of withMessages.slice(0, 3)) {
    ops.push({ op: 'reserve', chat: big.id, bytes: 150 * 1024 * 1024 });
    ops.push({ op: 'refusal', chat: big.id });
  }
}

// Benches: new ones blind and not, rounds, votes, edits, branches, retries.
for (let b = 0; b < 4; b += 1) {
  tick();
  const bench = `$bench${b}`;
  const id = `bench-0000-${b}`;
  const blind = b % 2 === 0;
  ops.push({ op: 'attachments', chat: id, files: [fill('round.png', 80 + b, 1000, 'image/png')], as: `$roundFiles${b}` });
  ops.push({ op: 'createBench', id, title: `Bench ${b}`, blind, resolution: b === 3 ? undefined : b + 1, content: b === 1 ? [{ type: 'text', text: 'A castle' }, { type: 'attachment', attachment: `$roundFiles${b}[0]` }] : `A castle ${b}`, prompts: b % 2 ? [{ id: 'cliff-lighthouse', title: 'Cliff lighthouse' }] : [], members: [{ provider: 'openrouter', model: 'a/model', effort: null }, { provider: 'openrouter', model: 'b/model', effort: 'high' }, { provider: 'anthropic', model: 'a/model', effort: null }], as: bench });
  for (let m = 0; m < 3; m += 1) {
    ops.push({ op: 'benchPrompt', bench, member: m, index: 0, share: b === 1 });
    ops.push({ op: 'buildMember', bench, member: m, blocks: [[m, 0, b, 'minecraft:stone', {}]] });
    ops.push({ op: 'memberReply', bench, member: m, reply: m === 1 ? null : { content: `Model ${m} built it.`, finish_reason: 'stop' } });
  }
  ops.push({ op: 'appendRound', bench, content: 'Add a moat', prompts: [] });
  for (let m = 0; m < 3; m += 1) {
    ops.push({ op: 'benchPrompt', bench, member: m, index: 1, share: false });
    ops.push({ op: 'memberReply', bench, member: m, reply: { content: `Moat ${m}.`, finish_reason: 'stop' } });
  }
  ops.push({ op: 'benchProgress', bench });
  ops.push({ op: 'vote', bench, round: 0, pick: 0, judged: [0, 1, 2] });
  ops.push({ op: 'vote', bench, round: 1, pick: null, judged: [0, 1, 2] });
  ops.push({ op: 'vote', bench, round: 1, pick: 2, judged: [0, 2] });
  ops.push({ op: 'leaderboard' });
  ops.push({ op: 'editRound', bench, index: 1, content: 'Add a spire', links: [{ id: 'glass-cathedral', title: 'Glass cathedral' }] });
  for (let m = 0; m < 3; m += 1) {
    ops.push({ op: 'benchPrompt', bench, member: m, index: 1, share: false });
    ops.push({ op: 'memberReply', bench, member: m, reply: { content: `Spire ${m}.`, finish_reason: 'stop' } });
  }
  ops.push({ op: 'followBranch', bench, round: 1, which: 'previous' });
  ops.push({ op: 'followBranch', bench, round: 1, which: 'next' });
  ops.push({ op: 'retryRound', bench, member: 1 });
  ops.push({ op: 'clearVote', bench, round: 0 });
  if (b === 2) ops.push({ op: 'reveal', bench });
  ops.push({ op: 'vote', bench, round: 0, pick: 1, judged: [0, 1, 2] });
  ops.push({ op: 'benchTitle', bench, title: `Bench ${b}, renamed` });
  ops.push({ op: 'forkMember', bench, member: 0, as: name('fork') });
  ops.push({ op: 'leaderboard' });
}
ops.push({ op: 'createBench', id: 'bench-broken', title: 'Broken', blind: false, content: 'A hut', prompts: [], members: [{ provider: 'openrouter', model: 'a/model', effort: null }, { provider: '', model: 'b/model', effort: null }] });
for (const bench of benches.slice(0, 3)) {
  ops.push({ op: 'loadBench', id: bench, as: `$old-${bench}` });
  ops.push({ op: 'appendRound', bench: `$old-${bench}`, content: 'One more round', prompts: [] });
  ops.push({ op: 'followBranch', bench: `$old-${bench}`, round: 0, which: 'self' });
}
ops.push({ op: 'voteCount' });

// The archive: archive, restore, delete, and the purge.
tick();
ops.push({ op: 'archive', chat: '$new5' });
ops.push({ op: 'archive', chat: '$new5' });
ops.push({ op: 'restore', chat: '$new5' });
ops.push({ op: 'restore', chat: '$new5' });
ops.push({ op: 'delete', chat: '$new5' });
ops.push({ op: 'archive', chat: '$new5' });
ops.push({ op: 'delete', chat: '$new5' });
ops.push({ op: 'delete', chat: '$new5' });
ops.push({ op: 'archive', chat: withMessages[7].id });
ops.push({ op: 'save', chat: withMessages[7].id });
ops.push({ op: 'setTitle', chat: withMessages[7].id, title: 'Late title' });
ops.push({ op: 'restore', chat: '$bench0.member0' });
ops.push({ op: 'archiveBench', bench: '$bench3' });
ops.push({ op: 'restoreBench', bench: 'bench-0000-3' });
ops.push({ op: 'deleteBench', bench: 'bench-0000-3' });
ops.push({ op: 'archiveBench', bench: '$bench3' });
ops.push({ op: 'deleteBench', bench: 'bench-0000-3' });
ops.push({ op: 'archiveBench', bench: '$bench2' });
ops.push({ op: 'archiveBench', bench: benches[0] });
ops.push({ op: 'archive', chat: '$new4' });
ops.push({ op: 'interrupt' });
ops.push({ op: 'collect' });
for (const id of archived.slice(0, 2)) ops.push({ op: 'delete', chat: id });
ops.push({ op: 'nextExpiry', now: T0 });
// An empty chat this run did not load, and one it did.
ops.push({ op: 'purge', now: T0 + 3 * 24 * 3600 * 1000 });
ops.push({ op: 'now', ms: T0 + 8 * 24 * 3600 * 1000 });
ops.push({ op: 'purge', now: T0 + 8 * 24 * 3600 * 1000 });
ops.push({ op: 'nextExpiry', now: T0 + 8 * 24 * 3600 * 1000 });
ops.push({ op: 'clearAllVotes' });
ops.push({ op: 'voteCount' });

fs.writeFileSync(path.join(inputs, 'ops.json'), JSON.stringify(ops, null, 1));
console.log(`${queries.length} queries, ${sorted.length} urls, ${ops.length} ops`);
