'use strict';

// The read document of a data folder through today's Node store: every read
// the store exports, as callers get it. `tests/differential.rs` builds the
// same document through mcbench-store and compares the bytes.
//
//   MCBENCH_NODE_DIFF=<checkout> MCBENCH_DATA_DIR=<clone> \
//     node -r ./det.js read.js <inputs folder> <out.json>
//
// The inputs folder holds queries.json and urls.json (gen.js writes them).

const crypto = require('node:crypto');
const fs = require('node:fs');
const path = require('node:path');

const W = process.env.MCBENCH_NODE_DIFF;
const [inputs, out] = process.argv.slice(2);

function attempt(fn) {
  try {
    return { ok: fn() };
  } catch (error) {
    return { error: String(error.message), status: error.statusCode ?? null };
  }
}

const sha256 = (text) => crypto.createHash('sha256').update(text).digest('hex');

// JSON with every object's keys sorted, no spaces.
function canonical(value) {
  if (Array.isArray(value)) return `[${value.map(canonical).join(',')}]`;
  if (value && typeof value === 'object') return `{${Object.keys(value).sort().map((key) => `${JSON.stringify(key)}:${canonical(value[key])}`).join(',')}}`;
  return JSON.stringify(value);
}

// What the test keeps of a settings value (they hold keys): the sha256 of
// its canonical JSON, and per field path (`providers.openai.apiKey`,
// `[0]`), sorted, the sha256 of that field's canonical JSON. Never a value.
function settingsSummary(value) {
  const fields = [];
  const leaves = (field, at) => {
    if (Array.isArray(field) && field.length) field.forEach((item, i) => leaves(item, `${at}[${i}]`));
    else if (field && typeof field === 'object' && !Array.isArray(field) && Object.keys(field).length) for (const key of Object.keys(field)) leaves(field[key], at ? `${at}.${key}` : key);
    else fields.push([at, sha256(canonical(field))]);
  };
  leaves(value, '');
  fields.sort((a, b) => (a[0] < b[0] ? -1 : a[0] > b[0] ? 1 : 0));
  return { sha256: sha256(canonical(value)), fields: Object.fromEntries(fields) };
}

function readDocument() {
  const store = require(path.join(W, 'server', 'store'));
  const storage = require(path.join(W, 'server', 'storage'));
  const prompts = require(path.join(W, 'server', 'prompts'));
  const settings = require(path.join(W, 'server', 'settings'));
  const dataDir = storage.DATA_DIR;
  const queries = JSON.parse(fs.readFileSync(path.join(inputs, 'queries.json'), 'utf8'));
  const urls = JSON.parse(fs.readFileSync(path.join(inputs, 'urls.json'), 'utf8'));
  const sha = (bytes) => crypto.createHash('sha256').update(bytes).digest('hex');
  const manifest = (world) => JSON.stringify(world.serializeManifest(() => { throw new Error('a loaded world has every chunk stored'); }));
  const world = (w) => ({ manifest: manifest(w), size: w.size(), bounds: w.bounds() });

  const doc = {};
  doc.conversations = store.listConversations();
  doc.promptTitles = store.promptTitles();
  doc.conversationIds = store.listConversationIds();
  doc.benchIds = store.listBenchIds();
  doc.archive = store.listArchive();
  const times = [0, globalThis.__now];
  for (const row of [...doc.archive.conversations, ...doc.archive.benches]) times.push(row.expiresAt - 1, row.expiresAt, row.expiresAt + 1);
  doc.nextArchiveExpiry = times.map((now) => [now, store.nextArchiveExpiry(now)]);
  doc.benches = store.listBenches();
  doc.bench = {};
  doc.benchMembers = {};
  for (const { id } of doc.benchIds) {
    doc.bench[id] = store.getBench(id);
    doc.benchMembers[id] = store.benchMembers(id).map((member) => member.id);
  }
  doc.leaderboard = store.leaderboard();
  doc.voteCount = store.voteCount();
  doc.chats = {};
  for (const { id } of doc.conversationIds) {
    const chat = store.getConversation(id);
    if (!chat) {
      doc.chats[id] = null;
      continue;
    }
    const roots = chat.messages.filter((message) => message.role === 'user' && !message.seeBuild && !message.parentId);
    doc.chats[id] = {
      header: { id: chat.id, title: chat.title, provider: chat.provider, model: chat.model, effort: chat.effort || null, resolution: chat.resolution, benchId: chat.benchId, benchSlot: chat.benchSlot, createdAt: chat.createdAt },
      messages: chat.messages.map((message) => JSON.stringify(message)),
      publicMessages: JSON.stringify(store.publicMessages(chat)),
      messageTree: store.messageTree(chat),
      currentMessageId: chat.currentMessageId,
      nextMessagePosition: chat.nextMessagePosition,
      world: world(chat.world),
      checkpoints: [...chat.checkpoints].map(([key, w]) => [key, world(w)]).sort((a, b) => (a[0] < b[0] ? -1 : 1)),
      benchProgress: attempt(() => store.benchProgress(id)),
      reportReferences: [...store.reportReferences(id)],
      repeatsRootPrompt: roots.length ? attempt(() => store.repeatsLatestRootPrompt(chat, roots.at(-1).content)) : null,
    };
  }
  doc.used = {};
  doc.refusal = {};
  for (const { id } of doc.conversationIds) {
    doc.used[id] = attempt(() => storage.used(id));
    doc.refusal[id] = attempt(() => storage.refusal(id));
  }
  doc.search = queries.map((query) => [query, attempt(() => store.search('build', query)), attempt(() => store.search('bench', query))]);
  doc.files = urls.map((url) => {
    const kind = url.split('/')[2];
    const file = attempt(() => path.relative(dataDir, kind === 'images' ? store.imageFile(url) : store.attachmentFile(url)));
    // A file that cannot be read: Node's fs error here, the OS error in Rust.
    const read = attempt(() => sha(kind === 'images' ? store.readImage(url) : store.readAttachment(url)));
    const bytes = read.error === undefined ? read : { error: 'unreadable' };
    return [url, file, bytes];
  });
  doc.prompts = attempt(() => prompts.list());
  doc.settings = settingsSummary(settings.get());
  doc.limits = { chatMaxBytes: storage.CHAT_MAX_BYTES, diskReserveBytes: storage.DISK_RESERVE_BYTES, reportsKept: storage.REPORTS_KEPT };
  store.close();
  return doc;
}

module.exports = { readDocument, attempt, settingsSummary };

if (require.main === module) {
  fs.writeFileSync(out, JSON.stringify(readDocument()));
}
